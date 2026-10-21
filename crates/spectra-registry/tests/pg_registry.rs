#![cfg(feature = "postgres")]

use spectra_core::Json;
use spectra_db::{connect, migrate, new_id, PoolConfig};
use spectra_engine::{config_fingerprint, BacktestConfig, Metrics, RunRecord};
use spectra_registry::{PgRegistry, Registry, RunFilter, RunInput, RunPage, StoredArtifact};
use spectra_research::ParamSet;

async fn database_registry() -> Option<(PgRegistry, spectra_db::PgPool)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = connect(&PoolConfig::new(url)).await.expect("DATABASE_URL must be reachable");
    migrate(&pool).await.expect("migrations must apply");
    let workspace_id = new_id();
    let slug = format!("m5-{}", workspace_id.simple());
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'M5 integration test')")
        .bind(workspace_id)
        .bind(slug)
        .execute(&pool)
        .await
        .expect("test workspace must be created");
    Some((PgRegistry::new(pool.clone(), workspace_id), pool))
}

#[tokio::test]
async fn postgres_registry_commits_projections_and_exportable_chain_atomically() {
    let Some((registry, _pool)) = database_registry().await else { return };

    let parent = registry
        .create_experiment("baseline", "The baseline is reproducible", &["daily", "control"], "", None)
        .await
        .unwrap();
    let child =
        registry.fork_experiment(&parent, "variant", "Change one parameter", &["daily"], None).await.unwrap();
    assert_eq!(registry.lineage(&child).await.unwrap(), vec![parent.clone(), child.clone()]);

    let config = BacktestConfig::default();
    let input = RunInput {
        experiment_id: child.clone(),
        kind: "backtest".into(),
        dataset_id: "dataset-v1".into(),
        strategy: "buy-and-hold".into(),
        strategy_sha256: "b".repeat(64),
        params: ParamSet::default(),
        config,
        run: RunRecord {
            engine_version: "test-engine".into(),
            dataset_sha256: "a".repeat(64),
            config_sha256: config_fingerprint(&config),
            result_sha256: "d".repeat(64),
            seed: 7,
            bar_count: 3,
        },
        metrics: Metrics::default(),
        returns: vec![0.01, -0.02, 0.03],
        counts_as_trial: true,
        note: String::new(),
    };
    let run_id = registry.record_run(&input, None).await.unwrap();
    let run = registry.run(&run_id).await.unwrap().unwrap();
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.dataset_bars_sha256, "a".repeat(64));
    assert_eq!(registry.trial_count(Some(&child)).await.unwrap(), 1);

    let artifact_id = registry
        .record_artifact(
            &child,
            &run_id,
            "equity_curve",
            b"equity-series",
            "sha256/ab/cd/example",
            Json::object().with("points", Json::integer(3)),
        )
        .await
        .unwrap();
    registry.add_note(&run_id, "Reviewed against the source data", None).await.unwrap();
    let artifacts = registry.artifacts(&child).await.unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id, artifact_id);
    assert_eq!(registry.verify().await.unwrap().events, 5);

    let export = registry.export_jsonl().await.unwrap();
    let Some((imported, _)) = database_registry().await else { unreachable!("database remains configured") };
    assert_eq!(imported.import_jsonl(&export).await.unwrap(), 5);
    assert_eq!(imported.export_jsonl().await.unwrap(), export);
    assert!(imported.verify().await.unwrap().ok);
    assert_eq!(imported.experiments().await.unwrap().len(), 2);
    assert_eq!(imported.runs(&RunFilter::default()).await.unwrap().len(), 1);

    let temp = std::env::temp_dir().join(format!("spectra-pg-registry-{}", new_id()));
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(temp.join("registry.jsonl"), export).unwrap();
    let mut file_registry = Registry::open(&temp).unwrap();
    assert!(file_registry.verify().ok);
    assert_eq!(file_registry.experiments().unwrap().len(), 2);
    assert_eq!(file_registry.runs(&RunFilter::default()).unwrap().len(), 1);
    std::fs::remove_dir_all(temp).unwrap();
}

