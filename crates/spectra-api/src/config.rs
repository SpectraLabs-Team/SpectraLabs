//! Service configuration, read once from the environment at startup. Every
//! problem is reported together, and the process refuses to start until they
//! are fixed: a missing secret is an error, never a default.
//!
//! | variable | required | default |
//! |---|---|---|
//! | `DATABASE_URL` | yes | |
//! | `ENGINE_SECRET` | yes (32+ characters) | |
//! | `SPECTRA_API_ADDR` | | `127.0.0.1:8000` |
//! | `SPECTRA_CORS_ORIGINS` | | `http://localhost:3000` |
//! | `SPECTRA_DB_MAX_CONNECTIONS` | | `10` |
//! | `SPECTRA_MIGRATE_ON_START` | | `false` |

use std::fmt;
use std::net::SocketAddr;

/// Shortest accepted `ENGINE_SECRET` (bytes). `openssl rand -base64 48` gives 64.
pub const MIN_SECRET_LEN: usize = 32;

/// A secret that never prints itself.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret's bytes, for signing.
    pub fn expose(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// The secret as text (a connection URL, say).
    pub fn expose_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Storage per workspace when it has no quota of its own (1 GiB).
pub const DEFAULT_ARTIFACT_QUOTA_BYTES: i64 = 1 << 30;

/// Everything `spectra-api` reads from the environment.
#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address. Keep it on 127.0.0.1 or a private network: only the
    /// Next.js proxy should reach the engine.
    pub addr: SocketAddr,
    /// Origins allowed by CORS.
    pub cors_origins: Vec<String>,
    /// Postgres URL.
    pub database_url: Secret,
    /// Pool size.
    pub db_max_connections: u32,
    /// Apply pending migrations at startup.
    pub migrate_on_start: bool,
    /// Shared with the Next.js proxy; signs the identity headers.
    pub engine_secret: Secret,
<<<<<<< HEAD
    /// Where run artifacts are read from (the worker's `ARTIFACT_*` settings).
    pub artifact_storage: ArtifactStorage,
=======
    /// Where the worker stores artifacts (the same `ARTIFACT_STORAGE` settings);
    /// read for backtest chart series, and written by uploads.
    pub artifact_storage: ArtifactStorage,
    /// Default storage quota per workspace (`SPECTRA_ARTIFACT_QUOTA_BYTES`,
    /// the worker's setting), applied to uploads.
    pub artifact_quota_bytes: i64,
    /// Optional local dataset store (`SPECTRA_DATA_DIR`), read to chart
    /// versions registered from the command line (the others are read from
    /// the artifact store).
    pub data_dir: Option<std::path::PathBuf>,
>>>>>>> origin/main
}

/// Everything wrong with the environment, one line per problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub Vec<String>);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "invalid configuration:")?;
        for p in &self.0 {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" | "" => Some(false),
        _ => None,
    }
}

impl Config {
    /// Reads the process environment.
    ///
    /// # Errors
    /// [`ConfigError`] listing every missing or invalid variable.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Reads configuration through `get` (the environment, or a map in tests).
    ///
    /// # Errors
    /// [`ConfigError`] listing every missing or invalid variable.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut problems = Vec::new();
        let val = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

        let addr_text = val("SPECTRA_API_ADDR").unwrap_or_else(|| "127.0.0.1:8000".into());
        let addr = addr_text.parse().unwrap_or_else(|_| {
            problems
                .push(format!("SPECTRA_API_ADDR must be host:port with an IP address, got {addr_text:?}"));
            SocketAddr::from(([127, 0, 0, 1], 8000))
        });

        let cors_origins = val("SPECTRA_CORS_ORIGINS").map_or_else(
            || vec!["http://localhost:3000".to_string()],
            |v| v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        );

