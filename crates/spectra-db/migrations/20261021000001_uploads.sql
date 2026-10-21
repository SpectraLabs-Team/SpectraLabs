-- M1 Data Studio, tasks 4-5: file uploads, datasets stored with the artifacts, and one storage
-- figure per workspace.

-- A file uploaded through the Studio, waiting to be ingested (or kept as the source of the
-- dataset version it became). The bytes live in the artifact store under `sha256`.
CREATE TABLE uploads (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    sha256       sha256_hex NOT NULL,
    size_bytes   bigint NOT NULL CHECK (size_bytes >= 0),
    -- As the browser named it; for display only (never a path).
    filename     text NOT NULL DEFAULT '' CHECK (length(filename) <= 255),
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    -- Uploads that are never ingested are removed after this (24 hours by default).
    expires_at   timestamptz NOT NULL,
    ingested_at  timestamptz,
    UNIQUE (workspace_id, id)
);
CREATE INDEX uploads_expiry ON uploads (expires_at) WHERE ingested_at IS NULL;
CREATE INDEX uploads_sha ON uploads (sha256);

-- Where the version's bars.qfb lives in the artifact store (local volume now, S3 later), so
-- every worker can read it. NULL for versions registered from the command line, which are
-- read from the worker's local dataset store as before.
ALTER TABLE dataset_versions ADD COLUMN blob_sha256 sha256_hex;
-- How the version was made from an upload: {"upload_id", "file_sha256", "filename",
-- "settings"}. NULL for command-line versions.
ALTER TABLE dataset_versions ADD COLUMN source jsonb;
CREATE INDEX dataset_versions_blob ON dataset_versions (blob_sha256) WHERE blob_sha256 IS NOT NULL;

-- Bytes a workspace keeps in the artifact store: every distinct hash it references, once,
-- whether a run artifact, an upload or a dataset's bars. The quota queries read this.
CREATE VIEW workspace_storage AS
SELECT DISTINCT ON (workspace_id, sha256) workspace_id, sha256, size_bytes
  FROM (
        SELECT workspace_id, sha256::text AS sha256, size_bytes FROM run_artifacts
        UNION ALL
        SELECT workspace_id, sha256::text, size_bytes FROM uploads
        UNION ALL
        SELECT workspace_id, blob_sha256::text, byte_size FROM dataset_versions WHERE blob_sha256 IS NOT NULL
       ) s
 ORDER BY workspace_id, sha256;
