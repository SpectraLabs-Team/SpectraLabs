'use client';

import React, { useMemo, useState } from 'react';
import {
  Area,
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import {
  ArrowDownRight,
  ArrowUpRight,
  BarChart2,
  CandlestickChart,
  DollarSign,
  TrendingUp,
} from 'lucide-react';
import { GlassCard } from '@/components/ui/primitives';
import { cn, formatCurrency } from '@/lib/utils';

export interface MarketChartProps {
  currentSymbol: string;
  onSelectSymbol: (symbol: string, price: number) => void;
  onQuickOrder?: (symbol: string, price: number, side: 'buy' | 'sell') => void;
}

const POPULAR_SYMBOLS = [
  { symbol: 'AAPL', name: 'Apple Inc.', basePrice: 182.5, changePct: 1.45 },
  { symbol: 'SPY', name: 'SPDR S&P 500 ETF', basePrice: 512.2, changePct: 0.82 },
  { symbol: 'NVDA', name: 'NVIDIA Corporation', basePrice: 128.4, changePct: 3.12 },
  { symbol: 'MSFT', name: 'Microsoft Corp.', basePrice: 425.8, changePct: -0.45 },
  { symbol: 'TSLA', name: 'Tesla Inc.', basePrice: 218.6, changePct: -1.88 },
  { symbol: 'QQQ', name: 'Invesco QQQ Trust', basePrice: 446.3, changePct: 1.15 },
];

function generateCandles(basePrice: number, points = 30) {
  const candles = [];
  let price = basePrice * 0.92;
  const now = Date.now();
  const step = 24 * 3600 * 1000;

  for (let i = points; i >= 0; i--) {
    const t = now - i * step;
    const vol = 0.015;
    const change = price * (Math.sin(i * 0.4) * 0.01 + (Math.random() - 0.48) * vol);
    const close = Math.max(1, price + change);
    const open = price;
    const high = Math.max(open, close) + Math.random() * price * 0.008;
    const low = Math.min(open, close) - Math.random() * price * 0.008;
    const volume = Math.floor(100_000 + Math.random() * 500_000);
    price = close;

    candles.push({
      t,
      date: new Date(t).toLocaleDateString('en-US', { month: 'short', day: 'numeric' }),
      open: Math.round(open * 100) / 100,
      high: Math.round(high * 100) / 100,
      low: Math.round(low * 100) / 100,
      close: Math.round(close * 100) / 100,
      band: [Math.round(low * 100) / 100, Math.round(high * 100) / 100],
      volume,
    });
  }
  return candles;
}

export function MarketChart({ currentSymbol, onSelectSymbol, onQuickOrder }: MarketChartProps) {
  const [symbol, setSymbol] = useState(currentSymbol || 'AAPL');
  const [timeframe, setTimeframe] = useState<'1D' | '1W' | '1M' | '1Y'>('1M');
  const [chartType, setChartType] = useState<'area' | 'candles'>('area');

  const activeMeta = useMemo(() => {
    return (
      POPULAR_SYMBOLS.find((s) => s.symbol === symbol) || {
        symbol,
        name: `${symbol} Equity`,
        basePrice: 150.0,
        changePct: 0.5,
      }
    );
  }, [symbol]);

  const data = useMemo(() => {
    const count = timeframe === '1D' ? 14 : timeframe === '1W' ? 20 : timeframe === '1M' ? 30 : 60;
    return generateCandles(activeMeta.basePrice, count);
  }, [activeMeta.basePrice, timeframe]);

  const lastCandle = data[data.length - 1];
  const firstCandle = data[0];
  const priceChange = lastCandle ? lastCandle.close - firstCandle.open : 0;
  const pctChange = firstCandle ? (priceChange / firstCandle.open) * 100 : 0;
  const isUp = priceChange >= 0;

  const handleSymbolChange = (sym: string) => {
    setSymbol(sym);
    const found = POPULAR_SYMBOLS.find((s) => s.symbol === sym);
    const p = found ? found.basePrice : 150.0;
    onSelectSymbol(sym, p);
  };

  return (
    <GlassCard className="overflow-hidden p-0">
      {/* Chart Header Bar */}
      <div className="border-b border-ink/10 p-4">
        <div className="flex flex-wrap items-center justify-between gap-4">
          {/* Symbol Selector Pills */}
          <div className="flex flex-wrap items-center gap-1.5">
            {POPULAR_SYMBOLS.map((s) => (
              <button
                key={s.symbol}
                type="button"
                onClick={() => handleSymbolChange(s.symbol)}
                className={cn(
                  'rounded-lg px-2.5 py-1 text-xs font-mono font-medium transition',
                  symbol === s.symbol
                    ? 'bg-accent/15 text-accent font-semibold ring-1 ring-accent/30'
                    : 'bg-ink/[0.03] text-ink/60 hover:bg-ink/[0.06] hover:text-ink'
                )}
              >
                {s.symbol}
              </button>
            ))}
          </div>

          {/* Timeframe & Chart Type */}
          <div className="flex items-center gap-2">
            <div className="flex rounded-lg border border-ink/10 bg-ink/[0.02] p-0.5 text-[11px] font-mono">
              {(['1D', '1W', '1M', '1Y'] as const).map((tf) => (
                <button
                  key={tf}
                  type="button"
                  onClick={() => setTimeframe(tf)}
                  className={cn(
                    'rounded px-2 py-0.5 transition',
                    timeframe === tf ? 'bg-ink/10 font-semibold text-ink' : 'text-ink/50 hover:text-ink'
                  )}
                >
                  {tf}
                </button>
              ))}
            </div>

            <button
              type="button"
              onClick={() => setChartType(chartType === 'area' ? 'candles' : 'area')}
              className="rounded-lg border border-ink/10 p-1.5 text-ink/60 hover:bg-ink/5 hover:text-ink"
              title="Toggle Chart Type"
            >
              {chartType === 'area' ? <CandlestickChart className="h-3.5 w-3.5" /> : <BarChart2 className="h-3.5 w-3.5" />}
            </button>
          </div>
        </div>

        {/* Live Quote Summary & Quick Action Buttons */}
        <div className="mt-3 flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="flex items-baseline gap-2.5">
              <span className="font-mono text-2xl font-bold tracking-tight text-ink">
                {lastCandle ? formatCurrency(lastCandle.close) : '—'}
              </span>
              <div
                className={cn(
                  'flex items-center font-mono text-xs font-semibold',
                  isUp ? 'text-profit' : 'text-loss'
                )}
              >
                {isUp ? <ArrowUpRight className="h-3.5 w-3.5" /> : <ArrowDownRight className="h-3.5 w-3.5" />}
                <span>
                  {isUp ? '+' : ''}
                  {priceChange.toFixed(2)} ({isUp ? '+' : ''}
                  {pctChange.toFixed(2)}%)
                </span>
              </div>
            </div>
            <p className="mt-0.5 text-[11.5px] text-ink/50">{activeMeta.name} · Real-time simulation mark</p>
          </div>

          {/* Direct Buy / Sell Actions */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onQuickOrder && lastCandle && onQuickOrder(symbol, lastCandle.close, 'buy')}
              className="rounded-lg bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white shadow-sm transition hover:bg-emerald-500"
            >
              Quick Buy {symbol}
            </button>
            <button
              type="button"
              onClick={() => onQuickOrder && lastCandle && onQuickOrder(symbol, lastCandle.close, 'sell')}
              className="rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white shadow-sm transition hover:bg-rose-500"
            >
              Quick Sell {symbol}
            </button>
          </div>
        </div>
      </div>

      {/* Interactive Chart Surface */}
      <div className="h-[260px] p-2">
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart data={data} margin={{ top: 10, right: 12, bottom: 0, left: -10 }}>
            <CartesianGrid strokeDasharray="3 3" opacity={0.15} vertical={false} />
            <XAxis dataKey="date" tick={{ fontSize: 10, fill: '#64748B' }} tickLine={false} axisLine={false} />
            <YAxis
              yAxisId="price"
              domain={['auto', 'auto']}
              tick={{ fontSize: 10, fill: '#64748B' }}
              tickLine={false}
              axisLine={false}
              tickFormatter={(v) => `$${v}`}
              width={54}
            />
            <YAxis yAxisId="volume" orientation="right" hide domain={[0, (max: number) => max * 4]} />
            <Tooltip
              contentStyle={{
                backgroundColor: 'rgba(15, 23, 42, 0.92)',
                border: '1px solid rgba(255, 255, 255, 0.12)',
                borderRadius: '8px',
                fontSize: '11px',
                color: '#F8FAFC',
              }}
              formatter={(val, name) => [
                name === 'volume' ? Number(val).toLocaleString() : `$${Number(val).toFixed(2)}`,
                name === 'close' ? 'Close Price' : name === 'volume' ? 'Volume' : name,
              ]}
              labelFormatter={(label) => `Date: ${label}`}
            />
            <Bar yAxisId="volume" dataKey="volume" fill="#0ea5e9" opacity={0.2} isAnimationActive={false} />
            <Area
              yAxisId="price"
              type="monotone"
              dataKey="band"
              stroke="none"
              fill="#0ea5e9"
              fillOpacity={0.08}
              isAnimationActive={false}
            />
            <Line
              yAxisId="price"
              type="monotone"
              dataKey="close"
              stroke="#0ea5e9"
              strokeWidth={2}
              dot={false}
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </GlassCard>
  );
}
