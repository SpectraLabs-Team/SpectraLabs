'use client';

import { useCallback, useEffect, useState } from 'react';
import { Beaker, Copy, Download, ExternalLink, FileText, Fingerprint, GitFork, Layers, Pencil, Scale, Square } from 'lucide-react';
import { ApiError, request } from '@/lib/api';
import {
  cancelBacktest,
  describeRequest,
  downloadArtifact,
  getArtifactText,
  getBacktest,
  getMonthlyReturns,
  getSeries,
  isFinishedStatus,
  money,
  parseStrategyCurves,
  waitForBacktest,
  DOWNLOADS,
  type Backtest,
  type Catalog,
  type DatasetVersion,
  type DownloadKind,
  type MonthlyReturn,
  type Series,
  type StoredRequest,
  type StrategyCurve,
  type Summary,
} from '@/lib/backtests';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';
import type { MetricId } from '@/lib/learn/metrics';
import { cn } from '@/lib/utils';
import { DrawdownChart, EquityChart, MonthlyHeatmap, StrategyCurvesChart } from './Charts';
import { ago, cash, dash, int, pct, ratio, shortHash, signedPct, tone } from './format';
import { StatusBadge } from './RunList';
import { SurvivalLabPanel } from '../SurvivalLabPanel';
import { BacktestToPaperGap } from '../BacktestToPaperGap';
import { TearSheetModal } from '../TearSheetModal';

type Row = [label: string, value: string, tone?: 'profit' | 'loss' | 'neutral', metric?: MetricId];

function StatCard({ title, rows, note }: { title: string; rows: Row[]; note?: string }) {
  return (
    <GlassCard as="section" className="p-4">
      <h3 className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink/45">{title}</h3>
      <dl className="mt-3 space-y-1.5 text-[12.5px]">
        {rows.map(([k, v, t, metric]) => (
          <div key={k} className="flex justify-between gap-3">
            <dt className="text-ink/55">{metric ? <MetricLabel metric={metric}>{k}</MetricLabel> : k}</dt>
            <dd className={cn('num text-right', t === 'profit' ? 'text-profit' : t === 'loss' ? 'text-loss' : 'text-ink/90')}>{v}</dd>
          </div>
        ))}
      </dl>
      {note && <p className="mt-3 text-[10.5px] leading-4 text-ink/40">{note}</p>}
    </GlassCard>
  );
}

function Kpi({ label, value, sub, t, metric }: { label: string; value: string; sub?: string; t?: 'profit' | 'loss' | 'neutral'; metric?: MetricId }) {
  return (
    <div className="rounded-xl border border-ink/[0.07] bg-ink/[0.02] px-3.5 py-3">
      <p className="text-[10.5px] font-medium uppercase tracking-wider text-ink/45">{metric ? <MetricLabel metric={metric}>{label}</MetricLabel> : label}</p>
      <p className={cn('num mt-1 text-lg font-semibold', t === 'profit' ? 'text-profit' : t === 'loss' ? 'text-loss' : 'text-ink')}>{value}</p>
      {sub && <p className="num mt-0.5 text-[10.5px] text-ink/45">{sub}</p>}
    </div>
  );
}

/** Table column headers that are metrics get an ⓘ. */
const HEADER_METRICS: Partial<Record<string, MetricId>> = {
  Return: 'total_return',
  'Final equity': 'final_equity',
  Fills: 'orders_fills',
  Contribution: 'contribution',
  Sharpe: 'sharpe_ratio',
  'Max DD': 'max_drawdown',
  Fees: 'fees',
  Financing: 'financing',
  Realized: 'realized_pnl',
  Unrealized: 'unrealized_pnl',
  Borrow: 'borrow_fees',
  'Net P&L': 'net_pnl',
};

function HeaderLabel({ h }: { h: string }) {
  const metric = HEADER_METRICS[h];
  if (!metric) return <>{h}</>;
  return (
    <span className="inline-flex items-center justify-end gap-1">
      {h}
      <InfoTip metric={metric} label={h} />
    </span>
  );
}

function duration(b: Backtest): string {
  if (!b.started_at || !b.finished_at) return '';
  const s = (Date.parse(b.finished_at) - Date.parse(b.started_at)) / 1000;
  return s < 60 ? `${s.toFixed(1)} s` : `${(s / 60).toFixed(1)} min`;
}

