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

        if problems.is_empty() {
            Ok(Self { addr, cors_origins, database_url, db_max_connections, migrate_on_start, engine_secret })
        } else {
            Err(ConfigError(problems))
        }
    }
}

/// Configuration used by the background job worker.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Postgres URL.
    pub database_url: Secret,
    /// Directory for content-addressed artifacts.
    pub artifact_dir: std::path::PathBuf,
    /// Number of jobs this worker may run concurrently.
    pub concurrency: usize,
    /// Maximum jobs one workspace may run across all workers.
    pub workspace_max_running: i64,
    /// Job kinds this worker claims.
    pub kinds: Vec<String>,
    /// Default artifact quota per workspace.
    pub artifact_quota_bytes: i64,
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
        let artifact_quota_bytes = number("SPECTRA_ARTIFACT_QUOTA_BYTES", 1 << 30, 0..=i64::MAX);
        let all: Vec<String> = crate::engine::JOB_KINDS.iter().map(|kind| (*kind).to_string()).collect();
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
        let artifact_dir =
            val("ARTIFACT_DIR").map_or_else(|| std::path::PathBuf::from("artifacts"), Into::into);
        if problems.is_empty() {
            Ok(Self {
                database_url,
                artifact_dir,
                concurrency,
                workspace_max_running,
                kinds,
                artifact_quota_bytes,
            })
        } else {
            Err(ConfigError(problems))
        }
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
    fn secrets_never_print() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://u:hunter2@localhost/db"),
            ("ENGINE_SECRET", SECRET),
        ]))
        .unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("hunter2") && !shown.contains(SECRET), "{shown}");
    }
}
