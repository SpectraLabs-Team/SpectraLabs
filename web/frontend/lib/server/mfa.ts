import crypto from 'crypto';
import { authSecret, type UserRecord } from './authStore';
import { query, transaction } from './db';
import type { SessionInfo } from './sessions';
import { generateTotpSecret, matchTotp, otpauthUri, base32Encode } from './totp';
import { openUserSecret, sealUserSecret } from './vault';

/**
 * Second factors and step-up (M19 task 10). Server-only.
 *
 * - Authenticator app (TOTP): enrol → confirm with a first code → required at every sign-in
 *   instead of the emailed code. The seed is encrypted with the master key (./vault).
 * - Recovery codes: 10 single-use codes, shown once at enrolment (or regeneration), stored as
 *   HMAC-SHA256(AUTH_SECRET, code), so a database copy alone can't use them.
 * - Sign-in challenge: after the password step, a 5-minute single-use token that the code step
 *   must present, so the second step can't be reached without the first.
 * - Step-up ("confirm it's you"): sensitive actions need the session to have proved the password
 *   or a code within the last 10 minutes. Signing in counts.
 * - Who must use two-step login: platform admins, owners of team workspaces, and anyone who has
 *   added a broker key.
 */

export class MfaError extends Error {
  constructor(
    public code: 'already_on' | 'no_pending' | 'bad_code' | 'not_on',
    message: string,
  ) {
    super(message);
    this.name = 'MfaError';
  }
}

// ── Recovery codes ──

export const RECOVERY_CODE_COUNT = 10;
const RC_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'; // no 0/O or 1/I: easy to copy by hand

const normRecovery = (code: string) => String(code).toUpperCase().replace(/[^A-Z0-9]/g, '');
const recoveryHash = (code: string) => crypto.createHmac('sha256', authSecret()).update(`recovery:${normRecovery(code)}`).digest();

/** "ABCDE-FGHJK": 10 characters from a 32-letter alphabet = 50 random bits each. */
function newRecoveryCode(): string {
  const bytes = crypto.randomBytes(10);
  const chars = Array.from(bytes, (b) => RC_ALPHABET[b & 31]).join('');
  return `${chars.slice(0, 5)}-${chars.slice(5)}`;
}

export const looksLikeRecoveryCode = (code: string) => normRecovery(code).length === 10 && !/^\d+$/.test(normRecovery(code));

async function replaceRecoveryCodes(c: Parameters<typeof query>[2], userId: string): Promise<string[]> {
  const codes = Array.from({ length: RECOVERY_CODE_COUNT }, newRecoveryCode);
  await query('DELETE FROM recovery_codes WHERE user_id = $1', [userId], c);
  for (const code of codes) await query('INSERT INTO recovery_codes (user_id, code_hash) VALUES ($1, $2)', [userId, recoveryHash(code)], c);
  return codes;
}

// ── Status ──

export type MfaStatus = { totp: 'off' | 'pending' | 'on'; totpSince: number | null; recoveryCodesLeft: number };

export async function getMfaStatus(userId: string): Promise<MfaStatus> {
  const [t] = await query<{ confirmed_at: Date | null }>('SELECT confirmed_at FROM user_totp WHERE user_id = $1', [userId]);
  const [{ left }] = await query<{ left: number }>('SELECT count(*)::int AS left FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL', [userId]);
  return { totp: !t ? 'off' : t.confirmed_at ? 'on' : 'pending', totpSince: t?.confirmed_at?.getTime() ?? null, recoveryCodesLeft: left };
}

export async function hasTotp(userId: string): Promise<boolean> {
  return (await query('SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL', [userId])).length > 0;
}

// ── Enrolment ──

/** Start (or restart) enrolment: a fresh secret, pending until confirmed. Returns what the QR encodes. */
export async function startTotpEnrollment(user: { id: string; email: string }): Promise<{ secret: string; uri: string }> {
  if (await hasTotp(user.id)) throw new MfaError('already_on', 'An authenticator app is already set up. Remove it first to set up a new one.');
  const secret = generateTotpSecret();
  const sealed = sealUserSecret(user.id, secret);
  await query(
    `INSERT INTO user_totp (user_id, secret_ciphertext, master_version) VALUES ($1, $2, $3)
     ON CONFLICT (user_id) DO UPDATE SET secret_ciphertext = EXCLUDED.secret_ciphertext, master_version = EXCLUDED.master_version,
       created_at = now(), confirmed_at = NULL, last_step = NULL
     WHERE user_totp.confirmed_at IS NULL`,
    [user.id, sealed.ciphertext, sealed.version],
  );
  const out = { secret: base32Encode(secret), uri: otpauthUri(user.email, secret) };
  secret.fill(0);
  return out;
}

