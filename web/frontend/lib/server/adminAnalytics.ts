import { query } from './db';
import { ACTIVITY_RETENTION_DAYS } from './activity';
import type { AdminOverview, AnalyticsDay, AdminAnalytics, DirectoryRow, DateRange } from '@/lib/adminTypes';

export type { AdminOverview, AnalyticsDay, AdminAnalytics, DirectoryRow, DateRange };

/**
 * Admin console data: real counts only, read from the database. Server-only; callers check
 * requireAdmin. Nothing here returns password hashes, keys, tokens or message contents.
 *
 * Engine tables (runs, jobs, workers, paper_orders) come from the Rust migrations. They are checked
 * with to_regclass first, so a database without them still shows the web-side numbers.
 */

const DAY_MS = 86_400_000;
export const MAX_RANGE_DAYS = 366;

const utcDay = (d: Date) => d.toISOString().slice(0, 10);

/** Parses `from`/`to` (YYYY-MM-DD, inclusive, UTC). Defaults to the last 30 days; caps at a year. */
export function parseRange(from: string | null, to: string | null, now: Date = new Date()): DateRange {
  // A real calendar date: the format must match and must survive a round trip (rejects 2026-02-31).
  const valid = (s: string | null) => !!s && /^\d{4}-\d{2}-\d{2}$/.test(s) && new Date(`${s}T00:00:00Z`).toISOString().slice(0, 10) === s;
  const end = valid(to) ? (to as string) : utcDay(now);
  let start = valid(from) ? (from as string) : utcDay(new Date(Date.parse(`${end}T00:00:00Z`) - 29 * DAY_MS));
  if (start > end) start = end;
  const days = Math.round((Date.parse(`${end}T00:00:00Z`) - Date.parse(`${start}T00:00:00Z`)) / DAY_MS) + 1;
  if (days > MAX_RANGE_DAYS) start = utcDay(new Date(Date.parse(`${end}T00:00:00Z`) - (MAX_RANGE_DAYS - 1) * DAY_MS));
  return { from: start, to: end, days: Math.min(days, MAX_RANGE_DAYS) };
}

async function tableExists(name: string): Promise<boolean> {
  const [row] = await query<{ ok: boolean }>('SELECT to_regclass($1) IS NOT NULL AS ok', [name]);
  return !!row?.ok;
}

// ── Overview ──


export async function getAdminOverview(now: Date = new Date()): Promise<AdminOverview> {
  const [u] = await query<{ total: number; admins: number; disabled: number; registered: number }>(
    `SELECT count(*)::int AS total,
            count(*) FILTER (WHERE platform_role = 'admin')::int AS admins,
            count(*) FILTER (WHERE status = 'disabled')::int AS disabled,
            count(*) FILTER (WHERE created_at >= now() - interval '30 days')::int AS registered
       FROM users`,
  );
  const [a] = await query<{ today: number; week: number; month: number }>(
    `SELECT count(DISTINCT user_id) FILTER (WHERE day = (now() AT TIME ZONE 'UTC')::date AND active_seconds > 0)::int AS today,
            count(DISTINCT user_id) FILTER (WHERE day > (now() AT TIME ZONE 'UTC')::date - 7 AND active_seconds > 0)::int AS week,
            count(DISTINCT user_id) FILTER (WHERE day > (now() AT TIME ZONE 'UTC')::date - 30 AND active_seconds > 0)::int AS month
       FROM user_activity_daily`,
  );
  const [s] = await query<{ failed: number; alerts: number }>(
    `SELECT count(*) FILTER (WHERE action = 'auth.login.failure' AND occurred_at > now() - interval '24 hours')::int AS failed,
            count(*) FILTER (WHERE action = 'security.alert' AND occurred_at > now() - interval '24 hours')::int AS alerts
       FROM audit_events WHERE occurred_at > now() - interval '24 hours'`,
  );

  let simulations: AdminOverview['simulations'] = null;
  let paperOrders: AdminOverview['paperOrders'] = null;
  let workers: AdminOverview['workers'] = null;
  let engine: AdminOverview['engine'] = null;

  if (await tableExists('runs')) {
    const [r] = await query<{ runs: number; failed: number }>(
      `SELECT count(*)::int AS runs, count(*) FILTER (WHERE status = 'failed')::int AS failed
         FROM runs WHERE created_at > now() - interval '24 hours'`,
    );
    const [j] = await query<{ failed: number }>(
      `SELECT count(*)::int AS failed FROM jobs WHERE status = 'failed' AND finished_at > now() - interval '24 hours'`,
    );
    simulations = { runsLast24h: r?.runs ?? 0, runsFailedLast24h: r?.failed ?? 0, jobsFailedLast24h: j?.failed ?? 0 };
  }
  if (await tableExists('paper_orders')) {
    const [p] = await query<{ placed: number; rejected: number }>(
      `SELECT count(*)::int AS placed, count(*) FILTER (WHERE status = 'rejected')::int AS rejected
         FROM paper_orders WHERE created_at > now() - interval '24 hours'`,
    );
    paperOrders = { placedLast24h: p?.placed ?? 0, rejectedLast24h: p?.rejected ?? 0 };
  }
  if (await tableExists('workers')) {
    const rows = await query<{ id: string; hostname: string; version: string; kinds: string[]; concurrency: number; heartbeat_at: Date; running: number }>(
      `SELECT w.id, w.hostname, w.version, w.kinds, w.concurrency, w.heartbeat_at,
              (SELECT count(*)::int FROM jobs j WHERE j.worker_id = w.id AND j.status = 'running') AS running
         FROM workers w ORDER BY w.heartbeat_at DESC LIMIT 50`,
    );
    workers = rows.map((w) => ({
      id: w.id,
      hostname: w.hostname,
      version: w.version,
      kinds: w.kinds,
      concurrency: w.concurrency,
      online: now.getTime() - w.heartbeat_at.getTime() < 60_000,
      heartbeatAt: w.heartbeat_at.getTime(),
      runningJobs: w.running,
    }));
    const [q] = await query<{ running: number; queued: number }>(
      `SELECT count(*) FILTER (WHERE status = 'running')::int AS running, count(*) FILTER (WHERE status = 'queued')::int AS queued FROM jobs`,
    );
    engine = { runningJobs: q?.running ?? 0, queuedJobs: q?.queued ?? 0 };
  }

  return {
    generatedAt: now.getTime(),
    users: { total: u?.total ?? 0, admins: u?.admins ?? 0, disabled: u?.disabled ?? 0, registeredLast30Days: u?.registered ?? 0 },
    activity: { activeToday: a?.today ?? 0, activeLast7Days: a?.week ?? 0, activeLast30Days: a?.month ?? 0, retentionDays: ACTIVITY_RETENTION_DAYS },
    simulations,
    paperOrders,
    security: { failedLoginsLast24h: s?.failed ?? 0, alertsLast24h: s?.alerts ?? 0 },
    workers,
    engine,
  };
}

