'use client';

import Link from 'next/link';
import { Fragment, useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { Activity, ArrowRight, Check, Copy, Cpu, RefreshCw, ScrollText, Search, Server, Users, Workflow, X } from 'lucide-react';
import { AuditLog } from '@/components/audit/AuditLog';
import { AdminUserPanel } from './AdminUserPanel';
import { StatusPills } from '@/components/ui/StatusPills';
import { getCpuTelemetry } from '@/lib/api';
import type { CpuTelemetry } from '@/lib/types';
import { displayName, initials, useAuth, type Role } from '@/lib/auth';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';

// Skeleton data: placeholders until admin telemetry endpoints exist.
const WORKERS = [
  { id: 'mc-01', job: 'Monte Carlo · 100k paths', pct: 72 },
  { id: 'mc-02', job: 'Stress · 2008 replay', pct: 41 },
  { id: 'bt-01', job: 'Backtest sweep · 48 trials', pct: 88 },
  { id: 'rs-01', job: 'PBO / DSR validation', pct: 15 },
];

function Card({ title, icon, className, children, i, demo }: { title: string; icon: ReactNode; className?: string; children: ReactNode; i: number; demo?: boolean }) {
  const reduce = useReducedMotion();
  return (
    <motion.section
      initial={reduce ? false : { opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: i * 0.06, duration: 0.4 }}
      className={cn('glass flex flex-col p-5 sm:p-6', className)}
    >
      <div className="flex items-center gap-2 text-sm font-semibold text-ink">
        <span className="text-accent">{icon}</span>
        {title}
        {demo && (
          <span title="Placeholder figures until this metric has an engine endpoint" className="ml-auto rounded-full border border-amber-500/35 bg-amber-500/10 px-2 py-0.5 font-mono text-[9.5px] font-semibold uppercase tracking-wider text-amber-700 dark:text-amber-300">
            Demo data
          </span>
        )}
      </div>
      <div className="mt-4 flex-1">{children}</div>
    </motion.section>
  );
}

function Bar({ pct, tone = 'accent' }: { pct: number; tone?: 'accent' | 'profit' | 'caution' }) {
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-ink/[0.08]">
      <div className={cn('h-full rounded-full', tone === 'profit' ? 'bg-profit' : tone === 'caution' ? 'bg-caution' : 'bg-accent')} style={{ width: `${pct}%` }} />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="font-mono text-[10.5px] uppercase tracking-[0.14em] text-ink/45">{label}</p>
      <p className="mt-1 font-mono text-xl font-semibold tabular-nums text-ink">{value}</p>
    </div>
  );
}

/** Live host load from the engine (GET /api/telemetry/cpu via the signed-in proxy), every 5s. */
function useHostTelemetry(intervalMs = 5_000) {
  const [host, setHost] = useState<CpuTelemetry | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    const tick = () =>
      getCpuTelemetry()
        .then((t) => live && (setHost(t), setFailed(false)))
        .catch(() => live && setFailed(true));
    tick();
    const id = window.setInterval(tick, intervalMs);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, [intervalMs]);
  return { host, failed };
}

const pct = (n: number) => `${n.toFixed(n < 10 ? 1 : 0)}%`;

type Account = { id: string; email: string; name: string; role: Role; status: 'active' | 'disabled'; createdAt: number; twoFactor?: boolean; lastActiveAt: number | null };

const joined = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });

