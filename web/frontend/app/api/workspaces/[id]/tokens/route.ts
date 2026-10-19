import { NextResponse } from 'next/server';
import { createToken, listTokens, TokenError } from '@/lib/server/apiTokens';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { hit, tooMany } from '@/lib/server/rateLimit';
import { requireRecentAuth, requireWorkspace } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';
type Ctx = { params: { id: string } };

/** Live API tokens for this workspace: owners see all of them, everyone else only their own. */
export async function GET(_req: Request, { params }: Ctx) {
  const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;
  return NextResponse.json({ ok: true, role: workspace.role, tokens: await listTokens(workspace, auth.user.id) }, { headers: { 'Cache-Control': 'no-store' } });
}

/**
 * Make a token, shown once. Any member can make a personal token (read-only for viewers);
 * workspace tokens are owner-only (checked in createToken). Needs a fresh "confirm it's you".
 */
export async function POST(req: Request, { params }: Ctx) {
  const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  const limit = await hit(`api-token:create:${auth.user.id}`, 20, 60 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'new tokens');
  try {
    const body = await req.json().catch(() => ({}));
    const { token, info } = await createToken({ by: auth.user, workspace, kind: body.kind, name: body.name, scopes: body.scopes, expiresInDays: body.expiresInDays });
    await audit(
      {
        action: 'api_token.created',
        actor: auth.user,
        workspaceId: workspace.id,
        target: { type: 'api_token', id: info.id },
        details: { kind: info.kind, name: info.name, scopes: info.scopes, expiresAt: new Date(info.expiresAt).toISOString() },
      },
      requestMeta(req),
    );
    return NextResponse.json({ ok: true, token, info }, { status: 201, headers: { 'Cache-Control': 'no-store' } });
  } catch (err) {
    if (err instanceof TokenError) return NextResponse.json({ ok: false, error: err.message }, { status: err.status });
    console.error('[API TOKEN ERROR] create:', err instanceof Error ? err.message : err);
    return NextResponse.json({ ok: false, error: 'Could not create the token.' }, { status: 500 });
  }
}