// ── Analytics by day ──



type Counts = Record<string, number>;

async function byDay(sql: string, params: unknown[]): Promise<Map<string, Record<string, number>>> {
  const rows = await query<{ day: string } & Record<string, number>>(sql, params);
  return new Map(rows.map((r) => [r.day, r]));
}

export async function getAdminAnalytics(range: DateRange): Promise<AdminAnalytics> {
  const params = [range.from, range.to];
  const activity = await byDay(
    `SELECT to_char(day, 'YYYY-MM-DD') AS day,
            (sum(active_seconds) / 60)::int AS "activeMinutes",
            count(DISTINCT user_id) FILTER (WHERE active_seconds > 0)::int AS "activeUsers"
       FROM user_activity_daily WHERE day BETWEEN $1::date AND $2::date GROUP BY day`,
    params,
  );
  const signups = await byDay(
    `SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day, count(*)::int AS signups
       FROM users WHERE (created_at AT TIME ZONE 'UTC')::date BETWEEN $1::date AND $2::date GROUP BY 1`,
    params,
  );
  const auth = await byDay(
    `SELECT to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day,
            count(*) FILTER (WHERE action = 'auth.login.success')::int AS logins,
            count(*) FILTER (WHERE action = 'auth.login.failure')::int AS "loginFailures"
       FROM audit_events
      WHERE action IN ('auth.login.success', 'auth.login.failure') AND (occurred_at AT TIME ZONE 'UTC')::date BETWEEN $1::date AND $2::date
      GROUP BY 1`,
    params,
  );
  const runs: Map<string, Counts> = (await tableExists('runs'))
    ? await byDay(
        `SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day, count(*)::int AS runs,
                count(*) FILTER (WHERE status = 'failed')::int AS "runsFailed"
           FROM runs WHERE (created_at AT TIME ZONE 'UTC')::date BETWEEN $1::date AND $2::date GROUP BY 1`,
        params,
      )
    : new Map();
  const jobs: Map<string, Counts> = (await tableExists('jobs'))
    ? await byDay(
        `SELECT to_char(finished_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day, count(*) FILTER (WHERE status = 'failed')::int AS "jobsFailed"
           FROM jobs WHERE finished_at IS NOT NULL AND (finished_at AT TIME ZONE 'UTC')::date BETWEEN $1::date AND $2::date GROUP BY 1`,
        params,
      )
    : new Map();
  const orders: Map<string, Counts> = (await tableExists('paper_orders'))
    ? await byDay(
        `SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day, count(*)::int AS "paperOrders",
                count(*) FILTER (WHERE status = 'rejected')::int AS "paperRejected"
           FROM paper_orders WHERE (created_at AT TIME ZONE 'UTC')::date BETWEEN $1::date AND $2::date GROUP BY 1`,
        params,
      )
    : new Map();

  const series: AnalyticsDay[] = [];
  for (let t = Date.parse(`${range.from}T00:00:00Z`); t <= Date.parse(`${range.to}T00:00:00Z`); t += DAY_MS) {
    const day = utcDay(new Date(t));
    const a = activity.get(day) ?? {};
    const s = signups.get(day) ?? {};
    const l = auth.get(day) ?? {};
    const r = runs.get(day) ?? {};
    const j = jobs.get(day) ?? {};
    const o = orders.get(day) ?? {};
    series.push({
      day,
      activeMinutes: a.activeMinutes ?? 0,
      activeUsers: a.activeUsers ?? 0,
      signups: s.signups ?? 0,
      logins: l.logins ?? 0,
      loginFailures: l.loginFailures ?? 0,
      runs: r.runs ?? 0,
      runsFailed: r.runsFailed ?? 0,
      paperOrders: o.paperOrders ?? 0,
      paperRejected: o.paperRejected ?? 0,
      jobsFailed: j.jobsFailed ?? 0,
    });
  }

  const sum = (k: keyof Omit<AnalyticsDay, 'day'>) => series.reduce((n, d) => n + d[k], 0);
  const totals = {
    activeMinutes: sum('activeMinutes'),
    /** Peak daily active users in the range (not a sum: the same person counts once per day). */
    activeUsers: Math.max(0, ...series.map((d) => d.activeUsers)),
    signups: sum('signups'),
    logins: sum('logins'),
    loginFailures: sum('loginFailures'),
    runs: sum('runs'),
    runsFailed: sum('runsFailed'),
    paperOrders: sum('paperOrders'),
    paperRejected: sum('paperRejected'),
    jobsFailed: sum('jobsFailed'),
  };
  return { range, totals, series };
}

