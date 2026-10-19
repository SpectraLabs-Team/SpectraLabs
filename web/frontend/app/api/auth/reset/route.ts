import { NextResponse } from 'next/server';
import { isPasswordValid } from '@/lib/auth-shared';
import { checkResetCode, hashPassword, publicUser, updatePassword } from '@/lib/server/authStore';
import { clientIp, hit, reset as resetLimit, tooMany } from '@/lib/server/rateLimit';
import { setSessionCookie } from '@/lib/server/session';

/**
 * Step 2 of password reset: code + new password. On success every older session for the
 * account stops working (see passwordChangedAt) and this browser is signed in fresh.
 */
export async function POST(req: Request) {
  try {
    const { email, code, password } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || typeof password !== 'string') {
      return NextResponse.json({ ok: false, error: 'Email, code and new password are required.' }, { status: 400 });
    }

    const ipLimit = hit(`reset-verify:ip:${clientIp(req)}`, 30, 15 * 60_000);
    if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'reset attempts');

    // Validate the password first so a weak password doesn't burn one of the code attempts.
    if (!isPasswordValid(password)) {
      return NextResponse.json(
        { ok: false, error: 'Password must be 8+ chars and contain at least 1 letter, 1 number, and 1 special character.' },
        { status: 400 },
      );
    }

    const cleanEmail = email.trim().toLowerCase();
    const { result } = checkResetCode(cleanEmail, code);
    if (result === 'missing') {
      return NextResponse.json({ ok: false, error: 'This reset code has expired. Please request a new one.' }, { status: 400 });
    }
    if (result === 'locked') {
      return NextResponse.json({ ok: false, error: 'Too many incorrect attempts. Please request a new code.' }, { status: 429 });
    }
    if (result === 'wrong') {
      return NextResponse.json({ ok: false, error: 'Invalid reset code. Please check your email.' }, { status: 400 });
    }

    const record = updatePassword(cleanEmail, hashPassword(password));
    if (!record) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });

    resetLimit(`login:fail:${cleanEmail}`); // a successful reset lifts any login lockout
    const user = publicUser(record);
    setSessionCookie(user);
    return NextResponse.json({ ok: true, user });
  } catch (err) {
    console.error('[RESET ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not reset the password.' }, { status: 500 });
  }
}
