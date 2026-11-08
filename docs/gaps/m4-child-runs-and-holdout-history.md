# M4 Optimization & Validation: Child Runs & Holdout History

## Context & Gap Description
- **Module**: M4 Optimization & Validation (Status: 100% complete)
- **Gaps Reported (Rev 48)**:
  > "Child runs for sweep trials, and holdout history on the run page. The holdout run link, status column, PSR label and succeeded-only filter are on main (7817a434).
  > Fix: Create a child run per sweep trial. Show holdout history on the run page.
  > Done when: child runs and holdout history show on a finished sweep."

## Implementation Details

1. **Child Runs per Sweep Trial (`crates/spectra-api/src/optimization_api.rs`)**:
   - `get_sweep` queries the `runs` table for all child runs linked to the sweep trials:
     ```sql
     SELECT id, job_id, kind, status, started_at, finished_at, metrics_summary
     FROM runs
     WHERE workspace_id = $1
       AND (
         job_id = $2
         OR (kind = 'sweep_trial' AND strategy_version_id IS NOT DISTINCT FROM $3)
       )
     ORDER BY started_at ASC
     ```
   - Each trial record in `trials` is enriched with its associated `child_run_id`.
   - The overall sweep detail JSON exposes `child_runs: SweepChildRun[]` along with `trials`.

2. **Holdout Vault History on Sweep & Backtest Run Pages**:
   - `get_sweep` calls `optimization::evaluations_for_run_target(pool, ws, sweep.job_id)` and returns `holdout_history: SweepHoldoutHistoryItem[]`.
   - `OptimizationLab.tsx` renders:
     - `Child runs: <count>` in the sweep summary bar.
     - A dedicated `Child Run` column in the Trials Ledger linking each trial to its backtest child run (`Run #{id.slice(0, 8)} succeeded`).
     - A `Holdout Vault History ({n} unlocks)` audit panel for finished sweeps showing holdout dataset version, holdout Sharpe ratio, DSR, total return, and unlock evaluation date.
   - `RunDetail.tsx` (Backtest Run page) fetches holdout evaluation history via `/api/v1/runs/${runId}` when backtest status is succeeded and renders the audited `Holdout Vault History` card.

## Verification Checklist
- [x] Child runs query and linking integrated into `get_sweep` API response.
- [x] Sweep detail test in `crates/spectra-api/tests/optimization_flow.rs` verifies `child_runs` and `holdout_history` arrays.
- [x] `OptimizationLab.tsx` displays child runs count, links trials to child runs, and displays holdout vault history on finished sweeps.
- [x] `RunDetail.tsx` displays holdout history card on the backtest run view.
- [x] `cargo fmt`, `cargo clippy`, `npm run lint`, and `npm run typecheck` all pass with 0 errors.
