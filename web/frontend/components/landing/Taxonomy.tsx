'use client';

import { ArrowRight, Database, FolderOpen, Link2, LineChart, ShieldCheck, TrendingUp, Zap } from 'lucide-react';
import { GITHUB_URL } from '@/lib/site';
import { cn } from '@/lib/utils';

type Status = 'live' | 'backtest' | 'roadmap';
const STATUS: Record<Status, { label: string; legend: string; cls: string; title: string }> = {
  live: { label: 'Live', legend: 'Live in the risk engine', cls: 'border-profit/30 bg-profit/10 text-profit', title: 'Computed by the Monte Carlo engine (/api/risk)' },
  backtest: { label: 'Backtest', legend: 'On backtest returns', cls: 'border-accent/30 bg-accent/10 text-accent', title: 'Computed on backtest returns by spectra-engine' },
  roadmap: { label: 'Roadmap', legend: 'Not built yet', cls: 'border-ink/15 bg-ink/[0.04] text-ink/45', title: 'Not implemented yet: needs a dedicated model or data' },
};

const GROUPS: { title: string; icon: typeof Zap; color: string; dot: string; items: [string, Status][] }[] = [
  {
    title: 'Market Tail Risk',
    icon: TrendingUp,
    color: 'text-rose-500',
    dot: 'bg-rose-500',
    items: [['VaR', 'live'], ['CVaR (ES)', 'live'], ['Skewness', 'live'], ['Kurtosis', 'live'], ['SVaR', 'roadmap'], ['Extreme Value Theory', 'roadmap']],
  },
  {
    title: 'Options & Derivatives',
    icon: Link2,
    color: 'text-violet-500',
    dot: 'bg-violet-500',
    items: [['The Greeks (Δ, Γ, ν, θ)', 'roadmap'], ['Implied Volatility', 'roadmap'], ['Volatility Surface', 'roadmap'], ['Options Risk Metrics', 'roadmap']],
  },
  {
    title: 'Performance Ratios',
    icon: LineChart,
    color: 'text-sky-500',
    dot: 'bg-sky-500',
    items: [['Sharpe Ratio', 'backtest'], ['Sortino Ratio', 'backtest'], ['Calmar Ratio', 'backtest'], ['Omega Ratio', 'backtest'], ['Max Drawdown', 'backtest']],
  },
  {
    title: 'Liquidity & Execution',
    icon: Zap,
    color: 'text-amber-500',
    dot: 'bg-amber-500',
    items: [['Slippage Modeling', 'backtest'], ['LVaR (Liquidity VaR)', 'roadmap'], ['Bid-Ask Spread', 'roadmap'], ['Market Impact', 'roadmap']],
  },
  {
    title: 'Fixed Income & Duration',
    icon: FolderOpen,
    color: 'text-orange-500',
    dot: 'bg-orange-500',
    items: [['DV01', 'roadmap'], ['Duration', 'roadmap'], ['Convexity', 'roadmap'], ['Yield Curve Risk', 'roadmap']],
  },
  {
    title: 'Systemic & Regulatory',
    icon: ShieldCheck,
    color: 'text-emerald-500',
    dot: 'bg-emerald-500',
    items: [['HHI Concentration', 'live'], ['CoVaR, MES', 'roadmap'], ['Basel III Metrics', 'roadmap'], ['Systemic Risk Indicators', 'roadmap']],
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
            <a href={`${GITHUB_URL}#readme`} target="_blank" rel="noreferrer" className="btn-secondary shrink-0 border-accent/40 text-accent hover:border-accent">
              View Documentation <ArrowRight className="h-4 w-4" />
            </a>
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