/**
 * Finish enrolment with a first code from the app. Turns two-step login on (using the app from now
 * on) and returns 10 recovery codes: show them once, they can't be retrieved later.
 */
export async function confirmTotpEnrollment(userId: string, code: string): Promise<string[]> {
  return transaction(async (c) => {
    const [row] = await query<{ secret_ciphertext: Buffer; master_version: number; confirmed_at: Date | null }>(
      'SELECT secret_ciphertext, master_version, confirmed_at FROM user_totp WHERE user_id = $1 FOR UPDATE',
      [userId],
      c,
    );
    if (!row) throw new MfaError('no_pending', 'Start setting up the authenticator app first.');
    if (row.confirmed_at) throw new MfaError('already_on', 'An authenticator app is already set up.');
    const secret = openUserSecret(userId, row.secret_ciphertext, row.master_version);
    const step = matchTotp(secret, code);
    secret.fill(0);
    if (step === null) throw new MfaError('bad_code', 'That code doesn’t match. Check the time on your phone and try the newest code.');
    await query('UPDATE user_totp SET confirmed_at = now(), last_step = $2 WHERE user_id = $1', [userId, step], c);
    await query('UPDATE users SET two_factor = true WHERE id = $1', [userId], c);
    return replaceRecoveryCodes(c, userId);
  });
}

/** Remove the authenticator app (and its recovery codes). Two-step login falls back to emailed codes. */
export async function disableTotp(userId: string): Promise<boolean> {
  return transaction(async (c) => {
    await query('DELETE FROM recovery_codes WHERE user_id = $1', [userId], c);
    return (await query('DELETE FROM user_totp WHERE user_id = $1 RETURNING user_id', [userId], c)).length > 0;
  });
}

/** New set of 10 recovery codes (the old ones stop working). Needs a confirmed authenticator. */
export async function regenerateRecoveryCodes(userId: string): Promise<string[]> {
  if (!(await hasTotp(userId))) throw new MfaError('not_on', 'Set up an authenticator app first.');
  return transaction((c) => replaceRecoveryCodes(c, userId));
}

// ── Checking a second factor ──

/** A 6-digit authenticator code. Each code (time step) works once. */
export async function verifyTotpCode(userId: string, code: string): Promise<boolean> {
  const [row] = await query<{ secret_ciphertext: Buffer; master_version: number }>(
    'SELECT secret_ciphertext, master_version FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL',
    [userId],
  );
  if (!row) return false;
  const secret = openUserSecret(userId, row.secret_ciphertext, row.master_version);
  const step = matchTotp(secret, code);
  secret.fill(0);
  if (step === null) return false;
  // Atomic replay guard: only a step newer than the last accepted one can be used.
  const won = await query(
    'UPDATE user_totp SET last_step = $2 WHERE user_id = $1 AND confirmed_at IS NOT NULL AND (last_step IS NULL OR last_step < $2) RETURNING user_id',
    [userId, step],
  );
  return won.length > 0;
}

/** A recovery code. Each works once. */
export async function consumeRecoveryCode(userId: string, code: string): Promise<{ ok: boolean; left: number }> {
  if (!looksLikeRecoveryCode(code)) return { ok: false, left: -1 };
  const used = await query('UPDATE recovery_codes SET used_at = now() WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL RETURNING user_id', [userId, recoveryHash(code)]);
  const [{ left }] = await query<{ left: number }>('SELECT count(*)::int AS left FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL', [userId]);
  return { ok: used.length > 0, left };
}

