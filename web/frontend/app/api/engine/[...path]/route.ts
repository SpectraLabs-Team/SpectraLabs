import { NextResponse } from 'next/server';
import { hit, tooMany } from '@/lib/server/rateLimit';
import { getSession } from '@/lib/server/session';

/**
 * Authenticated proxy to the Rust engine. The browser never talks to spectra-api directly:
 * every call lands here, the session cookie is checked, and only then is it forwarded to
 * ENGINE_URL (server-side env, default http://127.0.0.1:8000, the engine's default bind).
 */
export const dynamic = 'force-dynamic';
export const runtime = 'nodejs';

const ENGINE_URL = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/+$/, '');
const MAX_BODY = 2 * 1024 * 1024; // portfolio CSVs are small; refuse anything bigger

/** Allowlist: method + path. Anything not listed is a 404, so new engine routes stay private until added. */
const ROUTES: Record<string, { method: 'GET' | 'POST'; public?: boolean; timeoutMs: number; heavy?: boolean }> = {
  health: { method: 'GET', public: true, timeoutMs: 4_000 },
  'api/sample-portfolio': { method: 'GET', timeoutMs: 10_000 },
  'api/telemetry/cpu': { method: 'GET', timeoutMs: 4_000 },
  'api/risk': { method: 'POST', timeoutMs: 60_000 },
  'api/stress': { method: 'POST', timeoutMs: 60_000 },
  'api/benchmark': { method: 'POST', timeoutMs: 130_000, heavy: true },
  'api/alpaca/account': { method: 'GET', timeoutMs: 10_000 },
};

async function proxy(req: Request, path: string[], method: 'GET' | 'POST') {
  const key = path.join('/');
  const route = ROUTES[key];
  if (!route || route.method !== method) {
    return NextResponse.json({ detail: 'Not found.' }, { status: 404 });
  }

  if (!route.public) {
    const session = getSession();
    if (!session) return NextResponse.json({ detail: 'Sign in to use the engine.' }, { status: 401 });

    // Per-account compute budget: generous for interactive use, stops scripted hammering.
    const limit = route.heavy ? hit(`engine:heavy:${session.email}`, 40, 10 * 60_000) : hit(`engine:${session.email}`, 240, 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'engine requests');
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
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
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
