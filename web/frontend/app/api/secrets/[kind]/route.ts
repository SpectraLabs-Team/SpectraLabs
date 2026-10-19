import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireRecentAuth, requireTwoFactorOn, requireWorkspace } from '@/lib/server/requireUser';
import { deleteSecret, isSecretKind, listSecrets, putSecret, SecretValidationError, VaultConfigError } from '@/lib/server/vault';

export const dynamic = 'force-dynamic';

type Ctx = { params: { kind: string } };

/**
 * Add or replace a broker key in the active workspace. Owners only. Write-only: the response
 * carries metadata (last 4 characters, dates), never the key. The request body is never logged.
 */
export async function PUT(req: Request, { params }: Ctx) {
  if (!isSecretKind(params.kind)) return NextResponse.json({ ok: false, error: 'Unknown key type.' }, { status: 404 });
  const { auth, workspace, res } = await requireWorkspace('owner');
  if (res) return res;
  // Anyone who adds a broker key must use two-step login, and confirm it's them right now.
  const need2fa = requireTwoFactorOn(auth.user, 'adding a broker key');
  if (need2fa) return need2fa;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  const limit = await hit(`secret-put:${auth.user.id}:${clientIp(req)}`, 10, 60 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'key changes');
  try {
    const body = await req.json().catch(() => null);
    const existed = (await listSecrets(workspace.id)).some((s) => s.kind === params.kind);
    const secret = await putSecret(workspace.id, params.kind, body, auth.user.id);
    // Metadata only: the kind and the last 4 characters of the key id, never the key.
    await audit({ action: existed ? 'secret.replaced' : 'secret.added', actor: auth.user, workspaceId: workspace.id, target: { type: 'secret', id: params.kind }, details: { hint: secret.hint } }, requestMeta(req));
    return NextResponse.json({ ok: true, secret });
  } catch (err) {
    if (err instanceof SecretValidationError) return NextResponse.json({ ok: false, error: err.message }, { status: 400 });
    if (err instanceof VaultConfigError) {
      console.error('[SECRETS CONFIG]', err.message);
      return NextResponse.json({ ok: false, error: 'Key storage is not configured on this server (SECRETS_MASTER_KEY).' }, { status: 503 });
    }
    // Log the error type only: messages from lower layers must never risk echoing key material.
    console.error('[SECRETS PUT ERROR]', err instanceof Error ? err.name : 'unknown');
    return NextResponse.json({ ok: false, error: 'Could not save the key.' }, { status: 500 });
  }
}

/** Delete a broker key from the active workspace. Owners only. */
export async function DELETE(req: Request, { params }: Ctx) {
  if (!isSecretKind(params.kind)) return NextResponse.json({ ok: false, error: 'Unknown key type.' }, { status: 404 });
  const { auth, workspace, res } = await requireWorkspace('owner');
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  const deleted = await deleteSecret(workspace.id, params.kind);
  if (!deleted) return NextResponse.json({ ok: false, error: 'No key stored.' }, { status: 404 });
  await audit({ action: 'secret.deleted', actor: auth.user, workspaceId: workspace.id, target: { type: 'secret', id: params.kind } }, requestMeta(req));
  return NextResponse.json({ ok: true });
}
