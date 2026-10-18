import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { EMAIL_RE } from '@/lib/auth-shared';
import { ADMIN_EMAIL, createSessionToken, createUser, findUser, hashPassword, verifyPassword } from '@/lib/server/authStore';

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { email, password } = body;

    if (!email || !EMAIL_RE.test(email.trim()) || !password) {
      return NextResponse.json({ ok: false, error: 'Email and password are required.' }, { status: 400 });
    }

    const cleanEmail = email.trim().toLowerCase();
    let user = findUser(cleanEmail);

    if (!user) {
      // First-time admin login bootstrapping: if the admin email logs in, provision automatically
      if (cleanEmail === ADMIN_EMAIL) {
        user = createUser(cleanEmail, hashPassword(password)) as any;
      } else {
        return NextResponse.json(
          { ok: false, error: 'No account found with this email. Please click "Sign Up" to register.' },
          { status: 404 },
        );
      }
    }

    if (!user || !verifyPassword(password, user.passwordHash)) {
      return NextResponse.json(
        { ok: false, error: 'Incorrect email or password. Please try again.' },
        { status: 401 },
      );
    }

    const userPayload = { email: user.email, role: user.role };
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
    console.error('[LOGIN ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Internal login error.' }, { status: 500 });
  }
}
