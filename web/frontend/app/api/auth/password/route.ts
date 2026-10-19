import { NextResponse } from 'next/server';
import { isPasswordValid } from '@/lib/auth-shared';
import { hashPassword, publicUser, updatePassword, verifyPassword } from '@/lib/server/authStore';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireUser } from '@/lib/server/requireUser';
import { setSessionCookie } from '@/lib/server/session';

/**
 * Change password while signed in. Needs the current password. Every other session for the
 * account stops working; this browser gets a fresh session so the user stays signed in here.
 */
export async function POST(req: Request) {
  const { record, res } = requireUser();
  if (res) return res;
  try {
    const limit = hit(`pw-change:${record.email}:${clientIp(req)}`, 5, 15 * 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'password attempts');

    const { currentPassword, newPassword } = await req.json();
    if (typeof currentPassword !== 'string' || typeof newPassword !== 'string') {
      return NextResponse.json({ ok: false, error: 'Current and new password are required.' }, { status: 400 });
    }
    if (!verifyPassword(currentPassword, record.passwordHash)) {
      return NextResponse.json({ ok: false, error: 'Your current password is incorrect.' }, { status: 401 });
    }
    if (!isPasswordValid(newPassword)) {
      return NextResponse.json(
        { ok: false, error: 'New password must be 8+ chars and contain at least 1 letter, 1 number, and 1 special character.' },
        { status: 400 },
      );
    }
    if (verifyPassword(newPassword, record.passwordHash)) {
      return NextResponse.json({ ok: false, error: 'Choose a password different from your current one.' }, { status: 400 });
    }

    const updated = updatePassword(record.email, hashPassword(newPassword));
    if (!updated) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
    setSessionCookie(publicUser(updated));
    return NextResponse.json({ ok: true });
  } catch (err) {
    console.error('[PASSWORD CHANGE ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not change the password.' }, { status: 500 });
  }
}
