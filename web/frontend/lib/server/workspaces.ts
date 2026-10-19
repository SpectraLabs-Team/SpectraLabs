import crypto from 'crypto';
import type { PoolClient } from 'pg';
import { query, transaction } from './db';

/**
 * Workspaces and membership (tables `workspaces`, `workspace_members`). Server-only.
 *
 * Every user has exactly one personal workspace (created with the account). Teams are workspaces
 * with several members. Research data is owned by a workspace, never directly by a user.
 * Roles: viewer = read only · editor = create and run · owner = members, settings, delete.
 */

export type WorkspaceRole = 'owner' | 'editor' | 'viewer';
export const WORKSPACE_ROLES: readonly WorkspaceRole[] = ['viewer', 'editor', 'owner'];
/** Higher rank = more rights. Task 4's requireRole compares against this. */
export const ROLE_RANK: Record<WorkspaceRole, number> = { viewer: 1, editor: 2, owner: 3 };

export const MAX_WORKSPACE_NAME = 60;

export type Workspace = {
  id: string;
  name: string;
  slug: string;
  personal: boolean;
  /** The current user's role in this workspace. */
  role: WorkspaceRole;
  createdAt: number;
};

type WorkspaceRow = { id: string; name: string; slug: string; personal_owner_id: string | null; role: WorkspaceRole; created_at: Date };

const toWorkspace = (r: WorkspaceRow): Workspace => ({
  id: r.id,
  name: r.name,
  slug: r.slug,
  personal: r.personal_owner_id !== null,
  role: r.role,
  createdAt: r.created_at.getTime(),
});

type Queryable = Pick<PoolClient, 'query'>;

/** Trim, collapse spaces, cap the length. Returns '' if nothing usable is left. */
export function cleanWorkspaceName(name: unknown): string {
  if (typeof name !== 'string') return '';
  const clean = name.replace(/\s+/g, ' ').trim();
  return clean.length >= 1 && clean.length <= MAX_WORKSPACE_NAME ? clean : '';
}

/** URL-safe slug: lower-case words joined by '-', plus a short random suffix so it is unique. */
export function makeSlug(base: string): string {
  const stem = base.toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 40).replace(/^-+|-+$/g, '') || 'workspace';
  return `${stem}-${crypto.randomBytes(3).toString('hex')}`;
}

const isUniqueViolation = (err: unknown, constraint: string) =>
  (err as { code?: string; constraint?: string })?.code === '23505' && (err as { constraint?: string }).constraint === constraint;

async function insertWorkspace(c: Queryable, name: string, slugBase: string, createdBy: string, personal: boolean): Promise<string> {
  // A slug collision is astronomically unlikely (24 random bits per name), but retry anyway.
  for (let attempt = 0; ; attempt++) {
    try {
      const [row] = await query<{ id: string }>(
        `INSERT INTO workspaces (name, slug, created_by, personal_owner_id) VALUES ($1, $2, $3, $4) RETURNING id`,
        [name, makeSlug(slugBase), createdBy, personal ? createdBy : null],
        c,
      );
      return row.id;
    } catch (err) {
      if (attempt < 3 && isUniqueViolation(err, 'workspaces_slug_key')) continue;
      throw err;
    }
  }
}

/**
 * Create the personal workspace for a new account (owner role). Call inside the transaction that
 * creates the user. A no-op if the user already has one.
 */
export async function createPersonalWorkspace(c: Queryable, userId: string, email: string): Promise<void> {
  const [existing] = await query('SELECT 1 FROM workspaces WHERE personal_owner_id = $1', [userId], c);
  if (existing) return;
  const id = await insertWorkspace(c, 'Personal', email.split('@')[0], userId, true);
  await query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')`, [id, userId], c);
}

const SELECT_FOR_USER = `
  SELECT w.id, w.name, w.slug, w.personal_owner_id, w.created_at, m.role
    FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id
   WHERE m.user_id = $1`;

/** Create a team workspace; the creator becomes its owner. */
export async function createWorkspace(userId: string, name: string): Promise<Workspace> {
  return transaction(async (c) => {
    const id = await insertWorkspace(c, name, name, userId, false);
    await query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')`, [id, userId], c);
    const [row] = await query<WorkspaceRow>(`${SELECT_FOR_USER} AND w.id = $2`, [userId, id], c);
    return toWorkspace(row);
  });
}

/** Every workspace the user belongs to: personal first, then teams by name. */
export async function listWorkspaces(userId: string): Promise<Workspace[]> {
  const rows = await query<WorkspaceRow>(`${SELECT_FOR_USER} ORDER BY (w.personal_owner_id IS NULL), lower(w.name), w.created_at`, [userId]);
  return rows.map(toWorkspace);
}

/** The workspace with the user's role in it, or null if they aren't a member. */
export async function getMembership(userId: string, workspaceId: string, c?: Queryable): Promise<Workspace | null> {
  if (!/^[0-9a-f-]{36}$/i.test(workspaceId)) return null;
  const [row] = await query<WorkspaceRow>(`${SELECT_FOR_USER} AND w.id = $2`, [userId, workspaceId], c);
  return row ? toWorkspace(row) : null;
}

