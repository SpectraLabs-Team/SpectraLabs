'use client';

import { forwardRef, useEffect, useMemo, useState, type ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { FileText, ShieldCheck, Zap } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { HEADLINE } from '@/lib/baseline';
import { buildAndVerify } from '@/lib/hashChain';
import { analyticVaR } from '@/lib/model';
import { DEFAULT_PAPER, paperAccount } from '@/lib/paperLedger';
import { SAMPLE_CSV, parseCsv, portfolioStats } from '@/lib/portfolio';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import type { Rect } from './heroLayout';

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

type HoloProps = { icon: ReactNode; title: string; children: ReactNode; className?: string; style?: React.CSSProperties; delay: number; tone?: 'cyan' | 'blue' };

// No backdrop-filter: these cards sit over a moving backdrop, and Safari re-blurs every frame.
const Holo = forwardRef<HTMLDivElement, HoloProps>(function Holo({ icon, title, children, className, style, delay, tone = 'cyan' }, ref) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      style={style}
      initial={reduce ? false : { opacity: 0, y: 16, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.7, delay, ease: [0.22, 1, 0.36, 1] }}
      className={cn('w-[236px]', className)}
    >
      <div className="animate-float" style={{ animationDelay: `${delay * 3}s` }}>
        <div ref={ref} className="relative rounded-xl border border-cyan-300/30 bg-[#06122a]/[0.88] p-4 shadow-[0_0_0_1px_rgba(34,211,238,0.08),0_0_32px_-6px_rgba(34,211,238,0.55),inset_0_1px_0_rgba(255,255,255,0.08)]">
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
});

/**
 * `placed`: absolutely positioned at the rects computed by heroLayout (null = not shown; undefined =
 * not measured yet, rendered invisible so it can be measured). `grid`: a simple row for small screens.
 */
export function HoloCards({
  data,
  layout,
  positions,
  cardRefs,
}: {
  data: ReturnType<typeof useHeroData>;
  layout: 'placed' | 'grid';
  positions?: (Rect | null)[] | null;
  cardRefs?: React.MutableRefObject<(HTMLDivElement | null)[]>;
}) {
  const { kernel, risk, ledger } = data;
  const place = (i: number): { className?: string; style?: React.CSSProperties } => {
    if (layout !== 'placed') return {};
    const r = positions?.[i];
    // Unplaceable: keep it laid out (invisible) so its size stays measurable on the next pass.
    if (r === null) return { className: 'absolute left-0 top-0 invisible' };
    if (!r) return { className: 'absolute left-0 top-0 invisible' };
    return { className: 'absolute pointer-events-auto', style: { left: r.x, top: r.y } }; // left/top: Framer owns transform here
  };
  const setRef = (i: number) => (el: HTMLDivElement | null) => {
    if (cardRefs) cardRefs.current[i] = el;
  };
  return (
    <div className={layout === 'placed' ? 'contents' : 'grid gap-3 sm:grid-cols-3'}>
      <Holo ref={setRef(0)} icon={<Zap className="h-6 w-6" />} title="Pure Rust Kernel" delay={0.2} {...place(0)}>
        <p className="num text-[22px] font-semibold leading-7 text-white">{Math.round(kernel.pps).toLocaleString('en-US')}</p>
        <p>paths / sec {kernel.live ? <span className="text-slate-500">· live</span> : <span className="text-slate-500">· reference run</span>}</p>
        <p>
          Latency: <span className="num font-semibold text-emerald-400">{formatDuration(kernel.latency)}</span> <span className="text-slate-500">· 10k paths</span>
        </p>
      </Holo>
      <Holo ref={setRef(1)} icon={<ShieldCheck className="h-6 w-6" />} title="Pre-Trade Risk Check" delay={0.35} tone="blue" {...place(1)}>
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
      <Holo ref={setRef(2)} icon={<FileText className="h-6 w-6" />} title="SHA-256 Ledger" delay={0.5} {...place(2)}>
        <p className="num">Hash: {ledger ? `${ledger.head.slice(0, 6)}…${ledger.head.slice(-4)}` : '…'}</p>
        <p>
          Audit Status: <span className={cn('font-semibold', ledger?.verified ? 'text-emerald-400' : 'text-slate-500')}>{ledger ? (ledger.verified ? 'Verified' : 'Broken') : 'checking'}</span>
        </p>
      </Holo>
    </div>
  );
}

