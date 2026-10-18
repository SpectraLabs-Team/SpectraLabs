import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { verifySessionToken } from '@/lib/server/authStore';

export const dynamic = 'force-dynamic';

export async function GET() {
  try {
    const cookieStore = cookies();
    const token = cookieStore.get('spectra_session')?.value;

    if (!token) {
      return NextResponse.json({ ok: true, user: null });
    }

    const user = verifySessionToken(token);
    if (!user) {
      // Expired or invalid token, clean cookie
      cookieStore.delete('spectra_session');
      return NextResponse.json({ ok: true, user: null });
    }

    return NextResponse.json({ ok: true, user });
  } catch (err: any) {
    console.error('[SESSION ERROR]', err);
    return NextResponse.json({ ok: true, user: null });
  }
}
