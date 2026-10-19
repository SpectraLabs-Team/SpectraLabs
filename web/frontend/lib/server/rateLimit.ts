import { query } from './db';

/**
 * Fixed-window rate limiter backed by Postgres (table `rate_limits`), so limits survive restarts
 * and hold across several server processes. Each hit is one atomic upsert.
 *
 * Keys name what is limited and for whom, e.g. `login:ip:<ip>` or `otp:email:hour:<email>`.
 * A key's window starts at its first hit; once it ends, the next hit starts a new window.
 */

export type Limit = { ok: true } | { ok: false; retryAfter: number };

type Row = { count: number; retry_ms: number };

const retryAfter = (ms: number) => Math.max(1, Math.ceil(ms / 1000));

/** Count one hit against `key`; refuse once more than `max` hits land within `windowMs`. */
export async function hit(key: string, max: number, windowMs: number): Promise<Limit> {
  const row = await bump(key, windowMs);
  return row.count <= max ? { ok: true } : { ok: false, retryAfter: retryAfter(row.retry_ms) };
}

/** Count one hit against `key` and return the count in the current window (no limit applied). */
export async function count(key: string, windowMs: number): Promise<number> {
  return (await bump(key, windowMs)).count;
}

async function bump(key: string, windowMs: number): Promise<Row> {
  const [row] = await query<Row>(
    `INSERT INTO rate_limits AS r (key, count, reset_at)
     VALUES ($1, 1, now() + $2 * interval '1 millisecond')
     ON CONFLICT (key) DO UPDATE SET
       count    = CASE WHEN r.reset_at <= now() THEN 1 ELSE r.count + 1 END,
       reset_at = CASE WHEN r.reset_at <= now() THEN EXCLUDED.reset_at ELSE r.reset_at END
     RETURNING count, (extract(epoch FROM (reset_at - now())) * 1000)::float8 AS retry_ms`,
    [key, windowMs],
  );
  // Housekeeping now and then: rows whose window ended long ago are dead weight.
  if (Math.random() < 0.01) {
    query(`DELETE FROM rate_limits WHERE reset_at < now() - interval '1 hour'`).catch(() => {});
  }
  return row;
}

/** Check without counting (e.g. "is this already over the limit?"). */
export async function peek(key: string, max: number): Promise<Limit> {
  const [row] = await query<Row>(
    `SELECT count, (extract(epoch FROM (reset_at - now())) * 1000)::float8 AS retry_ms
       FROM rate_limits WHERE key = $1 AND reset_at > now()`,
    [key],
  );
  return !row || row.count < max ? { ok: true } : { ok: false, retryAfter: retryAfter(row.retry_ms) };
}

/** Clear a key (e.g. failures after a successful sign-in). */
export async function reset(key: string): Promise<void> {
  await query('DELETE FROM rate_limits WHERE key = $1', [key]);
}

/**
 * Run several limits in order; the first one exceeded wins. Every limit up to and including that
 * one is counted.
 */
export async function hitAll(limits: readonly (readonly [key: string, max: number, windowMs: number, what: string])[]): Promise<{ ok: true } | { ok: false; retryAfter: number; what: string }> {
  for (const [key, max, windowMs, what] of limits) {
    const r = await hit(key, max, windowMs);
    if (!r.ok) return { ok: false, retryAfter: r.retryAfter, what };
  }
  return { ok: true };
}

/**
 * Best-effort client IP. x-forwarded-for is only trustworthy behind a proxy you control
 * (Vercel, nginx) that overwrites it; per-account limits don't depend on it.
 */
export function clientIp(req: Request): string {
  const xff = req.headers.get('x-forwarded-for');
  if (xff) return xff.split(',')[0].trim();
  return req.headers.get('x-real-ip')?.trim() || 'unknown';
}

export function tooMany(retryAfter: number, what = 'attempts') {
  const mins = Math.max(1, Math.ceil(retryAfter / 60));
  return Response.json(
    { ok: false, error: `Too many ${what}. Please wait ${retryAfter < 90 ? `${retryAfter}s` : `${mins} min`} and try again.` },
    { status: 429, headers: { 'Retry-After': String(retryAfter) } },
  );
}