function ExperimentLineageCard({ runId }: { runId: string }) {
  const [experiment, setExperiment] = useState<{ id: string; name: string } | null>(null);
  const [lineage, setLineage] = useState<{ id: string; name: string }[]>([]);

  useEffect(() => {
    let active = true;
    (async () => {
      try {
        const [run, exps] = await Promise.all([
          request<{ id: string; experiment_id: string }>(`/api/v1/runs/${encodeURIComponent(runId)}`),
          request<{ items: { id: string; name: string }[] }>('/api/v1/experiments').catch(() => ({ items: [] })),
        ]);
        if (!active) return;
        const expId = run.experiment_id;
        const currentExp = exps.items.find((e) => e.id === expId) ?? { id: expId, name: 'Experiment' };
        setExperiment(currentExp);

        if (expId) {
          const lin = await request<{ items: string[] }>(`/api/v1/experiments/${encodeURIComponent(expId)}/lineage`).catch(() => ({ items: [expId] }));
          if (!active) return;
          const chain = (lin.items || [expId]).map((id) => {
            const found = exps.items.find((e) => e.id === id);
            return { id, name: found ? found.name : id.slice(0, 8) };
          });
          setLineage(chain);
        }
      } catch {
        /* run not linked or lineage unavailable */
      }
    })();
    return () => {
      active = false;
    };
  }, [runId]);

  return (
    <GlassCard as="section" className="p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <Beaker className="h-4 w-4 text-accent" /> M5 Research & Lineage
          </h3>
          <p className="mt-0.5 text-xs text-ink/50">
            Recorded in the experiment registry with lineage tracking.
          </p>
        </div>
        {experiment && (
          <a
            href={`/?tab=registry&experiment=${encodeURIComponent(experiment.id)}&run=${encodeURIComponent(runId)}`}
            className="btn-secondary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-accent hover:border-accent/40"
          >
            <span>Open in Experiment Registry</span>
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        )}
      </div>

      <div className="mt-4 space-y-2.5 text-xs">
        <div className="grid grid-cols-[110px_1fr] items-center gap-2">
          <span className="font-mono text-[10.5px] uppercase tracking-wider text-ink/45">Research Run</span>
          <span className="font-mono text-ink/80">{runId}</span>
        </div>

        {experiment && (
          <div className="grid grid-cols-[110px_1fr] items-center gap-2">
            <span className="font-mono text-[10.5px] uppercase tracking-wider text-ink/45">Experiment</span>
            <a
              href={`/?tab=registry&experiment=${encodeURIComponent(experiment.id)}`}
              className="font-medium text-accent hover:underline"
            >
              {experiment.name}
            </a>
          </div>
        )}

        {lineage.length > 0 && (
          <div className="grid grid-cols-[110px_1fr] items-baseline gap-2">
            <span className="flex items-center gap-1 font-mono text-[10.5px] uppercase tracking-wider text-ink/45">
              <GitFork className="h-3 w-3 text-accent" /> Lineage
            </span>
            <div className="flex flex-wrap items-center gap-1.5 font-mono text-[11px]">
              {lineage.map((item, idx) => {
                const isCurrent = item.id === experiment?.id;
                return (
                  <span key={item.id} className="inline-flex items-center gap-1.5">
                    {idx > 0 && <span className="text-ink/30">→</span>}
                    {isCurrent ? (
                      <span className="rounded-md border border-accent/30 bg-accent/10 px-2 py-0.5 font-semibold text-accent">
                        {item.name} (current)
                      </span>
                    ) : (
                      <a
                        href={`/?tab=registry&experiment=${encodeURIComponent(item.id)}`}
                        className="rounded-md bg-ink/5 px-2 py-0.5 text-ink/75 transition hover:bg-ink/10 hover:text-ink"
                        title={`Ancestor: ${item.id}`}
                      >
                        {item.name}
                      </a>
                    )}
                  </span>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </GlassCard>
  );
}

function Results({
  b,
  summary,
  equity,
  drawdown,
  monthly,
  curves,
  catalog,
  onDownload,
  downloading,
}: {
  b: Backtest;
  summary: Summary;
  equity: Series | null;
  drawdown: Series | null;
  monthly: MonthlyReturn[] | null;
  curves: StrategyCurve[];
  catalog: Catalog | null;
  onDownload: (k: DownloadKind) => void;
  downloading: DownloadKind | null;
}) {
  const m = summary.metrics;
  const p = summary.performance;
  const a = summary.activity;
  const tr = summary.trades;
  const ex = summary.exposure;
  const bm = summary.benchmark;
  const start = Number(summary.run.config.initial_cash);
  const multi = summary.strategies?.list ?? [];
  const label = (name: string) => catalog?.strategies.find((s) => s.name === name)?.label ?? name;
  const stored = new Set((b.artifacts ?? []).map((f) => f.kind));

  return (
    <div className="space-y-5">
      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3 xl:grid-cols-6">
        <Kpi metric="total_return" label="Total return" value={signedPct(m.total_return)} t={tone(m.total_return)} sub={`${int(m.periods)} periods`} />
        <Kpi metric="cagr" label="CAGR" value={signedPct(p.cagr)} t={tone(p.cagr)} sub={`vol ${pct(m.annualized_volatility)}`} />
        <Kpi metric="sharpe_ratio" label="Sharpe" value={ratio(m.sharpe)} sub={`Sortino ${ratio(p.sortino)}`} />
        <Kpi metric="max_drawdown" label="Max drawdown" value={pct(m.max_drawdown)} t={m.max_drawdown ? 'loss' : 'neutral'} sub={`Calmar ${ratio(p.calmar)}`} />
        <Kpi metric="final_equity" label="Final equity" value={cash(a.final_equity, 0)} sub={`from ${cash(start, 0)}`} />
        <Kpi metric="trade_count" label="Trades" value={int(tr.count)} sub={`win rate ${pct(tr.win_rate, 0)} · ${int(a.fill_count)} fills`} />
      </div>

      <GlassCard as="section" className="p-5">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h3 className="text-sm font-semibold text-ink">
            <MetricLabel metric="equity_curve">Equity</MetricLabel>
          </h3>
          <p className="num text-[11px] text-ink/45">
            {equity ? `${int(equity.total_points)} points${equity.downsampled ? ', downsampled for the chart (the extremes are kept)' : ''}` : 'loading…'}
          </p>
        </div>
        <div className="mt-3">
          <EquityChart series={equity} start={start} />
        </div>
        <h3 className="mt-4 text-xs font-semibold text-ink/70">
          <MetricLabel metric="max_drawdown">Drawdown</MetricLabel>
        </h3>
        <div className="mt-2">
          <DrawdownChart series={drawdown} />
        </div>
        <div className="mt-5 border-t border-ink/[0.06] pt-4">
          <h3 className="text-xs font-semibold text-ink/70">
            <MetricLabel metric="monthly_returns">Monthly returns</MetricLabel>
          </h3>
          <div className="mt-2.5">
            <MonthlyHeatmap months={monthly} />
          </div>
        </div>
      </GlassCard>

      {multi.length > 0 && (
        <GlassCard as="section" className="overflow-hidden">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink/[0.06] px-5 py-4">
            <div>
              <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
                <Layers className="h-4 w-4 text-accent" /> Strategies
              </h3>
              <p className="mt-0.5 text-xs text-ink/50">Each on its own books and its own share of the cash; the account is their sum.</p>
            </div>
            {summary.strategies?.unallocated && (
              <p className="num text-[11px] text-ink/50">
                Unallocated {cash(summary.strategies.unallocated.allocation, 0)} → {cash(summary.strategies.unallocated.final_cash, 0)}
              </p>
            )}
          </div>
          {curves.length > 0 && (
            <div className="px-5 pt-4">
              <StrategyCurvesChart curves={curves} names={multi.map((s) => label(s.strategy))} />
            </div>
          )}
          <div className="overflow-x-auto">
            <table className="w-full min-w-[860px] text-[12.5px]">
              <thead>
                <tr className="text-left text-[10.5px] uppercase tracking-wider text-ink/45">
                  {['Strategy', 'Weight', 'Allocation', 'Final equity', 'Return', 'Contribution', 'Sharpe', 'Max DD', 'Fills', 'Fees', 'Financing'].map((h, i) => (
                    <th key={h} scope="col" className={cn('px-3 py-2.5 font-medium', i === 0 ? 'pl-5 text-left' : 'text-right', i === 10 && 'pr-5')}>
                      <HeaderLabel h={h} />
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="num">
                {multi.map((s) => (
                  <tr key={s.index} className="border-t border-ink/[0.04]">
                    <td className="py-2 pl-5 pr-3 font-sans text-ink/90">
                      <span className="text-ink/40">#{s.index + 1}</span> {label(s.strategy)}
                    </td>
                    <td className="px-3 py-2 text-right text-ink/70">{pct(s.weight, 1)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{cash(s.allocation, 0)}</td>
                    <td className="px-3 py-2 text-right text-ink/90">{cash(s.final_equity, 0)}</td>
                    <td className={cn('px-3 py-2 text-right', tone(s.total_return) === 'profit' ? 'text-profit' : tone(s.total_return) === 'loss' ? 'text-loss' : '')}>{signedPct(s.total_return)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{signedPct(s.contribution)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{ratio(s.sharpe)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{pct(s.max_drawdown)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{int(s.fill_count)}</td>
                    <td className="px-3 py-2 text-right text-ink/70">{cash(s.total_fees)}</td>
                    <td className="py-2 pl-3 pr-5 text-right text-ink/70">{cash(s.financing)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </GlassCard>
      )}

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <StatCard
          title="Performance"
          rows={[
            ['Annualized return', signedPct(m.annualized_return), tone(m.annualized_return), 'annualized_return'],
            ['Annualized volatility', pct(m.annualized_volatility), undefined, 'annualized_volatility'],
            ['Sharpe', ratio(m.sharpe), undefined, 'sharpe_ratio'],
            ['Sortino', ratio(p.sortino), undefined, 'sortino_ratio'],
            ['Calmar', ratio(p.calmar), undefined, 'calmar_ratio'],
            ['Omega', ratio(p.omega), undefined, 'omega_ratio'],
            ['Skewness', ratio(p.skewness), undefined, 'skewness'],
            ['Excess kurtosis', ratio(p.excess_kurtosis), undefined, 'excess_kurtosis'],
          ]}
        />
        <StatCard
          title="Risk"
          rows={[
            ['Max drawdown', pct(summary.drawdown.max_drawdown), undefined, 'max_drawdown'],
            ['Current drawdown', pct(summary.drawdown.current_drawdown), undefined, 'current_drawdown'],
            ['Longest underwater', `${int(summary.drawdown.longest_underwater_periods)} periods`, undefined, 'longest_underwater'],
            ['Drawdown episodes', int(p.drawdown_episodes), undefined, 'drawdown_episodes'],
            ['Ulcer index', ratio(summary.drawdown.ulcer_index, 4), undefined, 'ulcer_index'],
            [`VaR ${pct(summary.tail.level, 0)}`, pct(summary.tail.var), undefined, 'var_historical'],
            [`Expected shortfall`, pct(summary.tail.expected_shortfall), undefined, 'expected_shortfall_historical'],
            ['Worst period', signedPct(summary.tail.worst_return), tone(summary.tail.worst_return), 'worst_period'],
          ]}
        />
        <StatCard
          title="Trades"
          rows={[
            ['Round trips', int(tr.count), undefined, 'trade_count'],
            ['Win rate', pct(tr.win_rate, 1), undefined, 'win_rate'],
            ['Profit factor', ratio(tr.profit_factor), undefined, 'profit_factor'],
            ['Payoff ratio', ratio(tr.payoff_ratio), undefined, 'payoff_ratio'],
            ['Expectancy', cash(tr.expectancy), undefined, 'expectancy'],
            ['Best / worst', `${cash(tr.best, 0)} / ${cash(tr.worst, 0)}`, undefined, 'best_worst_trade'],
            ['Average holding', tr.avg_holding_days == null ? dash : `${tr.avg_holding_days.toFixed(1)} days`, undefined, 'avg_holding'],
            ['Open positions', int(a.open_positions), undefined, 'open_positions'],
          ]}
          note="Round trips from the fill journal, first in, first out."
        />
        <StatCard
          title="Activity and exposure"
          rows={[
            ['Orders / fills', `${int(a.order_count)} / ${int(a.fill_count)}`, undefined, 'orders_fills'],
            ['Turnover (annual)', ratio(a.turnover_annualized), undefined, 'turnover'],
            ['Fees', cash(a.total_fees), undefined, 'fees'],
            ['Financing', cash(a.financing), tone(money(a.financing)), 'financing'],
            ['Realized P&L', cash(a.realized_pnl), tone(money(a.realized_pnl)), 'realized_pnl'],
            ['Average gross', ex.valid ? pct(ex.average_gross, 0) : dash, undefined, 'average_gross_exposure'],
            ['Average net', ex.valid ? pct(ex.average_net, 0) : dash, undefined, 'average_net_exposure'],
            ['Time in market', ex.valid ? pct(ex.time_in_market, 0) : dash, undefined, 'time_in_market'],
          ]}
        />
      </div>

      {bm.valid && (
        <StatCard
          title="Against the benchmark"
          note={bm.name ? `Benchmark: ${bm.name}.` : undefined}
          rows={[
            ['Benchmark return', signedPct(bm.total_return), tone(bm.total_return), 'benchmark_return'],
            ['Excess return', signedPct(bm.excess_total_return), tone(bm.excess_total_return), 'excess_return'],
            ['Beta', ratio(bm.beta), undefined, 'beta'],
            ['Alpha (annual)', signedPct(bm.alpha_annualized), tone(bm.alpha_annualized), 'alpha'],
            ['Correlation', ratio(bm.correlation), undefined, 'correlation'],
            ['Information ratio', ratio(bm.information_ratio), undefined, 'information_ratio'],
            ['Up / down capture', `${ratio(bm.up_capture)} / ${ratio(bm.down_capture)}`, undefined, 'capture_ratios'],
          ]}
        />
      )}

      {summary.instruments.length > 0 && (
        <GlassCard as="section" className="overflow-hidden">
          <div className="border-b border-ink/[0.06] px-5 py-4">
            <h3 className="text-sm font-semibold text-ink">Per instrument</h3>
            <p className="mt-0.5 text-xs text-ink/50">Net P&amp;L = realized + unrealized − fees − borrow fees.</p>
          </div>
          <div className="max-h-[420px] overflow-auto">
            <table className="w-full min-w-[820px] text-[12.5px]">
              <thead className="sticky top-0 bg-panel/95 backdrop-blur">
                <tr className="text-left text-[10.5px] uppercase tracking-wider text-ink/45">
                  {['Instrument', 'Fills', 'Traded', 'Final qty', 'Market value', 'Realized', 'Unrealized', 'Fees', 'Borrow', 'Net P&L'].map((h, i) => (
                    <th key={h} scope="col" className={cn('px-3 py-2.5 font-medium', i === 0 ? 'pl-5 text-left' : 'text-right', i === 9 && 'pr-5')}>
                      <HeaderLabel h={h} />
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="num">
                {[...summary.instruments]
                  .sort((x, y) => money(y.net_pnl) - money(x.net_pnl))
                  .map((r) => {
                    const net = money(r.net_pnl);
                    return (
                      <tr key={r.instrument} className="border-t border-ink/[0.04]">
                        <td className="py-2 pl-5 pr-3 font-sans text-ink/90">{r.symbol ?? `#${r.instrument}`}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{int(r.fills)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.traded_notional, 0)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{Number(r.final_qty).toLocaleString('en-US', { maximumFractionDigits: 4 })}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.market_value, 0)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.realized_pnl, 0)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.unrealized_pnl, 0)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.fees)}</td>
                        <td className="px-3 py-2 text-right text-ink/70">{cash(r.borrow_fees)}</td>
                        <td className={cn('py-2 pl-3 pr-5 text-right', net > 0 ? 'text-profit' : net < 0 ? 'text-loss' : 'text-ink/70')}>{cash(r.net_pnl, 0)}</td>
                      </tr>
                    );
                  })}
              </tbody>
            </table>
          </div>
        </GlassCard>
      )}

      {b.run_id && <ExperimentLineageCard runId={b.run_id} />}

      <div className="grid gap-4 lg:grid-cols-[1.2fr_1fr]">
        <GlassCard as="section" className="p-5">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <Fingerprint className="h-4 w-4 text-accent" /> Fingerprints
          </h3>
          <p className="mt-0.5 text-xs text-ink/50">The same request, data and engine give the same hashes, here or from <span className="font-mono">spectra-backtest run</span>.</p>
          <dl className="mt-3 space-y-1.5 text-[12px]">
            {(
              [
                ['Engine', b.fingerprints?.engine_version ?? summary.run.engine_version],
                ['Dataset', b.fingerprints?.dataset_sha256 ?? summary.run.dataset_sha256],
                ['Config', b.fingerprints?.config_sha256 ?? summary.run.config_sha256],
                ['Strategy', b.fingerprints?.strategy_sha256 ?? summary.run.strategy_sha256],
                ['Result', b.fingerprints?.result_sha256 ?? summary.run.result_sha256],
                ['Seed', summary.run.seed],
                ['Bars', int(summary.run.bar_count)],
                ...(b.run_id ? ([['Research Run', b.run_id]] as [string, string][]) : []),
              ] as [string, string][]
            ).map(([k, v]) => (
              <div key={k} className="grid grid-cols-[72px_1fr] gap-3">
                <dt className="text-ink/50">{k}</dt>
                <dd className="truncate font-mono text-ink/80" title={v}>
                  {v}
                </dd>
              </div>
            ))}
          </dl>
        </GlassCard>
        <GlassCard as="section" className="p-5">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <Download className="h-4 w-4 text-accent" /> Files
          </h3>
          <p className="mt-0.5 text-xs text-ink/50">Stored with the run, each checked against its SHA-256 when read.</p>
          <div className="mt-3 flex flex-wrap gap-2">
            {DOWNLOADS.filter((d) => stored.has(d.kind)).map((d) => (
              <button key={d.kind} type="button" className="btn-secondary px-2.5 py-1.5 text-[11.5px]" onClick={() => onDownload(d.kind)} disabled={downloading !== null}>
                {downloading === d.kind ? <Spinner className="h-3 w-3" /> : <Download className="h-3 w-3" />}
                {d.file}
              </button>
            ))}
            {!stored.size && <p className="text-[11.5px] text-ink/45">No files were stored for this run.</p>}
          </div>
          <h4 className="mt-4 font-mono text-[10px] uppercase tracking-[0.16em] text-ink/40">Configuration</h4>
          <dl className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-[11.5px]">
            {Object.entries(summary.run.config).map(([k, v]) => (
              <div key={k} className="flex justify-between gap-2">
                <dt className="truncate font-mono text-ink/45">{k}</dt>
                <dd className="num text-ink/80">{v === null ? 'none' : String(v)}</dd>
              </div>
            ))}
          </dl>
        </GlassCard>
      </div>
    </div>
  );
}

/**
 * One backtest: follows it while it runs (progress, cancel), then shows its results. `onChanged`
 * is told when its status changes, so the run list can refresh.
 */
export function RunDetail({
  backtestId,
  catalog,
  datasets,
  canRun,
  onRerun,
  onChanged,
  onAddToCompare,
  isCompared = false,
}: {
  backtestId: string;
  catalog: Catalog | null;
  datasets: DatasetVersion[];
  canRun: boolean;
  onRerun: (r: StoredRequest) => void;
  onChanged: () => void;
  onAddToCompare?: (id: string) => void;
  isCompared?: boolean;
}) {
  const [b, setB] = useState<Backtest | null>(null);
  const [equity, setEquity] = useState<Series | null>(null);
  const [drawdown, setDrawdown] = useState<Series | null>(null);
  const [monthly, setMonthly] = useState<MonthlyReturn[] | null>(null);
  const [curves, setCurves] = useState<StrategyCurve[]>([]);
  const [error, setError] = useState('');
  const [downloading, setDownloading] = useState<DownloadKind | null>(null);
  const [copied, setCopied] = useState(false);
  const [tearSheetOpen, setTearSheetOpen] = useState(false);

  useEffect(() => {
    const ctl = new AbortController();
    setB(null);
    setEquity(null);
    setDrawdown(null);
    setMonthly(null);
    setCurves([]);
    setError('');
    let lastStatus = '';
    (async () => {
      try {
        let current = await getBacktest(backtestId);
        if (ctl.signal.aborted) return;
        setB(current);
        lastStatus = current.status;
        if (!isFinishedStatus(current.status)) {
          current = await waitForBacktest(backtestId, {
            signal: ctl.signal,
            onUpdate: (u) => {
              if (ctl.signal.aborted) return;
              setB(u);
              if (u.status !== lastStatus) {
                lastStatus = u.status;
                onChanged();
              }
            },
          });
        }
        if (ctl.signal.aborted || current.status !== 'succeeded') return;
        const [eq, dd, monthlyRes] = await Promise.all([
          getSeries(backtestId, 'equity'),
          getSeries(backtestId, 'drawdown'),
          getMonthlyReturns(backtestId).catch(() => null),
        ]);
        if (ctl.signal.aborted) return;
        setEquity(eq);
        setDrawdown(dd);
        if (monthlyRes?.months) setMonthly(monthlyRes.months);
        if (current.summary?.strategies && current.artifacts?.some((f) => f.kind === 'strategies_csv')) {
          const text = await getArtifactText(backtestId, 'strategies_csv');
          if (!ctl.signal.aborted) setCurves(parseStrategyCurves(text));
        }
      } catch (e) {
        if (!ctl.signal.aborted) setError(e instanceof ApiError && e.status === 404 ? 'This backtest no longer exists in this workspace.' : e instanceof Error ? e.message : 'Could not load the backtest.');
      }
    })();
    return () => ctl.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the selection changes
  }, [backtestId]);

  const cancel = useCallback(async () => {
    try {
      setB(await cancelBacktest(backtestId));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not cancel the backtest.');
    }
  }, [backtestId]);

  const download = async (kind: DownloadKind) => {
    setDownloading(kind);
    try {
      await downloadArtifact(backtestId, kind);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not download the file.');
    } finally {
      setDownloading(null);
    }
  };

  if (!b) {
    return (
      <GlassCard className="flex min-h-[200px] items-center justify-center p-5 text-sm text-ink/50">
        {error ? <ErrorNote>{error}</ErrorNote> : <Spinner />}
      </GlassCard>
    );
  }

  const dataset = datasets.find((d) => d.id === b.request.dataset_version_id);
  const running = !isFinishedStatus(b.status);

  return (
    <div className="space-y-5">
      <GlassCard as="section" className="p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-base font-semibold text-ink">{describeRequest(b.request, catalog)}</h2>
              <StatusBadge status={b.status} cancelling={b.cancel_requested && running} />
            </div>
            <p className="num mt-1 text-[11.5px] text-ink/50">
              {dataset ? `${dataset.dataset_name} v${dataset.version}` : shortHash(b.request.dataset_version_id, 8)} · queued {ago(b.created_at)}
              {duration(b) && ` · ran ${duration(b)}`}
              {typeof b.request.config?.preset === 'string' && ` · ${b.request.config.preset} preset`}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="btn-ghost"
              onClick={() => {
                navigator.clipboard?.writeText(b.id).then(() => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1200);
                });
              }}
              title={b.id}
            >
              <Copy className="h-3.5 w-3.5" /> {copied ? 'Copied' : b.id.slice(0, 8)}
            </button>
            {b.status === 'succeeded' && b.summary && (
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-xs flex items-center gap-1.5 text-accent border-accent/30 hover:bg-accent/10"
                onClick={() => setTearSheetOpen(true)}
                title="Export executive tear-sheet PDF and audit reports"
              >
                <FileText className="h-3.5 w-3.5" />
                <span>Tear-Sheet & PDF</span>
              </button>
            )}
            {b.status === 'succeeded' && onAddToCompare && (
              <button
                type="button"
                className={cn(
                  'btn-secondary px-3 py-1.5 text-xs flex items-center gap-1.5',
                  isCompared && 'bg-accent/15 text-accent border-accent/40 font-semibold'
                )}
                onClick={() => onAddToCompare(b.id)}
                title={isCompared ? 'Remove from comparison' : 'Add to comparison list'}
              >
                <Scale className="h-3.5 w-3.5" />
                <span>{isCompared ? 'In Comparison' : 'Compare'}</span>
              </button>
            )}
            {canRun && running && !b.cancel_requested && (
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={cancel}>
                <Square className="h-3.5 w-3.5" /> Cancel
              </button>
            )}
            {canRun && (
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => onRerun(b.request)}>
                <Pencil className="h-3.5 w-3.5" /> Edit and run again
              </button>
            )}
          </div>
        </div>

        {running && (
          <div className="mt-4" aria-live="polite">
            <div className="flex justify-between text-[11px] text-ink/55">
              <span>{b.status === 'queued' ? 'Waiting for a worker…' : b.cancel_requested ? 'Stopping at the next bar…' : 'Running on a worker'}</span>
              <span className="num">{Math.round(b.progress * 100)}%</span>
            </div>
            <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-ink/5">
              <div className={cn('h-full rounded-full bg-accent transition-[width] duration-500', b.status === 'queued' && 'animate-pulse bg-accent/40')} style={{ width: `${b.status === 'queued' ? 100 : Math.max(2, Math.min(100, b.progress * 100))}%` }} />
            </div>
          </div>
        )}
        {b.status === 'failed' && (
          <div className="mt-4">
            <ErrorNote>{b.error ?? 'The backtest failed.'}</ErrorNote>
          </div>
        )}
        {b.status === 'cancelled' && <p className="mt-4 text-[12px] text-ink/55">Cancelled before it finished; nothing was recorded.</p>}
        {error && (
          <div className="mt-4">
            <ErrorNote>{error}</ErrorNote>
          </div>
        )}
        {b.status === 'succeeded' && !b.summary && <p className="mt-4 text-[12px] text-ink/55">Finished, but no summary was recorded.</p>}
      </GlassCard>

      {b.status === 'succeeded' && b.summary && (
        <Results b={b} summary={b.summary} equity={equity} drawdown={drawdown} monthly={monthly} curves={curves} catalog={catalog} onDownload={download} downloading={downloading} />
      )}
      {b.status === 'succeeded' && b.run_id && (
        <SurvivalLabPanel
          runId={b.run_id}
          enabled
          strategy={b.request.strategy ?? describeRequest(b.request, catalog)}
          params={b.request.params ?? {}}
          datasetId={dataset ? `${dataset.dataset_name} v${dataset.version}` : b.request.dataset_version_id ?? ''}
        />
      )}
      {b.status === 'succeeded' && (
        <GlassCard className="p-5">
          <BacktestToPaperGap strategyId={b.request.strategy_version_id} />
        </GlassCard>
      )}

      {b.status === 'succeeded' && b.summary && (
        <TearSheetModal
          open={tearSheetOpen}
          onClose={() => setTearSheetOpen(false)}
          title="Quantitative Strategy Tear-Sheet"
          subtitle={`${b.request.strategy ?? describeRequest(b.request, catalog)} · Started ${new Date(b.created_at).toLocaleDateString()}`}
          reportType="Quantitative Backtest"
          timestamp={new Date(b.created_at).toISOString()}
          metadata={{
            engine: 'Spectra Quantitative Backtest Engine',
            engineVersion: '0.1.0',
            commitSha: b.run_id ? b.run_id.slice(0, 8) : undefined,
            seed: b.request.params?.seed != null ? String(b.request.params.seed) : undefined,
            datasetOrBook: dataset ? `${dataset.dataset_name} v${dataset.version}` : (b.request.dataset_version_id ?? 'Market Dataset'),
            duration: b.summary.run.wall_clock_seconds ? `${b.summary.run.wall_clock_seconds.toFixed(2)}s wall-clock` : undefined,
          }}
          sections={[
            {
              category: 'Return & Capital Dynamics',
              items: [
                {
                  label: 'Total Return',
                  value: signedPct(b.summary.metrics.total_return),
                  sub: `${int(b.summary.metrics.periods)} trading periods`,
                  tone: tone(b.summary.metrics.total_return),
                },
                {
                  label: 'Compound Annual Growth (CAGR)',
                  value: signedPct(b.summary.performance.cagr),
                  sub: `Annualized σ ${pct(b.summary.metrics.annualized_volatility)}`,
                  tone: tone(b.summary.performance.cagr),
                },
                {
                  label: 'Initial Capital',
                  value: cash(Number(b.summary.run.config.initial_cash), 0),
                  sub: 'Starting cash',
                },
                {
                  label: 'Final Equity',
                  value: cash(b.summary.activity.final_equity, 0),
                  sub: `Net profit ${cash(b.summary.performance.net_profit, 0)}`,
                  tone: tone(b.summary.performance.net_profit),
                },
              ],
            },
            {
              category: 'Risk-Adjusted Performance Ratios',
              items: [
                {
                  label: 'Sharpe Ratio',
                  value: ratio(b.summary.metrics.sharpe),
                  sub: 'Annualized excess return / total risk',
                },
                {
                  label: 'Sortino Ratio',
                  value: ratio(b.summary.performance.sortino),
                  sub: 'Excess return / downside deviation',
                },
                {
                  label: 'Calmar Ratio',
                  value: ratio(b.summary.performance.calmar),
                  sub: 'CAGR / maximum drawdown',
                },
                {
                  label: 'Maximum Drawdown',
                  value: pct(b.summary.drawdown.max_drawdown),
                  sub: `Longest underwater: ${int(b.summary.drawdown.longest_underwater_periods)} periods`,
                  tone: 'loss',
                },
              ],
            },
            {
              category: 'Trading Execution & Distribution',
              items: [
                {
                  label: 'Total Completed Trades',
                  value: int(b.summary.trades.count),
                  sub: `${int(b.summary.activity.fill_count)} individual fills`,
                },
                {
                  label: 'Win Rate',
                  value: pct(b.summary.trades.win_rate, 1),
                  sub: `Profit factor: ${ratio(b.summary.trades.profit_factor)}`,
                },
                {
                  label: 'Payoff Ratio (Win / Loss)',
                  value: ratio(b.summary.trades.payoff_ratio),
                  sub: `Expectancy: ${cash(b.summary.trades.expectancy)}`,
                },
                {
                  label: 'Commission & Fees Paid',
                  value: cash(b.summary.activity.commission_paid, 2),
                  sub: 'Transaction friction',
                },
              ],
            },
            {
              category: 'Tail Risk & Statistical Moments',
              items: [
                {
                  label: `Value at Risk (${pct(b.summary.tail.level, 0)})`,
                  value: pct(b.summary.tail.var),
                  sub: 'Historical simulation VaR',
                  tone: 'loss',
                },
                {
                  label: 'Expected Shortfall (CVaR)',
                  value: pct(b.summary.tail.expected_shortfall),
                  sub: 'Average loss in tail regime',
                  tone: 'loss',
                },
                {
                  label: 'Return Skewness',
                  value: ratio(b.summary.performance.skewness),
                  sub: 'Asymmetry of return distribution',
                },
                {
                  label: 'Excess Kurtosis',
                  value: ratio(b.summary.performance.excess_kurtosis),
                  sub: 'Fat-tail indicator',
                },
              ],
            },
          ]}
          table={
            b.summary.strategies?.list && b.summary.strategies.list.length > 0
              ? {
                  title: 'Strategy Sub-Component Allocation & Attribution',
                  headers: ['Strategy', 'Allocation', 'Final Equity', 'Return', 'Contribution', 'Sharpe', 'Max DD'],
                  rows: b.summary.strategies.list.map((s) => [
                    catalog?.strategies.find((strat) => strat.name === s.name)?.label ?? s.name,
                    cash(s.allocation, 0),
                    cash(s.final_equity, 0),
                    signedPct(s.total_return),
                    signedPct(s.contribution),
                    ratio(s.sharpe),
                    pct(s.max_drawdown),
                  ]),
                }
              : undefined
          }
          rawJsonData={b.summary}
          rawCsvData={
            b.summary.strategies?.list
              ? [
                  'Strategy,Allocation,Final Equity,Total Return,Contribution,Sharpe,Max Drawdown',
                  ...b.summary.strategies.list.map(
                    (s) => `"${s.name}",${s.allocation},${s.final_equity},${s.total_return},${s.contribution},${s.sharpe},${s.max_drawdown}`
                  ),
                ].join('\n')
              : undefined
          }
        />
      )}
    </div>
  );
}
