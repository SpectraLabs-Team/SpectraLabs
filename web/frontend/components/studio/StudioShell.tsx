'use client';

import Link from 'next/link';
import { useEffect, useState, type ReactNode } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Briefcase, ChevronsLeft, ChevronsRight, Cpu, LayoutDashboard, LineChart, Lock, LogOut, Menu, PlugZap, ShieldAlert, X } from 'lucide-react';
import { RolePill } from '@/components/landing/Header';
import { StatusPills } from '@/components/ui/StatusPills';
import { Logo } from '@/components/ui/primitives';
import { ThemeToggle } from '@/components/ui/ThemeToggle';
import { displayName, initials, useAuth } from '@/lib/auth';
import { cn, formatCompactCurrency, formatCurrency } from '@/lib/utils';
import { usePortfolio } from './PortfolioContext';
import { WorkspaceSwitcher } from './WorkspaceSwitcher';

export type StudioTab = 'risk' | 'portfolio' | 'backtests' | 'benchmark' | 'settings';

export const TABS: { id: StudioTab; label: string; short: string; icon: typeof Cpu }[] = [
  { id: 'risk', label: 'Risk & Stress Lab', short: 'Risk', icon: ShieldAlert },
  { id: 'portfolio', label: 'Portfolio Studio', short: 'Portfolio', icon: Briefcase },
  { id: 'backtests', label: 'Backtests', short: 'Backtests', icon: LineChart },
  { id: 'benchmark', label: 'Hardware Benchmarks', short: 'Benchmarks', icon: Cpu },
  { id: 'settings', label: 'Market Data & Alpaca Settings', short: 'Settings', icon: PlugZap },
];

const COLLAPSE_KEY = 'spectra-sidebar-collapsed';

