import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { cleanName, MAX_NAME, MIN_NAME } from '@/lib/auth-shared';
import { deleteUser, publicUser, twoFactorEnforced, twoFactorOn, updateName, verifyPassword, type UserRecord } from '@/lib/server/authStore';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';
import { requireUser } from '@/lib/server/requireUser';
import { SESSION_COOKIE } from '@/lib/server/session';

export const dynamic = 'force-dynamic';

const profile = (r: UserRecord) => ({
  ...publicUser(r),
  createdAt: r.createdAt,
  twoFactor: twoFactorOn(r),
  twoFactorEnforced: twoFactorEnforced(r),
  passwordChangedAt: r.passwordChangedAt ?? null,
});

/** The signed-in user's profile. */
export async function GET() {
  const { record, res } = requireUser();
  if (res) return res;
  return NextResponse.json({ ok: true, profile: profile(record) });
}

/** Update the display name. */
export async function PATCH(req: Request) {
  const { record, res } = requireUser();
  if (res) return res;
  try {
    const { name } = await req.json();
    const clean = cleanName(name);
    if (!clean) return NextResponse.json({ ok: false, error: `Name must be ${MIN_NAME}–${MAX_NAME} characters.` }, { status: 400 });
    const updated = updateName(record.email, clean);
    if (!updated) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
    return NextResponse.json({ ok: true, profile: profile(updated) });
  } catch {
    return NextResponse.json({ ok: false, error: 'Could not update your name.' }, { status: 500 });
  }
}

/**
 * Delete the account. Needs the current password, and the admin account can't delete itself
 * (that would leave the platform without an administrator).
 */
export async function DELETE(req: Request) {
  const { record, res } = requireUser();
  if (res) return res;
  try {
    if (record.role === 'admin') {
      return NextResponse.json({ ok: false, error: 'The admin account can’t be deleted from here.' }, { status: 403 });
    }
    const limit = hit(`account-delete:${record.email}:${clientIp(req)}`, 5, 15 * 60_000);
    if (!limit.ok) return tooMany(limit.retryAfter, 'password attempts');
    const { password } = await req.json();
    if (typeof password !== 'string' || !verifyPassword(password, record.passwordHash)) {
      return NextResponse.json({ ok: false, error: 'Incorrect password.' }, { status: 401 });
    }
    deleteUser(record.email);
    cookies().delete(SESSION_COOKIE);
    return NextResponse.json({ ok: true });
  } catch (err) {
    console.error('[ACCOUNT DELETE ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not delete the account.' }, { status: 500 });
  }
}
