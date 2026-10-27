'use client';

import { useId, useMemo, useState } from 'react';
import { Calculator } from 'lucide-react';
import { commission, drawdownStats, effectiveBps, equityMetrics, parseNumbers, returnsFromEquity, tradeStats } from '@/lib/learn/calc';
import type { InteractiveId } from '@/lib/learn/types';
import { cn } from '@/lib/utils';

const pct = (v: number, d = 2) => (Number.isFinite(v) ? `${(v * 100).toFixed(d)}%` : '—');
const num = (v: number | null, d = 2) => (v === null || !Number.isFinite(v) ? '—' : v.toFixed(d));
const usd = (v: number, d = 2) => (Number.isFinite(v) ? v.toLocaleString('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: d, maximumFractionDigits: d }) : '—');

function Panel({ title, hint, children }: { title: string; hint: string; children: React.ReactNode }) {
  return (
    <section className="my-6 rounded-xl border border-accent/25 bg-accent/[0.035] p-4 sm:p-5" aria-label={title}>
      <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
        <Calculator className="h-4 w-4 text-accent" aria-hidden /> {title}
      </h3>
      <p className="mt-1 text-xs leading-5 text-ink/55">{hint}</p>
      <div className="mt-4">{children}</div>
    </section>
  );
}

function Field({ label, value, onChange, step = 'any', suffix }: { label: string; value: number; onChange: (v: number) => void; step?: string; suffix?: string }) {
  const id = useId();
  return (
    <div>
      <label htmlFor={id} className="mb-1 block text-[11px] font-medium text-ink/60">
        {label}
      </label>
      <div className="flex items-center gap-1.5">
        <input id={id} type="number" inputMode="decimal" step={step} value={Number.isFinite(value) ? value : ''} onChange={(e) => onChange(e.target.value === '' ? NaN : Number(e.target.value))} className="input num py-1.5 text-[13px]" />
        {suffix && <span className="shrink-0 text-[11px] text-ink/45">{suffix}</span>}
      </div>
    </div>
  );
}

function Results({ rows }: { rows: [string, string, ('profit' | 'loss')?][] }) {
  return (
    <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3" aria-live="polite">
      {rows.map(([k, v, t]) => (
        <div key={k} className="rounded-lg border border-ink/[0.08] bg-panel/60 px-3 py-2">
          <dt className="text-[10.5px] uppercase tracking-wider text-ink/45">{k}</dt>
          <dd className={cn('num mt-0.5 text-[15px] font-semibold', t === 'profit' ? 'text-profit' : t === 'loss' ? 'text-loss' : 'text-ink')}>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

function EquityMetricsWidget() {
  const id = useId();
  const [text, setText] = useState('100000, 101200, 100400, 102900, 104100, 102300, 105600, 106900, 105200, 108300, 110100, 109000, 111800');
  const [ppy, setPpy] = useState(12);
  const [rf, setRf] = useState(0);
  const eq = useMemo(() => parseNumbers(text), [text]);
  const m = useMemo(() => equityMetrics(eq, ppy, rf / 100), [eq, ppy, rf]);
  const dd = useMemo(() => drawdownStats(eq), [eq]);
  const nReturns = returnsFromEquity(eq).length;
  return (
    <Panel title="Try it: metrics from an equity curve" hint="Month-end equity for a year (periods per year 12). Edit the values or the settings: the numbers use the same formulas as the backtest engine.">
      <label htmlFor={id} className="mb-1 block text-[11px] font-medium text-ink/60">
        Equity per bar
      </label>
      <textarea id={id} rows={2} value={text} onChange={(e) => setText(e.target.value)} className="input num resize-y text-[13px]" />
      <div className="mt-3 grid max-w-md grid-cols-2 gap-3">
        <Field label="Periods per year" value={ppy} onChange={setPpy} step="1" />
        <Field label="Risk-free rate" value={rf} onChange={setRf} suffix="% / yr" />
      </div>
      <div className="mt-4">
        {m.valid ? (
          <Results
            rows={[
              ['Total return', pct(m.totalReturn), m.totalReturn >= 0 ? 'profit' : 'loss'],
              ['Annualized return', pct(m.annualizedReturn)],
              ['Annualized volatility', pct(m.annualizedVolatility)],
              ['Sharpe ratio', num(m.sharpe)],
              ['Sortino ratio', num(m.sortino)],
              ['Max drawdown', pct(m.maxDrawdown), m.maxDrawdown > 0 ? 'loss' : undefined],
              ['Current drawdown', pct(dd.currentDrawdown)],
              ['Longest underwater', `${dd.longestUnderwater} bars`],
              ['Ulcer index', num(dd.ulcerIndex, 4)],
            ]}
          />
        ) : (
          <p className="text-xs text-ink/55">Enter at least two equity values.</p>
        )}
        {m.valid && nReturns < 30 && <p className="mt-2 text-[11px] text-ink/45">With only {nReturns} returns, annualized figures are extrapolations, not estimates.</p>}
      </div>
    </Panel>
  );
}

function TradeStatsWidget() {
  const id = useId();
  const [text, setText] = useState('400, -100, 350, -150, -120, 250, -80, -90, 500, -60');
  const pnl = useMemo(() => parseNumbers(text), [text]);
  const s = useMemo(() => tradeStats(pnl), [pnl]);
  const breakEven = s.payoffRatio > 0 ? 1 / (1 + s.payoffRatio) : NaN;
  return (
    <Panel title="Try it: trade statistics" hint="Enter the net P&L of each closed trade (negative for losses). Computed the way the engine computes trade statistics.">
      <label htmlFor={id} className="mb-1 block text-[11px] font-medium text-ink/60">
        Net P&amp;L per trade ($)
      </label>
      <textarea id={id} rows={2} value={text} onChange={(e) => setText(e.target.value)} className="input num resize-y text-[13px]" />
      <div className="mt-4">
        {s.n > 0 ? (
          <Results
            rows={[
              ['Trades', String(s.n)],
              ['Win rate', pct(s.winRate, 1)],
              ['Profit factor', Number.isFinite(s.profitFactor) ? num(s.profitFactor) : '—'],
              ['Payoff ratio', s.payoffRatio ? num(s.payoffRatio) : '—'],
              ['Expectancy', usd(s.expectancy), s.expectancy >= 0 ? 'profit' : 'loss'],
              ['Break-even win rate', pct(breakEven, 1)],
              ['Average win', usd(s.avgWin)],
              ['Average loss', usd(s.avgLoss)],
              ['Total net', usd(s.totalNet), s.totalNet >= 0 ? 'profit' : 'loss'],
            ]}
          />
        ) : (
          <p className="text-xs text-ink/55">Enter at least one trade.</p>
        )}
      </div>
    </Panel>
  );
}

function SpreadCostWidget() {
  const [price, setPrice] = useState(50);
  const [qty, setQty] = useState(100);
  const [slip, setSlip] = useState(2);
  const [spread, setSpread] = useState(4);
  const [volSlip, setVolSlip] = useState(10);
  const [barVol, setBarVol] = useState(20000);
  const [commBps, setCommBps] = useState(1);
  const [minFee, setMinFee] = useState(1);
  const bps = effectiveBps(slip, spread, volSlip, qty, barVol);
  const fill = price * (1 + bps / 10_000);
  const notional = fill * qty;
  const fee = commission(0, commBps, minFee, qty, notional);
  const slipCost = (fill - price) * qty;
  const total = slipCost + fee;
  return (
    <Panel title="Try it: the cost of one market buy" hint="Same model as a backtest: flat slippage + half the spread + volume-based slippage on the fill price, then commission.">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field label="Open price" value={price} onChange={setPrice} suffix="$" />
        <Field label="Quantity" value={qty} onChange={setQty} step="1" />
        <Field label="Slippage" value={slip} onChange={setSlip} suffix="bps" />
        <Field label="Spread" value={spread} onChange={setSpread} suffix="bps" />
        <Field label="Volume slippage" value={volSlip} onChange={setVolSlip} suffix="bps" />
        <Field label="Bar volume" value={barVol} onChange={setBarVol} step="1" />
        <Field label="Commission" value={commBps} onChange={setCommBps} suffix="bps" />
        <Field label="Minimum fee" value={minFee} onChange={setMinFee} suffix="$" />
      </div>
      <div className="mt-4">
        <Results
          rows={[
            ['Effective slippage', `${num(bps, 2)} bps`],
            ['Fill price', usd(fill, 4)],
            ['Slippage cost', usd(slipCost), 'loss'],
            ['Commission', usd(fee), 'loss'],
            ['Total cost', usd(total), 'loss'],
            ['Cost / notional', `${num((total / (price * qty)) * 10_000, 2)} bps`],
          ]}
        />
      </div>
    </Panel>
  );
}

function PositionSizingWidget() {
  const [equity, setEquity] = useState(100_000);
  const [risk, setRisk] = useState(0.5);
  const [price, setPrice] = useState(50);
  const [vol, setVol] = useState(2);
  const value = vol > 0 ? (equity * (risk / 100)) / (vol / 100) : NaN;
  const shares = Math.floor(value / price);
  return (
    <Panel title="Try it: volatility-based position sizing" hint="Hold less of a volatile stock so that a typical one-day move costs the same share of equity.">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field label="Equity" value={equity} onChange={setEquity} suffix="$" />
        <Field label="Target daily risk" value={risk} onChange={setRisk} suffix="% of equity" />
        <Field label="Share price" value={price} onChange={setPrice} suffix="$" />
        <Field label="Daily volatility" value={vol} onChange={setVol} suffix="%" />
      </div>
      <div className="mt-4">
        <Results
          rows={[
            ['Position value', usd(value, 0)],
            ['Shares', Number.isFinite(shares) ? shares.toLocaleString('en-US') : '—'],
            ['Share of equity', pct(value / equity, 1)],
            ['Typical daily move', usd(value * (vol / 100), 0)],
            ['Annualized vol', pct((vol / 100) * Math.sqrt(252), 1)],
          ]}
        />
      </div>
    </Panel>
  );
}

const WIDGETS: Record<InteractiveId, () => React.JSX.Element> = {
  'equity-metrics': EquityMetricsWidget,
  'trade-stats': TradeStatsWidget,
  'spread-cost': SpreadCostWidget,
  'position-sizing': PositionSizingWidget,
};

export function Interactive({ id }: { id: InteractiveId }) {
  const W = WIDGETS[id];
  return <W />;
}
