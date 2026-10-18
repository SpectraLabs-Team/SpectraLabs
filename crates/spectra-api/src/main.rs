//! `spectra-api`: serves the dashboard API. See the crate documentation.

use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;

use spectra_api::{handle, http, ApiConfig};

fn main() -> ExitCode {
    let addr = std::env::var("SPECTRA_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8000".into());
    let cfg = ApiConfig::from_env();
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("spectra-api: cannot listen on {addr}: {e}");
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "spectra-api {} listening on http://{addr} (CORS origins: {})",
        spectra_core::ENGINE_VERSION,
        cfg.cors_origins.join(", ")
    );
    http::serve(&listener, Arc::new(move |req| handle(&cfg, req)));
    ExitCode::SUCCESS
}
