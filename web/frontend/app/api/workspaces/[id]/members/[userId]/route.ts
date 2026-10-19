import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireRecentAuth, requireWorkspace } from '@/lib/server/requireUser';
import { changeRole, removeMember } from '@/lib/server/team';
import { teamErrorResponse } from '@/lib/server/teamRoute';

type Ctx = { params: { id: string; userId: string } };
const validId = (id: string) => /^[0-9a-f-]{36}$/i.test(id);

/** Change a member's role. Owners only, with a fresh "confirm it's you". Keeps at least one owner. */
export async function PATCH(req: Request, { params }: Ctx) {
  if (!validId(params.userId)) return NextResponse.json({ ok: false, error: 'Member not found.' }, { status: 404 });
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  try {
    const { role } = await req.json().catch(() => ({}));
    const change = await changeRole(workspace.id, params.userId, String(role ?? ''));
    if (change.from !== change.to) {
      await audit({ action: 'membership.role_changed', actor: auth.user, workspaceId: workspace.id, target: { type: 'user', id: params.userId }, details: change }, requestMeta(req));
    }
    return NextResponse.json({ ok: true, ...change });
  } catch (err) {
    return teamErrorResponse(err, 'change the role');
  }
}

/**
 * Remove a member (owners, with a fresh "confirm it's you"), or leave the workspace yourself (any
 * member). The last owner can't leave or be removed.
 */
export async function DELETE(req: Request, { params }: Ctx) {
  if (!validId(params.userId)) return NextResponse.json({ ok: false, error: 'Member not found.' }, { status: 404 });
  const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;
  const leaving = params.userId === auth.user.id;
  if (!leaving) {
    if (workspace.role !== 'owner') return NextResponse.json({ ok: false, error: 'Only owners can remove members.' }, { status: 403 });
    const stepUp = requireRecentAuth(auth.session);
    if (stepUp) return stepUp;
  }
  try {
    const { role } = await removeMember(workspace.id, params.userId);
    await audit(
      { action: leaving ? 'membership.left' : 'membership.removed', actor: auth.user, workspaceId: workspace.id, target: { type: 'user', id: params.userId }, details: { role } },
      requestMeta(req),
    );
    return NextResponse.json({ ok: true });
  } catch (err) {
    return teamErrorResponse(err, leaving ? 'leave the workspace' : 'remove the member');
  }
}
