-- M1b task 9: resumable uploads. A session receives a file in chunks, in order; the bytes go to
-- a staging file named after the session id (never stored here). Completing it moves the file
-- into the artifact store and records an `uploads` row, which the session then points at, so a
-- retried completion returns the same upload.
CREATE TABLE upload_sessions (
    id             uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id   uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    created_by     uuid REFERENCES users (id) ON DELETE SET NULL,
    filename       text NOT NULL DEFAULT '' CHECK (length(filename) <= 255),
    size_bytes     bigint NOT NULL CHECK (size_bytes > 0),
    received_bytes bigint NOT NULL DEFAULT 0 CHECK (received_bytes >= 0 AND received_bytes <= size_bytes),
    upload_id      uuid,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    -- Pushed back by every chunk; an abandoned session is removed after this.
    expires_at     timestamptz NOT NULL,
    UNIQUE (workspace_id, id),
    FOREIGN KEY (workspace_id, upload_id) REFERENCES uploads (workspace_id, id) ON DELETE SET NULL (upload_id)
);
CREATE INDEX upload_sessions_expiry ON upload_sessions (expires_at);
CREATE INDEX upload_sessions_open ON upload_sessions (workspace_id) WHERE upload_id IS NULL;
