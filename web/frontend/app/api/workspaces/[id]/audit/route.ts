import { NextResponse } from 'next/server';
import { auditResponse } from '@/lib/server/auditRoute';
import { requireWorkspace } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/**
 * A workspace's audit log: events recorded against this workspace (keys, settings, membership,
 * trading). Owners only: entries include members' IP addresses. Same filters and CSV export as
 * the admin view.
 */
export async function GET(req: Request, props: { params: Promise<{ id: string }> }) {
  const params = await props.params;
  const { auth, workspace, res } = await requireWorkspace('owner', params.id);
  if (res) return res;
  try {
    return await auditResponse(req, { workspaceId: workspace.id }, auth.user);
  } catch (err) {
    console.error('[WORKSPACE AUDIT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not load the audit log.' }, { status: 500 });
  }
}
