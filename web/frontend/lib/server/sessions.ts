import crypto from 'crypto';
import { authSecret, toUser, USER_COLUMNS, type UserRecord, type UserRow } from './authStore';
import { query } from './db';
import { looksLikeSessionToken } from '../sessionEdge';
import { defaultWorkspaceId, getMembership, personalWorkspace, type Workspace } from './workspaces';

/**
 * Database-backed sessions (table `sessions`). Server-only.
 *
 * - The cookie holds a random 32-byte token. Only HMAC-SHA256(AUTH_SECRET, token) is stored.
 * - A session ends at the first of: 7 days after sign-in (absolute), 24 hours without a request
 *   (idle), revocation (sign-out, "sign out everywhere", password change/reset), or the account
 *   being disabled or deleted.
 * - last_seen_at is refreshed at most once a minute, so most requests are a single SELECT.
 * - Each session has an active workspace. A new session starts where the user left off (or in
 *   their personal workspace); if they lose access to it, the next request falls back to personal.
 */

export const SESSION_ABSOLUTE_MS = 7 * 24 * 60 * 60 * 1000;
export const SESSION_IDLE_MS = 24 * 60 * 60 * 1000;
export const SESSION_TOUCH_MS = 60 * 1000;

const MAX_UA = 512;

export type SessionInfo = {
  id: string;
  userId: string;
  workspaceId: string | null;
  createdAt: number;
  lastSeenAt: number;
  expiresAt: number;
  ip: string | null;
  userAgent: string | null;
  /** When this session last proved it's the owner (sign-in, password or code): see mfa.ts step-up. */
  reauthAt: number | null;
};

type SessionRow = {
  id: string;
  user_id: string;
  workspace_id: string | null;
  created_at: Date;
  last_seen_at: Date;
  expires_at: Date;
  ip: string | null;
  user_agent: string | null;
  reauth_at: Date | null;
};

const SESSION_COLUMNS = 'id, user_id, workspace_id, created_at, last_seen_at, expires_at, ip, user_agent, reauth_at';

const toSession = (r: SessionRow): SessionInfo => ({
  id: r.id,
  userId: r.user_id,
  workspaceId: r.workspace_id,
  createdAt: r.created_at.getTime(),
  lastSeenAt: r.last_seen_at.getTime(),
  expiresAt: r.expires_at.getTime(),
  ip: r.ip,
  userAgent: r.user_agent,
  reauthAt: r.reauth_at?.getTime() ?? null,
});

/** Keyed hash of a token, as stored in sessions.token_hash. */
function tokenHash(token: string): Buffer {
  return crypto.createHmac('sha256', authSecret()).update(token).digest();
}

/**
 * Start a session for an account and return the token for the cookie. Pass `replacing` (the
 * browser's current token, if any) to rotate: the old session is revoked, so a token planted in
 * the browser before sign-in (session fixation) is worthless afterwards.
 */
export async function createSession(
  user: { id: string; email: string },
  meta: { ip?: string | null; userAgent?: string | null; workspaceId?: string | null; replacing?: string | null; deviceHash?: Buffer | null } = {},
): Promise<{ token: string; session: SessionInfo }> {
  if (meta.replacing) await revokeSessionByToken(meta.replacing);
  const workspaceId = meta.workspaceId ?? (await defaultWorkspaceId(user.id, user.email));
  const token = crypto.randomBytes(32).toString('base64url');
  const [row] = await query<SessionRow>(
    `INSERT INTO sessions (token_hash, user_id, workspace_id, expires_at, ip, user_agent, reauth_at, device_hash)
     VALUES ($1, $2, $3, now() + $4 * interval '1 millisecond', $5, $6, now(), $7)
     RETURNING ${SESSION_COLUMNS}`,
    [tokenHash(token), user.id, workspaceId, SESSION_ABSOLUTE_MS, meta.ip ?? null, meta.userAgent ? meta.userAgent.slice(0, MAX_UA) : null, meta.deviceHash ?? null],
  );
  // Housekeeping: rows that ended over a week ago are no use, even for a device list.
  await query(`DELETE FROM sessions WHERE expires_at < now() - interval '7 days' OR revoked_at < now() - interval '7 days'`);
  return { token, session: toSession(row) };
}

