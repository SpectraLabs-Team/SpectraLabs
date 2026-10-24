'use client';

import Link from 'next/link';
import { ArrowRight, Database, FolderOpen, Link2, LineChart, ShieldCheck, TrendingUp, Zap } from 'lucide-react';
import { cn } from '@/lib/utils';

type Status = 'live' | 'backtest' | 'roadmap';
const STATUS: Record<Status, { label: string; legend: string; cls: string; title: string }> = {
  live: { label: 'Live', legend: 'Live in quantitative engine', cls: 'border-profit/30 bg-profit/10 text-profit', title: 'Computed natively in Rust by spectra-risk / spectra-engine' },
  backtest: { label: 'Backtest', legend: 'On strategy returns', cls: 'border-accent/30 bg-accent/10 text-accent', title: 'Computed on backtest & walk-forward returns by spectra-engine' },
  roadmap: { label: 'Roadmap', legend: 'Scheduled feature', cls: 'border-ink/15 bg-ink/[0.04] text-ink/45', title: 'Scheduled for upcoming module rollout' },
};

const GROUPS: { title: string; icon: typeof Zap; color: string; dot: string; items: [string, Status][] }[] = [
  {
    title: 'Market Tail Risk',
    icon: TrendingUp,
    color: 'text-rose-500',
    dot: 'bg-rose-500',
    items: [
      ['VaR & CVaR (ES)', 'live'],
      ['SVaR (Stressed VaR)', 'live'],
      ['Extreme Value Theory (EVT)', 'live'],
      ['Clayton & Student-t Copulas', 'live'],
      ['GARCH(1,1) / GJR-Volatility', 'live'],
      ['Markov Regime Switching (HMM)', 'live'],
    ],
  },
  {
    title: 'Options & Derivatives',
    icon: Link2,
    color: 'text-violet-500',
    dot: 'bg-violet-500',
    items: [
      ['The Greeks (Δ, Γ, ν, θ)', 'roadmap'],
      ['Implied Volatility Surface', 'roadmap'],
      ['Local Volatility SVI', 'roadmap'],
      ['Cross-Asset Smile Calibration', 'roadmap'],
    ],
  },
  {
    title: 'Performance Ratios',
    icon: LineChart,
    color: 'text-sky-500',
    dot: 'bg-sky-500',
    items: [
      ['Sharpe & Sortino Ratios', 'backtest'],
      ['Deflated Sharpe Ratio (DSR)', 'live'],
      ['Prob. Backtest Overfit (PBO)', 'live'],
      ['Calmar & Omega Ratios', 'backtest'],
      ['Combinatorial CV (CSCV)', 'live'],
    ],
  },
  {
    title: 'Liquidity & Execution',
    icon: Zap,
    color: 'text-amber-500',
    dot: 'bg-amber-500',
    items: [
      ['Slippage Modeling', 'live'],
      ['Bid-Ask Spread Dynamics', 'live'],
      ['Square-Root Market Impact', 'live'],
      ['Order Fill Latency Simulation', 'live'],
      ['LVaR (Liquidity VaR)', 'roadmap'],
    ],
  },
  {
    title: 'Fixed Income & Rates',
    icon: FolderOpen,
    color: 'text-orange-500',
    dot: 'bg-orange-500',
    items: [
      ['DV01 & Key-Rate Durations', 'roadmap'],
      ['Macaulay & Modified Duration', 'roadmap'],
      ['Bond Convexity', 'roadmap'],
      ['Nelson-Siegel Yield Curves', 'roadmap'],
    ],
  },
  {
    title: 'Systemic & Regulatory',
    icon: ShieldCheck,
    color: 'text-emerald-500',
    dot: 'bg-emerald-500',
    items: [
      ['HHI Concentration', 'live'],
      ['Euler CVaR Risk Attribution', 'live'],
      ['Cryptographic Audit Chain', 'live'],
      ['CoVaR & Marginal Expected Shortfall', 'roadmap'],
      ['Basel III / FRTB Capital Metrics', 'roadmap'],
    ],
  },
];

export function Taxonomy() {
  return (
    <section id="taxonomy" className="py-10" aria-labelledby="taxonomy-title">
      <div className="mx-auto max-w-[1320px] px-5 sm:px-8">
        <div className="glass rounded-2xl border-accent/20 p-5 sm:p-7">
          <div className="flex flex-col justify-between gap-4 md:flex-row md:items-start">
            <div className="flex gap-4">
              <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent/10 text-accent ring-1 ring-accent/30">
                <Database className="h-6 w-6" />
              </span>
              <div>
                <h2 id="taxonomy-title" className="text-lg font-semibold text-ink">
                  Institutional Risk &amp; Quantitative Metrics Taxonomy
                </h2>
                <p className="mt-1 text-sm text-ink/60">Full-spectrum coverage map for quantitative research and institutional risk, with what ships today marked.</p>
                <div className="mt-3 flex flex-wrap gap-2">
                  {(Object.keys(STATUS) as Status[]).map((k) => (
                    <span key={k} className={cn('rounded-full border px-2 py-0.5 text-[10.5px] font-medium', STATUS[k].cls)} title={STATUS[k].title}>
                      {STATUS[k].label} · {STATUS[k].legend}
                    </span>
                  ))}
                </div>
              </div>
            </div>
            <Link href="/architecture" className="btn-secondary shrink-0 border-accent/40 text-accent hover:border-accent">
              View Architecture &amp; Parity <ArrowRight className="h-4 w-4" />
            </Link>
          </div>

          <div className="mt-7 grid gap-3 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-6">
            {GROUPS.map((g) => (
              <div key={g.title} className="rounded-xl border border-ink/[0.08] bg-canvas/50 p-4 transition hover:border-ink/20">
                <div className="flex items-center gap-2.5">
                  <g.icon className={cn('h-5 w-5', g.color)} />
                  <h3 className="text-[13.5px] font-semibold text-ink">{g.title}</h3>
                </div>
                <ul className="mt-3.5 space-y-2">
                  {g.items.map(([name, status]) => (
                    <li key={name} className="flex items-center gap-2 text-[12.5px]">
                      <span className={cn('h-1.5 w-1.5 shrink-0 rounded-full', status === 'roadmap' ? 'bg-ink/25' : g.dot)} />
                      <span className={cn('min-w-0 flex-1', status === 'roadmap' ? 'text-ink/50' : 'text-ink/85')}>{name}</span>
                      <span className={cn('shrink-0 rounded border px-1 py-px font-mono text-[9px] uppercase tracking-wide', STATUS[status].cls)} title={STATUS[status].title}>
                        {STATUS[status].label}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
