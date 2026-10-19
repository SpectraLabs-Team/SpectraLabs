/**
 * Fixed-window rate limiter kept in server memory. Good for a single Next.js process;
 * with several instances or serverless, move the counters to Redis/Upstash.
 */
type Bucket = { count: number; resetAt: number };

const g = global as unknown as { __spectra_rl?: Map<string, Bucket> };
const buckets = g.__spectra_rl || new Map<string, Bucket>();
g.__spectra_rl = buckets;

let lastSweep = 0;
function sweep(now: number) {
  if (now - lastSweep < 60_000) return;
  lastSweep = now;
  buckets.forEach((b, k) => {
    if (b.resetAt <= now) buckets.delete(k);
  });
}

export type Limit = { ok: true } | { ok: false; retryAfter: number };

/** Count one hit against `key`; refuse once `max` hits land within `windowMs`. */
export function hit(key: string, max: number, windowMs: number): Limit {
  const now = Date.now();
  sweep(now);
  const b = buckets.get(key);
  if (!b || b.resetAt <= now) {
    buckets.set(key, { count: 1, resetAt: now + windowMs });
    return { ok: true };
  }
  if (b.count >= max) return { ok: false, retryAfter: Math.ceil((b.resetAt - now) / 1000) };
  b.count += 1;
  return { ok: true };
}

/** Check without counting (e.g. "is this email locked out by failures?"). */
export function peek(key: string, max: number): Limit {
  const now = Date.now();
  const b = buckets.get(key);
  if (!b || b.resetAt <= now || b.count < max) return { ok: true };
  return { ok: false, retryAfter: Math.ceil((b.resetAt - now) / 1000) };
}

export function reset(key: string) {
  buckets.delete(key);
}

/**
 * Best-effort client IP. x-forwarded-for is only trustworthy behind a proxy you control
 * (Vercel, nginx) that overwrites it; per-email limits below don't depend on it.
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
