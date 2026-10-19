-- Worker registry (M20 step 6): each spectra-worker upserts its row every few seconds, so the
-- API's /ready can tell whether any worker is alive.
CREATE TABLE workers (
    id           text PRIMARY KEY,
    hostname     text NOT NULL DEFAULT '',
    version      text NOT NULL DEFAULT '',
    kinds        text[] NOT NULL DEFAULT '{}',
    concurrency  integer NOT NULL DEFAULT 1 CHECK (concurrency >= 1),
    started_at   timestamptz NOT NULL DEFAULT now(),
    heartbeat_at timestamptz NOT NULL DEFAULT now()
);

-- Artifacts are content-addressed per workspace: look up by hash for dedupe and quota.
CREATE INDEX run_artifacts_workspace_sha_size ON run_artifacts (workspace_id, sha256) INCLUDE (size_bytes);
DROP INDEX run_artifacts_workspace_sha;
