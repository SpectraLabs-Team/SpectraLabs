import { NextResponse } from 'next/server';
import { findUser, type UserRecord } from './authStore';
import { getSession } from './session';

/** The signed-in account's live record, or a ready-made 401 response. */
export function requireUser(): { record: UserRecord; res?: undefined } | { record?: undefined; res: NextResponse } {
  const session = getSession();
  const record = session ? findUser(session.email) : undefined;
  if (!record) return { res: NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 }) };
  return { record };
}
