import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { setTwoFactor, twoFactorOn, verifyPassword } from '@/lib/server/authStore';
import { hasTotp, twoFactorRequirement } from '@/lib/server/mfa';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireUser } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** Current login-security setting for the signed-in account. */
export async function GET() {
  const { record, res } = await requireUser();
  if (res) return res;
  const required = await twoFactorRequirement(record);
  return NextResponse.json({ ok: true, enabled: twoFactorOn(record), enforced: !!required, reason: required, method: (await hasTotp(record.id)) ? 'totp' : 'email' });
}

/**
 * Turn 2FA on or off. Turning it OFF lowers the account's protection, so it needs the current
 * password: someone at an unlocked, signed-in browser can't quietly downgrade the account.
 */
export async function POST(req: Request) {
  try {
    const { record, res } = await requireUser();
    if (res) return res;

    const { enabled, password } = await req.json();
    if (typeof enabled !== 'boolean') return NextResponse.json({ ok: false, error: 'Invalid request.' }, { status: 400 });
    const required = await twoFactorRequirement(record);
    if (!enabled && required) return NextResponse.json({ ok: false, error: required }, { status: 403 });
    if (!enabled && (await hasTotp(record.id))) {
      return NextResponse.json({ ok: false, error: 'Remove the authenticator app first (Account → Authenticator app).' }, { status: 409 });
    }
    if (!enabled) {
      const limit = await hit(`2fa-off:${record.email}:${clientIp(req)}`, 5, 15 * 60_000);
      if (!limit.ok) return tooMany(limit.retryAfter, 'password attempts');
      if (typeof password !== 'string' || !(await verifyPassword(password, record.passwordHash)).ok) {
        return NextResponse.json({ ok: false, error: 'Incorrect password.' }, { status: 401 });
      }
    }
    const updated = await setTwoFactor(record.email, enabled);
    if (updated && !!record.twoFactor !== !!updated.twoFactor) {
      await audit({ action: updated.twoFactor ? 'auth.two_factor.enabled' : 'auth.two_factor.disabled', actor: record, target: { type: 'user', id: record.id } }, requestMeta(req));
    }
    return NextResponse.json({ ok: true, enabled: !!updated && twoFactorOn(updated), enforced: !!required, reason: required, method: (await hasTotp(record.id)) ? 'totp' : 'email' });
  } catch (err) {
    console.error('[2FA SETTING ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not update the setting.' }, { status: 500 });
  }
}
