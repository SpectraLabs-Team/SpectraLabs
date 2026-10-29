import { query } from './db';
import { listAudit, type AuditEvent } from './audit';
import { findUserById } from './authStore';
import { getMfaStatus } from './mfa';
import { listSessions } from './sessions';
import { describeUserAgent } from '../userAgent';

/**
 * Everything a platform admin needs to look at one account (M19 task 13). Server-only; callers
 * check requireAdmin. Never includes password hashes, codes, keys or session tokens.
 */
export type AdminUserDetail = {
  user: {
    id: string;
    email: string;
    name: string;
    role: 'admin' | 'user';
    status: 'active' | 'disabled';
    createdAt: number;
    passwordChangedAt: number | null;
    twoFactor: boolean;
    authenticator: 'off' | 'pending' | 'on';
    recoveryCodesLeft: number;
    failedLogins: number;
    lockedUntil: number | null;
  };
  workspaces: { id: string; name: string; personal: boolean; role: 'owner' | 'editor' | 'viewer'; members: number; joinedAt: number }[];
  sessions: { id: string; device: string; ip: string | null; createdAt: number; lastSeenAt: number }[];
  events: AuditEvent[];
  /** `true` when IP addresses and browser strings were left out (the default). */
  ipsHidden: boolean;
};

/**
 * IP addresses and browser strings (on sessions and audit events) are left out unless `ips` is
 * set, so an admin sees them only after choosing "Show IPs", which the route audit-logs.
 */
export async function getAdminUserDetail(userId: string, opts: { ips?: boolean } = {}): Promise<AdminUserDetail | null> {
  const ips = opts.ips === true;
  if (!/^[0-9a-f-]{36}$/i.test(userId)) return null;
  const u = await findUserById(userId);
  if (!u) return null;
  const [lock] = await query<{ failed_login_count: number; locked_until: Date | null }>('SELECT failed_login_count, locked_until FROM users WHERE id = $1', [userId]);
  const mfa = await getMfaStatus(userId);
  const workspaces = await query<{ id: string; name: string; personal_owner_id: string | null; role: 'owner' | 'editor' | 'viewer'; members: number; created_at: Date }>(
    `SELECT w.id, w.name, w.personal_owner_id, m.role, m.created_at,
            (SELECT count(*)::int FROM workspace_members x WHERE x.workspace_id = w.id) AS members
       FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id
      WHERE m.user_id = $1
      ORDER BY (w.personal_owner_id IS NULL), lower(w.name)`,
    [userId],
  );
  const sessions = await listSessions(userId);
  return {
    user: {
      id: u.id,
      email: u.email,
      name: u.name || '',
      role: u.role,
      status: u.status,
      createdAt: u.createdAt,
      passwordChangedAt: u.passwordChangedAt ?? null,
      twoFactor: u.role === 'admin' || !!u.twoFactor,
      authenticator: mfa.totp,
      recoveryCodesLeft: mfa.recoveryCodesLeft,
      failedLogins: lock?.failed_login_count ?? 0,
      lockedUntil: lock?.locked_until && lock.locked_until.getTime() > Date.now() ? lock.locked_until.getTime() : null,
    },
    workspaces: workspaces.map((w) => ({ id: w.id, name: w.name, personal: w.personal_owner_id !== null, role: w.role, members: w.members, joinedAt: w.created_at.getTime() })),
    sessions: sessions.map((s) => ({ id: s.id, device: describeUserAgent(s.userAgent).label, ip: ips ? s.ip : null, createdAt: s.createdAt, lastSeenAt: s.lastSeenAt })),
    events: (await listAudit({ subjectUserId: userId, limit: 30 })).map((e) => (ips ? e : { ...e, ip: null, userAgent: null })),
    ipsHidden: !ips,
  };
}
