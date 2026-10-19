import { NextResponse } from 'next/server';
import { publicUser, revokeSessions } from '@/lib/server/authStore';
import { requireUser } from '@/lib/server/requireUser';
import { setSessionCookie } from '@/lib/server/session';

/** "Sign out of all other devices": revoke every session, then re-issue one for this browser. */
export async function DELETE() {
  const { record, res } = requireUser();
  if (res) return res;
  const updated = revokeSessions(record.email);
  if (!updated) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
  setSessionCookie(publicUser(updated));
  return NextResponse.json({ ok: true });
}
