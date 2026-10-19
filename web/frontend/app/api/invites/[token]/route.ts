import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { getSession } from '@/lib/server/session';
import { acceptInvite, isInviteToken, previewInvite } from '@/lib/server/team';
import { teamErrorResponse } from '@/lib/server/teamRoute';
import { setActiveWorkspace } from '@/lib/server/workspaces';

export const dynamic = 'force-dynamic';
type Ctx = { params: { token: string } };

/** What an invite is for (workspace, role, masked email), for the invite page. Public. */
export async function GET(req: Request, { params }: Ctx) {
  const limit = await hit(`invite-view:${clientIp(req)}`, 60, 15 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'requests');
  if (!isInviteToken(params.token)) return NextResponse.json({ ok: true, invite: { status: 'missing' } });
  return NextResponse.json({ ok: true, invite: await previewInvite(params.token) }, { headers: { 'Cache-Control': 'no-store' } });
}

/** Accept: must be signed in with the invited email. Joins the workspace and makes it active here. */
export async function POST(req: Request, { params }: Ctx) {
  const auth = await getSession();
  if (!auth) return NextResponse.json({ ok: false, error: 'Sign in to accept the invite.' }, { status: 401 });
  const limit = await hit(`invite-accept:${auth.user.id}`, 20, 15 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'attempts');
  try {
    const joined = await acceptInvite(params.token, auth.user);
    await setActiveWorkspace(auth.session.id, auth.user.id, joined.workspaceId);
    await audit(
      { action: 'membership.added', actor: auth.user, workspaceId: joined.workspaceId, target: { type: 'user', id: auth.user.id }, details: { role: joined.role, via: 'invite', invite: joined.inviteId } },
      requestMeta(req),
    );
    return NextResponse.json({ ok: true, workspace: { id: joined.workspaceId, name: joined.workspaceName, role: joined.role } });
  } catch (err) {
    return teamErrorResponse(err, 'accept the invite');
  }
}
