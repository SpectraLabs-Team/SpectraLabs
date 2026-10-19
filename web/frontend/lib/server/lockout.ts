import { query } from './db';
import { hit } from './rateLimit';

/**
 * Account lockout (M19 task 7): 10 failed password attempts lock sign-in to the account for
 * 15 minutes, and the owner is emailed. Failures older than an hour are forgotten, and each lock
 * period starts a fresh count. A successful sign-in or a password reset clears it.
 * While locked, sign-in is refused even with the right password (so the lock can't be used to
 * test guesses).
 */
export const LOCK_THRESHOLD = 10;
export const LOCK_DURATION_MS = 15 * 60 * 1000;
const FAILURE_MEMORY = '1 hour';
/** At most one lockout email per account per 6 hours, however long an attack goes on. */
const EMAIL_EVERY_MS = 6 * 60 * 60 * 1000;

/** ms epoch until which the account is locked, or null. */
export async function lockedUntil(userId: string): Promise<number | null> {
  const [row] = await query<{ locked_until: Date }>('SELECT locked_until FROM users WHERE id = $1 AND locked_until > now()', [userId]);
  return row ? row.locked_until.getTime() : null;
}

export type FailureResult = {
  /** Failures in the current count. */
  failures: number;
  /** ms epoch the lock ends, if this failure (or an earlier one) locked the account. */
  lockedUntil: number | null;
  /** True only for the failure that started this lock: send the email then. */
  justLocked: boolean;
};

/** Count a wrong password for an existing account (atomic; safe under parallel attempts). */
export async function recordFailedLogin(userId: string): Promise<FailureResult> {
  const [row] = await query<{ failed_login_count: number; locked_until: Date | null; just_locked: boolean }>(
    `UPDATE users u SET
       failed_login_count   = f.n,
       last_failed_login_at = now(),
       -- Lock at the threshold; otherwise keep a running lock and drop an expired one (so the
       -- next period counts from 1 and can lock again).
       locked_until         = CASE WHEN f.n >= $2 THEN now() + $3 * interval '1 millisecond'
                                   WHEN u.locked_until > now() THEN u.locked_until
                                   ELSE NULL END
     FROM (
       SELECT id,
              CASE WHEN last_failed_login_at IS NULL
                     OR last_failed_login_at < now() - interval '${FAILURE_MEMORY}'
                     OR (locked_until IS NOT NULL AND locked_until <= now())
                   THEN 1 ELSE failed_login_count + 1 END AS n
         FROM users WHERE id = $1 FOR UPDATE
     ) f
     WHERE u.id = f.id
     RETURNING u.failed_login_count, u.locked_until, (f.n = $2) AS just_locked`,
    [userId, LOCK_THRESHOLD, LOCK_DURATION_MS],
  );
  if (!row) return { failures: 0, lockedUntil: null, justLocked: false };
  const until = row.locked_until && row.locked_until.getTime() > Date.now() ? row.locked_until.getTime() : null;
  return { failures: row.failed_login_count, lockedUntil: until, justLocked: row.just_locked };
}

/** Forget failures and lift any lock (successful sign-in, password reset). */
export async function clearFailedLogins(userId: string): Promise<void> {
  await query(
    'UPDATE users SET failed_login_count = 0, last_failed_login_at = NULL, locked_until = NULL WHERE id = $1 AND (failed_login_count <> 0 OR locked_until IS NOT NULL)',
    [userId],
  );
}

/** Whether to send a lockout email now (rate-limited per account so an attack can't flood the inbox). */
export async function shouldSendLockoutEmail(userId: string): Promise<boolean> {
  return (await hit(`lockout-email:${userId}`, 1, EMAIL_EVERY_MS)).ok;
}

/** "Locked for 12 more minutes" style text. */
export function lockMessage(until: number): string {
  const mins = Math.max(1, Math.ceil((until - Date.now()) / 60_000));
  return `Too many failed sign-in attempts. This account is locked for ${mins} more minute${mins === 1 ? '' : 's'}. Reset your password to unlock it now.`;
}
