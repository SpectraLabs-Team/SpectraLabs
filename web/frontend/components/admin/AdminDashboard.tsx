'use client';

import Link from 'next/link';
import { useEffect, useState, type ReactNode } from 'react';
import { ArrowRight, BarChart3, ChevronLeft, ChevronRight, LayoutGrid, ScrollText, Server, Users } from 'lucide-react';
import { AuditLog } from '@/components/audit/AuditLog';
import { AdminUserPanel } from './AdminUserPanel';
import { StatusPills } from '@/components/ui/StatusPills';
import { getCpuTelemetry } from '@/lib/api';
import type { CpuTelemetry } from '@/lib/types';
import { displayName, useAuth } from '@/lib/auth';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';
import type { AdminOverview, AdminAnalytics, DirectoryRow } from '@/lib/adminTypes';

// ── Data hooks ──

/** GET a JSON endpoint; re-reads every `intervalMs` (if set) and whenever `url` changes. */
function useJson<T>(url: string | null, intervalMs?: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState('');
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!url) return;
    let cancelled = false;
    const load = () =>
      fetch(url, { cache: 'no-store', credentials: 'same-origin' })
        .then(async (res) => {
          const body = await res.json().catch(() => ({}));
          if (cancelled) return;
          if (!res.ok || body.ok === false) setError(body.error || `Could not load (${res.status}).`);
          else {
            setError('');
            setData(body as T);
          }
        })
        .catch(() => !cancelled && setError('Could not reach the server.'));
    void load();
    const timer = intervalMs ? window.setInterval(load, intervalMs) : undefined;
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
    };
  }, [url, intervalMs, tick]);
  return { data, error, reload: () => setTick((t) => t + 1) };
}

function useHostTelemetry(intervalMs = 5_000) {
  const [host, setHost] = useState<CpuTelemetry | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    const tick = () =>
      getCpuTelemetry()
        .then((t) => alive && (setHost(t), setFailed(false)))
        .catch(() => alive && setFailed(true));
    tick();
    const id = window.setInterval(tick, intervalMs);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [intervalMs]);
  return { host, failed };
}

// ── Small pieces ──

const utcDay = (d: Date) => d.toISOString().slice(0, 10);
const shortDate = (ms: number) => new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
const shortDateTime = (ms: number) => new Date(ms).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });

function Kpi({ label, value, sub, tone }: { label: string; value: ReactNode; sub?: string; tone?: 'loss' | 'profit' }) {
  return (
    <div className="glass-subtle p-4">
      <p className="text-[12px] text-ink/55">{label}</p>
      <p className={cn('mt-1.5 font-mono text-2xl font-semibold tabular-nums text-ink', tone === 'loss' && 'text-loss', tone === 'profit' && 'text-profit')}>{value}</p>
      {sub && <p className="mt-1 text-[11.5px] text-ink/45">{sub}</p>}
    </div>
  );
}

function Notice({ children }: { children: ReactNode }) {
  return <p className="rounded-xl border border-ink/[0.08] bg-ink/[0.02] p-4 text-[13px] leading-6 text-ink/65">{children}</p>;
}

// ── Overview ──

