import { NextResponse } from 'next/server';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { endSession, getSession } from '@/lib/server/session';

/** Sign out this browser: the session is revoked on the server, not just the cookie cleared. */
export async function POST(req: Request) {
  try {
    const auth = await getSession().catch(() => null);
    await endSession();
    if (auth) await audit({ action: 'auth.logout', actor: auth.user, target: { type: 'session', id: auth.session.id } }, requestMeta(req));
    return NextResponse.json({ ok: true, message: 'Signed out successfully.' });
  } catch (err) {
    console.error('[LOGOUT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to sign out.' }, { status: 500 });
  }
}