/**
 * Illustrative day moves for the ticker row (there is no live market data feed; the row carries an
 * "Illustrative quotes" note). Prices are the sample book's.
 */
const QUOTES: { sym: string; change: number; tile: string }[] = [
  { sym: 'AAPL', change: 0.0085, tile: 'from-slate-200/20 to-slate-400/5 text-slate-100' },
  { sym: 'NVDA', change: 0.0265, tile: 'from-emerald-400/25 to-emerald-600/5 text-emerald-300' },
  { sym: 'MSFT', change: 0.0092, tile: 'from-sky-400/25 to-sky-600/5 text-sky-200' },
];

/**
 * An intraday path that ends exactly at `change`: a Brownian bridge (78 five-minute steps) around a
 * straight line from 0 to `change`, so the sparkline wiggles naturally but agrees with the label.
 */
function bridgePath(seed: number, change: number, steps = 78): number[] {
  let s = seed >>> 0 || 1;
  const rand = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  const vol = Math.max(0.004, Math.abs(change) * 0.6);
  let w = 0;
  const walk = [0];
  for (let i = 0; i < steps; i++) {
    w += (Math.sqrt(-2 * Math.log(rand() || 1e-12)) * Math.cos(2 * Math.PI * rand()) * vol) / Math.sqrt(steps);
    walk.push(w);
  }
  const end = walk[steps];
  return walk.map((v, i) => 1 + (change * i) / steps + (v - (end * i) / steps));
}

const EMERALD = '#10B981';

function Sparkline({ points }: { points: number[] }) {
  const reduce = useReducedMotion();
  const lo = Math.min(...points);
  const hi = Math.max(...points);
  const d = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${((i / (points.length - 1)) * 84).toFixed(1)},${(28 - ((p - lo) / (hi - lo || 1)) * 24).toFixed(1)}`).join(' ');
  return (
    <svg viewBox="0 0 84 30" className="h-8 w-[84px] overflow-visible" aria-hidden>
      <motion.path
        key={d}
        d={d}
        fill="none"
        stroke={EMERALD}
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ filter: `drop-shadow(0 0 5px ${EMERALD})` }}
        initial={reduce ? false : { pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ duration: 1.2, ease: 'easeOut' }}
      />
    </svg>
  );
}

/** Ticker chips: symbol, sample-book price, an illustrative day move and a matching green sparkline. */
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
      QUOTES.map((q, k) => {
        const p = SAMPLE.find((x) => x.symbol === q.sym);
        return { ...q, price: p?.price ?? 0, path: bridgePath(1000 * (k + 1) + tick * 7919, q.change) };
      }),
    [tick],
  );

  return (
    <>
      {chips.map((c, i) => (
        <motion.div key={c.sym} initial={reduce ? false : { opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.6 + i * 0.12, duration: 0.6 }} className={cn(i === 1 && 'lg:-translate-y-6')}>
          <div className="animate-float" style={{ animationDelay: `${i * 1.7}s` }}>
            <div className="flex items-center gap-3 rounded-xl border border-cyan-300/25 bg-[#06122a]/[0.88] py-2.5 pl-3 pr-4 shadow-[0_0_28px_-8px_rgba(34,211,238,0.6),inset_0_1px_0_rgba(255,255,255,0.06)]">
              <span className={cn('flex h-10 w-10 items-center justify-center rounded-lg bg-gradient-to-br font-mono text-[12px] font-bold ring-1 ring-white/10', c.tile)}>{c.sym.slice(0, 2)}</span>
              <div>
                <p className="text-[13px] font-semibold leading-4 text-white">{c.sym}</p>
                <p className="num text-[15px] leading-5 text-white">{formatCurrency(c.price, 2)}</p>
                <p className="num text-[12.5px] font-semibold leading-4 text-emerald-400 drop-shadow-[0_0_6px_rgba(16,185,129,0.6)]">+{(c.change * 100).toFixed(2)}%</p>
              </div>
              <div className="ml-1">
                <Sparkline points={c.path} />
              </div>
            </div>
          </div>
        </motion.div>
      ))}
    </>
  );
}
