import crypto from 'crypto';
import { query } from './db';

/**
 * Audit log (M19 task 9): append-only, hash-chained table `audit_events` (see
 * db/migrations/006_audit.sql). Server-only.
 *
 * record() never throws: a failed audit write is logged loudly but doesn't fail the user's action
 * (signing in must not depend on the audit table). Never put passwords, codes, tokens or keys in
 * `details`; everything here is visible to admins and, for workspace events, to workspace owners.
 */

export const AUDIT_ACTIONS = [
  // Sign-in and account
  'auth.signup',
  'auth.login.success',
  'auth.login.failure',
  'auth.login.locked',
  'auth.login.new_device',
  'auth.logout',
  'auth.two_factor.enabled',
  'auth.two_factor.disabled',
  'auth.password.changed',
  'auth.password.reset_requested',
  'auth.password.reset',
  'auth.session.revoked',
  'auth.session.revoked_others',
  'auth.totp.enabled',
  'auth.totp.disabled',
  'auth.recovery_codes.regenerated',
  'auth.recovery_code.used',
  'auth.reauth',
  'account.deleted',
  // Platform admin
  'admin.role.granted',
  'admin.role.revoked',
  'admin.user.disabled',
  'admin.user.enabled',
  'admin.user.viewed',
  // Workspaces and membership
  'workspace.created',
  'workspace.renamed',
  'workspace.deleted',
  'workspace.layout_published',
  'membership.invited',
  'membership.invite_revoked',
  'membership.added',
  'membership.role_changed',
  'membership.removed',
  'membership.left',
  'workspace.ownership_transferred',
  // Broker keys
  'secret.added',
  'secret.replaced',
  'secret.deleted',
  'secret.used',
  // API tokens (lib/server/apiTokens.ts); every use is logged
  'api_token.created',
  'api_token.revoked',
  'api_token.used',
  // Data Studio (M1): a file uploaded for ingesting (the engine logs what it became)
  'dataset.uploaded',
  // Trading (M9)
  'order.placed',
  'kill_switch.used',
  // The audit log itself
  'audit.exported',
  // Site-wide spikes of failed sign-ins or code emails (lib/server/spikes.ts)
  'security.alert',
] as const;
export type AuditAction = (typeof AUDIT_ACTIONS)[number];
export type AuditOutcome = 'success' | 'failure' | 'denied';

export type AuditInput = {
  action: AuditAction;
  outcome?: AuditOutcome;
  actor?: { id?: string | null; email?: string | null } | null;
  workspaceId?: string | null;
  target?: { type: string; id: string } | null;
  details?: Record<string, unknown>;
};

/** Where a request came from. Build it with requestMeta(req) in a route handler. */
export type RequestMeta = { ip: string | null; userAgent: string | null; requestId: string };

/** IP, user agent and a request id (the proxy's X-Request-Id if any, else a fresh one). */
export function requestMeta(req: Request): RequestMeta {
  const xff = req.headers.get('x-forwarded-for');
  const ip = (xff ? xff.split(',')[0].trim() : req.headers.get('x-real-ip')?.trim()) || null;
  const rid = req.headers.get('x-request-id');
  return {
    ip,
    userAgent: req.headers.get('user-agent')?.slice(0, 512) ?? null,
    requestId: rid && /^[A-Za-z0-9._:-]{1,128}$/.test(rid) ? rid : crypto.randomUUID(),
  };
}

const MAX_DETAILS = 4096;

