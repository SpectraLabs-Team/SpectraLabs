import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { confirmTotpEnrollment, disableTotp, getMfaStatus, MfaError, regenerateRecoveryCodes, startTotpEnrollment } from '@/lib/server/mfa';
import { toQrSvg } from '@/lib/server/qr';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireRecentAuth, requireUser } from '@/lib/server/requireUser';
import { VaultConfigError } from '@/lib/server/vault';

export const dynamic = 'force-dynamic';

/** Authenticator-app status for the signed-in account. */
export async function GET() {
  const { record, res } = await requireUser();
  if (res) return res;
  return NextResponse.json({ ok: true, status: await getMfaStatus(record.id) });
}

/**
 * { action: 'start' }                 → new secret + QR code (needs a recent "confirm it's you")
 * { action: 'confirm', code }         → turn it on; returns 10 recovery codes, shown once
 * { action: 'recovery-codes' }        → replace the recovery codes (needs a recent confirm)
 */
export async function POST(req: Request) {
  const { record, session, res } = await requireUser();
  if (res) return res;
  const meta = requestMeta(req);
  try {
    const { action, code } = await req.json().catch(() => ({}));

    if (action === 'start') {
      const stepUp = requireRecentAuth(session);
      if (stepUp) return stepUp;
      const { secret, uri } = await startTotpEnrollment(record);
      // The QR and the secret go to this browser only; never logged.
      return NextResponse.json({ ok: true, secret, uri, qrSvg: await toQrSvg(uri) }, { headers: { 'Cache-Control': 'no-store' } });
    }

    if (action === 'confirm') {
      const limit = await hit(`totp-confirm:${record.id}:${clientIp(req)}`, 10, 15 * 60_000);
      if (!limit.ok) return tooMany(limit.retryAfter, 'attempts');
      const recoveryCodes = await confirmTotpEnrollment(record.id, String(code ?? ''));
      await audit({ action: 'auth.totp.enabled', actor: record, target: { type: 'user', id: record.id } }, meta);
      return NextResponse.json({ ok: true, recoveryCodes, status: await getMfaStatus(record.id) }, { headers: { 'Cache-Control': 'no-store' } });
    }

    if (action === 'recovery-codes') {
      const stepUp = requireRecentAuth(session);
      if (stepUp) return stepUp;
      const recoveryCodes = await regenerateRecoveryCodes(record.id);
      await audit({ action: 'auth.recovery_codes.regenerated', actor: record, target: { type: 'user', id: record.id } }, meta);
      return NextResponse.json({ ok: true, recoveryCodes, status: await getMfaStatus(record.id) }, { headers: { 'Cache-Control': 'no-store' } });
    }

    return NextResponse.json({ ok: false, error: 'Unknown action.' }, { status: 400 });
  } catch (err) {
    if (err instanceof MfaError) return NextResponse.json({ ok: false, error: err.message }, { status: err.code === 'bad_code' ? 400 : 409 });
    if (err instanceof VaultConfigError) return NextResponse.json({ ok: false, error: 'Authenticator setup isn’t configured on this server (SECRETS_MASTER_KEY).' }, { status: 503 });
    console.error('[TOTP ERROR]', err instanceof Error ? err.name : 'unknown');
    return NextResponse.json({ ok: false, error: 'Could not update the authenticator app.' }, { status: 500 });
  }
}

/** Remove the authenticator app. Sign-in falls back to emailed codes. Needs a recent confirm. */
export async function DELETE(req: Request) {
  const { record, session, res } = await requireUser();
  if (res) return res;
  const stepUp = requireRecentAuth(session);
  if (stepUp) return stepUp;
  const removed = await disableTotp(record.id);
  if (!removed) return NextResponse.json({ ok: false, error: 'No authenticator app is set up.' }, { status: 404 });
  await audit({ action: 'auth.totp.disabled', actor: record, target: { type: 'user', id: record.id } }, requestMeta(req));
  return NextResponse.json({ ok: true, status: await getMfaStatus(record.id) });
}
