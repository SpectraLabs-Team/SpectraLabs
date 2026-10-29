'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { Database } from 'lucide-react';
import { listDatasetSummaries, type DatasetSummary, type IngestResult } from '@/lib/datasets';
import { loadAllSampleDatasets, SAMPLE_DATASETS } from '@/lib/sampleData';
import { useToast } from '@/components/ui/StudioToast';
import { GlassCard } from '@/components/ui/primitives';
import { DatasetDetail } from './data/DatasetDetail';
import { DatasetList } from './data/DatasetList';
import { UploadWizard } from './data/UploadWizard';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Studio → Data (M1 Data Studio): upload a CSV and ingest it, browse the workspace's datasets,
 * read their quality reports and charts, and start a backtest on one. The open dataset is kept in
 * the URL (`?tab=data&dataset=<id>`). Viewers browse; editors upload, rename and delete.
 */
export function DataLab() {
  const params = useSearchParams();
  const router = useRouter();
  const [items, setItems] = useState<DatasetSummary[] | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [canEdit, setCanEdit] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [loadingSamples, setLoadingSamples] = useState(false);
  const { success, error: toastError } = useToast();
  const [selected, setSelected] = useState<string | null>(() => {
    const d = params.get('dataset');
    return d && UUID.test(d) ? d : null;
  });
  const detailRef = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await listDatasetSummaries());
      setError('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not list datasets.');
    } finally {
      setLoading(false);
    }
  }, []);

  const existingNames = useMemo(() => new Set((items ?? []).map((d) => d.name)), [items]);
  const missingSamples = useMemo(() => SAMPLE_DATASETS.filter((s) => !existingNames.has(s.name)), [existingNames]);

  const handleLoadSamples = useCallback(async () => {
    if (!canEdit || loadingSamples) return;
    const targets = missingSamples.length > 0 ? missingSamples.map((s) => s.kind) : SAMPLE_DATASETS.map((s) => s.kind);
    setLoadingSamples(true);
    try {
      const res = await loadAllSampleDatasets(undefined, undefined, targets);
      if (res.succeeded.length > 0) {
        success('Sample datasets loaded', `Loaded ${res.succeeded.length} sample dataset${res.succeeded.length > 1 ? 's' : ''} (Uptrend, Crash & recovery, Sideways range).`);
        await load();
      } else {
        toastError('Failed to load sample datasets', 'The ingest did not finish.');
      }
    } catch (e) {
      toastError('Could not load sample datasets', e instanceof Error ? e.message : 'Unknown error');
    } finally {
      setLoadingSamples(false);
    }
  }, [canEdit, loadingSamples, missingSamples, success, toastError, load]);

  useEffect(() => {
    load();
    fetch('/api/auth/session', { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((s: { workspace?: { role?: string } | null }) => setCanEdit(s.workspace?.role === 'owner' || s.workspace?.role === 'editor'))
      .catch(() => setCanEdit(false));
  }, [load]);

  const select = useCallback((id: string | null) => {
    setSelected(id);
    try {
      const url = new URL(window.location.href);
      url.searchParams.set('tab', 'data');
      if (id) url.searchParams.set('dataset', id);
      else url.searchParams.delete('dataset');
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* the URL is a convenience */
    }
    if (id) requestAnimationFrame(() => detailRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  }, []);

  /** "Backtest this dataset": the Backtests tab with this version picked in its form. */
  const backtest = useCallback((versionId: string) => router.push(`/studio?tab=backtests&dataset=${versionId}`), [router]);

  const ingested = useCallback(
    async (r: IngestResult) => {
      const list = await listDatasetSummaries().catch(() => null);
      if (!list) return;
      setItems(list);
      const d = list.find((x) => x.name === r.dataset_name);
      if (d) setSelected(d.id);
    },
    [],
  );

  return (
    <div className="space-y-5">
      {uploading && (
        <UploadWizard
          datasetNames={(items ?? []).map((d) => d.name)}
          onClose={() => setUploading(false)}
          onDone={ingested}
          onBacktest={backtest}
        />
      )}
      <DatasetList
        items={items}
        error={error}
        loading={loading}
        selectedId={selected}
        canEdit={canEdit}
        onSelect={select}
        onRefresh={load}
        onUpload={() => setUploading(true)}
        onLoadSamples={handleLoadSamples}
        loadingSamples={loadingSamples}
        missingSamplesCount={missingSamples.length}
      />
      <div ref={detailRef} className="scroll-mt-20">
        {selected ? (
          <DatasetDetail
            key={selected}
            datasetId={selected}
            canEdit={canEdit}
            onBacktest={backtest}
            onChanged={load}
            onDeleted={() => {
              select(null);
              load();
            }}
          />
        ) : (
          items &&
          items.length > 0 && (
            <GlassCard className="flex min-h-[120px] flex-col items-center justify-center gap-2 p-6 text-center text-sm text-ink/50">
              <Database className="h-5 w-5 text-ink/30" />
              Pick a dataset to see its quality report, charts and versions.
            </GlassCard>
          )
        )}
      </div>
    </div>
  );
}
