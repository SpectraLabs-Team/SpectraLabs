'use client';

import { useEffect, useMemo, useState } from 'react';
import { Area, Bar, CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { CandlestickChart } from 'lucide-react';
import { getBars, type Bars } from '@/lib/datasets';
import { chartColors, useTheme } from '@/lib/theme';
import { ErrorNote, Segmented, Spinner } from '@/components/ui/primitives';
import { axisDate, compactCash, fullDate } from '../backtests/format';
import { int } from './format';

const tick = { fill: '#64748B', fontSize: 11 };

/** Close (with the high–low range shaded) and volume for one symbol of a dataset version. */
export function PriceChart({ versionId, symbols, chartable }: { versionId: string; symbols: string[]; chartable: boolean }) {
  const cc = chartColors(useTheme().theme);
  const [symbol, setSymbol] = useState(symbols[0] ?? '');
  const [bars, setBars] = useState<Bars | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  useEffect(() => setSymbol(symbols[0] ?? ''), [versionId, symbols]);

  useEffect(() => {
    if (!chartable || !symbol) return;
    let live = true;
    setLoading(true);
    setError('');
    getBars(versionId, { symbol, points: 800 })
      .then((b) => live && setBars(b))
      .catch((e) => live && setError(e instanceof Error ? e.message : 'Could not load the bars.'))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [versionId, symbol, chartable]);

  const rows = useMemo(() => (bars?.points ?? []).map((p) => ({ t: p.t, close: p.close, band: [p.low, p.high] as [number, number], volume: p.volume })), [bars]);
  const span = rows.length > 1 ? rows[rows.length - 1].t - rows[0].t : 0;

  if (!chartable) {
    return (
      <div className="flex h-[260px] flex-col items-center justify-center gap-2 text-center text-[12px] text-ink/50">
        <CandlestickChart className="h-5 w-5 text-ink/30" />
        This version was added from the command line and its bars aren’t stored where the API can read them.
      </div>
    );
  }
  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        {symbols.length > 1 && symbols.length <= 8 ? (
          <Segmented label="chart-symbol" size="sm" value={symbol} onChange={setSymbol} options={symbols.map((s) => ({ value: s, label: s }))} />
        ) : symbols.length > 8 ? (
          <select className="input w-44 py-1 text-[12px]" value={symbol} onChange={(e) => setSymbol(e.target.value)} aria-label="Symbol">
            {symbols.map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
        ) : (
          <span className="text-[12px] font-medium text-ink/70">{symbol}</span>
        )}
        {bars && (
          <span className="num text-[11px] text-ink/45">
            {int(bars.total_bars)} bars{bars.points.length < bars.total_bars ? ` · ${int(bars.points.length)} points` : ''}
          </span>
        )}
      </div>
      {error ? (
        <ErrorNote>{error}</ErrorNote>
      ) : loading && !bars ? (
        <div className="flex h-[300px] items-center justify-center">
          <Spinner />
        </div>
      ) : (
        <div className="h-[300px]" aria-label={`${symbol} price chart`}>
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={rows} margin={{ top: 6, right: 8, bottom: 0, left: 0 }}>
              <CartesianGrid stroke={cc.grid} vertical={false} />
              <XAxis dataKey="t" type="number" scale="time" domain={['dataMin', 'dataMax']} tickFormatter={(t: number) => axisDate(t, span)} tick={tick} tickLine={false} axisLine={{ stroke: cc.axis }} minTickGap={48} />
              <YAxis yAxisId="p" domain={['auto', 'auto']} tickFormatter={(v: number) => compactCash(v)} tick={tick} tickLine={false} axisLine={false} width={56} />
              <YAxis yAxisId="v" orientation="right" hide domain={[0, (max: number) => max * 4]} />
              <Tooltip
                contentStyle={cc.tooltip}
                labelFormatter={(t) => fullDate(Number(t))}
                formatter={(v, name) => {
                  if (name === 'band' && Array.isArray(v)) return [`${Number(v[0]).toLocaleString('en-US')} – ${Number(v[1]).toLocaleString('en-US')}`, 'Low – high'];
                  return [Number(v).toLocaleString('en-US', { maximumFractionDigits: 8 }), name === 'close' ? 'Close' : 'Volume'];
                }}
              />
              <Bar yAxisId="v" dataKey="volume" fill={cc.muted} opacity={0.35} isAnimationActive={false} />
              <Area yAxisId="p" dataKey="band" stroke="none" fill={cc.accent} fillOpacity={0.12} isAnimationActive={false} />
              <Line yAxisId="p" dataKey="close" stroke={cc.accent} strokeWidth={1.6} dot={false} isAnimationActive={false} />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      )}
    </div>
  );
}
