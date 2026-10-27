import { NextResponse, type NextRequest } from 'next/server';
import { getAdminAnalytics, parseRange } from '@/lib/server/adminAnalytics';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/**
 * Daily analytics for a date range (UTC, inclusive; `from` and `to` as YYYY-MM-DD, at most a year).
 * Admins only. Each view is audit-logged.
 */
export async function GET(req: NextRequest) {
  const { auth, res } = await requireAdmin();
  if (res) return res;
  const sp = req.nextUrl.searchParams;
  const range = parseRange(sp.get('from'), sp.get('to'));
  const analytics = await getAdminAnalytics(range);
  await audit({ action: 'admin.analytics.viewed', actor: auth.user, details: { from: range.from, to: range.to } }, requestMeta(req));
  return NextResponse.json({ ok: true, ...analytics }, { headers: { 'Cache-Control': 'no-store' } });
}
