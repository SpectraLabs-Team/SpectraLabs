'use client';

import { Database, RefreshCw, Sparkles, Upload } from 'lucide-react';
import type { DatasetSummary } from '@/lib/datasets';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { ago, bytes, int, range } from './format';

/** The workspace's datasets: name, versions, symbols, date range, rows, last updated. */
export function DatasetList({
  items,
  error,
  loading,
  selectedId,
  canEdit,
  onSelect,
  onRefresh,
  onUpload,
  onLoadSamples,
  loadingSamples,
  missingSamplesCount = 0,
}: {
  items: DatasetSummary[] | null;
  error: string;
  loading: boolean;
  selectedId: string | null;
  canEdit: boolean;
  onSelect: (id: string) => void;
  onRefresh: () => void;
  onUpload: () => void;
  onLoadSamples?: () => void;
  loadingSamples?: boolean;
  missingSamplesCount?: number;
}) {
  return (
    <GlassCard as="section" className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-4 py-3">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Database className="h-4 w-4 text-accent" /> Datasets
        </h2>
        <div className="flex items-center gap-1.5">
          <button type="button" className="btn-ghost" onClick={onRefresh} disabled={loading} aria-label="Refresh datasets">
            {loading ? <Spinner className="h-3.5 w-3.5" /> : <RefreshCw className="h-3.5 w-3.5" />}
          </button>
          {canEdit && missingSamplesCount > 0 && onLoadSamples && (
            <button
              type="button"
              className="btn-secondary px-2.5 py-1.5 text-xs inline-flex items-center gap-1.5"
              onClick={onLoadSamples}
              disabled={loadingSamples}
              title="Load built-in synthetic datasets (Uptrend, Crash & recovery, Sideways range)"
            >
              {loadingSamples ? <Spinner className="h-3.5 w-3.5" /> : <Sparkles className="h-3.5 w-3.5 text-accent" />}
              {loadingSamples ? 'Loading samples…' : `Load sample datasets (${missingSamplesCount})`}
            </button>
          )}
          {canEdit && (
            <button type="button" className="btn-primary px-3 py-1.5 text-xs inline-flex items-center gap-1.5" onClick={onUpload}>
              <Upload className="h-3.5 w-3.5" /> Upload CSV
            </button>
          )}
        </div>
      </div>
      {error && (
        <div className="p-3">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}
      {items === null && !error ? (
        <div className="flex justify-center p-8">
          <Spinner />
        </div>
      ) : items?.length === 0 ? (
        <div className="flex flex-col items-center gap-2 p-8 text-center text-[12.5px] text-ink/55">
          <Database className="h-5 w-5 text-ink/30" />
          No datasets in this workspace yet.
          {canEdit ? (
            <div className="flex flex-wrap items-center justify-center gap-2 mt-2">
              {onLoadSamples && (
                <button
                  type="button"
                  className="btn-primary px-3 py-1.5 text-xs inline-flex items-center gap-1.5"
                  onClick={onLoadSamples}
                  disabled={loadingSamples}
                >
                  {loadingSamples ? <Spinner className="h-3.5 w-3.5" /> : <Sparkles className="h-3.5 w-3.5" />}
                  {loadingSamples ? 'Loading 3 sample datasets…' : 'Load 3 sample datasets'}
                </button>
              )}
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs inline-flex items-center gap-1.5" onClick={onUpload}>
                <Upload className="h-3.5 w-3.5" /> Upload CSV
              </button>
            </div>
          ) : (
            <span className="text-[11.5px] text-ink/45">An editor of the workspace can upload one.</span>
          )}
        </div>
      ) : (
        items && (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] text-left text-[12px]" aria-label="Datasets">
              <thead className="text-ink/45">
                <tr>
                  {['Name', 'Versions', 'Symbols', 'Date range', 'Rows', 'Size', 'Updated'].map((h) => (
                    <th key={h} className="whitespace-nowrap px-4 py-2 font-medium">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {items.map((d) => (
                  <tr
                    key={d.id}
                    onClick={() => onSelect(d.id)}
                    onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && onSelect(d.id)}
                    tabIndex={0}
                    aria-selected={d.id === selectedId}
                    className={cn('cursor-pointer border-t border-ink/[0.05] transition', d.id === selectedId ? 'bg-accent/[0.08]' : 'hover:bg-ink/[0.03]')}
                  >
                    <td className="px-4 py-2.5">
                      <div className="font-medium text-ink">{d.name}</div>
                      {d.description && <div className="max-w-[260px] truncate text-[11px] text-ink/45">{d.description}</div>}
                    </td>
                    <td className="num px-4 py-2.5 text-ink/70">{d.latest_version ? `v${d.latest_version}` : '—'}{d.version_count > 1 ? <span className="text-ink/40"> of {d.version_count}</span> : null}</td>
                    <td className="px-4 py-2.5 text-ink/70">
                      <span className="block max-w-[180px] truncate" title={d.symbols.join(', ')}>
                        {d.symbols.length > 4 ? `${d.symbols.slice(0, 4).join(', ')} +${d.symbols.length - 4}` : d.symbols.join(', ') || '—'}
                      </span>
                    </td>
                    <td className="num whitespace-nowrap px-4 py-2.5 text-ink/60">{range(d.first_ts, d.last_ts)}</td>
                    <td className="num px-4 py-2.5 text-ink/70">{int(d.row_count)}</td>
                    <td className="num px-4 py-2.5 text-ink/50">{bytes(d.byte_size)}</td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-ink/50">{d.updated_at ? ago(d.updated_at) : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}
    </GlassCard>
  );
}
