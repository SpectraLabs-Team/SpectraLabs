import { NextResponse } from 'next/server';
import { authenticateToken, bearerToken, scopeFor } from '@/lib/server/apiTokens';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { hasRole } from '@/lib/server/authz';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { ENGINE_IDENTITY_HEADER, signEngineIdentity } from '@/lib/server/engineIdentity';
import { ENGINE_ROUTES } from '@/lib/server/engineRoutes';
import { requireWorkspace } from '@/lib/server/requireUser';

/**
 * Authenticated proxy to the Rust engine. The browser never talks to spectra-api directly:
 * every call lands here, the session cookie is checked, and only then is it forwarded to
 * ENGINE_URL (server-side env, default http://127.0.0.1:8000, the engine's default bind).
 * Signed-in calls carry a signed identity header (user, session, active workspace and role); see
 * lib/server/engineIdentity.ts. Scripts can call it with an API token instead of a session
 * (`Authorization: Bearer spectra_pat_…`, lib/server/apiTokens.ts); every token call is audit-logged.
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
  const bearer = bearerToken(req.headers.get('authorization'));
  if (!route.public && bearer !== undefined) {
    // API token (M19 task 17): the token's workspace, capped by its scopes; never the session.
    const ip = clientIp(req);
    const guard = await hit(`api-token:ip:${ip}`, 600, 60_000);
    if (!guard.ok) return tooMany(guard.retryAfter, 'API requests');
    const caller = await authenticateToken(bearer, ip);
    if (!caller) return NextResponse.json({ detail: 'Invalid, expired or revoked API token.' }, { status: 401, headers: { 'WWW-Authenticate': 'Bearer' } });
    const meta = requestMeta(req);
    const used = { action: 'api_token.used' as const, actor: caller.user, workspaceId: caller.workspace.id, target: { type: 'api_token', id: caller.token.id } };
    const scope = scopeFor(route.minRole);
    if (!scope || !caller.token.scopes.includes(scope) || !hasRole(caller.workspace.role, route.minRole)) {
      await audit({ ...used, outcome: 'denied', details: { method, path: key, needs: scope ?? 'owner' } }, meta);
      return NextResponse.json({ detail: scope ? `This token can't ${scope === 'run' ? 'run computations' : 'read'} here (needs the "${scope}" scope and a high enough role).` : 'Not available to API tokens.' }, { status: 403 });
    }
    const limit = route.heavy ? await hit(`engine:heavy:token:${caller.token.id}`, 40, 10 * 60_000) : await hit(`engine:token:${caller.token.id}`, 240, 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'engine requests');
    await audit({ ...used, details: { method, path: key } }, meta);

    const identity = signEngineIdentity({
      // Workspace tokens act as themselves, not as whoever created them.
      userId: caller.user?.id ?? `token:${caller.token.id}`,
      sessionId: `token:${caller.token.id}`,
      workspaceId: caller.workspace.id,
      workspaceRole: caller.workspace.role,
      platformRole: 'user', // admin powers never travel with a token
      method,
      path: key,
    });
    if (identity) headers[ENGINE_IDENTITY_HEADER] = identity;
  } else if (!route.public) {
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

export async function GET(req: Request, props: { params: Promise<{ path: string[] }> }) {
  const params = await props.params;
  return proxy(req, params.path, 'GET');
}

export async function POST(req: Request, props: { params: Promise<{ path: string[] }> }) {
  const params = await props.params;
  return proxy(req, params.path, 'POST');
}
