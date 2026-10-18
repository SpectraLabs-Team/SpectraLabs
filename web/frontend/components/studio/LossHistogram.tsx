'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { lossHistogram, tailProbability, type ModelInput } from '@/lib/model';
import { cn, formatCompactCurrency, formatCurrency, formatPercent } from '@/lib/utils';

/** Round tick values (1/2/2.5/5 × 10^k steps) inside [lo, hi]. */
function niceTicks(lo: number, hi: number, target: number): number[] {
  const raw = (hi - lo) / Math.max(1, target - 1);
  if (!Number.isFinite(raw) || raw <= 0) return [];
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) out.push(Math.round(v / step) * step);
  return out;
}

/**
 * Loss-distribution histogram: exact bin probabilities of the engine's model, bins past VaR
 * shaded as tail. Plain SVG so both themes style it through CSS classes.
 */
export function LossHistogram({ input, varValue, cvarValue, confidenceLabel }: { input: ModelInput; varValue: number; cvarValue: number | null; confidenceLabel: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 320 });
  const [hover, setHover] = useState<number | null>(null);
  const positive = input.value > 0;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [positive]); // re-attach if the empty-state branch swapped the element

  const bins = useMemo(() => lossHistogram(input, 56), [input]);
  if (!(input.value > 0) || !bins.length) {
    return (
      <div ref={ref} className="h-full w-full">
        <div className="flex h-full items-center justify-center text-sm text-ink/50">The loss distribution needs a positive net portfolio value.</div>
      </div>
    );
  }

  const pad = { l: 8, r: 8, t: 26, b: 26 };
  const W = Math.max(200, size.w);
  const H = Math.max(160, size.h);
  const lo = bins[0].lo;
  const hi = bins[bins.length - 1].hi;
  const maxP = Math.max(...bins.map((b) => b.p));
  const x = (v: number) => pad.l + ((v - lo) / (hi - lo)) * (W - pad.l - pad.r);
  const y = (p: number) => H - pad.b - (p / maxP) * (H - pad.t - pad.b);
  const ticks = niceTicks(lo, hi, Math.max(3, Math.floor(W / 120))).filter((t) => x(t) > pad.l + 26 && x(t) < W - pad.r - 26);
  const hb = hover !== null ? bins[hover] : null;

  return (
    <div ref={ref} className="relative h-full w-full">
      <svg width={W} height={H} className="block" role="img" aria-label={`Loss distribution histogram. VaR ${confidenceLabel} ${formatCurrency(varValue)}.`} onMouseLeave={() => setHover(null)}>
        <defs>
          <linearGradient id="lh-body" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style={{ stopColor: 'rgb(var(--accent))', stopOpacity: 0.9 }} />
            <stop offset="1" style={{ stopColor: 'rgb(var(--accent))', stopOpacity: 0.25 }} />
          </linearGradient>
          <linearGradient id="lh-tail" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style={{ stopColor: 'rgb(var(--loss))', stopOpacity: 1 }} />
            <stop offset="1" style={{ stopColor: 'rgb(var(--loss))', stopOpacity: 0.35 }} />
          </linearGradient>
        </defs>
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={pad.l} x2={W - pad.r} y1={pad.t + f * (H - pad.t - pad.b)} y2={pad.t + f * (H - pad.t - pad.b)} className="stroke-ink/[0.06]" />
        ))}
        {bins.map((b, i) => {
          const tail = b.lo >= varValue;
          const x0 = x(b.lo) + 1;
          const w = Math.max(1, x(b.hi) - x(b.lo) - 2);
          return (
            <rect
              key={i}
              x={x0}
              y={y(b.p)}
              width={w}
              height={Math.max(0, H - pad.b - y(b.p))}
              rx={1.5}
              fill={tail ? 'url(#lh-tail)' : 'url(#lh-body)'}
              className={cn('transition-opacity', hover !== null && hover !== i && 'opacity-50')}
              onMouseEnter={() => setHover(i)}
            />
          );
        })}
        <line x1={pad.l} x2={W - pad.r} y1={H - pad.b} y2={H - pad.b} className="stroke-ink/15" />
        {lo < 0 && hi > 0 && <line x1={x(0)} x2={x(0)} y1={pad.t} y2={H - pad.b} className="stroke-ink/20" strokeDasharray="2 3" />}
        <line x1={x(varValue)} x2={x(varValue)} y1={pad.t - 6} y2={H - pad.b} className="stroke-loss" strokeWidth={1.5} strokeDasharray="4 3" />
        <text x={x(varValue)} y={pad.t - 10} textAnchor="middle" className="fill-loss font-mono text-[11px]">
          VaR {confidenceLabel}
        </text>
        {cvarValue !== null && cvarValue < hi && (
          <>
            <line x1={x(cvarValue)} x2={x(cvarValue)} y1={pad.t + 8} y2={H - pad.b} className="stroke-rose-400" strokeDasharray="2 3" />
            <text x={x(cvarValue)} y={pad.t + 4} textAnchor="middle" className="fill-rose-500 font-mono text-[10px] dark:fill-rose-300">
              ES
            </text>
          </>
        )}
        {ticks.map((t) => (
          <text key={t} x={x(t)} y={H - 8} textAnchor="middle" className="fill-ink/45 font-mono text-[11px]">
            {formatCompactCurrency(t)}
          </text>
        ))}
      </svg>
      {hb && (
        <div className="pointer-events-none absolute top-2 rounded-lg border border-ink/10 bg-panel/95 px-3 py-2 font-mono text-[11px] shadow-xl backdrop-blur" style={{ left: Math.min(W - 190, Math.max(0, x(hb.lo) - 80)) }}>
          <p className={hb.lo >= varValue ? 'text-loss' : 'text-ink/90'}>
            {formatCurrency(hb.lo)} to {formatCurrency(hb.hi)}
          </p>
          <p className="mt-0.5 text-ink/50">
            bin {formatPercent(hb.p, 3)} · P(loss ≥ {formatCompactCurrency(hb.lo)}) {formatPercent(tailProbability(input, hb.lo), 2)}
          </p>
        </div>
      )}
    </div>
  );
}