function Sidebar({ tab, onTab, collapsed, onCollapse, onNavigate }: { tab: StudioTab; onTab: (t: StudioTab) => void; collapsed: boolean; onCollapse?: () => void; onNavigate?: () => void }) {
  const { account, paper, engine } = usePortfolio();
  const { user } = useAuth();
  const levPct = Math.min(1, account.grossLeverage / Math.max(paper.leverageLimit, 1e-9));
  const online = engine.status === 'online';

  return (
    <div className="flex h-full flex-col">
      <div className={cn('flex h-14 items-center border-b border-ink/[0.06] px-3', collapsed ? 'justify-center' : 'justify-between')}>
        <Link href="/" aria-label="Spectra Labs home" className="flex items-center">
          {collapsed ? <Logo className="[&>span:last-child]:hidden" /> : <Logo />}
        </Link>
        {onCollapse && !collapsed && (
          <button type="button" onClick={onCollapse} className="rounded-md p-1.5 text-ink/40 transition hover:bg-ink/5 hover:text-ink" aria-label="Collapse sidebar">
            <ChevronsLeft className="h-4 w-4" />
          </button>
        )}
      </div>

      <div className="flex-1 space-y-5 overflow-y-auto p-3">
        {/* Profile → Account page */}
        <Link
          href="/account"
          onClick={onNavigate}
          className={cn('flex items-center gap-3 rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-2.5 transition hover:border-accent/30 hover:bg-accent/[0.05]', collapsed && 'justify-center p-2')}
          title={user ? `${displayName(user)} · ${user.email} · Account settings` : 'Account settings'}
        >
          <span aria-hidden className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-gradient font-mono text-xs font-bold text-onaccent shadow-glow">
            {user ? initials(user) : 'SL'}
          </span>
          {!collapsed && (
            <div className="min-w-0">
              <p className="truncate text-sm font-medium text-ink">{user ? displayName(user) : 'Research Workspace'}</p>
              <span className="mt-0.5 inline-flex items-center rounded-full border border-accent/30 bg-accent/10 px-1.5 py-px text-[10px] font-medium text-accent">
                {user?.role === 'admin' ? 'Admin' : 'Quant'} · v0.7.0
              </span>
            </div>
          )}
        </Link>

        {/* Trading mode */}
        {!collapsed ? (
          <div>
            <p className="mb-2 px-1 font-mono text-[10px] uppercase tracking-[0.16em] text-ink/40">Trading mode</p>
            <div className="space-y-1.5" role="radiogroup" aria-label="Trading mode">
              <button type="button" role="radio" aria-checked className="flex w-full items-center gap-2.5 rounded-lg border border-profit/40 bg-profit/10 px-3 py-2 text-left">
                <span className="h-2 w-2 rounded-full bg-profit shadow-[0_0_8px_rgb(var(--profit))]" />
                <span className="min-w-0">
                  <span className="block text-[13px] font-medium text-ink">Paper Trading</span>
                  <span className="block text-[10.5px] text-ink/50">Simulated ledger in this browser</span>
                </span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={false}
                aria-disabled
                disabled
                title="Not connected: the Rust API answers 501 on /api/alpaca/* (the brokerage proxy needs an HTTPS client)."
                className="flex w-full cursor-not-allowed items-center gap-2.5 rounded-lg border border-ink/[0.07] px-3 py-2 text-left opacity-60"
              >
                <span className="h-2 w-2 rounded-full bg-loss/60" />
                <span className="min-w-0 flex-1">
                  <span className="block text-[13px] font-medium text-ink/70">Live Trading</span>
                  <span className="block text-[10.5px] text-ink/45">Alpaca · not connected</span>
                </span>
                <Lock className="h-3.5 w-3.5 text-ink/40" />
              </button>
            </div>
          </div>
        ) : (
          <div className="flex justify-center" title="Paper trading (simulated)">
            <span className="h-2.5 w-2.5 rounded-full bg-profit shadow-[0_0_8px_rgb(var(--profit))]" />
          </div>
        )}

        {/* Account health */}
        {!collapsed && (
          <div className="rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-3">
            <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink/40">Paper account</p>
            <dl className="mt-2.5 space-y-2 text-[12.5px]">
              <div className="flex justify-between gap-2">
                <dt className="text-ink/55">Cash balance</dt>
                <dd className={cn('num', account.cash < 0 ? 'text-loss' : 'text-ink')}>{formatCurrency(account.cash)}</dd>
              </div>
              <div className="flex justify-between gap-2">
                <dt className="text-ink/55">Buying power</dt>
                <dd className="num text-ink">{formatCurrency(account.buyingPower)}</dd>
              </div>
              <div>
                <div className="flex justify-between gap-2">
                  <dt className="text-ink/55">Gross leverage</dt>
                  <dd className={cn('num', account.withinLimit ? 'text-profit' : 'text-loss')}>
                    {account.grossLeverage.toFixed(2)}× <span className="text-ink/40">/ {paper.leverageLimit.toFixed(1)}×</span>
                  </dd>
                </div>
                <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-ink/[0.07]">
                  <motion.div className={cn('h-full rounded-full', account.withinLimit ? 'bg-profit' : 'bg-loss')} animate={{ width: `${levPct * 100}%` }} />
                </div>
              </div>
            </dl>
            <p className="mt-2.5 text-[10.5px] leading-4 text-ink/40">Equity {formatCompactCurrency(account.equity)} · marked at CSV prices</p>
          </div>
        )}

        {/* Navigation */}
        <nav aria-label="Studio" className="space-y-0.5">
          {!collapsed && <p className="mb-2 px-1 font-mono text-[10px] uppercase tracking-[0.16em] text-ink/40">Workspaces</p>}
          {TABS.map((t) => {
            const active = t.id === tab;
            return (
              <button
                key={t.id}
                type="button"
                onClick={() => {
                  onTab(t.id);
                  onNavigate?.();
                }}
                aria-current={active ? 'page' : undefined}
                title={collapsed ? t.label : undefined}
                className={cn(
                  'relative flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-[13px] transition-colors',
                  collapsed && 'justify-center px-0',
                  active ? 'bg-accent/10 text-ink' : 'text-ink/60 hover:bg-ink/[0.04] hover:text-ink',
                )}
              >
                {active && <motion.span layoutId="studio-nav" className="absolute inset-y-1.5 left-0 w-0.5 rounded-full bg-accent shadow-glow" />}
                <t.icon className={cn('h-4 w-4 shrink-0', active && 'text-accent')} />
                {!collapsed && <span className="leading-tight">{t.label}</span>}
              </button>
            );
          })}
        </nav>
      </div>

      <div className={cn('flex items-center gap-2 border-t border-ink/[0.06] p-3', collapsed ? 'flex-col' : 'justify-between')}>
        <span className="flex items-center gap-2 text-[11px] text-ink/50" title={online ? 'Engine online' : 'Engine offline'}>
          <span className={cn('h-2 w-2 rounded-full', online ? 'bg-profit' : engine.status === 'offline' ? 'bg-loss' : 'bg-ink/30')} />
          {!collapsed && (online ? 'Rust engine online' : engine.status === 'offline' ? 'Engine offline' : 'Connecting…')}
        </span>
        <div className={cn('flex items-center gap-1.5', collapsed && 'flex-col')}>
          <ThemeToggle className="h-8 w-8" />
          {onCollapse && collapsed && (
            <button type="button" onClick={onCollapse} className="flex h-8 w-8 items-center justify-center rounded-lg border border-ink/10 text-ink/50 transition hover:text-ink" aria-label="Expand sidebar">
              <ChevronsRight className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export function StudioShell({ tab, onTab, children }: { tab: StudioTab; onTab: (t: StudioTab) => void; children: ReactNode }) {
  const { stats, positions, source, engine } = usePortfolio();
  const { user, logout } = useAuth();
  const signOut = () => logout('/');
  const [collapsed, setCollapsed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const current = TABS.find((t) => t.id === tab)!;

  useEffect(() => {
    try {
      setCollapsed(localStorage.getItem(COLLAPSE_KEY) === '1');
    } catch {
      /* default expanded */
    }
  }, []);
  const toggleCollapse = () =>
    setCollapsed((c) => {
      try {
        localStorage.setItem(COLLAPSE_KEY, c ? '0' : '1');
      } catch {
        /* not persisted */
      }
      return !c;
    });

  return (
    <div className="relative min-h-screen">
      <div className="bg-dot-grid pointer-events-none fixed inset-0 opacity-60" />

      {/* Desktop sidebar */}
      <aside className={cn('fixed inset-y-0 left-0 z-30 hidden border-r border-ink/[0.07] bg-panel/80 backdrop-blur-xl transition-[width] duration-300 lg:block', collapsed ? 'w-[68px]' : 'w-[264px]')}>
        <Sidebar tab={tab} onTab={onTab} collapsed={collapsed} onCollapse={toggleCollapse} />
      </aside>

      {/* Mobile sidebar */}
      <AnimatePresence>
        {mobileOpen && (
          <>
            <motion.div className="fixed inset-0 z-40 bg-black/50 lg:hidden" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={() => setMobileOpen(false)} />
            <motion.aside className="fixed inset-y-0 left-0 z-50 w-[280px] border-r border-ink/10 bg-panel lg:hidden" initial={{ x: '-100%' }} animate={{ x: 0 }} exit={{ x: '-100%' }} transition={{ type: 'spring', stiffness: 380, damping: 40 }}>
              <button type="button" onClick={() => setMobileOpen(false)} className="absolute right-3 top-3.5 rounded-md p-1 text-ink/50" aria-label="Close menu">
                <X className="h-5 w-5" />
              </button>
              <Sidebar tab={tab} onTab={onTab} collapsed={false} onNavigate={() => setMobileOpen(false)} />
            </motion.aside>
          </>
        )}
      </AnimatePresence>

      <div className={cn('relative transition-[padding] duration-300', collapsed ? 'lg:pl-[68px]' : 'lg:pl-[264px]')}>
        <header className="sticky top-0 z-20 flex h-14 items-center gap-3 border-b border-ink/[0.06] bg-canvas/80 px-4 backdrop-blur-xl sm:px-6">
          <button type="button" onClick={() => setMobileOpen(true)} className="-ml-1 rounded-md p-1.5 text-ink/60 lg:hidden" aria-label="Open menu">
            <Menu className="h-5 w-5" />
          </button>
          <WorkspaceSwitcher className="shrink-0" />
          <span aria-hidden className="hidden h-5 w-px bg-ink/10 sm:block" />
          <div className="flex min-w-0 items-center gap-2.5">
            <current.icon className="hidden h-4 w-4 shrink-0 text-accent sm:block" />
            <h1 className="hidden truncate text-sm font-semibold text-ink sm:block">{current.label}</h1>
          </div>
          <div className="ml-auto flex items-center gap-3">
            <span className="hidden items-center gap-2 rounded-full border border-ink/10 bg-ink/[0.03] px-3 py-1 text-xs text-ink/60 2xl:inline-flex">
              <Briefcase className="h-3.5 w-3.5 text-ink/40" />
              <span className="num text-ink/90">{formatCompactCurrency(stats.netValue)}</span>· {positions.length} positions · {source}
            </span>
            <StatusPills className="hidden xl:flex" />
            {user && <RolePill user={user} className="hidden sm:flex" />}
            {user?.role === 'admin' && (
              <Link
                href="/admin"
                className="hidden items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[12.5px] font-medium text-ink/65 transition hover:bg-ink/[0.05] hover:text-ink md:inline-flex"
              >
                <LayoutDashboard className="h-4 w-4" /> Admin Dashboard
              </Link>
            )}
            <button
              type="button"
              onClick={signOut}
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/15 bg-ink/[0.03] px-3 py-1.5 text-[12.5px] font-semibold text-ink/80 transition hover:border-loss/40 hover:bg-loss/10 hover:text-loss"
            >
              <LogOut className="h-4 w-4" /> <span className="hidden sm:inline">Sign Out</span>
            </button>
          </div>
        </header>

        {engine.status === 'offline' && (
          <div className="px-4 pt-4 sm:px-6">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-xl border border-caution/25 bg-caution/[0.06] px-4 py-2.5 text-xs text-amber-800 dark:text-amber-200/90">
              <span className="font-medium">Engine offline.</span>
              <span className="text-amber-700 dark:text-amber-200/70">Closed-form previews only. Start the API from the repo root:</span>
              <code className="kbd text-amber-900 dark:text-amber-100">cargo run --release -p spectra-api</code>
            </div>
          </div>
        )}

        <main className="mx-auto max-w-[1400px] px-4 py-6 sm:px-6 sm:py-8">{children}</main>
      </div>
    </div>
  );
}
