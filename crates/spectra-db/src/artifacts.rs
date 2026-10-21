//! Content-addressed artifact storage (equity curves, trade lists, reports).
//!
//! Bytes live on a local volume under `ARTIFACT_DIR`, at
//! `sha256/<first 2 hex>/<next 2 hex>/<full hash>`: the same bytes are stored
//! once, a file's name is its checksum, and every read re-verifies it. Each
//! use of an artifact is a `run_artifacts` row, and a workspace's distinct
//! hashes count against its quota (`workspace_quotas.artifact_quota_bytes`, or a
//! default).
//!
//! [`ArtifactBackend`] picks where the bytes go: the local volume
//! ([`ArtifactStore`]), or, when built with the `s3` feature, an
//! S3-compatible bucket ([`s3::S3ArtifactStore`]: AWS S3, MinIO, R2). Both
//! use the same content-addressed paths and re-verify every read.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use spectra_core::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

/// The local, content-addressed store.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

/// Where `put` left the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// Lower-case hex SHA-256 of the bytes.
    pub sha256: String,
    /// Path relative to the store root.
    pub path: String,
    /// Byte count.
    pub size: u64,
}

/// Lower-case hex SHA-256 of `bytes` (the address [`ArtifactStore::put`] gives them).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.hex_digest()
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ArtifactStore {
    /// A store rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The relative path for a hash.
    pub fn relative_path(sha256: &str) -> String {
        format!("sha256/{}/{}/{sha256}", &sha256[..2], &sha256[2..4])
    }

    /// Stores `bytes` and returns their address. Writing the same bytes again
    /// is a no-op. The file appears atomically (written aside, then renamed).
    ///
    /// # Errors
    /// Any I/O error.
    pub fn put(&self, bytes: &[u8]) -> io::Result<Stored> {
        let sha256 = sha256_hex(bytes);
        let rel = Self::relative_path(&sha256);
        let path = self.root.join(&rel);
        if !path.exists() {
            let dir = path.parent().expect("artifact paths have a parent");
            fs::create_dir_all(dir)?;
            let tmp = dir.join(format!(".{sha256}.{}.tmp", std::process::id()));
            {
                let mut f = fs::File::create(&tmp)?;
                f.write_all(bytes)?;
                f.sync_all()?;
            }
            if let Err(e) = fs::rename(&tmp, &path) {
                let _ = fs::remove_file(&tmp);
                // Another writer stored the same bytes first: that is fine.
                if !path.exists() {
                    return Err(e);
                }
            }
        }
        Ok(Stored { sha256, path: rel, size: bytes.len() as u64 })
    }

    /// A directory inside the store for files on their way in, so that
    /// [`ArtifactStore::put_file`] can rename them into place.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// Moves `file`, whose SHA-256 the caller has computed as `sha256`, into
    /// the store without reading it into memory. The hash is checked again
    /// while copying when the file is not on the store's file system. If the
    /// store already holds these bytes, `file` is just removed.
    ///
    /// # Errors
    /// `InvalidInput` for a malformed hash, `InvalidData` if the copied bytes
    /// do not match it, or any I/O error.
    pub fn put_file(&self, file: &Path, sha256: &str) -> io::Result<Stored> {
        if !is_sha256_hex(sha256) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a lower-case SHA-256 hex string"));
        }
        let size = fs::metadata(file)?.len();
        let rel = Self::relative_path(sha256);
        let path = self.root.join(&rel);
        if path.exists() {
            let _ = fs::remove_file(file);
            return Ok(Stored { sha256: sha256.to_string(), path: rel, size });
        }
        let dir = path.parent().expect("artifact paths have a parent");
        fs::create_dir_all(dir)?;
        if fs::rename(file, &path).is_err() {
            // Another file system: copy (hashing as we go), then rename into place.
            let tmp = dir.join(format!(".{sha256}.{}.tmp", std::process::id()));
            let copied = (|| -> io::Result<String> {
                let mut src = fs::File::open(file)?;
                let mut dst = fs::File::create(&tmp)?;
                let mut h = Sha256::new();
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    let n = io::Read::read(&mut src, &mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                    dst.write_all(&buf[..n])?;
                }
                dst.sync_all()?;
                Ok(h.hex_digest())
            })();
            match copied {
                Ok(got) if got == sha256 => {}
                Ok(_) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "file does not match its SHA-256",
                    ));
                }
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(e);
                }
            }
            if let Err(e) = fs::rename(&tmp, &path) {
                let _ = fs::remove_file(&tmp);
                if !path.exists() {
                    return Err(e);
                }
            }
            let _ = fs::remove_file(file);
        }
        Ok(Stored { sha256: sha256.to_string(), path: rel, size })
    }

    /// Removes the bytes for `sha256`. Removing what is not there is not an error.
    ///
    /// # Errors
    /// `InvalidInput` for a malformed hash, or any other I/O error.
    pub fn delete(&self, sha256: &str) -> io::Result<()> {
        if !is_sha256_hex(sha256) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a lower-case SHA-256 hex string"));
        }
        match fs::remove_file(self.root.join(Self::relative_path(sha256))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// The bytes for `sha256`, re-verified against it.
    ///
    /// # Errors
    /// `InvalidInput` for a malformed hash, `NotFound` if absent, and
    /// `InvalidData` if the file no longer matches its hash.
    pub fn get(&self, sha256: &str) -> io::Result<Vec<u8>> {
        if !is_sha256_hex(sha256) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a lower-case SHA-256 hex string"));
        }
        let bytes = fs::read(self.root.join(Self::relative_path(sha256)))?;
        if sha256_hex(&bytes) != sha256 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("artifact {sha256} is corrupt")));
        }
        Ok(bytes)
    }
}

