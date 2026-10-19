import { NextResponse } from 'next/server';
import { listUsers } from '@/lib/server/authStore';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** Registered accounts for the admin console. Platform admins only (role read from the database). */
export async function GET() {
  const { res } = await requireAdmin();
  if (res) return res;
  return NextResponse.json({ ok: true, users: await listUsers() });
}
