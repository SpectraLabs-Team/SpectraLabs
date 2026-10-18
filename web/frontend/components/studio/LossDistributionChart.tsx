'use client';

import { useMemo } from 'react';
import { Area, AreaChart, CartesianGrid, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { densityCurve, tailProbability, type ModelInput } from '@/lib/model';
import { formatCompactCurrency, formatCurrency, formatPercent } from '@/lib/utils';

/** Round tick values (1/2/5 × 10^k steps) inside [lo, hi]. */
function niceTicks(lo: number, hi: number, target: number): number[] {
  const raw = (hi - lo) / Math.max(1, target - 1);
  if (!Number.isFinite(raw) || raw <= 0) return [];
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) out.push(Math.round(v / step) * step);
  return out;
}

export function LossDistributionChart({ input, varValue, cvarValue, confidenceLabel }: { input: ModelInput; varValue: number; cvarValue: number | null; confidenceLabel: string }) {
  const data = useMemo(() => densityCurve(input, varValue, 180), [input, varValue]);
  const ticks = useMemo(() => niceTicks(data[0]?.loss ?? 0, data[data.length - 1]?.loss ?? 1, 7), [data]);

  if (!(input.value > 0)) {
    return <div className="flex h-full items-center justify-center text-sm text-slate-500">The loss density needs a positive net portfolio value.</div>;
  }

  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={data} margin={{ top: 24, right: 12, bottom: 4, left: 4 }}>
        <defs>
          <linearGradient id="ld-body" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#22D3EE" stopOpacity={0.35} />
            <stop offset="100%" stopColor="#22D3EE" stopOpacity={0.02} />
          </linearGradient>
          <linearGradient id="ld-tail" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#F43F5E" stopOpacity={0.75} />
            <stop offset="100%" stopColor="#F43F5E" stopOpacity={0.08} />
          </linearGradient>
        </defs>
        <CartesianGrid stroke="rgba(255,255,255,0.04)" vertical={false} />
        <XAxis
          dataKey="loss"
          type="number"
          domain={['dataMin', 'dataMax']}
          tickFormatter={(v: number) => formatCompactCurrency(v)}
          tick={{ fill: '#64748B', fontSize: 11 }}
          tickLine={false}
          axisLine={{ stroke: 'rgba(255,255,255,0.08)' }}
          ticks={ticks}
        />
        <YAxis hide domain={[0, 'auto']} />
        <Tooltip
          cursor={{ stroke: 'rgba(255,255,255,0.25)', strokeDasharray: '3 3' }}
          content={({ active, label }) => {
            if (!active || typeof label !== 'number') return null;
            const p = tailProbability(input, label);
            return (
              <div className="rounded-lg border border-white/10 bg-obsidian-900/95 px-3 py-2 font-mono text-[11px] shadow-xl backdrop-blur">
                <p className={label >= varValue ? 'text-loss' : 'text-slate-200'}>{label < 0 ? `gain ${formatCurrency(-label)}` : `loss ${formatCurrency(label)}`}</p>
                <p className="mt-0.5 text-slate-500">P(loss ≥ this) = {formatPercent(p, p < 0.01 ? 3 : 2)}</p>
              </div>
            );
          }}
        />
        <Area type="monotone" dataKey="body" stroke="#22D3EE" strokeWidth={1.75} fill="url(#ld-body)" isAnimationActive={false} connectNulls={false} />
        <Area type="monotone" dataKey="tail" stroke="#F43F5E" strokeWidth={1.75} fill="url(#ld-tail)" isAnimationActive={false} connectNulls={false} />
        <ReferenceLine x={0} stroke="rgba(255,255,255,0.15)" />
        <ReferenceLine x={varValue} stroke="#F43F5E" strokeDasharray="4 3" label={{ value: `VaR ${confidenceLabel}`, position: 'top', fill: '#F43F5E', fontSize: 11 }} />
        {cvarValue !== null && <ReferenceLine x={cvarValue} stroke="#FDA4AF" strokeDasharray="2 3" label={{ value: 'ES', position: 'top', fill: '#FDA4AF', fontSize: 11 }} />}
      </AreaChart>
    </ResponsiveContainer>
  );
}