/// Where artifact bytes live.
#[derive(Debug, Clone)]
pub enum ArtifactBackend {
    /// A local directory (`ARTIFACT_DIR`).
    Local(ArtifactStore),
    /// An S3-compatible bucket.
    #[cfg(feature = "s3")]
    S3(s3::S3ArtifactStore),
}

impl From<ArtifactStore> for ArtifactBackend {
    fn from(store: ArtifactStore) -> Self {
        Self::Local(store)
    }
}

impl ArtifactBackend {
    /// Stores `bytes` under their SHA-256 and returns the address. Storing the
    /// same bytes again changes nothing.
    ///
    /// # Errors
    /// Any I/O or object-store error.
    pub async fn put(&self, bytes: Vec<u8>) -> io::Result<Stored> {
        match self {
            Self::Local(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || store.put(&bytes)).await.map_err(io::Error::other)?
            }
            #[cfg(feature = "s3")]
            Self::S3(store) => store.put(bytes).await,
        }
    }

    /// The bytes for `sha256`, re-verified against it.
    ///
    /// # Errors
    /// As [`ArtifactStore::get`]: `InvalidInput`, `NotFound` or `InvalidData`.
    pub async fn get(&self, sha256: &str) -> io::Result<Vec<u8>> {
        match self {
            Self::Local(store) => {
                let (store, sha256) = (store.clone(), sha256.to_string());
                tokio::task::spawn_blocking(move || store.get(&sha256)).await.map_err(io::Error::other)?
            }
            #[cfg(feature = "s3")]
            Self::S3(store) => store.get(sha256).await,
        }
    }

    /// Stores a file whose SHA-256 the caller computed while writing it.
    /// Locally the file is moved into place without being read into memory;
    /// for S3 it is read and uploaded. `file` is gone afterwards.
    ///
    /// # Errors
    /// As [`ArtifactStore::put_file`], or any object-store error.
    pub async fn put_file(&self, file: PathBuf, sha256: &str) -> io::Result<Stored> {
        match self {
            Self::Local(store) => {
                let (store, sha256) = (store.clone(), sha256.to_string());
                tokio::task::spawn_blocking(move || store.put_file(&file, &sha256))
                    .await
                    .map_err(io::Error::other)?
            }
            #[cfg(feature = "s3")]
            Self::S3(store) => {
                let bytes = tokio::fs::read(&file).await?;
                let stored = store.put(bytes).await?;
                let _ = tokio::fs::remove_file(&file).await;
                if stored.sha256 != sha256 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "file does not match its SHA-256",
                    ));
                }
                Ok(stored)
            }
        }
    }

    /// Removes the bytes for `sha256`; removing what is not there is fine.
    /// Callers check first that nothing references the hash.
    ///
    /// # Errors
    /// Any I/O or object-store error.
    pub async fn delete(&self, sha256: &str) -> io::Result<()> {
        match self {
            Self::Local(store) => {
                let (store, sha256) = (store.clone(), sha256.to_string());
                tokio::task::spawn_blocking(move || store.delete(&sha256)).await.map_err(io::Error::other)?
            }
            #[cfg(feature = "s3")]
            Self::S3(store) => store.delete(sha256).await,
        }
    }

    /// The bytes for `sha256` as a file on this machine, for reading as a
    /// stream: the stored file itself for a local backend, or a download
    /// into `staging` for S3 (fetched in 8 MiB ranges, never held whole).
    /// The caller verifies the content (for example, by hashing it while
    /// reading) and removes the file when [`LocalFile::temporary`] is set.
    ///
    /// # Errors
    /// `InvalidInput` for a malformed hash, `NotFound` if absent, or any
    /// I/O or object-store error.
    pub async fn local_file(&self, sha256: &str, staging: &Path) -> io::Result<LocalFile> {
        if !is_sha256_hex(sha256) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a lower-case SHA-256 hex string"));
        }
        #[cfg(not(feature = "s3"))]
        let _ = staging; // only S3 downloads into it
        match self {
            Self::Local(store) => {
                let path = store.root().join(ArtifactStore::relative_path(sha256));
                if tokio::fs::metadata(&path).await.is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("artifact {sha256} not found"),
                    ));
                }
                Ok(LocalFile { path, temporary: false })
            }
            #[cfg(feature = "s3")]
            Self::S3(store) => {
                let path = staging.join(format!("download-{sha256}-{}", std::process::id()));
                store.download(sha256, &path).await?;
                Ok(LocalFile { path, temporary: true })
            }
        }
    }

    /// A local directory to write incoming files to before [`Self::put_file`]:
    /// inside the store for a local backend (so the move is a rename), the
    /// system temporary directory otherwise.
    pub fn staging_dir(&self) -> PathBuf {
        match self {
            Self::Local(store) => store.staging_dir(),
            #[cfg(feature = "s3")]
            Self::S3(_) => std::env::temp_dir().join("spectra-staging"),
        }
    }

    /// Where the bytes go, for logs: `local:<dir>` or `s3://<bucket>/<prefix>`.
    pub fn describe(&self) -> String {
        match self {
            Self::Local(store) => format!("local:{}", store.root().display()),
            #[cfg(feature = "s3")]
            Self::S3(store) => format!("s3://{}/{}", store.bucket(), store.prefix()),
        }
    }
}

