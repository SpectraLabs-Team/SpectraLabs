import { NextResponse } from 'next/server';
import { recordHeartbeat, HEARTBEAT_INTERVAL_SECONDS } from '@/lib/server/activity';
import { requireUser } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/**
 * Active-time heartbeat from the browser (about once a minute while the page is visible and in use).
 * The client sends `{ active: true }`; anything else records nothing. Stores daily totals only.
 */
export async function POST(req: Request) {
  const { record, res } = await requireUser();
  if (res) return res;
  const body = await req.json().catch(() => ({}));
  if (body?.active !== true) return NextResponse.json({ ok: true, credited: 0 }, { headers: { 'Cache-Control': 'no-store' } });
  const credited = await recordHeartbeat(record.id);
  return NextResponse.json({ ok: true, credited, intervalSeconds: HEARTBEAT_INTERVAL_SECONDS }, { headers: { 'Cache-Control': 'no-store' } });
}
