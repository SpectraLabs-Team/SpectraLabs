'use client';

import type { ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { Activity, Cpu, Gauge, ScrollText, Server, Workflow } from 'lucide-react';
import { StatusPills } from '@/components/ui/StatusPills';
import { useAuth } from '@/lib/auth';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';

// Skeleton data: placeholders until admin telemetry endpoints exist.
const WORKERS = [
  { id: 'mc-01', job: 'Monte Carlo · 100k paths', pct: 72 },
  { id: 'mc-02', job: 'Stress · 2008 replay', pct: 41 },
  { id: 'bt-01', job: 'Backtest sweep · 48 trials', pct: 88 },
  { id: 'rs-01', job: 'PBO / DSR validation', pct: 15 },
];
const LEDGER = [
  ['21:14:07', 'registry.append', 'exp#0193 sealed', '9f3a…c21e'],
  ['21:09:52', 'auth.verify', 'admin session issued', '41be…07d9'],
  ['20:58:31', 'risk.calculate', 'VaR 99% · seed 42', 'c7d0…88a4'],
  ['20:41:16', 'vault.unlock', 'holdout read denied', '2e19…f5b3'],
  ['20:33:04', 'data.ingest', 'SPY 1d · 5,031 bars', 'a8c6…1d70'],
];

function Card({ title, icon, className, children, i }: { title: string; icon: ReactNode; className?: string; children: ReactNode; i: number }) {
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

export function AdminDashboard() {
  const { user } = useAuth();
  const health = useEngineHealth();
  const online = health.status === 'online';

  return (
    <>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Admin console</p>
          <h1 className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.15rem]">Platform operations</h1>
        </div>
        <div className="flex flex-wrap items-center gap-2.5">
          <StatusPills />
          <span className="flex items-center gap-3 rounded-full border border-slate-300/80 bg-slate-100/90 px-4 py-1.5 font-mono text-[12px] text-slate-700 dark:border-white/10 dark:bg-white/5 dark:text-slate-200">
            <span className="flex items-center gap-1.5"><Cpu className="h-3.5 w-3.5 text-accent" /> CPU 63%</span>
            <span className="h-3 w-px bg-slate-300 dark:bg-white/15" />
            <span>MEM 3.1/8 GB</span>
          </span>
        </div>
      </div>
      <p className="mt-3 text-sm text-ink/60">
        Signed in as <span className="font-medium text-ink/85">{user?.email}</span>. Figures below are placeholders until the telemetry API is wired up.
      </p>

      <div className="mt-10 grid auto-rows-[minmax(180px,auto)] gap-5 md:grid-cols-2 lg:grid-cols-4">
        <Card i={0} title="System Telemetry" icon={<Activity className="h-4 w-4" />} className="lg:col-span-2">
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
          <div className="space-y-4">
            <div>
              <div className="mb-1.5 flex justify-between text-[12px]"><span className="text-ink/60">CPU</span><span className="font-mono text-ink/85">63%</span></div>
              <Bar pct={63} />
            </div>
            <div>
              <div className="mb-1.5 flex justify-between text-[12px]"><span className="text-ink/60">Memory</span><span className="font-mono text-ink/85">3.1 / 8 GB</span></div>
              <Bar pct={39} tone="profit" />
            </div>
            <div>
              <div className="mb-1.5 flex justify-between text-[12px]"><span className="text-ink/60">Disk</span><span className="font-mono text-ink/85">81%</span></div>
              <Bar pct={81} tone="caution" />
            </div>
          </div>
        </Card>

        <Card i={3} title="Active Simulation Workers" icon={<Workflow className="h-4 w-4" />} className="lg:col-span-2">
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

        <Card i={4} title="Audit Ledger" icon={<ScrollText className="h-4 w-4" />} className="lg:col-span-2">
          <ul className="divide-y divide-ink/[0.06] font-mono text-[11.5px]">
            {LEDGER.map(([t, ev, msg, hash]) => (
              <li key={t} className="grid grid-cols-[64px_1fr] gap-x-3 py-2 sm:grid-cols-[64px_120px_1fr_88px]">
                <span className="text-ink/45">{t}</span>
                <span className="text-accent">{ev}</span>
                <span className="col-span-2 truncate text-ink/70 sm:col-span-1">{msg}</span>
                <span className="hidden text-right text-ink/40 sm:block">{hash}</span>
              </li>
            ))}
          </ul>
          <p className="mt-3 flex items-center gap-1.5 text-[11px] text-ink/45"><Gauge className="h-3 w-3" /> SHA-256 hash-chained · tamper-evident</p>
        </Card>
      </div>
    </>
  );
}
