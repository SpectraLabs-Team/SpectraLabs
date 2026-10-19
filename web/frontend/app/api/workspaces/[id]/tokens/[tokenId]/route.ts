import { NextResponse } from 'next/server';
import { revokeToken } from '@/lib/server/apiTokens';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireWorkspace } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** Revoke a token: your own, or (owners) any token of the workspace. Takes effect immediately. */
export async function DELETE(req: Request, { params }: { params: { id: string; tokenId: string } }) {
  const { auth, workspace, res } = await requireWorkspace('viewer', params.id);
  if (res) return res;
  const info = await revokeToken(workspace, params.tokenId, auth.user);
  if (!info) return NextResponse.json({ ok: false, error: 'Token not found.' }, { status: 404 });
  await audit(
    { action: 'api_token.revoked', actor: auth.user, workspaceId: workspace.id, target: { type: 'api_token', id: info.id }, details: { kind: info.kind, name: info.name, owner: info.owner?.email ?? null } },
    requestMeta(req),
  );
  return NextResponse.json({ ok: true });
}
