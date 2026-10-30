//! `spectra-db`: database maintenance.
//!
//! ```text
//! spectra-db migrate   apply pending migrations: the dashboard's, then the engine's
//! spectra-db seed      migrate, then seed the dev workspace and admin (ADMIN_EMAIL)
//! spectra-db status    list pending migrations of both; exit 1 if any
//! spectra-db dataset-add --workspace ID|SLUG --name NAME --store DIR --id CHECKSUM
//!                      register a dataset-store entry (from `spectra-ingest`) as the next
//!                      version of a workspace dataset, and print the version id to run on
//! ```
//!
//! Reads `DATABASE_URL`.

use std::process::ExitCode;

use spectra_db::datasets::{register_version, resolve_workspace, NewDatasetVersion};
use spectra_db::{connect, migrate, pending_migrations, seed_dev, web_migrations, PoolConfig};

const USAGE: &str =
    "usage: spectra-db <migrate|seed|status|provision-roles>   (reads DATABASE_URL; seed also reads ADMIN_EMAIL)
       spectra-db dataset-add --workspace ID|SLUG --name NAME --store DIR --id CHECKSUM";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_default();
    let dataset_args = if cmd == "dataset-add" {
        let Some(a) = DatasetArgs::parse(&args[1..]) else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        Some(a)
    } else {
        None
    };
    if !matches!(cmd.as_str(), "migrate" | "seed" | "status" | "dataset-add" | "provision-roles") {
        eprintln!("{USAGE}");
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
    let result = match &dataset_args {
        Some(a) => add_dataset(a, &pool).await,
        None => run(&cmd, &pool).await,
    };
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
    if cmd == "provision-roles" {
        let app_user = std::env::var("POSTGRES_APP_USER").unwrap_or_else(|_| "spectra_app".into());
        let app_pw = std::env::var("POSTGRES_APP_PASSWORD").ok();
        let outcome = spectra_db::roles::provision_service_roles(pool, &app_user, app_pw.as_deref())
            .await
            .map_err(|e| format!("provision service roles: {e}"))?;
        let ver = spectra_db::roles::verify_service_role(pool, &app_user)
            .await
            .map_err(|e| format!("verify service role: {e}"))?;
        eprintln!(
            "spectra-db: provisioned role '{}' (created: {}, connect: {}, write_jobs: {}, read_workspaces: {}, cannot_write_users: {})",
            outcome.role_name,
            outcome.created,
            ver.can_connect,
            ver.can_write_jobs,
            ver.can_read_workspaces,
            ver.cannot_write_users
        );

        let backup_user = std::env::var("POSTGRES_BACKUP_USER").unwrap_or_else(|_| "spectra_backup".into());
        let backup_pw = std::env::var("POSTGRES_BACKUP_PASSWORD").ok();
        let b_outcome = spectra_db::roles::provision_backup_role(pool, &backup_user, backup_pw.as_deref())
            .await
            .map_err(|e| format!("provision backup role: {e}"))?;
        let b_ver = spectra_db::roles::verify_backup_role(pool, &backup_user)
            .await
            .map_err(|e| format!("verify backup role: {e}"))?;
        eprintln!(
            "spectra-db: provisioned backup role '{}' (created: {}, connect: {}, read_all: {}, cannot_write: {})",
            b_outcome.role_name,
            b_outcome.created,
            b_ver.can_connect,
            b_ver.can_read_all,
            b_ver.cannot_write
        );
    }
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

/// Arguments of `dataset-add`.
struct DatasetArgs {
    workspace: String,
    name: String,
    store: std::path::PathBuf,
    id: String,
}

impl DatasetArgs {
    fn parse(args: &[String]) -> Option<Self> {
        let (mut workspace, mut name, mut store, mut id) = (None, None, None, None);
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            let value = it.next()?.clone();
            match flag.as_str() {
                "--workspace" => workspace = Some(value),
                "--name" => name = Some(value),
                "--store" => store = Some(value.into()),
                "--id" => id = Some(value),
                _ => return None,
            }
        }
        Some(Self { workspace: workspace?, name: name?, store: store?, id: id? })
    }
}

/// Registers a dataset-store entry and prints the version id.
async fn add_dataset(a: &DatasetArgs, pool: &spectra_db::PgPool) -> Result<(), String> {
    let store = spectra_data::DataStore::new(a.store.clone()).map_err(|e| e.to_string())?;
    if !store.contains(&a.id) {
        return Err(format!("dataset {} is not in the store at {}", a.id, a.store.display()));
    }
    // Reading the dataset verifies it against its checksum.
    let d = store.get(&a.id).map_err(|e| format!("dataset {}: {e}", a.id))?;
    let byte_size = std::fs::metadata(store.root().join(&a.id).join("bars.qfb")).map_or(0, |m| m.len());
    let q = &d.quality;
    let quality = serde_json::json!({
        "rows_total": q.rows_total,
        "rows_accepted": q.rows_accepted,
        "rows_rejected": q.rows_rejected,
        "warnings": q.warnings,
        "symbols": d.symbols.symbols,
        "first_ts_ns": d.info.first_ts,
        "last_ts_ns": d.info.last_ts,
        "bars_sha256": d.info.bars_sha256,
    });

    let Some(workspace_id) = resolve_workspace(pool, &a.workspace).await.map_err(|e| e.to_string())? else {
        return Err(format!("no workspace with id or slug {:?}", a.workspace));
    };
    let v = register_version(
        pool,
        &NewDatasetVersion {
            workspace_id,
            dataset_name: &a.name,
            checksum: &a.id,
            row_count: d.bars.len() as i64,
            byte_size: i64::try_from(byte_size).unwrap_or(i64::MAX),
            quality_report: quality,
            created_by: None,
            blob_sha256: None,
            source: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    eprintln!(
        "spectra-db: {} {} version {} of {:?} ({} bars)",
        if v.existed { "already registered as" } else { "registered as" },
        v.id,
        v.version,
        a.name.trim(),
        d.bars.len()
    );
    // The id alone on stdout, for scripts.
    println!("{}", v.id);
    Ok(())
}
