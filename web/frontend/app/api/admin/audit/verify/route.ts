import { NextResponse } from 'next/server';
import { verifyChain } from '@/lib/server/audit';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** Check the audit log's hash chain (admins only). */
export async function GET() {
  const { res } = await requireAdmin();
  if (res) return res;
  return NextResponse.json({ ok: true, chain: await verifyChain() });
}