        let database_url = match val("DATABASE_URL") {
            Some(u) if u.starts_with("postgres://") || u.starts_with("postgresql://") => Secret::new(u),
            Some(_) => {
                problems.push("DATABASE_URL must start with postgres:// or postgresql://".into());
                Secret::new("")
            }
            None => {
                problems.push(
                    "DATABASE_URL is not set (e.g. postgres://spectra:spectra@127.0.0.1:5432/spectra)".into(),
                );
                Secret::new("")
            }
        };

        let db_max_connections = match val("SPECTRA_DB_MAX_CONNECTIONS").map(|v| v.parse::<u32>()) {
            None => 10,
            Some(Ok(n)) if (1..=200).contains(&n) => n,
            Some(_) => {
                problems.push("SPECTRA_DB_MAX_CONNECTIONS must be a whole number from 1 to 200".into());
                10
            }
        };

        let migrate_on_start = match val("SPECTRA_MIGRATE_ON_START").as_deref().map(parse_bool) {
            None => false,
            Some(Some(b)) => b,
            Some(None) => {
                problems.push("SPECTRA_MIGRATE_ON_START must be true or false".into());
                false
            }
        };

        let engine_secret = match val("ENGINE_SECRET") {
            Some(s) if s.len() >= MIN_SECRET_LEN => Secret::new(s),
            Some(_) => {
                problems.push(format!("ENGINE_SECRET must be at least {MIN_SECRET_LEN} characters"));
                Secret::new("")
            }
            None => {
                problems.push(
                    "ENGINE_SECRET is not set (the same value as the frontend's; `openssl rand -base64 48`)"
                        .into(),
                );
                Secret::new("")
            }
        };

<<<<<<< HEAD
        let artifact_storage = parse_artifact_storage(&val, &mut problems);

=======
        let artifact_storage = read_artifact_storage(&val, &mut problems);
        let artifact_quota_bytes = match val("SPECTRA_ARTIFACT_QUOTA_BYTES").map(|v| v.parse::<i64>()) {
            None => DEFAULT_ARTIFACT_QUOTA_BYTES,
            Some(Ok(n)) if n >= 0 => n,
            Some(_) => {
                problems.push("SPECTRA_ARTIFACT_QUOTA_BYTES must be a whole number of bytes".into());
                DEFAULT_ARTIFACT_QUOTA_BYTES
            }
        };
>>>>>>> origin/main
        if problems.is_empty() {
            Ok(Self {
                addr,
                cors_origins,
                database_url,
                db_max_connections,
                migrate_on_start,
                engine_secret,
                artifact_storage,
<<<<<<< HEAD
=======
                artifact_quota_bytes,
                data_dir: val("SPECTRA_DATA_DIR").map(Into::into),
>>>>>>> origin/main
            })
        } else {
            Err(ConfigError(problems))
        }
    }
}