/** Authenticator code or recovery code. */
export async function verifySecondFactor(userId: string, code: string): Promise<{ ok: false } | { ok: true; method: 'totp' | 'recovery_code'; recoveryCodesLeft?: number }> {
  if (/^\s*\d{6}\s*$/.test(code)) return (await verifyTotpCode(userId, code)) ? { ok: true, method: 'totp' } : { ok: false };
  const r = await consumeRecoveryCode(userId, code);
  return r.ok ? { ok: true, method: 'recovery_code', recoveryCodesLeft: r.left } : { ok: false };
}

// ── Sign-in challenge (password step → authenticator step) ──

export const MFA_CHALLENGE_TTL_MS = 5 * 60 * 1000;
const MFA_CHALLENGE_MAX_ATTEMPTS = 5;
const challengeHash = (token: string) => crypto.createHash('sha256').update(token).digest('hex');
const normEmail = (e: string) => e.trim().toLowerCase();

/** Issued only after a correct password. One live challenge per account; a new one replaces it. */
export async function createMfaChallenge(email: string): Promise<string> {
  const token = crypto.randomBytes(32).toString('base64url');
  await query(
    `INSERT INTO email_codes (kind, email, code_hash, expires_at, attempts)
     VALUES ('mfa', $1, $2, now() + $3 * interval '1 millisecond', 0)
     ON CONFLICT (kind, email) DO UPDATE SET code_hash = EXCLUDED.code_hash, expires_at = EXCLUDED.expires_at, attempts = 0, created_at = now()`,
    [normEmail(email), challengeHash(token), MFA_CHALLENGE_TTL_MS],
  );
  return token;
}

/**
 * Check a challenge and count the attempt (5 per challenge). Doesn't consume it: call
 * consumeMfaChallenge once the code is right.
 */
export async function checkMfaChallenge(email: string, token: string): Promise<'ok' | 'missing' | 'locked'> {
  if (typeof token !== 'string' || token.length < 20) return 'missing';
  const [row] = await query<{ code_hash: string; attempts: number }>(
    `UPDATE email_codes SET attempts = attempts + 1 WHERE kind = 'mfa' AND email = $1 AND expires_at > now() RETURNING code_hash, attempts`,
    [normEmail(email)],
  );
  if (!row) return 'missing';
  const a = Buffer.from(row.code_hash, 'hex');
  const b = Buffer.from(challengeHash(token), 'hex');
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) return 'missing';
  if (row.attempts > MFA_CHALLENGE_MAX_ATTEMPTS) {
    await query(`DELETE FROM email_codes WHERE kind = 'mfa' AND email = $1`, [normEmail(email)]);
    return 'locked';
  }
  return 'ok';
}

/** Single use: true for exactly one caller. */
export async function consumeMfaChallenge(email: string, token: string): Promise<boolean> {
  return (await query(`DELETE FROM email_codes WHERE kind = 'mfa' AND email = $1 AND code_hash = $2 RETURNING email`, [normEmail(email), challengeHash(token)])).length > 0;
}

// ── Step-up ("confirm it's you") ──

export const REAUTH_MAX_AGE_MS = 10 * 60 * 1000;

export const isRecentlyAuthenticated = (session: Pick<SessionInfo, 'reauthAt'>, now = Date.now()) =>
  session.reauthAt !== null && now - session.reauthAt < REAUTH_MAX_AGE_MS;

export async function markReauthenticated(sessionId: string): Promise<void> {
  await query('UPDATE sessions SET reauth_at = now() WHERE id = $1', [sessionId]);
}

// ── Who must use two-step login ──

/** Why this account must keep two-step login on, or null if it may turn it off. */
export async function twoFactorRequirement(user: Pick<UserRecord, 'id' | 'role'>): Promise<string | null> {
  if (user.role === 'admin') return 'Two-step login is required for admin accounts.';
  const [row] = await query<{ team_owner: boolean; key_adder: boolean }>(
    `SELECT EXISTS (SELECT 1 FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id
                     WHERE m.user_id = $1 AND m.role = 'owner' AND w.personal_owner_id IS NULL) AS team_owner,
            EXISTS (SELECT 1 FROM secrets WHERE created_by = $1) AS key_adder`,
    [user.id],
  );
  if (row?.team_owner) return 'Two-step login is required for owners of team workspaces.';
  if (row?.key_adder) return 'Two-step login is required because you’ve added broker API keys.';
  return null;
}
