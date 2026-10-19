/**
 * Content-Security-Policy and cross-site request rules (M19 task 14). Pure functions with no Node
 * or Next imports, so the Edge middleware can use them and the tests can load them.
 *
 * CSP: scripts run only with this request's nonce ('strict-dynamic' lets those scripts load the
 * app's chunks), so injected markup can't run script even if some text slips past escaping.
 * Styles allow inline (React style attributes, Tailwind) — the risk there is far lower.
 * Everything else is same-origin; the app has no third-party hosts.
 *
 * CSP_MODE: 'report-only' (default: violations are reported to /api/csp-report, nothing is
 * blocked), 'enforce', or 'off'.
 */
export type CspMode = 'report-only' | 'enforce' | 'off';

export function cspMode(raw = process.env.CSP_MODE): CspMode {
  return raw === 'enforce' || raw === 'off' ? raw : 'report-only';
}

export const CSP_REPORT_PATH = '/api/csp-report';

export function buildCsp({ nonce, dev, https }: { nonce: string; dev: boolean; https: boolean }): string {
  const directives: [string, ...string[]][] = [
    ['default-src', "'self'"],
    // Next's dev server evaluates modules with eval(); production never needs it.
    ['script-src', "'self'", `'nonce-${nonce}'`, "'strict-dynamic'", ...(dev ? ["'unsafe-eval'"] : [])],
    ['style-src', "'self'", "'unsafe-inline'"],
    ['img-src', "'self'", 'data:', 'blob:'],
    ['font-src', "'self'", 'data:'],
    // Hot reload talks to the dev server over a WebSocket.
    ['connect-src', "'self'", ...(dev ? ['ws:', 'wss:'] : [])],
    ['worker-src', "'self'", 'blob:'],
    ['manifest-src', "'self'"],
    ['frame-src', "'none'"],
    ['frame-ancestors', "'none'"],
    ['object-src', "'none'"],
    ['base-uri', "'self'"],
    ['form-action', "'self'"],
    ...(https ? ([['upgrade-insecure-requests']] as [string][]) : []),
    ['report-uri', CSP_REPORT_PATH],
    ['report-to', 'csp'],
  ];
  return directives.map((d) => d.join(' ')).join('; ');
}

/** A fresh nonce: 128 random bits, base64. Works on the Edge runtime and in Node. */
export function newNonce(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let bin = '';
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin);
}

// ── Cross-site request guard (CSRF) ──

/**
 * Origins allowed to make state-changing requests: this request's own host, APP_URL, and any in
 * ALLOWED_ORIGINS (comma-separated full origins, e.g. "https://app.example.com,https://admin.example.com")
 * for when the app is served from more than one domain.
 */
export function allowedOrigins(env: Record<string, string | undefined> = process.env): string[] {
  const list: string[] = [];
  const add = (v: string | undefined) => {
    if (!v) return;
    try {
      list.push(new URL(v.trim()).origin);
    } catch {
      /* ignore malformed entries */
    }
  };
  add(env.APP_URL);
  for (const o of (env.ALLOWED_ORIGINS ?? '').split(',')) if (o.trim()) add(o);
  return list;
}

/**
 * Should a request be refused as cross-site? Only POST/PUT/PATCH/DELETE are checked.
 * - Origin present: it must be this host or an allowed origin (a malformed or "null" Origin is refused).
 * - No Origin (non-browser clients, which can't ride a victim's cookies): allowed, unless the
 *   browser's Sec-Fetch-Site says it is cross-site.
 */
export function isCrossSite(
  req: { method: string; origin: string | null; host: string | null; secFetchSite: string | null },
  allowed: string[] = allowedOrigins(),
): boolean {
  if (!['POST', 'PUT', 'PATCH', 'DELETE'].includes(req.method.toUpperCase())) return false;
  if (!req.origin) return req.secFetchSite === 'cross-site';
  let origin: URL;
  try {
    origin = new URL(req.origin);
  } catch {
    return true;
  }
  if (req.host && origin.host === req.host) return false;
  return !allowed.includes(origin.origin);
}
