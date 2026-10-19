import { NextResponse } from 'next/server';
import { EMAIL_RE } from '@/lib/auth-shared';
import { findUser, generateOtp, publicUser, setLoginCode, twoFactorOn, verifyPassword } from '@/lib/server/authStore';
import { sendOtpEmail } from '@/lib/server/email';
import { setSessionCookie } from '@/lib/server/session';
import { clientIp, hit, peek, reset, tooMany } from '@/lib/server/rateLimit';

const FAIL_MAX = 5; // wrong passwords per email per window
const FAIL_WINDOW = 15 * 60_000;

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, password } = body;

    if (typeof email !== 'string' || typeof password !== 'string' || !EMAIL_RE.test(email.trim()) || !password) {
      return NextResponse.json({ ok: false, error: 'Email and password are required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();

    // Per-IP cap on login traffic, and a per-email lockout after repeated failures
    // (the per-email one holds even if an attacker rotates IPs).
    const ipLimit = hit(`login:ip:${clientIp(req)}`, 30, FAIL_WINDOW);
    if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'login attempts');
    const failKey = `login:fail:${cleanEmail}`;
    const locked = peek(failKey, FAIL_MAX);
    if (!locked.ok) return tooMany(locked.retryAfter, 'failed login attempts for this account');

    const user = findUser(cleanEmail);

    if (!user) {
      hit(failKey, FAIL_MAX, FAIL_WINDOW);
      return NextResponse.json(
        { ok: false, error: 'No account found with this email. Please click "Sign Up" to register.' },
        { status: 404 },
      );
    }

    if (!verifyPassword(password, user.passwordHash)) {
      hit(failKey, FAIL_MAX, FAIL_WINDOW);
      return NextResponse.json(
        { ok: false, error: 'Incorrect email or password. Please try again.' },
        { status: 401 },
      );
    }

    reset(failKey);

    // 2FA accounts: the password alone doesn't sign in. Email a one-time code and finish at
    // /api/auth/login/verify. The code can only be requested by someone who knew the password.
    if (twoFactorOn(user)) {
      for (const [key, max, win, what] of [
        [`login-otp:email:burst:${cleanEmail}`, 1, 45_000, 'code requests'],
        [`login-otp:email:hour:${cleanEmail}`, 10, 60 * 60_000, 'login codes for this email'],
      ] as const) {
        const r = hit(key, max, win);
        if (!r.ok) return tooMany(r.retryAfter, what);
      }
      const code = generateOtp();
      setLoginCode(cleanEmail, code);
      try {
        await sendOtpEmail({ to: cleanEmail, code, type: 'login' });
      } catch (err) {
        console.error('[LOGIN 2FA SMTP ERROR]', err);
        if (process.env.NODE_ENV !== 'production') console.log(`[DEV FALLBACK LOGIN CODE FOR ${cleanEmail}]: ${code}`);
        return NextResponse.json({ ok: false, error: 'Could not send your login code. Please try again.' }, { status: 500 });
      }
      return NextResponse.json({ ok: true, twoFactor: true });
    }

    const userPayload = publicUser(user);
    setSessionCookie(userPayload);
    return NextResponse.json({ ok: true, user: userPayload });
  } catch (err: any) {
    console.error('[LOGIN ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal login error.' }, { status: 500 });
  }
}
