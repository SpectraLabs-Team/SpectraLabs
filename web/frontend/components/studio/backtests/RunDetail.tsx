'use client';

import { useCallback, useEffect, useState } from 'react';
import { Copy, Download, Fingerprint, Layers, Pencil, Square } from 'lucide-react';
import { ApiError } from '@/lib/api';
import {
  cancelBacktest,
  describeRequest,
  downloadArtifact,
  getArtifactText,
  getBacktest,
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
  type Series,
  type StoredRequest,
  type StrategyCurve,
  type Summary,
} from '@/lib/backtests';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { DrawdownChart, EquityChart, StrategyCurvesChart } from './Charts';
import { ago, cash, dash, int, pct, ratio, shortHash, signedPct, tone } from './format';
import { StatusBadge } from './RunList';

type Row = [label: string, value: string, tone?: 'profit' | 'loss' | 'neutral'];

function StatCard({ title, rows, note }: { title: string; rows: Row[]; note?: string }) {
  return (
    <GlassCard as="section" className="p-4">
      <h3 className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink/45">{title}</h3>
      <dl className="mt-3 space-y-1.5 text-[12.5px]">
        {rows.map(([k, v, t]) => (
          <div key={k} className="flex justify-between gap-3">
            <dt className="text-ink/55">{k}</dt>
            <dd className={cn('num text-right', t === 'profit' ? 'text-profit' : t === 'loss' ? 'text-loss' : 'text-ink/90')}>{v}</dd>
          </div>
        ))}
      </dl>
      {note && <p className="mt-3 text-[10.5px] leading-4 text-ink/40">{note}</p>}
    </GlassCard>
  );
}

function Kpi({ label, value, sub, t }: { label: string; value: string; sub?: string; t?: 'profit' | 'loss' | 'neutral' }) {
  return (
    <div className="rounded-xl border border-ink/[0.07] bg-ink/[0.02] px-3.5 py-3">
      <p className="text-[10.5px] font-medium uppercase tracking-wider text-ink/45">{label}</p>
      <p className={cn('num mt-1 text-lg font-semibold', t === 'profit' ? 'text-profit' : t === 'loss' ? 'text-loss' : 'text-ink')}>{value}</p>
      {sub && <p className="num mt-0.5 text-[10.5px] text-ink/45">{sub}</p>}
    </div>
  );
}

function duration(b: Backtest): string {
  if (!b.started_at || !b.finished_at) return '';
  const s = (Date.parse(b.finished_at) - Date.parse(b.started_at)) / 1000;
  return s < 60 ? `${s.toFixed(1)} s` : `${(s / 60).toFixed(1)} min`;
}