function OverviewPanel() {
  const { data, error } = useJson<AdminOverview>('/api/admin/overview', 15_000);
  if (error) return <Notice>{error}</Notice>;
  if (!data) return <Notice>Loading the overview…</Notice>;
  const workersOnline = data.workers?.filter((w) => w.online).length ?? null;
  return (
    <div className="space-y-6">
      <section aria-labelledby="kpi-accounts">
        <h2 id="kpi-accounts" className="text-sm font-semibold text-ink">Accounts</h2>
        <div className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Kpi label="Registered accounts" value={data.users.total} sub={`${data.users.admins} admin · ${data.users.disabled} disabled`} />
          <Kpi label="New in the last 30 days" value={data.users.registeredLast30Days} />
          <Kpi label="Active today" value={data.activity.activeToday} sub="Distinct accounts with active time (UTC day)" />
          <Kpi label="Active in 7 / 30 days" value={`${data.activity.activeLast7Days} / ${data.activity.activeLast30Days}`} sub={`Weekly / monthly active accounts`} />
        </div>
      </section>
      <section aria-labelledby="kpi-engine">
        <h2 id="kpi-engine" className="text-sm font-semibold text-ink">Simulations and trading, last 24 hours</h2>
        <div className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {data.simulations ? (
            <>
              <Kpi label="Backtests started" value={data.simulations.runsLast24h} />
              <Kpi label="Backtests failed" value={data.simulations.runsFailedLast24h} tone={data.simulations.runsFailedLast24h > 0 ? 'loss' : undefined} />
              <Kpi label="Background jobs failed" value={data.simulations.jobsFailedLast24h} tone={data.simulations.jobsFailedLast24h > 0 ? 'loss' : undefined} />
            </>
          ) : (
            <Notice>Simulation counts need the engine tables. They are not in this database yet.</Notice>
          )}
          {data.paperOrders ? (
            <Kpi label="Paper orders" value={data.paperOrders.placedLast24h} sub={`${data.paperOrders.rejectedLast24h} rejected`} />
          ) : null}
        </div>
      </section>
      <section aria-labelledby="kpi-security">
        <h2 id="kpi-security" className="text-sm font-semibold text-ink">Security, last 24 hours</h2>
        <div className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Kpi label="Failed sign-ins" value={data.security.failedLoginsLast24h} tone={data.security.failedLoginsLast24h > 0 ? 'loss' : undefined} />
          <Kpi label="Security alerts" value={data.security.alertsLast24h} tone={data.security.alertsLast24h > 0 ? 'loss' : undefined} />
          <Kpi label="Workers online" value={workersOnline === null ? '—' : `${workersOnline} / ${data.workers?.length ?? 0}`} />
        </div>
      </section>
    </div>
  );
}

// ── Users ──

const PAGE_SIZE = 25;

