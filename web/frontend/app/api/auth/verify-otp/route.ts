import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { checkSignupCode, createUser, findUser, publicUser } from '@/lib/server/authStore';
import { clientIp, hitAll, tooMany } from '@/lib/server/rateLimit';
import { startSession } from '@/lib/server/session';

export async function POST(req: Request) {
  try {
    const { email, code } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || !email || !code) {
      return NextResponse.json({ ok: false, error: 'Email and 6-digit code are required.' }, { status: 400 });
    }

    // Per IP and per account (each code also locks after 5 wrong guesses).
    const cleanEmail = email.trim().toLowerCase();
    const limit = await hitAll([
      [`verify:ip:${clientIp(req)}`, 30, 15 * 60_000, 'verification attempts'],
      [`verify:email:${cleanEmail}`, 10, 15 * 60_000, 'verification attempts for this email'],
    ]);
    if (!limit.ok) return tooMany(limit.retryAfter, limit.what);

    const { result, entry } = await checkSignupCode(cleanEmail, code);
    if (result === 'missing') {
      return NextResponse.json({ ok: false, error: 'Verification code has expired or does not exist. Please request a new one.' }, { status: 400 });
    }
    if (result === 'locked') {
      return NextResponse.json({ ok: false, error: 'Too many incorrect attempts. Please request a new code.' }, { status: 429 });
    }
    if (result === 'wrong' || !entry) {
      return NextResponse.json({ ok: false, error: 'Invalid verification code. Please check your email.' }, { status: 400 });
    }

    // Register the user (or reuse an existing record). Never issue a session without an account.
    let userRecord = await findUser(cleanEmail);
    const isNew = !userRecord;
    if (!userRecord && entry.passwordHash) userRecord = await createUser(cleanEmail, entry.passwordHash, entry.name, entry.twoFactor !== false);
    if (!userRecord) {
      return NextResponse.json({ ok: false, error: 'Your sign-up session expired. Please start again.' }, { status: 400 });
    }

    const meta = requestMeta(req);
    if (isNew) await audit({ action: 'auth.signup', actor: userRecord, target: { type: 'user', id: userRecord.id }, details: { role: userRecord.role, twoFactor: !!userRecord.twoFactor } }, meta);
    const session = await startSession(userRecord, req);
    await audit({ action: 'auth.login.success', actor: userRecord, target: { type: 'session', id: session.id }, details: { method: 'signup' } }, meta);
    return NextResponse.json({ ok: true, user: publicUser(userRecord) });
  } catch (err) {
    console.error('[VERIFY-OTP ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to verify code.' }, { status: 500 });
  }
}
