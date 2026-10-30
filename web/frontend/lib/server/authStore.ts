import crypto from 'crypto';
import { hash as argon2Hash, verify as argon2Verify } from '@node-rs/argon2';
import { record as audit } from './audit';
import { query, transaction } from './db';
import { createPersonalWorkspace, releaseWorkspacesOf } from './workspaces';

/**
 * Seeds the first platform admin only: an account signing up with this email becomes admin while
 * no admin exists yet. After that the role lives in the database (users.platform_role) and is
 * changed with `npm run admin -- grant|revoke <email>`. Unset = no automatic admin.
 */
export const adminSeedEmail = () => (process.env.ADMIN_EMAIL || '').trim().toLowerCase();

// ── AUTH_SECRET: required, no fallback ──
// Keep these rules in step with lib/sessionEdge.ts (Edge middleware) and next.config.js (startup check).
const MIN_AUTH_SECRET_LENGTH = 32;
/** Former hard-coded development default; it is public, so it must never sign anything. */
const RETIRED_SECRETS = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

/** Server secret (keys the session-token hash). Throws if AUTH_SECRET is missing, short or a retired default. */
export function authSecret(): string {
  const s = process.env.AUTH_SECRET;
  let problem: string | null = null;
  if (!s || !s.trim()) problem = 'AUTH_SECRET is not set';
  else if (s.length < MIN_AUTH_SECRET_LENGTH) problem = `AUTH_SECRET is too short (need ${MIN_AUTH_SECRET_LENGTH}+ characters)`;
  else if (RETIRED_SECRETS.has(s)) problem = 'AUTH_SECRET is the old public development default';
  if (problem) throw new Error(`${problem}. Run \`npm run setup\` in web/frontend to generate one.`);
  return s as string;
}


/**
 * Auth data lives in Postgres (tables `users` and `email_codes`, see db/migrations). Every
 * function that touches the database is async; callers `await` them. Emails are always stored
 * and looked up trimmed and lower-cased.
 */

export type UserRecord = {
  id: string;
  email: string;
  name?: string;
  passwordHash: string;
  role: 'admin' | 'user';
  /** 'disabled' accounts can't sign in and their sessions stop working. */
  status: 'active' | 'disabled';
  createdAt: number;
  /** ms epoch of the last password change or reset. */
  passwordChangedAt?: number;
  /** Ask for an emailed code at every login (in addition to the password). */
  twoFactor?: boolean;
};

export type UserRow = {
  id: string;
  email: string;
  name: string;
  password_hash: string;
  platform_role: 'admin' | 'user';
  status: 'active' | 'disabled';
  two_factor: boolean;
  created_at: Date;
  password_changed_at: Date | null;
};

export const USER_COLUMNS = 'id, email, name, password_hash, platform_role, status, two_factor, created_at, password_changed_at';

export function toUser(r: UserRow): UserRecord {
  return {
    id: r.id,
    email: r.email,
    name: r.name,
    passwordHash: r.password_hash,
    role: r.platform_role,
    status: r.status,
    twoFactor: r.two_factor,
    createdAt: r.created_at.getTime(),
    passwordChangedAt: r.password_changed_at?.getTime(),
  };
}

const normEmail = (email: string) => email.trim().toLowerCase();

// ── Password Hashing (Argon2id with OWASP recommendations, fallback for legacy PBKDF2) ──
export const ARGON2_PARAMS = {
  memoryCost: 19456, // 19 MiB in KiB
  timeCost: 2,
  parallelism: 1,
  algorithm: 2, // Algorithm.Argon2id
} as const;

export async function hashPassword(password: string): Promise<string> {
  if (typeof password !== 'string' || password.length > 256) {
    throw new Error('Password must not exceed 256 characters.');
  }
  return argon2Hash(password, {
    memoryCost: ARGON2_PARAMS.memoryCost,
    timeCost: ARGON2_PARAMS.timeCost,
    parallelism: ARGON2_PARAMS.parallelism,
    algorithm: ARGON2_PARAMS.algorithm,
  });
}

