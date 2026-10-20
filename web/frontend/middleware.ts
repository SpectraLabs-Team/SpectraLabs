import { NextResponse, type NextRequest } from 'next/server';
import { buildCsp, CSP_REPORT_PATH, cspMode, isCrossSite, newNonce } from '@/lib/csp';
import { looksLikeSessionToken } from '@/lib/sessionEdge';

const PROTECTED = ['/studio', '/admin', '/account'];
const isProtected = (p: string) => PROTECTED.some((base) => p === base || p.startsWith(`${base}/`));

/**
 * - /api/*: cross-site request guard (CSRF; see lib/csp.ts isCrossSite). Each route still does its
 *   own session and role checks. CSP violation reports are exempt (browsers send them without a
 *   usable Origin and they change nothing).
 * - Pages: a per-request nonce and the Content-Security-Policy header (report-only by default;
 *   CSP_MODE=enforce to block). Next adds the nonce to its own scripts; app/layout.tsx adds it to ours.
 * - /studio, /admin, /account: a request without a session cookie never receives the page; it is
 *   redirected to the landing page, which opens Log In. (The real session and role checks are
 *   database lookups in the API routes.)
 */
export function middleware(req: NextRequest) {
  const { pathname } = req.nextUrl;

  if (pathname.startsWith('/api/')) {
    const blocked =
      pathname !== CSP_REPORT_PATH &&
      isCrossSite({
        method: req.method,
        origin: req.headers.get('origin'),
        host: req.headers.get('x-forwarded-host') || req.headers.get('host'),
        secFetchSite: req.headers.get('sec-fetch-site'),
      });
    if (blocked) return NextResponse.json({ ok: false, error: 'Cross-site request blocked.' }, { status: 403 });
    return NextResponse.next();
  }

  if (isProtected(pathname)) {
    // Cheap check only (Edge runtime has no database): is there a cookie shaped like a session
    // token? A page reached with a dead session gets no data, and its client gate shows Log In.
    const token = req.cookies.get('spectra_session')?.value;
    if (!looksLikeSessionToken(token)) {
      const url = req.nextUrl.clone();
      url.pathname = '/';
      url.search = '';
      url.searchParams.set('auth', 'login');
      url.searchParams.set('next', pathname);
      const res = NextResponse.redirect(url);
      // A malformed cookie (e.g. an old signed-JSON one) is useless; clear it.
      if (req.cookies.get('spectra_session')) res.cookies.delete('spectra_session');
      return res;
    }
  }

  const mode = cspMode();
  if (mode === 'off') return NextResponse.next();

  const nonce = newNonce();
  const csp = buildCsp({
    nonce,
    dev: process.env.NODE_ENV !== 'production',
    https: req.nextUrl.protocol === 'https:' || req.headers.get('x-forwarded-proto') === 'https',
  });
  // Next reads the nonce from the request's CSP header and puts it on its own <script> tags;
  // x-nonce is for our inline scripts (app/layout.tsx).
  const requestHeaders = new Headers(req.headers);
  requestHeaders.set('x-nonce', nonce);
  requestHeaders.set('content-security-policy', csp);
  const res = NextResponse.next({ request: { headers: requestHeaders } });
  res.headers.set(mode === 'enforce' ? 'Content-Security-Policy' : 'Content-Security-Policy-Report-Only', csp);
  res.headers.set('Reporting-Endpoints', `csp="${CSP_REPORT_PATH}"`);
  return res;
}

export const config = {
  // Every page and API route; not Next's static files, images or the favicon (no HTML there).
  // Not /api/engine-upload either: Next buffers request bodies for middleware and cuts them at
  // 10 MB, and uploads are up to 100 MiB. That route does the cross-site check itself.
  matcher: ['/((?!_next/static|_next/image|favicon.ico|robots.txt|api/engine-upload$|.*\\.(?:png|jpg|jpeg|gif|svg|ico|webp|woff2?|txt|xml|json)$).*)'],
};