/** The user's personal workspace (created on the spot if it is somehow missing). */
export async function personalWorkspace(userId: string, email: string): Promise<Workspace> {
  const find = async () => (await query<WorkspaceRow>(`${SELECT_FOR_USER} AND w.personal_owner_id = $1`, [userId]))[0];
  let row = await find();
  if (!row) {
    await transaction((c) => createPersonalWorkspace(c, userId, email));
    row = await find();
  }
  return toWorkspace(row);
}

/**
 * Where a new session starts: the workspace of the user's most recent session if they are still a
 * member (so a sign-in lands where they left off), otherwise their personal workspace.
 */
export async function defaultWorkspaceId(userId: string, email: string): Promise<string> {
  const [recent] = await query<{ workspace_id: string }>(
    `SELECT s.workspace_id FROM sessions s
       JOIN workspace_members m ON m.workspace_id = s.workspace_id AND m.user_id = s.user_id
      WHERE s.user_id = $1
      ORDER BY s.last_seen_at DESC LIMIT 1`,
    [userId],
  );
  return recent?.workspace_id ?? (await personalWorkspace(userId, email)).id;
}

/** Point a session at a workspace the user belongs to. Returns the workspace, or null if not a member. */
export async function setActiveWorkspace(sessionId: string, userId: string, workspaceId: string): Promise<Workspace | null> {
  const ws = await getMembership(userId, workspaceId);
  if (!ws) return null;
  await query('UPDATE sessions SET workspace_id = $1 WHERE id = $2 AND user_id = $3', [ws.id, sessionId, userId]);
  return ws;
}

export class SoleOwnerError extends Error {
  constructor(public workspaces: string[]) {
    super(`You are the only owner of ${workspaces.map((w) => `"${w}"`).join(', ')}. Make someone else an owner or remove the other members first.`);
    this.name = 'SoleOwnerError';
  }
}

/**
 * Before an account is deleted (inside the same transaction): team workspaces where the user is
 * the only member are deleted with it; if the user is the only owner of a team that still has
 * other members, throw SoleOwnerError (a workspace must always keep an owner). Personal
 * workspaces go with the account via ON DELETE CASCADE.
 */
export async function releaseWorkspacesOf(c: Queryable, userId: string): Promise<void> {
  const blocking = await query<{ name: string }>(
    `SELECT w.name FROM workspaces w
       JOIN workspace_members me ON me.workspace_id = w.id AND me.user_id = $1 AND me.role = 'owner'
      WHERE w.personal_owner_id IS NULL
        AND NOT EXISTS (SELECT 1 FROM workspace_members o WHERE o.workspace_id = w.id AND o.user_id <> $1 AND o.role = 'owner')
        AND EXISTS (SELECT 1 FROM workspace_members x WHERE x.workspace_id = w.id AND x.user_id <> $1)
      ORDER BY w.name`,
    [userId],
    c,
  );
  if (blocking.length) throw new SoleOwnerError(blocking.map((b) => b.name));
  await query(
    `DELETE FROM workspaces w
      WHERE w.personal_owner_id IS NULL
        AND EXISTS (SELECT 1 FROM workspace_members me WHERE me.workspace_id = w.id AND me.user_id = $1)
        AND NOT EXISTS (SELECT 1 FROM workspace_members x WHERE x.workspace_id = w.id AND x.user_id <> $1)`,
    [userId],
    c,
  );
}

// ── Workspace settings (callers check the role first: see requireRole in ./authz) ──

export type Member = { userId: string; email: string; name: string; role: WorkspaceRole; joinedAt: number };

/** Members of a workspace, owners first. Caller must be at least a viewer. */
export async function listMembers(workspaceId: string): Promise<Member[]> {
  const rows = await query<{ user_id: string; email: string; name: string; role: WorkspaceRole; created_at: Date }>(
    `SELECT m.user_id, u.email, u.name, m.role, m.created_at
       FROM workspace_members m JOIN users u ON u.id = m.user_id
      WHERE m.workspace_id = $1
      ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'editor' THEN 1 ELSE 2 END, lower(u.email)`,
    [workspaceId],
  );
  return rows.map((r) => ({ userId: r.user_id, email: r.email, name: r.name, role: r.role, joinedAt: r.created_at.getTime() }));
}

/** Rename a workspace. Caller must be an owner. */
export async function renameWorkspace(workspaceId: string, name: string): Promise<boolean> {
  const rows = await query('UPDATE workspaces SET name = $2 WHERE id = $1 RETURNING id', [workspaceId, name]);
  return rows.length > 0;
}

export class PersonalWorkspaceError extends Error {
  constructor() {
    super('Your personal workspace can’t be deleted. It goes away only with your account.');
    this.name = 'PersonalWorkspaceError';
  }
}

/**
 * Delete a team workspace and everything it owns (ON DELETE CASCADE). Caller must be an owner.
 * Sessions that had it active fall back to their user's personal workspace on the next request.
 */
export async function deleteWorkspace(workspaceId: string): Promise<boolean> {
  return transaction(async (c) => {
    const [ws] = await query<{ personal_owner_id: string | null }>('SELECT personal_owner_id FROM workspaces WHERE id = $1 FOR UPDATE', [workspaceId], c);
    if (!ws) return false;
    if (ws.personal_owner_id) throw new PersonalWorkspaceError();
    await query('DELETE FROM workspaces WHERE id = $1', [workspaceId], c);
    return true;
  });
}
