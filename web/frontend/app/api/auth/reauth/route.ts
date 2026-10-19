import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { verifyPassword } from '@/lib/server/authStore';
import { hasTotp, markReauthenticated, REAUTH_MAX_AGE_MS, verifySecondFactor } from '@/lib/server/mfa';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireUser } from '@/lib/server/requireUser';

/**
 * "Confirm it's you" before a sensitive action: the current password, or (if set up) an
 * authenticator / recovery code. Marks this session as recently authenticated for 10 minutes.
 */
export async function POST(req: Request) {
  const { record, session, res } = await requireUser();
  if (res) return res;
  const limit = await hit(`reauth:${record.id}:${clientIp(req)}`, 8, 15 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'attempts');
  const meta = requestMeta(req);
  try {
    const { password, code } = await req.json().catch(() => ({}));
    let ok = false;
    let method = '';
    if (typeof password === 'string' && password) {
      ok = (await verifyPassword(password, record.passwordHash)).ok;
      method = 'password';
    } else if (typeof code === 'string' && code && (await hasTotp(record.id))) {
      const r = await verifySecondFactor(record.id, code);
      ok = r.ok;
      method = r.ok ? r.method : 'code';
    } else {
      return NextResponse.json({ ok: false, error: 'Enter your password or an authenticator code.' }, { status: 400 });
    }
    await audit({ action: 'auth.reauth', outcome: ok ? 'success' : 'failure', actor: record, target: { type: 'session', id: session.id }, details: { method } }, meta);
    if (!ok) return NextResponse.json({ ok: false, error: method === 'password' ? 'Incorrect password.' : 'That code doesn’t match.' }, { status: 401 });
    await markReauthenticated(session.id);
    return NextResponse.json({ ok: true, validForMs: REAUTH_MAX_AGE_MS });
  } catch (err) {
    console.error('[REAUTH ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not confirm. Try again.' }, { status: 500 });
  }
}
