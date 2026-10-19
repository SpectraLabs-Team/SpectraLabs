import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { listSecrets, parseKeyring, SECRET_KINDS } from '@/lib/server/vault';

export const dynamic = 'force-dynamic';

/**
 * Broker keys stored for the active workspace: metadata only (kind, last 4 characters, dates).
 * Any member can see that a key exists; only owners can add, replace or delete one.
 */
export async function GET() {
  const { workspace, res } = await requireWorkspace('viewer');
  if (res) return res;
  let configured = true;
  try {
    parseKeyring(process.env.SECRETS_MASTER_KEY);
  } catch {
    configured = false;
  }
  return NextResponse.json({
    ok: true,
    workspace: { id: workspace.id, name: workspace.name, role: workspace.role },
    canManage: workspace.role === 'owner',
    configured,
    kinds: Object.entries(SECRET_KINDS).map(([kind, k]) => ({
      kind,
      label: k.label,
      fields: Object.entries(k.fields).map(([name, f]) => ({ name, label: f.label })),
    })),
    secrets: await listSecrets(workspace.id),
  });
}
