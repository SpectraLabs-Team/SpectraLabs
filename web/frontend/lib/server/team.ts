import crypto from 'crypto';
import type { PoolClient } from 'pg';
import { authSecret } from './authStore';
import { query, transaction } from './db';
import type { WorkspaceRole } from './workspaces';

/**
 * Team management (M19 task 12). Server-only; callers check the caller's role first (owner for
 * everything here except leaving and accepting an invite).
 *
 * - Invites: random 32-byte token in an emailed link, valid 7 days, single use, and only the
 *   invited email address can accept it (a forwarded link is useless). Stored as
 *   HMAC-SHA256(AUTH_SECRET, token). Invites grant editor or viewer; owners are made by a role
 *   change or an ownership transfer.
 * - A team workspace always keeps at least one owner. Changes lock the workspace row, so two
 *   owners demoting each other at the same moment can't leave it ownerless.
 * - Owners must use two-step login: promoting someone to owner needs them to have it on.
 * - Personal workspaces have exactly one member and none of this applies.
 */

export class TeamError extends Error {
  constructor(
    public status: 400 | 403 | 404 | 409,
    message: string,
  ) {
    super(message);
    this.name = 'TeamError';
  }
}

export const INVITE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
export const INVITE_ROLES = ['editor', 'viewer'] as const;
export type InviteRole = (typeof INVITE_ROLES)[number];
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
const TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;

export const isInviteToken = (t: unknown): t is string => typeof t === 'string' && TOKEN_RE.test(t);
const inviteHash = (token: string) => crypto.createHmac('sha256', authSecret()).update(`invite:${token}`).digest();
const norm = (e: string) => e.trim().toLowerCase();

/** "a•••@example.com": enough for the right person to recognise, not a full address to a stranger. */
export function maskEmail(email: string): string {
  const [local, domain] = email.split('@');
  return `${local.slice(0, 1)}•••@${domain ?? ''}`;
}

type Queryable = Pick<PoolClient, 'query'>;

/** Lock the workspace row for the rest of the transaction; refuse personal workspaces. */
async function lockTeam(c: Queryable, workspaceId: string): Promise<{ name: string }> {
  const [ws] = await query<{ name: string; personal_owner_id: string | null }>('SELECT name, personal_owner_id FROM workspaces WHERE id = $1 FOR UPDATE', [workspaceId], c);
  if (!ws) throw new TeamError(404, 'Workspace not found.');
  if (ws.personal_owner_id) throw new TeamError(400, 'Personal workspaces can’t have other members. Create a team workspace to collaborate.');
  return { name: ws.name };
}

const ownerCount = async (c: Queryable, workspaceId: string) =>
  (await query<{ n: number }>(`SELECT count(*)::int AS n FROM workspace_members WHERE workspace_id = $1 AND role = 'owner'`, [workspaceId], c))[0].n;

async function memberRole(c: Queryable, workspaceId: string, userId: string): Promise<WorkspaceRole | null> {
  const [m] = await query<{ role: WorkspaceRole }>('SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2', [workspaceId, userId], c);
  return m?.role ?? null;
}

// ── Invites ──

export type InviteInfo = { id: string; email: string; role: InviteRole; invitedBy: string | null; createdAt: number; expiresAt: number };

type InviteRow = { id: string; email: string; role: InviteRole; invited_by_email: string | null; created_at: Date; expires_at: Date };
const toInvite = (r: InviteRow): InviteInfo => ({
  id: r.id,
  email: r.email,
  role: r.role,
  invitedBy: r.invited_by_email,
  createdAt: r.created_at.getTime(),
  expiresAt: r.expires_at.getTime(),
});