/** Append one event. Never throws (see above). */
export async function record(event: AuditInput, meta?: RequestMeta | null): Promise<void> {
  try {
    let details = JSON.stringify(event.details ?? {});
    if (details.length > MAX_DETAILS) details = JSON.stringify({ truncated: true });
    await query(
      `INSERT INTO audit_events (actor_id, actor_email, workspace_id, action, outcome, target_type, target_id, ip, user_agent, request_id, details)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb)`,
      [
        event.actor?.id ?? null,
        event.actor?.email ? event.actor.email.slice(0, 320) : null,
        event.workspaceId ?? null,
        event.action,
        event.outcome ?? 'success',
        event.target?.type ?? null,
        event.target?.id ?? null,
        meta?.ip ?? null,
        meta?.userAgent ?? null,
        meta?.requestId ?? null,
        details,
      ],
    );
  } catch (err) {
    console.error(`[AUDIT WRITE FAILED] ${event.action}:`, err instanceof Error ? err.message : err);
  }
}

// ── Reading ──

export type AuditEvent = {
  id: string;
  seq: string;
  occurredAt: number;
  actorId: string | null;
  actorEmail: string | null;
  workspaceId: string | null;
  workspaceName: string | null;
  action: string;
  outcome: AuditOutcome;
  targetType: string | null;
  targetId: string | null;
  ip: string | null;
  userAgent: string | null;
  requestId: string | null;
  details: Record<string, unknown>;
  hash: string;
};

export type AuditFilter = {
  workspaceId?: string;
  /** Exact action, or a prefix ending in '.' (e.g. "auth."). */
  action?: string;
  outcome?: AuditOutcome;
  /** Matches the actor's email (case-insensitive substring). */
  actor?: string;
  from?: Date;
  to?: Date;
  /** Events done by this user or done to them (target user). */
  subjectUserId?: string;
  /** Page backwards: events with seq lower than this. */
  beforeSeq?: string;
  limit?: number;
};

type Row = {
  id: string;
  seq: string;
  occurred_at: Date;
  actor_id: string | null;
  actor_email: string | null;
  workspace_id: string | null;
  workspace_name: string | null;
  action: string;
  outcome: AuditOutcome;
  target_type: string | null;
  target_id: string | null;
  ip: string | null;
  user_agent: string | null;
  request_id: string | null;
  details: Record<string, unknown>;
  hash: Buffer;
};

const toEvent = (r: Row): AuditEvent => ({
  id: String(r.id),
  seq: String(r.seq),
  occurredAt: r.occurred_at.getTime(),
  actorId: r.actor_id,
  actorEmail: r.actor_email,
  workspaceId: r.workspace_id,
  workspaceName: r.workspace_name,
  action: r.action,
  outcome: r.outcome,
  targetType: r.target_type,
  targetId: r.target_id,
  ip: r.ip,
  userAgent: r.user_agent,
  requestId: r.request_id,
  details: r.details,
  hash: r.hash.toString('hex'),
});

/** Newest first. `limit` is capped at 1000 (CSV export pages through with beforeSeq). */
export async function listAudit(f: AuditFilter = {}): Promise<AuditEvent[]> {
  const where: string[] = [];
  const params: unknown[] = [];
  const add = (sql: string, v: unknown) => {
    params.push(v);
    where.push(sql.replace('?', `$${params.length}`));
  };
  if (f.workspaceId) add('e.workspace_id = ?', f.workspaceId);
  if (f.subjectUserId) {
    params.push(f.subjectUserId);
    const n = params.length;
    where.push(`(e.actor_id = $${n}::uuid OR (e.target_type = 'user' AND e.target_id = $${n}::text))`);
  }
  if (f.action) {
    if (f.action.endsWith('.')) add(`e.action LIKE ? || '%'`, f.action.replace(/[%_\\]/g, '\\$&'));
    else add('e.action = ?', f.action);
  }
  if (f.outcome) add('e.outcome = ?', f.outcome);
  if (f.actor) add(`e.actor_email ILIKE '%' || ? || '%'`, f.actor.replace(/[%_\\]/g, '\\$&'));
  if (f.from) add('e.occurred_at >= ?', f.from);
  if (f.to) add('e.occurred_at < ?', f.to);
  if (f.beforeSeq && /^\d+$/.test(f.beforeSeq)) add('e.seq < ?::bigint', f.beforeSeq);
  params.push(Math.min(Math.max(1, f.limit ?? 100), 1000));
  const rows = await query<Row>(
    `SELECT e.id, e.seq, e.occurred_at, e.actor_id, e.actor_email, e.workspace_id, w.name AS workspace_name,
            e.action, e.outcome, e.target_type, e.target_id, e.ip, e.user_agent, e.request_id, e.details, e.hash
       FROM audit_events e LEFT JOIN workspaces w ON w.id = e.workspace_id
      ${where.length ? `WHERE ${where.join(' AND ')}` : ''}
      ORDER BY e.seq DESC
      LIMIT $${params.length}`,
    params,
  );
  return rows.map(toEvent);
}

