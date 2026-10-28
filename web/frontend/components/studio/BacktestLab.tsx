'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { LineChart, RefreshCw } from 'lucide-react';
import { getCatalog, listDatasets, type Backtest, type Catalog, type DatasetVersion, type StoredRequest } from '@/lib/backtests';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { BacktestForm, type Prefill } from './backtests/BacktestForm';
import { CompareRuns } from './backtests/CompareRuns';
import { RunDetail } from './backtests/RunDetail';
import { RunList } from './backtests/RunList';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Studio → Backtests (M3): queue a backtest of a built-in strategy (or several on one account) on
 * a dataset version, follow it, and read its results. The selected run is kept in the URL
 * (`?tab=backtests&run=<id>`) so a link opens it.
 */
export function BacktestLab() {
  const params = useSearchParams();
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [datasets, setDatasets] = useState<DatasetVersion[]>([]);
  const [loadError, setLoadError] = useState('');
  const [loading, setLoading] = useState(true);
  const [canRun, setCanRun] = useState(false);
  const [selected, setSelected] = useState<string | null>(() => {
    const r = params.get('run');
    return r && UUID.test(r) ? r : null;
  });
  const [compareIds, setCompareIds] = useState<string[]>(() => {
    const c = params.get('compare');
    if (!c) return [];
    return c.split(',').filter((id) => UUID.test(id)).slice(0, 4);
  });
  const [isComparing, setIsComparing] = useState<boolean>(() => {
    const c = params.get('compare');
    if (!c) return false;
    const ids = c.split(',').filter((id) => UUID.test(id));
    return ids.length >= 2;
  });
  const [refreshKey, setRefreshKey] = useState(0);
  const [prefill, setPrefill] = useState<Prefill | null>(null);
  const formRef = useRef<HTMLDivElement>(null);
  const detailRef = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError('');
    try {
      const [c, d] = await Promise.all([getCatalog(), listDatasets()]);
      setCatalog(c);
      setDatasets(d);
      // "Backtest this dataset" from the Data tab: ?tab=backtests&dataset=<version id>.
      const wanted = params.get('dataset');
      if (wanted && UUID.test(wanted) && d.some((x) => x.id === wanted)) {
        setPrefill({ request: { dataset_version_id: wanted }, nonce: Date.now() });
      }
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : 'Could not reach the engine.');
    } finally {
      setLoading(false);
    }
    // Read the URL once, on load; later changes come from the form itself.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    load();
    // Editors and owners can run; viewers only look.
    fetch('/api/auth/session', { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((s: { workspace?: { role?: string } | null }) => setCanRun(s.workspace?.role === 'owner' || s.workspace?.role === 'editor'))
      .catch(() => setCanRun(false));
  }, [load]);

  const select = useCallback((id: string) => {
    setSelected(id);
    setIsComparing(false);
    try {
      const url = new URL(window.location.href);
      url.searchParams.set('tab', 'backtests');
      url.searchParams.set('run', id);
      url.searchParams.delete('compare');
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* the URL is a convenience */
    }
  }, []);

  const toggleCompareId = useCallback((id: string) => {
    setCompareIds((prev) => {
      const exists = prev.includes(id);
      const next = exists ? prev.filter((x) => x !== id) : prev.length < 4 ? [...prev, id] : prev;
      return next;
    });
  }, []);

  const openComparison = useCallback(() => {
    if (compareIds.length >= 2) {
      setIsComparing(true);
      try {
        const url = new URL(window.location.href);
        url.searchParams.set('tab', 'backtests');
        url.searchParams.set('compare', compareIds.join(','));
        url.searchParams.delete('run');
        window.history.replaceState(null, '', url.toString());
      } catch {
        /* url is a convenience */
      }
      requestAnimationFrame(() => detailRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
    }
  }, [compareIds]);

  const clearComparison = useCallback(() => {
    setCompareIds([]);
    setIsComparing(false);
    try {
      const url = new URL(window.location.href);
      url.searchParams.delete('compare');
      if (selected) url.searchParams.set('run', selected);
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* url is a convenience */
    }
  }, [selected]);

  const removeCompareId = useCallback((id: string) => {
    setCompareIds((prev) => {
      const next = prev.filter((x) => x !== id);
      if (next.length < 2) {
        setIsComparing(false);
      }
      return next;
    });
  }, []);

  const created = (b: Backtest) => {
    select(b.id);
    setRefreshKey((k) => k + 1);
    requestAnimationFrame(() => detailRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  };

  const rerun = (request: StoredRequest) => {
    setPrefill({ request, nonce: Date.now() });
    requestAnimationFrame(() => formRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  };

  const changed = useCallback(() => setRefreshKey((k) => k + 1), []);

  return (
    <div className="space-y-5">
      <div className="grid gap-5 lg:grid-cols-[1fr_340px] [&>*]:min-w-0">
        <div ref={formRef} data-tour="backtest-form" className="min-w-0 scroll-mt-20">
          {catalog ? (
            <BacktestForm catalog={catalog} datasets={datasets} canRun={canRun} onCreated={created} prefill={prefill} onDatasetsChanged={load} />
          ) : (
            <GlassCard as="section" className="flex min-h-[320px] flex-col items-center justify-center gap-3 p-6 text-center">
              {loading ? (
                <Spinner />
              ) : (
                <>
                  <LineChart className="h-6 w-6 text-ink/30" />
                  <div className="max-w-md">
                    <ErrorNote>{loadError || 'Could not load the strategies.'}</ErrorNote>
                  </div>
                  <p className="max-w-md text-[11.5px] leading-5 text-ink/50">Backtests need the engine API with its database, and a worker to run them (docker compose starts all three).</p>
                  <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={load}>
                    <RefreshCw className="h-3.5 w-3.5" /> Try again
                  </button>
                </>
              )}
            </GlassCard>
          )}
        </div>
        {/* Desktop: the run list stays in view beside the long form and scrolls on its own (same rule as the Risk Lab panel). */}
        <div className="min-w-0 lg:sticky lg:top-20 lg:self-start">
          <RunList
            catalog={catalog}
            selectedId={selected}
            onSelect={select}
            refreshKey={refreshKey}
            compareIds={compareIds}
            onToggleCompareId={toggleCompareId}
            isComparing={isComparing}
            onOpenComparison={openComparison}
            onClearComparison={clearComparison}
          />
        </div>
      </div>

      <div ref={detailRef} data-tour="backtest-results" className="scroll-mt-20">
        {isComparing && compareIds.length >= 2 ? (
          <CompareRuns
            runIds={compareIds}
            catalog={catalog}
            onSelectRun={(id) => select(id)}
            onRemoveRun={removeCompareId}
            onClose={() => setIsComparing(false)}
          />
        ) : selected ? (
          <RunDetail
            key={selected}
            backtestId={selected}
            catalog={catalog}
            datasets={datasets}
            canRun={canRun}
            onRerun={rerun}
            onChanged={changed}
            onAddToCompare={toggleCompareId}
            isCompared={compareIds.includes(selected)}
          />
        ) : (
          <GlassCard className="flex min-h-[160px] flex-col items-center justify-center gap-2 p-6 text-center text-sm text-ink/50">
            <LineChart className="h-5 w-5 text-ink/30" />
            Pick a run from the list, or start a new one, to see its equity curve, metrics and files.
          </GlassCard>
        )}
      </div>
    </div>
  );
}
