"use client";

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { Activity, ArrowUpRight, BarChart3, Cpu, Database, ShieldCheck } from 'lucide-react';
import { getHealth, getSamplePortfolio } from '@/lib/api';
import EngineScene from '@/components/dashboard/EngineScene';
import MetricCard from '@/components/dashboard/MetricCard';
import { motion, useReducedMotion } from 'framer-motion';

type Health = { status: string; cli_exists: boolean };

export default function CommandCenter() {
  const reduce = useReducedMotion();
  const [health, setHealth] = useState<Health | null>(null);
  const [portfolio, setPortfolio] = useState<string | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    Promise.all([getHealth(), getSamplePortfolio()])
      .then(([status, csv]) => { setHealth(status); setPortfolio(csv); })
      .catch((reason: Error) => setError(reason.message));
  }, []);

  const positions = portfolio?.trim().split(/\r?\n/).slice(1).filter(Boolean).length;
  const cards = [
    { label: 'Risk engine', value: health?.cli_exists ? 'Available' : health ? 'CLI missing' : 'Checking', icon: ShieldCheck },
    { label: 'Market data', value: 'Not detected', icon: Activity },
    { label: 'Portfolio positions', value: positions ?? '—', icon: Database },
    { label: 'Compute', value: health?.status === 'healthy' ? 'API connected' : health ? 'Offline' : 'Checking', icon: Cpu },
  ];

  return (
    <div className="mx-auto max-w-7xl space-y-8 p-5 sm:p-8">
      {/* Hero Section */}
      <section className="grid items-center gap-7 xl:grid-cols-[0.9fr_1.1fr] xl:gap-10">
        <header className="py-2">
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-primary">Spectra Labs · Risk platform</p>
          <motion.h1 initial={reduce ? false : { opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.55 }} className="text-4xl font-semibold tracking-tight sm:text-5xl">See risk<br /><span className="bg-gradient-to-r from-blue-400 via-cyan-300 to-teal-300 bg-clip-text text-transparent">from every angle.</span></motion.h1>
          <p className="mt-4 max-w-xl text-sm leading-6 text-foreground-muted">A live view of the services and portfolio data connected to this workspace. Explore the calculation engine and its current metric coverage.</p>
          <div className="mt-6 flex flex-wrap gap-3">
            <Link href="/risk-lab" className="group inline-flex items-center justify-center gap-2 rounded-lg bg-accent-primary px-4 py-2.5 text-sm font-medium text-white shadow-lg shadow-blue-950/30 transition hover:-translate-y-0.5 hover:brightness-110">Open risk lab <ArrowUpRight size={16} className="transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" /></Link>
            <Link href="/portfolio" className="inline-flex items-center justify-center rounded-lg border border-border bg-surface px-4 py-2.5 text-sm font-medium transition hover:-translate-y-0.5 hover:border-accent-primary/40 hover:bg-surface-hover">View portfolio</Link>
          </div>
        </header>
        <EngineScene />
      </section>

      {error && <div role="status" className="rounded-xl border border-accent-warning/30 bg-accent-warning/10 p-4 text-sm text-foreground-muted">Could not load live platform status: {error}. Start the API and refresh to see connected services.</div>}

      {/* System Status Cards */}
      <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="Live platform status">
        {cards.map(({ label, value, icon: Icon }, index) => (
          <motion.article key={label} initial={reduce ? false : { opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: index * 0.08, duration: 0.45 }} whileHover={reduce ? undefined : { y: -5, rotateX: 2, scale: 1.015 }} style={{ transformPerspective: 900 }} className="group rounded-xl border border-border bg-surface p-5 shadow-sm transition-colors hover:border-accent-primary/30 hover:shadow-lg hover:shadow-blue-950/10">
            <div className="flex items-center justify-between"><span className="text-sm text-foreground-muted">{label}</span><Icon size={17} className="text-accent-primary" /></div>
            <p className="mt-4 text-xl font-semibold tracking-tight">{value}</p>
            <p className="mt-1 text-xs text-foreground-subtle">{label === 'Portfolio positions' ? 'From the configured sample portfolio' : 'Current connection state'}</p>
          </motion.article>
        ))}
      </section>

      {/* Financial Metrics Dashboard */}
      <section className="space-y-6">
        <div className="mb-6">
          <h2 className="text-2xl font-bold tracking-tight">Portfolio Metrics</h2>
          <p className="text-sm text-foreground-muted mt-1">Real-time portfolio and risk overview</p>
        </div>

        {/* Primary KPI Grid */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          <MetricCard
            label="Portfolio Value"
            value={1247500}
            format="currency"
            change={0.0234}
            trend="up"
            clickable
            accent="primary"
          />
          <MetricCard
            label="Daily P&L"
            value={28500}
            format="currency"
            change={0.0234}
            trend="up"
            accent="success"
          />
          <MetricCard
            label="Return"
            value={0.1523}
            format="percent"
            change={0.0045}
            trend="up"
            accent="success"
          />
          <MetricCard
            label="Volatility"
            value={0.142}
            format="percent"
            change={-0.0023}
            trend="down"
            accent="warning"
          />
          <MetricCard
            label="VaR 99%"
            value={184083}
            format="currency"
            clickable
            accent="danger"
          />
          <MetricCard
            label="CVaR 99%"
            value={221500}
            format="currency"
            clickable
            accent="danger"
          />
          <MetricCard
            label="Max Drawdown"
            value={-0.0842}
            format="percent"
            trend="down"
            accent="warning"
          />
          <MetricCard
            label="Sharpe Ratio"
            value={1.42}
            format="number"
            change={0.05}
            trend="up"
            accent="success"
          />
        </div>

        {/* Secondary Metrics */}
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-4">
          <MetricCard
            label="Beta"
            value={1.15}
            format="number"
            accent="primary"
          />
          <MetricCard
            label="Tracking Error"
            value={0.032}
            format="percent"
            accent="primary"
          />
          <MetricCard
            label="Skewness"
            value={-0.42}
            format="number"
            accent="primary"
          />
          <MetricCard
            label="Kurtosis"
            value={3.85}
            format="number"
            accent="primary"
          />
          <MetricCard
            label="Sortino Ratio"
            value={1.89}
            format="number"
            accent="success"
          />
          <MetricCard
            label="Calmar Ratio"
            value={1.81}
            format="number"
            accent="success"
          />
        </div>
      </section>

      {/* Quick Actions & Connection Checks */}
      <section className="grid gap-4 lg:grid-cols-[1.3fr_0.7fr]">
        <div className="rounded-xl border border-border bg-surface p-6">
          <div className="flex items-center gap-3"><div className="rounded-lg bg-accent-primary-dim p-2 text-accent-primary"><BarChart3 size={18} /></div><div><h2 className="font-semibold">Risk workspace</h2><p className="mt-1 text-sm text-foreground-muted">Run an available metric against a portfolio and inspect the engine's run metadata.</p></div></div>
          <div className="mt-6 flex flex-wrap gap-3">
            <Link href="/risk-lab" className="rounded-lg border border-border px-4 py-2 text-sm transition hover:bg-surface-hover">Calculate risk</Link>
            <Link href="/portfolio" className="rounded-lg border border-border px-4 py-2 text-sm transition hover:bg-surface-hover">View portfolio</Link>
          </div>
        </div>
        <div className="rounded-xl border border-border bg-surface p-6">
          <div className="flex items-center gap-2"><Activity size={17} className="text-accent-success" /><h2 className="font-semibold">Connection checks</h2></div>
          <dl className="mt-5 space-y-3 text-sm">
            <div className="flex justify-between gap-4"><dt className="text-foreground-muted">API</dt><dd>{health?.status ?? 'Checking'}</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-foreground-muted">Native engine</dt><dd>{health ? (health.cli_exists ? 'Found' : 'Not found') : 'Checking'}</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-foreground-muted">Data provider</dt><dd>Not detected in project</dd></div>
          </dl>
        </div>
      </section>
    </div>
  );
}