// ── User directory ──

export const DIRECTORY_SORTS = ['email', 'name', 'created', 'role', 'status', 'active', 'lastSeen'] as const;
export type DirectorySort = (typeof DIRECTORY_SORTS)[number];

export type DirectoryQuery = {
  q?: string;
  role?: 'admin' | 'user' | 'all';
  status?: 'active' | 'disabled' | 'all';
  sort?: DirectorySort;
  dir?: 'asc' | 'desc';
  page?: number;
  pageSize?: number;
};


export async function listDirectory(input: DirectoryQuery): Promise<{ items: DirectoryRow[]; total: number; page: number; pageSize: number }> {
  const pageSize = Math.min(100, Math.max(5, Math.floor(input.pageSize ?? 25)));
  const page = Math.max(1, Math.floor(input.page ?? 1));
  const params: unknown[] = [];
  const where: string[] = [];
  const q = (input.q ?? '').trim().slice(0, 200);
  if (q) {
    params.push(`%${q.replace(/[\\%_]/g, (c) => `\\${c}`)}%`);
    where.push(`(u.email ILIKE $${params.length} OR u.name ILIKE $${params.length})`);
  }
  if (input.role === 'admin' || input.role === 'user') {
    params.push(input.role);
    where.push(`u.platform_role = $${params.length}`);
  }
  if (input.status === 'active' || input.status === 'disabled') {
    params.push(input.status);
    where.push(`u.status = $${params.length}`);
  }
  const sortExpr: Record<DirectorySort, string> = {
    email: 'lower(u.email)',
    name: 'lower(u.name)',
    created: 'u.created_at',
    role: 'u.platform_role',
    status: 'u.status',
    active: 'coalesce(act.seconds, 0)',
    lastSeen: 'ls.last_seen',
  };
  const sort = sortExpr[(input.sort ?? 'email') as DirectorySort] ?? sortExpr.email;
  const dir = input.dir === 'desc' ? 'DESC' : 'ASC';
  params.push(pageSize, (page - 1) * pageSize);
  const rows = await query<{
    id: string; email: string; name: string; platform_role: 'admin' | 'user'; status: 'active' | 'disabled'; created_at: Date; seconds: number | null; last_seen: Date | null; total: number;
  }>(
    `SELECT u.id, u.email, u.name, u.platform_role, u.status, u.created_at,
            act.seconds, ls.last_seen, count(*) OVER() AS total
       FROM users u
       LEFT JOIN LATERAL (SELECT sum(active_seconds)::int AS seconds FROM user_activity_daily d
                           WHERE d.user_id = u.id AND d.day > (now() AT TIME ZONE 'UTC')::date - 30) act ON true
       LEFT JOIN LATERAL (SELECT max(last_seen_at) AS last_seen FROM sessions s WHERE s.user_id = u.id) ls ON true
       ${where.length ? `WHERE ${where.join(' AND ')}` : ''}
      ORDER BY ${sort} ${dir} NULLS LAST, lower(u.email) ASC
      LIMIT $${params.length - 1} OFFSET $${params.length}`,
    params,
  );
  return {
    total: rows[0] ? Number(rows[0].total) : 0,
    page,
    pageSize,
    items: rows.map((r) => ({
      id: r.id,
      email: r.email,
      name: r.name,
      role: r.platform_role,
      status: r.status,
      createdAt: r.created_at.getTime(),
      activeMinutes30d: Math.round((r.seconds ?? 0) / 60),
      lastSeenAt: r.last_seen ? r.last_seen.getTime() : null,
    })),
  };
}
