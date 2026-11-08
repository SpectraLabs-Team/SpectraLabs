# Admin Runbook (M21)

This runbook guides operators and platform administrators through role gate enforcement, data purge procedures, audit log verification, and production observability verification.

---

## 1. Role Gates & RBAC Administration

Spectra enforces a two-tier role model: **Platform Roles** and **Workspace Roles**.

### Platform Roles
Platform roles govern access to system-level administration and the Admin Console (`/admin`).

| Role | Access Scope | Enforced By |
|---|---|---|
| `admin` | Full platform administrative access: `/admin`, user disable/delete, role assignment, audit logs, workspace oversight. Always requires 2FA. | Next.js middleware, Admin API role gate (`requireAdmin`) |
| `user` | Standard platform account. Access scoped strictly to workspaces where they hold membership. | Platform session token |

#### CLI Platform Role Operations
Execute from `web/frontend` with `DATABASE_URL` pointing to the application database:

```bash
# List all platform administrators
npm run admin -- list

# Grant platform admin role (automatically enforces 2FA requirement)
npm run admin -- grant operator@example.com

# Revoke platform admin role (cannot revoke the last remaining admin)
npm run admin -- revoke operator@example.com
```

### Workspace Roles
Workspace roles govern resource access, computation execution, and risk configuration inside a workspace.

| Workspace Role | Capabilities | Restrictions |
|---|---|---|
| `owner` | Full workspace control, quota adjustments, member invites, role changes, billing, destructive deletion. | None within workspace. |
| `admin` | Manage strategies, datasets, backtest sweeps, risk limits, paper trading accounts. | Cannot delete workspace or remove owner. |
| `member` | Author DSL strategies, run backtests, initiate optimization sweeps, view reports. | Cannot modify risk limits, kill switches, or workspace membership. |
| `viewer` | Read-only access to strategies, backtests, and reports. | Read-only gate: blocked by `ensure_can_edit` (`403 viewer_read_only`). Cannot place paper orders or edit configuration. |

---

## 2. Purge & Data Retention Procedures

### User Activity Purge
Under the statutory data retention policy, daily active time metrics (`user_activity_daily`) are retained for **400 days**.

#### Execution Modes:
1. **Automated Worker Purge**:
   - Background workers execute the purge routine every 6 hours under a Postgres advisory lock.
   - Configured via environment variable: `SPECTRA_ACTIVITY_PURGE=on`.
   - Metrics emitted on worker `/metrics`: `activity_purge_runs_total`, `activity_purge_rows_total`, `activity_purge_last_success_timestamp_seconds`.
2. **Manual Operator Execution**:
   ```bash
   cd web/frontend
   # 1. Always run a dry run first to inspect eligible row count:
   npm run activity:purge -- --dry-run

   # 2. Execute live purge:
   npm run activity:purge
   ```

### Unreferenced Artifact Purge
- When a workspace is deleted, its metadata and registry events are removed from Postgres.
- Content-addressed artifact bytes are swept by the worker background process every 30 minutes (`sweep_unreferenced` in `crates/spectra-db/src/artifacts.rs`).
- Any blob hash no longer referenced by any workspace is safely purged from the artifact volume / S3 bucket.

### Staging Uploads Cleanup
- Resumable file chunks staged in `<ARTIFACT_DIR>/staging` (or S3 temporary prefixes) that remain idle for >24 hours are automatically reclaimed by API maintenance routines.

---

## 3. Audit Log Views & Integrity Verification

### Audit Trail Characteristics
- Every administrative action (`admin.role.granted`, `admin.role.revoked`, `user.disable`, `user.enable`), risk control mutation, and emergency kill-switch toggle writes an append-only row to `audit_events`.
- Rows include `actor_id`, `action`, `resource_type`, `resource_id`, `details`, `client_ip`, and cryptographic linking (`prev_hash` $\to$ `event_hash`).

### Viewing Audit Logs
- Navigate to the Admin Console at `/admin/audit`.
- Filter events by actor, action category (`admin.`, `user.`, `risk.`), or timestamp range.
- The UI displays cryptographic verification badges reflecting chain consistency.

### Verifying Chain Cryptographic Integrity
Run the audit verification script from `web/frontend`:

```bash
npm run audit:verify
```
The script validates that no rows have been deleted, inserted out-of-order, or tampered with by verifying sequential SHA-256 hashes against stored heads.

---

## 4. Sentry Production Verification Runbook (M20)

To confirm error tracking and panic reporting are functional in production:

### 1. Verify Configuration
Ensure `SENTRY_DSN` is configured in production environment secrets:
```bash
docker compose exec api env | grep SENTRY_DSN
```

### 2. Check Observability Status Endpoint
```bash
curl -fsS https://<SITE_ADDRESS>/api/v1/observability/sentry
# Expected response:
# {"enabled":true,"dsn_configured":true,"environment":"production"}
```

### 3. Send Verification Test Event
Trigger a synthetic test event to Sentry:
```bash
curl -fsS -X POST https://<SITE_ADDRESS>/api/v1/observability/sentry/verify
# Expected response:
# {"status":"verified","event":"SpectraSentryVerificationTestEvent","flushed":true}
```

### 4. Verify in Sentry UI
1. Open the production Sentry project dashboard.
2. Locate the issue titled `SpectraSentryVerificationTestEvent`.
3. Confirm event tags (`environment=production`, `release=<git-sha>`).
4. Resolve or archive the test issue.
5. Record confirmation in `KNOWN_ISSUES.md`.
