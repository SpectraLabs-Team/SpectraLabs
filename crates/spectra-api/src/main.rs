//! `spectra-api`: serves the dashboard API. See the crate documentation.

use std::process::ExitCode;

use spectra_api::{serve, shutdown_signal, ApiConfig, Config};
use spectra_db::PoolConfig;

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprint!("spectra-api: {e}");
            return ExitCode::from(2);
        }
    };

    let pool_cfg = PoolConfig {
        max_connections: cfg.db_max_connections,
        ..PoolConfig::new(cfg.database_url.expose_str())
    };
    let pool = match spectra_db::connect(&pool_cfg).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("spectra-api: cannot connect to the database: {e}");
            return ExitCode::from(1);
        }
    };
    if cfg.migrate_on_start {
        if let Err(e) = spectra_db::migrate(&pool).await {
            eprintln!("spectra-api: migrations failed: {e}");
            return ExitCode::from(1);
        }
        eprintln!("spectra-api: migrations applied");
    }

    let ip = cfg.addr.ip();
    if ip.is_unspecified() {
        eprintln!("spectra-api: listening on all interfaces; do not publish this port (docker compose keeps it private)");
    } else if !(ip.is_loopback() || is_private(ip)) {
        eprintln!(
            "spectra-api: warning: listening on {ip}. Only the Next.js proxy should reach the engine: bind \
             127.0.0.1, or a private network as in docker compose, and never publish the port."
        );
    }
    let listener = match tokio::net::TcpListener::bind(cfg.addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("spectra-api: cannot listen on {}: {e}", cfg.addr);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "spectra-api {} listening on http://{} (CORS origins: {})",
        spectra_core::ENGINE_VERSION,
        cfg.addr,
        cfg.cors_origins.join(", ")
    );
    let result = serve(listener, ApiConfig::from(&cfg), Some(pool.clone()), shutdown_signal()).await;
    pool.close().await;
    match result {
        Ok(()) => {
            eprintln!("spectra-api: shut down cleanly");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("spectra-api: server error: {e}");
            ExitCode::from(1)
        }
    }
}

/// RFC 1918 IPv4 and unique-local IPv6 addresses.
fn is_private(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_private(),
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}
