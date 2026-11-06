'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Layers, HelpCircle, BookOpen, X } from 'lucide-react';
import { ApiError, getWorkspacePaperAttribution } from '@/lib/api';
import { getAttribution } from '@/lib/backtests';
import { hasNoFills, sharePct, sortByShare, waterfallBars, type Attribution, type AttributionDirection, type WorkspaceAttribution } from '@/lib/attribution';
import { buildWaterfallCsv, buildInstrumentsCsv } from '@/lib/csvExport';
import { getGlossaryEntry, getAllGlossaryEntries } from '@/lib/attributionGlossary';
import { GlassCard, Spinner, ErrorNote } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { cash, dash, int, pct } from './format';

/** Where the numbers come from, for the panel's caption. Backtests are simulated; paper is a replay of fills. */
type Source = 'backtest' | 'paper' | 'workspace';

function signedCash(v: number, currency?: string): string {
  const s = cash(Math.abs(v));
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  return `${sign}${currency ? `${currency} ` : ''}${s}`;
}

function toneClass(v: number): string {
  return v > 0 ? 'text-profit' : v < 0 ? 'text-loss' : 'text-ink/90';
}

function Block({ title, children, className, action }: { title: string; children: ReactNode; className?: string; action?: ReactNode }) {
  return (
    <section className={cn('space-y-2', className)}>
      <div className="flex items-center justify-between">
        <h4 className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink/45">{title}</h4>
        {action}
      </div>
      {children}
    </section>
  );
}

/** Inline contextual help tip trigger that shows a summary tooltip and can open the full glossary. */
export function HelpTip({
  term,
  onOpenGlossary,
}: {
  term: string;
  onOpenGlossary?: (term: string) => void;
}) {
  const entry = getGlossaryEntry(term);
  if (!entry) return null;

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onOpenGlossary?.(entry.term);
      }}
      title={`${entry.term}: ${entry.short} (Click for full glossary)`}
      aria-label={`Learn about ${entry.term}`}
      className="inline-flex items-center justify-center text-ink/35 hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded transition-colors"
    >
      <HelpCircle className="h-3 w-3" />
    </button>
  );
}

