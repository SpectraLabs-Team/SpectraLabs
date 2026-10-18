import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';

export async function POST() {
  try {
    const cookieStore = cookies();
    cookieStore.delete('spectra_session');
    return NextResponse.json({ ok: true, message: 'Signed out successfully.' });
  } catch (err: any) {
    console.error('[LOGOUT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Failed to sign out.' }, { status: 500 });
  }
}
