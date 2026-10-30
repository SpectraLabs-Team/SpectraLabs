import { NextResponse } from 'next/server';
import { getAdminUserDetail } from '@/lib/server/adminUsers';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { findUserById, LastAdminError, setPlatformRole, setUserStatus } from '@/lib/server/authStore';
import { requireAdmin, requireRecentAuth } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';
type Ctx = { params: Promise<{ id: string }> };

/**
 * One account in detail: profile, security, workspaces, devices, recent activity. Admins only; the
 * view is audit-logged. IP addresses and browser strings are left out unless `?reveal=ips`, and
 * revealing them is logged separately (`admin.user.ips_revealed`).
 */
export async function GET(req: Request, props: Ctx) {
  const params = await props.params;
  const { auth, res } = await requireAdmin();
  if (res) return res;
  const reveal = new URL(req.url).searchParams.get('reveal') === 'ips';
  const detail = await getAdminUserDetail(params.id, { ips: reveal });
  if (!detail) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
  if (detail.user.id !== auth.user.id) {
    const action = reveal ? 'admin.user.ips_revealed' : 'admin.user.viewed';
    await audit({ action, actor: auth.user, target: { type: 'user', id: detail.user.id }, details: { email: detail.user.email } }, requestMeta(req));
  }
  return NextResponse.json({ ok: true, ...detail }, { headers: { 'Cache-Control': 'no-store' } });
}

/**
 * { status: 'active' | 'disabled' }  disable (signs them out everywhere) or re-enable
 * { role: 'admin' | 'user' }         grant or remove platform admin (signs them out everywhere)
 * Admins only, with a fresh "confirm it's you". Not on your own account; the last admin stays.
 */
export async function PATCH(req: Request, props: Ctx) {
  const params = await props.params;
  const { auth, res } = await requireAdmin();
  if (res) return res;
  const stepUp = requireRecentAuth(auth.session);
  if (stepUp) return stepUp;
  const target = await findUserById(params.id);
  if (!target) return NextResponse.json({ ok: false, error: 'Account not found.' }, { status: 404 });
  if (target.id === auth.user.id) return NextResponse.json({ ok: false, error: 'You can’t change your own status or role here. Ask another admin.' }, { status: 403 });
  try {
    const body = await req.json().catch(() => ({}));
    if (body.status === 'active' || body.status === 'disabled') {
      await setUserStatus(target.id, body.status, auth.user);
    } else if (body.role === 'admin' || body.role === 'user') {
      await setPlatformRole(target.email, body.role, auth.user);
    } else {
      return NextResponse.json({ ok: false, error: 'Send a status (active/disabled) or a role (admin/user).' }, { status: 400 });
    }
    return NextResponse.json({ ok: true, ...(await getAdminUserDetail(target.id)) });
  } catch (err) {
    if (err instanceof LastAdminError) return NextResponse.json({ ok: false, error: err.message }, { status: 409 });
    console.error('[ADMIN USER UPDATE ERROR]', err instanceof Error ? err.message : err);
    return NextResponse.json({ ok: false, error: 'Could not update the account.' }, { status: 500 });
  }
}