function Results({ b, summary, equity, drawdown, curves, catalog, onDownload, downloading }: { b: Backtest; summary: Summary; equity: Series | null; drawdown: Series | null; curves: StrategyCurve[]; catalog: Catalog | null; onDownload: (k: DownloadKind) => void; downloading: DownloadKind | null }) {
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
        <Kpi label="Total return" value={signedPct(m.total_return)} t={tone(m.total_return)} sub={`${int(m.periods)} periods`} />
        <Kpi label="CAGR" value={signedPct(p.cagr)} t={tone(p.cagr)} sub={`vol ${pct(m.annualized_volatility)}`} />
        <Kpi label="Sharpe" value={ratio(m.sharpe)} sub={`Sortino ${ratio(p.sortino)}`} />
        <Kpi label="Max drawdown" value={pct(m.max_drawdown)} t={m.max_drawdown ? 'loss' : 'neutral'} sub={`Calmar ${ratio(p.calmar)}`} />
        <Kpi label="Final equity" value={cash(a.final_equity, 0)} sub={`from ${cash(start, 0)}`} />
        <Kpi label="Trades" value={int(tr.count)} sub={`win rate ${pct(tr.win_rate, 0)} · ${int(a.fill_count)} fills`} />
      </div>

      <GlassCard as="section" className="p-5">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h3 className="text-sm font-semibold text-ink">Equity</h3>
          <p className="num text-[11px] text-ink/45">
            {equity ? `${int(equity.total_points)} points${equity.downsampled ? ', downsampled for the chart (the extremes are kept)' : ''}` : 'loading…'}
          </p>
        </div>
        <div className="mt-3">
          <EquityChart series={equity} start={start} />
        </div>
        <h3 className="mt-4 text-xs font-semibold text-ink/70">Drawdown</h3>
        <div className="mt-2">
          <DrawdownChart series={drawdown} />
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
                    <th key={h} className={cn('px-3 py-2.5 font-medium', i === 0 ? 'pl-5 text-left' : 'text-right', i === 10 && 'pr-5')}>
                      {h}
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
            ['Annualized return', signedPct(m.annualized_return), tone(m.annualized_return)],
            ['Annualized volatility', pct(m.annualized_volatility)],
            ['Sharpe', ratio(m.sharpe)],
            ['Sortino', ratio(p.sortino)],
            ['Calmar', ratio(p.calmar)],
            ['Omega', ratio(p.omega)],
            ['Skewness', ratio(p.skewness)],
            ['Excess kurtosis', ratio(p.excess_kurtosis)],
          ]}
        />
        <StatCard
          title="Risk"
          rows={[
            ['Max drawdown', pct(summary.drawdown.max_drawdown)],
            ['Current drawdown', pct(summary.drawdown.current_drawdown)],
            ['Longest underwater', `${int(summary.drawdown.longest_underwater_periods)} periods`],
            ['Drawdown episodes', int(p.drawdown_episodes)],
            ['Ulcer index', ratio(summary.drawdown.ulcer_index, 4)],
            [`VaR ${pct(summary.tail.level, 0)}`, pct(summary.tail.var)],
            [`Expected shortfall`, pct(summary.tail.expected_shortfall)],
            ['Worst period', signedPct(summary.tail.worst_return), tone(summary.tail.worst_return)],
          ]}
        />
        <StatCard
          title="Trades"
          rows={[
            ['Round trips', int(tr.count)],
            ['Win rate', pct(tr.win_rate, 1)],
            ['Profit factor', ratio(tr.profit_factor)],
            ['Payoff ratio', ratio(tr.payoff_ratio)],
            ['Expectancy', cash(tr.expectancy)],
            ['Best / worst', `${cash(tr.best, 0)} / ${cash(tr.worst, 0)}`],
            ['Average holding', tr.avg_holding_days == null ? dash : `${tr.avg_holding_days.toFixed(1)} days`],
            ['Open positions', int(a.open_positions)],
          ]}
          note="Round trips from the fill journal, first in, first out."
        />
        <StatCard
          title="Activity and exposure"
          rows={[
            ['Orders / fills', `${int(a.order_count)} / ${int(a.fill_count)}`],
            ['Turnover (annual)', ratio(a.turnover_annualized)],
            ['Fees', cash(a.total_fees)],
            ['Financing', cash(a.financing), tone(money(a.financing))],
            ['Realized P&L', cash(a.realized_pnl), tone(money(a.realized_pnl))],
            ['Average gross', ex.valid ? pct(ex.average_gross, 0) : dash],
            ['Average net', ex.valid ? pct(ex.average_net, 0) : dash],
            ['Time in market', ex.valid ? pct(ex.time_in_market, 0) : dash],
          ]}
        />
      </div>

      {bm.valid && (
        <StatCard
          title="Against the benchmark"
          note={bm.name ? `Benchmark: ${bm.name}.` : undefined}
          rows={[
            ['Benchmark return', signedPct(bm.total_return), tone(bm.total_return)],
            ['Excess return', signedPct(bm.excess_total_return), tone(bm.excess_total_return)],
            ['Beta', ratio(bm.beta)],
            ['Alpha (annual)', signedPct(bm.alpha_annualized), tone(bm.alpha_annualized)],
            ['Correlation', ratio(bm.correlation)],
            ['Information ratio', ratio(bm.information_ratio)],
            ['Up / down capture', `${ratio(bm.up_capture)} / ${ratio(bm.down_capture)}`],
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
                    <th key={h} className={cn('px-3 py-2.5 font-medium', i === 0 ? 'pl-5 text-left' : 'text-right', i === 9 && 'pr-5')}>
                      {h}
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
export function RunDetail({ backtestId, catalog, datasets, canRun, onRerun, onChanged }: { backtestId: string; catalog: Catalog | null; datasets: DatasetVersion[]; canRun: boolean; onRerun: (r: StoredRequest) => void; onChanged: () => void }) {
  const [b, setB] = useState<Backtest | null>(null);
  const [equity, setEquity] = useState<Series | null>(null);
  const [drawdown, setDrawdown] = useState<Series | null>(null);
  const [curves, setCurves] = useState<StrategyCurve[]>([]);
  const [error, setError] = useState('');
  const [downloading, setDownloading] = useState<DownloadKind | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const ctl = new AbortController();
    setB(null);
    setEquity(null);
    setDrawdown(null);
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
        const [eq, dd] = await Promise.all([getSeries(backtestId, 'equity'), getSeries(backtestId, 'drawdown')]);
        if (ctl.signal.aborted) return;
        setEquity(eq);
        setDrawdown(dd);
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
        <Results b={b} summary={b.summary} equity={equity} drawdown={drawdown} curves={curves} catalog={catalog} onDownload={download} downloading={downloading} />
      )}
    </div>
  );
}
