import crypto from 'crypto';
import { authSecret, toUser, USER_COLUMNS, type UserRecord, type UserRow } from './authStore';
import { query } from './db';
import { getMembership, ROLE_RANK, type Workspace, type WorkspaceRole } from './workspaces';

/**
 * API tokens (M19 task 17): `Authorization: Bearer <token>` on /api/engine/* for scripts,
 * notebooks and CI. See db/migrations/010_api_tokens.sql for the model. Server-only.
 *
 *   spectra_pat_<43 chars>   personal: acts as its owner, capped by their current role
 *   spectra_wat_<43 chars>   workspace: belongs to the workspace, made by an owner
 *
 * The token is 32 random bytes, shown once; the database keeps HMAC-SHA256(AUTH_SECRET) of it, so
 * a database leak doesn't leak usable tokens (and changing AUTH_SECRET invalidates them all, like
 * sessions). The prefix lets secret scanners (gitleaks, see .gitleaks.toml) spot leaked tokens.
 */

export type TokenKind = 'personal' | 'workspace';
export type TokenScope = 'read' | 'run';

export const TOKEN_PREFIX: Record<TokenKind, string> = { personal: 'spectra_pat_', workspace: 'spectra_wat_' };
export const TOKEN_RE = /^spectra_(pat|wat)_[A-Za-z0-9_-]{43}$/;
export const MAX_TOKEN_DAYS = 365;
export const DEFAULT_TOKEN_DAYS = 90;
export const MAX_TOKEN_NAME = 60;
/** Live tokens allowed per person per workspace (personal) and per workspace (workspace tokens). */
export const MAX_PERSONAL_TOKENS = 10;
export const MAX_WORKSPACE_TOKENS = 25;
const TOUCH_MS = 60_000;

/** The highest role a scope set can act with. Owner-level actions are never available to tokens. */
export const scopeRole = (scopes: readonly TokenScope[]): WorkspaceRole => (scopes.includes('run') ? 'editor' : 'viewer');
/** The scope a route needs, from its minimum role. Null: not available to tokens at all. */
export const scopeFor = (minRole: WorkspaceRole): TokenScope | null => (minRole === 'viewer' ? 'read' : minRole === 'editor' ? 'run' : null);

export class TokenError extends Error {
  constructor(public status: 400 | 403 | 404 | 409, message: string) {
    super(message);
    this.name = 'TokenError';
  }
}

export type ApiTokenInfo = {
  id: string;
  kind: TokenKind;
  workspaceId: string;
  name: string;
  scopes: TokenScope[];
  hint: string;
  /** Personal tokens: whose. */
  owner: { id: string; email: string } | null;
  createdBy: string | null;
  createdAt: number;
  expiresAt: number;
  lastUsedAt: number | null;
  lastUsedIp: string | null;
  revokedAt: number | null;
};

type Row = {
  id: string;
  kind: TokenKind;
  workspace_id: string;
  name: string;
  scopes: TokenScope[];
  hint: string;
  user_id: string | null;
  owner_email: string | null;
  creator_email: string | null;
  created_at: Date;
  expires_at: Date;
  last_used_at: Date | null;
  last_used_ip: string | null;
  revoked_at: Date | null;
};

const toInfo = (r: Row): ApiTokenInfo => ({
  id: r.id,
  kind: r.kind,
  workspaceId: r.workspace_id,
  name: r.name,
  scopes: r.scopes,
  hint: r.hint,
  owner: r.user_id ? { id: r.user_id, email: r.owner_email ?? '' } : null,
  createdBy: r.creator_email,
  createdAt: r.created_at.getTime(),
  expiresAt: r.expires_at.getTime(),
  lastUsedAt: r.last_used_at?.getTime() ?? null,
  lastUsedIp: r.last_used_ip,
  revokedAt: r.revoked_at?.getTime() ?? null,
});

const SELECT = `
  SELECT t.id, t.kind, t.workspace_id, t.name, t.scopes, t.hint, t.user_id, o.email AS owner_email, c.email AS creator_email,
         t.created_at, t.expires_at, t.last_used_at, t.last_used_ip, t.revoked_at
    FROM api_tokens t
    LEFT JOIN users o ON o.id = t.user_id
    LEFT JOIN users c ON c.id = t.created_by`;