/// An artifact's bytes as a file on this machine; see [`ArtifactBackend::local_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    /// Where to read it.
    pub path: PathBuf,
    /// A downloaded copy the caller should remove when done.
    pub temporary: bool,
}

/// Why an artifact could not be recorded.
#[derive(Debug)]
pub enum ArtifactError {
    /// The workspace's quota would be exceeded.
    QuotaExceeded {
        /// Bytes allowed.
        quota: i64,
        /// Bytes already used.
        used: i64,
        /// Bytes this artifact needs.
        needed: i64,
    },
    /// The workspace does not exist.
    UnknownWorkspace,
    /// Reading or writing the bytes failed.
    Io(io::Error),
    /// The database failed.
    Db(sqlx::Error),
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactError::QuotaExceeded { quota, used, needed } => write!(
                f,
                "artifact storage quota exceeded: {needed} more bytes needed, {used} of {quota} used"
            ),
            ArtifactError::UnknownWorkspace => f.write_str("unknown workspace"),
            ArtifactError::Io(e) => write!(f, "artifact storage: {e}"),
            ArtifactError::Db(e) => write!(f, "artifact database: {e}"),
        }
    }
}

impl std::error::Error for ArtifactError {}

impl From<sqlx::Error> for ArtifactError {
    fn from(e: sqlx::Error) -> Self {
        ArtifactError::Db(e)
    }
}

