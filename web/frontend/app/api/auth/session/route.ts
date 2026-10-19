import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { publicUser } from '@/lib/server/authStore';
import { getSession, SESSION_COOKIE } from '@/lib/server/session';

export const dynamic = 'force-dynamic';

/** Who is signed in on this browser (null if nobody). Clears a dead cookie. */
export async function GET() {
  try {
    const auth = await getSession();
    if (!auth) {
      if (cookies().get(SESSION_COOKIE)) cookies().delete(SESSION_COOKIE);
      return NextResponse.json({ ok: true, user: null });
    }
    const { id, name, slug, personal, role } = auth.workspace;
    return NextResponse.json({ ok: true, user: publicUser(auth.user), workspace: { id, name, slug, personal, role } });
  } catch (err) {
    console.error('[SESSION ERROR]', err);
    return NextResponse.json({ ok: false, user: null, error: 'Could not check your session.' }, { status: 500 });
  }
}