export async function verifyPassword(
  password: string,
  stored: string,
): Promise<{ ok: boolean; needsRehash: boolean }> {
  if (typeof password !== 'string' || typeof stored !== 'string' || password.length > 256 || !stored) {
    return { ok: false, needsRehash: false };
  }

  try {
    // 1. Argon2id PHC string ($argon2id$v=19$m=19456,t=2,p=1$...)
    if (stored.startsWith('$argon2id$')) {
      const match = stored.match(/^\$argon2id\$v=\d+\$m=(\d+),t=(\d+),p=(\d+)\$/);
      let needsRehash = true;
      if (match) {
        const m = parseInt(match[1], 10);
        const t = parseInt(match[2], 10);
        const p = parseInt(match[3], 10);
        needsRehash =
          m !== ARGON2_PARAMS.memoryCost ||
          t !== ARGON2_PARAMS.timeCost ||
          p !== ARGON2_PARAMS.parallelism;
      }
      const ok = await argon2Verify(stored, password);
      return { ok, needsRehash: ok ? needsRehash : false };
    }

    // 2. Legacy PBKDF2 string ("saltHex:hashHex")
    const parts = stored.split(':');
    if (parts.length === 2) {
      const [salt, originalHash] = parts;
      if (salt && originalHash && /^[0-9a-f]+$/i.test(salt) && /^[0-9a-f]+$/i.test(originalHash)) {
        const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512');
        const expected = Buffer.from(originalHash, 'hex');
        const ok = expected.length === hash.length && crypto.timingSafeEqual(hash, expected);
        return { ok, needsRehash: ok };
      }
    }

    return { ok: false, needsRehash: false };
  } catch {
    return { ok: false, needsRehash: false };
  }
}

/**
 * Updates password_hash only without revoking sessions or touching password_changed_at.
 * If expectedCurrentHash is supplied, uses compare-and-set so a concurrent password change isn't overwritten.
 */
export async function rehashPassword(
  userId: string,
  newHash: string,
  expectedCurrentHash?: string,
): Promise<boolean> {
  const sql = expectedCurrentHash
    ? `UPDATE users SET password_hash = $2 WHERE id = $1 AND password_hash = $3 RETURNING id`
    : `UPDATE users SET password_hash = $2 WHERE id = $1 RETURNING id`;
  const params = expectedCurrentHash ? [userId, newHash, expectedCurrentHash] : [userId, newHash];
  const rows = await query<{ id: string }>(sql, params);
  return rows.length > 0;
}

// ── One-time email codes (table email_codes) ──
export const OTP_TTL_MS = 5 * 60 * 1000;
export const OTP_MAX_ATTEMPTS = 5;

export function generateOtp(): string {
  return crypto.randomInt(100000, 1000000).toString();
}

const hashCode = (code: string) => crypto.createHash('sha256').update(code.trim()).digest('hex');

/** Codes for sign-up, reset and login live in separate namespaces so they can't be swapped. */
type CodeKind = 'signup' | 'reset' | 'login';

export type PendingOtp = {
  /** SHA-256 of the 6-digit code; the code itself is never stored. */
  codeHash: string;
  passwordHash: string;
  name: string;
  /** Sign-up only: the 2FA choice made on the form, applied when the account is created. */
  twoFactor?: boolean;
  expiresAt: number;
  attempts: number;
};

type CodeRow = {
  code_hash: string;
  password_hash: string | null;
  name: string | null;
  two_factor: boolean | null;
  expires_at: Date;
  attempts: number;
};

const toPending = (r: CodeRow): PendingOtp => ({
  codeHash: r.code_hash,
  passwordHash: r.password_hash ?? '',
  name: r.name ?? '',
  twoFactor: r.two_factor ?? undefined,
  expiresAt: r.expires_at.getTime(),
  attempts: r.attempts,
});

/** Issue (or replace) the code for this kind + email. Expiry uses the database clock. */
async function setCode(kind: CodeKind, email: string, code: string, passwordHash = '', name = '', twoFactor?: boolean) {
  await query(
    `INSERT INTO email_codes (kind, email, code_hash, expires_at, attempts, password_hash, name, two_factor)
     VALUES ($1, $2, $3, now() + $4 * interval '1 millisecond', 0, $5, $6, $7)
     ON CONFLICT (kind, email) DO UPDATE SET
       code_hash = EXCLUDED.code_hash, expires_at = EXCLUDED.expires_at, attempts = 0,
       password_hash = EXCLUDED.password_hash, name = EXCLUDED.name, two_factor = EXCLUDED.two_factor,
       created_at = now()`,
    [kind, normEmail(email), hashCode(code), OTP_TTL_MS, passwordHash || null, name || null, twoFactor ?? null],
  );
  // Housekeeping: expired codes are useless; drop them so the table stays small.
  await query(`DELETE FROM email_codes WHERE expires_at < now() - interval '1 hour'`);
}

