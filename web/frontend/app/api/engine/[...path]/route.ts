import { NextResponse } from 'next/server';
import { hit, tooMany } from '@/lib/server/rateLimit';
import { ENGINE_IDENTITY_HEADER, signEngineIdentity } from '@/lib/server/engineIdentity';
import { ENGINE_ROUTES } from '@/lib/server/engineRoutes';
import { requireWorkspace } from '@/lib/server/requireUser';

/**
 * Authenticated proxy to the Rust engine. The browser never talks to spectra-api directly:
 * every call lands here, the session cookie is checked, and only then is it forwarded to
 * ENGINE_URL (server-side env, default http://127.0.0.1:8000, the engine's default bind).
 * Signed-in calls carry a signed identity header (user, session, active workspace and role); see
 * lib/server/engineIdentity.ts.
 */
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

const ENGINE_URL = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/+$/, '');
const MAX_BODY = 2 * 1024 * 1024; // portfolio CSVs are small; refuse anything bigger

async function proxy(req: Request, path: string[], method: 'GET' | 'POST') {
  const key = path.join('/');
  const route = Object.prototype.hasOwnProperty.call(ENGINE_ROUTES, key) ? ENGINE_ROUTES[key] : undefined;
  if (!route || route.method !== method) {
    return NextResponse.json({ detail: 'Not found.' }, { status: 404 });
  }

  const headers: Record<string, string> = {};
  if (!route.public) {
    // Signed in, and at least route.minRole in the active workspace (viewers can't run computations).
    const { auth, workspace, res } = await requireWorkspace(route.minRole, undefined, 'detail');
    if (res) return res;

    // Per-account compute budget: generous for interactive use, stops scripted hammering.
    const limit = route.heavy ? await hit(`engine:heavy:${auth.user.id}`, 40, 10 * 60_000) : await hit(`engine:${auth.user.id}`, 240, 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'engine requests');

    const identity = signEngineIdentity({
      userId: auth.user.id,
      sessionId: auth.session.id,
      workspaceId: workspace.id,
      workspaceRole: workspace.role,
      platformRole: auth.user.role,
      method,
      path: key,
    });
    if (identity) headers[ENGINE_IDENTITY_HEADER] = identity;
  }

  let body: string | undefined;
  if (method === 'POST') {
    const len = Number(req.headers.get('content-length') || 0);
    if (len > MAX_BODY) return NextResponse.json({ detail: 'Request too large.' }, { status: 413 });
    body = await req.text();
    if (body.length > MAX_BODY) return NextResponse.json({ detail: 'Request too large.' }, { status: 413 });
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), route.timeoutMs);
  try {
    const upstream = await fetch(`${ENGINE_URL}/${key}`, {
      method,
      body,
      headers: body ? { ...headers, 'Content-Type': 'application/json' } : headers,
      signal: controller.signal,
      cache: 'no-store',
    });
    // Pass the engine's status and JSON through unchanged.
    return new NextResponse(await upstream.text(), {
      status: upstream.status,
      headers: { 'Content-Type': upstream.headers.get('content-type') || 'application/json', 'Cache-Control': 'no-store' },
    });
  } catch (e) {
    const timedOut = e instanceof Error && e.name === 'AbortError';
    return NextResponse.json(
      { detail: timedOut ? 'The engine took too long to respond.' : 'The Rust engine is not reachable. Start it with `cargo run --release -p spectra-api`.' },
      { status: timedOut ? 504 : 502 },
    );
  } finally {
    clearTimeout(timer);
  }
}

export function GET(req: Request, { params }: { params: { path: string[] } }) {
  return proxy(req, params.path, 'GET');
}

export function POST(req: Request, { params }: { params: { path: string[] } }) {
  return proxy(req, params.path, 'POST');
}
