//! `spectra-api`: serves the dashboard API. See the crate documentation.

use std::process::ExitCode;

use spectra_api::{serve, shutdown_signal, ApiConfig, Config};
use spectra_db::PoolConfig;

#[tokio::main]
async fn main() -> ExitCode {
    spectra_api::observability::init_observability();
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "invalid configuration");
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
            tracing::error!(error = %e, "cannot connect to the database");
            return ExitCode::from(1);
        }
    };
    if cfg.migrate_on_start {
        if let Err(e) = spectra_db::migrate(&pool).await {
            tracing::error!(error = %e, "migrations failed");
            return ExitCode::from(1);
        }
        tracing::info!("migrations applied");
    }

    let ip = cfg.addr.ip();
    if ip.is_unspecified() {
        tracing::warn!(
            "listening on all interfaces; do not publish this port (docker compose keeps it private)"
        );
    } else if !(ip.is_loopback() || is_private(ip)) {
        tracing::warn!(%ip, "listening on a public address; only the Next.js proxy should reach the engine");
    }
    let listener = match tokio::net::TcpListener::bind(cfg.addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(address = %cfg.addr, error = %e, "cannot listen");
            return ExitCode::from(1);
        }
    };
    tracing::info!(engine_version = spectra_core::ENGINE_VERSION, address = %cfg.addr, cors_origins = %cfg.cors_origins.join(", "), "API listening");
    let mut api_cfg = ApiConfig::from(&cfg);
    match spectra_api::config::artifact_backend(&cfg.artifact_storage) {
        Ok(store) => {
            tracing::info!(artifact_store = %store.describe(), "run artifacts readable");
            api_cfg.artifacts = Some(store);
        }
        Err(e) => {
            tracing::error!(error = %e, "cannot open the artifact store");
            return ExitCode::from(1);
        }
    }
    let result = serve(listener, api_cfg, Some(pool.clone()), shutdown_signal()).await;
    pool.close().await;
    match result {
        Ok(()) => {
            tracing::info!("shut down cleanly");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "server error");
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