<<<<<<< HEAD
/// `ARTIFACT_STORAGE`, `ARTIFACT_DIR`, `ARTIFACT_S3_BUCKET` and
/// `ARTIFACT_S3_PREFIX`, shared by the API (which reads artifacts) and the
/// worker (which writes them).
fn parse_artifact_storage(
    val: &impl Fn(&str) -> Option<String>,
=======
/// `ARTIFACT_STORAGE` with `ARTIFACT_DIR` or `ARTIFACT_S3_BUCKET` and
/// `ARTIFACT_S3_PREFIX`, shared by the worker (which writes) and the API
/// (which reads chart series).
fn read_artifact_storage(
    val: &dyn Fn(&str) -> Option<String>,
>>>>>>> origin/main
    problems: &mut Vec<String>,
) -> ArtifactStorage {
    let artifact_dir = val("ARTIFACT_DIR").map_or_else(|| std::path::PathBuf::from("artifacts"), Into::into);
    match val("ARTIFACT_STORAGE").as_deref().unwrap_or("local") {
        "local" => ArtifactStorage::Local(artifact_dir),
        "s3" => {
            if let Some(bucket) = val("ARTIFACT_S3_BUCKET") {
                ArtifactStorage::S3 { bucket, prefix: val("ARTIFACT_S3_PREFIX").unwrap_or_default() }
            } else {
                problems.push("ARTIFACT_S3_BUCKET must be set when ARTIFACT_STORAGE=s3".into());
                ArtifactStorage::Local(artifact_dir)
            }
        }
        other => {
            problems.push(format!("ARTIFACT_STORAGE must be local or s3, not {other:?}"));
            ArtifactStorage::Local(artifact_dir)
        }
    }
}

<<<<<<< HEAD
/// Where the worker stores artifacts and the API reads them.
=======
/// The artifact backend for `storage`.
///
/// # Errors
/// An S3 bucket that cannot be configured, or `s3` asked of a build without
/// the `s3` feature.
pub fn artifact_backend(storage: &ArtifactStorage) -> Result<spectra_db::artifacts::ArtifactBackend, String> {
    use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
    match storage {
        ArtifactStorage::Local(dir) => Ok(ArtifactBackend::Local(ArtifactStore::new(dir))),
        #[cfg(feature = "s3")]
        ArtifactStorage::S3 { bucket, prefix } => {
            spectra_db::artifacts::s3::S3ArtifactStore::from_env(bucket, prefix)
                .map(ArtifactBackend::S3)
                .map_err(|e| format!("cannot use artifact bucket {bucket}: {e}"))
        }
        #[cfg(not(feature = "s3"))]
        ArtifactStorage::S3 { .. } => Err("ARTIFACT_STORAGE=s3, but this build lacks the `s3` feature \
                                           (cargo build --features s3)"
            .into()),
    }
}

/// Where the worker stores artifacts.
>>>>>>> origin/main
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactStorage {
    /// `ARTIFACT_STORAGE=local` (the default): the `ARTIFACT_DIR` directory.
    Local(std::path::PathBuf),
    /// `ARTIFACT_STORAGE=s3`: `ARTIFACT_S3_BUCKET` (required) and
    /// `ARTIFACT_S3_PREFIX` (optional). Credentials come from the standard
    /// `AWS_*` variables. Needs a worker built with the `s3` feature.
    S3 {
        /// Bucket name.
        bucket: String,
        /// Key prefix (may be empty).
        prefix: String,
    },
}

/// Configuration used by the background job worker.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Postgres URL.
    pub database_url: Secret,
    /// Where content-addressed artifacts are stored.
    pub artifact_storage: ArtifactStorage,
    /// `SPECTRA_WORKER_METRICS_ADDR`: where the worker serves `/health` and
    /// Prometheus `/metrics`. Unset: no listener.
    pub metrics_addr: Option<SocketAddr>,
    /// Number of jobs this worker may run concurrently.
    pub concurrency: usize,
    /// Maximum jobs one workspace may run across all workers.
    pub workspace_max_running: i64,
    /// Job kinds this worker claims.
    pub kinds: Vec<String>,
    /// Default artifact quota per workspace.
    pub artifact_quota_bytes: i64,
    /// Root of the content-addressed dataset store that backtests read (`SPECTRA_DATA_DIR`).
    pub data_dir: std::path::PathBuf,
    /// Most bars one backtest may load (`SPECTRA_BACKTEST_MAX_BARS`).
    pub backtest_max_bars: usize,
    /// Most instruments one backtest may trade (`SPECTRA_BACKTEST_MAX_INSTRUMENTS`).
    pub backtest_max_instruments: usize,
}

