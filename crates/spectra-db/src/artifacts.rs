//! Content-addressed artifact storage (equity curves, trade lists, reports).
//!
//! Bytes live on a local volume under `ARTIFACT_DIR`, at
//! `sha256/<first 2 hex>/<next 2 hex>/<full hash>`: the same bytes are stored
//! once, a file's name is its checksum, and every read re-verifies it. Each
//! use of an artifact is a `run_artifacts` row, and a workspace's distinct
//! hashes count against its quota (`workspaces.artifact_quota_bytes`, or a
//! default). S3-compatible storage can replace the volume later behind the
//! same `put`/`get`.

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

fn sha256_hex(bytes: &[u8]) -> String {
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

/// Stores an artifact and records it against its run, refusing it when the
/// workspace's quota would be exceeded. The quota is checked before any
/// bytes are written, and again (under a lock on the workspace row) when the
/// row is inserted. `default_quota` applies to workspaces without their own.
///
/// # Errors
/// [`ArtifactError`].
pub async fn store_for_run(
    pool: &PgPool,
    store: &ArtifactStore,
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

    let (bytes, artifact_store) = (a.bytes.to_vec(), store.clone());
    let stored = tokio::task::spawn_blocking(move || artifact_store.put(&bytes))
        .await
        .map_err(|e| ArtifactError::Io(io::Error::other(e)))?
        .map_err(ArtifactError::Io)?;

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
}
