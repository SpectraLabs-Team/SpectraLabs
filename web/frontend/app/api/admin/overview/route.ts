import { NextResponse } from 'next/server';
import { getAdminOverview } from '@/lib/server/adminAnalytics';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** Headline numbers and worker status for the admin console. Admins only. Read-only, not audit-logged. */
export async function GET() {
  const { res } = await requireAdmin();
  if (res) return res;
  return NextResponse.json({ ok: true, ...(await getAdminOverview()) }, { headers: { 'Cache-Control': 'no-store' } });
}
