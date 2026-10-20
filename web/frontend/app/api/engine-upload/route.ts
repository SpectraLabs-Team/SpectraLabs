import { NextResponse } from 'next/server';
import crypto from 'crypto';
import fs from 'fs';
import { Readable } from 'stream';
import { bearerToken } from '@/lib/server/apiTokens';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { isCrossSite } from '@/lib/csp';
import { ENGINE_IDENTITY_HEADER, signEngineIdentity } from '@/lib/server/engineIdentity';
import { ENGINE_UPLOAD_ROUTE } from '@/lib/server/engineRoutes';
import { cleanFilename, MAX_UPLOAD_BYTES, spoolToTempFile, UploadTooLarge } from '@/lib/server/engineUpload';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireWorkspace } from '@/lib/server/requireUser';
import { engineSecret, signEngineRequest } from '@/lib/server/engineSign';

/**
 * Data file uploads for the Studio's Data tab (M1 task 4): POST the raw file as the body,
 * `?filename=` for its name. Editors of the active workspace only.
 *
 * The JSON proxy (/api/engine) caps bodies at 2 MB, so files come here instead. The file is
 * written to a temporary file while it is hashed, then streamed to the engine's
 * POST /api/v1/uploads with its SHA-256 in the signed headers; the engine checks the bytes against
 * it. middleware.ts skips this path (it would buffer and cut bodies over 10 MB), so the
 * cross-site check it does for every other /api route is done here.
 */
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

const ENGINE_URL = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/+$/, '');

export async function POST(req: Request) {
  if (
    isCrossSite({
      method: req.method,
      origin: req.headers.get('origin'),
      host: req.headers.get('x-forwarded-host') || req.headers.get('host'),
      secFetchSite: req.headers.get('sec-fetch-site'),
    })
  ) {
    return NextResponse.json({ ok: false, error: 'Cross-site request blocked.' }, { status: 403 });
  }
  if (bearerToken(req.headers.get('authorization')) !== undefined) {
    return NextResponse.json({ detail: 'Uploads need a signed-in session; API tokens cannot upload yet.' }, { status: 403 });
  }
  // Editors upload (ENGINE_UPLOAD_ROUTE.minRole; test-auth.mjs checks the two agree).
  const { auth, workspace, res } = await requireWorkspace('editor', undefined, 'detail');
  if (res) return res;

  const limit = await hit(`engine:upload:${auth.user.id}`, 30, 10 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'uploads');
  const ipLimit = await hit(`engine:upload:ip:${clientIp(req)}`, 60, 10 * 60_000);
  if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'uploads');

  const declared = Number(req.headers.get('content-length') || 0);
  if (declared > MAX_UPLOAD_BYTES) {
    return NextResponse.json({ detail: `Files are limited to ${MAX_UPLOAD_BYTES / 1024 / 1024} MiB for now.` }, { status: 413 });
  }
  if (!req.body) return NextResponse.json({ detail: 'Send the file as the request body.' }, { status: 400 });
  const secret = engineSecret();
  if (!secret) {
    console.error('[engine upload] ENGINE_SECRET is missing or shorter than 32 characters');
    return NextResponse.json({ detail: 'The server is not configured to reach the engine.' }, { status: 500 });
  }

  const filename = cleanFilename(new URL(req.url).searchParams.get('filename'));
  let spooled;
  try {
    spooled = await spoolToTempFile(req.body);
  } catch (e) {
    if (e instanceof UploadTooLarge) return NextResponse.json({ detail: e.message }, { status: 413 });
    return NextResponse.json({ detail: 'The upload was interrupted; try again.' }, { status: 400 });
  }

  const meta = requestMeta(req);
  try {
    if (spooled.size === 0) return NextResponse.json({ detail: 'The file is empty.' }, { status: 400 });
    const path = ENGINE_UPLOAD_ROUTE.path;
    const pathAndQuery = `/${path}?filename=${encodeURIComponent(filename)}`;
    const requestId = crypto.randomUUID();
    const headers: Record<string, string> = {
      'Content-Type': 'application/octet-stream',
      'Content-Length': String(spooled.size),
      'X-Spectra-Content-SHA256': spooled.sha256,
      ...signEngineRequest(secret, { method: 'POST', pathAndQuery, requestId, bodySha256: spooled.sha256 }, { user: auth.user.id, workspace: workspace.id }),
    };
    const identity = signEngineIdentity({
      userId: auth.user.id,
      sessionId: auth.session.id,
      workspaceId: workspace.id,
      workspaceRole: workspace.role,
      platformRole: auth.user.role,
      method: 'POST',
      path,
    });
    if (identity) headers[ENGINE_IDENTITY_HEADER] = identity;

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), ENGINE_UPLOAD_ROUTE.timeoutMs);
    let upstream: Response;
    try {
      upstream = await fetch(`${ENGINE_URL}${pathAndQuery}`, {
        method: 'POST',
        headers,
        body: Readable.toWeb(fs.createReadStream(spooled.file)) as ReadableStream<Uint8Array>,
        signal: controller.signal,
        cache: 'no-store',
        // Required by Node's fetch for a streamed request body.
        duplex: 'half',
      } as RequestInit & { duplex: 'half' });
    } catch (e) {
      const timedOut = e instanceof Error && e.name === 'AbortError';
      return NextResponse.json(
        { detail: timedOut ? 'The engine took too long to store the file.' : 'The Rust engine is not reachable.' },
        { status: timedOut ? 504 : 502 },
      );
    } finally {
      clearTimeout(timer);
    }
    const text = await upstream.text();
    if (upstream.status === 201) {
      let uploadId = '';
      try {
        uploadId = String(JSON.parse(text).id ?? '');
      } catch {
        /* the engine's body is passed on as is */
      }
      await audit(
        {
          action: 'dataset.uploaded',
          actor: { id: auth.user.id, email: auth.user.email },
          workspaceId: workspace.id,
          target: { type: 'upload', id: uploadId },
          details: { filename, size: spooled.size, sha256: spooled.sha256 },
        },
        meta,
      );
    }
    return new NextResponse(text, {
      status: upstream.status,
      headers: {
        'Content-Type': upstream.headers.get('content-type') || 'application/json',
        'Cache-Control': 'no-store',
        'X-Request-Id': upstream.headers.get('x-request-id') || requestId,
      },
    });
  } finally {
    await fs.promises.rm(spooled.file, { force: true });
  }
}
