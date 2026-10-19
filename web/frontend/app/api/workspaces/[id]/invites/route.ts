import { NextResponse } from 'next/server';
import { appUrl } from '@/lib/server/appUrl';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { sendInviteEmail } from '@/lib/server/email';
import { hit, tooMany } from '@/lib/server/rateLimit';
import { requireRecentAuth, requireWorkspace } from '@/lib/server/requireUser';
import { createInvite, listInvites } from '@/lib/server/team';
import { teamErrorResponse } from '@/lib/server/teamRoute';

export const dynamic = 'force-dynamic';
type Ctx = { params: { id: string } };

/** Open invites for this workspace. Owners only. */
export async function GET(_req: Request, { params }: Ctx) {
  const { workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  return NextResponse.json({ ok: true, invites: await listInvites(workspace.id) });
}

/**
 * Invite someone by email as an editor or viewer. Owners only, with a fresh "confirm it's you"
 * (it grants access). The link is emailed; it's also returned so the owner can share it another
 * way if the email doesn't arrive (it only works for the invited address).
 */
export async function POST(req: Request, { params }: Ctx) {
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  const limit = await hit(`invite:${workspace.id}:${auth.user.id}`, 30, 60 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'invites');
  try {
    const { email, role } = await req.json().catch(() => ({}));
    const base = appUrl(req);
    const { invite, token } = await createInvite(workspace.id, email, role, auth.user.id);
    const link = `${base}/invite/${token}`;
    await audit(
      { action: 'membership.invited', actor: auth.user, workspaceId: workspace.id, target: { type: 'invite', id: invite.id }, details: { email: invite.email, role: invite.role } },
      requestMeta(req),
    );
    let emailed = true;
    try {
      await sendInviteEmail({ to: invite.email, inviter: auth.user.name || auth.user.email, workspace: workspace.name, role: invite.role, link });
    } catch (err) {
      emailed = false;
      console.error('[INVITE SMTP ERROR]', err instanceof Error ? err.message : err);
    }
    return NextResponse.json({ ok: true, invite, link, emailed }, { status: 201, headers: { 'Cache-Control': 'no-store' } });
  } catch (err) {
    return teamErrorResponse(err, 'create the invite');
  }
}
