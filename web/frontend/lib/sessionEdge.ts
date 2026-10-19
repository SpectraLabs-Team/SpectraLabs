/**
 * Session cookie format, shared by the Edge middleware and the Node session store.
 *
 * The cookie is an opaque random token (32 bytes, base64url = 43 characters). It means nothing
 * on its own: the real check (does the session exist, is it revoked, expired or idle, is the
 * account active?) is a database lookup in route handlers and the engine proxy
 * (lib/server/sessions.ts). Middleware runs on the Edge runtime without a database, so it only
 * does this cheap check to keep visitors with no session (or a garbage cookie) off protected pages.
 */
export const SESSION_TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;

/** True if the cookie value has the shape of a session token. Not proof of a valid session. */
export function looksLikeSessionToken(token: string | undefined | null): token is string {
  return typeof token === 'string' && SESSION_TOKEN_RE.test(token);
}