/// What to attach to a run.
#[derive(Debug, Clone, Copy)]
pub struct NewArtifact<'a> {
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// The run it belongs to.
    pub run_id: Uuid,
    /// e.g. `report`, `equity_curve`, `trades`.
    pub kind: &'a str,
    /// MIME type.
    pub content_type: &'a str,
    /// The bytes.
    pub bytes: &'a [u8],
}

/// A workspace's storage: its quota, the bytes it uses, and whether it
/// already holds a given hash (then storing it again costs nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    /// Bytes allowed.
    pub quota: i64,
    /// Bytes used (each distinct hash once: run artifacts, uploads, dataset files).
    pub used: i64,
    /// The hash asked about is already stored for the workspace.
    pub present: bool,
}

/// The storage quota and usage of `workspace_id`, and whether it holds
/// `sha256`. `None` for an unknown workspace.
///
/// # Errors
/// Any database error.
pub async fn quota(
    pool: &PgPool,
    workspace_id: Uuid,
    sha256: &str,
    default_quota: i64,
) -> Result<Option<Quota>, sqlx::Error> {
    let row: Option<(i64, i64, bool)> = sqlx::query_as(include_str!("../queries/artifact_quota.sql"))
        .bind(workspace_id)
        .bind(sha256)
        .bind(default_quota)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(quota, used, present)| Quota { quota, used, present }))
}

/// Stores an artifact and records it against its run, refusing it when the
/// workspace's quota would be exceeded. The quota is checked before any
/// bytes are written, and again (under a lock on the workspace row) when the
/// row is inserted. `default_quota` applies to workspaces without their own.
///
/// # Errors
/// [`ArtifactError`].
pub async fn store_for_run(
    pool: &PgPool,
    store: &ArtifactBackend,
    a: NewArtifact<'_>,
    default_quota: i64,
) -> Result<Stored, ArtifactError> {
    let sha256 = sha256_hex(a.bytes);
    let needed = a.bytes.len() as i64;
    let row: Option<(i64, i64, bool)> = sqlx::query_as(include_str!("../queries/artifact_quota.sql"))
        .bind(a.workspace_id)
        .bind(&sha256)
        .bind(default_quota)
        .fetch_optional(pool)
        .await?;
    let Some((quota, used, present)) = row else { return Err(ArtifactError::UnknownWorkspace) };
    if !present && used + needed > quota {
        return Err(ArtifactError::QuotaExceeded { quota, used, needed });
    }

    let stored = store.put(a.bytes.to_vec()).await.map_err(ArtifactError::Io)?;

    let (inserted, present, quota, used): (i64, bool, Option<i64>, Option<i64>) =
        sqlx::query_as(include_str!("../queries/artifact_record.sql"))
            .bind(a.workspace_id)
            .bind(a.run_id)
            .bind(a.kind)
            .bind(&stored.sha256)
            .bind(&stored.path)
            .bind(needed)
            .bind(a.content_type)
            .bind(default_quota)
            .fetch_one(pool)
            .await?;
    if inserted == 0 && !present {
        // Lost a race for the last of the quota. The bytes stay (another
        // workspace may share them); nothing references them here.
        return Err(ArtifactError::QuotaExceeded {
            quota: quota.unwrap_or(0),
            used: used.unwrap_or(0),
            needed,
        });
    }
    Ok(stored)
}

/// A stored artifact of a run, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRef {
    /// Lower-case hex SHA-256 (the key in [`ArtifactStore`]).
    pub sha256: String,
    /// MIME type.
    pub content_type: String,
    /// Byte count.
    pub size: i64,
}

