//! Database tests. They need a Postgres to own: set `DATABASE_URL` (CI runs a
//! Postgres service). Without it they pass without doing anything, so
//! `cargo test --workspace` still works on a laptop with no database.

use spectra_db::{connect, migrate, pending_migrations, ping, seed_dev, PoolConfig};

async fn pool() -> Option<spectra_db::PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

#[tokio::test]
async fn migrations_apply_and_leave_nothing_pending() {
    let Some(pool) = pool().await else { return };
    ping(&pool).await.unwrap();
    assert!(pending_migrations(&pool).await.unwrap().is_empty());
    assert!(
        spectra_db::web_migrations::pending(&pool).await.unwrap().is_empty(),
        "dashboard migrations applied first"
    );
    // Running them again is a no-op.
    migrate(&pool).await.unwrap();
}

#[tokio::test]
async fn the_dev_seed_is_idempotent() {
    let Some(pool) = pool().await else { return };
    let a = seed_dev(&pool, " Seed-Test@Example.com ").await.unwrap();
    let b = seed_dev(&pool, "seed-test@example.com").await.unwrap();
    assert_eq!(a, b);
    let role: String =
        sqlx::query_scalar("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(a.workspace_id)
            .bind(a.admin_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(role, "owner");
    // Like a dashboard sign-up: the admin also has exactly one personal workspace they own.
    let personal: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE w.personal_owner_id = $1 AND m.user_id = $1 AND m.role = 'owner'",
    )
    .bind(a.admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(personal, 1);
}

#[tokio::test]
async fn database_ids_are_uuid_v7() {
    let Some(pool) = pool().await else { return };
    let id: uuid::Uuid = sqlx::query_scalar("SELECT spectra_uuidv7()").fetch_one(&pool).await.unwrap();
    assert_eq!(id.get_version_num(), 7);
}

#[tokio::test]
async fn queue_depth_counts_newly_enqueued_work() {
    let Some(pool) = pool().await else { return };
    let seed = seed_dev(&pool, "queue-metrics@example.test").await.unwrap();
    let before = spectra_db::jobs::queue_depth(&pool).await.unwrap();
    let job = spectra_db::jobs::enqueue(
        &pool,
        &spectra_db::jobs::NewJob {
            workspace_id: seed.workspace_id,
            kind: "risk".into(),
            payload: serde_json::json!({}),
            priority: 0,
            max_attempts: 1,
            created_by: Some(seed.admin_id),
        },
    )
    .await
    .unwrap();
    let after = spectra_db::jobs::queue_depth(&pool).await.unwrap();
    sqlx::query("DELETE FROM jobs WHERE id = $1").bind(job.id).execute(&pool).await.unwrap();
    assert!(after.0 > before.0, "queue depth increased: after={}, before={}", after.0, before.0);
    assert_eq!(after.1, before.1);
}

#[tokio::test]
async fn backtest_runs_link_dataset_versions_and_artifacts() {
    use spectra_db::artifacts::{find_for_run, store_for_run, ArtifactBackend, ArtifactStore, NewArtifact};
    use spectra_db::datasets::{
        insert_backtest_run, register_version, resolve_version, resolve_workspace, Access, NewBacktestRun,
        NewDatasetVersion, Resolved,
    };

    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "backtest-test@example.com").await.unwrap();
    let ws = resolve_workspace(&pool, &seeded.workspace_id.to_string()).await.unwrap().unwrap();
    assert_eq!(ws, seeded.workspace_id);

    // A fresh checksum and dataset name, so the test can run against a shared database.
    let checksum = format!("{}{}", spectra_db::new_id().simple(), spectra_db::new_id().simple());
    let name = format!("bt-{}", spectra_db::new_id().simple());
    let new = NewDatasetVersion {
        workspace_id: ws,
        dataset_name: &name,
        checksum: &checksum,
        row_count: 2400,
        byte_size: 9000,
        quality_report: serde_json::json!({ "rows_rejected": 0 }),
        created_by: Some(seeded.admin_id),
        blob_sha256: None,
        source: None,
    };
    let v1 = register_version(&pool, &new).await.unwrap();
    assert_eq!((v1.version, v1.existed), (1, false));
    let again = register_version(&pool, &new).await.unwrap();
    assert_eq!((again.id, again.existed), (v1.id, true));

    let resolved = resolve_version(&pool, ws, v1.id, Access::engine("test")).await.unwrap().found().unwrap();
    assert_eq!(
        (resolved.checksum.as_str(), resolved.row_count, resolved.dataset_name.as_str()),
        (checksum.as_str(), 2400, name.as_str())
    );
    assert_eq!(
        resolve_version(&pool, spectra_db::new_id(), v1.id, Access::engine("test")).await.unwrap(),
        Resolved::Missing,
        "other workspace"
    );
    let listed = spectra_db::datasets::list_versions(&pool, ws, 500).await.unwrap();
    let mine = listed.iter().find(|v| v.id == v1.id).expect("listed");
    assert_eq!(
        (mine.dataset_name.as_str(), mine.version, mine.row_count, mine.byte_size),
        (name.as_str(), 1, 2400, 9000)
    );
    assert!(spectra_db::datasets::list_versions(&pool, spectra_db::new_id(), 500).await.unwrap().is_empty());

    let job_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO jobs (workspace_id, kind, payload, status, attempts, started_at, finished_at, created_by) \
         VALUES ($1, 'backtest', $2, 'succeeded', 1, now(), now(), $3) RETURNING id",
    )
    .bind(ws)
    .bind(serde_json::json!({ "dataset_version_id": v1.id.to_string() }))
    .bind(seeded.admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let page = spectra_db::jobs::list_kind(&pool, ws, "backtest", None, None, 200).await.unwrap();
    assert!(page.items.iter().all(|j| j.kind == "backtest"));
    assert!(page.items.iter().any(|j| j.id == job_id), "listed by kind");
    let run_id = insert_backtest_run(
        &pool,
        &NewBacktestRun {
            id: spectra_db::new_id(),
            workspace_id: ws,
            job_id,
            params: serde_json::json!({ "strategy": "sma_cross" }),
            metrics: serde_json::json!({ "schema": "spectra.backtest.summary/v1" }),
            seed: Some(7),
            engine_version: "test".into(),
            created_by: Some(seeded.admin_id),
            started_at: chrono::Utc::now(),
            dataset_version_id: v1.id,
            strategy_version_id: None,
            strategy: "sma_cross".into(),
            strategy_sha256: String::new(),
        },
    )
    .await
    .unwrap();

    let dir = std::env::temp_dir().join(format!("spectra-db-bt-{}", spectra_db::new_id().simple()));
    let store = ArtifactBackend::Local(ArtifactStore::new(&dir));
    let bytes = br#"{"kind":"equity","points":[]}"#;
    let artifact = NewArtifact {
        workspace_id: ws,
        run_id,
        kind: "equity_series",
        content_type: "application/json",
        bytes,
    };
    let stored = store_for_run(&pool, &store, artifact, 1 << 30).await.unwrap();
    let found = find_for_run(&pool, ws, run_id, "equity_series").await.unwrap().unwrap();
    assert_eq!(found.sha256, stored.sha256);
    assert_eq!(store.get(&found.sha256).await.unwrap(), bytes);
    assert!(find_for_run(&pool, ws, run_id, "drawdown_series").await.unwrap().is_none());
    assert!(find_for_run(&pool, spectra_db::new_id(), run_id, "equity_series").await.unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);

    // Leave the shared database as found: the SQL checks that run later count jobs and events.
    for (sql, id) in [
        ("DELETE FROM runs WHERE id = $1", run_id),
        ("DELETE FROM jobs WHERE id = $1", job_id),
        ("DELETE FROM datasets WHERE id = (SELECT dataset_id FROM dataset_versions WHERE id = $1)", v1.id),
    ] {
        sqlx::query(sql).bind(id).execute(&pool).await.unwrap();
    }
}

#[tokio::test]
async fn signed_in_identities_are_mirrored_once() {
    use spectra_db::identity::ensure_identity;

    let Some(pool) = pool().await else { return };
    // Ids the engine has never seen (the frontend makes its own).
    let (user, workspace) = (spectra_db::new_id(), spectra_db::new_id());
    let enqueue = |by: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            spectra_db::jobs::enqueue(
                &pool,
                &spectra_db::jobs::NewJob {
                    workspace_id: workspace,
                    kind: "backtest".into(),
                    payload: serde_json::json!({}),
                    priority: 0,
                    max_attempts: 1,
                    created_by: Some(by),
                },
            )
            .await
        }
    };
    let err = enqueue(user).await.expect_err("unknown workspace and user");
    assert_eq!(err.as_database_error().and_then(sqlx::error::DatabaseError::code).as_deref(), Some("23503"));

    ensure_identity(&pool, Some(user), workspace).await.unwrap();
    sqlx::query("UPDATE workspaces SET name = 'Kept' WHERE id = $1")
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
    ensure_identity(&pool, Some(user), workspace).await.unwrap();
    let (name, created_by): (String, Option<uuid::Uuid>) =
        sqlx::query_as("SELECT name, created_by FROM workspaces WHERE id = $1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((name.as_str(), created_by), ("Kept", Some(user)), "existing rows are left alone");

    let job = enqueue(user).await.expect("mirrored identity can queue work");
    assert_eq!(job.created_by, Some(user));
    assert!(spectra_db::datasets::list_versions(&pool, workspace, 500).await.unwrap().is_empty());

    // Without a user (an API token with none), only the workspace is created.
    let token_ws = spectra_db::new_id();
    ensure_identity(&pool, None, token_ws).await.unwrap();

    for (sql, id) in [
        ("DELETE FROM workspaces WHERE id = $1", workspace),
        ("DELETE FROM workspaces WHERE id = $1", token_ws),
        ("DELETE FROM users WHERE id = $1", user),
    ] {
        sqlx::query(sql).bind(id).execute(&pool).await.unwrap();
    }
}

