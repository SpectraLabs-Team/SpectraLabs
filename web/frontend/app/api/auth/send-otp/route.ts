import { NextResponse } from 'next/server';
import { EMAIL_RE, isPasswordValid } from '@/lib/auth-shared';
import { generateOtp, hashPassword, setPendingOtp, findUser } from '@/lib/server/authStore';
import { sendOtpEmail } from '@/lib/server/email';

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, password, mode } = body;

    if (!email || !EMAIL_RE.test(email.trim())) {
      return NextResponse.json({ ok: false, error: 'Valid email address is required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();

    // Check if user exists on signup
    const existing = findUser(cleanEmail);
    if (mode === 'signup' && existing) {
      return NextResponse.json(
        { ok: false, error: 'An account with this email already exists. Please log in.' },
        { status: 400 },
      );
    }

    if (password && !isPasswordValid(password)) {
      return NextResponse.json(
        { ok: false, error: 'Password must be 8+ chars and contain at least 1 letter, 1 number, and 1 special character.' },
        { status: 400 },
      );
    }

    const passwordHash = password ? hashPassword(password) : '';
    const code = generateOtp();
    setPendingOtp(cleanEmail, code, passwordHash);

    // Send email via Gmail SMTP
    try {
      await sendOtpEmail({
        to: cleanEmail,
        code,
        type: mode === 'signup' ? 'signup' : 'login',
      });
      console.log(`[AUTH] Sent 6-digit OTP code to ${cleanEmail}`);
    } catch (emailErr) {
      console.error('[AUTH SMTP ERROR]', emailErr);
      // In dev fallback, if SMTP fails, still log code to console
      console.log(`[DEV FALLBACK OTP FOR ${cleanEmail}]: ${code}`);
      return NextResponse.json(
        { ok: false, error: 'Failed to deliver verification email. Please check the email address or try again.' },
        { status: 500 },
      );
    }

    return NextResponse.json({ ok: true, message: 'Verification code sent.' });
  } catch (err: any) {
    console.error('[SEND-OTP ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal server error.' }, { status: 500 });
  }
}
