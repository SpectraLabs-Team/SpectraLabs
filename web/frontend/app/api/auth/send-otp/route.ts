import { NextResponse } from 'next/server';
import { cleanName, EMAIL_RE, isPasswordValid, MIN_NAME } from '@/lib/auth-shared';
import { findUser, generateOtp, getPendingOtp, hashPassword, setPendingOtp } from '@/lib/server/authStore';
import { isBreachedPassword } from '@/lib/server/breached';
import { sendOtpEmail } from '@/lib/server/email';
import { clientIp, hitAll, tooMany } from '@/lib/server/rateLimit';
import { noteSpike } from '@/lib/server/spikes';

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, password, mode } = body;

    if (!email || typeof email !== 'string' || !EMAIL_RE.test(email.trim())) {
      return NextResponse.json({ ok: false, error: 'Valid email address is required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();

    let passwordHash: string;
    let name: string;
    let twoFactor = true;

    if (mode === 'resend') {
      // Re-issue a code for the sign-up already in progress. The password hash and name
      // must carry over, otherwise verify-otp would have nothing to create the account from.
      const pending = await getPendingOtp(cleanEmail);
      if (!pending || !pending.passwordHash) {
        return NextResponse.json(
          { ok: false, error: 'Your sign-up session expired. Please go back and start again.' },
          { status: 400 },
        );
      }
      passwordHash = pending.passwordHash;
      name = pending.name;
      twoFactor = pending.twoFactor !== false;
    } else if (mode === 'signup') {
      if (await findUser(cleanEmail)) {
        return NextResponse.json(
          { ok: false, error: 'An account with this email already exists. Please log in.' },
          { status: 400 },
        );
      }
      name = cleanName(body.name);
      if (!name) {
        return NextResponse.json({ ok: false, error: `Full name must be at least ${MIN_NAME} characters.` }, { status: 400 });
      }
      if (typeof password !== 'string' || !isPasswordValid(password)) {
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
      passwordHash = await hashPassword(password);
      twoFactor = body.twoFactor !== false; // on unless explicitly turned off
    } else {
      return NextResponse.json({ ok: false, error: 'Unsupported request.' }, { status: 400 });
    }

    // Stop inbox flooding and SMTP quota burn: one code per email every 45s, 5 an hour,
    // and 20 an hour from one IP across all emails.
    await noteSpike('code_request');
    const limit = await hitAll([
      [`otp:ip:${clientIp(req)}`, 20, 60 * 60_000, 'verification emails from this network'],
      [`otp:email:burst:${cleanEmail}`, 1, 45_000, 'code requests'],
      [`otp:email:hour:${cleanEmail}`, 5, 60 * 60_000, 'code requests for this email'],
    ]);
    if (!limit.ok) return tooMany(limit.retryAfter, limit.what);

    const code = generateOtp();
    await setPendingOtp(cleanEmail, code, passwordHash, name, twoFactor);

    try {
      await sendOtpEmail({ to: cleanEmail, code, type: 'signup' });
      console.log(`[AUTH] Sent 6-digit OTP code to ${cleanEmail}`);
    } catch (emailErr) {
      console.error('[AUTH SMTP ERROR]', emailErr);
      if (process.env.NODE_ENV !== 'production') {
        console.log(`[DEV FALLBACK OTP FOR ${cleanEmail}]: ${code}`);
        return NextResponse.json({ ok: true, message: 'Verification code generated (dev fallback).', devCode: code });
      }
      return NextResponse.json(
        { ok: false, error: 'Failed to deliver verification email. Please check the email address or try again.' },
        { status: 500 },
      );
    }

    return NextResponse.json({
      ok: true,
      message: 'Verification code sent.',
      devCode: process.env.NODE_ENV !== 'production' ? code : undefined,
    });
  } catch (err: any) {
    console.error('[SEND-OTP ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal server error.' }, { status: 500 });
  }
}
