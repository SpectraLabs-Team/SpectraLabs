import crypto from 'crypto';
import { authSecret } from './authStore';
import { query } from './db';

/**
 * Known devices (M19 task 11), to tell the owner when their account is signed into from a browser
 * it hasn't seen before. A device is a browser holding the random `spectra_device` cookie (1 year,
 * HttpOnly); the server stores only HMAC-SHA256(AUTH_SECRET, token). Server-only.
 */
export const DEVICE_COOKIE = 'spectra_device';
export const DEVICE_COOKIE_MAX_AGE_S = 365 * 24 * 60 * 60;
const TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;

export const looksLikeDeviceToken = (v: string | undefined | null): v is string => typeof v === 'string' && TOKEN_RE.test(v);
export const newDeviceToken = () => crypto.randomBytes(32).toString('base64url');
export const deviceHash = (token: string) => crypto.createHmac('sha256', authSecret()).update(`device:${token}`).digest();

/**
 * Note that this account signed in on this device. `isNew`: first time for this account on this
 * device. `isFirst`: the account had no devices at all before (a brand-new account), so there is
 * nobody to warn.
 */
export async function recordDevice(userId: string, token: string, meta: { ip?: string | null; userAgent?: string | null } = {}): Promise<{ isNew: boolean; isFirst: boolean; hash: Buffer }> {
  const hash = deviceHash(token);
  const [row] = await query<{ inserted: boolean; others: number }>(
    `WITH others AS (SELECT count(*)::int AS n FROM known_devices WHERE user_id = $1 AND device_hash <> $2),
          up AS (
            INSERT INTO known_devices (user_id, device_hash, user_agent, ip) VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id, device_hash) DO UPDATE SET last_seen_at = now(), user_agent = EXCLUDED.user_agent, ip = EXCLUDED.ip
            RETURNING (xmax = 0) AS inserted
          )
     SELECT up.inserted, others.n AS others FROM up, others`,
    [userId, hash, meta.userAgent?.slice(0, 512) ?? null, meta.ip ?? null],
  );
  return { isNew: row.inserted, isFirst: row.inserted && row.others === 0, hash };
}

/** Devices this account has signed in from, most recent first. */
export async function listDevices(userId: string): Promise<{ hash: string; firstSeenAt: number; lastSeenAt: number; userAgent: string | null; ip: string | null }[]> {
  const rows = await query<{ device_hash: Buffer; first_seen_at: Date; last_seen_at: Date; user_agent: string | null; ip: string | null }>(
    'SELECT device_hash, first_seen_at, last_seen_at, user_agent, ip FROM known_devices WHERE user_id = $1 ORDER BY last_seen_at DESC',
    [userId],
  );
  return rows.map((r) => ({ hash: r.device_hash.toString('hex'), firstSeenAt: r.first_seen_at.getTime(), lastSeenAt: r.last_seen_at.getTime(), userAgent: r.user_agent, ip: r.ip }));
}
