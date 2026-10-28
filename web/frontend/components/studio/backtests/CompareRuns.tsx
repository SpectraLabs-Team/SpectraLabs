'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  ArrowDownRight,
  ArrowRight,
  ArrowUpRight,
  Award,
  Check,
  ChevronRight,
  Download,
  Eye,
  Layers,
  Percent,
  RefreshCw,
  Scale,
  SlidersHorizontal,
  TrendingDown,
  TrendingUp,
  X,
  Zap,
} from 'lucide-react';
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { chartColors, useTheme } from '@/lib/theme';
import {
  describeRequest,
  getBacktest,
  getSeries,
  nsToMs,
  type Backtest,
  type Catalog,
  type Series,
  type Summary,
} from '@/lib/backtests';
import { Badge, GlassCard, Spinner } from '@/components/ui/primitives';
import { useToast } from '@/components/ui/StudioToast';
import { cn, formatCurrency } from '@/lib/utils';
import { axisDate, cash, compactCash, fullDate, int, pct, ratio, signedPct, tone } from './format';

const PALETTE = [
  { stroke: '#06B6D4', bg: 'bg-cyan-500/10', text: 'text-cyan-400', border: 'border-cyan-500/30' },
  { stroke: '#8B5CF6', bg: 'bg-violet-500/10', text: 'text-violet-400', border: 'border-violet-500/30' },
  { stroke: '#10B981', bg: 'bg-emerald-500/10', text: 'text-emerald-400', border: 'border-emerald-500/30' },
  { stroke: '#F59E0B', bg: 'bg-amber-500/10', text: 'text-amber-400', border: 'border-amber-500/30' },
];

interface LoadedRun {
  backtest: Backtest;
  series: Series | null;
  error?: string;
}

export interface CompareRunsProps {
  runIds: string[];
  catalog: Catalog | null;
  onSelectRun: (id: string) => void;
  onRemoveRun: (id: string) => void;
  onClose: () => void;
}

const tick = { fill: '#64748B', fontSize: 11 };