const tokenHash = (token: string) => crypto.createHmac('sha256', authSecret()).update(`api-token:${token}`).digest();

export function cleanTokenName(name: unknown): string {
  if (typeof name !== 'string') return '';
  return name.replace(/[\u0000-\u001f\u007f]/g, '').replace(/\s+/g, ' ').trim().slice(0, MAX_TOKEN_NAME).trim();
}

/** Validate what the client asked for: 'read' or 'run' (which includes read). */
export function parseScopes(raw: unknown): TokenScope[] | null {
  if (!Array.isArray(raw) || raw.length === 0 || raw.length > 2) return null;
  const set = new Set(raw);
  if ([...set].some((s) => s !== 'read' && s !== 'run')) return null;
  return set.has('run') ? ['read', 'run'] : ['read'];
}

/**
 * Make a token. `by` is the caller with their role in `workspace` (from requireWorkspace).
 * Personal: any member; 'run' needs editor or owner. Workspace tokens: owners only.
 * Returns the token (show it once) and its listing.
 */
export async function createToken(input: {
  by: { id: string };
  workspace: Workspace;
  kind: unknown;
  name: unknown;
  scopes: unknown;
  expiresInDays: unknown;
}): Promise<{ token: string; info: ApiTokenInfo }> {
  const kind = input.kind === 'workspace' ? 'workspace' : input.kind === 'personal' || input.kind === undefined ? 'personal' : null;
  if (!kind) throw new TokenError(400, 'Choose a personal or a workspace token.');
  const name = cleanTokenName(input.name);
  if (!name) throw new TokenError(400, 'Give the token a name, e.g. "research notebook".');
  const scopes = parseScopes(input.scopes);
  if (!scopes) throw new TokenError(400, 'Choose what the token can do: read, or read and run.');
  const days = input.expiresInDays === undefined ? DEFAULT_TOKEN_DAYS : Number(input.expiresInDays);
  if (!Number.isInteger(days) || days < 1 || days > MAX_TOKEN_DAYS) throw new TokenError(400, `Tokens expire after 1 to ${MAX_TOKEN_DAYS} days.`);

  const role = input.workspace.role;
  if (kind === 'workspace' && role !== 'owner') throw new TokenError(403, 'Only owners can make workspace tokens.');
  if (ROLE_RANK[role] < ROLE_RANK[scopeRole(scopes)]) throw new TokenError(403, 'Viewers can only make read-only tokens.');

  const [{ n }] = await query<{ n: number }>(
    `SELECT count(*)::int AS n FROM api_tokens
      WHERE workspace_id = $1 AND kind = $2 AND ($3::uuid IS NULL OR user_id = $3) AND revoked_at IS NULL AND expires_at > now()`,
    [input.workspace.id, kind, kind === 'personal' ? input.by.id : null],
  );
  const max = kind === 'personal' ? MAX_PERSONAL_TOKENS : MAX_WORKSPACE_TOKENS;
  if (n >= max) throw new TokenError(409, `You can have at most ${max} live ${kind} tokens here. Revoke one you no longer use.`);

  const token = TOKEN_PREFIX[kind] + crypto.randomBytes(32).toString('base64url');
  const [{ id }] = await query<{ id: string }>(
    `INSERT INTO api_tokens (kind, workspace_id, user_id, created_by, name, scopes, token_hash, hint, expires_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() + $9 * interval '1 day')
     RETURNING id`,
    [kind, input.workspace.id, kind === 'personal' ? input.by.id : null, input.by.id, name, scopes, tokenHash(token), token.slice(-4), days],
  );
  const [row] = await query<Row>(`${SELECT} WHERE t.id = $1`, [id]);
  return { token, info: toInfo(row) };
}

/**
 * Tokens of a workspace that are still live (not revoked, not expired). Owners see every token
 * that can reach the workspace; everyone else sees only their own.
 */
export async function listTokens(workspace: Workspace, viewerId: string): Promise<ApiTokenInfo[]> {
  const rows = await query<Row>(
    `${SELECT} WHERE t.workspace_id = $1 AND t.revoked_at IS NULL AND t.expires_at > now() AND ($2::uuid IS NULL OR t.user_id = $2)
      ORDER BY t.created_at DESC`,
    [workspace.id, workspace.role === 'owner' ? null : viewerId],
  );
  return rows.map(toInfo);
}

