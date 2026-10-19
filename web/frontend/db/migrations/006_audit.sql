-- M19 task 9: append-only, hash-chained audit log.
--
-- Append-only: UPDATE, DELETE and TRUNCATE are refused by triggers, whoever runs them (the app's
-- database user included). Removing old rows later (retention) needs a deliberate migration that
-- drops the guard first.
--
-- Tamper-evident: each row stores hash = sha256(previous row's hash || canonical row content),
-- computed here in the database. Editing or deleting any past row (e.g. by someone who disables the
-- triggers) breaks the chain from that row on; `npm run audit:verify` and the admin page check it.
--
-- No foreign keys: events outlive the users and workspaces they mention. actor_email is a snapshot.

CREATE TABLE audit_events (
  id           bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  -- Position in the hash chain (assigned under a lock, so it is the true insertion order).
  seq          bigint      NOT NULL UNIQUE,
  occurred_at  timestamptz NOT NULL DEFAULT now(),
  actor_id     uuid,
  actor_email  text        CHECK (length(actor_email) <= 320),
  workspace_id uuid,
  action       text        NOT NULL CHECK (action ~ '^[a-z0-9_]+(\.[a-z0-9_]+)+$'),
  outcome      text        NOT NULL DEFAULT 'success' CHECK (outcome IN ('success', 'failure', 'denied')),
  target_type  text,
  target_id    text,
  ip           text,
  user_agent   text        CHECK (length(user_agent) <= 512),
  request_id   text,
  details      jsonb       NOT NULL DEFAULT '{}'::jsonb,
  prev_hash    bytea       NOT NULL,
  hash         bytea       NOT NULL
);

CREATE INDEX audit_events_occurred_at_idx ON audit_events (occurred_at DESC);
CREATE INDEX audit_events_workspace_idx   ON audit_events (workspace_id, occurred_at DESC) WHERE workspace_id IS NOT NULL;
CREATE INDEX audit_events_actor_idx       ON audit_events (actor_id, occurred_at DESC) WHERE actor_id IS NOT NULL;
CREATE INDEX audit_events_action_idx      ON audit_events (action, occurred_at DESC);

-- The exact bytes that are hashed for a row (shared by the insert trigger and verification).
CREATE FUNCTION audit_event_content(e audit_events) RETURNS bytea LANGUAGE sql IMMUTABLE AS $$
  SELECT convert_to(jsonb_build_array(
    e.seq, (extract(epoch FROM e.occurred_at) * 1000000)::bigint, e.actor_id, e.actor_email, e.workspace_id,
    e.action, e.outcome, e.target_type, e.target_id, e.ip, e.user_agent, e.request_id, e.details
  )::text, 'UTF8')
$$;

CREATE FUNCTION audit_events_chain() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  last audit_events%ROWTYPE;
BEGIN
  -- One writer at a time extends the chain (held until this transaction ends).
  PERFORM pg_advisory_xact_lock(7312090);
  SELECT * INTO last FROM audit_events ORDER BY seq DESC LIMIT 1;
  NEW.seq := coalesce(last.seq, 0) + 1;
  NEW.occurred_at := now();
  NEW.prev_hash := coalesce(last.hash, decode(repeat('00', 32), 'hex'));
  NEW.hash := sha256(NEW.prev_hash || audit_event_content(NEW));
  RETURN NEW;
END $$;

CREATE TRIGGER audit_events_chain BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION audit_events_chain();

CREATE FUNCTION audit_events_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'audit_events is append-only (% refused)', TG_OP USING ERRCODE = 'insufficient_privilege';
END $$;

CREATE TRIGGER audit_events_no_update   BEFORE UPDATE ON audit_events FOR EACH ROW EXECUTE FUNCTION audit_events_append_only();
CREATE TRIGGER audit_events_no_delete   BEFORE DELETE ON audit_events FOR EACH ROW EXECUTE FUNCTION audit_events_append_only();
CREATE TRIGGER audit_events_no_truncate BEFORE TRUNCATE ON audit_events FOR EACH STATEMENT EXECUTE FUNCTION audit_events_append_only();

-- Rows where the chain is broken (by seq); no rows means the chain is intact. Note: removing the
-- newest rows can't be detected from the table alone; compare the head hash with one kept elsewhere
-- (the admin page shows it, and CSV exports include it).
CREATE FUNCTION audit_chain_breaks() RETURNS TABLE (seq bigint, problem text) LANGUAGE sql STABLE AS $$
  SELECT seq, CASE WHEN bad_link THEN 'previous row missing or altered' ELSE 'row content altered' END
    FROM (
      SELECT e.seq,
             e.prev_hash IS DISTINCT FROM coalesce(lag(e.hash) OVER (ORDER BY e.seq), decode(repeat('00', 32), 'hex')) AS bad_link,
             e.hash IS DISTINCT FROM sha256(e.prev_hash || audit_event_content(e)) AS bad_content
        FROM audit_events e
    ) x
   WHERE bad_link OR bad_content
   ORDER BY seq
$$;
