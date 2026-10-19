import { NextResponse } from 'next/server';
import { auditResponse } from '@/lib/server/auditRoute';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/**
 * Platform audit log (admins only). Filters: action (exact, or a prefix like "auth."), outcome,
 * actor (email contains), from / to (ISO dates), before (seq cursor), limit. ?format=csv exports.
 */
export async function GET(req: Request) {
  const { auth, res } = await requireAdmin();
  if (res) return res;
  try {
    return await auditResponse(req, {}, auth.user);
  } catch (err) {
    console.error('[ADMIN AUDIT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not load the audit log.' }, { status: 500 });
  }
}
