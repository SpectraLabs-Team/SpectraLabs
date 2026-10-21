//! `spectra-db`: database maintenance.
//!
//! ```text
//! spectra-db migrate   apply pending migrations: the dashboard's, then the engine's
//! spectra-db seed      migrate, then seed the dev workspace and admin (ADMIN_EMAIL)
//! spectra-db status    list pending migrations of both; exit 1 if any
//! ```
//!
//! Reads `DATABASE_URL`.

use std::process::ExitCode;

use spectra_db::{connect, migrate, pending_migrations, seed_dev, web_migrations, PoolConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    if !matches!(cmd.as_str(), "migrate" | "seed" | "status") {
        eprintln!(
            "usage: spectra-db <migrate|seed|status>   (reads DATABASE_URL; seed also reads ADMIN_EMAIL)"
        );
        return ExitCode::from(2);
    }
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("spectra-db: DATABASE_URL is not set");
        return ExitCode::from(2);
    };
    let pool = match connect(&PoolConfig { max_connections: 2, ..PoolConfig::new(url) }).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("spectra-db: cannot connect: {e}");
            return ExitCode::from(1);
        }
    };
    let result = run(&cmd, &pool).await;
    pool.close().await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("spectra-db: {e}");
            ExitCode::from(1)
        }
    }
}

async fn run(cmd: &str, pool: &spectra_db::PgPool) -> Result<(), String> {
    if cmd == "status" {
        let web = web_migrations::pending(pool).await.map_err(|e| e.to_string())?;
        let pending = pending_migrations(pool).await.map_err(|e| e.to_string())?;
        if !web.is_empty() || !pending.is_empty() {
            return Err(format!("pending migrations: dashboard {web:?}, engine {pending:?}"));
        }
        eprintln!("spectra-db: schema is up to date");
        return Ok(());
    }
    let (applied, edited) = web_migrations::apply(pool).await.map_err(|e| e.to_string())?;
    for name in applied {
        eprintln!("spectra-db: applied dashboard migration {name}");
    }
    for web_migrations::Edited(name) in edited {
        eprintln!(
            "spectra-db: warning: dashboard migration {name} was edited after it was applied; \
             add a new migration instead"
        );
    }
    migrate(pool).await.map_err(|e| e.to_string())?;
    eprintln!("spectra-db: migrations applied");
    if cmd == "seed" {
        let email = std::env::var("ADMIN_EMAIL").map_err(|_| "ADMIN_EMAIL is not set".to_string())?;
        let s = seed_dev(pool, &email).await.map_err(|e| e.to_string())?;
        eprintln!(
            "spectra-db: seeded admin {} ({}) and workspace dev ({})",
            email.trim(),
            s.admin_id,
            s.workspace_id
        );
    }
    Ok(())
}
