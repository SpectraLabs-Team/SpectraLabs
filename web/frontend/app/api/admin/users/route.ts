import { NextResponse } from 'next/server';
import { cookies } from 'next/headers';
import { listUsers, verifySessionToken } from '@/lib/server/authStore';

export const dynamic = 'force-dynamic';

/** Registered accounts for the admin console. Role is checked from the signed session cookie, never the client. */
export async function GET() {
  const token = cookies().get('spectra_session')?.value;
  const session = token ? verifySessionToken(token) : null;
  if (!session) return NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 });
  if (session.role !== 'admin') return NextResponse.json({ ok: false, error: 'Admin privileges required.' }, { status: 403 });
  return NextResponse.json({ ok: true, users: listUsers() });
}
