# Step 3: Production DB Role Per Service; CI Green on Main (M20)

## Context & Ownership
- **Modules**: M20
- **Owner**: Ops
- **Size**: Small to medium

## Summary
Enforce least-privilege PostgreSQL roles across services (`spectra_api`, `spectra_worker`, `spectra_migrator`) and ensure CI test workflows are consistently green without linker bus errors or test flakes.

## Implementation & Verification Checklist
- [ ] Enforce isolated database roles using `scripts/setup-db-roles.sh` and `spectra-db provision-roles`:
  - `spectra_migrator`: Schema migrations and DDL only.
  - `spectra_api`: Application runtime (read-only users/workspaces, read-write engine tables).
  - `spectra_worker`: Background job runner (read-write jobs, artifacts, executions).
  - `spectra_backup`: Read-only `SELECT` for backup dumps.
- [ ] Maintain `CARGO_INCREMENTAL=0` and adjusted debuginfo levels in `.github/workflows/ci.yml` to prevent linker bus errors during test jobs.
- [ ] Verify full CI workflow runs green on `main` across lint, clippy, unit, integration, and container smoke checks.
