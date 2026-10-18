'use client';

import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { FileText, ShieldCheck, Zap } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { HEADLINE } from '@/lib/baseline';
import { buildAndVerify } from '@/lib/hashChain';
import { DEFAULT_VOLATILITY, analyticVaR } from '@/lib/model';
import { DEFAULT_PAPER, paperAccount } from '@/lib/paperLedger';
import { SAMPLE_CSV, parseCsv, portfolioStats } from '@/lib/portfolio';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';

const SAMPLE = parseCsv(SAMPLE_CSV).positions;
const STATS = portfolioStats(SAMPLE);
const ACCOUNT = paperAccount(STATS, DEFAULT_PAPER);

/** Live data for the three hero cards; reference-run / closed-form values until the engine answers. */
export function useHeroData() {
  const engine = useEngineHealth();
  const online = engine.status === 'online';
  const [kernel, setKernel] = useState({ pps: HEADLINE.rustPps, latency: HEADLINE.smallMs ?? 0, live: false });
  const [risk, setRisk] = useState({ v: analyticVaR({ value: STATS.netValue, volatility: STATS.modelVolatility, horizonDays: 1 }, 0.99), live: false });
  const [ledger, setLedger] = useState<{ head: string; verified: boolean } | null>(null);

  useEffect(() => {
    buildAndVerify().then(setLedger);
  }, []);

  useEffect(() => {
    if (!online) return;
    let live = true;
    (async () => {
      try {
        const base = { portfolio_csv: SAMPLE_CSV, metric: 'var' as const, confidence: 0.99, horizon_days: 1, backend: 'auto' as const };
        await calculateRisk({ ...base, simulations: 100_000, backend: 'parallel' }); // warm-up, untimed
        const big = await calculateRisk({ ...base, simulations: 1_000_000, backend: 'parallel' });
        const small = await calculateRisk({ ...base, simulations: 10_000 });
        const r = await calculateRisk({ ...base, simulations: 100_000 });
        if (!live) return;
        setKernel({ pps: (big.simulations_run * 1000) / big.compute_ms, latency: small.compute_ms, live: true });
        setRisk({ v: r.value, live: true });
      } catch {
        /* keep reference values */
      }
    })();
    return () => {
      live = false;
    };
  }, [online]);

  return { kernel, risk, ledger };
}

function Holo({ icon, title, children, className, delay, tone = 'cyan' }: { icon: ReactNode; title: string; children: ReactNode; className?: string; delay: number; tone?: 'cyan' | 'blue' }) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      initial={reduce ? false : { opacity: 0, y: 16, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.7, delay, ease: [0.22, 1, 0.36, 1] }}
      className={cn('w-[236px]', className)}
    >
      <div className="animate-float" style={{ animationDelay: `${delay * 3}s` }}>
        <div className="relative rounded-xl border border-cyan-300/30 bg-[#06122a]/70 p-4 shadow-[0_0_0_1px_rgba(34,211,238,0.08),0_0_32px_-6px_rgba(34,211,238,0.55),inset_0_1px_0_rgba(255,255,255,0.08)] backdrop-blur-md">
          <div className="flex items-start gap-3">
            <span className={cn('mt-0.5 drop-shadow-[0_0_8px_currentColor]', tone === 'blue' ? 'text-sky-400' : 'text-cyan-300')}>{icon}</span>
            <div className="min-w-0">
              <p className="text-[13px] font-semibold text-white">{title}</p>
              <div className="mt-1.5 space-y-0.5 text-[12px] text-slate-300">{children}</div>
            </div>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

/** Three cards stepped diagonally inside their own column, so they can never cover the headline. */
export function HoloCards({ data, layout }: { data: ReturnType<typeof useHeroData>; layout: 'stepped' | 'grid' }) {
  const { kernel, risk, ledger } = data;
  const step = (cls: string) => (layout === 'stepped' ? cls : '');
  return (
    <div className={layout === 'stepped' ? 'relative h-[440px] w-full' : 'grid gap-3 sm:grid-cols-3'}>
      <Holo icon={<Zap className="h-6 w-6" />} title="Pure Rust Kernel" delay={0.2} className={step('absolute left-0 top-0')}>
        <p className="num text-[22px] font-semibold leading-7 text-white">{Math.round(kernel.pps).toLocaleString('en-US')}</p>
        <p>paths / sec {kernel.live ? <span className="text-slate-500">· live</span> : <span className="text-slate-500">· reference run</span>}</p>
        <p>
          Latency: <span className="num font-semibold text-emerald-400">{formatDuration(kernel.latency)}</span> <span className="text-slate-500">· 10k paths</span>
        </p>
      </Holo>
      <Holo icon={<ShieldCheck className="h-6 w-6" />} title="Pre-Trade Risk Check" delay={0.35} tone="blue" className={step('absolute left-[33%] top-[154px]')}>
        <p>
          VaR (99%):{' '}
          <span className="num text-[17px] font-semibold text-rose-400">−{formatCurrency(risk.v)}</span>
        </p>
        <p className="num font-medium text-rose-400">({formatPercent(-risk.v / STATS.netValue)})</p>
        <p>
          Leverage: <span className="num font-semibold text-emerald-400">{ACCOUNT.grossLeverage.toFixed(2)}× Passed</span>
          <span className="text-slate-500"> · paper</span>
        </p>
      </Holo>
      <Holo icon={<FileText className="h-6 w-6" />} title="SHA-256 Ledger" delay={0.5} className={step('absolute right-0 top-[308px]')}>
        <p className="num">Hash: {ledger ? `${ledger.head.slice(0, 6)}…${ledger.head.slice(-4)}` : '…'}</p>
        <p>
          Audit Status: <span className={cn('font-semibold', ledger?.verified ? 'text-emerald-400' : 'text-slate-500')}>{ledger ? (ledger.verified ? 'Verified' : 'Broken') : 'checking'}</span>
        </p>
      </Holo>
    </div>
  );
}

