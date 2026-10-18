'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Activity, Info, Play, ShieldAlert, Sigma, TrendingDown, Wallet } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { analyticCVaR, analyticLossStd, analyticVaR } from '@/lib/model';
import type { BackendRequest, RiskResult } from '@/lib/types';
import { cn, confidenceLabel, formatCompact, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { AnimatedValue, Badge, ErrorNote, GlassCard, RangeSlider, Segmented, Spinner } from '@/components/ui/primitives';
import { LossHistogram } from './LossHistogram';
import { usePortfolio } from './PortfolioContext';
import { StressPanel } from './StressPanel';

const SIMS = [10_000, 25_000, 50_000, 100_000, 250_000, 500_000, 1_000_000];
const HORIZON_PRESETS = [
  { value: 1, label: '1D' },
  { value: 5, label: '1W' },
  { value: 10, label: '10D' },
  { value: 21, label: '1M' },
  { value: 63, label: '1Q' },
  { value: 252, label: '1Y' },
];
const BACKENDS: { value: BackendRequest; label: string }[] = [
  { value: 'auto', label: 'Auto' },
  { value: 'cpu', label: 'CPU' },
  { value: 'parallel', label: 'Parallel' },
];

interface Results {
  var: RiskResult;
  cvar: RiskResult;
  vol: RiskResult;
  /** Portfolio revision the run was made against. */
  rev: number;
}

function MetricCard({ icon: Icon, label, value, sub, tone = 'neutral', loading }: { icon: typeof Activity; label: string; value: string; sub: React.ReactNode; tone?: 'neutral' | 'loss' | 'accent'; loading?: boolean }) {
  return (
    <GlassCard className="relative overflow-hidden p-4 sm:p-5">
      {loading && <div className="absolute inset-x-0 top-0 h-px animate-shimmer bg-[linear-gradient(90deg,transparent,#22D3EE,transparent)] bg-[length:200%_100%]" />}
      <div className="flex items-center justify-between">
        <span className="text-xs text-ink/60">{label}</span>
        <Icon className={cn('h-4 w-4', tone === 'loss' ? 'text-loss' : tone === 'accent' ? 'text-accent' : 'text-ink/50')} />
      </div>
      <p className={cn('num mt-3 text-2xl font-semibold tracking-tight sm:text-[26px]', tone === 'loss' ? 'text-loss' : 'text-ink')}>
        <AnimatedValue value={value} />
      </p>
      <p className="num mt-1 text-[11.5px] text-ink/50">{sub}</p>
    </GlassCard>
  );
}

export function RiskLab() {
  const { csv, stats, revision, engine } = usePortfolio();
  const online = engine.status === 'online';
  const [confidence, setConfidence] = useState(0.99);
  const [simulations, setSimulations] = useState(100_000);
  const [horizon, setHorizon] = useState(1);
  const [backend, setBackend] = useState<BackendRequest>('auto');
  const [auto, setAuto] = useState(true);
  const [results, setResults] = useState<Results | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const reqId = useRef(0);

  const run = useCallback(async () => {
    const id = ++reqId.current;
    setLoading(true);
    setError('');
    try {
      const base = { portfolio_csv: csv, confidence, simulations, horizon_days: horizon, backend };
      const [v, c, vol] = await Promise.all([
        calculateRisk({ ...base, metric: 'var' }),
        calculateRisk({ ...base, metric: 'cvar' }),
        calculateRisk({ ...base, metric: 'volatility' }),
      ]);
      if (id === reqId.current) setResults({ var: v, cvar: c, vol, rev: revision });
    } catch (e) {
      if (id === reqId.current) setError(e instanceof Error ? e.message : 'Risk calculation failed');
    } finally {
      if (id === reqId.current) setLoading(false);
    }
  }, [csv, confidence, simulations, horizon, backend, revision]);

  // Live mode: re-run (debounced) whenever an input or the portfolio changes.
  useEffect(() => {
    if (!online || !auto) return;
    const t = setTimeout(run, simulations >= 500_000 ? 450 : 250);
    return () => clearTimeout(t);
  }, [online, auto, run, simulations, revision]);

  // Closed-form values: instant preview offline, and a sanity check next to the Monte Carlo result.
  const modelInput = useMemo(() => ({ value: stats.netValue, volatility: stats.modelVolatility, horizonDays: horizon }), [stats.netValue, stats.modelVolatility, horizon]);
  const cf = { var: analyticVaR(modelInput, confidence), cvar: analyticCVaR(modelInput, confidence), std: analyticLossStd(modelInput) };
  const fresh = results && results.rev === revision && results.var.confidence === confidence && Math.round(results.var.horizon_years * 252) === horizon;
  const shown = {
    value: fresh ? results!.var.portfolio_value : stats.netValue,
    var: fresh ? results!.var.value : cf.var,
    cvar: fresh ? results!.cvar.value : cf.cvar,
    std: fresh ? results!.vol.value : cf.std,
    vol: fresh ? results!.var.model_volatility : stats.modelVolatility,
  };
  const mcErr = fresh ? results!.var.value / cf.var - 1 : null;

  return (
    <div className="grid gap-5 xl:grid-cols-[340px_1fr]">
      {/* Controls */}
      <GlassCard as="section" className="h-fit p-5 xl:sticky xl:top-20">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-ink">Simulation</h2>
          <label className="flex cursor-pointer items-center gap-2 text-[11px] text-ink/60">
            Live
            <button type="button" role="switch" aria-checked={auto} onClick={() => setAuto((a) => !a)} className={cn('relative h-4 w-7 rounded-full transition-colors', auto ? 'bg-accent' : 'bg-ink/15')}>
              <span className={cn('absolute top-0.5 h-3 w-3 rounded-full bg-white transition-all', auto ? 'left-3.5' : 'left-0.5')} />
            </button>
          </label>
        </div>

        <div className="mt-5 space-y-6">
          <RangeSlider label="Confidence level" value={confidence} min={0.9} max={0.999} step={0.001} onChange={setConfidence} display={confidenceLabel(confidence)} hint={<><span>90%</span><span>99.9%</span></>} />

          <RangeSlider
            label="Simulations"
            value={SIMS.indexOf(simulations)}
            min={0}
            max={SIMS.length - 1}
            step={1}
            onChange={(i) => setSimulations(SIMS[i])}
            display={`${simulations.toLocaleString()} paths`}
            hint={
              <>
                <span>10k</span>
                <span>1M</span>
              </>
            }
          />

          <div>
            <RangeSlider label="Horizon" value={horizon} min={1} max={252} step={1} onChange={setHorizon} display={`${horizon} ${horizon === 1 ? 'day' : 'days'}`} />
            <div className="mt-2.5 flex flex-wrap gap-1">
              {HORIZON_PRESETS.map((h) => (
                <button key={h.value} type="button" onClick={() => setHorizon(h.value)} className={cn('rounded px-2 py-0.5 font-mono text-[10.5px] transition', horizon === h.value ? 'bg-ink/10 text-ink' : 'text-ink/50 hover:text-ink/90')}>
                  {h.label}
                </button>
              ))}
            </div>
          </div>

          <div>
            <p className="mb-2 text-xs font-medium text-ink/60">Execution backend</p>
            <Segmented label="backend" value={backend} onChange={setBackend} options={BACKENDS} className="w-full" />
            <p className="mt-2 text-[11px] leading-4 text-ink/40">Auto picks CPU for small runs and Parallel for large ones. Results never depend on thread count.</p>
          </div>

          <div className="flex gap-2">
            <button type="button" onClick={run} disabled={!online || loading} className="btn-primary flex-1">
              {loading ? <Spinner /> : <Play className="h-4 w-4" />} {loading ? 'Simulating' : 'Run simulation'}
            </button>
          </div>
          {error && <ErrorNote>{error}</ErrorNote>}
        </div>
      </GlassCard>

      <div className="min-w-0 space-y-5">
        {/* Metric cards */}
        <div className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-4">
          <MetricCard icon={Wallet} label="Portfolio value" value={formatCurrency(shown.value)} sub={`gross ${formatCurrency(stats.grossExposure)}`} loading={loading} />
          <MetricCard icon={ShieldAlert} tone="loss" label={`Value at Risk · ${confidenceLabel(confidence)}`} value={formatCurrency(shown.var)} sub={`${formatPercent(shown.var / shown.value)} of book · ${horizon}d`} loading={loading} />
          <MetricCard icon={TrendingDown} tone="loss" label="Expected shortfall (CVaR)" value={formatCurrency(shown.cvar)} sub={`${formatPercent(shown.cvar / shown.value)} · mean of worst ${formatPercent(1 - confidence, confidence > 0.99 ? 1 : 0)}`} loading={loading} />
          <MetricCard icon={Sigma} tone="accent" label="Annualized volatility" value={formatPercent(shown.vol)} sub={`σ of ${horizon}d P&L ${formatCurrency(shown.std)}`} loading={loading} />
        </div>

        {/* Distribution */}
        <GlassCard as="section" className="p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold text-ink">Loss distribution · density histogram</h2>
              <p className="mt-0.5 text-xs text-ink/50">{horizon}-day horizon · the shaded tail is the loss beyond VaR</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {fresh ? <Badge tone="profit">Monte Carlo · {results!.var.simulations_run.toLocaleString()} paths</Badge> : <Badge tone="accent">closed-form preview</Badge>}
              {mcErr !== null && (
                <span className="num text-[11px] text-ink/50" title="Monte Carlo VaR vs the exact closed-form value of the same model">
                  vs closed form {mcErr >= 0 ? '+' : ''}
                  {formatPercent(mcErr, 2)}
                </span>
              )}
            </div>
          </div>
          <div className="mt-4 h-[320px]">
            <LossHistogram input={modelInput} varValue={shown.var} cvarValue={shown.cvar} confidenceLabel={confidenceLabel(confidence)} />
          </div>
          <p className="mt-3 flex items-start gap-2 text-[11px] leading-5 text-ink/40">
            <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            Bars are exact bin probabilities of the engine&apos;s model: one aggregate GBM position, zero drift, σ = average asset volatility. The markers are the engine&apos;s Monte Carlo estimates.
          </p>
        </GlassCard>

        <StressPanel simulations={simulations} horizonDays={horizon} backend={backend} />

        {/* Run metadata */}
        <div>
          <GlassCard as="section" className="p-5">
            <h2 className="text-sm font-semibold text-ink">Run record</h2>
            {results ? (
              <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-3 font-mono text-[12px] sm:grid-cols-3">
                {[
                  ['backend', `${results.var.requested_backend} → ${results.var.backend}`],
                  ['compute', formatDuration(results.var.compute_ms)],
                  ['wall', formatDuration(results.var.total_ms)],
                  ['paths', results.var.simulations_run.toLocaleString()],
                  ['seed', String(results.var.seed)],
                  ['throughput', `${formatCompact((results.var.simulations_run * 1000) / results.var.compute_ms)}/s`],
                  ['engine', `v${results.var.library_version}`],
                  ['commit', results.var.git_commit.slice(0, 10)],
                  ['model', 'GBM aggregate'],
                ].map(([k, v]) => (
                  <div key={k}>
                    <dt className="text-[10px] uppercase tracking-wider text-ink/40">{k}</dt>
                    <dd className="mt-0.5 truncate text-ink/75">{v}</dd>
                  </div>
                ))}
              </dl>
            ) : (
              <p className="mt-4 text-sm text-ink/50">{online ? 'Running the first simulation…' : 'Engine results and their reproducibility metadata show here once the API is running.'}</p>
            )}
          </GlassCard>

        </div>
      </div>

    </div>
  );
}
