import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { isPasswordValid } from '@/lib/auth-shared';
import { checkResetCode, hashPassword, publicUser, updatePassword } from '@/lib/server/authStore';
import { isBreachedPassword } from '@/lib/server/breached';
import { clearFailedLogins } from '@/lib/server/lockout';
import { clientIp, hitAll, tooMany } from '@/lib/server/rateLimit';
import { startSession } from '@/lib/server/session';

/**
 * Step 2 of password reset: code + new password. On success every older session for the
 * account is revoked and this browser is signed in fresh.
 */
export async function POST(req: Request) {
  try {
    const { email, code, password } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || typeof password !== 'string') {
      return NextResponse.json({ ok: false, error: 'Email, code and new password are required.' }, { status: 400 });
    }

    // Per IP and per account (each code also locks after 5 wrong guesses).
    const limit = await hitAll([
      [`reset-verify:ip:${clientIp(req)}`, 30, 15 * 60_000, 'reset attempts'],
      [`reset-verify:email:${email.trim().toLowerCase()}`, 10, 15 * 60_000, 'reset attempts for this email'],
    ]);
    if (!limit.ok) return tooMany(limit.retryAfter, limit.what);

    // Validate the password first so a weak password doesn't burn one of the code attempts.
    if (!isPasswordValid(password)) {
      return NextResponse.json(
        { ok: false, error: 'Password must be 8+ chars and contain at least 1 letter, 1 number, and 1 special character.' },
        { status: 400 },
      );
    }
    if (await isBreachedPassword(password)) {
      return NextResponse.json(
        { ok: false, error: 'This password has appeared in a data breach. Choose a different one.' },
        { status: 400 },
      );
    }

    const cleanEmail = email.trim().toLowerCase();
    const { result } = await checkResetCode(cleanEmail, code);
    if (result === 'missing') {
      return NextResponse.json({ ok: false, error: 'This reset code has expired. Please request a new one.' }, { status: 400 });
    }
    if (result === 'locked') {
      return NextResponse.json({ ok: false, error: 'Too many incorrect attempts. Please request a new code.' }, { status: 429 });
    }
    if (result === 'wrong') {
      return NextResponse.json({ ok: false, error: 'Invalid reset code. Please check your email.' }, { status: 400 });
    }

    const record = await updatePassword(cleanEmail, await hashPassword(password));
    if (!record) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });

    await clearFailedLogins(record.id); // a successful reset lifts any login lockout
    // updatePassword revoked every session; this browser gets a fresh one (if the account is active).
    if (record.status === 'active') await startSession(record, req);
    await audit({ action: 'auth.password.reset', actor: record, target: { type: 'user', id: record.id }, details: { sessionsRevoked: true } }, requestMeta(req));
    return NextResponse.json({ ok: true, user: publicUser(record) });
  } catch (err) {
    console.error('[RESET ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not reset the password.' }, { status: 500 });
  }
}