fn sample_input(experiment_id: &str, result_hex: char) -> RunInput {
    let config = BacktestConfig::default();
    RunInput {
        experiment_id: experiment_id.to_owned(),
        kind: "backtest".into(),
        dataset_id: "dataset-v1".into(),
        strategy: "sma_cross".into(),
        strategy_sha256: "b".repeat(64),
        params: ParamSet::default(),
        config,
        run: RunRecord {
            engine_version: "test-engine".into(),
            dataset_sha256: "a".repeat(64),
            config_sha256: config_fingerprint(&config),
            result_sha256: result_hex.to_string().repeat(64),
            seed: 7,
            bar_count: 3,
        },
        metrics: Metrics::default(),
        returns: vec![0.01, -0.02, 0.03],
        counts_as_trial: true,
        note: String::new(),
    }
}

#[tokio::test]
async fn completing_a_run_links_stored_artifacts_in_the_same_transaction() {
    let Some((registry, pool)) = database_registry().await else { return };
    let experiment = registry.create_experiment("artifacts", "", &[], "", None).await.unwrap();
    let input = sample_input(&experiment, 'c');
    let run_id = registry.create_queued_run(&input, None, None).await.unwrap();
    registry.start_run(run_id).await.unwrap();

    // The artifact store records its own row when it stores the bytes; the
    // registry adopts that row instead of adding a second one.
    let sha = "e".repeat(64);
    sqlx::query(
        "INSERT INTO run_artifacts (workspace_id, run_id, kind, sha256, path, size_bytes, content_type) \
         VALUES ($1, $2, 'equity_curve', $3, 'sha256/ee/ee/x', 10, 'text/csv')",
    )
    .bind(registry.workspace_id())
    .bind(run_id)
    .bind(&sha)
    .execute(&pool)
    .await
    .unwrap();
    let artifacts = [
        StoredArtifact {
            kind: "equity_curve".into(),
            sha256: sha.clone(),
            uri: "sha256/ee/ee/x".into(),
            size_bytes: 10,
            content_type: "text/csv".into(),
            summary: Json::object().with("rows", Json::integer(2)),
        },
        StoredArtifact {
            kind: "report".into(),
            sha256: "f".repeat(64),
            uri: "sha256/ff/ff/y".into(),
            size_bytes: 20,
            content_type: "application/json".into(),
            summary: Json::object(),
        },
    ];
    let registry_run = registry.complete_run_with_artifacts(run_id, &input, &artifacts).await.unwrap();

    let linked = registry.run_artifacts(&registry_run).await.unwrap();
    assert_eq!(linked.iter().map(|a| a.kind.as_str()).collect::<Vec<_>>(), ["equity_curve", "report"]);
    assert_eq!(linked[0].content_type, "text/csv");
    let kinds: Vec<String> = registry.events().await.unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, ["experiment.created", "run.recorded", "artifact.recorded", "artifact.recorded"]);
    assert!(registry.verify().await.unwrap().ok);

    // A bad artifact rolls the whole completion back: the run stays running.
    let other = registry.create_queued_run(&sample_input(&experiment, 'd'), None, None).await.unwrap();
    registry.start_run(other).await.unwrap();
    let bad = StoredArtifact { sha256: "not-a-hash".into(), ..artifacts[1].clone() };
    assert!(registry
        .complete_run_with_artifacts(other, &sample_input(&experiment, 'd'), &[bad])
        .await
        .is_err());
    assert_eq!(registry.run(&other.to_string()).await.unwrap().unwrap().status, "running");
    assert_eq!(registry.events().await.unwrap().len(), 4);
}

#[tokio::test]
async fn default_experiment_is_created_once_and_reused() {
    let Some((registry, _pool)) = database_registry().await else { return };
    let (a, b) = tokio::join!(registry.default_experiment(None), registry.default_experiment(None));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    let experiments = registry.experiments().await.unwrap();
    assert_eq!(experiments.len(), 1);
    assert_eq!(experiments[0].name, "Unsorted");
    assert_eq!(experiments[0].tags, ["default"]);
}