/// The newest artifact of `kind` for `run_id`, if that run belongs to `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn find_for_run(
    pool: &PgPool,
    workspace_id: Uuid,
    run_id: Uuid,
    kind: &str,
) -> Result<Option<ArtifactRef>, sqlx::Error> {
    let row: Option<(String, String, i64)> = sqlx::query_as(include_str!("../queries/run_artifact_get.sql"))
        .bind(workspace_id)
        .bind(run_id)
        .bind(kind)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(sha256, content_type, size)| ArtifactRef { sha256, content_type, size }))
}

/// S3-compatible artifact storage (the `s3` feature).
#[cfg(feature = "s3")]
pub mod s3 {
    use std::io;
    use std::sync::Arc;

    use object_store::aws::AmazonS3Builder;
    use object_store::path::Path as ObjectPath;
    use object_store::{ObjectStore, ObjectStoreExt, PutPayload};

    use super::{is_sha256_hex, sha256_hex, ArtifactStore, Stored};

    /// Artifacts in a bucket, at `<prefix>/sha256/<2 hex>/<2 hex>/<hash>`.
    #[derive(Debug, Clone)]
    pub struct S3ArtifactStore {
        store: Arc<dyn ObjectStore>,
        bucket: String,
        prefix: String,
    }

    fn not_found(e: &object_store::Error) -> bool {
        matches!(e, object_store::Error::NotFound { .. })
    }

    impl S3ArtifactStore {
        /// A store for `bucket`, with credentials, region and endpoint from the
        /// standard variables: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
        /// `AWS_REGION`, and for MinIO `AWS_ENDPOINT` plus `AWS_ALLOW_HTTP=true`.
        ///
        /// # Errors
        /// Invalid or incomplete configuration.
        pub fn from_env(bucket: &str, prefix: &str) -> io::Result<Self> {
            let store =
                AmazonS3Builder::from_env().with_bucket_name(bucket).build().map_err(io::Error::other)?;
            Ok(Self::new(Arc::new(store), bucket, prefix))
        }

        /// A store over any object store (tests use the in-memory one).
        pub fn new(store: Arc<dyn ObjectStore>, bucket: &str, prefix: &str) -> Self {
            Self { store, bucket: bucket.to_string(), prefix: prefix.trim_matches('/').to_string() }
        }

        /// The bucket name.
        pub fn bucket(&self) -> &str {
            &self.bucket
        }

        /// The key prefix, without slashes at either end (may be empty).
        pub fn prefix(&self) -> &str {
            &self.prefix
        }

        fn location(&self, sha256: &str) -> ObjectPath {
            let rel = ArtifactStore::relative_path(sha256);
            if self.prefix.is_empty() {
                ObjectPath::from(rel)
            } else {
                ObjectPath::from(format!("{}/{rel}", self.prefix))
            }
        }

        /// Uploads `bytes` unless an object with their hash already exists.
        ///
        /// # Errors
        /// Any object-store error.
        pub async fn put(&self, bytes: Vec<u8>) -> io::Result<Stored> {
            let sha256 = sha256_hex(&bytes);
            let size = bytes.len() as u64;
            let location = self.location(&sha256);
            match self.store.head(&location).await {
                Ok(_) => {}
                Err(e) if not_found(&e) => {
                    self.store.put(&location, PutPayload::from(bytes)).await.map_err(io::Error::other)?;
                }
                Err(e) => return Err(io::Error::other(e)),
            }
            Ok(Stored { path: ArtifactStore::relative_path(&sha256), sha256, size })
        }