export function CompareRuns({ runIds, catalog, onSelectRun, onRemoveRun, onClose }: CompareRunsProps) {
  const { theme } = useTheme();
  const cc = chartColors(theme);
  const { success, error: toastError } = useToast();

  const [loading, setLoading] = useState(true);
  const [runs, setRuns] = useState<LoadedRun[]>([]);
  const [chartMode, setChartMode] = useState<'normalized' | 'absolute'>('normalized');

  // Load all selected runs and their equity series in parallel
  const loadRuns = useCallback(async () => {
    setLoading(true);
    try {
      const results = await Promise.all(
        runIds.map(async (id) => {
          try {
            const [b, s] = await Promise.all([
              getBacktest(id),
              getSeries(id, 'equity').catch(() => null),
            ]);
            return { backtest: b, series: s };
          } catch (err) {
            return {
              backtest: {
                id,
                status: 'failed',
                progress: 0,
                cancel_requested: false,
                error: err instanceof Error ? err.message : 'Failed to load run',
                request: {},
                run_id: null,
                fingerprints: null,
                summary: null,
                artifacts: null,
                created_at: new Date().toISOString(),
                started_at: null,
                finished_at: null,
              } as Backtest,
              series: null,
              error: err instanceof Error ? err.message : 'Error loading',
            };
          }
        })
      );
      setRuns(results);
    } catch {
      toastError('Comparison Error', 'Could not load all backtests for comparison.');
    } finally {
      setLoading(false);
    }
  }, [runIds, toastError]);

  useEffect(() => {
    loadRuns();
  }, [loadRuns]);

  // Valid runs with summaries
  const validRuns = useMemo(
    () => runs.filter((r) => r.backtest.summary !== null),
    [runs]
  );

  // Consolidated Chart Data: Align all series by timestamp
  const { chartRows, spanMs } = useMemo(() => {
    if (!validRuns.length) return { chartRows: [], spanMs: 0 };

    // Map: ms timestamp -> object with values for each run
    const timeMap = new Map<number, Record<string, number>>();

    validRuns.forEach((r, idx) => {
      const s = r.series;
      if (!s || !s.points || !s.points.length) return;

      const firstPoint = s.points.find((p) => p[1] !== null);
      const firstVal = firstPoint && Number.isFinite(firstPoint[1]) ? (firstPoint[1] as number) : 100_000;

      s.points.forEach(([ts, val]) => {
        if (val === null || !Number.isFinite(val)) return;
        const ms = nsToMs(ts);
        const existing = timeMap.get(ms) ?? { t: ms };

        if (chartMode === 'normalized') {
          // Return percentage relative to start: ((v - first) / first) * 100
          existing[`r_${r.backtest.id}`] = firstVal > 0 ? ((val - firstVal) / firstVal) * 100 : 0;
        } else {
          existing[`r_${r.backtest.id}`] = val;
        }

        timeMap.set(ms, existing);
      });
    });

    const sortedRows = Array.from(timeMap.values()).sort((a, b) => a.t - b.t);
    const s = sortedRows.length > 1 ? sortedRows[sortedRows.length - 1].t - sortedRows[0].t : 0;
    return { chartRows: sortedRows, spanMs: s };
  }, [validRuns, chartMode]);

  // Scorecard: Find best performers
  const champions = useMemo(() => {
    if (validRuns.length < 2) return null;

    let bestReturn = validRuns[0];
    let bestSharpe = validRuns[0];
    let lowestDrawdown = validRuns[0];
    let bestWinRate = validRuns[0];

    validRuns.forEach((r) => {
      const sum = r.backtest.summary!;
      if ((sum.metrics.total_return ?? -Infinity) > (bestReturn.backtest.summary!.metrics.total_return ?? -Infinity)) {
        bestReturn = r;
      }
      if ((sum.metrics.sharpe ?? -Infinity) > (bestSharpe.backtest.summary!.metrics.sharpe ?? -Infinity)) {
        bestSharpe = r;
      }
      if ((sum.metrics.max_drawdown ?? Infinity) < (lowestDrawdown.backtest.summary!.metrics.max_drawdown ?? Infinity)) {
        lowestDrawdown = r;
      }
      if ((sum.trades.win_rate ?? -Infinity) > (bestWinRate.backtest.summary!.trades.win_rate ?? -Infinity)) {
        bestWinRate = r;
      }
    });

    return { bestReturn, bestSharpe, lowestDrawdown, bestWinRate };
  }, [validRuns]);

  // Parameter Diff Map: find which parameter keys exist and if values differ
  const paramDiffs = useMemo(() => {
    const keys = new Set<string>();
    validRuns.forEach((r) => {
      const p = r.backtest.summary?.run.params;
      if (p) Object.keys(p).forEach((k) => keys.add(k));
    });

    return Array.from(keys).map((key) => {
      const values = validRuns.map((r) => {
        const val = r.backtest.summary?.run.params?.[key];
        return val !== undefined ? String(val) : '—';
      });
      const isDiff = new Set(values).size > 1;
      return { key, values, isDiff };
    });
  }, [validRuns]);

  // Export comparison table to CSV
  const handleExportCsv = () => {
    if (!validRuns.length) return;

    const headers = ['Metric', ...validRuns.map((r) => describeRequest(r.backtest.request, catalog))];
    const metricRows = [
      ['Total Return', ...validRuns.map((r) => signedPct(r.backtest.summary?.metrics.total_return))],
      ['CAGR', ...validRuns.map((r) => signedPct(r.backtest.summary?.performance.cagr))],
      ['Sharpe Ratio', ...validRuns.map((r) => ratio(r.backtest.summary?.metrics.sharpe))],
      ['Sortino Ratio', ...validRuns.map((r) => ratio(r.backtest.summary?.performance.sortino))],
      ['Calmar Ratio', ...validRuns.map((r) => ratio(r.backtest.summary?.performance.calmar))],
      ['Max Drawdown', ...validRuns.map((r) => pct(r.backtest.summary?.metrics.max_drawdown))],
      ['Annualized Volatility', ...validRuns.map((r) => pct(r.backtest.summary?.metrics.annualized_volatility))],
      ['Win Rate', ...validRuns.map((r) => pct(r.backtest.summary?.trades.win_rate, 1))],
      ['Profit Factor', ...validRuns.map((r) => ratio(r.backtest.summary?.trades.profit_factor))],
      ['Total Trades', ...validRuns.map((r) => int(r.backtest.summary?.trades.count))],
      ['Fills Count', ...validRuns.map((r) => int(r.backtest.summary?.activity.fill_count))],
      ['Total Fees ($)', ...validRuns.map((r) => cash(r.backtest.summary?.activity.total_fees))],
      ['Final Equity ($)', ...validRuns.map((r) => cash(r.backtest.summary?.activity.final_equity))],
    ];

    const csvContent = [headers.join(','), ...metricRows.map((row) => row.join(','))].join('\n');
    const blob = new Blob([csvContent], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `backtest-comparison-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    success('Comparison Exported', 'Downloaded backtest comparison CSV report.');
  };

  return (
    <div className="space-y-6">
      {/* Top Comparison Header */}
      <GlassCard as="section" className="p-5">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-accent/10 text-accent">
              <Scale className="h-5 w-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold text-ink">Multi-Backtest Comparison</h2>
                <Badge tone="accent">{validRuns.length} Runs</Badge>
              </div>
              <p className="mt-0.5 text-xs text-ink/50">
                Compare performance metrics, risk characteristics, and equity trajectories side-by-side.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={handleExportCsv}
              className="btn-secondary px-3 py-1.5 text-xs"
              title="Export comparison metrics as CSV"
            >
              <Download className="h-3.5 w-3.5" /> Export CSV
            </button>
            <button
              type="button"
              onClick={loadRuns}
              disabled={loading}
              className="btn-secondary px-3 py-1.5 text-xs"
              title="Refresh runs"
            >
              <RefreshCw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} /> Refresh
            </button>
            <button
              type="button"
              onClick={onClose}
              className="btn-ghost px-3 py-1.5 text-xs text-ink/60 hover:text-ink"
            >
              <X className="h-4 w-4" /> Close
            </button>
          </div>
        </div>

        {/* Selected Run Pills */}
        <div className="mt-4 flex flex-wrap gap-2 pt-3 border-t border-ink/[0.06]">
          {runs.map((r, i) => {
            const pal = PALETTE[i % PALETTE.length];
            const sum = r.backtest.summary;
            return (
              <div
                key={r.backtest.id}
                className={cn(
                  'flex items-center gap-2 rounded-xl border px-3 py-1.5 text-xs transition',
                  pal.border,
                  pal.bg
                )}
              >
                <span className="h-2.5 w-2.5 rounded-full" style={{ backgroundColor: pal.stroke }} />
                <span className={cn('font-semibold', pal.text)}>
                  #{i + 1} {describeRequest(r.backtest.request, catalog)}
                </span>
                {sum && (
                  <span className={cn('font-mono font-medium', tone(sum.metrics.total_return) === 'profit' ? 'text-profit' : 'text-loss')}>
                    {signedPct(sum.metrics.total_return)}
                  </span>
                )}
                <div className="flex items-center gap-1 ml-1">
                  <button
                    type="button"
                    onClick={() => onSelectRun(r.backtest.id)}
                    className="rounded p-0.5 text-ink/40 hover:bg-ink/10 hover:text-ink"
                    title="Focus single run"
                  >
                    <Eye className="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    onClick={() => onRemoveRun(r.backtest.id)}
                    className="rounded p-0.5 text-ink/40 hover:bg-ink/10 hover:text-loss"
                    title="Remove from comparison"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </GlassCard>

      {/* Champion Highlights (When comparing >= 2 valid runs) */}
      {champions && (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {/* Best Return */}
          <GlassCard className="p-4 border border-profit/20 bg-profit/[0.02]">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-profit">
                Top Return
              </span>
              <Award className="h-4 w-4 text-profit" />
            </div>
            <p className="mt-2 text-xl font-bold font-mono text-profit">
              {signedPct(champions.bestReturn.backtest.summary?.metrics.total_return)}
            </p>
            <p className="mt-1 truncate text-xs text-ink/60 font-medium">
              {describeRequest(champions.bestReturn.backtest.request, catalog)}
            </p>
          </GlassCard>

          {/* Best Sharpe */}
          <GlassCard className="p-4 border border-accent/20 bg-accent/[0.02]">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-accent">
                Best Sharpe Ratio
              </span>
              <Zap className="h-4 w-4 text-accent" />
            </div>
            <p className="mt-2 text-xl font-bold font-mono text-accent">
              {ratio(champions.bestSharpe.backtest.summary?.metrics.sharpe)}
            </p>
            <p className="mt-1 truncate text-xs text-ink/60 font-medium">
              {describeRequest(champions.bestSharpe.backtest.request, catalog)}
            </p>
          </GlassCard>

          {/* Lowest Max Drawdown */}
          <GlassCard className="p-4 border border-cyan-500/20 bg-cyan-500/[0.02]">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-cyan-400">
                Lowest Max DD
              </span>
              <TrendingDown className="h-4 w-4 text-cyan-400" />
            </div>
            <p className="mt-2 text-xl font-bold font-mono text-cyan-400">
              {pct(champions.lowestDrawdown.backtest.summary?.metrics.max_drawdown)}
            </p>
            <p className="mt-1 truncate text-xs text-ink/60 font-medium">
              {describeRequest(champions.lowestDrawdown.backtest.request, catalog)}
            </p>
          </GlassCard>

          {/* Highest Win Rate */}
          <GlassCard className="p-4 border border-violet-500/20 bg-violet-500/[0.02]">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-violet-400">
                Highest Win Rate
              </span>
              <TrendingUp className="h-4 w-4 text-violet-400" />
            </div>
            <p className="mt-2 text-xl font-bold font-mono text-violet-400">
              {pct(champions.bestWinRate.backtest.summary?.trades.win_rate, 1)}
            </p>
            <p className="mt-1 truncate text-xs text-ink/60 font-medium">
              {describeRequest(champions.bestWinRate.backtest.request, catalog)}
            </p>
          </GlassCard>
        </div>
      )}

      {/* Multi-Equity Chart */}
      <GlassCard as="section" className="p-5">
        <div className="flex flex-wrap items-center justify-between gap-3 mb-4">
          <div>
            <h3 className="text-sm font-semibold text-ink">Comparative Equity Curves</h3>
            <p className="text-xs text-ink/50">
              {chartMode === 'normalized'
                ? 'Normalized percentage return (%) starting from zero base.'
                : 'Absolute account equity value ($ USD) over historical period.'}
            </p>
          </div>

          <div className="inline-flex rounded-lg border border-ink/10 bg-canvas/70 p-0.5 text-xs">
            <button
              type="button"
              onClick={() => setChartMode('normalized')}
              className={cn(
                'rounded-md px-2.5 py-1 font-medium transition',
                chartMode === 'normalized' ? 'bg-accent/15 text-accent font-semibold' : 'text-ink/60 hover:text-ink'
              )}
            >
              Normalized Return %
            </button>
            <button
              type="button"
              onClick={() => setChartMode('absolute')}
              className={cn(
                'rounded-md px-2.5 py-1 font-medium transition',
                chartMode === 'absolute' ? 'bg-accent/15 text-accent font-semibold' : 'text-ink/60 hover:text-ink'
              )}
            >
              Absolute Equity $
            </button>
          </div>
        </div>

        {loading ? (
          <div className="flex h-[320px] items-center justify-center">
            <Spinner />
          </div>
        ) : chartRows.length === 0 ? (
          <div className="flex h-[260px] items-center justify-center text-xs text-ink/40">
            No equity series available for the selected backtests.
          </div>
        ) : (
          <div style={{ height: 320 }}>
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={chartRows} margin={{ top: 8, right: 12, bottom: 0, left: 0 }}>
                <CartesianGrid stroke={cc.grid} vertical={false} />
                <XAxis
                  dataKey="t"
                  type="number"
                  scale="time"
                  domain={['dataMin', 'dataMax']}
                  tickFormatter={(t: number) => axisDate(t, spanMs)}
                  tick={tick}
                  tickLine={false}
                  axisLine={{ stroke: cc.axis }}
                  minTickGap={48}
                />
                <YAxis
                  domain={['auto', 'auto']}
                  tickFormatter={(v: number) =>
                    chartMode === 'normalized' ? `${v >= 0 ? '+' : ''}${v.toFixed(0)}%` : compactCash(v)
                  }
                  tick={tick}
                  tickLine={false}
                  axisLine={false}
                  width={56}
                />
                <Tooltip
                  contentStyle={cc.tooltip}
                  labelFormatter={(t) => fullDate(Number(t))}
                  formatter={(val, name) => {
                    const v = Number(val);
                    return [
                      chartMode === 'normalized'
                        ? `${v >= 0 ? '+' : ''}${v.toFixed(2)}%`
                        : formatCurrency(v),
                      String(name),
                    ];
                  }}
                />
                <Legend iconType="circle" wrapperStyle={{ fontSize: 11, color: cc.tick }} />
                {chartMode === 'normalized' && <ReferenceLine y={0} stroke={cc.muted} strokeDasharray="4 4" />}
                {validRuns.map((r, i) => {
                  const pal = PALETTE[i % PALETTE.length];
                  return (
                    <Line
                      key={r.backtest.id}
                      type="monotone"
                      dataKey={`r_${r.backtest.id}`}
                      name={`#${i + 1} ${describeRequest(r.backtest.request, catalog)}`}
                      stroke={pal.stroke}
                      strokeWidth={1.8}
                      dot={false}
                      isAnimationActive={false}
                      connectNulls
                    />
                  );
                })}
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}
      </GlassCard>

      {/* Side-by-Side Metric Table */}
      <GlassCard as="section" className="overflow-hidden">
        <div className="border-b border-ink/[0.06] px-5 py-4">
          <h3 className="text-sm font-semibold text-ink">Institutional Performance Comparison</h3>
          <p className="mt-0.5 text-xs text-ink/50">
            Side-by-side scorecard across return, risk, trade execution, and portfolio statistics.
          </p>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left font-mono text-xs">
            <thead>
              <tr className="border-b border-ink/10 bg-ink/[0.02] text-[11px] text-ink/50">
                <th className="py-3 px-4 font-semibold uppercase tracking-wider">Metric</th>
                {validRuns.map((r, i) => (
                  <th key={r.backtest.id} className="py-3 px-4 text-right">
                    <span
                      className="inline-block rounded px-2 py-0.5 text-xs font-semibold"
                      style={{ color: PALETTE[i % PALETTE.length].stroke }}
                    >
                      #{i + 1} {describeRequest(r.backtest.request, catalog)}
                    </span>
                  </th>
                ))}
                {validRuns.length === 2 && (
                  <th className="py-3 px-4 text-right text-accent font-semibold">
                    Delta (#1 vs #2)
                  </th>
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-ink/[0.04]">
              {/* Returns Section */}
              <tr className="bg-ink/[0.015]">
                <td
                  colSpan={validRuns.length + (validRuns.length === 2 ? 2 : 1)}
                  className="py-1.5 px-4 text-[10px] font-bold uppercase tracking-wider text-ink/40"
                >
                  Return & Growth
                </td>
              </tr>
              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Total Return</td>
                {validRuns.map((r) => {
                  const val = r.backtest.summary?.metrics.total_return;
                  return (
                    <td
                      key={r.backtest.id}
                      className={cn(
                        'py-2.5 px-4 text-right font-semibold',
                        tone(val) === 'profit' ? 'text-profit' : 'text-loss'
                      )}
                    >
                      {signedPct(val)}
                    </td>
                  );
                })}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right font-semibold text-ink">
                    {signedPct(
                      (validRuns[0].backtest.summary?.metrics.total_return ?? 0) -
                        (validRuns[1].backtest.summary?.metrics.total_return ?? 0)
                    )}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">CAGR</td>
                {validRuns.map((r) => {
                  const val = r.backtest.summary?.performance.cagr;
                  return (
                    <td
                      key={r.backtest.id}
                      className={cn(
                        'py-2.5 px-4 text-right font-semibold',
                        tone(val) === 'profit' ? 'text-profit' : 'text-loss'
                      )}
                    >
                      {signedPct(val)}
                    </td>
                  );
                })}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right font-semibold text-ink">
                    {signedPct(
                      (validRuns[0].backtest.summary?.performance.cagr ?? 0) -
                        (validRuns[1].backtest.summary?.performance.cagr ?? 0)
                    )}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Final Equity</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {cash(r.backtest.summary?.activity.final_equity)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {cash(
                      Number(validRuns[0].backtest.summary?.activity.final_equity ?? 0) -
                        Number(validRuns[1].backtest.summary?.activity.final_equity ?? 0)
                    )}
                  </td>
                )}
              </tr>

              {/* Risk-Adjusted Ratios */}
              <tr className="bg-ink/[0.015]">
                <td
                  colSpan={validRuns.length + (validRuns.length === 2 ? 2 : 1)}
                  className="py-1.5 px-4 text-[10px] font-bold uppercase tracking-wider text-ink/40"
                >
                  Risk-Adjusted Efficiency
                </td>
              </tr>
              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Sharpe Ratio</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right font-semibold text-ink">
                    {ratio(r.backtest.summary?.metrics.sharpe)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right font-semibold text-accent">
                    {(
                      (validRuns[0].backtest.summary?.metrics.sharpe ?? 0) -
                      (validRuns[1].backtest.summary?.metrics.sharpe ?? 0)
                    ).toFixed(2)}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Sortino Ratio</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {ratio(r.backtest.summary?.performance.sortino)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {(
                      (validRuns[0].backtest.summary?.performance.sortino ?? 0) -
                      (validRuns[1].backtest.summary?.performance.sortino ?? 0)
                    ).toFixed(2)}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Calmar Ratio</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {ratio(r.backtest.summary?.performance.calmar)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {(
                      (validRuns[0].backtest.summary?.performance.calmar ?? 0) -
                      (validRuns[1].backtest.summary?.performance.calmar ?? 0)
                    ).toFixed(2)}
                  </td>
                )}
              </tr>

              {/* Risk & Drawdowns */}
              <tr className="bg-ink/[0.015]">
                <td
                  colSpan={validRuns.length + (validRuns.length === 2 ? 2 : 1)}
                  className="py-1.5 px-4 text-[10px] font-bold uppercase tracking-wider text-ink/40"
                >
                  Risk & Tail Statistics
                </td>
              </tr>
              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Max Drawdown</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-loss font-semibold">
                    {pct(r.backtest.summary?.metrics.max_drawdown)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {signedPct(
                      (validRuns[0].backtest.summary?.metrics.max_drawdown ?? 0) -
                        (validRuns[1].backtest.summary?.metrics.max_drawdown ?? 0)
                    )}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Annualized Volatility</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {pct(r.backtest.summary?.metrics.annualized_volatility)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {signedPct(
                      (validRuns[0].backtest.summary?.metrics.annualized_volatility ?? 0) -
                        (validRuns[1].backtest.summary?.metrics.annualized_volatility ?? 0)
                    )}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Underwater Days (Max)</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {r.backtest.summary?.drawdown.longest_underwater_periods ?? '—'}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {(validRuns[0].backtest.summary?.drawdown.longest_underwater_periods ?? 0) -
                      (validRuns[1].backtest.summary?.drawdown.longest_underwater_periods ?? 0)}
                  </td>
                )}
              </tr>

              {/* Trade Execution */}
              <tr className="bg-ink/[0.015]">
                <td
                  colSpan={validRuns.length + (validRuns.length === 2 ? 2 : 1)}
                  className="py-1.5 px-4 text-[10px] font-bold uppercase tracking-wider text-ink/40"
                >
                  Trade Execution & Fees
                </td>
              </tr>
              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Win Rate</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink font-semibold">
                    {pct(r.backtest.summary?.trades.win_rate, 1)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {signedPct(
                      (validRuns[0].backtest.summary?.trades.win_rate ?? 0) -
                        (validRuns[1].backtest.summary?.trades.win_rate ?? 0),
                      1
                    )}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Profit Factor</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {ratio(r.backtest.summary?.trades.profit_factor)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {(
                      (validRuns[0].backtest.summary?.trades.profit_factor ?? 0) -
                      (validRuns[1].backtest.summary?.trades.profit_factor ?? 0)
                    ).toFixed(2)}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Total Trades</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    {int(r.backtest.summary?.trades.count)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    {(validRuns[0].backtest.summary?.trades.count ?? 0) -
                      (validRuns[1].backtest.summary?.trades.count ?? 0)}
                  </td>
                )}
              </tr>

              <tr>
                <td className="py-2.5 px-4 text-ink/70 font-sans font-medium">Total Fees Paid</td>
                {validRuns.map((r) => (
                  <td key={r.backtest.id} className="py-2.5 px-4 text-right text-ink">
                    ${cash(r.backtest.summary?.activity.total_fees)}
                  </td>
                ))}
                {validRuns.length === 2 && (
                  <td className="py-2.5 px-4 text-right text-ink/60">
                    $
                    {cash(
                      Number(validRuns[0].backtest.summary?.activity.total_fees ?? 0) -
                        Number(validRuns[1].backtest.summary?.activity.total_fees ?? 0)
                    )}
                  </td>
                )}
              </tr>
            </tbody>
          </table>
        </div>
      </GlassCard>

      {/* Strategy Parameters Diff Table */}
      {paramDiffs.length > 0 && (
        <GlassCard as="section" className="overflow-hidden">
          <div className="border-b border-ink/[0.06] px-5 py-4 flex items-center justify-between">
            <div>
              <h3 className="text-sm font-semibold text-ink flex items-center gap-2">
                <SlidersHorizontal className="h-4 w-4 text-accent" /> Strategy Parameters Matrix
              </h3>
              <p className="mt-0.5 text-xs text-ink/50">
                Input variables across configurations. Varied parameters are highlighted with a Diff tag.
              </p>
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="w-full text-left font-mono text-xs">
              <thead>
                <tr className="border-b border-ink/10 bg-ink/[0.02] text-[11px] text-ink/50">
                  <th className="py-3 px-4 font-semibold uppercase tracking-wider">Parameter</th>
                  {validRuns.map((r, i) => (
                    <th key={r.backtest.id} className="py-3 px-4 text-right">
                      #{i + 1}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-ink/[0.04]">
                {paramDiffs.map(({ key, values, isDiff }) => (
                  <tr key={key} className={cn(isDiff ? 'bg-amber-500/[0.03]' : 'hover:bg-ink/[0.01]')}>
                    <td className="py-2.5 px-4 font-medium flex items-center gap-2 text-ink">
                      <span>{key}</span>
                      {isDiff && (
                        <span className="rounded bg-amber-500/15 px-1.5 py-0.2 text-[10px] font-semibold text-amber-500">
                          Diff
                        </span>
                      )}
                    </td>
                    {values.map((v, idx) => (
                      <td
                        key={idx}
                        className={cn(
                          'py-2.5 px-4 text-right font-semibold',
                          isDiff ? 'text-amber-400' : 'text-ink/80'
                        )}
                      >
                        {v}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </GlassCard>
      )}

      {/* Drilldown Section: Focus Individual Runs */}
      <GlassCard as="section" className="p-5">
        <h3 className="text-sm font-semibold text-ink mb-3">Individual Run Actions</h3>
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {validRuns.map((r, i) => (
            <div
              key={r.backtest.id}
              className="flex flex-col justify-between rounded-xl border border-ink/10 bg-ink/[0.02] p-3.5"
            >
              <div>
                <div className="flex items-center gap-2">
                  <span
                    className="h-2 w-2 rounded-full"
                    style={{ backgroundColor: PALETTE[i % PALETTE.length].stroke }}
                  />
                  <span className="font-semibold text-xs text-ink">
                    #{i + 1} Run
                  </span>
                </div>
                <p className="mt-1 text-xs text-ink/60 font-medium truncate">
                  {describeRequest(r.backtest.request, catalog)}
                </p>
                <p className="mt-0.5 text-[11px] font-mono text-ink/40">
                  ID: {r.backtest.id.slice(0, 8)}
                </p>
              </div>

              <button
                type="button"
                onClick={() => onSelectRun(r.backtest.id)}
                className="mt-3 btn-secondary w-full justify-center text-xs py-1.5"
              >
                Inspect Run Details <ChevronRight className="h-3 w-3 ml-1" />
              </button>
            </div>
          ))}
        </div>
      </GlassCard>
    </div>
  );
}
