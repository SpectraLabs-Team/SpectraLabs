import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { deleteSecret } from '@/lib/server/vault';
import { record as audit, requestMeta } from '@/lib/server/audit';

export const dynamic = 'force-dynamic';

/** Disconnect Alpaca credentials from the workspace. */
export async function POST(req: Request) {
  const { auth, workspace, res } = await requireWorkspace('editor');
  if (res) return res;

  try {
    const deleted = await deleteSecret(workspace.id, 'alpaca_paper');
    if (deleted) {
      await audit(
        {
          action: 'secret.deleted',
          actor: auth.user,
          workspaceId: workspace.id,
          target: { type: 'secret', id: 'alpaca_paper' },
          details: { broker: 'alpaca_paper' },
        },
        requestMeta(req),
      );
    }

    return NextResponse.json({ ok: true, connected: false });
  } catch (err) {
    console.error('[ALPACA DISCONNECT ERROR]', err instanceof Error ? err.name : 'unknown');
    return NextResponse.json({ ok: false, error: 'Could not disconnect credentials.' }, { status: 500 });
  }
}
