import { NextResponse } from 'next/server';
import { checkLoginCode, findUser, publicUser } from '@/lib/server/authStore';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { setSessionCookie } from '@/lib/server/session';

/** Second step of a 2FA login: the emailed code. Issued only after the password was correct. */
export async function POST(req: Request) {
  try {
    const { email, code } = await req.json();
    if (typeof email !== 'string' || typeof code !== 'string' || !email || !code) {
      return NextResponse.json({ ok: false, error: 'Email and 6-digit code are required.' }, { status: 400 });
    }
    const ipLimit = hit(`login-verify:ip:${clientIp(req)}`, 30, 15 * 60_000);
    if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'verification attempts');

    const cleanEmail = email.trim().toLowerCase();
    const { result } = checkLoginCode(cleanEmail, code);
    if (result === 'missing') {
      return NextResponse.json({ ok: false, error: 'This login code has expired. Please log in again.' }, { status: 400 });
    }
    if (result === 'locked') {
      return NextResponse.json({ ok: false, error: 'Too many incorrect attempts. Please log in again.' }, { status: 429 });
    }
    if (result === 'wrong') {
      return NextResponse.json({ ok: false, error: 'Invalid code. Please check your email.' }, { status: 400 });
    }

    const record = findUser(cleanEmail);
    if (!record) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
    const user = publicUser(record);
    setSessionCookie(user);
    return NextResponse.json({ ok: true, user });
  } catch (err) {
    console.error('[LOGIN VERIFY ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to verify code.' }, { status: 500 });
  }
}
