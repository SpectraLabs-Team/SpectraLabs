import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireRecentAuth, requireWorkspace } from '@/lib/server/requireUser';
import { transferOwnership } from '@/lib/server/team';
import { teamErrorResponse } from '@/lib/server/teamRoute';

/** Make another member the owner; you become an editor. Owners only, with a fresh "confirm it's you". */
export async function POST(req: Request, { params }: { params: { id: string } }) {
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  try {
    const { userId } = await req.json().catch(() => ({}));
    if (typeof userId !== 'string' || !/^[0-9a-f-]{36}$/i.test(userId)) return NextResponse.json({ ok: false, error: 'Choose a member.' }, { status: 400 });
    await transferOwnership(workspace.id, auth.user.id, userId);
    await audit({ action: 'workspace.ownership_transferred', actor: auth.user, workspaceId: workspace.id, target: { type: 'user', id: userId }, details: { from: auth.user.id, to: userId } }, requestMeta(req));
    return NextResponse.json({ ok: true });
  } catch (err) {
    return teamErrorResponse(err, 'transfer ownership');
  }
}
