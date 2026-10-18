'use client';

import { useEffect, useMemo, useState } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { cn, formatCurrency, formatPercent } from '@/lib/utils';
import { SAMPLE_CSV, parseCsv } from '@/lib/portfolio';

const SAMPLE = parseCsv(SAMPLE_CSV).positions;

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