async function getCode(kind: CodeKind, email: string): Promise<PendingOtp | undefined> {
  const [row] = await query<CodeRow>(
    'SELECT code_hash, password_hash, name, two_factor, expires_at, attempts FROM email_codes WHERE kind = $1 AND email = $2 AND expires_at > now()',
    [kind, normEmail(email)],
  );
  return row ? toPending(row) : undefined;
}

async function clearCode(kind: CodeKind, email: string) {
  await query('DELETE FROM email_codes WHERE kind = $1 AND email = $2', [kind, normEmail(email)]);
}

export type CodeCheck = 'ok' | 'missing' | 'locked' | 'wrong';

/**
 * Check a code. Counts every attempt (5 wrong guesses lock the code), compares in constant time,
 * and deletes the code on success. Safe under concurrency: the attempt counter is incremented
 * atomically before comparing, and only one request can delete (and so redeem) a code.
 */
async function checkCode(kind: CodeKind, email: string, code: string): Promise<{ result: CodeCheck; entry?: PendingOtp }> {
  const key = normEmail(email);

  const [row] = await query<CodeRow>(
    `UPDATE email_codes SET attempts = attempts + 1
     WHERE kind = $1 AND email = $2 AND expires_at > now()
     RETURNING code_hash, password_hash, name, two_factor, expires_at, attempts`,
    [kind, key],
  );
  if (!row) return { result: 'missing' };
  if (row.attempts > OTP_MAX_ATTEMPTS) {
    await clearCode(kind, key);
    return { result: 'locked' };
  }
  const a = Buffer.from(hashCode(String(code)), 'hex');
  const b = Buffer.from(row.code_hash, 'hex');
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) return { result: 'wrong' };

  const [redeemed] = await query<CodeRow>(
    `DELETE FROM email_codes WHERE kind = $1 AND email = $2 AND code_hash = $3
     RETURNING code_hash, password_hash, name, two_factor, expires_at, attempts`,
    [kind, key, row.code_hash],
  );
  if (!redeemed) return { result: 'missing' }; // another request redeemed it first
  return { result: 'ok', entry: toPending(redeemed) };
}

// Sign-up codes (carry the hashed password + name until the code is confirmed)
export const setPendingOtp = (email: string, code: string, passwordHash: string, name = '', twoFactor = true) =>
  setCode('signup', email, code, passwordHash, name, twoFactor);
export const getPendingOtp = (email: string) => getCode('signup', email);
export const checkSignupCode = (email: string, code: string) => checkCode('signup', email, code);

// Password-reset codes
export const setResetCode = (email: string, code: string) => setCode('reset', email, code);
export const checkResetCode = (email: string, code: string) => checkCode('reset', email, code);

// Login 2FA codes (only ever issued after the password was verified)
export const setLoginCode = (email: string, code: string) => setCode('login', email, code);
export const checkLoginCode = (email: string, code: string) => checkCode('login', email, code);

// ── User Management (table users) ──
export async function findUser(email: string): Promise<UserRecord | undefined> {
  const [row] = await query<UserRow>(`SELECT ${USER_COLUMNS} FROM users WHERE email = $1`, [normEmail(email)]);
  return row ? toUser(row) : undefined;
}

export async function findUserById(id: string): Promise<UserRecord | undefined> {
  if (!/^[0-9a-f-]{36}$/i.test(id)) return undefined;
  const [row] = await query<UserRow>(`SELECT ${USER_COLUMNS} FROM users WHERE id = $1`, [id]);
  return row ? toUser(row) : undefined;
}

/**
 * Create an account together with its personal workspace (owner), in one transaction. If an
 * account already exists for the email (e.g. two confirmations raced), it is returned unchanged.
 */
