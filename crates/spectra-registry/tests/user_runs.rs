#![cfg(feature = "postgres")]
#![allow(clippy::similar_names)]

use spectra_db::{connect, migrate, new_id, PoolConfig};
use spectra_engine::{config_fingerprint, BacktestConfig, Metrics, RunRecord};
use spectra_registry::{PgRegistry, RunFilter, RunInput};
use spectra_research::ParamSet;
use uuid::Uuid;

async fn setup() -> Option<(PgRegistry, spectra_db::PgPool, Uuid, Uuid, Uuid)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = connect(&PoolConfig::new(url)).await.expect("DATABASE_URL must be reachable");
    migrate(&pool).await.expect("migrations must apply");
    let workspace_id = new_id();
    let slug = format!("m25-runs-{}", workspace_id.simple());
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'M25 runs test')")
        .bind(workspace_id)
        .bind(slug)
        .execute(&pool)
        .await
        .expect("test workspace must be created");

    let user_a = new_id();
    let email_a = format!("user-a-{}@example.com", user_a.simple());
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
        .bind(user_a)
        .bind(email_a)
        .execute(&pool)
        .await
        .expect("user a must be created");

    let user_b = new_id();
    let email_b = format!("user-b-{}@example.com", user_b.simple());
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
        .bind(user_b)
        .bind(email_b)
        .execute(&pool)
        .await
        .expect("user b must be created");

    Some((PgRegistry::new(pool.clone(), workspace_id), pool, workspace_id, user_a, user_b))
}

fn sample_run_input(experiment_id: String, strategy: &str) -> RunInput {
    let config = BacktestConfig::default();
    RunInput {
        experiment_id,
        kind: "backtest".into(),
        dataset_id: "dataset-v1".into(),
        strategy: strategy.into(),
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
    }
}

#[tokio::test]
async fn user_list_shows_only_that_users_runs() {
    let Some((registry, _pool, _workspace_id, user_a, user_b)) = setup().await else { return };
    let exp = registry.create_experiment("exp-1", "hypothesis", &["tag"], "", Some(user_a)).await.unwrap();

    let input_a = sample_run_input(exp.clone(), "strat-a");
    let run_a = registry.record_run(&input_a, Some(user_a)).await.unwrap();

    let input_b = sample_run_input(exp.clone(), "strat-b");
    let run_b = registry.record_run(&input_b, Some(user_b)).await.unwrap();

    let user_a_runs = registry
        .runs(&RunFilter { created_by: Some(user_a.to_string()), ..RunFilter::default() })
        .await
        .unwrap();
    assert_eq!(user_a_runs.len(), 1);
    assert_eq!(user_a_runs[0].id, run_a);

    let user_b_runs = registry
        .runs(&RunFilter { created_by: Some(user_b.to_string()), ..RunFilter::default() })
        .await
        .unwrap();
    assert_eq!(user_b_runs.len(), 1);
    assert_eq!(user_b_runs[0].id, run_b);
}

#[tokio::test]
async fn hiding_removes_a_run_from_my_list_and_keeps_it() {
    let Some((registry, _pool, _workspace_id, user_a, _user_b)) = setup().await else { return };
    let exp = registry.create_experiment("exp-2", "hypothesis", &["tag"], "", Some(user_a)).await.unwrap();

    let input_1 = sample_run_input(exp.clone(), "strat-1");
    let run_1 = registry.record_run(&input_1, Some(user_a)).await.unwrap();

    let input_2 = sample_run_input(exp.clone(), "strat-2");
    let run_2 = registry.record_run(&input_2, Some(user_a)).await.unwrap();

    let runs_initial = registry
        .runs(&RunFilter {
            created_by: Some(user_a.to_string()),
            hidden_for: Some(user_a.to_string()),
            ..RunFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(runs_initial.len(), 2);

    registry.hide_run_for_user(&run_1, user_a).await.unwrap();
    registry.hide_run_for_user(&run_1, user_a).await.unwrap();

    let runs_after_hide = registry
        .runs(&RunFilter {
            created_by: Some(user_a.to_string()),
            hidden_for: Some(user_a.to_string()),
            ..RunFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(runs_after_hide.len(), 1);
    assert_eq!(runs_after_hide[0].id, run_2);

    let fetched = registry.run(&run_1).await.unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().id, run_1);

    let all_runs = registry.runs(&RunFilter::default()).await.unwrap();
    assert!(all_runs.iter().any(|r| r.id == run_1));
    assert!(all_runs.iter().any(|r| r.id == run_2));
}
