import crypto from 'crypto';

/**
 * Signed identity header sent by the /api/engine proxy to the Rust engine (M19 task 3; the engine
 * starts verifying it in M20 task 5 / M19 task 4).
 *
 *   X-Spectra-Identity: <payload>.<signature>
 *     payload   = base64url(JSON {
 *                   v: 1,
 *                   uid: user id, sid: session id,
 *                   wid: active workspace id, wrole: 'owner' | 'editor' | 'viewer',
 *                   prole: 'admin' | 'user',
 *                   m: HTTP method, p: path after /api/engine/ (e.g. "api/risk"),
 *                   iat, exp: unix seconds (exp = iat + 60)
 *                 })
 *     signature = base64url(HMAC-SHA256(ENGINE_SHARED_SECRET, payload))
 *
 * The engine must: recompute the HMAC over the payload string and compare in constant time;
 * reject if exp has passed (allow a few seconds of clock skew) or if m/p don't match the request
 * it received (so a captured header can't be replayed against another route); then scope every
 * read and write to `wid`, and allow writes only for wrole editor/owner.
 *
 * ENGINE_SHARED_SECRET is shared only by this server and the engine (`npm run setup` generates
 * one). If it is not set, no header is sent and the engine sees anonymous requests, as before.
 */
export const ENGINE_IDENTITY_HEADER = 'X-Spectra-Identity';
export const ENGINE_IDENTITY_TTL_S = 60;

export type EngineIdentity = {
  userId: string;
  sessionId: string;
  workspaceId: string;
  workspaceRole: 'owner' | 'editor' | 'viewer';
  platformRole: 'admin' | 'user';
  method: string;
  path: string;
};

let warned = false;

/** The header value, or null if ENGINE_SHARED_SECRET isn't configured. */
export function signEngineIdentity(id: EngineIdentity, now = Date.now()): string | null {
  const secret = process.env.ENGINE_SHARED_SECRET;
  if (!secret || secret.length < 32) {
    if (!warned && process.env.NODE_ENV !== 'test') {
      warned = true;
      console.warn('[engine] ENGINE_SHARED_SECRET is not set (or shorter than 32 characters): requests to the engine carry no signed identity. Run `npm run setup`.');
    }
    return null;
  }
  const iat = Math.floor(now / 1000);
  const payload = Buffer.from(
    JSON.stringify({
      v: 1,
      uid: id.userId,
      sid: id.sessionId,
      wid: id.workspaceId,
      wrole: id.workspaceRole,
      prole: id.platformRole,
      m: id.method.toUpperCase(),
      p: id.path,
      iat,
      exp: iat + ENGINE_IDENTITY_TTL_S,
    }),
  ).toString('base64url');
  const sig = crypto.createHmac('sha256', secret).update(payload).digest('base64url');
  return `${payload}.${sig}`;
}

/** Reference verifier (what the engine does). Used by the tests; handy for a Node-side engine stub. */
export function verifyEngineIdentity(header: string, expect: { method: string; path: string }, secret: string, now = Date.now()): Record<string, unknown> | null {
  const [payload, sig] = header.split('.');
  if (!payload || !sig) return null;
  const want = crypto.createHmac('sha256', secret).update(payload).digest();
  const got = Buffer.from(sig, 'base64url');
  if (got.length !== want.length || !crypto.timingSafeEqual(got, want)) return null;
  const data = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'));
  const t = Math.floor(now / 1000);
  if (data.v !== 1 || typeof data.exp !== 'number' || t > data.exp + 5) return null;
  if (data.m !== expect.method.toUpperCase() || data.p !== expect.path) return null;
  return data;
}
