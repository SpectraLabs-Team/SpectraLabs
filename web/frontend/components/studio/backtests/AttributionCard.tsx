'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { Layers } from 'lucide-react';
import { ApiError } from '@/lib/api';
import { getAttribution } from '@/lib/backtests';
import { hasNoFills, sharePct, sortByShare, waterfallBars, type Attribution, type AttributionDirection } from '@/lib/attribution';
import { GlassCard, Spinner, ErrorNote } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { cash, dash, int, pct } from './format';

/** Where the numbers come from, for the panel's caption. Backtests are simulated; paper is a replay of fills. */
type Source = 'backtest' | 'paper';

function signedCash(v: number, currency?: string): string {
  const s = cash(Math.abs(v));
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  return `${sign}${currency ? `${currency} ` : ''}${s}`;
}

function toneClass(v: number): string {
  return v > 0 ? 'text-profit' : v < 0 ? 'text-loss' : 'text-ink/90';
}

function Block({ title, children, className }: { title: string; children: ReactNode; className?: string }) {
  return (
    <section className={cn('space-y-2', className)}>
      <h4 className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink/45">{title}</h4>
      {children}
    </section>
  );
}

function Waterfall({ a, currency }: { a: Attribution; currency?: string }) {
  const bars = waterfallBars(a.waterfall);
  return (
    <Block title="Waterfall">
      <div className="overflow-x-auto">
        <table className="w-full min-w-[520px] text-[12px]">
          <caption className="sr-only">P&amp;L waterfall from gross trading P&amp;L to net account P&amp;L</caption>
          <thead>
            <tr className="text-left text-ink/45">
              <th scope="col" className="py-1.5 pr-3 font-medium">Step</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Change</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Running total</th>
              <th scope="col" className="w-[40%] py-1.5 font-medium">Bridge</th>
            </tr>
          </thead>
          <tbody>
            {bars.map((b) => (
              <tr key={b.name} className={cn('border-t border-ink/[0.05]', b.subtotal && 'font-semibold')}>
                <th scope="row" className="py-2 pr-3 text-left font-normal text-ink/80">{b.name}</th>
                <td className={cn('num py-2 pr-3 text-right', b.subtotal ? 'text-ink' : toneClass(b.amount))}>
                  {b.subtotal ? dash : signedCash(b.amount)}
                </td>
                <td className={cn('num py-2 pr-3 text-right', toneClass(b.running_total))}>{signedCash(b.running_total, currency)}</td>
                <td className="py-2">
                  <div className="relative h-3 w-full rounded bg-ink/[0.04]" aria-hidden="true">
                    <div
                      className={cn(
                        'absolute top-0 h-3 rounded',
                        b.tone === 'profit' ? 'bg-profit/70' : b.tone === 'loss' ? 'bg-loss/70' : 'bg-ink/30',
                        b.subtotal && 'bg-accent/70',
                      )}
                      style={{ left: `${b.left * 100}%`, width: `${Math.max(b.width * 100, 0.5)}%` }}
                    />
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Block>
  );
}

function Instruments({ a, currency }: { a: Attribution; currency?: string }) {
  const rows = sortByShare(a.instruments);
  if (rows.length === 0) return <p className="text-[12px] text-ink/50">No closed trades, so no instrument has a result yet.</p>;
  return (
    <Block title="Instruments">
      <div className="overflow-x-auto">
        <table className="w-full min-w-[640px] text-[12px]">
          <thead>
            <tr className="text-left text-ink/45">
              <th scope="col" className="py-1.5 pr-3 font-medium">Instrument</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Trades</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Win rate</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Gross</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Fees</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Net</th>
              <th scope="col" className="py-1.5 text-right font-medium">Share of |net|</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.instrument} className="border-t border-ink/[0.05]">
                <th scope="row" className="py-2 pr-3 text-left font-medium text-ink/85">{r.symbol ?? `#${r.instrument}`}</th>
                <td className="num py-2 pr-3 text-right">{int(r.trades_count)}</td>
                <td className="num py-2 pr-3 text-right">{pct(r.win_rate, 0)}</td>
                <td className={cn('num py-2 pr-3 text-right', toneClass(r.gross_pnl))}>{signedCash(r.gross_pnl)}</td>
                <td className="num py-2 pr-3 text-right">{cash(r.fees)}</td>
                <td className={cn('num py-2 pr-3 text-right font-semibold', toneClass(r.net_pnl))}>{signedCash(r.net_pnl, currency)}</td>
                <td className="num py-2 text-right">{sharePct(r.pnl_share)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Block>
  );
}

function Side({ label, trades, wins, winRate, gross, fees, net }: { label: string; trades: number; wins: number; winRate: number; gross: number; fees: number; net: number }) {
  return (
    <div className="rounded-xl border border-ink/[0.07] bg-ink/[0.02] px-3.5 py-3">
      <p className="text-[10.5px] font-medium uppercase tracking-wider text-ink/45">{label}</p>
      <p className={cn('num mt-1 text-lg font-semibold', toneClass(net))}>{signedCash(net)}</p>
      <dl className="mt-2 space-y-1 text-[11.5px]">
        <div className="flex justify-between"><dt className="text-ink/55">Trades</dt><dd className="num">{int(trades)} ({int(wins)} won, {pct(winRate, 0)})</dd></div>
        <div className="flex justify-between"><dt className="text-ink/55">Gross</dt><dd className={cn('num', toneClass(gross))}>{signedCash(gross)}</dd></div>
        <div className="flex justify-between"><dt className="text-ink/55">Fees</dt><dd className="num">{cash(fees)}</dd></div>
      </dl>
    </div>
  );
}

function LongShort({ d, currency }: { d: AttributionDirection; currency?: string }) {
  return (
    <Block title="Long / short">
      <div className="grid gap-2.5 sm:grid-cols-2">
        <Side label="Long" trades={d.long_trades} wins={d.long_wins} winRate={d.long_win_rate} gross={d.long_gross_pnl} fees={d.long_fees} net={d.long_net_pnl} />
        <Side label="Short" trades={d.short_trades} wins={d.short_wins} winRate={d.short_win_rate} gross={d.short_gross_pnl} fees={d.short_fees} net={d.short_net_pnl} />
      </div>
      <p className="text-[11px] text-ink/45">Net of both sides: {signedCash(d.long_net_pnl + d.short_net_pnl, currency)}</p>
    </Block>
  );
}

function Costs({ a, currency }: { a: Attribution; currency?: string }) {
  const c = a.costs;
  const rows: [string, number, string?][] = [
    ['Commissions', -c.commission_fees],
    ['Borrow fees', -c.borrow_fees],
    ['Financing interest', -c.financing_interest],
    ['Interest income', c.interest_income],
    ['Dividends (in trade gross)', c.dividends, 'memo'],
    ['Coupons (in trade gross)', c.coupons, 'memo'],
    ['Slippage estimate (in fill prices)', c.slippage_costs, 'memo'],
  ];
  return (
    <Block title="Costs">
      <dl className="grid gap-x-6 gap-y-1.5 text-[12px] sm:grid-cols-2">
        {rows.map(([label, v, memo]) => (
          <div key={label} className={cn('flex justify-between gap-3', memo && 'text-ink/45')}>
            <dt>{label}</dt>
            <dd className="num">{v === 0 ? dash : memo ? cash(v) : signedCash(v, currency)}</dd>
          </div>
        ))}
      </dl>
      <p className="text-[11px] text-ink/45">
        Charged in total: {cash(c.total_costs)}. Slippage is already in the fill prices, so it is not charged again.
      </p>
    </Block>
  );
}

/**
 * The P&L attribution of a backtest or paper account: waterfall, instruments, long/short split and
 * costs. The numbers come from the engine's attribution endpoint; this card only lays them out.
 */
export function AttributionCard({ a, source, currency, asOf }: { a: Attribution; source: Source; currency?: string; asOf?: string }) {
  const caption = source === 'backtest' ? 'Simulated from this backtest’s fills. Not a live result.' : 'Replayed from this account’s fills.';
  return (
    <GlassCard as="section" className="overflow-hidden">
      <div className="border-b border-ink/[0.06] px-5 py-4">
        <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Layers className="h-4 w-4 text-accent" /> P&amp;L attribution
          {source === 'backtest' && <span className="rounded bg-caution/15 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-caution">Simulated</span>}
        </h3>
        <p className="mt-0.5 text-xs text-ink/50">
          Where the result came from. {caption}
          {asOf && <> As of {asOf}.</>}
          {currency && <> Values in {currency}.</>}
        </p>
      </div>
      <div className="space-y-6 p-5">
        {hasNoFills(a) ? (
          <p className="text-[12.5px] text-ink/55">{source === 'paper' ? 'No fills yet, so there is nothing to attribute.' : 'This run has no fills, so there is nothing to attribute.'}</p>
        ) : (
          <>
            <Waterfall a={a} currency={currency} />
            <Instruments a={a} currency={currency} />
            <LongShort d={a.direction} currency={currency} />
            <Costs a={a} currency={currency} />
          </>
        )}
      </div>
    </GlassCard>
  );
}

/** Loads a finished backtest's attribution and shows the card, or a loading or error note. Mount it with `key` set to the run id. */
export function AttributionPanel({ backtestId, currency }: { backtestId: string; currency?: string }) {
  const [state, setState] = useState<{ status: 'loading' } | { status: 'ready'; a: Attribution } | { status: 'error'; message: string }>({ status: 'loading' });

  useEffect(() => {
    const ctl = new AbortController();
    getAttribution(backtestId)
      .then((a) => {
        if (!ctl.signal.aborted) setState({ status: 'ready', a });
      })
      .catch((e) => {
        if (ctl.signal.aborted) return;
        const message = e instanceof ApiError && e.status === 409 ? 'The attribution appears once this backtest has finished.' : e instanceof ApiError && e.status === 404 ? 'This run has no attribution recorded.' : e instanceof Error ? e.message : 'Could not load the attribution.';
        setState({ status: 'error', message });
      });
    return () => ctl.abort();
  }, [backtestId]);

  if (state.status === 'loading') {
    return (
      <GlassCard as="section" className="flex items-center gap-2 p-5 text-[12px] text-ink/55">
        <Spinner className="h-3.5 w-3.5" /> Loading P&amp;L attribution…
      </GlassCard>
    );
  }
  if (state.status === 'error') return <ErrorNote>{state.message}</ErrorNote>;
  return <AttributionCard a={state.a} source="backtest" currency={currency} />;
}