/** Interactive modal presenting definitions, worked examples, and categorized entries for P&L attribution terms. */
export function Glossary({
  isOpen,
  onClose,
  initialTerm,
}: {
  isOpen: boolean;
  onClose: () => void;
  initialTerm?: string;
}) {
  const [filter, setFilter] = useState('');
  const initialEntry = initialTerm ? getGlossaryEntry(initialTerm) : undefined;
  const [category, setCategory] = useState<'all' | 'waterfall' | 'costs' | 'metrics'>(
    initialEntry?.category ?? 'all'
  );
  const [selectedTerm] = useState<string | undefined>(initialTerm);

  const dialogRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!isOpen) return;
    const opener = document.activeElement as HTMLElement | null;

    // Focus into the dialog (the search box)
    const timer = setTimeout(() => {
      searchRef.current?.focus();
    }, 0);

    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== 'Tab' || !dialogRef.current) return;
      // Focus trap: keep Tab cycling inside the dialog
      const focusables = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
        )
      ).filter((el) => el.offsetParent !== null || el.getClientRects().length > 0);
      if (!focusables.length) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || !dialogRef.current.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !dialogRef.current.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    };

    window.addEventListener('keydown', onKey);
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    return () => {
      clearTimeout(timer);
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = prevOverflow;
      if (opener && document.contains(opener)) {
        opener.focus();
      }
    };
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const entries = getAllGlossaryEntries().filter((e) => {
    if (category !== 'all' && e.category !== category) return false;
    if (!filter) return true;
    const q = filter.toLowerCase();
    return (
      e.term.toLowerCase().includes(q) ||
      e.short.toLowerCase().includes(q) ||
      e.description.toLowerCase().includes(q) ||
      e.aliases?.some((a) => a.toLowerCase().includes(q))
    );
  });

  return (
    <div
      ref={dialogRef}
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="attribution-glossary-title"
      onKeyDown={(e) => e.key === 'Escape' && onClose()}
    >
      <div className="relative flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-ink/10 bg-surface shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink/10 px-5 py-3.5">
          <div className="flex items-center gap-2">
            <BookOpen className="h-4 w-4 text-accent" />
            <h3 id="attribution-glossary-title" className="text-sm font-semibold text-ink">
              Attribution Glossary &amp; Help
            </h3>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-ink/40 hover:text-ink hover:bg-ink/[0.05] transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            aria-label="Close glossary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Search & Tabs */}
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink/5 bg-ink/[0.01] px-5 py-2.5">
          <div className="flex gap-1.5" role="group" aria-label="Filter glossary by category">
            {(['all', 'waterfall', 'costs', 'metrics'] as const).map((cat) => (
              <button
                key={cat}
                type="button"
                onClick={() => setCategory(cat)}
                aria-pressed={category === cat}
                className={cn(
                  'rounded-md px-2.5 py-1 text-xs font-medium capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent',
                  category === cat
                    ? 'bg-accent/15 text-accent font-semibold'
                    : 'text-ink/60 hover:text-ink hover:bg-ink/[0.04]'
                )}
              >
                {cat}
              </button>
            ))}
          </div>
          <input
            ref={searchRef}
            type="search"
            name="glossary-search"
            id="attribution-glossary-search"
            aria-label="Search glossary terms"
            placeholder="Search terms…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="rounded border border-ink/15 bg-surface px-2.5 py-1 text-xs text-ink placeholder:text-ink/35 focus:outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent"
          />
        </div>

        {/* Content list */}
        <div className="flex-1 overflow-y-auto p-5 space-y-4">
          {entries.length === 0 ? (
            <p className="text-center text-xs text-ink/40 py-8">No matching glossary terms found.</p>
          ) : (
            entries.map((item) => {
              const isHighlight =
                selectedTerm &&
                (item.term.toLowerCase() === selectedTerm.toLowerCase() ||
                  item.aliases?.some((a) => a.toLowerCase() === selectedTerm.toLowerCase()));
              return (
                <article
                  key={item.term}
                  className={cn(
                    'rounded-xl border p-3.5 transition-colors',
                    isHighlight
                      ? 'border-accent/40 bg-accent/[0.03]'
                      : 'border-ink/5 bg-ink/[0.02]'
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <h4 className="text-xs font-semibold text-ink">{item.term}</h4>
                    <span className="rounded bg-ink/[0.05] px-1.5 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-ink/45">
                      {item.category}
                    </span>
                  </div>
                  <p className="mt-1 text-[11.5px] text-ink/75 leading-relaxed">{item.description}</p>
                  <div className="mt-2 rounded bg-surface/80 p-2 border border-ink/[0.04]">
                    <span className="font-mono text-[10px] uppercase tracking-wider text-ink/45 block mb-0.5">
                      Example
                    </span>
                    <p className="font-mono text-[11px] text-ink/80">{item.example}</p>
                  </div>
                </article>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

function Waterfall({ a, currency, onOpenGlossary }: { a: Attribution; currency?: string; onOpenGlossary?: (term: string) => void }) {
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
                <th scope="row" className="py-2 pr-3 text-left font-normal text-ink/80">
                  <span className="inline-flex items-center gap-1.5">
                    {b.name}
                    <HelpTip term={b.name} onOpenGlossary={onOpenGlossary} />
                  </span>
                </th>
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

function Instruments({ a, currency, onOpenGlossary }: { a: Attribution; currency?: string; onOpenGlossary?: (term: string) => void }) {
  const rows = sortByShare(a.instruments);
  if (rows.length === 0) return <p className="text-[12px] text-ink/50">No closed trades, so no instrument has a result yet.</p>;
  return (
    <Block title="Instruments">
      <div className="overflow-x-auto">
        <table className="w-full min-w-[640px] text-[12px]">
          <caption className="sr-only">Attribution breakdown by instrument</caption>
          <thead>
            <tr className="text-left text-ink/45">
              <th scope="col" className="py-1.5 pr-3 font-medium">Instrument</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Trades</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">
                <span className="inline-flex items-center justify-end gap-1">
                  Win rate
                  <HelpTip term="Win rate" onOpenGlossary={onOpenGlossary} />
                </span>
              </th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Gross</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Fees</th>
              <th scope="col" className="py-1.5 pr-3 text-right font-medium">Net</th>
              <th scope="col" className="py-1.5 text-right font-medium">
                <span className="inline-flex items-center justify-end gap-1">
                  Share of |net|
                  <HelpTip term="Share of |net|" onOpenGlossary={onOpenGlossary} />
                </span>
              </th>
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

function LongShort({ d, currency, onOpenGlossary }: { d: AttributionDirection; currency?: string; onOpenGlossary?: (term: string) => void }) {
  return (
    <Block
      title="Long / short"
      action={<HelpTip term="Long / short" onOpenGlossary={onOpenGlossary} />}
    >
      <div className="grid gap-2.5 sm:grid-cols-2">
        <Side label="Long" trades={d.long_trades} wins={d.long_wins} winRate={d.long_win_rate} gross={d.long_gross_pnl} fees={d.long_fees} net={d.long_net_pnl} />
        <Side label="Short" trades={d.short_trades} wins={d.short_wins} winRate={d.short_win_rate} gross={d.short_gross_pnl} fees={d.short_fees} net={d.short_net_pnl} />
      </div>
      <p className="text-[11px] text-ink/45">Net of both sides: {signedCash(d.long_net_pnl + d.short_net_pnl, currency)}</p>
    </Block>
  );
}

function Costs({ a, currency, onOpenGlossary }: { a: Attribution; currency?: string; onOpenGlossary?: (term: string) => void }) {
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
            <dt className="inline-flex items-center gap-1.5">
              {label}
              <HelpTip term={label} onOpenGlossary={onOpenGlossary} />
            </dt>
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

/** Triggers a browser download of CSV text. */
export function downloadCsv(filename: string, content: string): void {
  if (typeof window === 'undefined') return;
  const blob = new Blob([content], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

/** Action buttons to download the Waterfall and Instruments CSV files. */
export function ExportButtons({
  a,
  exportId,
  currency,
}: {
  a: Attribution;
  exportId?: string;
  currency?: string;
}) {
  const onWaterfall = () => {
    const csv = buildWaterfallCsv(a, { id: exportId, currency });
    downloadCsv(`attribution-waterfall-${exportId ?? 'export'}.csv`, csv);
  };
  const onInstruments = () => {
    const csv = buildInstrumentsCsv(a, { id: exportId, currency });
    downloadCsv(`attribution-instruments-${exportId ?? 'export'}.csv`, csv);
  };
  return (
    <div className="flex items-center gap-2">
      <button
        type="button"
        onClick={onWaterfall}
        className="rounded border border-ink/15 bg-ink/[0.04] px-2.5 py-1 text-[11px] font-medium text-ink/75 hover:bg-ink/[0.08] hover:text-ink transition-colors"
      >
        Waterfall CSV
      </button>
      <button
        type="button"
        onClick={onInstruments}
        className="rounded border border-ink/15 bg-ink/[0.04] px-2.5 py-1 text-[11px] font-medium text-ink/75 hover:bg-ink/[0.08] hover:text-ink transition-colors"
      >
        Instruments CSV
      </button>
    </div>
  );
}

/**
 * The P&L attribution of a backtest or paper account: waterfall, instruments, long/short split and
 * costs. The numbers come from the engine's attribution endpoint; this card only lays them out.
 */
export function AttributionCard({
  a,
  source,
  currency,
  asOf,
  exportId,
}: {
  a: Attribution;
  source: Source;
  currency?: string;
  asOf?: string;
  exportId?: string;
}) {
  const [glossaryOpen, setGlossaryOpen] = useState(false);
  const [activeTerm, setActiveTerm] = useState<string | undefined>();

  const handleOpenGlossary = (term?: string) => {
    setActiveTerm(term);
    setGlossaryOpen(true);
  };

  const caption =
    source === 'backtest'
      ? 'Simulated from this backtest’s fills. Not a live result.'
      : source === 'workspace'
        ? 'Replayed from every paper account’s fills in this workspace.'
        : 'Replayed from this account’s fills.';
  return (
    <GlassCard as="section" className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink/[0.06] px-5 py-4">
        <div>
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
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => handleOpenGlossary()}
            className="inline-flex items-center gap-1.5 rounded border border-ink/15 bg-ink/[0.04] px-2.5 py-1 text-[11px] font-medium text-ink/75 hover:bg-ink/[0.08] hover:text-ink transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <BookOpen className="h-3.5 w-3.5 text-ink/50" />
            Glossary
          </button>
          {!hasNoFills(a) && <ExportButtons a={a} exportId={exportId} currency={currency} />}
        </div>
      </div>
      <div className="space-y-6 p-5">
        {hasNoFills(a) ? (
          <p className="text-[12.5px] text-ink/55">{source === 'backtest' ? 'This run has no fills, so there is nothing to attribute.' : 'No fills yet, so there is nothing to attribute.'}</p>
        ) : (
          <>
            <Waterfall a={a} currency={currency} onOpenGlossary={handleOpenGlossary} />
            <Instruments a={a} currency={currency} onOpenGlossary={handleOpenGlossary} />
            <LongShort d={a.direction} currency={currency} onOpenGlossary={handleOpenGlossary} />
            <Costs a={a} currency={currency} onOpenGlossary={handleOpenGlossary} />
          </>
        )}
      </div>

      <Glossary
        key={activeTerm ?? 'all'}
        isOpen={glossaryOpen}
        onClose={() => setGlossaryOpen(false)}
        initialTerm={activeTerm}
      />
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
      <GlassCard as="section" role="status" className="flex items-center gap-2 p-5 text-[12px] text-ink/55">
        <Spinner className="h-3.5 w-3.5" /> Loading P&amp;L attribution…
      </GlassCard>
    );
  }
  if (state.status === 'error') return <ErrorNote>{state.message}</ErrorNote>;
  return <AttributionCard a={state.a} source="backtest" currency={currency} exportId={backtestId} />;
}

/** Loads the combined attribution of every paper strategy in the workspace (M11 Task 7), with the count of strategies it covers. */
export function WorkspacePaperAttributionPanel({ currency }: { currency?: string }) {
  const [state, setState] = useState<{ status: 'loading' } | { status: 'ready'; a: WorkspaceAttribution } | { status: 'error'; message: string }>({ status: 'loading' });

  useEffect(() => {
    const ctl = new AbortController();
    getWorkspacePaperAttribution()
      .then((a) => {
        if (!ctl.signal.aborted) setState({ status: 'ready', a });
      })
      .catch((e) => {
        if (ctl.signal.aborted) return;
        const message = e instanceof ApiError && e.status === 404 ? 'No paper strategies to attribute yet.' : e instanceof Error ? e.message : 'Could not load the portfolio attribution.';
        setState({ status: 'error', message });
      });
    return () => ctl.abort();
  }, []);

  if (state.status === 'loading') {
    return (
      <GlassCard as="section" role="status" className="flex items-center gap-2 p-5 text-[12px] text-ink/55">
        <Spinner className="h-3.5 w-3.5" /> Loading portfolio P&amp;L attribution…
      </GlassCard>
    );
  }
  if (state.status === 'error') return <ErrorNote>{state.message}</ErrorNote>;
  const { strategy_count, ...a } = state.a;
  return (
    <div className="space-y-2">
      <p className="text-[12px] text-ink/55">
        Combined across {strategy_count} paper {strategy_count === 1 ? 'strategy' : 'strategies'}.
      </p>
      <AttributionCard a={a} source="workspace" currency={currency} />
    </div>
  );
}
