'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, CheckSquare, History, RefreshCw, Scale, Square, X } from 'lucide-react';
import { describeRequest, isFinishedStatus, listBacktests, type BacktestListItem, type Catalog } from '@/lib/backtests';
import type { JobStatus } from '@/lib/jobs';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { ago, pct, ratio, signedPct, tone } from './format';

export function StatusBadge({ status, cancelling }: { status: JobStatus; cancelling?: boolean }) {
  if (cancelling) return <Badge tone="caution">cancelling</Badge>;
  const t = ({ queued: 'neutral', running: 'accent', succeeded: 'profit', failed: 'loss', cancelled: 'caution' } as const)[status];
  return (
    <Badge tone={t}>
      {status === 'running' && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />}
      {status}
    </Badge>
  );
}

const PAGE = 25;

export interface RunListProps {
  catalog: Catalog | null;
  selectedId: string | null;
  onSelect: (id: string) => void;
  refreshKey: number;
  compareIds?: string[];
  onToggleCompareId?: (id: string) => void;
  isComparing?: boolean;
  onOpenComparison?: () => void;
  onClearComparison?: () => void;
}

/** The workspace's backtests, newest first; refreshes itself while any of them is still running. */
export function RunList({
  catalog,
  selectedId,
  onSelect,
  refreshKey,
  compareIds = [],
  onToggleCompareId,
  isComparing = false,
  onOpenComparison,
  onClearComparison,
}: RunListProps) {
  const [items, setItems] = useState<BacktestListItem[] | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [more, setMore] = useState(false);
  const [compareMode, setCompareMode] = useState(false);
  const shown = useRef(PAGE);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listBacktests({ limit: Math.max(PAGE, shown.current) });
      setItems(page.items);
      setCursor(page.next_cursor);
      setError('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not list backtests.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load, refreshKey]);

  // Keep compareMode in sync if compareIds is non-empty
  useEffect(() => {
    if (compareIds.length > 0) {
      setCompareMode(true);
    }
  }, [compareIds.length]);

  // Poll while something is queued or running.
  const active = items?.some((i) => !isFinishedStatus(i.status)) ?? false;
  useEffect(() => {
    if (!active) return;
    const t = setInterval(load, 4_000);
    return () => clearInterval(t);
  }, [active, load]);

  const loadMore = async () => {
    if (!cursor) return;
    setMore(true);
    try {
      const page = await listBacktests({ cursor, limit: PAGE });
      setItems((xs) => [...(xs ?? []), ...page.items]);
      shown.current += page.items.length;
      setCursor(page.next_cursor);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not list backtests.');
    } finally {
      setMore(false);
    }
  };

  const toggleCompare = (id: string, e?: React.MouseEvent) => {
    e?.stopPropagation();
    if (onToggleCompareId) {
      onToggleCompareId(id);
    }
  };

  return (
    <GlassCard as="section" data-tour="backtest-runs" className="flex max-h-[760px] flex-col overflow-hidden lg:max-h-[calc(100dvh-6rem)]">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-4 py-3">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <History className="h-4 w-4 text-accent" /> Runs
        </h2>
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => setCompareMode((m) => !m)}
            className={cn(
              'flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium transition',
              compareMode || compareIds.length > 0
                ? 'bg-accent/15 text-accent font-semibold'
                : 'text-ink/60 hover:bg-ink/[0.04] hover:text-ink'
            )}
            title="Toggle multi-run comparison selection"
          >
            <Scale className="h-3.5 w-3.5" />
            <span>Compare</span>
            {compareIds.length > 0 && (
              <span className="ml-0.5 rounded-full bg-accent px-1.5 py-0.2 text-[10px] text-white">
                {compareIds.length}
              </span>
            )}
          </button>
          <button type="button" className="btn-ghost" onClick={load} disabled={loading} aria-label="Refresh runs">
            {loading ? <Spinner className="h-3.5 w-3.5" /> : <RefreshCw className="h-3.5 w-3.5" />}
          </button>
        </div>
      </div>

      {/* Comparison Action Bar when compareMode is active */}
      {compareMode && (
        <div className="flex items-center justify-between border-b border-accent/20 bg-accent/[0.04] px-4 py-2 text-xs">
          <span className="text-ink/70">
            {compareIds.length === 0
              ? 'Select 2-4 runs to compare'
              : `${compareIds.length} selected (max 4)`}
          </span>
          <div className="flex items-center gap-2">
            {compareIds.length >= 2 && onOpenComparison && (
              <button
                type="button"
                onClick={onOpenComparison}
                className="flex items-center gap-1 rounded bg-accent px-2 py-0.5 text-[11px] font-semibold text-white transition hover:bg-accent/90"
              >
                <span>Compare</span>
                <Check className="h-3 w-3" />
              </button>
            )}
            {compareIds.length > 0 && onClearComparison && (
              <button
                type="button"
                onClick={onClearComparison}
                className="text-ink/50 hover:text-ink text-[11px]"
                title="Clear selection"
              >
                Clear
              </button>
            )}
          </div>
        </div>
      )}

      {error && (
        <div className="p-3">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}
      <ul className="min-h-0 flex-1 overflow-y-auto overscroll-contain" aria-label="Backtests">
        {items === null && !error && (
          <li className="flex justify-center p-6">
            <Spinner />
          </li>
        )}
        {items?.length === 0 && <li className="p-5 text-center text-[12px] text-ink/50">No backtests yet. Run one to see it here.</li>}
        {items?.map((it) => {
          const h = it.headline;
          const selected = it.id === selectedId && !isComparing;
          const isChecked = compareIds.includes(it.id);
          const isEligibleForCompare = it.status === 'succeeded';

          return (
            <li key={it.id} className="border-b border-ink/[0.04] last:border-0">
              <button
                type="button"
                onClick={() => {
                  if (compareMode && isEligibleForCompare) {
                    toggleCompare(it.id);
                  } else {
                    onSelect(it.id);
                  }
                }}
                aria-current={selected || undefined}
                className={cn(
                  'w-full px-4 py-3 text-left transition flex items-start gap-2.5',
                  selected ? 'bg-accent/[0.08]' : 'hover:bg-ink/[0.03]',
                  isChecked && 'bg-accent/[0.05]'
                )}
              >
                {compareMode && (
                  <div
                    onClick={(e) => {
                      if (isEligibleForCompare) toggleCompare(it.id, e);
                    }}
                    className={cn(
                      'mt-0.5 shrink-0 transition',
                      isEligibleForCompare ? 'cursor-pointer text-accent' : 'opacity-20 cursor-not-allowed'
                    )}
                  >
                    {isChecked ? (
                      <CheckSquare className="h-4 w-4 text-accent" />
                    ) : (
                      <Square className="h-4 w-4 text-ink/30" />
                    )}
                  </div>
                )}
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-[12.5px] font-medium text-ink/90">{describeRequest(it.request, catalog)}</span>
                    <StatusBadge status={it.status} cancelling={it.cancel_requested && !isFinishedStatus(it.status)} />
                  </div>
                  <div className="num mt-1 flex items-center justify-between gap-2 text-[11px] text-ink/50">
                    <span>{ago(it.created_at)}</span>
                    {it.status === 'succeeded' && h ? (
                      <span className="flex gap-2.5">
                        <span className={cn(tone(h.total_return) === 'profit' ? 'text-profit' : tone(h.total_return) === 'loss' ? 'text-loss' : '')}>{signedPct(h.total_return)}</span>
                        <span>SR {ratio(h.sharpe)}</span>
                        <span>DD {pct(h.max_drawdown, 1)}</span>
                      </span>
                    ) : it.status === 'running' ? (
                      <span>{Math.round(it.progress * 100)}%</span>
                    ) : it.status === 'failed' ? (
                      <span className="max-w-[60%] truncate text-loss/80" title={it.error ?? ''}>
                        {it.error}
                      </span>
                    ) : null}
                  </div>
                </div>
              </button>
            </li>
          );
        })}
      </ul>
      {cursor && (
        <div className="border-t border-ink/[0.06] p-2">
          <button type="button" className="btn-ghost w-full justify-center" onClick={loadMore} disabled={more}>
            {more ? <Spinner className="h-3.5 w-3.5" /> : null} Older runs
          </button>
        </div>
      )}
    </GlassCard>
  );
}
