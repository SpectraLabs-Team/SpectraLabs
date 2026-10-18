import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { clearPendingOtp, createSessionToken, createUser, findUser, getPendingOtp, publicUser } from '@/lib/server/authStore';

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, code } = body;

    if (!email || !code) {
      return NextResponse.json({ ok: false, error: 'Email and 6-digit code are required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();
    const pending = getPendingOtp(cleanEmail);

    if (!pending) {
      return NextResponse.json(
        { ok: false, error: 'Verification code has expired or does not exist. Please request a new one.' },
        { status: 400 },
      );
    }

    if (pending.attempts >= 5) {
      clearPendingOtp(cleanEmail);
      return NextResponse.json(
        { ok: false, error: 'Too many incorrect attempts. Please request a new code.' },
        { status: 429 },
      );
    }

    if (pending.code !== code.trim()) {
      pending.attempts += 1;
      return NextResponse.json(
        { ok: false, error: 'Invalid verification code. Please check your email.' },
        { status: 400 },
      );
    }

    // OTP matched!
    clearPendingOtp(cleanEmail);

    // Register user or get existing. Never issue a session for an email with no account.
    let userRecord = findUser(cleanEmail);
    if (!userRecord && pending.passwordHash) {
      userRecord = createUser(cleanEmail, pending.passwordHash, pending.name);
    }
    if (!userRecord) {
      return NextResponse.json(
        { ok: false, error: 'Your sign-up session expired. Please start again.' },
        { status: 400 },
      );
    }

    const userPayload = publicUser(userRecord);

    // Create session token and set HttpOnly cookie
    const token = createSessionToken(userPayload);
    const cookieStore = cookies();
    cookieStore.set('spectra_session', token, {
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax',
      path: '/',
      maxAge: 7 * 24 * 60 * 60, // 7 days
    });

    return NextResponse.json({ ok: true, user: userPayload });
  } catch (err: any) {
    console.error('[VERIFY-OTP ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to verify code.' }, { status: 500 });
  }
}