#[tokio::test]
async fn strategy_versions_are_idempotent_and_numbered_under_concurrency() {
    use spectra_db::strategies::{create, get, get_version, save_version, versions, NewVersion};

    let Some(pool) = pool().await else { return };
    let ws = spectra_db::new_id();
    spectra_db::identity::ensure_identity(&pool, None, ws).await.unwrap();
    let s = create(&pool, ws, "Trend", "", None).await.unwrap().expect("new name");
    assert!(create(&pool, ws, "Trend", "", None).await.unwrap().is_none(), "name taken");

    let v = |cfg: char| NewVersion {
        builtin: "sma_cross".into(),
        source_hash: "1".repeat(64),
        code: None,
        config_hash: cfg.to_string().repeat(64),
        parameter_schema: serde_json::json!({}),
        parameters: serde_json::json!({ "cfg": cfg.to_string() }),
        created_by: None,
    };

    // Eight saves at once: four distinct parameter sets, each twice.
    let saves = ['a', 'b', 'c', 'd', 'a', 'b', 'c', 'd'].map(|c| {
        let (pool, nv) = (pool.clone(), v(c));
        tokio::spawn(async move { save_version(&pool, ws, s.id, &nv).await })
    });
    let mut created = 0;
    for h in saves {
        let (_, was_created) = h.await.unwrap().unwrap().expect("strategy exists");
        created += usize::from(was_created);
    }
    assert_eq!(created, 4, "each parameter set is saved once");
    let all = versions(&pool, ws, s.id).await.unwrap();
    assert_eq!(all.iter().map(|x| x.version).collect::<Vec<_>>(), vec![4, 3, 2, 1]);

    // Saving again returns the same row.
    let (again, was_created) = save_version(&pool, ws, s.id, &v('a')).await.unwrap().unwrap();
    assert!(!was_created);
    assert_eq!(Some(&again), all.iter().find(|x| x.config_hash == "a".repeat(64)));

    // Another workspace sees none of it.
    let other = spectra_db::new_id();
    spectra_db::identity::ensure_identity(&pool, None, other).await.unwrap();
    assert!(get(&pool, other, s.id).await.unwrap().is_none());
    assert!(get_version(&pool, other, again.id).await.unwrap().is_none());
    assert!(save_version(&pool, other, s.id, &v('e')).await.unwrap().is_none());
    assert_eq!(get(&pool, ws, s.id).await.unwrap().unwrap().version_count, 4);
}