/** Revoke one token. Owners can revoke any token of the workspace; others only their own. Null if not found. */
export async function revokeToken(workspace: Workspace, tokenId: string, by: { id: string }): Promise<ApiTokenInfo | null> {
  if (!/^[0-9a-f-]{36}$/i.test(tokenId)) return null;
  const rows = await query<{ id: string }>(
    `UPDATE api_tokens SET revoked_at = now(), revoked_by = $3
      WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL AND ($4::uuid IS NULL OR user_id = $4)
      RETURNING id`,
    [tokenId, workspace.id, by.id, workspace.role === 'owner' ? null : by.id],
  );
  if (!rows.length) return null;
  const [row] = await query<Row>(`${SELECT} WHERE t.id = $1`, [tokenId]);
  return toInfo(row);
}

/** Revoke every personal token a person holds (in one workspace, or everywhere). Returns how many. */
export async function revokePersonalTokens(userId: string, workspaceId?: string, c?: Parameters<typeof query>[2]): Promise<number> {
  const rows = await query(
    `UPDATE api_tokens SET revoked_at = now() WHERE user_id = $1 AND ($2::uuid IS NULL OR workspace_id = $2) AND revoked_at IS NULL RETURNING id`,
    [userId, workspaceId ?? null],
    c,
  );
  return rows.length;
}

export type TokenAuth = {
  token: ApiTokenInfo;
  /** Personal tokens: the live account. Workspace tokens: null. */
  user: UserRecord | null;
  /** The token's workspace, with the role the token acts with (never owner). */
  workspace: Workspace;
};

/**
 * Who a bearer token acts as, or null for anything unusable: malformed, unknown, revoked or
 * expired; a personal token whose owner is disabled or no longer a member. Never throws for bad
 * input (database errors propagate).
 */
export async function authenticateToken(raw: string | null | undefined, ip?: string | null): Promise<TokenAuth | null> {
  if (!raw || !TOKEN_RE.test(raw)) return null;
  let hash: Buffer;
  try {
    hash = tokenHash(raw);
  } catch {
    return null; // AUTH_SECRET missing: fail closed
  }
  const [row] = await query<Row>(`${SELECT} WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND t.expires_at > now()`, [hash]);
  if (!row) return null;
  const token = toInfo(row);
  // The prefix must match the stored kind (a pasted token can't change what it is).
  if (!raw.startsWith(TOKEN_PREFIX[token.kind])) return null;
  const cap = scopeRole(token.scopes);

  let user: UserRecord | null = null;
  let workspace: Workspace | null;
  if (token.kind === 'personal') {
    const [u] = await query<UserRow>(`SELECT ${USER_COLUMNS} FROM users WHERE id = $1 AND status = 'active'`, [token.owner!.id]);
    if (!u) return null;
    user = toUser(u);
    const membership = await getMembership(user.id, token.workspaceId);
    if (!membership) return null;
    workspace = { ...membership, role: ROLE_RANK[membership.role] < ROLE_RANK[cap] ? membership.role : cap };
  } else {
    const [w] = await query<{ id: string; name: string; slug: string; personal_owner_id: string | null; created_at: Date }>(
      'SELECT id, name, slug, personal_owner_id, created_at FROM workspaces WHERE id = $1',
      [token.workspaceId],
    );
    if (!w) return null;
    workspace = { id: w.id, name: w.name, slug: w.slug, personal: w.personal_owner_id !== null, role: cap, createdAt: w.created_at.getTime() };
  }

  if (!token.lastUsedAt || Date.now() - token.lastUsedAt > TOUCH_MS) {
    await query(`UPDATE api_tokens SET last_used_at = now(), last_used_ip = $2 WHERE id = $1`, [token.id, ip ? ip.slice(0, 64) : null]);
  }
  return { token, user, workspace };
}

/** Pull a bearer token out of an Authorization header ("Bearer <token>"); undefined if there is none. */
export function bearerToken(header: string | null): string | undefined {
  if (!header) return undefined;
  const m = /^Bearer\s+(\S+)\s*$/i.exec(header);
  return m ? m[1] : '';
}
