import { query } from './db';

/**
 * Active-time accounting for the admin console (migration 013). Server-only.
 *
 * A heartbeat credits the time since the previous heartbeat, capped at one minute, and only when
 * that previous heartbeat was recent. Anything longer is treated as the browser being closed,
 * asleep or hidden, so it is not counted. Only daily totals are stored.
 */

export const HEARTBEAT_INTERVAL_SECONDS = 60;
/** A gap longer than this means the page was not in use, so nothing is credited. */
export const MAX_CREDITED_GAP_SECONDS = 120;
/** Default retention for daily activity rows. */
export const ACTIVITY_RETENTION_DAYS = 400;

/** Seconds to credit for a heartbeat that arrives `gapSeconds` after the previous one. Pure. */
export function heartbeatCredit(gapSeconds: number | null): number {
  if (gapSeconds === null || !Number.isFinite(gapSeconds)) return 0;
  if (gapSeconds <= 0 || gapSeconds > MAX_CREDITED_GAP_SECONDS) return 0;
  return Math.min(HEARTBEAT_INTERVAL_SECONDS, Math.floor(gapSeconds));
}

/** The UTC calendar day that `now` falls on, as YYYY-MM-DD. */
export function activityDay(now: Date): string {
  return now.toISOString().slice(0, 10);
}

/** Records one heartbeat for `userId` at `now` and returns the seconds credited. */
export async function recordHeartbeat(userId: string, now: Date = new Date()): Promise<number> {
  const [last] = await query<{ last_heartbeat_at: Date }>(
    'SELECT last_heartbeat_at FROM user_activity_daily WHERE user_id = $1 ORDER BY day DESC LIMIT 1',
    [userId],
  );
  const gap = last ? (now.getTime() - last.last_heartbeat_at.getTime()) / 1000 : null;
  const credit = heartbeatCredit(gap);
  await query(
    `INSERT INTO user_activity_daily (user_id, day, active_seconds, heartbeats, last_heartbeat_at)
     VALUES ($1, $2, $3, 1, $4)
     ON CONFLICT (user_id, day) DO UPDATE SET
       active_seconds = user_activity_daily.active_seconds + EXCLUDED.active_seconds,
       heartbeats = user_activity_daily.heartbeats + 1,
       last_heartbeat_at = EXCLUDED.last_heartbeat_at`,
    [userId, activityDay(now), credit, now],
  );
  return credit;
}

/** Deletes daily activity rows older than `days`. Returns how many rows were removed. */
export async function purgeActivityOlderThan(days: number, now: Date = new Date()): Promise<number> {
  if (!Number.isInteger(days) || days < 1) throw new Error('Retention must be a whole number of days, at least 1.');
  const cutoff = new Date(now.getTime() - days * 86_400_000);
  const rows = await query<{ user_id: string }>('DELETE FROM user_activity_daily WHERE day < $1 RETURNING user_id', [activityDay(cutoff)]);
  return rows.length;
}
