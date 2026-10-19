import { NextResponse } from 'next/server';
import { describeUserAgent } from '@/lib/userAgent';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireUser } from '@/lib/server/requireUser';
import { listSessions, revokeAllSessions, revokeSessionById } from '@/lib/server/sessions';

export const dynamic = 'force-dynamic';

/**
 * This account's live sessions (one per signed-in browser), most recent first, with a readable
 * device label, IP, last activity and when that device was first seen. The current one is marked.
 */
export async function GET() {
  const { record, session, res } = await requireUser();
  if (res) return res;
  const sessions = (await listSessions(record.id)).map((s) => ({
    id: s.id,
    current: s.id === session.id,
    createdAt: s.createdAt,
    lastSeenAt: s.lastSeenAt,
    expiresAt: s.expiresAt,
    ip: s.ip,
    device: describeUserAgent(s.userAgent),
    deviceFirstSeenAt: s.deviceFirstSeenAt,
  }));
  return NextResponse.json({ ok: true, sessions });
}

/**
 * DELETE /api/auth/sessions            → sign out of all other devices (this browser stays signed in)
 * DELETE /api/auth/sessions?id=<uuid>  → sign out one other device
 */
export async function DELETE(req: Request) {
  const { record, session, res } = await requireUser();
  if (res) return res;
  try {
    const id = new URL(req.url).searchParams.get('id');
    if (id) {
      if (!/^[0-9a-f-]{36}$/i.test(id)) return NextResponse.json({ ok: false, error: 'Invalid session id.' }, { status: 400 });
      if (id === session.id) return NextResponse.json({ ok: false, error: 'Use Sign out to end this session.' }, { status: 400 });
      const ended = await revokeSessionById(record.id, id);
      if (!ended) return NextResponse.json({ ok: false, error: 'Session not found.' }, { status: 404 });
      await audit({ action: 'auth.session.revoked', actor: record, target: { type: 'session', id } }, requestMeta(req));
      return NextResponse.json({ ok: true, ended: 1 });
    }
    const ended = await revokeAllSessions(record.id, { except: session.id });
    await audit({ action: 'auth.session.revoked_others', actor: record, target: { type: 'user', id: record.id }, details: { ended } }, requestMeta(req));
    return NextResponse.json({ ok: true, ended });
  } catch (err) {
    console.error('[SESSIONS ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not sign out other devices.' }, { status: 500 });
  }
}
