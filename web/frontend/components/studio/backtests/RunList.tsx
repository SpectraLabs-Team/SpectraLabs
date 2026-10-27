'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { History, RefreshCw } from 'lucide-react';
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

/** The workspace's backtests, newest first; refreshes itself while any of them is still running. */
export function RunList({ catalog, selectedId, onSelect, refreshKey }: { catalog: Catalog | null; selectedId: string | null; onSelect: (id: string) => void; refreshKey: number }) {
  const [items, setItems] = useState<BacktestListItem[] | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [more, setMore] = useState(false);
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

  return (
    <GlassCard as="section" data-tour="backtest-runs" className="flex max-h-[760px] flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-4 py-3">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <History className="h-4 w-4 text-accent" /> Runs
        </h2>
        <button type="button" className="btn-ghost" onClick={load} disabled={loading} aria-label="Refresh runs">
          {loading ? <Spinner className="h-3.5 w-3.5" /> : <RefreshCw className="h-3.5 w-3.5" />}
        </button>
      </div>
      {error && (
        <div className="p-3">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}
      <ul className="flex-1 overflow-y-auto" aria-label="Backtests">
        {items === null && !error && (
          <li className="flex justify-center p-6">
            <Spinner />
          </li>
        )}
        {items?.length === 0 && <li className="p-5 text-center text-[12px] text-ink/50">No backtests yet. Run one to see it here.</li>}
        {items?.map((it) => {
          const h = it.headline;
          const selected = it.id === selectedId;
          return (
            <li key={it.id} className="border-b border-ink/[0.04] last:border-0">
              <button
                type="button"
                onClick={() => onSelect(it.id)}
                aria-current={selected || undefined}
                className={cn('w-full px-4 py-3 text-left transition', selected ? 'bg-accent/[0.08]' : 'hover:bg-ink/[0.03]')}
              >
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
