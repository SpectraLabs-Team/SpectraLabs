'use client';

import { useCallback, useEffect, useState } from 'react';
import { CircleAlert, Download, Fingerprint, LoaderCircle, MessageSquarePlus, X } from 'lucide-react';
import { API_BASE, ApiError, request } from '@/lib/api';
import type { Run } from './ExperimentRegistry';

type Artifact = {
  id: string;
  kind: string;
  sha256: string;
  size_bytes: number;
  content_type: string;
  summary: Record<string, unknown>;
};

type RunDetailData = Run & {
  dataset_bars_sha256: string;
  strategy_sha256: string;
  config_sha256: string;
  result_sha256: string;
  engine_version: string;
  seed: number;
  bar_count: number;
  n_obs: number;
  counts_as_trial: boolean;
  job_id: string;
  notes: string[];
  artifacts: Artifact[];
  workspace_trial_count: number;
};

const ARTIFACT_LABELS: Record<string, string> = {
  report: 'Report (JSON)',
  equity_curve: 'Equity curve (CSV)',
  trades: 'Trades (CSV)',
  returns: 'Returns (CSV)',
};

function bytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function Hash({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-[10px] uppercase tracking-wider text-ink/40">{label}</dt>
      <dd className="truncate font-mono text-[11px] text-ink/75" title={value || 'not recorded'}>
        {value ? `${value.slice(0, 16)}…${value.slice(-6)}` : '—'}
      </dd>
    </div>
  );
}

/** The header of a run page: what produced it, how to reproduce it, its files and notes. */
export function RunDetail({ runId, experimentName, onClose, onChanged }: {
  runId: string;
  experimentName: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [run, setRun] = useState<RunDetailData | null>(null);
  const [error, setError] = useState('');
  const [note, setNote] = useState('');
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setError('');
    try {
      setRun(await request<RunDetailData>(`/api/v1/runs/${encodeURIComponent(runId)}`));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not load the run.');
    }
  }, [runId]);

  useEffect(() => { setRun(null); void load(); }, [load]);

  const addNote = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!note.trim()) return;
    setSaving(true); setError('');
    try {
      await request(`/api/v1/runs/${encodeURIComponent(runId)}/notes`, { method: 'POST', body: JSON.stringify({ text: note.trim() }) });
      setNote('');
      await load();
      onChanged();
    } catch (cause) {
      setError(cause instanceof ApiError ? cause.message : 'Could not add the note.');
    } finally { setSaving(false); }
  };

  return (
    <div className="rounded-2xl border border-accent/25 bg-accent/[0.04] p-5" aria-label="Run details">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="font-mono text-[10px] uppercase tracking-[0.18em] text-accent">Run</p>
          <h2 className="mt-1 truncate font-mono text-sm text-ink" title={runId}>{runId}</h2>
          <p className="mt-0.5 text-xs text-ink/50">{experimentName}</p>
        </div>
        <button type="button" onClick={onClose} className="rounded-lg p-1.5 text-ink/50 hover:bg-ink/5 hover:text-ink" aria-label="Close run details">
          <X className="h-4 w-4" />
        </button>
      </div>

      {error && <div role="alert" className="mt-3 flex gap-2 rounded-lg border border-loss/30 bg-loss/10 p-2 text-xs text-loss"><CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />{error}</div>}
      {!run && !error && <p className="mt-4 flex items-center gap-2 text-xs text-ink/50"><LoaderCircle className="h-3.5 w-3.5 animate-spin" />Loading run…</p>}

      {run && (
        <>
          <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-xs sm:grid-cols-4">
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Status</dt><dd className="text-ink">{run.status}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Strategy</dt><dd className="text-ink">{run.strategy}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Engine</dt><dd className="font-mono text-ink">{run.engine_version || '—'}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Seed</dt><dd className="font-mono text-ink">{run.seed}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Parameters</dt><dd className="font-mono text-ink">{Object.entries(run.params).map(([k, v]) => `${k}=${v}`).join(' ') || '—'}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Bars / returns</dt><dd className="font-mono text-ink">{run.bar_count} / {run.n_obs}</dd></div>
            <div><dt className="text-[10px] uppercase tracking-wider text-ink/40">Recorded</dt><dd className="text-ink">{new Date(run.recorded_ts / 1e6).toLocaleString()}</dd></div>
            <div>
              <dt className="text-[10px] uppercase tracking-wider text-ink/40">Trials in workspace</dt>
              <dd className="font-mono text-ink" title="Every completed trial counts toward the Deflated Sharpe Ratio">{run.workspace_trial_count}{run.counts_as_trial ? '' : ' (not counted)'}</dd>
            </div>
          </dl>
          {run.error && <p className="mt-3 rounded-lg border border-loss/30 bg-loss/10 p-2 text-xs text-loss">{run.error}</p>}

          <h3 className="mt-5 flex items-center gap-1.5 text-xs font-semibold text-ink/70"><Fingerprint className="h-3.5 w-3.5 text-accent" />Fingerprints</h3>
          <dl className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-2">
            <Hash label="Dataset bars" value={run.dataset_bars_sha256} />
            <Hash label="Strategy source" value={run.strategy_sha256} />
            <Hash label="Configuration" value={run.config_sha256} />
            <Hash label="Result" value={run.result_sha256} />
          </dl>

          {run.artifacts.length > 0 && (
            <>
              <h3 className="mt-5 text-xs font-semibold text-ink/70">Files</h3>
              <ul className="mt-2 space-y-1.5">
                {run.artifacts.map((artifact) => (
                  <li key={artifact.id} className="flex items-center justify-between gap-2 rounded-lg border border-ink/[0.08] px-3 py-2 text-xs">
                    <span className="min-w-0">
                      <span className="block text-ink">{ARTIFACT_LABELS[artifact.kind] ?? artifact.kind}</span>
                      <span className="font-mono text-[10px] text-ink/40">{bytes(artifact.size_bytes)} · {artifact.sha256.slice(0, 12)}</span>
                    </span>
                    <a href={`${API_BASE}/api/v1/runs/${encodeURIComponent(run.id)}/artifacts/${encodeURIComponent(artifact.kind)}`}
                      download className="inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-accent hover:bg-accent/10">
                      <Download className="h-3.5 w-3.5" />Download
                    </a>
                  </li>
                ))}
              </ul>
            </>
          )}

          <h3 className="mt-5 text-xs font-semibold text-ink/70">Notes</h3>
          {run.notes.length ? (
            <ul className="mt-2 space-y-1.5">{run.notes.map((text, i) => <li key={i} className="rounded-lg bg-ink/[0.03] px-3 py-2 text-xs text-ink/75">{text}</li>)}</ul>
          ) : <p className="mt-2 text-xs text-ink/40">No notes yet. Runs never change; notes are added alongside them.</p>}
          <form onSubmit={addNote} className="mt-2 flex gap-2">
            <input value={note} onChange={(event) => setNote(event.target.value)} maxLength={2000} placeholder="Add a note" className="input min-w-0 flex-1 text-xs" aria-label="New note" />
            <button disabled={saving || !note.trim()} className="btn-secondary inline-flex items-center gap-1.5 text-xs">
              {saving ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" /> : <MessageSquarePlus className="h-3.5 w-3.5" />}Add
            </button>
          </form>
        </>
      )}
    </div>
  );
}
