'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Activity, Cpu, Download, Info, Play, ShieldAlert, Sigma, SlidersHorizontal, Sparkles, TrendingDown, Wallet } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { analyticVaR } from '@/lib/model';
import type { BackendRequest } from '@/lib/types';
import { parseCsv, portfolioStats } from '@/lib/portfolio';
import { cn, confidenceLabel, formatCompact, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { AnimatedValue, Badge, ErrorNote, GlassCard, RangeSlider, Spinner } from '@/components/ui/primitives';
import { LossHistogram } from './LossHistogram';
import { usePortfolio, type RiskRun } from './PortfolioContext';
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
/**
 * The Risk Lab always runs the Parallel backend (all cores). It's the fastest choice for every
 * run size, and its results depend only on the seed and path count, never on the machine's
 * thread count. CPU vs Parallel comparisons live in the Hardware Benchmarks tab.
 */
const BACKEND: BackendRequest = 'parallel';


/**
 * Audit report of the last run, as JSON. Every figure comes from the run itself (its own book
 * snapshot and engine outputs), never from inputs edited afterwards; `matchesCurrentInputs`
 * says whether the screen still shows the same parameters.
 */
function exportRiskReport(run: RiskRun, matchesCurrentInputs: boolean) {
  const positions = parseCsv(run.csv).positions;
  const book = portfolioStats(positions);
  const stamp = new Date(run.at);
  const report = {
    reportType: 'spectra-labs.risk-report',
    schemaVersion: 1,
    timestamp: stamp.toISOString(),
    matchesCurrentInputs,
    portfolioValue: run.var.portfolio_value,
    modelVolatility: run.var.model_volatility,
    confidence: run.var.confidence,
    horizonDays: Math.round(run.var.horizon_years * 252),
    simulations: run.var.simulations_run,
    valueAtRisk: run.var.value,
    conditionalVaR: run.cvar.value,
    pnlStdDev: run.vol.value,
    hhiConcentration: book.hhi,
    effectiveNumberOfPositions: book.effectiveN,
    engineBackend: run.var.backend,
    requestedBackend: run.var.requested_backend,
    computeDurationMs: run.var.compute_ms,
    wallDurationMs: run.var.total_ms,
    engineVersion: run.var.library_version,
    engineCommitSha: run.var.git_commit,
    randomSeed: run.var.seed,
    model: run.var.model,
    positions,
  };
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `spectra-risk-report-${stamp.toISOString().replace(/[:.]/g, '-')}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
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
  const { csv, revision, engine, riskParams, setRiskParams, riskRun: results, setRiskRun: setResults } = usePortfolio();
  const online = engine.status === 'online';
  // Inputs and the last run live in PortfolioContext, so they survive switching Studio tabs.
  // `auto` (Live mode) is off by default: the engine only runs on an explicit click.
  const { confidence, simulations, horizon, auto, dependence = 'aggregate' } = riskParams;
  const backend = BACKEND;
  const setConfidence = (confidence: number) => setRiskParams({ confidence });
  const setSimulations = (simulations: number) => setRiskParams({ simulations });
  const setHorizon = (horizon: number) => setRiskParams({ horizon });
  const setDependence = (dep: 'aggregate' | 'gaussian_copula' | 't_copula') => setRiskParams({ dependence: dep });
  const setAuto = (f: (a: boolean) => boolean) => setRiskParams((p) => ({ auto: f(p.auto) }));
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const reqId = useRef(0);

  const run = useCallback(async () => {
    const id = ++reqId.current;
    setLoading(true);
    setError('');
    try {
      const base = {
        portfolio_csv: csv,
        confidence,
        simulations,
        horizon_days: horizon,
        backend,
        dependence: dependence !== 'aggregate' ? dependence : undefined,
      };
      const [v, c, vol] = await Promise.all([
        calculateRisk({ ...base, metric: 'var' }),
        calculateRisk({ ...base, metric: 'cvar' }),
        calculateRisk({ ...base, metric: 'volatility' }),
      ]);
      if (id === reqId.current) setResults({ var: v, cvar: c, vol, rev: revision, simulations, backend, at: Date.now(), csv });
    } catch (e) {
      if (id === reqId.current) setError(e instanceof Error ? e.message : 'Risk calculation failed');
    } finally {
      if (id === reqId.current) setLoading(false);
    }
  }, [csv, confidence, simulations, horizon, backend, dependence, revision, setResults]);

  // Live mode (opt-in): re-run (debounced) whenever an input or the portfolio changes.
  // Nothing runs on mount; the first run is always an explicit click or switching Live on.
  useEffect(() => {
    if (!online || !auto) return;
    const t = setTimeout(run, simulations >= 500_000 ? 450 : 250);
    return () => clearTimeout(t);
  }, [online, auto, run, simulations, revision]);

  // A run is "fresh" only if every input still matches: book, confidence, horizon, paths and backend.
  const fresh =
    !!results &&
    results.rev === revision &&
    results.var.confidence === confidence &&
    Math.round(results.var.horizon_years * 252) === horizon &&
    results.simulations === simulations &&
    results.backend === backend;
  const dirty = !!results && !fresh;

  // Everything below describes the LAST RUN, using that run's own inputs. Nothing here moves
  // when a slider moves: numbers change only when "Run simulation" is pressed (or Live is on).
  const ran = results
    ? {
        confidence: results.var.confidence,
        horizon: Math.round(results.var.horizon_years * 252),
        model: { value: results.var.portfolio_value, volatility: results.var.model_volatility, horizonDays: Math.round(results.var.horizon_years * 252) },
      }
    : null;
  const runGross = useMemo(() => (results ? portfolioStats(parseCsv(results.csv).positions).grossExposure : 0), [results]);
  // Closed-form value of the same model and inputs: a sanity check on the Monte Carlo estimate.
  const mcErr = results && ran ? results.var.value / analyticVaR(ran.model, ran.confidence) - 1 : null;
  const label = ran ? confidenceLabel(ran.confidence) : confidenceLabel(confidence);
  const days = ran ? ran.horizon : horizon;

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
            <label className="mb-1.5 block text-xs font-medium text-ink/60">Cross-Asset Dependence</label>
            <div className="grid grid-cols-3 gap-1 rounded-lg border border-ink/[0.08] bg-ink/[0.03] p-1 text-[11px]">
              {(['aggregate', 'gaussian_copula', 't_copula'] as const).map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => setDependence(d)}
                  className={cn(
                    'rounded px-2 py-1 text-center font-mono capitalize transition',
                    dependence === d ? 'bg-accent/15 font-semibold text-accent shadow-sm' : 'text-ink/60 hover:text-ink'
                  )}
                >
                  {d === 'gaussian_copula' ? 'Gaussian' : d === 't_copula' ? 't-Copula' : 'Aggregate'}
                </button>
              ))}
            </div>
          </div>

          <p className="flex items-start gap-2 rounded-lg border border-ink/[0.07] bg-ink/[0.02] px-3 py-2 text-[11px] leading-4 text-ink/50">
            <Cpu className="mt-px h-3.5 w-3.5 shrink-0 text-accent" />
            <span>
              Runs on all CPU cores. Results depend only on the seed and path count, so a run is reproducible on any machine.
            </span>
          </p>

          <div className="flex gap-2">
            <button type="button" onClick={run} disabled={!online || loading} className="btn-primary flex-1">
              {loading ? <Spinner /> : <Play className="h-4 w-4" />} {loading ? 'Simulating' : 'Run simulation'}
            </button>
          </div>
          {dirty && !loading && (
            <p role="status" className="-mt-3 flex items-center gap-2 rounded-lg border border-caution/30 bg-caution/[0.07] px-3 py-2 text-[11.5px] font-medium text-amber-800 dark:text-amber-200">
              <SlidersHorizontal className="h-3.5 w-3.5 shrink-0" />
              Parameters modified · Re-run simulation to update
            </p>
          )}
          {results && (
            <button type="button" onClick={() => exportRiskReport(results, fresh)} className="btn-ghost -mt-2 w-full justify-center border border-ink/10 text-xs">
              <Download className="h-3.5 w-3.5" /> Export Risk Report
            </button>
          )}
          {error && <ErrorNote>{error}</ErrorNote>}
        </div>
      </GlassCard>

      <div className="min-w-0 space-y-5">
        {/* Metric cards */}
        <div className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-4">
          <MetricCard icon={Wallet} label="Portfolio value" value={results ? formatCurrency(results.var.portfolio_value) : '—'} sub={results ? `gross ${formatCurrency(runGross)}` : 'Run a simulation'} loading={loading} />
          <MetricCard icon={ShieldAlert} tone="loss" label={`Value at Risk · ${label}`} value={results ? formatCurrency(results.var.value) : '—'} sub={results ? `${formatPercent(results.var.value / results.var.portfolio_value)} of book · ${days}d` : `${days}d horizon`} loading={loading} />
          <MetricCard icon={TrendingDown} tone="loss" label="Expected shortfall (CVaR)" value={results ? formatCurrency(results.cvar.value) : '—'} sub={results ? `${formatPercent(results.cvar.value / results.var.portfolio_value)} · mean of worst ${formatPercent(1 - ran!.confidence, ran!.confidence > 0.99 ? 1 : 0)}` : 'mean loss beyond VaR'} loading={loading} />
          <MetricCard icon={Sigma} tone="accent" label="Annualized volatility" value={results ? formatPercent(results.var.model_volatility) : '—'} sub={results ? `σ of ${days}d P&L ${formatCurrency(results.vol.value)}` : 'model volatility'} loading={loading} />
        </div>

        {/* Distribution */}
        <GlassCard as="section" className="p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold text-ink">Loss distribution · density histogram</h2>
              <p className="mt-0.5 text-xs text-ink/50">{days}-day horizon · the shaded tail is the loss beyond VaR</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {results && <Badge tone={fresh ? 'profit' : 'caution'}>Monte Carlo · {results.var.simulations_run.toLocaleString()} paths{fresh ? '' : ' · last run'}</Badge>}
              {mcErr !== null && (
                <span className="num text-[11px] text-ink/50" title="Monte Carlo VaR vs the exact closed-form value of the same model">
                  vs closed form {mcErr >= 0 ? '+' : ''}
                  {formatPercent(mcErr, 2)}
                </span>
              )}
            </div>
          </div>
          <div className="mt-4 h-[320px]">
            {results && ran ? (
              <LossHistogram input={ran.model} varValue={results.var.value} cvarValue={results.cvar.value} confidenceLabel={label} />
            ) : (
              <div className="relative flex h-full flex-col items-center justify-center overflow-hidden rounded-xl border border-dashed border-ink/[0.12] bg-ink/[0.015] px-6 text-center">
                <div aria-hidden className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_50%_40%,rgb(var(--accent)/0.10),transparent_65%)]" />
                {/* Faint silhouette of the histogram that will appear */}
                <div aria-hidden className="pointer-events-none absolute inset-x-10 bottom-6 flex h-24 items-end gap-1 opacity-[0.18]">
                  {[4, 7, 12, 19, 28, 39, 52, 66, 80, 92, 100, 96, 86, 72, 57, 43, 31, 21, 13, 8, 5, 3].map((h, k) => (
                    <div key={k} className={cn('flex-1 rounded-t', k >= 17 ? 'bg-loss' : 'bg-accent')} style={{ height: `${h}%` }} />
                  ))}
                </div>
                <span className="relative flex h-11 w-11 items-center justify-center rounded-xl border border-accent/30 bg-accent/10 text-accent shadow-glow">
                  <Sparkles className="h-5 w-5" />
                </span>
                <p className="relative mt-4 text-sm font-semibold text-ink">{loading ? 'Running simulation…' : 'Ready to simulate'}</p>
                <p className="relative mt-1 max-w-sm text-[12.5px] leading-5 text-ink/55">
                  Configure parameters on the left and click <span className="font-semibold text-ink/80">&lsquo;Run simulation&rsquo;</span>.
                  {!online && ' The Rust engine must be running first.'}
                </p>
              </div>
            )}
          </div>
          <p className="mt-3 flex items-start gap-2 text-[11px] leading-5 text-ink/40">
            <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            Bars are exact bin probabilities of the engine&apos;s model: one aggregate GBM position, zero drift, σ = average asset volatility. The markers are the engine&apos;s Monte Carlo estimates.
          </p>
        </GlassCard>

        {/* M7: Euler Risk Attribution */}
        {results?.var.position_contributions && results.var.position_contributions.length > 0 && (
          <GlassCard as="section" className="p-5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <h2 className="text-sm font-semibold text-ink">Euler Component Risk Attribution</h2>
                <p className="mt-0.5 text-xs text-ink/50">
                  Marginal risk contribution per position (Euler allocation property: sum of component CVaR equals portfolio CVaR)
                </p>
              </div>
              <Badge tone="accent">{results.var.dependence_model ?? 'Correlated Copula'}</Badge>
            </div>
            <div className="mt-4 overflow-x-auto">
              <table className="w-full text-left font-mono text-[11.5px]">
                <thead>
                  <tr className="border-b border-ink/10 text-[10.5px] uppercase text-ink/50">
                    <th className="pb-2">Asset</th>
                    <th className="pb-2">Position Value</th>
                    <th className="pb-2">Weight</th>
                    <th className="pb-2">Component VaR</th>
                    <th className="pb-2">Component CVaR</th>
                    <th className="pb-2 text-right">CVaR Contribution</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-ink/[0.06]">
                  {results.var.position_contributions.map((pos) => (
                    <tr key={pos.symbol} className="hover:bg-ink/[0.02]">
                      <td className="py-2 font-semibold text-ink">{pos.symbol}</td>
                      <td className="py-2 text-ink/75">{formatCurrency(pos.position_value)}</td>
                      <td className="py-2 text-ink/60">{formatPercent(pos.weight, 1)}</td>
                      <td className="py-2 text-loss">{formatCurrency(pos.component_var)}</td>
                      <td className="py-2 text-loss">{formatCurrency(pos.component_cvar)}</td>
                      <td className="py-2 text-right font-semibold text-accent">{formatPercent(pos.cvar_contribution_pct / 100, 1)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </GlassCard>
        )}

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
              <p className="mt-4 text-sm text-ink/50">{online ? 'No run yet. Click “Run simulation” to record backend, timings, seed and engine commit.' : 'Engine results and their reproducibility metadata show here once the API is running.'}</p>
            )}
          </GlassCard>

        </div>
      </div>

    </div>
  );
}
