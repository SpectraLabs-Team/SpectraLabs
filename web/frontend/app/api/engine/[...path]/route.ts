import { NextResponse } from 'next/server';
import crypto from 'crypto';
import { authenticateToken, bearerToken, scopeFor } from '@/lib/server/apiTokens';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { hasRole } from '@/lib/server/authz';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { ENGINE_IDENTITY_HEADER, signEngineIdentity } from '@/lib/server/engineIdentity';
import { allowsMethod, ENGINE_ROUTES, maxBodyBytes, minimumRole, routePolicyKey, type EngineMethod } from '@/lib/server/engineRoutes';
import { requireWorkspace } from '@/lib/server/requireUser';
import { engineSecret, signEngineRequest } from '@/lib/server/engineSign';

/**
 * Authenticated proxy to the Rust engine. Workspace roles and API-token scopes are checked here;
 * the forwarded request is HMAC-signed for the Axum engine to verify.
 */
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

const ENGINE_URL = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/+$/, '');

async function proxy(req: Request, path: string[], method: EngineMethod) {
  const key = path.join('/');
  const pathAndQuery = `/${key}${new URL(req.url).search}`;
  const policyKey = routePolicyKey(key);
  const route = Object.prototype.hasOwnProperty.call(ENGINE_ROUTES, policyKey) ? ENGINE_ROUTES[policyKey] : undefined;
  if (!route || !allowsMethod(route, method)) {
    return NextResponse.json({ detail: 'Not found.' }, { status: 404 });
  }
  const minRole = minimumRole(route, method);

  const headers: Record<string, string> = {};
  let signedUser = '';
  let signedWorkspace = '';
  const bearer = bearerToken(req.headers.get('authorization'));
  if (!route.public && bearer !== undefined) {
    const ip = clientIp(req);
    const guard = await hit(`api-token:ip:${ip}`, 600, 60_000);
    if (!guard.ok) return tooMany(guard.retryAfter, 'API requests');
    const caller = await authenticateToken(bearer, ip);
    if (!caller) {
      return NextResponse.json(
        { detail: 'Invalid, expired or revoked API token.' },
        { status: 401, headers: { 'WWW-Authenticate': 'Bearer' } },
      );
    }
    const meta = requestMeta(req);
    const used = {
      action: 'api_token.used' as const,
      actor: caller.user,
      workspaceId: caller.workspace.id,
      target: { type: 'api_token', id: caller.token.id },
    };
    const scope = scopeFor(minRole);
    if (!scope || !caller.token.scopes.includes(scope) || !hasRole(caller.workspace.role, minRole)) {
      await audit({ ...used, outcome: 'denied', details: { method, path: key, needs: scope ?? 'owner' } }, meta);
      return NextResponse.json(
        { detail: scope ? `This token can't ${scope === 'run' ? 'run computations' : 'read'} here (needs the "${scope}" scope and a high enough role).` : 'Not available to API tokens.' },
        { status: 403 },
      );
    }
    const limit = route.heavy
      ? await hit(`engine:heavy:token:${caller.token.id}`, 40, 10 * 60_000)
      : await hit(`engine:token:${caller.token.id}`, 240, 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'engine requests');
    await audit({ ...used, details: { method, path: key } }, meta);

    signedUser = caller.user?.id ?? `token:${caller.token.id}`;
    signedWorkspace = caller.workspace.id;
    const identity = signEngineIdentity({
      userId: signedUser,
      sessionId: `token:${caller.token.id}`,
      workspaceId: signedWorkspace,
      workspaceRole: caller.workspace.role,
      platformRole: 'user',
      method,
      path: key,
    });
    if (identity) headers[ENGINE_IDENTITY_HEADER] = identity;
  } else if (!route.public) {
    const { auth, workspace, res } = await requireWorkspace(minRole, undefined, 'detail');
    if (res) return res;

    const limit = route.heavy
      ? await hit(`engine:heavy:${auth.user.id}`, 40, 10 * 60_000)
      : await hit(`engine:${auth.user.id}`, 240, 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'engine requests');

    signedUser = auth.user.id;
    signedWorkspace = workspace.id;
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
  if (method === 'POST' || method === 'PATCH') {
    const maxBody = maxBodyBytes(route);
    const len = Number(req.headers.get('content-length') || 0);
    if (len > maxBody) return NextResponse.json({ detail: 'Request too large.' }, { status: 413 });
    body = await req.text();
    if (Buffer.byteLength(body) > maxBody) return NextResponse.json({ detail: 'Request too large.' }, { status: 413 });
    headers['Content-Type'] = 'application/json';
  }

  const requestId = crypto.randomUUID();
  if (!route.public) {
    const secret = engineSecret();
    if (!secret) {
      console.error('[engine proxy] ENGINE_SECRET is missing or shorter than 32 characters');
      return NextResponse.json({ detail: 'The server is not configured to reach the engine.' }, { status: 500 });
    }
    Object.assign(
      headers,
      signEngineRequest(
        secret,
        { method, pathAndQuery, body, requestId },
        { user: signedUser, workspace: signedWorkspace },
      ),
    );
  } else {
    headers['X-Request-Id'] = requestId;
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), route.timeoutMs);
  try {
    const upstream = await fetch(`${ENGINE_URL}${pathAndQuery}`, {
      method,
      body,
      headers,
      signal: controller.signal,
      cache: 'no-store',
    });
    const responseHeaders: Record<string, string> = {
      'Content-Type': upstream.headers.get('content-type') || 'application/json',
      'Cache-Control': 'no-store',
      'X-Request-Id': upstream.headers.get('x-request-id') || requestId,
    };
    // Artifact downloads keep their file name and checksum.
    for (const name of ['content-disposition', 'x-content-sha256']) {
      const value = upstream.headers.get(name);
      if (value) responseHeaders[name] = value;
    }
    const bodyBuf = Buffer.from(await upstream.arrayBuffer());
    return new NextResponse(bodyBuf, { status: upstream.status, headers: responseHeaders });
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

export async function PATCH(req: Request, props: { params: Promise<{ path: string[] }> }) {
  const params = await props.params;
  return proxy(req, params.path, 'PATCH');
}

export async function DELETE(req: Request, props: { params: Promise<{ path: string[] }> }) {
  const params = await props.params;
  return proxy(req, params.path, 'DELETE');
}
