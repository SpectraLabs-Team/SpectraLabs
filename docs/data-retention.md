# Data retention: accounts, activity and security records (M21)

Owner: product owner. Status: **approved by owner for the accounts, activity and workspace periods (M28 Decisions 1, 2 and 3).** The workspace section is still to be finished for review.
Covers the personal data the account system and admin console keep, and the workspace content
described in "Workspace content" below (M28).

Users are told the short version on the Account page (`web/frontend/components/account/AccountPage.tsx`).
This document and that page must say the same thing.

## What we keep and for how long

| Data | Where | Kept for | Removed by | After the account is deleted |
|---|---|---|---|---|
| Account (email, name, role, status, settings) | `users` | Until the account is deleted | Account deletion | Gone |
| Daily active time (one row per user per UTC day: seconds, heartbeat count, last heartbeat) | `user_activity_daily` | **400 days** | The worker's purge, every six hours (`SPECTRA_ACTIVITY_PURGE`), or `npm run activity:purge` by hand | Gone immediately (`ON DELETE CASCADE`) |
| Sessions (device, IP address, sign-in and last-seen times) | `sessions` | While the session is live (at most 7 days, or 24 hours idle), then about a week | Session housekeeping (`lib/server/sessions.ts`) | Gone |
| Security and admin records (sign-ins, password and two-step changes, admin actions, admin views, with email, IP address and browser) | `audit_events` | **Permanently** | Never: triggers refuse UPDATE, DELETE and TRUNCATE, and a hash chain makes changes detectable | **Kept**, including the email as it was at the time |
| Database backups | Backup host (`docs/operations.md`) | Daily copies `BACKUP_KEEP_DAYS` (14 by default); Sunday copies `BACKUP_KEEP_WEEKLY_DAYS` (90 by default) | `scripts/backup-postgres.sh` | Deleted data can remain in backups until they expire |
| Artifact-store backups | Backup host (`scripts/backup-artifacts.sh`) | Same periods as database backups | `scripts/backup-artifacts.sh` | Deleted data can remain in backups until they expire |

We don't record the pages people view, their clicks or what they type. Only daily activity totals.

## Workspace content (M28)

Status: **approved by owner (M28 Decisions 1 and 2).** Each
row says what the code does today; anything not yet confirmed is marked "to confirm".

| Data | Where | Kept for | Removed by | After the account is deleted |
|---|---|---|---|---|
| Runs and their notes and history | `runs`, `run_notes` | Until a user or admin deletes them, or the workspace is deleted. No automatic expiry in the code. | Deleting the run or workspace | Gone with the workspace |
| Strategy drafts | `strategy_drafts` | Until the strategy or workspace is deleted. No automatic expiry in the code. | Deleting the strategy or workspace (`ON DELETE CASCADE`) | Gone with the workspace |
| Uploads not yet ingested | `uploads`, `upload_sessions` | 24 hours (`UPLOAD_TTL_SECS`, `SESSION_TTL_SECS`), then removed by the worker | Expiry | Gone |
| Ingested uploads | `uploads` | While a dataset version uses them. Deleting the version makes them expire. | Expiry after the version is deleted | Gone with the workspace |
| Artifact files (blobs in the artifact store) | Artifact store | While referenced. The worker sweeps files that nothing references, including files from deleted workspaces. | Worker garbage collection | Removed by the sweep after the workspace is deleted (swept every 30 minutes) |

Deleting an account removes its personal workspace with everything in it, and any team workspace
where it is the only member. Team workspaces with other members stay (if the user is the sole owner
of a team with other members, another owner must be appointed or other members removed first).

## Why the audit log is permanent

The audit log is the record of who signed in and what admins did. It is append-only and hash-chained
so that nobody, including an admin with database access, can quietly change or remove entries. That
is why entries can't be deleted, even when an account is. Users are told this on the Account page.

Not done yet, and possible later: storing less in new entries (for example, only part of the IP
address), or moving personal fields outside the hashed record so they can be erased. If you have
users in the EU or UK, check this policy against your legal obligations.

## Who can see what

- Admins: account details, activity figures and audit records, in the admin console. Opening a
  user, the user directory, analytics or the audit log is recorded in the audit log. IP addresses in
  a user's detail view are hidden until the admin chooses "Show IPs", which is also recorded.
- Workspace owners: their workspace's audit log, including members' IP addresses.
- Users: their own account, sessions and activity figures.

## Changing the 400 days

The number lives in four places, and `web/frontend/scripts/test-activity.mjs` fails if they differ:
`web/frontend/lib/server/activity.ts`, `web/frontend/lib/activityClient.ts` (shown on the Account
page), `crates/spectra-db/src/activity.rs` (the worker) and `web/frontend/scripts/activity-purge.mjs`.
Shortening it deletes more on the next purge run: dry-run first (see `docs/operations.md`).

## Open points for the owner

- Decided (Decision 2): runs and drafts are kept until the user or account deletes them. No automatic expiry.
- Artifact sweep: the worker sweeps unreferenced files every 30 minutes (`spectra-worker.rs`). No grace period is set. Owner to decide whether one is wanted.
- Decided (Decision 6): an unset `SPECTRA_ACTIVITY_PURGE` means off (`config.rs` and `docs/operations.md`, M28 Decision 6, merged in PR #140).

## Sign-off

- Owner: product owner (name to fill in)
- Decisions 1, 2 and 3 recorded: 2026-10-30. Sign-off date to fill in.