/** Live list from /api/admin/users (admin-checked on the server). */
/** Copies an email to the clipboard and confirms with a tick for a moment. */
function CopyEmail({ email }: { email: string }) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(email);
      setDone(true);
      setTimeout(() => setDone(false), 1500);
    } catch {
      /* clipboard blocked (insecure context): the email is still selectable */
    }
  };
  return (
    <button
      type="button"
      onClick={copy}
      aria-label={done ? `Copied ${email}` : `Copy ${email}`}
      title={done ? 'Copied' : 'Copy email'}
      className={cn('rounded-md p-1.5 transition', done ? 'text-profit' : 'text-ink/40 hover:bg-ink/[0.06] hover:text-ink')}
    >
      {done ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

function RegisteredUsers() {
  const { user: me } = useAuth();
  const [open, setOpen] = useState<string | null>(null);
  const [users, setUsers] = useState<Account[] | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!users || !q) return users ?? [];
    return users.filter((u) => u.email.includes(q) || displayName(u).toLowerCase().includes(q));
  }, [users, query]);

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const res = await fetch('/api/admin/users', { cache: 'no-store', credentials: 'same-origin' });
      const data = await res.json();
      if (!res.ok || !data.ok) throw new Error(data.error || 'Could not load accounts.');
      setUsers(data.users);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load accounts.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <>
      <div className="-mt-1 mb-3 flex flex-wrap items-center gap-3">
        <label className="relative min-w-[200px] flex-1 sm:max-w-xs">
          <span className="sr-only">Search accounts</span>
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-ink/40" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search name or email"
            className="w-full rounded-lg border border-ink/10 bg-canvas/60 py-1.5 pl-8 pr-7 text-[12.5px] text-ink outline-none transition placeholder:text-ink/35 focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20 [&::-webkit-search-cancel-button]:hidden"
          />
          {query && (
            <button type="button" onClick={() => setQuery('')} aria-label="Clear search" className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-ink/40 hover:text-ink">
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </label>
        <p className="text-[12px] text-ink/50" aria-live="polite">
          {users
            ? query.trim()
              ? `Showing ${shown.length} of ${users.length} registered quant${users.length === 1 ? '' : 's'}`
              : `${users.length} registered quant${users.length === 1 ? '' : 's'}`
            : loading
              ? 'Loading…'
              : ''}
        </p>
        <button type="button" onClick={load} disabled={loading} className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-ink/55 transition hover:bg-ink/5 hover:text-ink disabled:opacity-50">
          <RefreshCw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} /> Refresh
        </button>
      </div>
      {error ? (
        <p className="rounded-lg border border-rose-500/25 bg-rose-500/[0.06] px-3 py-2 text-[12.5px] text-rose-600 dark:text-rose-300">{error}</p>
      ) : users && users.length === 0 ? (
        <p className="py-6 text-center text-sm text-ink/50">No accounts registered yet.</p>
      ) : users && shown.length === 0 ? (
        <p className="py-6 text-center text-sm text-ink/50">No accounts match &ldquo;{query.trim()}&rdquo;.</p>
      ) : (
        <div className="-mx-1 overflow-x-auto">
          <table className="w-full min-w-[640px] text-left text-[12.5px]">
            <thead>
              <tr className="border-b border-ink/[0.08] font-mono text-[10.5px] uppercase tracking-[0.12em] text-ink/45">
                <th className="px-1 pb-2 font-medium">Name</th>
                <th className="px-1 pb-2 font-medium">Email</th>
                <th className="px-1 pb-2 font-medium">Role</th>
                <th className="px-1 pb-2 font-medium">Login</th>
                <th className="px-1 pb-2 font-medium">Status</th>
                <th className="px-1 pb-2 text-right font-medium">Joined</th>
                <th className="px-1 pb-2" aria-label="Actions" />
              </tr>
            </thead>
            <tbody className="divide-y divide-ink/[0.06]">
              {shown.map((u) => (
                <Fragment key={u.id}>
                <tr>
                  <td className="px-1 py-2.5">
                    <span className="flex items-center gap-2.5">
                      <span aria-hidden className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-accent-gradient font-mono text-[10px] font-bold text-onaccent">{initials(u)}</span>
                      <span className="truncate font-medium text-ink/90">{displayName(u)}</span>
                    </span>
                  </td>
                  <td className="px-1 py-1.5 text-ink/65">
                    <span className="flex items-center gap-1">
                      <span className="truncate">{u.email}</span>
                      <CopyEmail email={u.email} />
                    </span>
                  </td>
                  <td className="px-1 py-2.5">
                    <span className={cn('rounded-full px-2 py-0.5 text-[10.5px] font-semibold', u.role === 'admin' ? 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300' : 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300')}>
                      {u.role === 'admin' ? 'Admin' : 'User'}
                    </span>
                  </td>
                  <td className="px-1 py-2.5 font-mono text-[11px]">
                    {u.twoFactor ? <span className="text-profit">2FA</span> : <span className="text-caution">Password only</span>}
                  </td>
                  <td className="px-1 py-2.5">
                    <span className={cn('rounded-full px-2 py-0.5 text-[10.5px] font-semibold', u.status === 'active' ? 'bg-ink/[0.05] text-ink/60' : 'bg-rose-500/15 text-rose-700 dark:text-rose-300')}>
                      {u.status === 'active' ? 'Active' : 'Disabled'}
                    </span>
                  </td>
                  <td className="px-1 py-2.5 text-right font-mono tabular-nums text-ink/55" title={u.lastActiveAt ? `Last active ${new Date(u.lastActiveAt).toLocaleString()}` : 'No activity recorded'}>
                    {joined.format(new Date(u.createdAt))}
                  </td>
                  <td className="px-1 py-2.5 text-right">
                    <button type="button" className="btn-ghost border border-ink/10" aria-expanded={open === u.id} onClick={() => setOpen((o) => (o === u.id ? null : u.id))}>
                      {open === u.id ? 'Close' : 'Manage'}
                    </button>
                  </td>
                </tr>
                {open === u.id && (
                  <tr>
                    <td colSpan={7} className="px-1 pb-3">
                      <AdminUserPanel userId={u.id} self={u.email === me?.email} onChanged={load} />
                    </td>
                  </tr>
                )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

export function AdminDashboard() {
  const { user } = useAuth();
  const health = useEngineHealth();
  const online = health.status === 'online';
  const { host, failed } = useHostTelemetry();

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
          <span className="flex items-center gap-3 rounded-full border border-slate-300/80 bg-slate-100/90 px-4 py-1.5 font-mono text-[12px] text-slate-700 dark:border-white/10 dark:bg-white/5 dark:text-slate-200">
            <span className="flex items-center gap-1.5"><Cpu className="h-3.5 w-3.5 text-accent" /> CPU {host ? pct(host.cpu_utilization) : '—'}</span>
            <span className="h-3 w-px bg-slate-300 dark:bg-white/15" />
            <span>MEM {host ? `${host.memory_used_gb.toFixed(1)}/${host.memory_total_gb.toFixed(0)} GB` : '—'}</span>
          </span>
        </div>
      </div>
      <p className="mt-3 text-sm text-ink/60">
        Signed in as <span className="font-medium text-ink/85">{user ? displayName(user) : ''}</span>. Engine health, CPU/memory and accounts are live; cards marked Demo data are placeholders.
      </p>

      <div className="mt-10 grid auto-rows-[minmax(180px,auto)] gap-5 md:grid-cols-2 lg:grid-cols-4">
        <Card i={0} demo title="System Telemetry" icon={<Activity className="h-4 w-4" />} className="lg:col-span-2">
          <div className="grid grid-cols-3 gap-4">
            <Stat label="Req / min" value="1,284" />
            <Stat label="p50 latency" value="0.42ms" />
            <Stat label="p99 latency" value="3.9ms" />
          </div>
          <div className="mt-5 flex h-16 items-end gap-1">
            {[38, 52, 44, 61, 57, 70, 48, 66, 74, 59, 81, 63, 72, 55, 68, 77, 62, 85, 71, 64].map((h, k) => (
              <div key={k} className="flex-1 rounded-t bg-accent/30" style={{ height: `${h}%` }} />
            ))}
          </div>
        </Card>

        <Card i={1} title="Rust Engine Health" icon={<Server className="h-4 w-4" />}>
          <div className="flex items-center gap-2.5">
            <span className={cn('h-2.5 w-2.5 rounded-full', online ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.9)]' : 'bg-rose-500 shadow-[0_0_8px_rgba(244,63,94,0.9)]')} />
            <span className="text-sm font-semibold text-ink">{online ? 'Online' : health.status === 'checking' ? 'Checking…' : 'Offline'}</span>
          </div>
          <dl className="mt-4 space-y-2 font-mono text-[12px]">
            <div className="flex justify-between"><dt className="text-ink/50">Version</dt><dd className="text-ink/85">v0.7.0</dd></div>
            <div className="flex justify-between"><dt className="text-ink/50">Kernel</dt><dd className="text-ink/85">Parallel</dd></div>
            <div className="flex justify-between"><dt className="text-ink/50">unsafe</dt><dd className="text-profit">forbid</dd></div>
          </dl>
        </Card>

        <Card i={2} title="Memory & CPU" icon={<Cpu className="h-4 w-4" />}>
          {host ? (
            <div className="space-y-4">
              <div>
                <div className="mb-1.5 flex justify-between text-[12px]"><span className="text-ink/60">CPU</span><span className="font-mono text-ink/85">{pct(host.cpu_utilization)}</span></div>
                <Bar pct={Math.min(100, host.cpu_utilization)} tone={host.cpu_utilization > 85 ? 'caution' : 'accent'} />
              </div>
              <div>
                <div className="mb-1.5 flex justify-between text-[12px]"><span className="text-ink/60">Memory</span><span className="font-mono text-ink/85">{host.memory_used_gb.toFixed(1)} / {host.memory_total_gb.toFixed(1)} GB</span></div>
                <Bar pct={Math.min(100, host.memory_utilization)} tone={host.memory_utilization > 85 ? 'caution' : 'profit'} />
              </div>
              <p className="font-mono text-[10.5px] text-ink/45">{host.cpu_cores} cores · {host.cpu_threads} threads · {host.source}</p>
            </div>
          ) : (
            <p className="text-[12.5px] leading-5 text-ink/50">{failed ? 'Telemetry unavailable: the engine is offline or does not report host load on this OS.' : 'Reading host telemetry…'}</p>
          )}
        </Card>

        <Card i={3} demo title="Active Simulation Workers" icon={<Workflow className="h-4 w-4" />} className="lg:col-span-2">
          <ul className="space-y-3">
            {WORKERS.map((w) => (
              <li key={w.id} className="grid grid-cols-[52px_1fr_44px] items-center gap-3 text-[12px]">
                <span className="font-mono text-accent">{w.id}</span>
                <div className="min-w-0">
                  <p className="truncate text-ink/80">{w.job}</p>
                  <div className="mt-1"><Bar pct={w.pct} /></div>
                </div>
                <span className="text-right font-mono tabular-nums text-ink/60">{w.pct}%</span>
              </li>
            ))}
          </ul>
        </Card>

        <Card i={4} title="Audit Log" icon={<ScrollText className="h-4 w-4" />} className="md:col-span-2 lg:col-span-4">
          <AuditLog endpoint="/api/admin/audit" admin />
        </Card>

        <Card i={5} title="Registered Quants & Fund Accounts" icon={<Users className="h-4 w-4" />} className="md:col-span-2 lg:col-span-4">
          <RegisteredUsers />
        </Card>
      </div>
    </>
  );
}
