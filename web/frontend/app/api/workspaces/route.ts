import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireTwoFactorOn, requireUser } from '@/lib/server/requireUser';
import { cleanWorkspaceName, createWorkspace, listWorkspaces, MAX_WORKSPACE_NAME, setActiveWorkspace } from '@/lib/server/workspaces';

export const dynamic = 'force-dynamic';

/** The signed-in user's workspaces (with their role in each) and which one is active. */
export async function GET() {
  const { record, workspace, res } = await requireUser();
  if (res) return res;
  return NextResponse.json({ ok: true, workspaces: await listWorkspaces(record.id), activeId: workspace.id });
}

/** Create a team workspace. The creator becomes its owner and it becomes the active workspace. */
export async function POST(req: Request) {
  const { record, session, res } = await requireUser();
  if (res) return res;
  try {
    // Owners of team workspaces must use two-step login.
    const need2fa = requireTwoFactorOn(record, 'creating a team workspace');
    if (need2fa) return need2fa;
    const limit = await hit(`ws-create:${record.id}:${clientIp(req)}`, 10, 60 * 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'new workspaces');

    const { name } = await req.json();
    const clean = cleanWorkspaceName(name);
    if (!clean) return NextResponse.json({ ok: false, error: `Name must be 1–${MAX_WORKSPACE_NAME} characters.` }, { status: 400 });

    const created = await createWorkspace(record.id, clean);
    await audit({ action: 'workspace.created', actor: record, workspaceId: created.id, target: { type: 'workspace', id: created.id }, details: { name: clean } }, requestMeta(req));
    await setActiveWorkspace(session.id, record.id, created.id);
    return NextResponse.json({ ok: true, workspace: created, activeId: created.id }, { status: 201 });
  } catch (err) {
    console.error('[WORKSPACE CREATE ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not create the workspace.' }, { status: 500 });
  }
}
