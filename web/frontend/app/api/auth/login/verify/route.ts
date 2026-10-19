import { NextResponse } from 'next/server';
import { record as audit, requestMeta, type RequestMeta } from '@/lib/server/audit';
import { checkLoginCode, findUser, publicUser } from '@/lib/server/authStore';
import { checkMfaChallenge, consumeMfaChallenge, verifySecondFactor } from '@/lib/server/mfa';
import { clientIp, hitAll, tooMany } from '@/lib/server/rateLimit';
import { startSession } from '@/lib/server/session';

/**
 * Second step of a two-step login. Reached only after the password was correct:
 *   - emailed code:        { email, code }               (the code was only sent after the password)
 *   - authenticator app:   { email, code, challenge }    (challenge issued by the password step;
 *                          code is the app's 6 digits or a recovery code)
 */
export async function POST(req: Request) {
  try {
    const { email, code, challenge } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || !email || !code) {
      return NextResponse.json({ ok: false, error: 'Email and code are required.' }, { status: 400 });
    }
    // Per IP and per account (each code also locks after 5 wrong guesses).
    const cleanEmail = email.trim().toLowerCase();
    const limit = await hitAll([
      [`login-verify:ip:${clientIp(req)}`, 30, 15 * 60_000, 'verification attempts'],
      [`login-verify:email:${cleanEmail}`, 10, 15 * 60_000, 'verification attempts for this account'],
    ]);
    if (!limit.ok) return tooMany(limit.retryAfter, limit.what);
    const meta = requestMeta(req);

    if (typeof challenge === 'string' && challenge) return verifyWithAuthenticator(req, cleanEmail, code, challenge, meta);

    const { result } = await checkLoginCode(cleanEmail, code);
    if (result === 'wrong' || result === 'locked') {
      await audit({ action: 'auth.login.failure', outcome: result === 'locked' ? 'denied' : 'failure', actor: { email: cleanEmail }, details: { reason: result === 'locked' ? 'code_locked' : 'wrong_code', step: 'email_code' } }, meta);
    }
    if (result === 'missing') {
      return NextResponse.json({ ok: false, error: 'This login code has expired. Please log in again.' }, { status: 400 });
    }
    if (result === 'locked') {
      return NextResponse.json({ ok: false, error: 'Too many incorrect attempts. Please log in again.' }, { status: 429 });
    }
    if (result === 'wrong') {
      return NextResponse.json({ ok: false, error: 'Invalid code. Please check your email.' }, { status: 400 });
    }

    const record = await findUser(cleanEmail);
    if (!record) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
    if (record.status !== 'active') return NextResponse.json({ ok: false, error: 'This account has been disabled. Contact support.' }, { status: 403 });
    const session = await startSession(record, req, { notifyNewDevice: true });
    await audit({ action: 'auth.login.success', actor: record, target: { type: 'session', id: session.id }, details: { method: 'password+email_code' } }, meta);
    return NextResponse.json({ ok: true, user: publicUser(record) });
  } catch (err) {
    console.error('[LOGIN VERIFY ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to verify code.' }, { status: 500 });
  }
}

async function verifyWithAuthenticator(req: Request, email: string, code: string, challenge: string, meta: RequestMeta) {
  const state = await checkMfaChallenge(email, challenge);
  if (state !== 'ok') {
    if (state === 'locked') await audit({ action: 'auth.login.failure', outcome: 'denied', actor: { email }, details: { reason: 'challenge_locked', step: 'authenticator' } }, meta);
    return NextResponse.json(
      { ok: false, error: state === 'locked' ? 'Too many incorrect attempts. Please log in again.' : 'This sign-in has expired. Please log in again.' },
      { status: state === 'locked' ? 429 : 400 },
    );
  }
  const record = await findUser(email);
  if (!record) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });

  const second = await verifySecondFactor(record.id, code);
  if (!second.ok) {
    await audit({ action: 'auth.login.failure', outcome: 'failure', actor: record, details: { reason: 'wrong_code', step: 'authenticator' } }, meta);
    return NextResponse.json({ ok: false, error: 'That code doesn’t match. Use the newest code from your app, or a recovery code.' }, { status: 400 });
  }
  if (!(await consumeMfaChallenge(email, challenge))) {
    return NextResponse.json({ ok: false, error: 'This sign-in has expired. Please log in again.' }, { status: 400 });
  }
  if (record.status !== 'active') return NextResponse.json({ ok: false, error: 'This account has been disabled. Contact support.' }, { status: 403 });

  if (second.method === 'recovery_code') {
    await audit({ action: 'auth.recovery_code.used', actor: record, target: { type: 'user', id: record.id }, details: { left: second.recoveryCodesLeft } }, meta);
  }
  const session = await startSession(record, req, { notifyNewDevice: true });
  await audit({ action: 'auth.login.success', actor: record, target: { type: 'session', id: session.id }, details: { method: `password+${second.method}` } }, meta);
  return NextResponse.json({
    ok: true,
    user: publicUser(record),
    ...(second.method === 'recovery_code' ? { recoveryCodesLeft: second.recoveryCodesLeft } : {}),
  });
}