/** Invite an email address (a new invite to the same address replaces the open one). Returns the token for the link. */
export async function createInvite(workspaceId: string, emailRaw: string, role: string, invitedBy: string): Promise<{ invite: InviteInfo; token: string }> {
  const email = norm(String(emailRaw ?? ''));
  if (!EMAIL_RE.test(email) || email.length > 254) throw new TeamError(400, 'Enter a valid email address.');
  if (!(INVITE_ROLES as readonly string[]).includes(role)) throw new TeamError(400, 'Invite as an editor or a viewer.');
  return transaction(async (c) => {
    await lockTeam(c, workspaceId);
    const [already] = await query('SELECT 1 FROM workspace_members m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 AND u.email = $2', [workspaceId, email], c);
    if (already) throw new TeamError(409, `${email} is already a member.`);
    await query('UPDATE workspace_invites SET revoked_at = now() WHERE workspace_id = $1 AND email = $2 AND accepted_at IS NULL AND revoked_at IS NULL', [workspaceId, email], c);
    const token = crypto.randomBytes(32).toString('base64url');
    const [row] = await query<{ id: string }>(
      `INSERT INTO workspace_invites (workspace_id, email, role, token_hash, invited_by, expires_at)
       VALUES ($1, $2, $3, $4, $5, now() + $6 * interval '1 millisecond') RETURNING id`,
      [workspaceId, email, role, inviteHash(token), invitedBy, INVITE_TTL_MS],
      c,
    );
    const [inv] = await query<InviteRow>(
      `SELECT i.id, i.email, i.role, u.email AS invited_by_email, i.created_at, i.expires_at
         FROM workspace_invites i LEFT JOIN users u ON u.id = i.invited_by WHERE i.id = $1`,
      [row.id],
      c,
    );
    return { invite: toInvite(inv), token };
  });
}

/** Open invites (not accepted, revoked or expired), newest first. */
export async function listInvites(workspaceId: string): Promise<InviteInfo[]> {
  const rows = await query<InviteRow>(
    `SELECT i.id, i.email, i.role, u.email AS invited_by_email, i.created_at, i.expires_at
       FROM workspace_invites i LEFT JOIN users u ON u.id = i.invited_by
      WHERE i.workspace_id = $1 AND i.accepted_at IS NULL AND i.revoked_at IS NULL AND i.expires_at > now()
      ORDER BY i.created_at DESC`,
    [workspaceId],
  );
  return rows.map(toInvite);
}

export async function revokeInvite(workspaceId: string, inviteId: string): Promise<boolean> {
  if (!/^[0-9a-f-]{36}$/i.test(inviteId)) return false;
  const rows = await query('UPDATE workspace_invites SET revoked_at = now() WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL RETURNING id', [inviteId, workspaceId]);
  return rows.length > 0;
}

export type InvitePreview =
  | { status: 'open'; workspaceName: string; role: InviteRole; email: string; invitedBy: string | null; expiresAt: number }
  | { status: 'expired' | 'used' | 'revoked' | 'missing' };

/** What the invite page shows before accepting. The invited email is masked. */
export async function previewInvite(token: string): Promise<InvitePreview> {
  if (!isInviteToken(token)) return { status: 'missing' };
  const [r] = await query<{ email: string; role: InviteRole; expires_at: Date; accepted_at: Date | null; revoked_at: Date | null; workspace_name: string; inviter_name: string | null; inviter_email: string | null }>(
    `SELECT i.email, i.role, i.expires_at, i.accepted_at, i.revoked_at, w.name AS workspace_name, u.name AS inviter_name, u.email AS inviter_email
       FROM workspace_invites i JOIN workspaces w ON w.id = i.workspace_id LEFT JOIN users u ON u.id = i.invited_by
      WHERE i.token_hash = $1`,
    [inviteHash(token)],
  );
  if (!r) return { status: 'missing' };
  if (r.accepted_at) return { status: 'used' };
  if (r.revoked_at) return { status: 'revoked' };
  if (r.expires_at.getTime() <= Date.now()) return { status: 'expired' };
  return { status: 'open', workspaceName: r.workspace_name, role: r.role, email: maskEmail(r.email), invitedBy: r.inviter_name || r.inviter_email, expiresAt: r.expires_at.getTime() };
}

/**
 * Accept an invite as the signed-in user. Single use; the account's email must be the invited
 * one. If they are already a member, their role is kept (an invite never demotes anyone).
 */
