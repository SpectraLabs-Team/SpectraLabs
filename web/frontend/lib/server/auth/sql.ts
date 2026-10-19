/**
 * Every statement the Postgres auth backend runs, in one place so they can be checked against
 * the schema (crates/spectra-db/migrations) with psql: see crates/spectra-db/tests/auth_sql_checks.sql.
 */

const USER_COLUMNS = `u.id::text AS id, u.email, u.name, u.password_hash, u.role, u.two_factor,
  (extract(epoch FROM u.created_at) * 1000)::float8 AS created_at_ms,
  (extract(epoch FROM u.password_changed_at) * 1000)::float8 AS password_changed_at_ms,
  (SELECT m.workspace_id::text FROM workspace_members m
    WHERE m.user_id = u.id ORDER BY (m.role = 'owner') DESC, m.created_at LIMIT 1) AS workspace_id`;

/** $1 email */
export const FIND_USER = `SELECT ${USER_COLUMNS} FROM users u WHERE u.email = lower(btrim($1))`;

export const LIST_USERS = `SELECT ${USER_COLUMNS} FROM users u ORDER BY u.created_at DESC, u.id DESC`;

/**
 * Creates the account, a personal workspace and the owner membership in one statement.
 * $1 email, $2 name, $3 password hash, $4 role, $5 two_factor. Returns no row if the email exists.
 */
export const INSERT_USER = `WITH new_user AS (
    INSERT INTO users (email, name, password_hash, role, two_factor)
    VALUES (lower(btrim($1)), $2, $3, $4, $5)
    ON CONFLICT (email) DO NOTHING
    RETURNING id, name
  ), new_ws AS (
    INSERT INTO workspaces (slug, name, created_by)
    SELECT 'ws-' || right(replace(id::text, '-', ''), 12),
           CASE WHEN btrim(name) = '' THEN 'Personal' ELSE btrim(name) || '''s workspace' END,
           id
      FROM new_user
    RETURNING id, created_by
  )
  INSERT INTO workspace_members (workspace_id, user_id, role)
  SELECT id, created_by, 'owner' FROM new_ws
  RETURNING user_id::text AS id`;

/** $1 email, $2 password hash or null, $3 password_changed_at ms or null, $4 two_factor or null. */
export const UPDATE_USER = `UPDATE users SET
    password_hash = COALESCE($2, password_hash),
    password_changed_at = COALESCE(to_timestamp($3::float8 / 1000), password_changed_at),
    two_factor = COALESCE($4, two_factor)
  WHERE email = lower(btrim($1))
  RETURNING id`;

/** $1 kind, $2 email, $3 code hash, $4 password hash, $5 name, $6 two_factor, $7 expires_at ms. */
export const PUT_CODE = `INSERT INTO auth_codes (kind, email, code_hash, password_hash, name, two_factor, attempts, expires_at)
  VALUES ($1, lower(btrim($2)), $3, $4, $5, $6, 0, to_timestamp($7::float8 / 1000))
  ON CONFLICT (kind, email) DO UPDATE SET
    code_hash = EXCLUDED.code_hash, password_hash = EXCLUDED.password_hash, name = EXCLUDED.name,
    two_factor = EXCLUDED.two_factor, attempts = 0, expires_at = EXCLUDED.expires_at, created_at = now()`;

const CODE_COLUMNS = `code_hash, password_hash, name, two_factor, attempts,
  (extract(epoch FROM expires_at) * 1000)::float8 AS expires_at_ms, expires_at <= now() AS expired`;

/** $1 kind, $2 email. Locks the row for the rest of the transaction. */
export const LOCK_CODE = `SELECT ${CODE_COLUMNS} FROM auth_codes WHERE kind = $1 AND email = lower(btrim($2)) FOR UPDATE`;

/** $1 kind, $2 email. */
export const PEEK_CODE = `SELECT ${CODE_COLUMNS} FROM auth_codes WHERE kind = $1 AND email = lower(btrim($2))`;

/** $1 kind, $2 email. */
export const DELETE_CODE = `DELETE FROM auth_codes WHERE kind = $1 AND email = lower(btrim($2))`;

/** $1 kind, $2 email. */
export const BUMP_ATTEMPTS = `UPDATE auth_codes SET attempts = attempts + 1 WHERE kind = $1 AND email = lower(btrim($2))`;

/** Housekeeping: drop expired codes. */
export const PURGE_EXPIRED_CODES = `DELETE FROM auth_codes WHERE expires_at <= now()`;