function UsersPanel() {
  const { user } = useAuth();
  const [search, setSearch] = useState('');
  const [q, setQ] = useState('');
  const [role, setRole] = useState<'all' | 'admin' | 'user'>('all');
  const [status, setStatus] = useState<'all' | 'active' | 'disabled'>('all');
  const [sort, setSort] = useState('email');
  const [dir, setDir] = useState<'asc' | 'desc'>('asc');
  const [page, setPage] = useState(1);
  const [open, setOpen] = useState<{ id: string; email: string } | null>(null);

  useEffect(() => {
    const t = window.setTimeout(() => {
      setQ(search.trim());
      setPage(1);
    }, 300);
    return () => window.clearTimeout(t);
  }, [search]);

  const params = new URLSearchParams({ q, role, status, sort, dir, page: String(page), pageSize: String(PAGE_SIZE) });
  const { data, error, reload } = useJson<{ items: DirectoryRow[]; total: number; page: number; pageSize: number }>(`/api/admin/users?${params}`);
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;

  const th = (key: string, label: string) => (
    <th scope="col" className="px-3 py-2 text-left font-medium" aria-sort={sort === key ? (dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
      <button
        type="button"
        onClick={() => {
          if (sort === key) setDir(dir === 'asc' ? 'desc' : 'asc');
          else {
            setSort(key);
            setDir('asc');
          }
          setPage(1);
        }}
        className="inline-flex items-center gap-1 hover:text-ink"
      >
        {label}
        {sort === key && <span aria-hidden>{dir === 'asc' ? '▲' : '▼'}</span>}
      </button>
    </th>
  );

  return (
    <div className="space-y-4">
      <div className="grid gap-2.5 md:grid-cols-[1fr_auto_auto]">
        <label className="sr-only" htmlFor="user-search">Search accounts</label>
        <input
          id="user-search"
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search by email or name"
          className="input h-10"
        />
        <label className="sr-only" htmlFor="user-role">Role</label>
        <select id="user-role" value={role} onChange={(e) => { setRole(e.target.value as typeof role); setPage(1); }} className="input h-10">
          <option value="all">All roles</option>
          <option value="admin">Admins</option>
          <option value="user">Users</option>
        </select>
        <label className="sr-only" htmlFor="user-status">Status</label>
        <select id="user-status" value={status} onChange={(e) => { setStatus(e.target.value as typeof status); setPage(1); }} className="input h-10">
          <option value="all">All statuses</option>
          <option value="active">Active</option>
          <option value="disabled">Disabled</option>
        </select>
      </div>

      {error && <Notice>{error}</Notice>}
      {!data && !error && <Notice>Loading accounts…</Notice>}

      {data && (
        <>
          <p className="text-[12.5px] text-ink/55" aria-live="polite">
            {data.total} {data.total === 1 ? 'account' : 'accounts'}
            {q || role !== 'all' || status !== 'all' ? ' match these filters' : ''}
          </p>
          <div className="overflow-x-auto rounded-xl border border-ink/[0.08]">
            <table className="w-full min-w-[760px] text-[13px]">
              <thead className="bg-ink/[0.03] text-[11.5px] uppercase tracking-wide text-ink/55">
                <tr>
                  {th('email', 'Email')}
                  {th('name', 'Name')}
                  {th('role', 'Role')}
                  {th('status', 'Status')}
                  {th('created', 'Joined')}
                  {th('active', 'Active, 30 days')}
                  {th('lastSeen', 'Last seen')}
                </tr>
              </thead>
              <tbody>
                {data.items.length === 0 && (
                  <tr>
                    <td colSpan={7} className="px-3 py-6 text-center text-ink/55">No accounts match.</td>
                  </tr>
                )}
                {data.items.map((u) => (
                  <tr key={u.id} className={cn('border-t border-ink/[0.06]', open?.id === u.id && 'bg-accent/[0.05]')}>
                    <td className="px-3 py-2.5">
                      <button type="button" onClick={() => setOpen(open?.id === u.id ? null : { id: u.id, email: u.email })} className="text-left font-medium text-ink hover:text-accent" aria-expanded={open?.id === u.id}>
                        {u.email}
                      </button>
                    </td>
                    <td className="px-3 py-2.5 text-ink/75">{u.name || '—'}</td>
                    <td className="px-3 py-2.5 text-ink/75">{u.role === 'admin' ? 'Admin' : 'User'}</td>
                    <td className="px-3 py-2.5">
                      <span className={cn('rounded-full px-2 py-0.5 text-[11.5px]', u.status === 'active' ? 'bg-profit/10 text-profit' : 'bg-loss/10 text-loss')}>
                        {u.status === 'active' ? 'Active' : 'Disabled'}
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-ink/65">{shortDate(u.createdAt)}</td>
                    <td className="px-3 py-2.5 font-mono tabular-nums text-ink/75">{u.activeMinutes30d} min</td>
                    <td className="px-3 py-2.5 text-ink/65">{u.lastSeenAt ? shortDateTime(u.lastSeenAt) : 'Never'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <nav className="flex items-center justify-between text-[12.5px] text-ink/65" aria-label="Pagination">
            <button type="button" className="btn-ghost" disabled={page <= 1} onClick={() => setPage((p) => Math.max(1, p - 1))}>
              <ChevronLeft className="h-4 w-4" /> Previous
            </button>
            <span>Page {page} of {pages}</span>
            <button type="button" className="btn-ghost" disabled={page >= pages} onClick={() => setPage((p) => p + 1)}>
              Next <ChevronRight className="h-4 w-4" />
            </button>
          </nav>
        </>
      )}

      {open && (
        <div className="rounded-xl border border-ink/[0.08] p-4">
          <AdminUserPanel userId={open.id} self={open.email === user?.email} onChanged={reload} />
        </div>
      )}
    </div>
  );
}

// ── Analytics ──

function AnalyticsPanel() {
  const today = utcDay(new Date());
  const [from, setFrom] = useState(() => utcDay(new Date(Date.now() - 29 * 86_400_000)));
  const [to, setTo] = useState(today);
  const url = `/api/admin/analytics?from=${from}&to=${to}`;
  const { data, error } = useJson<AdminAnalytics>(url);

  const preset = (days: number) => {
    setTo(today);
    setFrom(utcDay(new Date(Date.now() - (days - 1) * 86_400_000)));
  };
  const maxMinutes = Math.max(1, ...(data?.series.map((d) => d.activeMinutes) ?? [1]));

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end gap-3">
        <div role="group" aria-label="Date range presets" className="flex gap-1.5">
          {[7, 30, 90].map((d) => (
            <button key={d} type="button" onClick={() => preset(d)} className="btn-ghost text-[12.5px]">
              Last {d} days
            </button>
          ))}
        </div>
        <label className="text-[12.5px] text-ink/65">
          From
          <input type="date" value={from} max={to} onChange={(e) => e.target.value && setFrom(e.target.value)} className="input ml-2 h-9" />
        </label>
        <label className="text-[12.5px] text-ink/65">
          To
          <input type="date" value={to} min={from} max={today} onChange={(e) => e.target.value && setTo(e.target.value)} className="input ml-2 h-9" />
        </label>
        <p className="text-[11.5px] text-ink/45">UTC days, inclusive. Up to one year.</p>
      </div>

      {error && <Notice>{error}</Notice>}
      {!data && !error && <Notice>Loading analytics…</Notice>}

      {data && (
        <>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <Kpi label="Active time" value={`${data.totals.activeMinutes.toLocaleString()} min`} sub="Summed over the range" />
            <Kpi label="Peak daily active accounts" value={data.totals.activeUsers} />
            <Kpi label="New accounts" value={data.totals.signups} />
            <Kpi label="Sign-ins" value={data.totals.logins} sub={`${data.totals.loginFailures} failed`} tone={data.totals.loginFailures > 0 ? 'loss' : undefined} />
            <Kpi label="Backtests" value={data.totals.runs} sub={`${data.totals.runsFailed} failed`} tone={data.totals.runsFailed > 0 ? 'loss' : undefined} />
            <Kpi label="Paper orders" value={data.totals.paperOrders} sub={`${data.totals.paperRejected} rejected`} />
            <Kpi label="Background jobs failed" value={data.totals.jobsFailed} tone={data.totals.jobsFailed > 0 ? 'loss' : undefined} />
          </div>

          <figure className="glass-subtle p-4">
            <figcaption className="mb-3 text-[12.5px] text-ink/60">Active minutes per day</figcaption>
            <div className="flex h-36 items-end gap-[3px]" aria-hidden>
              {data.series.map((d) => (
                <div key={d.day} className="flex-1 rounded-t bg-accent/35" style={{ height: `${(d.activeMinutes / maxMinutes) * 100}%`, minHeight: d.activeMinutes > 0 ? 2 : 0 }} title={`${d.day}: ${d.activeMinutes} min`} />
              ))}
            </div>
            <p className="mt-2 flex justify-between font-mono text-[10.5px] text-ink/40">
              <span>{data.range.from}</span>
              <span>{data.range.to}</span>
            </p>
          </figure>

          <div className="max-h-[480px] overflow-auto rounded-xl border border-ink/[0.08]">
            <table className="w-full min-w-[860px] text-[12.5px]">
              <caption className="sr-only">Daily figures for the selected range</caption>
              <thead className="sticky top-0 bg-panel/95 text-[11px] uppercase tracking-wide text-ink/55 backdrop-blur">
                <tr>
                  {['Day', 'Active min', 'Active accounts', 'New', 'Sign-ins', 'Failed sign-ins', 'Backtests', 'Failed', 'Paper orders', 'Rejected', 'Jobs failed'].map((h) => (
                    <th key={h} scope="col" className="px-2.5 py-2 text-left font-medium">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {[...data.series].reverse().map((d) => (
                  <tr key={d.day} className="border-t border-ink/[0.06] font-mono tabular-nums">
                    <td className="px-2.5 py-1.5 text-ink/80">{d.day}</td>
                    <td className="px-2.5 py-1.5">{d.activeMinutes}</td>
                    <td className="px-2.5 py-1.5">{d.activeUsers}</td>
                    <td className="px-2.5 py-1.5">{d.signups}</td>
                    <td className="px-2.5 py-1.5">{d.logins}</td>
                    <td className={cn('px-2.5 py-1.5', d.loginFailures > 0 && 'text-loss')}>{d.loginFailures}</td>
                    <td className="px-2.5 py-1.5">{d.runs}</td>
                    <td className={cn('px-2.5 py-1.5', d.runsFailed > 0 && 'text-loss')}>{d.runsFailed}</td>
                    <td className="px-2.5 py-1.5">{d.paperOrders}</td>
                    <td className={cn('px-2.5 py-1.5', d.paperRejected > 0 && 'text-loss')}>{d.paperRejected}</td>
                    <td className={cn('px-2.5 py-1.5', d.jobsFailed > 0 && 'text-loss')}>{d.jobsFailed}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="text-[11.5px] leading-5 text-ink/45">
            Active time comes from browser heartbeats: about one minute per heartbeat while the page is visible and used in the last five minutes. Daily totals are kept for 400 days.
          </p>
        </>
      )}
    </div>
  );
}

// ── Engine ──

function EnginePanel() {
  const health = useEngineHealth();
  const { host, failed } = useHostTelemetry();
  const { data, error } = useJson<AdminOverview>('/api/admin/overview', 15_000);
  const online = health.status === 'online';
  const pct = (n: number) => `${n.toFixed(n < 10 ? 1 : 0)}%`;

  return (
    <div className="space-y-6">
      <div className="grid gap-3 md:grid-cols-3">
        <Kpi label="Rust engine" value={online ? 'Online' : health.status === 'checking' ? 'Checking…' : 'Offline'} tone={online ? 'profit' : 'loss'} />
        <Kpi label="CPU" value={host ? pct(host.cpu_utilization) : '—'} sub={host ? `${host.cpu_cores} cores · ${host.cpu_threads} threads` : failed ? 'Unavailable on this host' : undefined} />
        <Kpi label="Memory" value={host ? `${host.memory_used_gb.toFixed(1)} / ${host.memory_total_gb.toFixed(0)} GB` : '—'} />
      </div>

      <section aria-labelledby="workers-h">
        <h2 id="workers-h" className="text-sm font-semibold text-ink">Simulation workers</h2>
        <div className="mt-3">
          {error && <Notice>{error}</Notice>}
          {!error && !data && <Notice>Loading workers…</Notice>}
          {data && data.workers === null && <Notice>Worker status needs the engine tables. Run the database migrations (npm run db:migrate) and start a worker.</Notice>}
          {data && data.workers && data.workers.length === 0 && <Notice>No workers have registered yet. Start one with the engine worker binary.</Notice>}
          {data && data.workers && data.workers.length > 0 && (
            <div className="overflow-x-auto rounded-xl border border-ink/[0.08]">
              <table className="w-full min-w-[640px] text-[13px]">
                <thead className="bg-ink/[0.03] text-[11.5px] uppercase tracking-wide text-ink/55">
                  <tr>
                    {['Worker', 'Host', 'Version', 'Job kinds', 'Concurrency', 'Running', 'Status', 'Last heartbeat'].map((h) => (
                      <th key={h} scope="col" className="px-3 py-2 text-left font-medium">{h}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {data.workers.map((w) => (
                    <tr key={w.id} className="border-t border-ink/[0.06]">
                      <td className="px-3 py-2.5 font-mono text-accent">{w.id}</td>
                      <td className="px-3 py-2.5 text-ink/75">{w.hostname || '—'}</td>
                      <td className="px-3 py-2.5 font-mono text-ink/65">{w.version || '—'}</td>
                      <td className="px-3 py-2.5 text-ink/65">{w.kinds.join(', ') || '—'}</td>
                      <td className="px-3 py-2.5 tabular-nums">{w.concurrency}</td>
                      <td className="px-3 py-2.5 tabular-nums">{w.runningJobs}</td>
                      <td className="px-3 py-2.5">
                        <span className={cn('rounded-full px-2 py-0.5 text-[11.5px]', w.online ? 'bg-profit/10 text-profit' : 'bg-ink/[0.06] text-ink/60')}>
                          {w.online ? 'Online' : 'Offline'}
                        </span>
                      </td>
                      <td className="px-3 py-2.5 text-ink/65">{shortDateTime(w.heartbeatAt)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {data && data.engine && (
            <p className="mt-3 text-[12.5px] text-ink/60">
              Queue: {data.engine.runningJobs} running, {data.engine.queuedJobs} waiting.
            </p>
          )}
        </div>
      </section>
    </div>
  );
}

// ── Shell ──

const SECTIONS = [
  { id: 'overview', label: 'Overview', icon: LayoutGrid },
  { id: 'users', label: 'Users', icon: Users },
  { id: 'analytics', label: 'Analytics', icon: BarChart3 },
  { id: 'engine', label: 'Engine & workers', icon: Server },
  { id: 'audit', label: 'Audit log', icon: ScrollText },
] as const;
type SectionId = (typeof SECTIONS)[number]['id'];

export function AdminDashboard() {
  const { user } = useAuth();
  const [section, setSection] = useState<SectionId>('overview');

  return (
    <>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Admin console</p>
          <h1 className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.15rem]">Platform operations</h1>
        </div>
        <div className="flex flex-wrap items-center gap-2.5">
          <Link
            href="/studio"
            className="inline-flex items-center gap-1.5 rounded-xl bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#0284C7] px-4 py-2 text-sm font-bold text-[#03131A] shadow-glow transition hover:brightness-110 active:scale-[0.98]"
          >
            Open Quant Studio <ArrowRight className="h-4 w-4" />
          </Link>
          <StatusPills />
        </div>
      </div>
      <p className="mt-3 text-sm text-ink/60">
        Signed in as <span className="font-medium text-ink/85">{user ? displayName(user) : ''}</span>. Every figure is read from the database. Views of the directory, analytics and individual accounts are recorded in the audit log.
      </p>

      <div role="tablist" aria-label="Admin sections" className="mt-7 flex gap-1 overflow-x-auto border-b border-ink/[0.08]">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            role="tab"
            id={`admin-tab-${s.id}`}
            aria-selected={section === s.id}
            aria-controls="admin-panel"
            onClick={() => setSection(s.id)}
            className={cn(
              '-mb-px inline-flex shrink-0 items-center gap-2 border-b-2 px-3 py-2.5 text-[13px] transition',
              section === s.id ? 'border-accent text-ink' : 'border-transparent text-ink/55 hover:text-ink',
            )}
          >
            <s.icon className="h-4 w-4" /> {s.label}
          </button>
        ))}
      </div>

      <div id="admin-panel" role="tabpanel" aria-labelledby={`admin-tab-${section}`} className="mt-6 min-w-0">
        {section === 'overview' && <OverviewPanel />}
        {section === 'users' && <UsersPanel />}
        {section === 'analytics' && <AnalyticsPanel />}
        {section === 'engine' && <EnginePanel />}
        {section === 'audit' && (
          <section aria-labelledby="audit-h" className="glass p-5">
            <h2 id="audit-h" className="mb-4 text-sm font-semibold text-ink">Audit log</h2>
            <AuditLog endpoint="/api/admin/audit" admin />
          </section>
        )}
      </div>
    </>
  );
}
