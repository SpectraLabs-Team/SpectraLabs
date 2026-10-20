# Engine API conventions

These apply to every `/api/v1` route of `spectra-api`. The OpenAPI description is served at
`GET /api/v1/openapi.json`.

## Who can call it

Only the dashboard's server. The browser calls `/api/engine/...` on the dashboard, which checks the
session and forwards an allowlisted set of routes to the engine, signed with HMAC-SHA256 under the
shared `ENGINE_SECRET`:

| header | |
|---|---|
| `X-Spectra-User` | the signed-in user's id |
| `X-Spectra-Workspace` | their active workspace's id (empty if none) |
| `X-Request-Id` | a UUID made by the proxy |
| `X-Spectra-Timestamp` | Unix seconds |
| `X-Spectra-Signature` | hex HMAC of the canonical string below |

```text
spectra-v1\n<METHOD>\n<path and query>\n<timestamp>\n<user>\n<workspace>\n<request id>\n<hex SHA-256 of the body>
```

The engine refuses unsigned or altered requests, and signatures more than 60 seconds from its clock
(401, `unauthenticated`). Both sides pin the same test vector (`auth.rs` and
`scripts/test-auth.mjs`). `/health`, `/ready` and `/api/v1/openapi.json` need no signature. Bind the
engine to 127.0.0.1 or a private network only.

## Database query mode

The service uses SQLx runtime queries (`sqlx::query` / `query_as`) rather than
compile-time `query!` macros. This keeps builds independent of a live database
and avoids committing SQLx metadata for every migration state. Statements are
kept in `crates/spectra-db/queries/` and included by the Rust code; migrations
define their schema. CI applies migrations to PostgreSQL 16 and executes the
schema and queue SQL checks against that database. When a query changes, update
the corresponding SQL check so CI validates the statement and its expected
behavior.

## Errors

Every error has one shape, whatever produced it:

```json
{ "code": "invalid_request", "message": "Simulations must be between 1,000 and 100,000,000.",
  "details": null, "request_id": "0192…", "detail": "Simulations must be …" }
```

`code` is for programs (`invalid_request`, `unauthenticated`, `not_found`, `method_not_allowed`,
`payload_too_large`, `unavailable`, `no_workspace`, …), `message` for people. `detail` is a
deprecated copy of `message`, removed together with the old `/api/...` route aliases.

Validation errors that concern several fields list them all at once in `details.errors`, each with
a dotted field path (the Data Studio routes; see [datasets.md](datasets.md)):

```json
{ "code": "invalid_request", "message": "settings.delimiter: ...; settings.columns.close: required",
  "details": { "errors": [ { "field": "settings.columns.close", "message": "required" } ] }, ... }
```

Other errors keep `details: null`.

## Request ids

Each response carries `X-Request-Id`: the proxy's id when it sent a valid one (1–128 characters of
letters, digits, `-_.:`), otherwise a new UUIDv7. Error bodies repeat it as `request_id`. Quote it
when reporting a problem.

## Ids, timestamps, pagination

- **Ids** are UUIDv7: time-ordered, so sorting by id sorts by creation time.
- **Timestamps** are UTC, RFC 3339 (`2026-10-19T09:15:02.123Z`).
- **Lists** return `{ "items": [...], "next_cursor": "<id>" | null }`, newest first. Pass
  `cursor=<next_cursor>` for the next page and `limit` (1–200, default 50) for its size. Cursors are
  opaque; do not build them yourself.

## Versions

Breaking changes get a new prefix (`/api/v2`). The unversioned `/api/...` paths are temporary
aliases of `/api/v1/...` for the old dashboard and will be removed.
