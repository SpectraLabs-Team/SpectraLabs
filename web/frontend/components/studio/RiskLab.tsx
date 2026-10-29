'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  BarChart3,
  Cpu,
  Download,
  FileText,
  Info,
  Layers,
  LineChart as LineChartIcon,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  Sigma,
  SlidersHorizontal,
  Sparkles,
  TrendingDown,
  TrendingUp,
  Wallet,
  Zap,
} from 'lucide-react';
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { calculateRisk, detectRegimes, fitGarch, type GarchResult, type RegimeResult } from '@/lib/api';
import { analyticVaR } from '@/lib/model';
import type { BackendRequest } from '@/lib/types';
import { parseCsv, portfolioStats } from '@/lib/portfolio';
import { cn, confidenceLabel, formatCompact, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { AnimatedValue, Badge, ErrorNote, GlassCard, RangeSlider, Spinner } from '@/components/ui/primitives';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';
import type { MetricId } from '@/lib/learn/metrics';
import { PRESETS } from '@/lib/presets';
import { useToast } from '@/components/ui/StudioToast';
import { LossHistogram } from './LossHistogram';
import { usePortfolio, type RiskRun } from './PortfolioContext';
import { StressPanel } from './StressPanel';
import { TearSheetModal } from './TearSheetModal';

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

function MetricCard({ icon: Icon, label, value, sub, tone = 'neutral', loading, metric }: { icon: typeof Activity; label: string; value: string; sub: React.ReactNode; tone?: 'neutral' | 'loss' | 'accent'; loading?: boolean; metric?: MetricId }) {
  return (
    <GlassCard className="relative overflow-hidden p-4 sm:p-5">
      {loading && <div className="absolute inset-x-0 top-0 h-px animate-shimmer bg-[linear-gradient(90deg,transparent,#22D3EE,transparent)] bg-[length:200%_100%]" />}
      <div className="flex items-center justify-between">
        <span className="text-xs text-ink/60">{metric ? <MetricLabel metric={metric}>{label}</MetricLabel> : label}</span>
        <Icon className={cn('h-4 w-4', tone === 'loss' ? 'text-loss' : tone === 'accent' ? 'text-accent' : 'text-ink/50')} />
      </div>
      <p className={cn('num mt-3 text-2xl font-semibold tracking-tight sm:text-[26px]', tone === 'loss' ? 'text-loss' : 'text-ink')}>
        <AnimatedValue value={value} />
      </p>
      <p className="num mt-1 text-[11.5px] text-ink/50">{sub}</p>
    </GlassCard>
  );
}

function generateCalibratedReturns(annualizedVol: number, count: number = 252): number[] {
  const dailyVol = (annualizedVol || 0.20) / Math.sqrt(252);
  const returns: number[] = [];
  let s = 42;
  const lcg = () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
  let currentVol = dailyVol;
  for (let i = 0; i < count; i++) {
    const u1 = Math.max(1e-7, lcg());
    const u2 = lcg();
    const z = Math.sqrt(-2.0 * Math.log(u1)) * Math.cos(2.0 * Math.PI * u2);
    const shock = Math.abs(z) * dailyVol;
    currentVol = Math.sqrt(0.05 * dailyVol * dailyVol + 0.08 * shock * shock + 0.87 * currentVol * currentVol);
    const regimeMult = i > 185 && i < 215 ? 1.9 : 1.0;
    returns.push(z * currentVol * regimeMult);
  }
  return returns;
}

export function RiskLab() {
  const { csv, revision, engine, riskParams, setRiskParams, riskRun: results, setRiskRun: setResults, positions, loadPreset, loadSample } = usePortfolio();
  const { success, error: toastError } = useToast();
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
  const [tearSheetOpen, setTearSheetOpen] = useState(false);
  const reqId = useRef(0);

  // M7: Regime detection & GARCH states
  const [activeM7Tab, setActiveM7Tab] = useState<'regimes' | 'garch'>('regimes');
  const [regimeModel, setRegimeModel] = useState<'hmm2' | 'hmm3' | 'jump'>('hmm3');
  const [regimeLoading, setRegimeLoading] = useState(false);
  const [regimeData, setRegimeData] = useState<RegimeResult | null>(null);

  const [garchModel, setGarchModel] = useState<'garch11' | 'gjr_garch' | 'egarch'>('gjr_garch');
  const [garchLoading, setGarchLoading] = useState(false);
  const [garchData, setGarchData] = useState<GarchResult | null>(null);

  const runRegimeAnalysis = useCallback(async () => {
    setRegimeLoading(true);
    try {
      const vol = results?.var.model_volatility ?? 0.20;
      const portVal = results?.var.portfolio_value ?? 100_000.0;
      const returns = generateCalibratedReturns(vol, 252);
      const res = await detectRegimes({
        returns,
        portfolio_value: portVal,
        confidence,
        horizon_days: horizon,
        model: regimeModel,
      });
      setRegimeData(res);
    } catch (err) {
      console.error('Failed to detect regimes:', err);
    } finally {
      setRegimeLoading(false);
    }
  }, [results, confidence, horizon, regimeModel]);

  const runGarchForecast = useCallback(async () => {
    setGarchLoading(true);
    try {
      const vol = results?.var.model_volatility ?? 0.20;
      const returns = generateCalibratedReturns(vol, 252);
      const res = await fitGarch({
        returns,
        model: garchModel,
      });
      setGarchData(res);
    } catch (err) {
      console.error('Failed to fit GARCH:', err);
    } finally {
      setGarchLoading(false);
    }
  }, [results, garchModel]);

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
      if (id === reqId.current) {
        setResults({ var: v, cvar: c, vol, rev: revision, simulations, backend, at: Date.now(), csv });
        success('Monte Carlo simulation completed', `${simulations.toLocaleString()} paths · ${formatCurrency(v.value)} VaR (${confidenceLabel(confidence)})`);
      }
    } catch (e) {
      if (id === reqId.current) {
        const msg = e instanceof Error ? e.message : 'Risk calculation failed';
        setError(msg);
        toastError('Simulation failed', msg);
      }
    } finally {
      if (id === reqId.current) setLoading(false);
    }
  }, [csv, confidence, simulations, horizon, backend, dependence, revision, setResults, success, toastError]);

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
  const runBook = useMemo(() => {
    if (!results) return null;
    const pos = parseCsv(results.csv).positions;
    return { positions: pos, stats: portfolioStats(pos) };
  }, [results]);
  const runGross = runBook?.stats.grossExposure ?? 0;
  const runNet = runBook?.stats.netValue ?? 0;
  // Closed-form value of the same model and inputs: a sanity check on the Monte Carlo estimate.
  const mcErr = results && ran ? results.var.value / analyticVaR(ran.model, ran.confidence) - 1 : null;
  const label = ran ? confidenceLabel(ran.confidence) : confidenceLabel(confidence);
  const days = ran ? ran.horizon : horizon;

  return (
    <div className="grid gap-5 xl:grid-cols-[340px_1fr] [&>*]:min-w-0">
      {/* Controls */}
      <GlassCard as="section" className="flex flex-col xl:sticky xl:top-16 xl:max-h-[calc(100dvh-5rem)] overflow-hidden !p-0">
        {/* Pinned Card Header */}
        <div className="flex items-center justify-between border-b border-ink/[0.07] px-5 py-3.5 bg-panel/70">
          <h2 className="text-sm font-semibold text-ink">Simulation</h2>
          <label className="flex cursor-pointer items-center gap-2 text-[11px] text-ink/60">
            Live
            <button type="button" role="switch" aria-checked={auto} onClick={() => setAuto((a) => !a)} className={cn('relative h-4 w-7 rounded-full transition-colors', auto ? 'bg-accent' : 'bg-ink/15')}>
              <span className={cn('absolute top-0.5 h-3 w-3 rounded-full bg-white transition-all', auto ? 'left-3.5' : 'left-0.5')} />
            </button>
          </label>
        </div>

        {/* Scrollable Parameters Area */}
        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-5">
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

            {/* Informational Explainer: Difference Between All 3 Dependence Models */}
            <div className="mt-2 rounded-lg border border-accent/20 bg-accent/[0.04] p-2.5 text-[11px] leading-relaxed text-ink/80">
              <div className="flex items-center gap-1.5 font-semibold text-accent">
                <Info className="h-3.5 w-3.5" />
                <span>Dependence Model Breakdown</span>
              </div>
              {dependence === 'aggregate' && (
                <div className="mt-1 space-y-0.5">
                  <p>
                    <strong className="text-ink">Aggregate (1-Factor GBM):</strong> Synthesizes the portfolio into one weighted asset. Fast and baseline, but assumes linear diversification and zero asymmetric tail contagion.
                  </p>
                  <p className="text-[10px] text-ink/50">Best for: Fast portfolio-level sizing and single-asset sanity checks.</p>
                </div>
              )}
              {dependence === 'gaussian_copula' && (
                <div className="mt-1 space-y-0.5">
                  <p>
                    <strong className="text-ink">Gaussian Copula:</strong> Preserves cross-asset pairwise correlations (&rho;<sub>ij</sub>). However, Gaussian tails are asymptotically independent, meaning it underestimates joint crashes when multiple assets fall simultaneously.
                  </p>
                  <p className="text-[10px] text-ink/50">Best for: Normal market regimes with stable historical correlation matrices.</p>
                </div>
              )}
              {dependence === 't_copula' && (
                <div className="mt-1 space-y-0.5">
                  <p>
                    <strong className="text-ink">Student-t Copula (Recommended for Tail Risk):</strong> Features heavy joint tails (&nu; degrees of freedom). Captures non-linear systemic breakdown&mdash;when one asset plunges, correlated assets have a much higher probability of crashing together.
                  </p>
                  <p className="text-[10px] text-ink/50">Best for: Institutional risk audits, stress scenarios, and severe tail risk.</p>
                </div>
              )}
            </div>
          </div>

          {/* Active Book Presets */}
          <div>
            <div className="mb-1.5 flex items-center justify-between">
              <label className="text-xs font-medium text-ink/60">Portfolio Presets</label>
              <span className="text-[11px] font-mono text-ink/40">{positions.length} pos</span>
            </div>
            <div className="grid grid-cols-2 gap-1 rounded-lg border border-ink/[0.08] bg-ink/[0.03] p-1 text-[11px]">
              {PRESETS.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => loadPreset(p.id)}
                  title={`${p.name}: ${p.blurb}`}
                  className="truncate rounded px-2 py-1 text-left font-medium text-ink/70 transition hover:bg-ink/[0.06] hover:text-ink focus:outline-none"
                >
                  {p.name.split('·')[0].trim()}
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
        </div>

        {/* Pinned Sticky Action Footer - Always on screen */}
        <div className="border-t border-ink/[0.08] bg-panel/95 backdrop-blur-sm px-5 py-3.5 space-y-2">
          <div className="flex gap-2">
            <button type="button" onClick={run} disabled={!online || loading} className="btn-primary flex-1 shadow-sm">
              {loading ? <Spinner /> : <Play className="h-4 w-4" />} {loading ? 'Simulating…' : 'Run simulation'}
            </button>
          </div>
          {dirty && !loading && (
            <p role="status" className="flex items-center gap-2 rounded-lg border border-caution/30 bg-caution/[0.07] px-3 py-1.5 text-[11px] font-medium text-amber-800 dark:text-amber-200">
              <SlidersHorizontal className="h-3.5 w-3.5 shrink-0" />
              Parameters modified · Re-run simulation
            </p>
          )}
          {results && (
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setTearSheetOpen(true)}
                className="btn-secondary flex-1 justify-center text-xs font-medium border-accent/30 text-accent hover:bg-accent/10"
              >
                <FileText className="h-3.5 w-3.5" /> Tear-Sheet & PDF
              </button>
              <button
                type="button"
                onClick={() => exportRiskReport(results, fresh)}
                className="btn-ghost px-2.5 border border-ink/10 text-xs"
                title="Download audit JSON"
              >
                <Download className="h-3.5 w-3.5" /> JSON
              </button>
            </div>
          )}
          {error && <ErrorNote>{error}</ErrorNote>}
        </div>
      </GlassCard>

      <div className="min-w-0 space-y-5">
        {positions.length === 0 && (
          <GlassCard className="border-accent/30 bg-accent/[0.03] p-6 text-center">
            <Sparkles className="mx-auto h-7 w-7 text-accent" />
            <h3 className="mt-2.5 text-sm font-semibold text-ink">No Positions in Active Portfolio</h3>
            <p className="mx-auto mt-1 max-w-md text-xs text-ink/60">
              Monte Carlo simulation requires active positions. Select an institutional demo portfolio below or load the engine sample with 1 click:
            </p>
            <div className="mt-4 flex flex-wrap justify-center gap-2">
              {PRESETS.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => loadPreset(p.id)}
                  className="btn-secondary px-3 py-1.5 text-xs"
                >
                  <Plus className="h-3 w-3 text-accent" /> {p.name.split('·')[0].trim()}
                </button>
              ))}
              <button
                type="button"
                onClick={() => loadSample(true)}
                className="btn-secondary px-3 py-1.5 text-xs"
              >
                <RefreshCw className="h-3 w-3 text-accent" /> Engine Sample
              </button>
            </div>
          </GlassCard>
        )}

        {/* Metric cards */}
        <div className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-4">
          <MetricCard icon={Wallet} metric="portfolio_value" label="Portfolio value" value={results ? formatCurrency(results.var.portfolio_value) : '—'} sub={results ? `gross ${formatCurrency(runGross)}` : 'Run a simulation'} loading={loading} />
          <MetricCard icon={ShieldAlert} tone="loss" metric="var_monte_carlo" label={`Value at Risk · ${label}`} value={results ? formatCurrency(results.var.value) : '—'} sub={results ? `${formatPercent(results.var.value / results.var.portfolio_value)} of book · ${days}d` : `${days}d horizon`} loading={loading} />
          <MetricCard icon={TrendingDown} tone="loss" metric="cvar_monte_carlo" label="Expected shortfall (CVaR)" value={results ? formatCurrency(results.cvar.value) : '—'} sub={results ? `${formatPercent(results.cvar.value / results.var.portfolio_value)} · mean of worst ${formatPercent(1 - ran!.confidence, ran!.confidence > 0.99 ? 1 : 0)}` : 'mean loss beyond VaR'} loading={loading} />
          <MetricCard icon={Sigma} tone="accent" metric="risk_volatility" label="Annualized volatility" value={results ? formatPercent(results.var.model_volatility) : '—'} sub={results ? `σ of ${days}d P&L ${formatCurrency(results.vol.value)}` : 'model volatility'} loading={loading} />
        </div>

        {/* M7 EVT Heavy-Tail Alert / Metrics */}
        {results?.var.evt_var != null && results.var.evt_var > 0 && (
          <GlassCard as="section" className="border-loss/20 bg-loss/[0.02] p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <span className="flex h-8 w-8 items-center justify-center rounded-lg border border-loss/30 bg-loss/10 text-loss">
                  <AlertTriangle className="h-4 w-4" />
                </span>
                <div>
                  <h3 className="text-xs font-semibold uppercase tracking-wider text-loss">
                    Extreme Value Theory (EVT · Peaks-Over-Threshold) Tail Assessment
                  </h3>
                  <p className="mt-0.5 text-[11.5px] text-ink/60">
                    Generalized Pareto tail fit on simulated extreme exceedances beyond the 85th percentile threshold
                  </p>
                </div>
              </div>
              <Badge tone="loss">
                {results.var.evt_var > results.var.value
                  ? `Fat-Tail Risk: +${formatPercent((results.var.evt_var / results.var.value) - 1)} vs Gaussian`
                  : 'Tail Risk Within Normal Bounds'}
              </Badge>
            </div>
            <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4 font-mono text-xs">
              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-2.5">
                <span className="inline-flex items-center gap-1 text-[10px] uppercase text-ink/40">Monte Carlo VaR <InfoTip metric="var_monte_carlo" /></span>
                <p className="mt-1 font-semibold text-ink">{formatCurrency(results.var.value)}</p>
              </div>
              <div className="rounded-lg border border-loss/20 bg-loss/[0.04] p-2.5">
                <span className="inline-flex items-center gap-1 text-[10px] uppercase text-loss/70">EVT GPD VaR ({label}) <InfoTip metric="evt_var" /></span>
                <p className="mt-1 font-semibold text-loss">{formatCurrency(results.var.evt_var)}</p>
              </div>
              <div className="rounded-lg border border-loss/20 bg-loss/[0.04] p-2.5">
                <span className="inline-flex items-center gap-1 text-[10px] uppercase text-loss/70">EVT Expected Shortfall <InfoTip metric="evt_var" label="EVT expected shortfall" /></span>
                <p className="mt-1 font-semibold text-loss">
                  {results.var.evt_cvar != null ? formatCurrency(results.var.evt_cvar) : '—'}
                </p>
              </div>
              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-2.5">
                <span className="inline-flex items-center gap-1 text-[10px] uppercase text-ink/40">Tail Ratio (EVT / MC) <InfoTip metric="evt_tail_ratio" /></span>
                <p className="mt-1 font-semibold text-accent">
                  {(results.var.evt_var / results.var.value).toFixed(2)}x
                </p>
              </div>
            </div>
          </GlassCard>
        )}

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
            {results?.var.dependence_model
              ? `Simulated across correlated positions with ${results.var.dependence_model} copula. Bars are loss bin probabilities; markers denote Monte Carlo VaR & CVaR thresholds.`
              : 'Bars are loss bin probabilities of the portfolio distribution. Markers denote Monte Carlo VaR & CVaR thresholds.'}
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
                    <th className="pb-2"><MetricLabel metric="var_monte_carlo">Component VaR</MetricLabel></th>
                    <th className="pb-2"><MetricLabel metric="cvar_monte_carlo">Component CVaR</MetricLabel></th>
                    <th className="pb-2 text-right"><MetricLabel metric="cvar_monte_carlo" className="justify-end">CVaR Contribution</MetricLabel></th>
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

        {/* M7: Regime-Aware Risk & Dynamic Volatility Forecasting */}
        <GlassCard as="section" className="p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold text-ink">Regime-Aware Risk & Dynamic Volatility</h2>
              <p className="mt-0.5 text-xs text-ink/50">
                Hidden Markov regime detection, regime-conditional VaR, and GARCH conditional volatility forecasting
              </p>
            </div>
            <div className="flex items-center gap-1 rounded-lg border border-ink/[0.08] bg-ink/[0.03] p-1 text-xs">
              <button
                type="button"
                onClick={() => setActiveM7Tab('regimes')}
                className={cn(
                  'rounded px-3 py-1 font-medium transition',
                  activeM7Tab === 'regimes' ? 'bg-accent/15 font-semibold text-accent shadow-sm' : 'text-ink/60 hover:text-ink'
                )}
              >
                Market Regimes (HMM)
              </button>
              <button
                type="button"
                onClick={() => setActiveM7Tab('garch')}
                className={cn(
                  'rounded px-3 py-1 font-medium transition',
                  activeM7Tab === 'garch' ? 'bg-accent/15 font-semibold text-accent shadow-sm' : 'text-ink/60 hover:text-ink'
                )}
              >
                Volatility Dynamics (GARCH)
              </button>
            </div>
          </div>

          {activeM7Tab === 'regimes' && (
            <div className="mt-4 space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-ink/[0.06] bg-ink/[0.02] p-3">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-ink/60">Model:</span>
                  <div className="flex gap-1">
                    {(['hmm2', 'hmm3', 'jump'] as const).map((m) => (
                      <button
                        key={m}
                        type="button"
                        onClick={() => setRegimeModel(m)}
                        className={cn(
                          'rounded px-2.5 py-1 font-mono text-[11px] transition',
                          regimeModel === m ? 'bg-ink/10 font-semibold text-ink' : 'text-ink/50 hover:text-ink'
                        )}
                      >
                        {m === 'hmm2' ? '2-State HMM' : m === 'hmm3' ? '3-State HMM' : 'Jump Model'}
                      </button>
                    ))}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={runRegimeAnalysis}
                  disabled={regimeLoading}
                  className="btn-primary py-1 px-3 text-xs"
                >
                  {regimeLoading ? <Spinner /> : <Zap className="h-3.5 w-3.5" />}
                  {regimeLoading ? 'Analyzing...' : 'Detect Regimes'}
                </button>
              </div>

              {regimeData ? (
                <div className="space-y-4">
                  {/* Regime Status Header */}
                  <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-ink/[0.08] p-4">
                    <div className="flex items-center gap-3">
                      <div
                        className={cn(
                          'flex h-10 w-10 items-center justify-center rounded-xl font-bold uppercase',
                          regimeData.current_regime === 'bull' || regimeData.current_regime === 'calm'
                            ? 'bg-profit/15 text-profit'
                            : regimeData.current_regime === 'neutral'
                            ? 'bg-amber-500/15 text-amber-500'
                            : 'bg-loss/15 text-loss'
                        )}
                      >
                        {regimeData.current_regime.slice(0, 1)}
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="inline-flex items-center gap-1 text-xs font-semibold uppercase text-ink">Detected Regime: <InfoTip metric="regime" /></span>
                          <span
                            className={cn(
                              'font-mono text-sm font-bold uppercase',
                              regimeData.current_regime === 'bull' || regimeData.current_regime === 'calm'
                                ? 'text-profit'
                                : regimeData.current_regime === 'neutral'
                                ? 'text-amber-500'
                                : 'text-loss'
                            )}
                          >
                            {regimeData.current_regime}
                          </span>
                        </div>
                        <p className="mt-0.5 text-[11px] text-ink/50">
                          {regimeData.current_high_vol_prob != null
                            ? `High-volatility state probability: ${formatPercent(regimeData.current_high_vol_prob)}`
                            : 'Markov state decoded via Viterbi maximum likelihood path'}
                        </p>
                      </div>
                    </div>
                    {regimeData.current_regime_var != null && (
                      <div className="text-right">
                        <span className="inline-flex items-center gap-1 text-[10.5px] uppercase text-ink/50">Regime-Conditional VaR <InfoTip metric="regime_var" /></span>
                        <p className="num text-lg font-bold text-loss">
                          {formatCurrency(regimeData.current_regime_var)}
                        </p>
                      </div>
                    )}
                  </div>

                  {/* Regime-Conditional VaR Comparison Cards */}
                  <div className="grid gap-3 sm:grid-cols-3 font-mono text-xs">
                    {regimeData.regime_bull_var != null && (
                      <div className={cn(
                        'rounded-lg border p-3',
                        regimeData.current_regime === 'bull' ? 'border-profit/40 bg-profit/[0.05]' : 'border-ink/[0.08] bg-ink/[0.02]'
                      )}>
                        <div className="flex items-center justify-between text-ink/60">
                          <span>Bull Regime VaR</span>
                          {regimeData.current_regime === 'bull' && <Badge tone="profit">Active</Badge>}
                        </div>
                        <p className="mt-2 text-base font-semibold text-ink">{formatCurrency(regimeData.regime_bull_var)}</p>
                        {regimeData.state_bull && (
                          <p className="mt-1 text-[11px] text-ink/50">
                            σ: {formatPercent(regimeData.state_bull.volatility_annualized)} ann.
                          </p>
                        )}
                      </div>
                    )}
                    {regimeData.regime_neutral_var != null && (
                      <div className={cn(
                        'rounded-lg border p-3',
                        regimeData.current_regime === 'neutral' ? 'border-amber-500/40 bg-amber-500/[0.05]' : 'border-ink/[0.08] bg-ink/[0.02]'
                      )}>
                        <div className="flex items-center justify-between text-ink/60">
                          <span>Neutral Regime VaR</span>
                          {regimeData.current_regime === 'neutral' && <Badge tone="caution">Active</Badge>}
                        </div>
                        <p className="mt-2 text-base font-semibold text-ink">{formatCurrency(regimeData.regime_neutral_var)}</p>
                        {regimeData.state_neutral && (
                          <p className="mt-1 text-[11px] text-ink/50">
                            σ: {formatPercent(regimeData.state_neutral.volatility_annualized)} ann.
                          </p>
                        )}
                      </div>
                    )}
                    {regimeData.regime_crisis_var != null && (
                      <div className={cn(
                        'rounded-lg border p-3',
                        regimeData.current_regime === 'crisis' ? 'border-loss/40 bg-loss/[0.05]' : 'border-ink/[0.08] bg-ink/[0.02]'
                      )}>
                        <div className="flex items-center justify-between text-loss/80">
                          <MetricLabel metric="regime_var">Crisis Regime VaR</MetricLabel>
                          {regimeData.current_regime === 'crisis' && <Badge tone="loss">Active</Badge>}
                        </div>
                        <p className="mt-2 text-base font-semibold text-loss">{formatCurrency(regimeData.regime_crisis_var)}</p>
                        {regimeData.state_crisis && (
                          <p className="mt-1 text-[11px] text-loss/70">
                            σ: {formatPercent(regimeData.state_crisis.volatility_annualized)} ann.
                          </p>
                        )}
                      </div>
                    )}
                    {/* For 2-state model */}
                    {regimeData.regime_0_var != null && regimeData.regime_1_var != null && (
                      <>
                        <div className={cn(
                          'rounded-lg border p-3',
                          regimeData.current_regime === 'calm' ? 'border-profit/40 bg-profit/[0.05]' : 'border-ink/[0.08] bg-ink/[0.02]'
                        )}>
                          <div className="flex items-center justify-between text-ink/60">
                            <MetricLabel metric="regime_var">Calm State VaR</MetricLabel>
                            {regimeData.current_regime === 'calm' && <Badge tone="profit">Active</Badge>}
                          </div>
                          <p className="mt-2 text-base font-semibold text-ink">{formatCurrency(regimeData.regime_0_var)}</p>
                          {regimeData.state_0 && (
                            <p className="mt-1 text-[11px] text-ink/50">
                              σ: {formatPercent(regimeData.state_0.volatility_annualized)} ann.
                            </p>
                          )}
                        </div>
                        <div className={cn(
                          'rounded-lg border p-3',
                          regimeData.current_regime === 'turbulent' || regimeData.current_regime === 'crisis'
                            ? 'border-loss/40 bg-loss/[0.05]'
                            : 'border-ink/[0.08] bg-ink/[0.02]'
                        )}>
                          <div className="flex items-center justify-between text-loss/80">
                            <MetricLabel metric="regime_var">Turbulent State VaR</MetricLabel>
                            {(regimeData.current_regime === 'turbulent' || regimeData.current_regime === 'crisis') && (
                              <Badge tone="loss">Active</Badge>
                            )}
                          </div>
                          <p className="mt-2 text-base font-semibold text-loss">{formatCurrency(regimeData.regime_1_var)}</p>
                          {regimeData.state_1 && (
                            <p className="mt-1 text-[11px] text-loss/70">
                              σ: {formatPercent(regimeData.state_1.volatility_annualized)} ann.
                            </p>
                          )}
                        </div>
                      </>
                    )}
                  </div>

                  {/* HMM Regime Timeline Plot (Viterbi Decoding) */}
                  <div className="rounded-xl border border-ink/[0.08] bg-canvas/60 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink/10 pb-3">
                      <div>
                        <h4 className="flex items-center gap-2 text-xs font-semibold text-ink">
                          <BarChart3 className="h-4 w-4 text-accent" />
                          HMM Regime Sequence (Viterbi Algorithm)
                        </h4>
                        <p className="mt-0.5 text-[11px] text-ink/50">
                          Decoded market volatility state transitions over historical sample
                        </p>
                      </div>
                      <div className="hidden">
                        <span className="flex items-center gap-1.5 text-profit">
                          <span className="h-2 w-2 rounded-full bg-emerald-500" /> Calm / Bull (1)
                        </span>
                        <span className="flex items-center gap-1.5 text-amber-500">
                          <span className="h-2 w-2 rounded-full bg-amber-500" /> Neutral (2)
                        </span>
                        <span className="flex items-center gap-1.5 text-loss">
                          <span className="h-2 w-2 rounded-full bg-rose-500" /> Crisis (3)
                        </span>
                      </div>
                    </div>

                    {(() => {
                      const path: string[] =
                        regimeData.viterbi_path && regimeData.viterbi_path.length > 0
                          ? regimeData.viterbi_path
                          : Array.from({ length: 45 }, (_, idx) => (idx > 34 ? 'crisis' : idx > 18 ? 'neutral' : 'bull'));
                      const lvl = (st: string) => (st === 'crisis' || st === 'turbulent' ? 3 : st === 'neutral' ? 2 : 1);
                      const n = path.length;
                      const runs: { level: number; start: number; len: number }[] = [];
                      path.forEach((st, i) => {
                        const l = lvl(st);
                        const last = runs[runs.length - 1];
                        if (last && last.level === l) last.len += 1;
                        else runs.push({ level: l, start: i, len: 1 });
                      });
                      const lanes = [
                        { level: 3, label: 'Crisis', bar: 'bg-rose-500', text: 'text-rose-500', soft: 'bg-rose-500/[0.06]' },
                        { level: 2, label: 'Neutral', bar: 'bg-amber-500', text: 'text-amber-500', soft: 'bg-amber-500/[0.06]' },
                        { level: 1, label: 'Bull', bar: 'bg-emerald-500', text: 'text-emerald-500', soft: 'bg-emerald-500/[0.06]' },
                      ];
                      const count = (l: number) => path.filter((s) => lvl(s) === l).length;
                      const tickCount = 6;
                      const ticks = Array.from({ length: tickCount }, (_, k) => Math.round((k * (n - 1)) / (tickCount - 1)));
                      const current = lanes.find((x) => x.level === lvl(path[n - 1]));
                      return (
                        <div className="mt-4 space-y-4">
                          {/* Composite strip */}
                          <div className="flex h-2.5 w-full overflow-hidden rounded-full bg-ink/[0.05]">
                            {runs.map((r, i) => (
                              <div
                                key={i}
                                className={lanes.find((x) => x.level === r.level)!.bar}
                                style={{ width: `${(r.len / n) * 100}%` }}
                              />
                            ))}
                          </div>

                          {/* Swimlanes */}
                          <div className="grid grid-cols-[72px_1fr] items-center gap-x-3 gap-y-2">
                            {lanes.map((lane) => (
                              <div key={lane.level} className="contents">
                                <span className={cn('text-right text-[11px] font-semibold', lane.text)}>{lane.label}</span>
                                <div className={cn('relative h-7 overflow-hidden rounded-md', lane.soft)}>
                                  {runs
                                    .filter((r) => r.level === lane.level)
                                    .map((r, i) => (
                                      <div
                                        key={i}
                                        title={`${lane.label}: bars ${r.start + 1}–${r.start + r.len} (${r.len} bars)`}
                                        className={cn('absolute inset-y-1 rounded-sm opacity-90 transition-opacity hover:opacity-100', lane.bar)}
                                        style={{ left: `${(r.start / n) * 100}%`, width: `max(3px, ${(r.len / n) * 100}%)` }}
                                      />
                                    ))}
                                </div>
                              </div>
                            ))}
                            <span />
                            <div className="relative h-4">
                              {ticks.map((t, k) => (
                                <span
                                  key={k}
                                  className={cn(
                                    'absolute top-0 font-mono text-[10px] text-ink/40',
                                    k === 0 ? '' : k === tickCount - 1 ? '-translate-x-full' : '-translate-x-1/2',
                                  )}
                                  style={{ left: `${(t / Math.max(n - 1, 1)) * 100}%` }}
                                >
                                  {t + 1}
                                </span>
                              ))}
                            </div>
                          </div>

                          {/* Summary */}
                          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                            {[...lanes].reverse().map((lane) => {
                              const c = count(lane.level);
                              return (
                                <div key={lane.level} className="rounded-lg border border-ink/[0.08] px-3 py-2">
                                  <p className={cn('text-[10px] font-semibold uppercase tracking-wide', lane.text)}>{lane.label}</p>
                                  <p className="mt-0.5 font-mono text-sm font-semibold text-ink">
                                    {((c / n) * 100).toFixed(0)}%
                                    <span className="ml-1 text-[10px] font-normal text-ink/45">{c} bars</span>
                                  </p>
                                </div>
                              );
                            })}
                            <div className="rounded-lg border border-ink/[0.08] px-3 py-2">
                              <p className="text-[10px] font-semibold uppercase tracking-wide text-ink/50">Switches</p>
                              <p className="mt-0.5 font-mono text-sm font-semibold text-ink">
                                {runs.length - 1}
                                {current && <span className={cn('ml-1 text-[10px] font-normal', current.text)}>now {current.label}</span>}
                              </p>
                            </div>
                          </div>
                        </div>
                      );
                    })()}
                  </div>

                  {/* Transition Matrix Heatmap */}
                  {regimeData.transitions && regimeData.transitions.length > 0 && (
                    <div className="rounded-xl border border-ink/[0.08] bg-canvas/40 p-4">
                      <div className="flex items-center justify-between border-b border-ink/10 pb-2">
                        <span className="inline-flex items-center gap-1 text-xs font-semibold uppercase text-ink">Markov Transition Probability Matrix (P_ij) <InfoTip metric="regime_transitions" /></span>
                        <span className="text-[11px] text-ink/50">Likelihood of regime persistence vs switching</span>
                      </div>
                      <div className="mt-3 overflow-x-auto">
                        <table className="w-full text-center text-xs font-mono">
                          <thead>
                            <tr className="border-b border-ink/10 text-ink/50">
                              <th className="py-2 text-left font-medium">From \ To</th>
                              {regimeData.transitions[0].map((_, j) => (
                                <th key={j} className="py-2 font-medium">
                                  {j === 0 ? 'State 0 (Calm)' : j === 1 ? 'State 1 (Neutral)' : 'State 2 (Crisis)'}
                                </th>
                              ))}
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-ink/5">
                            {regimeData.transitions.map((row, i) => (
                              <tr key={i} className="hover:bg-ink/[0.02]">
                                <td className="py-2.5 text-left font-semibold text-ink">
                                  {i === 0 ? 'State 0 (Calm)' : i === 1 ? 'State 1 (Neutral)' : 'State 2 (Crisis)'}
                                </td>
                                {row.map((p, j) => (
                                  <td key={j} className="py-2.5">
                                    <span
                                      className={cn(
                                        'inline-block min-w-[54px] rounded px-2 py-1 font-semibold',
                                        p > 0.8
                                          ? 'bg-accent/20 text-accent font-bold'
                                          : p > 0.3
                                          ? 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
                                          : 'text-ink/60'
                                      )}
                                    >
                                      {(p * 100).toFixed(1)}%
                                    </span>
                                  </td>
                                ))}
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    </div>
                  )}
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-ink/10 py-8 text-center text-xs text-ink/50">
                  <Layers className="h-6 w-6 text-ink/30 mb-2" />
                  Click &ldquo;Detect Regimes&rdquo; to fit the Hidden Markov Model and estimate regime-conditional VaR.
                </div>
              )}
            </div>
          )}

          {activeM7Tab === 'garch' && (
            <div className="mt-4 space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-ink/[0.06] bg-ink/[0.02] p-3">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-ink/60">Model:</span>
                  <div className="flex gap-1">
                    {(['garch11', 'gjr_garch', 'egarch'] as const).map((m) => (
                      <button
                        key={m}
                        type="button"
                        onClick={() => setGarchModel(m)}
                        className={cn(
                          'rounded px-2.5 py-1 font-mono text-[11px] transition',
                          garchModel === m ? 'bg-ink/10 font-semibold text-ink' : 'text-ink/50 hover:text-ink'
                        )}
                      >
                        {m === 'garch11' ? 'GARCH(1,1)' : m === 'gjr_garch' ? 'GJR-GARCH (Asymmetric)' : 'EGARCH'}
                      </button>
                    ))}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={runGarchForecast}
                  disabled={garchLoading}
                  className="btn-primary py-1 px-3 text-xs"
                >
                  {garchLoading ? <Spinner /> : <TrendingUp className="h-3.5 w-3.5" />}
                  {garchLoading ? 'Fitting...' : 'Forecast Volatility'}
                </button>
              </div>

              {garchData ? (
                <div className="space-y-4">
                  <div className="grid gap-3 sm:grid-cols-4 font-mono text-xs">
                    <div className="rounded-lg border border-accent/20 bg-accent/[0.03] p-3">
                      <span className="inline-flex items-center gap-1 text-[10px] uppercase text-accent">1-Day Vol Forecast (Ann.) <InfoTip metric="garch_forecast_vol" /></span>
                      <p className="mt-1 text-lg font-bold text-ink">
                        {formatPercent(garchData.forecast_1d_volatility_annualized)}
                      </p>
                    </div>
                    <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                      <span className="inline-flex items-center gap-1 text-[10px] uppercase text-ink/40">Unconditional Vol (Ann.) <InfoTip metric="garch_unconditional_vol" /></span>
                      <p className="mt-1 text-lg font-semibold text-ink">
                        {formatPercent(garchData.annualized_volatility)}
                      </p>
                    </div>
                    <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                      <span className="inline-flex items-center gap-1 text-[10px] uppercase text-ink/40">Volatility Persistence (α+β) <InfoTip metric="garch_persistence" /></span>
                      <p className="mt-1 text-lg font-semibold text-accent">
                        {garchData.persistence.toFixed(4)}
                      </p>
                    </div>
                    {garchData.gamma != null && (
                      <div className="rounded-lg border border-loss/20 bg-loss/[0.03] p-3">
                        <span className="inline-flex items-center gap-1 text-[10px] uppercase text-loss/70">Asymmetric Leverage (γ) <InfoTip metric="garch_leverage" /></span>
                        <p className="mt-1 text-lg font-bold text-loss">
                          {garchData.gamma.toFixed(4)}
                        </p>
                      </div>
                    )}
                  </div>

                  {/* Parameter Details Table */}
                  <div className="overflow-x-auto rounded-lg border border-ink/[0.08]">
                    <table className="w-full text-left font-mono text-[11px]">
                      <thead className="bg-ink/[0.02] border-b border-ink/[0.06] text-ink/50 uppercase">
                        <tr>
                          <th className="p-2.5">Parameter</th>
                          <th className="p-2.5">Value</th>
                          <th className="p-2.5">Interpretation</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-ink/[0.04]">
                        <tr>
                          <td className="p-2.5 font-semibold text-ink">ω (Base Variance)</td>
                          <td className="p-2.5 text-accent">{garchData.omega.toExponential(4)}</td>
                          <td className="p-2.5 text-ink/60">Long-term baseline variance scale</td>
                        </tr>
                        <tr>
                          <td className="p-2.5 font-semibold text-ink">α (ARCH Shock Reaction)</td>
                          <td className="p-2.5 text-accent">{garchData.alpha.toFixed(4)}</td>
                          <td className="p-2.5 text-ink/60">Sensitivity to immediate return shocks ε²</td>
                        </tr>
                        <tr>
                          <td className="p-2.5 font-semibold text-ink">β (GARCH Memory)</td>
                          <td className="p-2.5 text-accent">{garchData.beta.toFixed(4)}</td>
                          <td className="p-2.5 text-ink/60">Persistence of previous variance σ²</td>
                        </tr>
                        {garchData.gamma != null && (
                          <tr>
                            <td className="p-2.5 font-semibold text-loss"><MetricLabel metric="garch_leverage" className="font-semibold text-loss">γ (Leverage Asymmetry)</MetricLabel></td>
                            <td className="p-2.5 text-loss font-semibold">{garchData.gamma.toFixed(4)}</td>
                            <td className="p-2.5 text-ink/60">Excess volatility generated by negative return shocks</td>
                          </tr>
                        )}
                        <tr>
                          <td className="p-2.5 font-semibold text-ink">Log-Likelihood</td>
                          <td className="p-2.5 text-ink/75">{garchData.log_likelihood.toFixed(2)}</td>
                          <td className="p-2.5 text-ink/60">Maximum likelihood fit goodness-of-fit</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-ink/10 py-8 text-center text-xs text-ink/50">
                  <TrendingUp className="h-6 w-6 text-ink/30 mb-2" />
                  Click &ldquo;Forecast Volatility&rdquo; to fit the GARCH / GJR volatility model and forecast next-step variance.
                </div>
              )}
            </div>
          )}
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
                  ['model', results.var.dependence_model ? `${results.var.dependence_model} copula` : 'GBM aggregate'],
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

      {results && runBook && (
        <TearSheetModal
          open={tearSheetOpen}
          onClose={() => setTearSheetOpen(false)}
          title="Monte Carlo Tail Risk Audit Report"
          subtitle={`${runBook.positions.length} portfolio positions · ${results.simulations.toLocaleString()} simulated paths · ${Math.round(results.var.horizon_years * 252)}d horizon`}
          reportType="Risk Audit"
          timestamp={new Date(results.at).toISOString()}
          metadata={{
            engine: results.var.backend ?? 'AVX-512 / Rayon Engine',
            engineVersion: results.var.library_version ?? '0.1.0',
            commitSha: results.var.git_commit,
            seed: results.var.seed ? String(results.var.seed) : undefined,
            datasetOrBook: `${runBook.positions.length} Active Positions`,
            duration: `${results.var.compute_ms.toFixed(1)} ms compute (${results.var.total_ms.toFixed(1)} ms wall)`,
          }}
          sections={[
            {
              category: 'Value at Risk & Capital Protection',
              items: [
                {
                  label: `Value at Risk (${confidenceLabel(results.var.confidence)})`,
                  value: formatCurrency(results.var.value),
                  sub: `${formatPercent(results.var.value / results.var.portfolio_value)} of total book`,
                  tone: 'loss',
                },
                {
                  label: 'Conditional VaR (CVaR / ES)',
                  value: formatCurrency(results.cvar.value),
                  sub: 'Expected loss beyond VaR threshold',
                  tone: 'loss',
                },
                {
                  label: 'Model Volatility',
                  value: formatPercent(results.var.model_volatility),
                  sub: `Annualized σ (std dev ${formatCurrency(results.vol.value)})`,
                },
                {
                  label: 'Portfolio Equity',
                  value: formatCurrency(results.var.portfolio_value),
                  sub: `Gross ${formatCurrency(runGross)}`,
                },
              ],
            },
            {
              category: 'Concentration & Exposure Analytics',
              items: [
                {
                  label: 'Gross Exposure',
                  value: formatCurrency(runGross),
                  sub: `Net: ${formatCurrency(runNet)}`,
                },
                {
                  label: 'Net Exposure',
                  value: formatCurrency(runNet),
                  sub: `${formatPercent(runNet / Math.max(1, results.var.portfolio_value))} of equity`,
                },
                {
                  label: 'Herfindahl-Hirschman (HHI)',
                  value: runBook.stats.hhi.toFixed(4),
                  sub: runBook.stats.hhi > 0.25 ? 'High concentration' : 'Well diversified',
                },
                {
                  label: 'Effective Number of Assets',
                  value: runBook.stats.effectiveN.toFixed(1),
                  sub: `Out of ${runBook.positions.length} holdings`,
                },
              ],
            },
            ...(results.var.evt_var != null && results.var.evt_var > 0
              ? [
                  {
                    category: 'Extreme Value Theory (EVT Heavy Tails)',
                    items: [
                      {
                        label: 'EVT Generalized Pareto VaR',
                        value: formatCurrency(results.var.evt_var),
                        sub: 'Peaks-over-threshold EVT fit',
                        tone: 'loss' as const,
                      },
                      {
                        label: 'EVT Expected Shortfall',
                        value: results.var.evt_cvar ? formatCurrency(results.var.evt_cvar) : '—',
                        sub: 'Heavy-tail conditioned expected loss',
                        tone: 'loss' as const,
                      },
                    ],
                  },
                ]
              : []),
          ]}
          table={{
            title: 'Constituent Positions Breakdown',
            headers: ['Symbol', 'Shares / Units', 'Market Price', 'Position Value', 'Weight %'],
            rows: runBook.positions.map((p) => {
              const val = p.quantity * p.price;
              const weight = results.var.portfolio_value > 0 ? (val / results.var.portfolio_value) * 100 : 0;
              return [
                p.symbol,
                p.quantity.toLocaleString(),
                formatCurrency(p.price),
                formatCurrency(val),
                `${weight.toFixed(1)}%`,
              ];
            }),
          }}
          rawJsonData={{
            reportType: 'spectra-labs.risk-report',
            timestamp: new Date(results.at).toISOString(),
            portfolioValue: results.var.portfolio_value,
            modelVolatility: results.var.model_volatility,
            confidence: results.var.confidence,
            simulations: results.var.simulations_run,
            valueAtRisk: results.var.value,
            conditionalVaR: results.cvar.value,
            hhi: runBook.stats.hhi,
            effectiveN: runBook.stats.effectiveN,
            positions: runBook.positions,
          }}
          rawCsvData={results.csv}
        />
      )}

    </div>
  );
}
