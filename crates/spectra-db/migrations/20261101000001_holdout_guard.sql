-- M4a: the database is the holdout gate.
--
-- A dataset version's checksum is the key to its bars in the dataset store. From this migration
-- on, the queries the services use never hand out the checksum of a sealed holdout:
--
--   * resolve_dataset_version() returns a sealed version with a NULL checksum and the holdout's
--     name, and logs the refused attempt in holdout_access_log.
--   * grant_holdout_evaluation() is the only way to get a sealed checksum. In one statement it
--     consumes one unit of budget (only if any is left), logs the grant, and then returns it.
--     The budget row lock serialises concurrent evaluations, so a holdout can never be read more
--     often than its budget allows, and every read is logged before the bars are loaded.
--
-- holdout_access_log is append-only (rows cannot be changed), and a holdout's budget can only be
-- spent, never refunded or pointed at different data.

CREATE TABLE IF NOT EXISTS holdout_access_log (
    id             UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id   UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    holdout_id     UUID NOT NULL REFERENCES holdout_vault(id) ON DELETE CASCADE,
    -- denied: a direct read was refused; granted: an evaluation was allowed to read the data;
    -- exhausted: an evaluation was refused because the budget is spent.
    action         TEXT NOT NULL CHECK (action IN ('denied', 'granted', 'exhausted')),
    -- What asked: 'bars', 'export', 'derive', 'backtest', 'job', 'sweep', 'walkforward',
    -- 'evaluate', 'seal', 'worker', ...
    purpose        TEXT NOT NULL,
    evaluation_num INT,
    -- No foreign key: the audit trail outlives the user, and engine callers have no user.
    user_id        UUID,
    at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_holdout_access_log ON holdout_access_log(holdout_id, at DESC);

CREATE OR REPLACE FUNCTION holdout_access_log_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'holdout_access_log is append-only' USING ERRCODE = 'insufficient_privilege';
END $$;

DROP TRIGGER IF EXISTS holdout_access_log_no_update ON holdout_access_log;
CREATE TRIGGER holdout_access_log_no_update BEFORE UPDATE ON holdout_access_log
    FOR EACH ROW EXECUTE FUNCTION holdout_access_log_append_only();

CREATE OR REPLACE FUNCTION holdout_vault_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.evaluations_used < OLD.evaluations_used THEN
        RAISE EXCEPTION 'a holdout''s used evaluations cannot be refunded' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF NEW.dataset_version_id <> OLD.dataset_version_id OR NEW.workspace_id <> OLD.workspace_id THEN
        RAISE EXCEPTION 'a sealed holdout cannot be moved to other data' USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS holdout_vault_guard ON holdout_vault;
CREATE TRIGGER holdout_vault_guard BEFORE UPDATE ON holdout_vault
    FOR EACH ROW EXECUTE FUNCTION holdout_vault_guard();

-- The dataset version p_id of workspace p_ws, as a job needs it. For a sealed holdout the
-- checksum and blob are NULL, sealed_holdout names the holdout, and the attempt is logged.
CREATE OR REPLACE FUNCTION resolve_dataset_version(p_ws uuid, p_id uuid, p_purpose text, p_user uuid)
RETURNS TABLE (
    id uuid, checksum text, row_count bigint, dataset_name text, version integer,
    blob_sha256 text, sealed_holdout text
) LANGUAGE plpgsql AS $$
#variable_conflict use_column
DECLARE
    h_id   uuid;
    h_name text;
BEGIN
    SELECT hv.id, hv.name INTO h_id, h_name
      FROM holdout_vault hv
     WHERE hv.workspace_id = p_ws AND hv.dataset_version_id = p_id
     LIMIT 1;
    IF h_id IS NOT NULL THEN
        INSERT INTO holdout_access_log (workspace_id, holdout_id, action, purpose, user_id)
        VALUES (p_ws, h_id, 'denied', p_purpose, p_user);
        RETURN QUERY
            SELECT v.id, NULL::text, v.row_count, d.name::text, v.version, NULL::text, h_name
              FROM dataset_versions v
              JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
             WHERE v.workspace_id = p_ws AND v.id = p_id;
        RETURN;
    END IF;
    RETURN QUERY
        SELECT v.id, v.checksum::text, v.row_count, d.name::text, v.version, v.blob_sha256::text, NULL::text
          FROM dataset_versions v
          JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
         WHERE v.workspace_id = p_ws AND v.id = p_id;
END $$;

-- Spends one evaluation of holdout p_holdout and returns its data. No row: the holdout does not
-- exist in p_ws, or its budget is spent (logged as 'exhausted').
CREATE OR REPLACE FUNCTION grant_holdout_evaluation(p_ws uuid, p_holdout uuid, p_purpose text, p_user uuid)
RETURNS TABLE (
    evaluation_num integer, budget integer, dataset_version_id uuid, checksum text, row_count bigint,
    dataset_name text, version integer, blob_sha256 text
) LANGUAGE plpgsql AS $$
#variable_conflict use_column
DECLARE
    used     integer;
    h_budget integer;
    h_ver    uuid;
BEGIN
    UPDATE holdout_vault hv
       SET evaluations_used = hv.evaluations_used + 1
     WHERE hv.workspace_id = p_ws AND hv.id = p_holdout AND hv.evaluations_used < hv.budget
    RETURNING hv.evaluations_used, hv.budget, hv.dataset_version_id INTO used, h_budget, h_ver;
    IF used IS NULL THEN
        INSERT INTO holdout_access_log (workspace_id, holdout_id, action, purpose, user_id)
        SELECT p_ws, hv.id, 'exhausted', p_purpose, p_user
          FROM holdout_vault hv WHERE hv.workspace_id = p_ws AND hv.id = p_holdout;
        RETURN;
    END IF;
    INSERT INTO holdout_access_log (workspace_id, holdout_id, action, purpose, evaluation_num, user_id)
    VALUES (p_ws, p_holdout, 'granted', p_purpose, used, p_user);
    RETURN QUERY
        SELECT used, h_budget, v.id, v.checksum::text, v.row_count, d.name::text, v.version, v.blob_sha256::text
          FROM dataset_versions v
          JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
         WHERE v.workspace_id = p_ws AND v.id = h_ver;
END $$;
