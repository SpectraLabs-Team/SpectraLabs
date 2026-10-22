'use client';

import { useMemo } from 'react';
import { Area, AreaChart, CartesianGrid, Legend, Line, LineChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { chartColors, useTheme } from '@/lib/theme';
import { nsToMs, type Series, type StrategyCurve } from '@/lib/backtests';
import { axisDate, compactCash, fullDate, pct } from './format';

const tick = { fill: '#64748B', fontSize: 11 };

function useRows(series: Series | null) {
  return useMemo(() => (series ? series.points.filter((p) => p[1] !== null).map(([ts, v]) => ({ t: nsToMs(ts), v: v as number })) : []), [series]);
}

const span = (rows: { t: number }[]) => (rows.length > 1 ? rows[rows.length - 1].t - rows[0].t : 0);

/** Equity over time, with the starting cash as a reference line. */
export function EquityChart({ series, start, height = 260 }: { series: Series | null; start?: number; height?: number }) {
  const cc = chartColors(useTheme().theme);
  const rows = useRows(series);
  const s = span(rows);
  if (!rows.length) return <div style={{ height }} className="flex items-center justify-center text-xs text-ink/45">No equity series.</div>;
  return (
    <div style={{ height }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={rows} margin={{ top: 6, right: 8, bottom: 0, left: 0 }}>
          <defs>
            <linearGradient id="bt-equity" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={cc.accent} stopOpacity={0.28} />
              <stop offset="100%" stopColor={cc.accent} stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid stroke={cc.grid} vertical={false} />
          <XAxis dataKey="t" type="number" scale="time" domain={['dataMin', 'dataMax']} tickFormatter={(t: number) => axisDate(t, s)} tick={tick} tickLine={false} axisLine={{ stroke: cc.axis }} minTickGap={48} />
          <YAxis domain={['auto', 'auto']} tickFormatter={(v: number) => compactCash(v)} tick={tick} tickLine={false} axisLine={false} width={56} />
          <Tooltip contentStyle={cc.tooltip} labelFormatter={(t) => fullDate(Number(t))} formatter={(v) => [Number(v).toLocaleString('en-US', { maximumFractionDigits: 2 }), 'Equity']} />
          {start !== undefined && Number.isFinite(start) && <ReferenceLine y={start} stroke={cc.muted} strokeDasharray="4 4" />}
          <Area type="monotone" dataKey="v" stroke={cc.accent} strokeWidth={1.8} fill="url(#bt-equity)" isAnimationActive={false} />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

/** Drawdown from the running peak, drawn downwards. */
export function DrawdownChart({ series, height = 150 }: { series: Series | null; height?: number }) {
  const cc = chartColors(useTheme().theme);
  const rows = useRows(series).map((r) => ({ t: r.t, v: -r.v }));
  const s = span(rows);
  if (!rows.length) return <div style={{ height }} className="flex items-center justify-center text-xs text-ink/45">No drawdown series.</div>;
  return (
    <div style={{ height }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={rows} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
          <defs>
            <linearGradient id="bt-dd" x1="0" y1="1" x2="0" y2="0">
              <stop offset="0%" stopColor={cc.loss} stopOpacity={0.35} />
              <stop offset="100%" stopColor={cc.loss} stopOpacity={0.02} />
            </linearGradient>
          </defs>
          <CartesianGrid stroke={cc.grid} vertical={false} />
          <XAxis dataKey="t" type="number" scale="time" domain={['dataMin', 'dataMax']} tickFormatter={(t: number) => axisDate(t, s)} tick={tick} tickLine={false} axisLine={{ stroke: cc.axis }} minTickGap={48} />
          <YAxis domain={['auto', 0]} tickFormatter={(v: number) => pct(v, 0)} tick={tick} tickLine={false} axisLine={false} width={56} />
          <Tooltip contentStyle={cc.tooltip} labelFormatter={(t) => fullDate(Number(t))} formatter={(v) => [pct(Number(v)), 'Drawdown']} />
          <Area type="monotone" dataKey="v" stroke={cc.loss} strokeWidth={1.4} fill="url(#bt-dd)" isAnimationActive={false} />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

/** Each strategy's own equity, as a share of its allocation (1.0 = where it started). */
export function StrategyCurvesChart({ curves, names, height = 240 }: { curves: StrategyCurve[]; names: string[]; height?: number }) {
  const cc = chartColors(useTheme().theme);
  const palette = [cc.accent, cc.violet, '#F59E0B', '#10B981', '#F472B6', '#60A5FA', '#F97316', '#A3E635', '#E879F9', '#94A3B8'];
  const { rows, s } = useMemo(() => {
    // Strategies share timestamps, so rows line up by index.
    const n = Math.max(0, ...curves.map((c) => c.points.length));
    const out: Record<string, number>[] = [];
    for (let i = 0; i < n; i++) {
      const row: Record<string, number> = {};
      for (const c of curves) {
        const p = c.points[i];
        const first = c.points[0]?.[1];
        if (!p) continue;
        row.t = nsToMs(p[0]);
        if (first && first > 0) row[`s${c.strategy}`] = p[1] / first;
      }
      out.push(row);
    }
    return { rows: out, s: out.length > 1 ? out[out.length - 1].t - out[0].t : 0 };
  }, [curves]);
  if (!rows.length) return null;
  return (
    <div style={{ height }}>
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={rows} margin={{ top: 6, right: 8, bottom: 0, left: 0 }}>
          <CartesianGrid stroke={cc.grid} vertical={false} />
          <XAxis dataKey="t" type="number" scale="time" domain={['dataMin', 'dataMax']} tickFormatter={(t: number) => axisDate(t, s)} tick={tick} tickLine={false} axisLine={{ stroke: cc.axis }} minTickGap={48} />
          <YAxis domain={['auto', 'auto']} tickFormatter={(v: number) => `${((v - 1) * 100).toFixed(0)}%`} tick={tick} tickLine={false} axisLine={false} width={48} />
          <Tooltip contentStyle={cc.tooltip} labelFormatter={(t) => fullDate(Number(t))} formatter={(v, name) => [pct(Number(v) - 1), String(name)]} />
          <Legend iconType="circle" wrapperStyle={{ fontSize: 11, color: cc.tick }} />
          <ReferenceLine y={1} stroke={cc.muted} strokeDasharray="4 4" />
          {curves.map((c, i) => (
            <Line key={c.strategy} type="monotone" dataKey={`s${c.strategy}`} name={`#${c.strategy + 1} ${names[c.strategy] ?? ''}`} stroke={palette[i % palette.length]} strokeWidth={1.6} dot={false} isAnimationActive={false} />
          ))}
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}

export { MonthlyHeatmap } from './MonthlyHeatmap';
export type { MonthlyHeatmapProps } from './MonthlyHeatmap';
