import { NextResponse } from 'next/server';
import { twoFactorOn, type UserRecord } from './authStore';
import { isRecentlyAuthenticated } from './mfa';
import { AuthzError, requireRole } from './authz';
import { getSession } from './session';
import type { Auth, SessionInfo } from './sessions';
import type { Workspace, WorkspaceRole } from './workspaces';

type Ok = { record: UserRecord; session: SessionInfo; workspace: Workspace; res?: undefined };
type Denied = { record?: undefined; session?: undefined; workspace?: undefined; res: NextResponse };

/** The signed-in account's live record, session and active workspace, or a ready-made 401 response. */
export async function requireUser(): Promise<Ok | Denied> {
  const auth = await getSession();
  if (!auth) return { res: NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 }) };
  return { record: auth.user, session: auth.session, workspace: auth.workspace };
}

export const authzResponse = (err: AuthzError, shape: 'ok' | 'detail' = 'ok') =>
  NextResponse.json(shape === 'detail' ? { detail: err.message } : { ok: false, error: err.message }, { status: err.status });

type Granted = { auth: Auth; workspace: Workspace; res?: undefined };
type Refused = { auth?: undefined; workspace?: undefined; res: NextResponse };

/**
 * For every route that reads or writes workspace data: the session plus the workspace (default:
 * the session's active workspace) if the user has at least `minRole` there, or a ready-made
 * 401 / 403 / 404 response. `shape: 'detail'` answers in the engine's `{detail}` error format.
 */
export async function requireWorkspace(minRole: WorkspaceRole, workspaceId?: string, shape: 'ok' | 'detail' = 'ok'): Promise<Granted | Refused> {
  const auth = await getSession();
  try {
    const workspace = await requireRole(auth, workspaceId ?? auth?.workspace.id ?? '', minRole);
    return { auth: auth as Auth, workspace };
  } catch (err) {
    if (err instanceof AuthzError) return { res: authzResponse(err, shape) };
    throw err;
  }
}

/** For platform-admin endpoints. The role is read from the database on every request. */
export async function requireAdmin(): Promise<{ auth: Auth; res?: undefined } | { auth?: undefined; res: NextResponse }> {
  const auth = await getSession();
  if (!auth) return { res: NextResponse.json({ ok: false, error: 'Not signed in.' }, { status: 401 }) };
  if (auth.user.role !== 'admin') return { res: NextResponse.json({ ok: false, error: 'Admin privileges required.' }, { status: 403 }) };
  return { auth };
}

/**
 * Step-up for sensitive actions (adding a broker key, deleting a workspace, changing roles, managing
 * the authenticator): a 403 asking the client to confirm it's the owner (POST /api/auth/reauth) and
 * retry, unless this session did so in the last 10 minutes. Signing in counts.
 */
export function requireRecentAuth(session: SessionInfo): NextResponse | null {
  if (isRecentlyAuthenticated(session)) return null;
  return NextResponse.json({ ok: false, reauth: true, error: 'Please confirm it’s you to continue.' }, { status: 403 });
}

/** Actions that need two-step login switched on first (team ownership, broker keys). */
export function requireTwoFactorOn(user: UserRecord, why: string): NextResponse | null {
  if (twoFactorOn(user)) return null;
  return NextResponse.json({ ok: false, twoFactorRequired: true, error: `Turn on two-step login (Account → Login security) before ${why}.` }, { status: 403 });
}
