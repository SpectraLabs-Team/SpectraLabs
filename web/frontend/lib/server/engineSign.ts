import crypto from 'crypto';

/**
 * Signs requests from this server to the Rust engine (M20 step 5). The engine trusts only
 * requests carrying a fresh HMAC-SHA256 over the method, path, timestamp, identity headers and
 * body hash, keyed with ENGINE_SECRET (shared with the engine; 32+ characters).
 *
 * The canonical string must match `canonical` in crates/spectra-api/src/auth.rs byte for byte;
 * both sides pin the same test vector.
 */

export const ENGINE_SECRET_MIN_LENGTH = 32;

/** ENGINE_SECRET, or null when it is missing or too short. */
export function engineSecret(): string | null {
  const s = (process.env.ENGINE_SECRET || '').trim();
  return s.length >= ENGINE_SECRET_MIN_LENGTH ? s : null;
}

export function canonical(
  method: string,
  pathAndQuery: string,
  timestamp: string,
  user: string,
  workspace: string,
  requestId: string,
  body: string,
): string {
  const bodyHash = crypto.createHash('sha256').update(body, 'utf8').digest('hex');
  return canonicalWithBodyHash(method, pathAndQuery, timestamp, user, workspace, requestId, bodyHash);
}

/** `canonical` with the body's hex SHA-256 already computed (uploads stream their body). */
export function canonicalWithBodyHash(
  method: string,
  pathAndQuery: string,
  timestamp: string,
  user: string,
  workspace: string,
  requestId: string,
  bodySha256: string,
): string {
  return ['spectra-v1', method.toUpperCase(), pathAndQuery, timestamp, user, workspace, requestId, bodySha256].join('\n');
}

export type EngineIdentity = {
  /** The signed-in user (email until users move to Postgres, then their id). */
  user: string;
  /** The active workspace's UUID, or '' when there is none yet. */
  workspace?: string;
};

/** The headers to add to one engine request. `now` is for tests. */
export function signEngineRequest(
  secret: string,
  req: { method: string; pathAndQuery: string; body?: string; requestId: string; bodySha256?: string },
  identity: EngineIdentity,
  now: number = Date.now(),
): Record<string, string> {
  const timestamp = Math.floor(now / 1000).toString();
  const workspace = identity.workspace || '';
  const message = req.bodySha256
    ? canonicalWithBodyHash(req.method, req.pathAndQuery, timestamp, identity.user, workspace, req.requestId, req.bodySha256)
    : canonical(req.method, req.pathAndQuery, timestamp, identity.user, workspace, req.requestId, req.body || '');
  return {
    'X-Spectra-User': identity.user,
    'X-Spectra-Workspace': workspace,
    'X-Request-Id': req.requestId,
    'X-Spectra-Timestamp': timestamp,
    'X-Spectra-Signature': crypto.createHmac('sha256', secret).update(message, 'utf8').digest('hex'),
  };
}