#[tokio::test]
async fn runs_page_by_cursor_and_filters_by_status() {
    let Some((registry, _pool)) = database_registry().await else { return };
    let experiment = registry.create_experiment("paging", "", &[], "", None).await.unwrap();
    for hex in ['1', '2', '3', '4', '5'] {
        registry.record_run(&sample_input(&experiment, hex), None).await.unwrap();
    }
    let failed = registry.create_queued_run(&sample_input(&experiment, '6'), None, None).await.unwrap();
    registry.fail_run(failed, "boom").await.unwrap();

    let filter = RunFilter::default();
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let page = RunPage { cursor: cursor.clone(), limit: 2, ..RunPage::default() };
        let (items, next) = registry.runs_page(&filter, &page).await.unwrap();
        assert!(items.len() <= 2);
        seen.extend(items.into_iter().map(|r| r.id));
        if next.is_none() {
            break;
        }
        cursor = next;
    }
    assert_eq!(seen.len(), 6);
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 6, "no run appears on two pages");

    let page = RunPage { status: Some("failed".into()), limit: 50, ..RunPage::default() };
    let (failed_runs, _) = registry.runs_page(&filter, &page).await.unwrap();
    assert_eq!(failed_runs.len(), 1);
    assert_eq!(failed_runs[0].error, "boom");
    assert!(registry
        .runs_page(&filter, &RunPage { status: Some("bogus".into()), limit: 5, ..RunPage::default() })
        .await
        .is_err());
    assert!(registry
        .runs_page(&filter, &RunPage { cursor: Some("garbage".into()), limit: 5, ..RunPage::default() })
        .await
        .is_err());

    let counts = registry.run_counts().await.unwrap();
    assert_eq!(counts[&experiment], (6, 5));
    assert_eq!(registry.trial_count(None).await.unwrap(), 5, "failed runs are not trials");
}

#[tokio::test]
async fn concurrent_writers_keep_one_valid_chain() {
    let Some((registry, _pool)) = database_registry().await else { return };
    let mut tasks = Vec::new();
    for i in 0..16 {
        let registry = registry.clone();
        tasks.push(tokio::spawn(async move {
            registry.create_experiment(&format!("parallel {i}"), "", &[], "", None).await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    let report = registry.verify().await.unwrap();
    assert!(report.ok, "{}", report.detail);
    assert_eq!(report.events, 16);
}

#[tokio::test]
async fn finished_runs_cannot_be_changed() {
    let Some((registry, pool)) = database_registry().await else { return };
    let experiment = registry.create_experiment("immutable", "", &[], "", None).await.unwrap();
    let run = registry.record_run(&sample_input(&experiment, '9'), None).await.unwrap();
    let edit = sqlx::query("UPDATE runs SET result_sha256 = $3 WHERE workspace_id = $1 AND registry_id = $2")
        .bind(registry.workspace_id())
        .bind(&run)
        .bind("0".repeat(64))
        .execute(&pool)
        .await;
    assert!(edit.unwrap_err().to_string().contains("finished runs are immutable"));
    assert!(registry.fail_run(run.parse().unwrap(), "late").await.is_err());
}

#[tokio::test]
async fn deleting_a_workspace_in_the_dashboard_removes_its_research() {
    let Some((registry, pool)) = database_registry().await else { return };
    let experiment = registry.create_experiment("to delete", "", &["x"], "", None).await.unwrap();
    let input = sample_input(&experiment, '7');
    let job = spectra_db::jobs::NewJob {
        workspace_id: registry.workspace_id(),
        kind: "backtest".into(),
        payload: "{}".parse::<sqlx::types::JsonValue>().unwrap(),
        priority: 0,
        max_attempts: 1,
        created_by: None,
    };
    let (_job, run_id) = registry.enqueue_run(&input, &job).await.unwrap();
    registry.start_run(run_id).await.unwrap();
    let artifact = StoredArtifact {
        kind: "report".into(),
        sha256: "f".repeat(64),
        uri: "sha256/ff/ff/z".into(),
        size_bytes: 3,
        content_type: "application/json".into(),
        summary: Json::object(),
    };
    let run = registry.complete_run_with_artifacts(run_id, &input, &[artifact]).await.unwrap();
    registry.add_note(&run, "kept until the workspace goes", None).await.unwrap();
    registry.add_note(&experiment, "experiment note", None).await.unwrap();
    assert!(registry.verify().await.unwrap().events >= 5);

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(registry.workspace_id())
        .execute(&pool)
        .await
        .unwrap();
    for table in
        ["registry_events", "runs", "run_artifacts", "run_notes", "experiment_notes", "experiments", "jobs"]
    {
        let left: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE workspace_id = $1"))
            .bind(registry.workspace_id())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "{table} kept rows of a deleted workspace");
    }
}