impl WorkerConfig {
    /// Read worker settings from the process environment.
    ///
    /// # Errors
    /// Returns every missing or invalid worker setting.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Read worker settings from a lookup function, useful for tests.
    ///
    /// # Errors
    /// Returns every missing or invalid worker setting.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut problems = Vec::new();
        let val = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let mut number = |k: &str, default: i64, range: std::ops::RangeInclusive<i64>| match val(k) {
            None => default,
            Some(v) => match v.parse::<i64>() {
                Ok(n) if range.contains(&n) => n,
                _ => {
                    problems.push(format!(
                        "{k} must be a whole number from {} to {}",
                        range.start(),
                        range.end()
                    ));
                    default
                }
            },
        };
        let concurrency = number("SPECTRA_WORKER_CONCURRENCY", 2, 1..=64) as usize;
        let workspace_max_running = number("SPECTRA_WORKSPACE_MAX_RUNNING", 2, 1..=1000);
        let artifact_quota_bytes =
            number("SPECTRA_ARTIFACT_QUOTA_BYTES", DEFAULT_ARTIFACT_QUOTA_BYTES, 0..=i64::MAX);
        let backtest_max_bars = number("SPECTRA_BACKTEST_MAX_BARS", 20_000_000, 1..=1_000_000_000) as usize;
        let backtest_max_instruments =
            number("SPECTRA_BACKTEST_MAX_INSTRUMENTS", 1000, 1..=1_000_000) as usize;
        let all: Vec<String> = crate::engine::WORKER_KINDS.iter().map(|kind| (*kind).to_string()).collect();
        let kinds = match val("SPECTRA_WORKER_KINDS") {
            None => all,
            Some(value) => {
                let selected: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(str::to_string)
                    .collect();
                if selected.is_empty() || selected.iter().any(|kind| !all.contains(kind)) {
                    problems.push(format!("SPECTRA_WORKER_KINDS must list kinds from: {}", all.join(", ")));
                }
                selected
            }
        };
        let database_url = match val("DATABASE_URL") {
            Some(url) if url.starts_with("postgres://") || url.starts_with("postgresql://") => {
                Secret::new(url)
            }
            _ => {
                problems.push("DATABASE_URL must be set to a postgres:// URL".into());
                Secret::new("")
            }
        };
<<<<<<< HEAD
        let artifact_storage = parse_artifact_storage(&val, &mut problems);
=======
        let artifact_storage = read_artifact_storage(&val, &mut problems);
>>>>>>> origin/main
        let metrics_addr = val("SPECTRA_WORKER_METRICS_ADDR").and_then(|v| {
            let addr = v.parse::<SocketAddr>().ok();
            if addr.is_none() {
                problems.push("SPECTRA_WORKER_METRICS_ADDR must be host:port, e.g. 0.0.0.0:9100".into());
            }
            addr
        });
        let data_dir = val("SPECTRA_DATA_DIR").map_or_else(|| std::path::PathBuf::from("data"), Into::into);
        if problems.is_empty() {
            Ok(Self {
                database_url,
                artifact_storage,
                metrics_addr,
                concurrency,
                workspace_max_running,
                kinds,
                artifact_quota_bytes,
                data_dir,
                backtest_max_bars,
                backtest_max_instruments,
            })
        } else {
            Err(ConfigError(problems))
        }
    }
}