export async function createUser(email: string, passwordHash: string, name = '', twoFactor = true): Promise<UserRecord> {
  const key = normEmail(email);
  const seed = adminSeedEmail();
  const created = await transaction(async (c) => {
    // First-admin seeding: the ADMIN_EMAIL account becomes admin only while no admin exists.
    // The advisory lock stops two sign-ups racing past the "no admin yet" check.
    let role: 'admin' | 'user' = 'user';
    if (seed && key === seed) {
      await query('SELECT pg_advisory_xact_lock(7312020)', [], c);
      const [admin] = await query(`SELECT 1 FROM users WHERE platform_role = 'admin' LIMIT 1`, [], c);
      if (!admin) role = 'admin';
    }
    const [row] = await query<UserRow>(
      `INSERT INTO users (email, name, password_hash, platform_role, two_factor)
       VALUES ($1, $2, $3, $4, $5)
       ON CONFLICT (email) DO NOTHING
       RETURNING ${USER_COLUMNS}`,
      [key, name, passwordHash, role, role === 'admin' || twoFactor],
      c,
    );
    if (row) await createPersonalWorkspace(c, row.id, key);
    return row;
  });
  if (created) return toUser(created);
  const existing = await findUser(key);
  if (!existing) throw new Error('createUser: insert conflicted but no account was found');
  return existing;
}

async function updateOne(email: string, set: string, params: unknown[] = []): Promise<UserRecord | undefined> {
  const [row] = await query<UserRow>(`UPDATE users SET ${set} WHERE email = $1 RETURNING ${USER_COLUMNS}`, [normEmail(email), ...params]);
  return row ? toUser(row) : undefined;
}

/**
 * Replace a password and sign the account out everywhere (every session is revoked in the same
 * transaction). The caller starts a fresh session for the current browser.
 */
export async function updatePassword(email: string, passwordHash: string): Promise<UserRecord | undefined> {
  return transaction(async (c) => {
    const [row] = await query<UserRow>(
      `UPDATE users SET password_hash = $2, password_changed_at = now() WHERE email = $1 RETURNING ${USER_COLUMNS}`,
      [normEmail(email), passwordHash],
      c,
    );
    if (!row) return undefined;
    await query('UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL', [row.id], c);
    // Personal API tokens too: a new password should lock out whoever had the old one.
    await query('UPDATE api_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL', [row.id], c);
    return toUser(row);
  });
}

/** Change the display name (already validated with cleanName). */
export const updateName = (email: string, name: string) => updateOne(email, 'name = $2', [name]);

/**
 * Permanently remove an account, its pending codes, its personal workspace and any team workspace
 * where it is the only member. Sessions and memberships go with it (ON DELETE CASCADE).
 * Throws SoleOwnerError (from ./workspaces) if it is the only owner of a team with other members.
 */
export async function deleteUser(email: string): Promise<boolean> {
  const key = normEmail(email);
  return transaction(async (c) => {
    const [user] = await query<{ id: string }>('SELECT id FROM users WHERE email = $1 FOR UPDATE', [key], c);
    await query('DELETE FROM email_codes WHERE email = $1', [key], c);
    if (!user) return false;
    await releaseWorkspacesOf(c, user.id);
    await query('DELETE FROM users WHERE id = $1', [user.id], c);
    return true;
  });
}

export class LastAdminError extends Error {
  constructor() {
    super('This is the only admin account. Grant admin to someone else first.');
    this.name = 'LastAdminError';
  }
}

/**
 * Grant or remove platform admin. A privilege change signs the account out everywhere (its next
 * sign-in gets a fresh session token with the new role), and admins always have 2FA on.
 * Refuses to remove the last admin.
 */
