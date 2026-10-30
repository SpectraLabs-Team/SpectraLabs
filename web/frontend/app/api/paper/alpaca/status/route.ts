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
    if (secret) {
      return NextResponse.json({
        ok: true,
        connected: true,
        hint: secret.hint,
        keyIdHint: secret.hint,
        lastUsedAt: secret.lastUsedAt,
        updatedAt: secret.updatedAt,
      });
    }

    // Fallback: check environment variables if configured on the server
    const envKey = process.env.ALPACA_API_KEY_ID?.trim();
    const envSecret = process.env.ALPACA_API_SECRET_KEY?.trim();
    if (envKey && envSecret) {
      const hint = envKey.slice(-4);
      return NextResponse.json({
        ok: true,
        connected: true,
        hint,
        keyIdHint: hint,
        source: 'env',
      });
    }

    return NextResponse.json({ ok: true, connected: false });
  } catch {
    // If vault / DB check threw, still check environment variables
    const envKey = process.env.ALPACA_API_KEY_ID?.trim();
    const envSecret = process.env.ALPACA_API_SECRET_KEY?.trim();
    if (envKey && envSecret) {
      const hint = envKey.slice(-4);
      return NextResponse.json({
        ok: true,
        connected: true,
        hint,
        keyIdHint: hint,
        source: 'env',
      });
    }
    return NextResponse.json({ ok: true, connected: false });
  }
}
