/**
 * Session check for middleware (Edge runtime: Web Crypto, no Node `crypto`).
 * Must stay byte-compatible with createSessionToken in lib/server/authStore.ts:
 *   token = base64url(JSON payload) + "." + base64url(HMAC-SHA256(AUTH_SECRET, base64url(payload)))
 */

// No fallback: without a valid AUTH_SECRET every session is rejected (fail closed).
// Same rules as authSecret() in lib/server/authStore.ts.
const RETIRED_SECRETS = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

function secret(): string | null {
  const s = process.env.AUTH_SECRET;
  if (!s || s.trim().length === 0 || s.length < 32 || RETIRED_SECRETS.has(s)) return null;
  return s;
}

function b64urlFromBytes(bytes: ArrayBuffer): string {
  let bin = '';
  new Uint8Array(bytes).forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function b64urlDecode(s: string): string {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4);
  return new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
}

function safeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export async function verifySessionEdge(token: string | undefined): Promise<{ email: string; role: 'admin' | 'user' } | null> {
  const key = secret();
  if (!token || !key) return null;
  try {
    const [payload, sig] = token.split('.');
    if (!payload || !sig) return null;
    const k = await crypto.subtle.importKey('raw', new TextEncoder().encode(key), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const expected = b64urlFromBytes(await crypto.subtle.sign('HMAC', k, new TextEncoder().encode(payload)));
    if (!safeEqual(sig, expected)) return null;
    const data = JSON.parse(b64urlDecode(payload));
    if (data.exp && Date.now() / 1000 > data.exp) return null;
    if (typeof data.email !== 'string' || (data.role !== 'admin' && data.role !== 'user')) return null;
    return { email: data.email, role: data.role };
  } catch {
    return null;
  }
}