#[tokio::test]
async fn service_role_provisioning_and_privileges() {
    let Some(pool) = pool().await else { return };
    let role = "test_spectra_app_unit";
    let outcome = spectra_db::roles::provision_service_roles(&pool, role, Some("secret_pw_123"))
        .await
        .expect("provision service role");
    assert_eq!(outcome.role_name, role);

    let ver = spectra_db::roles::verify_service_role(&pool, role).await.expect("verify service role");
    assert!(ver.can_connect);
    assert!(ver.can_read_workspaces);
    assert!(ver.can_write_jobs);
    assert!(ver.cannot_write_users);

    let backup_role = "test_spectra_backup_unit";
    let b_outcome = spectra_db::roles::provision_backup_role(&pool, backup_role, Some("backup_pw_123"))
        .await
        .expect("provision backup role");
    assert_eq!(b_outcome.role_name, backup_role);

    let b_ver = spectra_db::roles::verify_backup_role(&pool, backup_role).await.expect("verify backup role");
    assert!(b_ver.can_connect);
    assert!(b_ver.can_read_all);
    assert!(b_ver.cannot_write);
}

#[tokio::test]
async fn activity_purge_removes_only_old_rows_and_never_overlaps() {
    use spectra_db::activity::{purge_activity, PurgeOutcome, PURGE_LOCK};
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "activity-purge-test@example.com").await.unwrap();
    let user = seeded.admin_id;
    sqlx::query("DELETE FROM user_activity_daily WHERE user_id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    for age in [500, 401, 399, 10] {
        sqlx::query(
            "INSERT INTO user_activity_daily (user_id, day, active_seconds, heartbeats) \
             VALUES ($1, (now() AT TIME ZONE 'UTC')::date - $2::int, 60, 1)",
        )
        .bind(user)
        .bind(age)
        .execute(&pool)
        .await
        .unwrap();
    }
    let mine = || async {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM user_activity_daily WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
        n
    };

    // A dry run counts (at least our two old rows) and deletes nothing.
    match purge_activity(&pool, 400, true).await.unwrap() {
        PurgeOutcome::Done { rows, dry_run } => assert!(dry_run && rows >= 2, "dry run counted {rows}"),
        PurgeOutcome::Busy => panic!("dry run found the lock taken"),
    }
    assert_eq!(mine().await, 4, "a dry run deletes nothing");

    // While another session holds the lock, a purge does nothing.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(PURGE_LOCK)
        .execute(&mut *holder)
        .await
        .unwrap();
    assert_eq!(purge_activity(&pool, 400, false).await.unwrap(), PurgeOutcome::Busy);
    assert_eq!(mine().await, 4, "a busy purge deletes nothing");
    holder.rollback().await.unwrap();

    // The real purge removes the rows older than 400 days and keeps the rest.
    assert!(matches!(
        purge_activity(&pool, 400, false).await.unwrap(),
        PurgeOutcome::Done { dry_run: false, .. }
    ));
    let kept: Vec<i32> = sqlx::query_scalar(
        "SELECT ((now() AT TIME ZONE 'UTC')::date - day)::int FROM user_activity_daily WHERE user_id = $1 ORDER BY 1",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kept, vec![10, 399]);

    // Running it again is safe: nothing of ours left to delete.
    purge_activity(&pool, 400, false).await.unwrap();
    assert_eq!(mine().await, 2);

    assert!(purge_activity(&pool, 0, false).await.is_err(), "retention below 1 day is refused");
}

