'use client';

import Link from 'next/link';
import { motion } from 'framer-motion';
import { ArrowLeft, Briefcase, Cpu, ShieldAlert } from 'lucide-react';
import type { ReactNode } from 'react';
import { HealthPill } from '@/components/ui/HealthPill';
import { Logo } from '@/components/ui/primitives';
import { cn, formatCompactCurrency } from '@/lib/utils';
import { usePortfolio } from './PortfolioContext';

export type StudioTab = 'risk' | 'portfolio' | 'benchmark';

export const TABS: { id: StudioTab; label: string; icon: typeof Cpu }[] = [
  { id: 'risk', label: 'Risk & Stress Lab', icon: ShieldAlert },
  { id: 'portfolio', label: 'Portfolio Studio', icon: Briefcase },
  { id: 'benchmark', label: 'Hardware Benchmarking', icon: Cpu },
];

export function StudioShell({ tab, onTab, children }: { tab: StudioTab; onTab: (t: StudioTab) => void; children: ReactNode }) {
  const { stats, positions, source, engine } = usePortfolio();
  return (
    <div className="relative min-h-screen">
      <div className="bg-dot-grid pointer-events-none fixed inset-0 opacity-50" />
      <div className="pointer-events-none fixed left-1/2 top-0 h-[360px] w-[1000px] -translate-x-1/2 bg-[radial-gradient(closest-side,rgba(34,211,238,0.08),transparent)]" />

      <header className="sticky top-0 z-40 border-b border-white/[0.06] bg-obsidian-950/80 backdrop-blur-xl">
        <div className="mx-auto flex h-14 max-w-[1440px] items-center gap-4 px-4 sm:px-6">
          <Link href="/" className="flex items-center gap-2 text-slate-500 transition hover:text-white" aria-label="Back to home">
            <ArrowLeft className="h-4 w-4" />
            <Logo className="hidden sm:flex" />
          </Link>
          <span className="hidden h-5 w-px bg-white/10 sm:block" />
          <span className="font-mono text-[11px] uppercase tracking-[0.16em] text-slate-500">Studio</span>
          <div className="ml-auto flex items-center gap-3">
            <span className="hidden items-center gap-2 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1 text-xs text-slate-400 lg:inline-flex">
              <Briefcase className="h-3.5 w-3.5 text-slate-500" />
              <span className="num text-slate-200">{formatCompactCurrency(stats.netValue)}</span>· {positions.length} positions · {source}
            </span>
            <HealthPill />
          </div>
        </div>
        <nav className="mx-auto flex max-w-[1440px] gap-1 overflow-x-auto px-4 sm:px-6" aria-label="Studio sections" role="tablist">
          {TABS.map((t) => {
            const active = t.id === tab;
            return (
              <button
                key={t.id}
                role="tab"
                aria-selected={active}
                onClick={() => onTab(t.id)}
                className={cn('relative flex shrink-0 items-center gap-2 px-3 pb-3 pt-1.5 text-sm transition-colors', active ? 'text-white' : 'text-slate-500 hover:text-slate-200')}
              >
                <t.icon className={cn('h-4 w-4', active && 'text-accent')} />
                {t.label}
                {active && <motion.span layoutId="studio-tab" className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-accent shadow-[0_0_12px_#22D3EE]" />}
              </button>
            );
          })}
        </nav>
      </header>

      {engine.status === 'offline' && (
        <div className="relative mx-auto mt-4 max-w-[1440px] px-4 sm:px-6">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-xl border border-caution/25 bg-caution/[0.06] px-4 py-2.5 text-xs text-amber-200/90">
            <span className="font-medium">Engine offline.</span>
            <span className="text-amber-200/70">Closed-form previews only. Start the API from the repo root:</span>
            <code className="kbd text-amber-100">cargo run --release -p spectra-api</code>
          </div>
        </div>
      )}

      <main className="relative mx-auto max-w-[1440px] px-4 py-6 sm:px-6 sm:py-8">{children}</main>
    </div>
  );
}