        /// Writes the object for `sha256` to `dest`, in 8 MiB ranges.
        ///
        /// # Errors
        /// `NotFound` if absent, or any object-store or I/O error.
        pub async fn download(&self, sha256: &str, dest: &std::path::Path) -> io::Result<()> {
            use tokio::io::AsyncWriteExt;
            const PART: u64 = 8 << 20;
            let location = self.location(sha256);
            let size = match self.store.head(&location).await {
                Ok(meta) => meta.size,
                Err(e) if not_found(&e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("artifact {sha256} not found"),
                    ))
                }
                Err(e) => return Err(io::Error::other(e)),
            };
            if let Some(dir) = dest.parent() {
                tokio::fs::create_dir_all(dir).await?;
            }
            let mut file = tokio::fs::File::create(dest).await?;
            let mut at = 0u64;
            while at < size {
                let end = (at + PART).min(size);
                let part = self.store.get_range(&location, at..end).await.map_err(io::Error::other)?;
                file.write_all(&part).await?;
                at = end;
            }
            file.sync_all().await?;
            Ok(())
        }

        /// Removes the object for `sha256`; a missing object is not an error.
        ///
        /// # Errors
        /// `InvalidInput` for a malformed hash, or any object-store error.
        pub async fn delete(&self, sha256: &str) -> io::Result<()> {
            if !is_sha256_hex(sha256) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a lower-case SHA-256 hex string",
                ));
            }
            match self.store.delete(&self.location(sha256)).await {
                Err(e) if !not_found(&e) => Err(io::Error::other(e)),
                _ => Ok(()),
            }
        }

        /// The bytes for `sha256`, re-verified against it.
        ///
        /// # Errors
        /// `InvalidInput` for a malformed hash, `NotFound` if absent,
        /// `InvalidData` if the object no longer matches its hash, or any
        /// object-store error.
        pub async fn get(&self, sha256: &str) -> io::Result<Vec<u8>> {
            if !is_sha256_hex(sha256) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a lower-case SHA-256 hex string",
                ));
            }
            let bytes = match self.store.get(&self.location(sha256)).await {
                Ok(found) => found.bytes().await.map_err(io::Error::other)?,
                Err(e) if not_found(&e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("artifact {sha256} not found"),
                    ))
                }
                Err(e) => return Err(io::Error::other(e)),
            };
            if sha256_hex(&bytes) != sha256 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("artifact {sha256} is corrupt"),
                ));
            }
            Ok(bytes.to_vec())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use object_store::memory::InMemory;

        #[tokio::test]
        async fn put_get_and_tamper_detection() {
            let mem: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
            let s = S3ArtifactStore::new(Arc::clone(&mem), "bucket", "/runs/");
            assert_eq!(s.prefix(), "runs");
            let a = s.put(b"hello".to_vec()).await.unwrap();
            assert_eq!(a.path, format!("sha256/2c/f2/{}", a.sha256));
            assert_eq!(s.put(b"hello".to_vec()).await.unwrap(), a);
            assert_eq!(s.get(&a.sha256).await.unwrap(), b"hello");
            mem.put(&ObjectPath::from(format!("runs/{}", a.path)), PutPayload::from(b"tampered".to_vec()))
                .await
                .unwrap();
            assert_eq!(s.get(&a.sha256).await.unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(s.get(&"0".repeat(64)).await.unwrap_err().kind(), io::ErrorKind::NotFound);
            assert_eq!(s.get("../x").await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }

        #[tokio::test]
        async fn local_file_downloads_in_parts() {
            let s = S3ArtifactStore::new(Arc::new(InMemory::new()), "bucket", "p");
            let data: Vec<u8> = (0..(9u32 << 20)).map(|i| (i % 251) as u8).collect(); // > one 8 MiB part
            let a = s.put(data.clone()).await.unwrap();
            let backend = super::super::ArtifactBackend::S3(s);
            let dir = std::env::temp_dir().join(format!("spectra-s3-dl-{}", std::process::id()));
            let f = backend.local_file(&a.sha256, &dir).await.unwrap();
            assert!(f.temporary);
            assert_eq!(std::fs::read(&f.path).unwrap(), data);
            assert_eq!(
                backend.local_file(&"0".repeat(64), &dir).await.unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            let _ = std::fs::remove_dir_all(dir);
        }

        #[tokio::test]
        async fn delete_removes_the_object_and_tolerates_absence() {
            let s = S3ArtifactStore::new(Arc::new(InMemory::new()), "bucket", "p");
            let a = s.put(b"bye".to_vec()).await.unwrap();
            s.delete(&a.sha256).await.unwrap();
            assert_eq!(s.get(&a.sha256).await.unwrap_err().kind(), io::ErrorKind::NotFound);
            s.delete(&a.sha256).await.unwrap();
            assert_eq!(s.delete("x").await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> ArtifactStore {
        let dir = std::env::temp_dir().join(format!("spectra-artifacts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ArtifactStore::new(dir)
    }

    #[test]
    fn put_is_content_addressed_and_idempotent() {
        let s = temp_store("put");
        let a = s.put(b"hello").unwrap();
        assert_eq!(a.sha256, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert_eq!(a.path, format!("sha256/2c/f2/{}", a.sha256));
        assert_eq!(a.size, 5);
        assert_eq!(s.put(b"hello").unwrap(), a);
        assert_eq!(s.get(&a.sha256).unwrap(), b"hello");
        let leftovers: Vec<_> = fs::read_dir(s.root().join("sha256/2c/f2"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(s.root());
    }

    #[tokio::test]
    async fn the_local_backend_wraps_the_store() {
        let s = temp_store("backend");
        let b = ArtifactBackend::from(s.clone());
        let a = b.put(b"curve".to_vec()).await.unwrap();
        assert_eq!(b.get(&a.sha256).await.unwrap(), b"curve");
        assert!(b.describe().starts_with("local:"));
        let _ = fs::remove_dir_all(s.root());
    }

    #[test]
    fn get_verifies_and_validates() {
        let s = temp_store("get");
        let a = s.put(b"report").unwrap();
        fs::write(s.root().join(&a.path), b"tampered").unwrap();
        assert_eq!(s.get(&a.sha256).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(s.get("../../etc/passwd").unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(s.get(&"0".repeat(64)).unwrap_err().kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(s.root());
    }

    #[tokio::test]
    async fn local_file_is_the_stored_file_itself() {
        let s = temp_store("local-file");
        let stored = s.put(b"a,b\n1,2\n").unwrap();
        let backend = ArtifactBackend::Local(s.clone());
        let f = backend.local_file(&stored.sha256, Path::new("/nonexistent")).await.unwrap();
        assert!(!f.temporary);
        assert_eq!(fs::read(&f.path).unwrap(), b"a,b\n1,2\n");
        assert_eq!(
            backend.local_file(&"0".repeat(64), Path::new("/x")).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            backend.local_file("../x", Path::new("/x")).await.unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let _ = fs::remove_dir_all(s.root());
    }

    #[test]
    fn put_file_moves_a_hashed_file_in_and_delete_removes_it() {
        let s = temp_store("put-file");
        let staging = s.staging_dir();
        fs::create_dir_all(&staging).unwrap();
        let file = staging.join("upload-1");
        fs::write(&file, b"date,close\n").unwrap();
        let sha = sha256_hex(b"date,close\n");
        let stored = s.put_file(&file, &sha).unwrap();
        assert_eq!((stored.sha256.as_str(), stored.size), (sha.as_str(), 11));
        assert!(!file.exists(), "moved, not copied");
        assert_eq!(s.get(&sha).unwrap(), b"date,close\n");
        // The same bytes again: the incoming file is dropped, the stored one kept.
        fs::write(&file, b"date,close\n").unwrap();
        assert_eq!(s.put_file(&file, &sha).unwrap(), stored);
        assert!(!file.exists());
        assert_eq!(s.put_file(&file, "nope").unwrap_err().kind(), io::ErrorKind::InvalidInput);

        s.delete(&sha).unwrap();
        assert_eq!(s.get(&sha).unwrap_err().kind(), io::ErrorKind::NotFound);
        s.delete(&sha).unwrap();
        let _ = fs::remove_dir_all(s.root());
    }
}
