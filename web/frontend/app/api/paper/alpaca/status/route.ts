import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { listSecrets } from '@/lib/server/vault';

export const dynamic = 'force-dynamic';

/** Check whether Alpaca paper trading credentials are configured for the workspace. */
export async function GET() {
  const { workspace, res } = await requireWorkspace('viewer');
  if (res) return res;

  try {
    const secrets = await listSecrets(workspace.id);
    const secret = secrets.find((s) => s.kind === 'alpaca_paper');
    if (!secret) {
      return NextResponse.json({ ok: true, connected: false });
    }

    return NextResponse.json({
      ok: true,
      connected: true,
      hint: secret.hint,
      keyIdHint: secret.hint,
      lastUsedAt: secret.lastUsedAt,
      updatedAt: secret.updatedAt,
    });
  } catch {
    return NextResponse.json({ ok: true, connected: false });
  }
}
