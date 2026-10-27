import { NextResponse, type NextRequest } from 'next/server';
import { listDirectory, DIRECTORY_SORTS, type DirectorySort } from '@/lib/server/adminAnalytics';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { requireAdmin } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/**
 * The user directory: search (email or name), filter by role and status, sort, paginate.
 * Admins only (role read from the database). Each page view is audit-logged.
 */
export async function GET(req: NextRequest) {
  const { auth, res } = await requireAdmin();
  if (res) return res;
  const sp = req.nextUrl.searchParams;
  const sort = DIRECTORY_SORTS.includes(sp.get('sort') as DirectorySort) ? (sp.get('sort') as DirectorySort) : 'email';
  const role = sp.get('role');
  const status = sp.get('status');
  const directory = await listDirectory({
    q: sp.get('q') ?? '',
    role: role === 'admin' || role === 'user' ? role : 'all',
    status: status === 'active' || status === 'disabled' ? status : 'all',
    sort,
    dir: sp.get('dir') === 'desc' ? 'desc' : 'asc',
    page: Number(sp.get('page')) || 1,
    pageSize: Number(sp.get('pageSize')) || 25,
  });
  await audit({ action: 'admin.directory.viewed', actor: auth.user, details: { q: (sp.get('q') ?? '').slice(0, 100), page: directory.page } }, requestMeta(req));
  return NextResponse.json({ ok: true, ...directory }, { headers: { 'Cache-Control': 'no-store' } });
}
