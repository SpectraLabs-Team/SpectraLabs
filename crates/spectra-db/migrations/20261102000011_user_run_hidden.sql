-- M25 Task 9: per-user run visibility.
-- Allows users to hide runs from their personal "My runs" list without
-- modifying the immutable run record or affecting other workspace members.

CREATE TABLE user_run_hidden (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    run_id       uuid NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id, run_id),
    FOREIGN KEY (workspace_id, run_id) REFERENCES runs (workspace_id, id) ON DELETE CASCADE
);

CREATE INDEX user_run_hidden_workspace_user ON user_run_hidden (workspace_id, user_id);