export async function acceptInvite(token: string, user: { id: string; email: string }): Promise<{ workspaceId: string; workspaceName: string; role: WorkspaceRole; inviteId: string }> {
  if (!isInviteToken(token)) throw new TeamError(404, 'This invite link isn’t valid.');
  return transaction(async (c) => {
    const [inv] = await query<{ id: string; workspace_id: string; email: string; role: InviteRole; expires_at: Date; accepted_at: Date | null; revoked_at: Date | null }>(
      'SELECT id, workspace_id, email, role, expires_at, accepted_at, revoked_at FROM workspace_invites WHERE token_hash = $1 FOR UPDATE',
      [inviteHash(token)],
      c,
    );
    if (!inv) throw new TeamError(404, 'This invite link isn’t valid.');
    if (inv.accepted_at) throw new TeamError(409, 'This invite has already been used.');
    if (inv.revoked_at) throw new TeamError(409, 'This invite was cancelled. Ask for a new one.');
    if (inv.expires_at.getTime() <= Date.now()) throw new TeamError(409, 'This invite has expired. Ask for a new one.');
    if (norm(user.email) !== inv.email) throw new TeamError(403, `This invite is for ${maskEmail(inv.email)}. Sign in with that address to accept it.`);

    const { name } = await lockTeam(c, inv.workspace_id);
    const existing = await memberRole(c, inv.workspace_id, user.id);
    if (!existing) await query('INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)', [inv.workspace_id, user.id, inv.role], c);
    await query('UPDATE workspace_invites SET accepted_at = now(), accepted_by = $2 WHERE id = $1', [inv.id, user.id], c);
    return { workspaceId: inv.workspace_id, workspaceName: name, role: existing ?? inv.role, inviteId: inv.id };
  });
}

// ── Members ──

const hasTwoFactor = async (c: Queryable, userId: string) =>
  (await query<{ two_factor: boolean; platform_role: string }>('SELECT two_factor, platform_role FROM users WHERE id = $1', [userId], c)).some((u) => u.two_factor || u.platform_role === 'admin');

/** Change a member's role. Keeps at least one owner; new owners must use two-step login. */
export async function changeRole(workspaceId: string, userId: string, role: string): Promise<{ from: WorkspaceRole; to: WorkspaceRole }> {
  if (!['owner', 'editor', 'viewer'].includes(role)) throw new TeamError(400, 'Role must be owner, editor or viewer.');
  const to = role as WorkspaceRole;
  return transaction(async (c) => {
    await lockTeam(c, workspaceId);
    const from = await memberRole(c, workspaceId, userId);
    if (!from) throw new TeamError(404, 'Not a member of this workspace.');
    if (from === to) return { from, to };
    if (from === 'owner' && (await ownerCount(c, workspaceId)) <= 1) throw new TeamError(409, 'A workspace needs at least one owner. Make someone else an owner first.');
    if (to === 'owner' && !(await hasTwoFactor(c, userId))) throw new TeamError(409, 'They need to turn on two-step login before they can become an owner.');
    await query('UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2', [workspaceId, userId, to], c);
    return { from, to };
  });
}

/** Remove a member (or leave). Keeps at least one owner. Their sessions in it fall back to personal at once. */
export async function removeMember(workspaceId: string, userId: string): Promise<{ role: WorkspaceRole }> {
  return transaction(async (c) => {
    await lockTeam(c, workspaceId);
    const role = await memberRole(c, workspaceId, userId);
    if (!role) throw new TeamError(404, 'Not a member of this workspace.');
    if (role === 'owner' && (await ownerCount(c, workspaceId)) <= 1) {
      throw new TeamError(409, 'The last owner can’t leave or be removed. Transfer ownership first, or delete the workspace.');
    }
    await query('DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2', [workspaceId, userId], c);
    // Their personal API tokens for this workspace stop working anyway (membership is checked on
    // every use); revoking them keeps the token list honest.
    await query('UPDATE api_tokens SET revoked_at = now() WHERE workspace_id = $1 AND user_id = $2 AND revoked_at IS NULL', [workspaceId, userId], c);
    return { role };
  });
}

/** Hand ownership to another member: they become owner, the caller becomes an editor. */
export async function transferOwnership(workspaceId: string, fromUserId: string, toUserId: string): Promise<void> {
  if (fromUserId === toUserId) throw new TeamError(400, 'Choose another member.');
  await transaction(async (c) => {
    await lockTeam(c, workspaceId);
    if ((await memberRole(c, workspaceId, fromUserId)) !== 'owner') throw new TeamError(403, 'Only an owner can transfer ownership.');
    if (!(await memberRole(c, workspaceId, toUserId))) throw new TeamError(404, 'They aren’t a member of this workspace.');
    if (!(await hasTwoFactor(c, toUserId))) throw new TeamError(409, 'They need to turn on two-step login before they can become an owner.');
    await query(`UPDATE workspace_members SET role = 'owner' WHERE workspace_id = $1 AND user_id = $2`, [workspaceId, toUserId], c);
    await query(`UPDATE workspace_members SET role = 'editor' WHERE workspace_id = $1 AND user_id = $2`, [workspaceId, fromUserId], c);
  });
}
