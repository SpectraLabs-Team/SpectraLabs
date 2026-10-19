import { NextResponse } from 'next/server';
import { checkSignupCode, createUser, findUser, publicUser } from '@/lib/server/authStore';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { setSessionCookie } from '@/lib/server/session';

export async function POST(req: Request) {
  try {
    const { email, code } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || !email || !code) {
      return NextResponse.json({ ok: false, error: 'Email and 6-digit code are required.' }, { status: 400 });
    }

    const ipLimit = hit(`verify:ip:${clientIp(req)}`, 30, 15 * 60_000);
    if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'verification attempts');

    const cleanEmail = email.trim().toLowerCase();
    const { result, entry } = checkSignupCode(cleanEmail, code);
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
    let userRecord = findUser(cleanEmail);
    if (!userRecord && entry.passwordHash) userRecord = createUser(cleanEmail, entry.passwordHash, entry.name, entry.twoFactor !== false);
    if (!userRecord) {
      return NextResponse.json({ ok: false, error: 'Your sign-up session expired. Please start again.' }, { status: 400 });
    }

    const user = publicUser(userRecord);
    setSessionCookie(user);
    return NextResponse.json({ ok: true, user });
  } catch (err) {
    console.error('[VERIFY-OTP ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to verify code.' }, { status: 500 });
  }
}