#[tokio::test]
async fn report_versions_count_up_under_concurrent_exports() {
    use spectra_db::reports::{create_version, for_run, get_version, versions_of, NewReport};

    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "reports-test@example.com").await.unwrap();
    let ws = seeded.workspace_id;
    // A fresh run id, so the test can run against a shared database.
    let run = spectra_db::new_id();

    // Eight exports of the same run at once: versions 1..=8, one report id, nothing lost.
    let tasks: Vec<_> = (0..8u8)
        .map(|i| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let body = vec![b'%', b'P', b'D', b'F', i];
                let report = NewReport {
                    report_type: "validation",
                    run_kind: "sweep",
                    run_id: run,
                    format: "pdf",
                    content_type: "application/pdf",
                    content: &body,
                    created_by: None,
                };
                create_version(&pool, ws, &report).await.unwrap()
            })
        })
        .collect();
    let mut created = Vec::new();
    for t in tasks {
        created.push(t.await.unwrap());
    }
    let mut versions: Vec<i32> = created.iter().map(|v| v.version).collect();
    versions.sort_unstable();
    assert_eq!(versions, (1..=8).collect::<Vec<_>>());
    let report_id = created[0].report_id;
    assert!(created.iter().all(|v| v.report_id == report_id));

    // Fetch by report id and version: the bytes that version was created with.
    let v3 = created.iter().find(|v| v.version == 3).unwrap();
    let got = get_version(&pool, ws, report_id, 3).await.unwrap().unwrap();
    assert_eq!(got.meta, *v3);
    assert_eq!(got.content.len(), 5);
    assert_eq!(spectra_core::sha256_hex(&got.content), v3.sha256);
    assert!(get_version(&pool, spectra_db::new_id(), report_id, 3).await.unwrap().is_none());

    let listed = versions_of(&pool, ws, report_id).await.unwrap();
    assert_eq!(listed.iter().map(|v| v.version).collect::<Vec<_>>(), (1..=8).rev().collect::<Vec<_>>());
    assert_eq!(for_run(&pool, ws, "sweep", run).await.unwrap().len(), 8);

    // An unknown format is refused by the table.
    let bad = NewReport {
        format: "docx",
        ..NewReport {
            report_type: "validation",
            run_kind: "sweep",
            run_id: run,
            format: "pdf",
            content_type: "application/pdf",
            content: b"x",
            created_by: None,
        }
    };
    assert!(create_version(&pool, ws, &bad).await.is_err());
}