export type Auth = { session: SessionInfo; user: UserRecord; workspace: Workspace };

/**
 * The live session, account and active workspace (with the user's role in it) for a cookie token,
 * or null. Rejects revoked, expired and idle sessions and disabled accounts. Database errors
 * propagate (a 500, not a silent sign-out).
 */
export async function lookupSession(token: string | undefined | null): Promise<Auth | null> {
  if (!looksLikeSessionToken(token)) return null;
  let hash: Buffer;
  try {
    hash = tokenHash(token);
  } catch {
    return null; // AUTH_SECRET missing: fail closed
  }
  const userCols = USER_COLUMNS.split(', ').map((c) => `u.${c} AS u_${c}`).join(', ');
  const [row] = await query<SessionRow & Record<string, unknown>>(
    `SELECT ${SESSION_COLUMNS.split(', ').map((c) => `s.${c}`).join(', ')}, ${userCols}
       FROM sessions s JOIN users u ON u.id = s.user_id
      WHERE s.token_hash = $1
        AND s.revoked_at IS NULL
        AND s.expires_at > now()
        AND s.last_seen_at > now() - $2 * interval '1 millisecond'
        AND u.status = 'active'`,
    [hash, SESSION_IDLE_MS],
  );
  if (!row) return null;

  const user = toUser(Object.fromEntries(USER_COLUMNS.split(', ').map((c) => [c, row[`u_${c}`]])) as UserRow);
  const session = toSession(row);

  // Active workspace, re-checked on every request so a removed member loses access at once.
  let workspace = session.workspaceId ? await getMembership(user.id, session.workspaceId) : null;
  if (!workspace) {
    workspace = await personalWorkspace(user.id, user.email);
    await query('UPDATE sessions SET workspace_id = $1 WHERE id = $2', [workspace.id, session.id]);
    session.workspaceId = workspace.id;
  }

  if (Date.now() - session.lastSeenAt > SESSION_TOUCH_MS) {
    await query(
      `UPDATE sessions SET last_seen_at = now() WHERE id = $1 AND last_seen_at < now() - $2 * interval '1 millisecond'`,
      [session.id, SESSION_TOUCH_MS],
    );
  }
  return { session, user, workspace };
}

/** Sign out one browser (by its cookie token). */
export async function revokeSessionByToken(token: string): Promise<void> {
  if (!looksLikeSessionToken(token)) return;
  await query('UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL', [tokenHash(token)]);
}

/** Sign out one session of this account (device list). Returns false if it wasn't found or already ended. */
export async function revokeSessionById(userId: string, sessionId: string): Promise<boolean> {
  const rows = await query('UPDATE sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL RETURNING id', [sessionId, userId]);
  return rows.length > 0;
}

/** Sign out every session of an account, optionally keeping one (the current browser). Returns how many ended. */
export async function revokeAllSessions(userId: string, opts: { except?: string } = {}): Promise<number> {
  const rows = await query(
    'UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL AND ($2::uuid IS NULL OR id <> $2) RETURNING id',
    [userId, opts.except ?? null],
  );
  return rows.length;
}

/** Live sessions of an account, most recently used first, with when their device was first seen (devices list). */
export async function listSessions(userId: string): Promise<(SessionInfo & { deviceFirstSeenAt: number | null })[]> {
  const rows = await query<SessionRow & { device_first_seen_at: Date | null }>(
    `SELECT ${SESSION_COLUMNS.split(', ').map((c) => `s.${c}`).join(', ')}, d.first_seen_at AS device_first_seen_at
       FROM sessions s LEFT JOIN known_devices d ON d.user_id = s.user_id AND d.device_hash = s.device_hash
      WHERE s.user_id = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND s.last_seen_at > now() - $2 * interval '1 millisecond'
      ORDER BY s.last_seen_at DESC`,
    [userId, SESSION_IDLE_MS],
  );
  return rows.map((r) => ({ ...toSession(r), deviceFirstSeenAt: r.device_first_seen_at?.getTime() ?? null }));
}