export async function setPlatformRole(
  email: string,
  role: 'admin' | 'user',
  by: { id?: string | null; email?: string | null } | null = null,
): Promise<UserRecord | undefined> {
  const changed = await transaction(async (c) => {
    await query('SELECT pg_advisory_xact_lock(7312020)', [], c);
    const [current] = await query<UserRow>(`SELECT ${USER_COLUMNS} FROM users WHERE email = $1 FOR UPDATE`, [normEmail(email)], c);
    if (!current) return undefined;
    if (current.platform_role === role) return { user: toUser(current), changed: false };
    if (role === 'user') {
      const [{ n }] = await query<{ n: number }>(`SELECT count(*)::int AS n FROM users WHERE platform_role = 'admin'`, [], c);
      if (n <= 1) throw new LastAdminError();
    }
    const [row] = await query<UserRow>(
      `UPDATE users SET platform_role = $2, two_factor = (two_factor OR $2 = 'admin') WHERE id = $1 RETURNING ${USER_COLUMNS}`,
      [current.id, role],
      c,
    );
    await query('UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL', [row.id], c);
    return { user: toUser(row), changed: true };
  });
  if (changed?.changed) {
    await audit({
      action: role === 'admin' ? 'admin.role.granted' : 'admin.role.revoked',
      actor: by,
      target: { type: 'user', id: changed.user.id },
      details: { email: changed.user.email, sessionsRevoked: true },
    });
  }
  return changed?.user;
}

/** Admins can't turn 2FA off: their account can see and manage every user. */
export const twoFactorEnforced = (u: UserRecord) => u.role === 'admin';
export const twoFactorOn = (u: UserRecord) => twoFactorEnforced(u) || !!u.twoFactor;

export const setTwoFactor = (email: string, enabled: boolean) =>
  updateOne(email, `two_factor = (platform_role = 'admin' OR $2)`, [enabled]);

/** Public view of every account (no password hashes), newest first. Admin-only callers. */
export type UserListing = {
  id: string;
  email: string;
  name: string;
  role: 'admin' | 'user';
  status: 'active' | 'disabled';
  createdAt: number;
  twoFactor: boolean;
  /** Last request from any of their sessions (ms epoch), or null if never signed in since sessions existed. */
  lastActiveAt: number | null;
};

export async function listUsers(): Promise<UserListing[]> {
  const rows = await query<UserRow & { last_active: Date | null }>(
    `SELECT ${USER_COLUMNS.split(', ').map((c) => `u.${c}`).join(', ')}, (SELECT max(s.last_seen_at) FROM sessions s WHERE s.user_id = u.id) AS last_active
       FROM users u ORDER BY u.created_at DESC`,
  );
  return rows.map((r) => {
    const u = toUser(r);
    return { id: u.id, email: u.email, name: u.name || '', role: u.role, status: u.status, createdAt: u.createdAt, twoFactor: twoFactorOn(u), lastActiveAt: r.last_active?.getTime() ?? null };
  });
}

/**
 * Disable or re-enable an account (platform admins). Disabling signs it out everywhere at once and
 * blocks sign-in; the last active admin can't be disabled. Audit-logged with who did it.
 */
export async function setUserStatus(
  userId: string,
  status: 'active' | 'disabled',
  by: { id?: string | null; email?: string | null } | null,
): Promise<UserRecord | undefined> {
  const changed = await transaction(async (c) => {
    await query('SELECT pg_advisory_xact_lock(7312020)', [], c); // same lock as admin role changes
    const [current] = await query<UserRow>(`SELECT ${USER_COLUMNS} FROM users WHERE id = $1 FOR UPDATE`, [userId], c);
    if (!current) return undefined;
    if (current.status === status) return { user: toUser(current), changed: false };
    if (status === 'disabled' && current.platform_role === 'admin') {
      const [{ n }] = await query<{ n: number }>(`SELECT count(*)::int AS n FROM users WHERE platform_role = 'admin' AND status = 'active'`, [], c);
      if (n <= 1) throw new LastAdminError();
    }
    const [row] = await query<UserRow>(`UPDATE users SET status = $2 WHERE id = $1 RETURNING ${USER_COLUMNS}`, [userId, status], c);
    if (status === 'disabled') await query('UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL', [userId], c);
    return { user: toUser(row), changed: true };
  });
  if (changed?.changed) {
    await audit({
      action: status === 'disabled' ? 'admin.user.disabled' : 'admin.user.enabled',
      actor: by,
      target: { type: 'user', id: userId },
      details: { email: changed.user.email, ...(status === 'disabled' ? { sessionsRevoked: true } : {}) },
    });
  }
  return changed?.user;
}

/** Session payload for a stored user. */
export function publicUser(u: UserRecord): { email: string; role: 'admin' | 'user'; name: string } {
  return { email: u.email, role: u.role, name: u.name || '' };
}
