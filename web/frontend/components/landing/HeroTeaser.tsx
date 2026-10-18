'use client';

import { useEffect, useMemo, useState } from 'react';
import { Area, AreaChart, ReferenceLine, ResponsiveContainer, XAxis, YAxis } from 'recharts';
import { Cpu, Sigma } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { analyticCVaR, analyticVaR, densityCurve } from '@/lib/model';
import { SAMPLE_CSV, parseCsv, portfolioStats, shockPositions, toCsv } from '@/lib/portfolio';
import { useEngineHealth } from '@/lib/useEngineHealth';
import type { RiskResult } from '@/lib/types';
import { confidenceLabel, formatCompactCurrency, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { AnimatedValue, Segmented, RangeSlider, Spinner } from '@/components/ui/primitives';

const SAMPLE = parseCsv(SAMPLE_CSV).positions;
const SHOCKS = [
  { value: 1, label: 'No shock' },
  { value: 0.9, label: '−10%' },
  { value: 0.7, label: '−30%' },
];
const HORIZONS = [
  { value: 1, label: '1D' },
  { value: 10, label: '10D' },
  { value: 21, label: '1M' },
];

/** A playable mini risk calculator: closed-form results instantly, engine Monte Carlo when online. */
export function HeroTeaser() {
  const [confidence, setConfidence] = useState(0.99);
  const [shock, setShock] = useState(1);
  const [horizon, setHorizon] = useState(1);
  const engine = useEngineHealth();
  const [mc, setMc] = useState<RiskResult | null>(null);
  const [mcCvar, setMcCvar] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const positions = useMemo(() => shockPositions(SAMPLE, shock), [shock]);
  const stats = useMemo(() => portfolioStats(positions), [positions]);
  const input = { value: stats.netValue, volatility: stats.modelVolatility, horizonDays: horizon };
  const varCf = analyticVaR(input, confidence);
  const cvarCf = analyticCVaR(input, confidence);
  const shockLoss = portfolioStats(SAMPLE).netValue - stats.netValue;
  const { netValue, modelVolatility } = stats;
  const curve = useMemo(
    () => densityCurve({ value: netValue, volatility: modelVolatility, horizonDays: horizon }, varCf, 90),
    [netValue, modelVolatility, horizon, varCf],
  );

  // Live engine run, debounced, whenever a control moves.
  const online = engine.status === 'online';
  useEffect(() => {
    if (!online) return;
    let cancelled = false;
    const id = setTimeout(async () => {
      setBusy(true);
      try {
        const base = { portfolio_csv: toCsv(positions), confidence, simulations: 100_000, horizon_days: horizon, backend: 'auto' as const };
        const [v, c] = await Promise.all([calculateRisk({ ...base, metric: 'var' }), calculateRisk({ ...base, metric: 'cvar' })]);
        if (!cancelled) {
          setMc(v);
          setMcCvar(c.value);
        }
      } catch {
        if (!cancelled) {
          setMc(null);
          setMcCvar(null);
        }
      } finally {
        if (!cancelled) setBusy(false);
      }
    }, 220);
    return () => {
      cancelled = true;
      clearTimeout(id);
    };
  }, [online, positions, confidence, horizon]);

  return (
    <div className="glass relative overflow-hidden p-5 shadow-glow-lg sm:p-6">
      <div className="pointer-events-none absolute -right-20 -top-24 h-64 w-64 rounded-full bg-accent/20 blur-3xl" />
      <div className="relative flex items-center justify-between gap-3">
        <div>
          <p className="eyebrow">Live risk preview</p>
          <p className="mt-1 text-xs text-slate-400">
            Sample book · <span className="num text-slate-300">{formatCompactCurrency(stats.netValue)}</span> · 4 positions
          </p>
        </div>
        <span className="inline-flex items-center gap-1.5 rounded-full border border-white/10 px-2 py-0.5 font-mono text-[10px] text-slate-400">
          {online ? <Cpu className="h-3 w-3 text-profit" /> : <Sigma className="h-3 w-3 text-accent" />}
          {online ? 'engine · 100k paths' : 'closed form'}
        </span>
      </div>

      <div className="relative mt-5 grid grid-cols-2 gap-3">
        <div className="glass-subtle p-3.5">
          <p className="text-[11px] text-slate-500">VaR {confidenceLabel(confidence)}</p>
          <p className="num mt-1 text-2xl font-semibold text-loss">
            <AnimatedValue value={formatCurrency(online && mc ? mc.value : varCf)} />
          </p>
          <p className="num mt-0.5 text-[11px] text-slate-500">{formatPercent((online && mc ? mc.value : varCf) / stats.netValue)} of book</p>
        </div>
        <div className="glass-subtle p-3.5">
          <p className="text-[11px] text-slate-500">Expected shortfall</p>
          <p className="num mt-1 text-2xl font-semibold text-rose-300">
            <AnimatedValue value={formatCurrency(online && mcCvar !== null ? mcCvar : cvarCf)} />
          </p>
          <p className="num mt-0.5 text-[11px] text-slate-500">mean loss beyond VaR</p>
        </div>
      </div>

      <div className="relative mt-3 h-24" aria-hidden>
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={curve} margin={{ top: 4, right: 0, bottom: 0, left: 0 }}>
            <defs>
              <linearGradient id="tz-body" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor="#22D3EE" stopOpacity={0.45} />
                <stop offset="1" stopColor="#22D3EE" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="tz-tail" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor="#F43F5E" stopOpacity={0.7} />
                <stop offset="1" stopColor="#F43F5E" stopOpacity={0.05} />
              </linearGradient>
            </defs>
            <XAxis dataKey="loss" type="number" domain={['dataMin', 'dataMax']} hide />
            <YAxis hide />
            <Area dataKey="body" stroke="#22D3EE" strokeWidth={1.5} fill="url(#tz-body)" isAnimationActive={false} connectNulls={false} />
            <Area dataKey="tail" stroke="#F43F5E" strokeWidth={1.5} fill="url(#tz-tail)" isAnimationActive={false} connectNulls={false} />
            <ReferenceLine x={varCf} stroke="#F43F5E" strokeDasharray="3 3" />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      <div className="relative mt-4 space-y-4">
        <RangeSlider label="Confidence" value={confidence} min={0.95} max={0.999} step={0.001} onChange={setConfidence} display={confidenceLabel(confidence)} hint={<><span>95%</span><span>99.9%</span></>} />
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Segmented label="teaser-shock" value={shock} onChange={setShock} options={SHOCKS} size="sm" />
          <Segmented label="teaser-horizon" value={horizon} onChange={setHorizon} options={HORIZONS} size="sm" />
        </div>
      </div>

      <div className="relative mt-4 flex min-h-[20px] items-center justify-between gap-2 border-t border-white/[0.06] pt-3 font-mono text-[10.5px] text-slate-500">
        {shock < 1 ? <span>Shock loss <span className="text-loss">−{formatCurrency(shockLoss)}</span></span> : <span>GBM · σ {formatPercent(stats.modelVolatility, 0)} · zero drift</span>}
        {online ? (
          <span className="flex items-center gap-1.5">
            {busy && <Spinner className="h-3 w-3" />}
            {mc ? <>{mc.backend} · {formatDuration(mc.compute_ms)} · closed-form VaR {formatCurrency(varCf)}</> : 'running…'}
          </span>
        ) : (
          <span>start the engine for Monte Carlo</span>
        )}
      </div>
    </div>
  );
}
