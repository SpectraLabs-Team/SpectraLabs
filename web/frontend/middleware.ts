import { NextResponse, type NextRequest } from 'next/server';
import { verifySessionEdge } from '@/lib/sessionEdge';

const UNSAFE_METHODS = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

/**
 * CSRF guard for the API. Browsers always send `Origin` on cross-site POSTs, so a state-changing
 * request whose Origin isn't this site is refused. (Without this, another site could submit a
 * form to /api/auth/login and sign a visitor into the attacker's account.) Requests with no
 * Origin come from non-browser clients, which can't ride a victim's cookies, so they pass.
 */
function crossSiteBlocked(req: NextRequest): boolean {
  if (!UNSAFE_METHODS.has(req.method)) return false;
  const origin = req.headers.get('origin');
  if (!origin) return false;
  const host = req.headers.get('x-forwarded-host') || req.headers.get('host');
  try {
    return new URL(origin).host !== host;
  } catch {
    return true; // malformed / "null" origin
  }
}

/**
 * - /api/*: CSRF guard (each route still does its own session and role checks).
 * - /studio, /admin: a request without a valid session never receives the page; it is
 *   redirected to the landing page, which opens Log In. (Admin *role* is enforced by
 *   /admin's UI and, for data, by /api/admin/* on the server.)
 */
export async function middleware(req: NextRequest) {
  const { pathname } = req.nextUrl;

  if (pathname.startsWith('/api/')) {
    if (crossSiteBlocked(req)) {
      return NextResponse.json({ ok: false, error: 'Cross-site request blocked.' }, { status: 403 });
    }
    return NextResponse.next();
  }

  const session = await verifySessionEdge(req.cookies.get('spectra_session')?.value);
  if (session) return NextResponse.next();

  const url = req.nextUrl.clone();
  url.pathname = '/';
  url.search = '';
  url.searchParams.set('auth', 'login');
  url.searchParams.set('next', pathname);
  const res = NextResponse.redirect(url);
  // A bad or expired cookie is useless; clear it so the client doesn't keep sending it.
  if (req.cookies.get('spectra_session')) res.cookies.delete('spectra_session');
  return res;
}

export const config = {
  matcher: ['/studio/:path*', '/admin/:path*', '/api/:path*'],
};
