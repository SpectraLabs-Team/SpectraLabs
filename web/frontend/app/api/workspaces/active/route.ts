import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { setActiveWorkspace } from '@/lib/server/workspaces';

export const dynamic = 'force-dynamic';

/** Switch this browser's active workspace. Any membership (viewer and up). */
export async function PUT(req: Request) {
  try {
    const { workspaceId } = await req.json().catch(() => ({}));
    if (typeof workspaceId !== 'string') return NextResponse.json({ ok: false, error: 'workspaceId is required.' }, { status: 400 });
    // Not a member → 404, the same answer as a workspace that doesn't exist.
    const { auth, workspace, res } = await requireWorkspace('viewer', workspaceId);
    if (res) return res;
    await setActiveWorkspace(auth.session.id, auth.user.id, workspace.id);
    return NextResponse.json({ ok: true, workspace, activeId: workspace.id });
  } catch (err) {
    console.error('[WORKSPACE SWITCH ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not switch workspace.' }, { status: 500 });
  }
}
