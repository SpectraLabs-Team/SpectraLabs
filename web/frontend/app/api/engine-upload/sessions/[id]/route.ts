import { NextResponse } from 'next/server';
import crypto from 'crypto';
import { bearerToken } from '@/lib/server/apiTokens';
import { ENGINE_IDENTITY_HEADER, signEngineIdentity } from '@/lib/server/engineIdentity';
import { ENGINE_CHUNK_ROUTE } from '@/lib/server/engineRoutes';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireWorkspace } from '@/lib/server/requireUser';
import { engineSecret, signEngineRequest } from '@/lib/server/engineSign';

/**
 * One chunk of a resumable upload (M1b task 9): PUT the raw bytes (at most 8 MiB) with
 * `?offset=<bytes already received>`. Forwarded, signed, to the engine's
 * PUT /api/v1/uploads/sessions/{id}/chunk, which appends it or answers 409 `offset_mismatch`
 * with where to continue. Starting, checking, completing and abandoning a session go through the
 * JSON proxy (/api/engine). Editors of the active workspace only. Chunks are small enough for
 * middleware.ts, so its cross-site check applies as for every /api route.
 */
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

const ENGINE_URL = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/+$/, '');
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export async function PUT(req: Request, props: { params: Promise<{ id: string }> }) {
  const { id } = await props.params;
  if (!UUID.test(id)) return NextResponse.json({ detail: 'Not found.' }, { status: 404 });
  const offset = new URL(req.url).searchParams.get('offset') ?? '';
  if (!/^\d{1,16}$/.test(offset)) return NextResponse.json({ detail: 'offset: a whole number of bytes is required.' }, { status: 422 });
  if (bearerToken(req.headers.get('authorization')) !== undefined) {
    return NextResponse.json({ detail: 'Uploads need a signed-in session; API tokens cannot upload yet.' }, { status: 403 });
  }
  const { auth, workspace, res } = await requireWorkspace('editor', undefined, 'detail');
  if (res) return res;
  // 1 GiB in 8 MiB chunks is 128 requests; allow a few retries on top.
  const limit = await hit(`engine:chunk:${auth.user.id}`, 600, 10 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'upload chunks');
  const ipLimit = await hit(`engine:chunk:ip:${clientIp(req)}`, 1200, 10 * 60_000);
  if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'upload chunks');

  const declared = Number(req.headers.get('content-length') || 0);
  if (declared > ENGINE_CHUNK_ROUTE.maxBytes) return NextResponse.json({ detail: 'Chunks are at most 8 MiB.' }, { status: 413 });
  const body = Buffer.from(await req.arrayBuffer());
  if (body.length > ENGINE_CHUNK_ROUTE.maxBytes) return NextResponse.json({ detail: 'Chunks are at most 8 MiB.' }, { status: 413 });
  if (body.length === 0) return NextResponse.json({ detail: 'The chunk is empty.' }, { status: 422 });
  const secret = engineSecret();
  if (!secret) {
    console.error('[engine chunk] ENGINE_SECRET is missing or shorter than 32 characters');
    return NextResponse.json({ detail: 'The server is not configured to reach the engine.' }, { status: 500 });
  }

  const path = `api/v1/uploads/sessions/${id}/chunk`;
  const pathAndQuery = `/${path}?offset=${offset}`;
  const requestId = crypto.randomUUID();
  const bodySha256 = crypto.createHash('sha256').update(body).digest('hex');
  const headers: Record<string, string> = {
    'Content-Type': 'application/octet-stream',
    ...signEngineRequest(secret, { method: 'PUT', pathAndQuery, requestId, bodySha256 }, { user: auth.user.id, workspace: workspace.id }),
  };
  const identity = signEngineIdentity({
    userId: auth.user.id,
    sessionId: auth.session.id,
    workspaceId: workspace.id,
    workspaceRole: workspace.role,
    platformRole: auth.user.role,
    method: 'PUT',
    path,
  });
  if (identity) headers[ENGINE_IDENTITY_HEADER] = identity;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ENGINE_CHUNK_ROUTE.timeoutMs);
  try {
    const upstream = await fetch(`${ENGINE_URL}${pathAndQuery}`, { method: 'PUT', headers, body, signal: controller.signal, cache: 'no-store' });
    return new NextResponse(await upstream.text(), {
      status: upstream.status,
      headers: {
        'Content-Type': upstream.headers.get('content-type') || 'application/json',
        'Cache-Control': 'no-store',
        'X-Request-Id': upstream.headers.get('x-request-id') || requestId,
      },
    });
  } catch (e) {
    const timedOut = e instanceof Error && e.name === 'AbortError';
    return NextResponse.json({ detail: timedOut ? 'The engine took too long to store the chunk.' : 'The Rust engine is not reachable.' }, { status: timedOut ? 504 : 502 });
  } finally {
    clearTimeout(timer);
  }
}
