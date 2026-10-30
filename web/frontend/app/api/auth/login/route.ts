import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { EMAIL_RE } from '@/lib/auth-shared';
import { findUser, generateOtp, hashPassword, publicUser, rehashPassword, setLoginCode, twoFactorOn, verifyPassword } from '@/lib/server/authStore';
import { sendOtpEmail } from '@/lib/server/email';
import { startSession } from '@/lib/server/session';
import { clearFailedLogins, lockedUntil, lockMessage, LOCK_DURATION_MS, recordFailedLogin, shouldSendLockoutEmail } from '@/lib/server/lockout';
import { sendLockoutEmail } from '@/lib/server/email';
import { createMfaChallenge, hasTotp } from '@/lib/server/mfa';
import { clientIp, hit, hitAll, peek, tooMany } from '@/lib/server/rateLimit';
import { noteSpike } from '@/lib/server/spikes';

const WINDOW = 15 * 60_000;
const IP_MAX = 30; // login requests per IP per window
const IP_FAIL_MAX = 20; // failed logins (wrong password or unknown email) per IP per window

const locked = (until: number) =>
  NextResponse.json(
    { ok: false, error: lockMessage(until), locked: true },
    { status: 429, headers: { 'Retry-After': String(Math.max(1, Math.ceil((until - Date.now()) / 1000))) } },
  );

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, password } = body;

    if (typeof email !== 'string' || typeof password !== 'string' || !EMAIL_RE.test(email.trim()) || !password) {
      return NextResponse.json({ ok: false, error: 'Email and password are required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();
    const ip = clientIp(req);
    const meta = requestMeta(req);

    // Per IP: a cap on all login traffic, and a tighter one on failures (password spraying across
    // many accounts). Per account: the lockout below (holds even if the attacker rotates IPs).
    // All counters live in Postgres, so a restart doesn't reset them.
    const ipLimit = await hit(`login:ip:${ip}`, IP_MAX, WINDOW);
    if (!ipLimit.ok) return tooMany(ipLimit.retryAfter, 'login attempts');
    const ipFailKey = `login:ipfail:${ip}`;
    const ipFails = await peek(ipFailKey, IP_FAIL_MAX);
    if (!ipFails.ok) return tooMany(ipFails.retryAfter, 'failed login attempts from this network');

    const user = await findUser(cleanEmail);

    if (!user) {
      await hit(ipFailKey, IP_FAIL_MAX, WINDOW);
      await audit({ action: 'auth.login.failure', outcome: 'failure', actor: { email: cleanEmail }, details: { reason: 'unknown_email' } }, meta);
      await noteSpike('failed_login');
      return NextResponse.json(
        { ok: false, error: 'No account found with this email. Please click "Sign Up" to register.' },
        { status: 404 },
      );
    }

    // A locked account refuses even the right password, so the lock can't be used to test guesses.
    const until = await lockedUntil(user.id);
    if (until) {
      await audit({ action: 'auth.login.failure', outcome: 'denied', actor: user, details: { reason: 'locked' } }, meta);
      await noteSpike('failed_login');
      return locked(until);
    }

    const verifyRes = await verifyPassword(password, user.passwordHash);
    if (!verifyRes.ok) {
      await hit(ipFailKey, IP_FAIL_MAX, WINDOW);
      const failure = await recordFailedLogin(user.id);
      await audit({ action: 'auth.login.failure', outcome: 'failure', actor: user, details: { reason: 'wrong_password', failures: failure.failures } }, meta);
      await noteSpike('failed_login');
      if (failure.justLocked) {
        await audit({ action: 'auth.login.locked', outcome: 'denied', actor: user, target: { type: 'user', id: user.id }, details: { until: new Date(failure.lockedUntil ?? Date.now()).toISOString() } }, meta);
      }
      if (failure.justLocked && (await shouldSendLockoutEmail(user.id))) {
        // Fire and forget: a mail problem must not change the answer or slow the response.
        sendLockoutEmail({ to: user.email, minutes: Math.round(LOCK_DURATION_MS / 60_000), ip }).catch((err) =>
          console.error('[LOCKOUT EMAIL ERROR]', err),
        );
      }
      if (failure.lockedUntil) return locked(failure.lockedUntil);
      return NextResponse.json(
        { ok: false, error: 'Incorrect email or password. Please try again.' },
        { status: 401 },
      );
    }

    await clearFailedLogins(user.id);

    // Old PBKDF2 (or outdated Argon2 parameters): upgrade the stored hash now that we have the
    // password. Compare-and-set, so a concurrent password change wins. Never blocks the sign-in.
    if (verifyRes.needsRehash) {
      try {
        await rehashPassword(user.id, await hashPassword(password), user.passwordHash);
      } catch (rehashErr) {
        console.error('[LOGIN REHASH ERROR]', rehashErr);
      }
    }

    if (user.status !== 'active') {
      await audit({ action: 'auth.login.failure', outcome: 'denied', actor: user, details: { reason: 'disabled' } }, meta);
      await noteSpike('failed_login');
      return NextResponse.json({ ok: false, error: 'This account has been disabled. Contact support.' }, { status: 403 });
    }

    // Local dev bypass for spectralabs.team@gmail.com
    if (process.env.NODE_ENV !== 'production' && !process.env.TEST_DATABASE_URL && cleanEmail === 'spectralabs.team@gmail.com') {
      const session = await startSession(user, req, { notifyNewDevice: false });
      return NextResponse.json({ ok: true, user: publicUser(user) });
    }

    // 2FA accounts: the password alone doesn't sign in. Email a one-time code and finish at
    // /api/auth/login/verify. The code can only be requested by someone who knew the password.
    if (twoFactorOn(user)) {
      // Authenticator app set up: no email. Hand back a short-lived single-use challenge that
      // /api/auth/login/verify needs together with the app's code (or a recovery code).
      if (await hasTotp(user.id)) {
        const challenge = await createMfaChallenge(cleanEmail);
        return NextResponse.json({ ok: true, twoFactor: true, method: 'totp', challenge });
      }
      await noteSpike('code_request');
      const codeLimit = await hitAll([
        [`login-otp:email:burst:${cleanEmail}`, 1, 45_000, 'code requests'],
        [`login-otp:email:hour:${cleanEmail}`, 10, 60 * 60_000, 'login codes for this email'],
        [`login-otp:ip:hour:${ip}`, 30, 60 * 60_000, 'login codes from this network'],
      ]);
      if (!codeLimit.ok) return tooMany(codeLimit.retryAfter, codeLimit.what);
      const code = generateOtp();
      await setLoginCode(cleanEmail, code);
      try {
        await sendOtpEmail({ to: cleanEmail, code, type: 'login' });
      } catch (err) {
        console.error('[LOGIN 2FA SMTP ERROR]', err);
        return NextResponse.json({ ok: false, error: 'Could not send your login code. Please try again.' }, { status: 500 });
      }
      return NextResponse.json({
        ok: true,
        twoFactor: true,
        method: 'email',
      });
    }

    const session = await startSession(user, req, { notifyNewDevice: true });
    await audit({ action: 'auth.login.success', actor: user, target: { type: 'session', id: session.id }, details: { method: 'password', rehashed: verifyRes.needsRehash || undefined } }, meta);
    return NextResponse.json({ ok: true, user: publicUser(user) });
  } catch (err: any) {
    console.error('[LOGIN ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal login error.' }, { status: 500 });
  }
}
