import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { EMAIL_RE } from '@/lib/auth-shared';
import { findUser, generateOtp, setResetCode } from '@/lib/server/authStore';
import { sendOtpEmail } from '@/lib/server/email';
import { clientIp, hitAll, tooMany } from '@/lib/server/rateLimit';
import { noteSpike } from '@/lib/server/spikes';

/** Step 1 of password reset: email a 6-digit code to the account owner. */
export async function POST(req: Request) {
  try {
    const { email } = await req.json();
    if (typeof email !== 'string' || !EMAIL_RE.test(email.trim())) {
      return NextResponse.json({ ok: false, error: 'Enter a valid email address.' }, { status: 400 });
    }
    const cleanEmail = email.trim().toLowerCase();

    await noteSpike('code_request');
    const limit = await hitAll([
      [`reset:ip:${clientIp(req)}`, 10, 60 * 60_000, 'reset requests from this network'],
      [`reset:email:burst:${cleanEmail}`, 1, 45_000, 'reset requests'],
      [`reset:email:hour:${cleanEmail}`, 5, 60 * 60_000, 'reset requests for this email'],
    ]);
    if (!limit.ok) return tooMany(limit.retryAfter, limit.what);

    // Same answer whether or not the account exists, so this endpoint can't be used to
    // discover who has an account.
    const generic = NextResponse.json({ ok: true, message: 'If an account exists for that email, a reset code is on its way.' });

    const user = await findUser(cleanEmail);
    if (!user) return generic;
    await audit({ action: 'auth.password.reset_requested', actor: user, target: { type: 'user', id: user.id } }, requestMeta(req));

    const code = generateOtp();
    await setResetCode(cleanEmail, code);
    try {
      await sendOtpEmail({ to: cleanEmail, code, type: 'reset' });
    } catch (err) {
      console.error('[FORGOT SMTP ERROR]', err);
      if (process.env.NODE_ENV !== 'production') {
        console.log(`[DEV FALLBACK RESET CODE FOR ${cleanEmail}]: ${code}`);
        return NextResponse.json({ ok: true, message: 'Password reset code sent (dev mode).', devCode: code });
      }
      return NextResponse.json({ ok: false, error: 'Could not send the reset email. Please try again.' }, { status: 500 });
    }
    if (process.env.NODE_ENV !== 'production') {
      return NextResponse.json({ ok: true, message: 'Password reset code sent.', devCode: code });
    }
    return generic;
  } catch (err) {
    console.error('[FORGOT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal server error.' }, { status: 500 });
  }
}