/** Check the whole hash chain. `head` is the newest hash: keep a copy elsewhere to detect truncation. */
export async function verifyChain(): Promise<{ ok: boolean; events: number; head: string | null; breaks: { seq: string; problem: string }[] }> {
  const breaks = await query<{ seq: string; problem: string }>('SELECT seq::text, problem FROM audit_chain_breaks() LIMIT 20');
  const [stat] = await query<{ n: string; head: Buffer | null }>('SELECT count(*)::text AS n, (SELECT hash FROM audit_events ORDER BY seq DESC LIMIT 1) AS head FROM audit_events');
  return { ok: breaks.length === 0, events: Number(stat.n), head: stat.head ? stat.head.toString('hex') : null, breaks };
}

// ── CSV export ──

const CSV_COLUMNS: [string, (e: AuditEvent) => unknown][] = [
  ['seq', (e) => e.seq],
  ['time_utc', (e) => new Date(e.occurredAt).toISOString()],
  ['action', (e) => e.action],
  ['outcome', (e) => e.outcome],
  ['actor_email', (e) => e.actorEmail],
  ['actor_id', (e) => e.actorId],
  ['workspace', (e) => e.workspaceName],
  ['workspace_id', (e) => e.workspaceId],
  ['target_type', (e) => e.targetType],
  ['target_id', (e) => e.targetId],
  ['ip', (e) => e.ip],
  ['user_agent', (e) => e.userAgent],
  ['request_id', (e) => e.requestId],
  ['details', (e) => (Object.keys(e.details).length ? JSON.stringify(e.details) : '')],
  ['hash', (e) => e.hash],
];

/**
 * One CSV field. Quotes when needed, and defuses spreadsheet formulas: a cell starting with
 * = + - @ (or tab/CR) is prefixed with ' so Excel/Sheets show it as text instead of running it.
 * (Emails and user agents are attacker-controlled.)
 */
export function csvField(v: unknown): string {
  let s = v === null || v === undefined ? '' : String(v);
  if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`;
  return /[",\r\n]/.test(s) || s !== s.trim() ? `"${s.replace(/"/g, '""')}"` : s;
}

export function toCsv(events: AuditEvent[]): string {
  const lines = [CSV_COLUMNS.map(([h]) => h).join(',')];
  for (const e of events) lines.push(CSV_COLUMNS.map(([, get]) => csvField(get(e))).join(','));
  return lines.join('\r\n') + '\r\n';
}

/** Parse the query-string filters shared by the admin and workspace audit routes. */
export function filterFromQuery(sp: URLSearchParams): AuditFilter {
  const date = (k: string) => {
    const v = sp.get(k);
    const d = v ? new Date(v) : null;
    return d && !Number.isNaN(d.getTime()) ? d : undefined;
  };
  const outcome = sp.get('outcome');
  const action = sp.get('action')?.trim();
  return {
    action: action && /^[a-z0-9_.]{1,64}$/.test(action) ? action : undefined,
    outcome: outcome === 'success' || outcome === 'failure' || outcome === 'denied' ? outcome : undefined,
    actor: sp.get('actor')?.trim().slice(0, 120) || undefined,
    from: date('from'),
    to: date('to'),
    beforeSeq: sp.get('before') ?? undefined,
    limit: Number(sp.get('limit')) || undefined,
  };
}
