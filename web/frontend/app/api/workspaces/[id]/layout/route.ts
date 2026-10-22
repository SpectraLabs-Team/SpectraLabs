import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireWorkspace } from '@/lib/server/requireUser';
import {
  getWorkspaceLayout,
  saveUserLayout,
  saveWorkspaceDefaultLayout,
} from '@/lib/server/dashboardLayoutStore';
import type { WidgetLayoutItem } from '@/lib/dashboardLayout';

export const dynamic = 'force-dynamic';

type Ctx = { params: Promise<{ id: string }> };

/**
 * GET: Retrieve the workspace layout for the current user.
 * Open to any workspace member (viewer, editor, owner).
 */
export async function GET(_req: Request, props: Ctx) {
  const params = await props.params;
  const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;

  try {
    const result = await getWorkspaceLayout(workspace.id, auth.user?.id);
    return NextResponse.json({
      ok: true,
      layout: result.layout,
      isWorkspaceDefault: result.isWorkspaceDefault,
    });
  } catch (err) {
    console.error('[WORKSPACE LAYOUT GET ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not load layout from server.' }, { status: 500 });
  }
}

/**
 * PUT: Save layout to the server.
 * - If `asWorkspaceDefault: true`: requires Editor or Owner role. Publishes the layout to all members.
 * - Otherwise: saves the user's personal layout for this workspace across all their browsers.
 */
export async function PUT(req: Request, props: Ctx) {
  const params = await props.params;
  try {
    const body = (await req.json()) as { layout?: WidgetLayoutItem[]; asWorkspaceDefault?: boolean };
    const { layout, asWorkspaceDefault } = body;

    if (!Array.isArray(layout)) {
      return NextResponse.json({ ok: false, error: 'Invalid layout format.' }, { status: 400 });
    }

    if (asWorkspaceDefault) {
      // Publishing as workspace-wide default requires Editor or Owner role
      const { auth, workspace, res } = await requireWorkspace('editor', params.id);
      if (res) return res;

      await saveWorkspaceDefaultLayout(workspace.id, layout);
      await audit(
        {
          action: 'workspace.layout_published',
          actor: auth.user,
          workspaceId: workspace.id,
          target: { type: 'workspace', id: workspace.id },
          details: { widget_count: layout.length },
        },
        requestMeta(req)
      );

      return NextResponse.json({ ok: true, layout, isWorkspaceDefault: true });
    } else {
      // Saving personal layout requires Viewer role (any member)
      const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
      if (res) return res;

      await saveUserLayout(workspace.id, auth.user.id, layout);
      return NextResponse.json({ ok: true, layout, isWorkspaceDefault: false });
    }
  } catch (err) {
    console.error('[WORKSPACE LAYOUT PUT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not save layout to server.' }, { status: 500 });
  }
}
