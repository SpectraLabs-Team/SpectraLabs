import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireRecentAuth, requireWorkspace } from '@/lib/server/requireUser';
import { cleanWorkspaceName, deleteWorkspace, listMembers, MAX_WORKSPACE_NAME, PersonalWorkspaceError, renameWorkspace } from '@/lib/server/workspaces';

export const dynamic = 'force-dynamic';

type Ctx = { params: { id: string } };

/** Workspace details and members. Any member (viewer and up). */
export async function GET(_req: Request, { params }: Ctx) {
  const { workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;
  return NextResponse.json({ ok: true, workspace, members: await listMembers(workspace.id) });
}

/** Rename. Owners only. */
export async function PATCH(req: Request, { params }: Ctx) {
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  try {
    const { name } = await req.json();
    const clean = cleanWorkspaceName(name);
    if (!clean) return NextResponse.json({ ok: false, error: `Name must be 1–${MAX_WORKSPACE_NAME} characters.` }, { status: 400 });
    await renameWorkspace(workspace.id, clean);
    await audit({ action: 'workspace.renamed', actor: auth.user, workspaceId: workspace.id, target: { type: 'workspace', id: workspace.id }, details: { from: workspace.name, to: clean } }, requestMeta(req));
    return NextResponse.json({ ok: true, workspace: { ...workspace, name: clean } });
  } catch (err) {
    console.error('[WORKSPACE RENAME ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not rename the workspace.' }, { status: 500 });
  }
}

/** Delete a team workspace and everything in it. Owners only; personal workspaces can't be deleted. */
export async function DELETE(req: Request, { params }: Ctx) {
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  try {
    await deleteWorkspace(workspace.id);
    await audit({ action: 'workspace.deleted', actor: auth.user, workspaceId: workspace.id, target: { type: 'workspace', id: workspace.id }, details: { name: workspace.name } }, requestMeta(req));
    return NextResponse.json({ ok: true });
  } catch (err) {
    if (err instanceof PersonalWorkspaceError) return NextResponse.json({ ok: false, error: err.message }, { status: 400 });
    console.error('[WORKSPACE DELETE ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not delete the workspace.' }, { status: 500 });
  }
}
