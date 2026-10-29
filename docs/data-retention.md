# Data retention: accounts, activity and security records (M21)

Owner: product owner. Status: **draft, awaiting the owner's sign-off.**
Covers the personal data the account system and admin console keep. Workspace content (datasets,
strategies, runs) is out of scope.

Users are told the short version on the Account page (`web/frontend/components/account/AccountPage.tsx`).
This document and that page must say the same thing.

## What we keep and for how long

| Data | Where | Kept for | Removed by | After the account is deleted |
|---|---|---|---|---|
| Account (email, name, role, status, settings) | `users` | Until the account is deleted | Account deletion | Gone |
| Daily active time (one row per user per UTC day: seconds, heartbeat count, last heartbeat) | `user_activity_daily` | **400 days** | The worker's purge, every six hours (`SPECTRA_ACTIVITY_PURGE`), or `npm run activity:purge` by hand | Gone immediately (`ON DELETE CASCADE`) |
| Sessions (device, IP address, sign-in and last-seen times) | `sessions` | While the session is live (at most 7 days, or 24 hours idle), then about a week | Session housekeeping (`lib/server/sessions.ts`) | Gone |
| Security and admin records (sign-ins, password and two-step changes, admin actions, admin views, with email, IP address and browser) | `audit_events` | **Permanently** | Never: triggers refuse UPDATE, DELETE and TRUNCATE, and a hash chain makes changes detectable | **Kept**, including the email as it was at the time |
| Database backups | Backup host (`docs/operations.md`) | `BACKUP_KEEP_DAYS` (14 in the example) | `scripts/backup-postgres.sh` | Deleted data can remain in backups until they expire |

We don't record the pages people view, their clicks or what they type. Only daily activity totals.

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

## Sign-off

- Owner: _name_
- Date: _to fill in_
