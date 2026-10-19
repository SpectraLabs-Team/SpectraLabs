import { NextResponse } from 'next/server';
import { findUser, setTwoFactor, twoFactorEnforced, twoFactorOn, verifyPassword } from '@/lib/server/authStore';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { getSession } from '@/lib/server/session';

export const dynamic = 'force-dynamic';

/** Current login-security setting for the signed-in account. */
export async function GET() {
  const session = getSession();
  const record = session && findUser(session.email);
  if (!record) return NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 });
  return NextResponse.json({ ok: true, enabled: twoFactorOn(record), enforced: twoFactorEnforced(record) });
}

/**
 * Turn 2FA on or off. Turning it OFF lowers the account's protection, so it needs the current
 * password: someone at an unlocked, signed-in browser can't quietly downgrade the account.
 */
export async function POST(req: Request) {
  try {
    const session = getSession();
    const record = session && findUser(session.email);
    if (!record) return NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 });

    const { enabled, password } = await req.json();
    if (typeof enabled !== 'boolean') return NextResponse.json({ ok: false, error: 'Invalid request.' }, { status: 400 });
    if (!enabled && twoFactorEnforced(record)) {
      return NextResponse.json({ ok: false, error: 'Two-factor login is required for admin accounts.' }, { status: 403 });
    }
    if (!enabled) {
      const limit = hit(`2fa-off:${record.email}:${clientIp(req)}`, 5, 15 * 60_000);
      if (!limit.ok) return tooMany(limit.retryAfter, 'password attempts');
      if (typeof password !== 'string' || !verifyPassword(password, record.passwordHash)) {
        return NextResponse.json({ ok: false, error: 'Incorrect password.' }, { status: 401 });
      }
    }
    const updated = setTwoFactor(record.email, enabled);
    return NextResponse.json({ ok: true, enabled: !!updated && twoFactorOn(updated), enforced: !!updated && twoFactorEnforced(updated) });
  } catch (err) {
    console.error('[2FA SETTING ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not update the setting.' }, { status: 500 });
  }
}