/** A simulated intraday GBM path (78 five-minute steps over one trading day). */
function simPath(seed: number, vol: number, steps = 78): number[] {
  let s = seed >>> 0 || 1;
  const rand = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  const dt = 1 / (252 * steps);
  const out = [1];
  for (let i = 0; i < steps; i++) {
    const z = Math.sqrt(-2 * Math.log(rand() || 1e-12)) * Math.cos(2 * Math.PI * rand());
    out.push(out[i] * Math.exp(-0.5 * vol * vol * dt + vol * Math.sqrt(dt) * z));
  }
  return out;
}

function Sparkline({ points, up }: { points: number[]; up: boolean }) {
  const reduce = useReducedMotion();
  const lo = Math.min(...points);
  const hi = Math.max(...points);
  const d = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${((i / (points.length - 1)) * 80).toFixed(1)},${(26 - ((p - lo) / (hi - lo || 1)) * 22).toFixed(1)}`).join(' ');
  const color = up ? '#34D399' : '#FB7185';
  return (
    <svg viewBox="0 0 80 28" className="h-7 w-20 overflow-visible" aria-hidden>
      <motion.path key={d} d={d} fill="none" stroke={color} strokeWidth={1.6} strokeLinejoin="round" style={{ filter: `drop-shadow(0 0 4px ${color})` }} initial={reduce ? false : { pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 1.2, ease: 'easeOut' }} />
    </svg>
  );
}

/**
 * Ticker chips: real symbol and price from the sample book, the position's weight, and a
 * sparkline of a *simulated* one-day path (no market data feed exists), re-drawn every few seconds.
 */
export function TickerChips() {
  const reduce = useReducedMotion();
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (reduce) return;
    const id = setInterval(() => setTick((t) => t + 1), 6000);
    return () => clearInterval(id);
  }, [reduce]);
  const chips = useMemo(
    () =>
      ['AAPL', 'NVDA', 'MSFT'].map((sym, k) => {
        const i = SAMPLE.findIndex((p) => p.symbol === sym);
        const p = SAMPLE[i];
        const path = simPath(1000 * (k + 1) + tick * 7919, p.volatility ?? DEFAULT_VOLATILITY);
        return { sym, price: p.price, weight: STATS.weights[i] ?? 0, path, up: path[path.length - 1] >= 1 };
      }),
    [tick],
  );

  return (
    <>
      {chips.map((c, i) => (
        <motion.div key={c.sym} initial={reduce ? false : { opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.6 + i * 0.12, duration: 0.6 }} className={cn(i === 1 && 'lg:-translate-y-6')}>
          <div className="animate-float" style={{ animationDelay: `${i * 1.7}s` }}>
            <div className="flex items-center gap-3 rounded-xl border border-cyan-300/25 bg-[#06122a]/70 py-2.5 pl-3 pr-4 shadow-[0_0_28px_-8px_rgba(34,211,238,0.6),inset_0_1px_0_rgba(255,255,255,0.06)] backdrop-blur-md">
              <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-white/[0.08] font-mono text-[11px] font-bold text-white">{c.sym.slice(0, 2)}</span>
              <div>
                <p className="text-[13px] font-semibold leading-4 text-white">{c.sym}</p>
                <p className="num text-[15px] leading-5 text-white">{formatCurrency(c.price, 2)}</p>
                <p className="num text-[11px] leading-4 text-emerald-400">{formatPercent(c.weight, 1)} of book</p>
              </div>
              <div className="ml-1 flex flex-col items-end">
                <Sparkline points={c.path} up={c.up} />
                <span className="font-mono text-[9px] uppercase tracking-wider text-slate-500">sim 1D</span>
              </div>
            </div>
          </div>
        </motion.div>
      ))}
    </>
  );
}