/// The artifact backend for `storage`.
///
/// # Errors
/// An S3 store that cannot be configured, or `s3` without the `s3` feature.
pub fn artifact_backend(storage: &ArtifactStorage) -> Result<spectra_db::artifacts::ArtifactBackend, String> {
    use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
    match storage {
        ArtifactStorage::Local(dir) => Ok(ArtifactBackend::Local(ArtifactStore::new(dir))),
        #[cfg(feature = "s3")]
        ArtifactStorage::S3 { bucket, prefix } => {
            spectra_db::artifacts::s3::S3ArtifactStore::from_env(bucket, prefix)
                .map(ArtifactBackend::S3)
                .map_err(|e| format!("cannot use artifact bucket {bucket}: {e}"))
        }
        #[cfg(not(feature = "s3"))]
        ArtifactStorage::S3 { .. } => Err("ARTIFACT_STORAGE=s3, but this binary was built without the `s3` \
             feature (cargo build --features s3)"
            .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> =
            pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
        move |k| m.get(k).cloned()
    }

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn minimal_valid_config_uses_defaults() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://u:p@localhost/db"),
            ("ENGINE_SECRET", SECRET),
        ]))
        .unwrap();
        assert_eq!(c.addr.to_string(), "127.0.0.1:8000");
        assert_eq!(c.cors_origins, ["http://localhost:3000"]);
        assert_eq!(c.db_max_connections, 10);
        assert!(!c.migrate_on_start);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let err = Config::from_lookup(lookup(&[
            ("SPECTRA_API_ADDR", "localhost"),
            ("ENGINE_SECRET", "short"),
            ("SPECTRA_MIGRATE_ON_START", "maybe"),
        ]))
        .unwrap_err();
        let text = err.to_string();
        assert_eq!(err.0.len(), 4, "{text}");
        for needle in ["SPECTRA_API_ADDR", "DATABASE_URL", "ENGINE_SECRET", "SPECTRA_MIGRATE_ON_START"] {
            assert!(text.contains(needle), "{text}");
        }
    }

    #[test]
    fn worker_artifact_storage_and_metrics_addr() {
        let base = [("DATABASE_URL", "postgres://u:p@localhost/db")];
        let c = WorkerConfig::from_lookup(lookup(&base)).unwrap();
        assert_eq!(c.artifact_storage, ArtifactStorage::Local("artifacts".into()));
        assert_eq!(c.metrics_addr, None);

        let c = WorkerConfig::from_lookup(lookup(&[
            base[0],
            ("ARTIFACT_STORAGE", "s3"),
            ("ARTIFACT_S3_BUCKET", "spectra-artifacts"),
            ("ARTIFACT_S3_PREFIX", "prod"),
            ("SPECTRA_WORKER_METRICS_ADDR", "0.0.0.0:9100"),
        ]))
        .unwrap();
        assert_eq!(
            c.artifact_storage,
            ArtifactStorage::S3 { bucket: "spectra-artifacts".into(), prefix: "prod".into() }
        );
        assert_eq!(c.metrics_addr.unwrap().port(), 9100);

        let err = WorkerConfig::from_lookup(lookup(&[
            base[0],
            ("ARTIFACT_STORAGE", "s3"),
            ("SPECTRA_WORKER_METRICS_ADDR", "nine-thousand"),
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("ARTIFACT_S3_BUCKET") && err.contains("SPECTRA_WORKER_METRICS_ADDR"), "{err}");
        let err = WorkerConfig::from_lookup(lookup(&[base[0], ("ARTIFACT_STORAGE", "ftp")])).unwrap_err();
        assert!(err.to_string().contains("ARTIFACT_STORAGE"), "{err}");
    }

    #[test]
    fn secrets_never_print() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://u:hunter2@localhost/db"),
            ("ENGINE_SECRET", SECRET),
        ]))
        .unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("hunter2") && !shown.contains(SECRET), "{shown}");
    }

    #[test]
    fn worker_config_reads_backtest_settings() {
        let db = ("DATABASE_URL", "postgres://u:p@localhost/db");
        let w = WorkerConfig::from_lookup(lookup(&[db])).unwrap();
        assert_eq!(w.data_dir, std::path::PathBuf::from("data"));
        assert_eq!((w.backtest_max_bars, w.backtest_max_instruments), (20_000_000, 1000));
        assert!(w.kinds.iter().any(|k| k == "backtest"));

        let w = WorkerConfig::from_lookup(lookup(&[
            db,
            ("SPECTRA_DATA_DIR", "/var/lib/spectra/datasets"),
            ("SPECTRA_BACKTEST_MAX_BARS", "5000"),
            ("SPECTRA_BACKTEST_MAX_INSTRUMENTS", "3"),
            ("SPECTRA_WORKER_KINDS", "backtest"),
        ]))
        .unwrap();
        assert_eq!(w.data_dir, std::path::PathBuf::from("/var/lib/spectra/datasets"));
        assert_eq!((w.backtest_max_bars, w.backtest_max_instruments), (5000, 3));
        assert_eq!(w.kinds, ["backtest"]);

        let err = WorkerConfig::from_lookup(lookup(&[db, ("SPECTRA_BACKTEST_MAX_BARS", "0")])).unwrap_err();
        assert!(err.to_string().contains("SPECTRA_BACKTEST_MAX_BARS"), "{err}");
    }
}
