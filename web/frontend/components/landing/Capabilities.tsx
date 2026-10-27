'use client';

import Link from 'next/link';
import { motion, useReducedMotion } from 'framer-motion';
import { Activity, ArrowRight, BarChart3, Layers, Target } from 'lucide-react';
import type { ReactNode } from 'react';
import { GlowCard } from '@/components/ui/primitives';
import { HEADLINE } from '@/lib/baseline';
import { cn, formatCompact } from '@/lib/utils';

const CARDS: { title: string; body: ReactNode; icon: typeof Activity; tile: string; href: string; badge?: string }[] = [
  {
    title: 'Monte Carlo Risk Engine',
    body: `Monte Carlo VaR and Expected Shortfall with tail and higher-moment metrics (skewness, kurtosis, downside deviation). ${formatCompact(HEADLINE.rustPps, 0)}+ paths per second measured.`,
    icon: Target,
    tile: 'from-cyan-400/25 to-sky-500/10 text-accent ring-accent/30',
    href: '/studio?tab=risk',
  },
  {
    title: 'Event-Driven Backtester',
    body: 'Zero look-ahead bias by construction, a price-indexed limit order book, a gross leverage limit in the ledger, and realistic fills with slippage and participation caps.',
    icon: Activity,
    tile: 'from-sky-400/25 to-cyan-500/10 text-accent-ice ring-accent-ice/30',
    href: '/studio?tab=backtests',
  },
  {
    title: 'Overfitting & Validation Lab',
    body: 'Deflated Sharpe Ratio (DSR), Combinatorially Symmetric Cross-Validation (CSCV), PBO, walk-forward and strategy survival analysis, with every trial ledgered.',
    icon: BarChart3,
    tile: 'from-violet-500/30 to-fuchsia-500/10 text-violet-500 ring-violet-500/30 dark:text-violet-400',
    href: '/studio?tab=backtests',
  },
  {
    title: 'Brokerage & Execution Layer',
    body: 'Paper trading on a simulated ledger and live broker execution via Alpaca with pre-trade risk checks, order sync, and automated position reconciliation.',
    icon: Layers,
    tile: 'from-emerald-400/25 to-teal-500/10 text-profit ring-profit/30',
    href: '/studio?tab=settings',
    badge: 'Paper · Live Broker Proxy',
  },
];

export function Capabilities() {
  const reduce = useReducedMotion();
  return (
    <section id="features" className="relative pb-10 pt-20">
      <div className="mx-auto max-w-[1320px] px-5 sm:px-8">
        <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
          <div>
            <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Platform capabilities</p>
            <h2 className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.15rem]">A Complete Quantitative Research Platform</h2>
          </div>
          <p className="max-w-md text-sm leading-6 text-ink/60">From risk modeling to strategy validation and paper execution: research, test and audit with confidence.</p>
        </div>
        <div className="mt-10 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {CARDS.map((c, i) => {
            const external = c.href.startsWith('http');
            return (
              <motion.div key={c.title} initial={reduce ? false : { opacity: 0, y: 18 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true, margin: '-60px' }} transition={{ duration: 0.5, delay: i * 0.07 }}>
                <GlowCard className="group flex h-full flex-col p-6">
                  <span className={cn('flex h-14 w-14 items-center justify-center rounded-xl bg-gradient-to-br ring-1 shadow-[0_0_30px_-6px_currentColor]', c.tile)}>
                    <c.icon className="h-7 w-7" />
                  </span>
                  <h3 className="mt-6 text-lg font-semibold leading-6 text-ink">{c.title}</h3>
                  <p className="mt-2.5 text-sm leading-6 text-ink/60">{c.body}</p>
                  <div className="mt-auto flex items-end justify-between pt-5">
                    {c.badge ? <span className="rounded-full border border-caution/30 bg-caution/10 px-2 py-0.5 text-[10.5px] font-medium text-caution">{c.badge}</span> : <span />}
                    {external ? (
                      <a href={c.href} target="_blank" rel="noreferrer" aria-label={`${c.title}: source`} className="flex h-8 w-8 items-center justify-center rounded-full border border-ink/15 text-ink/60 transition group-hover:border-accent/50 group-hover:text-accent">
                        <ArrowRight className="h-4 w-4" />
                      </a>
                    ) : (
                      <Link href={c.href} aria-label={`Open ${c.title}`} className="flex h-8 w-8 items-center justify-center rounded-full border border-ink/15 text-ink/60 transition group-hover:border-accent/50 group-hover:text-accent">
                        <ArrowRight className="h-4 w-4" />
                      </Link>
                    )}
                  </div>
                </GlowCard>
              </motion.div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
