import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireWorkspace } from '@/lib/server/requireUser';
import { revokeInvite } from '@/lib/server/team';

/** Cancel an open invite. Owners only. */
export async function DELETE(req: Request, { params }: { params: { id: string; inviteId: string } }) {
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  if (!(await revokeInvite(workspace.id, params.inviteId))) return NextResponse.json({ ok: false, error: 'Invite not found.' }, { status: 404 });
  await audit({ action: 'membership.invite_revoked', actor: auth.user, workspaceId: workspace.id, target: { type: 'invite', id: params.inviteId } }, requestMeta(req));
  return NextResponse.json({ ok: true });
}
