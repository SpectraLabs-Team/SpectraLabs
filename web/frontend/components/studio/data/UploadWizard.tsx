'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ArrowLeft, CheckCircle2, ChevronDown, ChevronUp, Eye, FileUp, LineChart, Sparkles, Upload as UploadIcon, X } from 'lucide-react';
import { ApiError, detailErrors } from '@/lib/api';
import {
  applySuggestions,
  DELIMITERS,
  emptyDraft,
  EXCHANGE_CALENDARS,
  followIngest,
  formatOffset,
  getUploadLimits,
  ingest,
  MAX_UPLOAD_BYTES,
  preview as previewSample,
  readSample,
  settingsFromDraft,
  SUPPORTED_TIMEZONES,
  TIME_FORMATS,
  uploadFile,
  UTC_OFFSETS,
  type DraftSettings,
  type IngestRejection,
  type IngestResult,
  type Preview,
  type Suggestions,
} from '@/lib/datasets';
import { cancelJob, type Job } from '@/lib/jobs';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { Field, NumberInput, Toggle } from '../backtests/Fields';
import { bytes, int } from './format';
import { QualityReport } from './QualityReport';

type Stage = 'pick' | 'settings' | 'running' | 'done';
type Running = { phase: 'upload' | 'ingest'; sent: number; total: number; job?: Job<IngestResult | IngestRejection> };

const COLUMN_FIELDS = [
  { key: 'timestamp', label: 'Timestamp', required: true },
  { key: 'open', label: 'Open', required: true },
  { key: 'high', label: 'High', required: true },
  { key: 'low', label: 'Low', required: true },
  { key: 'close', label: 'Close', required: true },
  { key: 'volume', label: 'Volume', required: false },
  { key: 'symbol', label: 'Symbol', required: false },
] as const;

/** The header split by the chosen delimiter (quotes stripped), for the column pickers. */
function headerOf(sample: string, delimiter: string): string[] {
  if (!delimiter) return [];
  const first = sample.replace(/^﻿/, '').split(/\r?\n/, 1)[0] ?? '';
  return first
    .split(delimiter)
    .map((h) => h.trim().replace(/^"(.*)"$/, '$1').trim())
    .filter(Boolean);
}

/** A small "Suggested: x · use" hint next to a field. */
function Suggest({ value, label, current, onUse }: { value: string | null | undefined; label?: string; current: string; onUse: () => void }) {
  if (value == null || value === '' || value === current) return null;
  return (
    <button type="button" onClick={onUse} className="ml-auto inline-flex items-center gap-1 rounded-full border border-accent/30 bg-accent/[0.07] px-2 py-0.5 text-[10.5px] text-accent hover:bg-accent/15" title="Use the suggestion">
      <Sparkles className="h-3 w-3" /> {label ?? value}
    </button>
  );
}

/**
 * Studio → Data → Upload (M1 task 7): pick a CSV → choose its columns, timestamp format and UTC
 * offset → preview → name it → upload and ingest, with progress. Suggestions are shown next to
 * each setting but only applied when the person asks; the engine's rules stay strict.
 */
export function UploadWizard({ datasetNames, onClose, onDone, onBacktest }: { datasetNames: string[]; onClose: () => void; onDone: (r: IngestResult) => void; onBacktest: (versionId: string) => void }) {
  const [stage, setStage] = useState<Stage>('pick');
  const [file, setFile] = useState<File | null>(null);
  const [sample, setSample] = useState<{ text: string; truncated: boolean } | null>(null);
  const [suggestions, setSuggestions] = useState<Suggestions | null>(null);
  const [draft, setDraft] = useState<DraftSettings>(emptyDraft);
  const [previewed, setPreviewed] = useState<{ preview: Preview; settingsKey: string } | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [showQuality, setShowQuality] = useState(false);
  const [name, setName] = useState('');
  const [running, setRunning] = useState<Running | null>(null);
  const [result, setResult] = useState<{ ok: IngestResult } | { rejected: IngestRejection | null; message: string } | null>(null);
  const uploaded = useRef<{ file: File; id: string } | null>(null);
  const [maxBytes, setMaxBytes] = useState(MAX_UPLOAD_BYTES);
  useEffect(() => {
    getUploadLimits()
      .then((l) => setMaxBytes(l.max_bytes))
      .catch(() => {});
  }, []);
  const abort = useRef<AbortController | null>(null);

  const settings = useMemo(() => settingsFromDraft(draft), [draft]);
  const settingsKey = JSON.stringify(settings);
  const fresh = previewed !== null && previewed.settingsKey === settingsKey;
  const header = useMemo(() => (sample ? headerOf(sample.text, draft.delimiter || suggestions?.delimiter || ',') : []), [sample, draft.delimiter, suggestions]);
  const set = (patch: Partial<DraftSettings>) => setDraft((d) => ({ ...d, ...patch }));
  const setColumn = (key: keyof DraftSettings['columns'], value: string) => setDraft((d) => ({ ...d, columns: { ...d.columns, [key]: value } }));
  const err = (k: string) => errors[`settings.${k}`];

  const pick = async (f: File | undefined) => {
    if (!f) return;
    setError('');
    if (f.size > maxBytes) return setError(`${f.name} is ${bytes(f.size)}; files are limited to ${bytes(maxBytes)}.`);
    if (f.size === 0) return setError(`${f.name} is empty.`);
    setBusy(true);
    try {
      const s = await readSample(f);
      const r = await previewSample(s.text, s.truncated);
      setFile(f);
      setSample(s);
      setSuggestions(r.suggestions);
      setDraft(emptyDraft());
      setPreviewed(null);
      setErrors({});
      setName((n) => n || f.name.replace(/\.[^.]+$/, ''));
      setStage('settings');
    } catch (e) {
      setError(e instanceof ApiError && e.status === 403 ? 'Uploading needs the editor role in this workspace.' : e instanceof Error ? e.message : 'Could not read the file.');
    } finally {
      setBusy(false);
    }
  };

  const runPreview = async () => {
    if (!sample) return;
    setBusy(true);
    setError('');
    setErrors({});
    try {
      const r = await previewSample(sample.text, sample.truncated, settings);
      if (r.preview) setPreviewed({ preview: r.preview, settingsKey });
    } catch (e) {
      const f = detailErrors(e);
      setErrors(f);
      setPreviewed(null);
      setError(f.sample ?? f.settings ?? (Object.keys(f).length ? 'Some settings need fixing; see the messages next to them.' : e instanceof Error ? e.message : 'Preview failed.'));
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    if (!file || !fresh || !name.trim()) return;
    setStage('running');
    setError('');
    setResult(null);
    abort.current = new AbortController();
    try {
      let uploadId = uploaded.current?.file === file ? uploaded.current.id : '';
      if (!uploadId) {
        setRunning({ phase: 'upload', sent: 0, total: file.size });
        const u = await uploadFile(file, (sent, total) => setRunning({ phase: 'upload', sent, total }), abort.current.signal);
        uploaded.current = { file, id: u.id };
        uploadId = u.id;
      }
      setRunning({ phase: 'ingest', sent: file.size, total: file.size });
      const job = await ingest({ upload_id: uploadId, dataset_name: name.trim(), settings });
      const last = await followIngest(job.id, (j) => setRunning((r) => ({ ...(r ?? { phase: 'ingest', sent: 0, total: 0 }), job: j })), abort.current.signal);
      if (!last) return;
      if (last.status === 'succeeded') {
        const ok = last.result as unknown as IngestResult;
        setResult({ ok });
        onDone(ok);
      } else {
        const r = last.result as unknown as IngestRejection | null;
        setResult({ rejected: r?.rejected ? r : null, message: last.status === 'cancelled' ? 'The ingest was cancelled; nothing was added.' : last.error ?? 'The ingest failed.' });
      }
      setStage('done');
    } catch (e) {
      const f = detailErrors(e);
      if (f.upload_id) uploaded.current = null; // expired: upload again next time
      setError(Object.keys(f).length ? Object.entries(f).map(([k, m]) => (k ? `${k}: ${m}` : m)).join(' · ') : e instanceof Error ? e.message : 'The upload failed.');
      setStage('settings');
    } finally {
      setRunning(null);
      abort.current = null;
    }
  };

  const cancel = async () => {
    const job = running?.job;
    if (job) await cancelJob(job.id).catch(() => {});
    else abort.current?.abort();
  };

  const s = suggestions;
  return (
    <GlassCard as="section" className="p-5" aria-label="Upload data">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <FileUp className="h-4 w-4 text-accent" /> Upload a CSV
          </h2>
          <p className="mt-0.5 text-xs text-ink/50">Prices are read exactly (up to 8 decimals, never rounded). Nothing is guessed for you: confirm each setting, preview, then ingest.</p>
        </div>
        <button type="button" className="btn-ghost" onClick={onClose} aria-label="Close" disabled={stage === 'running'}>
          <X className="h-4 w-4" />
        </button>
      </div>

      <ol className="mt-4 flex flex-wrap gap-1.5 text-[11px]" aria-label="Steps">
        {(['File', 'Settings & preview', 'Ingest'] as const).map((label, i) => {
          const at = stage === 'pick' ? 0 : stage === 'settings' ? 1 : 2;
          return (
            <li key={label} className={cn('rounded-full border px-2.5 py-0.5', i === at ? 'border-accent/40 bg-accent/10 text-accent' : i < at ? 'border-profit/30 text-profit' : 'border-ink/10 text-ink/40')}>
              {i + 1}. {label}
            </li>
          );
        })}
      </ol>

      {error && (
        <div className="mt-4">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}

      {stage === 'pick' && (
        <label
          className="mt-4 flex min-h-[180px] cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-ink/20 bg-canvas/40 p-6 text-center transition hover:border-accent/50"
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            pick(e.dataTransfer.files?.[0]);
          }}
        >
          {busy ? <Spinner /> : <UploadIcon className="h-6 w-6 text-accent" />}
          <span className="text-sm font-medium text-ink">Drop a CSV here, or choose one</span>
          <span className="text-[11.5px] text-ink/50">Up to {bytes(maxBytes)}; an interrupted upload resumes when you choose the same file again. Columns: a timestamp, open, high, low, close; optionally volume and symbol.</span>
          <input type="file" accept=".csv,.tsv,.txt,text/csv" className="sr-only" onChange={(e) => pick(e.target.files?.[0])} disabled={busy} aria-label="CSV file" />
        </label>
      )}

      {stage === 'settings' && file && (
        <div className="mt-4 space-y-5">
          <div className="flex flex-wrap items-center gap-2 text-[12px] text-ink/70">
            <Badge tone="accent">{file.name}</Badge>
            <span className="num">{bytes(file.size)}</span>
            {sample?.truncated && <span className="text-ink/45">· previewing the first {bytes(sample.text.length)}</span>}
            <button type="button" className="btn-ghost ml-auto text-[11.5px]" onClick={() => setStage('pick')}>
              <ArrowLeft className="h-3.5 w-3.5" /> Another file
            </button>
          </div>

          {s && (
            <div className="flex flex-wrap items-center gap-2 rounded-lg border border-accent/20 bg-accent/[0.05] px-3 py-2 text-[11.5px] text-ink/70">
              <Sparkles className="h-3.5 w-3.5 text-accent" />
              <span>Suggestions from the first rows are shown next to each setting. They are not applied unless you choose them.</span>
              <button type="button" className="btn-secondary ml-auto px-2.5 py-1 text-[11px]" onClick={() => setDraft((d) => applySuggestions(d, s))}>
                Use all suggestions
              </button>
            </div>
          )}

          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            <Field label="delimiter" error={err('delimiter')}>
              <div className="flex items-center gap-2">
                <select className="input py-1.5 text-[13px]" value={draft.delimiter} onChange={(e) => set({ delimiter: e.target.value })} aria-label="Delimiter">
                  <option value="">Choose…</option>
                  {DELIMITERS.map((d) => (
                    <option key={d.label} value={d.value}>
                      {d.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="mt-1 flex">
                <Suggest value={s?.delimiter} label={DELIMITERS.find((d) => d.value === s?.delimiter)?.label} current={draft.delimiter} onUse={() => set({ delimiter: s!.delimiter! })} />
              </div>
            </Field>
            <Field label="timestamp format" error={err('time_format')}>
              <select className="input py-1.5 text-[13px]" value={draft.time_format} onChange={(e) => set({ time_format: e.target.value })} aria-label="Timestamp format">
                <option value="">Choose…</option>
                {TIME_FORMATS.map((f) => (
                  <option key={f.value} value={f.value}>
                    {f.label}
                  </option>
                ))}
              </select>
              <div className="mt-1 flex">
                <Suggest value={s?.time_format} label={TIME_FORMATS.find((f) => f.value === s?.time_format)?.label.split(' (')[0]} current={draft.time_format} onUse={() => set({ time_format: s!.time_format! })} />
              </div>
            </Field>
            <Field
              label="timezone (DST-aware)"
              error={err('timezone')}
              hint={draft.timezone ? `Converts local market time to UTC adjusting for daylight saving.` : 'Choose a DST-aware zone, or set a fixed offset below.'}
            >
              <select
                className="input py-1.5 text-[13px]"
                value={draft.timezone}
                onChange={(e) => {
                  const tz = e.target.value;
                  set({ timezone: tz, ...(tz ? { utc_offset_minutes: null } : {}) });
                }}
                aria-label="Timezone"
              >
                <option value="">None (use fixed UTC offset)</option>
                {SUPPORTED_TIMEZONES.map((tz) => (
                  <option key={tz.value} value={tz.value}>
                    {tz.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field
              label="UTC offset"
              error={err('utc_offset_minutes')}
              hint={
                draft.timezone
                  ? `Overridden by DST-aware timezone ${draft.timezone}.`
                  : s?.timestamps_have_zone
                    ? 'Your timestamps carry a zone, so this only applies to any that don’t.'
                    : 'Fixed offset if timezone is not selected.'
              }
            >
              <select
                className="input py-1.5 text-[13px]"
                disabled={Boolean(draft.timezone)}
                value={draft.utc_offset_minutes === null ? '' : String(draft.utc_offset_minutes)}
                onChange={(e) => set({ utc_offset_minutes: e.target.value === '' ? null : Number(e.target.value) })}
                aria-label="UTC offset"
              >
                <option value="">Choose…</option>
                {UTC_OFFSETS.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
                {draft.utc_offset_minutes !== null && !UTC_OFFSETS.some((o) => o.value === draft.utc_offset_minutes) && (
                  <option value={draft.utc_offset_minutes}>{formatOffset(draft.utc_offset_minutes)}</option>
                )}
              </select>
            </Field>
            <Field
              label="exchange calendar"
              error={err('exchange')}
              hint="Enforces trading sessions, flags out-of-session bars, and skips holidays during gap detection."
            >
              <select
                className="input py-1.5 text-[13px]"
                value={draft.exchange}
                onChange={(e) => set({ exchange: e.target.value })}
                aria-label="Exchange calendar"
              >
                <option value="">None (all timestamps accepted)</option>
                {EXCHANGE_CALENDARS.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <div>
            <h3 className="mb-2 text-[11.5px] font-semibold uppercase tracking-wider text-ink/50">Columns</h3>
            <div className="grid gap-3 sm:grid-cols-4 lg:grid-cols-7">
              {COLUMN_FIELDS.map((c) => {
                const suggested = s?.columns[c.key];
                return (
                  <Field key={c.key} label={c.label.toLowerCase()} error={err(`columns.${c.key}`)}>
                    <select className="input py-1.5 text-[13px]" value={draft.columns[c.key]} onChange={(e) => setColumn(c.key, e.target.value)} aria-label={`${c.label} column`}>
                      <option value="">{c.required ? 'Choose…' : 'None'}</option>
                      {header.map((h) => (
                        <option key={h} value={h}>
                          {h}
                        </option>
                      ))}
                      {draft.columns[c.key] && !header.includes(draft.columns[c.key]) && <option value={draft.columns[c.key]}>{draft.columns[c.key]}</option>}
                    </select>
                    <div className="mt-1 flex">
                      <Suggest value={suggested} current={draft.columns[c.key]} onUse={() => setColumn(c.key, suggested!)} />
                    </div>
                  </Field>
                );
              })}
            </div>
          </div>

          <div className="grid gap-4 sm:grid-cols-3">
            {!draft.columns.symbol && (
              <Field label="symbol for every row" error={err('default_symbol')} hint="The file has no symbol column, so name its instrument.">
                <input className="input py-1.5 text-[13px]" value={draft.default_symbol} onChange={(e) => set({ default_symbol: e.target.value })} placeholder="e.g. SPY" aria-label="Symbol for every row" />
              </Field>
            )}
            <Field label="bad rows" error={err('skip_bad_rows')} hint={draft.skip_bad_rows ? 'Skipped and counted by category in the quality report.' : 'Any bad row stops the ingest (the safe default).'}>
              <div className="flex items-center gap-2 py-1.5 text-[12.5px] text-ink/75">
                <Toggle checked={draft.skip_bad_rows} onChange={(v) => set({ skip_bad_rows: v })} label="Skip bad rows" />
                Skip bad rows
              </div>
            </Field>
          </div>

          <details className="rounded-lg border border-ink/[0.07] px-3 py-2">
            <summary className="cursor-pointer text-[12px] text-ink/60">Quality-check thresholds</summary>
            <div className="mt-3 grid gap-3 sm:grid-cols-3">
              <Field label="gap factor" error={err('quality.gap_factor')} hint="A gap is longer than this many median intervals.">
                <NumberInput value={draft.quality.gap_factor} min={1.01} max={1000} onChange={(v) => set({ quality: { ...draft.quality, gap_factor: v } })} ariaLabel="Gap factor" />
              </Field>
              <Field label="outlier z" error={err('quality.outlier_z')} hint="Robust z-score of a return that counts as an outlier.">
                <NumberInput value={draft.quality.outlier_z} min={0.01} max={1000} onChange={(v) => set({ quality: { ...draft.quality, outlier_z: v } })} ariaLabel="Outlier z" />
              </Field>
              <Field label="stale run" error={err('quality.stale_run')} hint="Unchanged bars in a row that count as stale.">
                <NumberInput value={draft.quality.stale_run} min={2} max={100000} integer onChange={(v) => set({ quality: { ...draft.quality, stale_run: v } })} ariaLabel="Stale run" />
              </Field>
            </div>
          </details>

          <div className="flex flex-wrap items-center gap-2">
            <button type="button" className="btn-secondary" onClick={runPreview} disabled={busy}>
              {busy ? <Spinner className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />} Preview
            </button>
            {previewed && !fresh && <span className="text-[11.5px] text-caution">Settings changed: preview again before ingesting.</span>}
          </div>

          {previewed && <PreviewPanel preview={previewed.preview} stale={!fresh} truncated={!!sample?.truncated} showQuality={showQuality} onToggleQuality={() => setShowQuality((v) => !v)} />}

          {fresh && (
            <div className="flex flex-wrap items-end gap-3 border-t border-ink/[0.06] pt-4">
              <Field label="dataset" className="min-w-[240px] flex-1" error={errors.dataset_name} hint={datasetNames.includes(name.trim()) ? 'Exists: different content becomes its next version; identical content changes nothing.' : 'A new dataset.'}>
                <input className="input py-1.5 text-[13px]" list="dataset-names" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. NIFTY daily" aria-label="Dataset name" maxLength={200} />
                <datalist id="dataset-names">
                  {datasetNames.map((n) => (
                    <option key={n} value={n} />
                  ))}
                </datalist>
              </Field>
              <button type="button" className="btn-primary" onClick={start} disabled={!name.trim() || busy}>
                <UploadIcon className="h-3.5 w-3.5" /> {uploaded.current?.file === file ? 'Ingest again' : 'Upload and ingest'}
              </button>
            </div>
          )}
        </div>
      )}

      {stage === 'running' && running && (
        <div className="mt-6 space-y-3" aria-live="polite">
          <Progress
            label={running.phase === 'upload' ? `Uploading ${file?.name ?? ''}` : running.job?.status === 'queued' ? 'Waiting for a worker…' : 'Reading rows and checking quality…'}
            value={running.phase === 'upload' ? running.sent / Math.max(1, running.total) : running.job?.progress ?? 0}
            detail={running.phase === 'upload' ? `${bytes(running.sent)} of ${bytes(running.total)}` : running.job ? `${Math.round((running.job.progress ?? 0) * 100)}%` : ''}
          />
          <button type="button" className="btn-ghost text-[12px]" onClick={cancel} disabled={running.job?.cancel_requested}>
            {running.job?.cancel_requested ? 'Cancelling…' : 'Cancel'}
          </button>
        </div>
      )}

      {stage === 'done' && result && (
        <div className="mt-5 space-y-4">
          {'ok' in result ? (
            <>
              <div className="flex items-start gap-2 rounded-lg border border-profit/30 bg-profit/[0.07] px-3 py-2.5 text-[12.5px] text-ink/80">
                <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-profit" />
                <div>
                  {result.ok.existed ? (
                    <>
                      <b>{result.ok.dataset_name}</b> already has this exact data as <b>version {result.ok.version}</b>; nothing new was added.
                    </>
                  ) : (
                    <>
                      Added <b>version {result.ok.version}</b> of <b>{result.ok.dataset_name}</b>: {int(result.ok.rows)} bars, {result.ok.symbols.join(', ')}.
                    </>
                  )}
                </div>
              </div>
              <QualityReport report={result.ok.quality_report} compact />
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary" onClick={() => onBacktest(result.ok.dataset_version_id)}>
                  <LineChart className="h-3.5 w-3.5" /> Backtest this dataset
                </button>
                <button type="button" className="btn-secondary" onClick={onClose}>
                  Done
                </button>
              </div>
            </>
          ) : (
            <>
              <div className="flex items-start gap-2 rounded-lg border border-loss/30 bg-loss/[0.07] px-3 py-2.5 text-[12.5px] text-ink/80">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-loss" />
                <div>
                  {result.message}
                  {result.rejected && <div className="mt-1 text-[11.5px] text-ink/60">Every problem in the file is listed below. Fix the file, or go back and change the settings (for example, skip bad rows); the upload is kept for 24 hours.</div>}
                </div>
              </div>
              {result.rejected?.quality_report && <QualityReport report={result.rejected.quality_report} compact />}
              <button type="button" className="btn-secondary" onClick={() => setStage('settings')}>
                <ArrowLeft className="h-3.5 w-3.5" /> Back to the settings
              </button>
            </>
          )}
        </div>
      )}
    </GlassCard>
  );
}

function Progress({ label, value, detail }: { label: string; value: number; detail: string }) {
  const pct = Math.max(0, Math.min(1, value));
  return (
    <div>
      <div className="mb-1 flex justify-between text-[12px] text-ink/70">
        <span>{label}</span>
        <span className="num text-ink/50">{detail}</span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-ink/[0.07]" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct * 100)} aria-label={label}>
        <div className="h-full rounded-full bg-accent transition-[width] duration-300" style={{ width: `${pct * 100}%` }} />
      </div>
    </div>
  );
}

function PreviewPanel({ preview, stale, truncated, showQuality, onToggleQuality }: { preview: Preview; stale: boolean; truncated: boolean; showQuality: boolean; onToggleQuality: () => void }) {
  const q = preview.quality;
  const rows = preview.rows.slice(0, 50);
  const shown = rows.length < preview.rows.length || q.rows_total > preview.rows.length ? `Showing the first ${rows.length} of ${int(q.rows_accepted)} rows.` : '';
  return (
    <div className={cn('space-y-3', stale && 'opacity-50')}>
      {preview.would_abort ? (
        <div className="flex gap-2 rounded-lg border border-loss/30 bg-loss/[0.07] px-3 py-2 text-[12px] leading-5 text-ink/80">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-loss" />
          <span>
            With these settings the ingest would stop: <span className="font-mono text-[11.5px]">{preview.would_abort}</span>
            {q.rows_rejected > 0 && ' Fix the file, or tick “Skip bad rows” to drop and count them.'}
          </span>
        </div>
      ) : (
        <div className="flex gap-2 rounded-lg border border-profit/30 bg-profit/[0.07] px-3 py-2 text-[12px] text-ink/80">
          <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-profit" />
          <span>
            {truncated ? `The first ${int(q.rows_total)} rows (the 1 MB sample) read cleanly` : `All ${int(q.rows_total)} rows read cleanly`}
            {q.rows_rejected ? `, apart from ${int(q.rows_rejected)} that would be skipped` : ''}.
            {truncated && ' The rest of the file is checked when you ingest.'}
          </span>
        </div>
      )}
      {shown && <p className="text-[11px] text-ink/45">{shown}</p>}
      {rows.length > 0 && (
        <div className="max-h-72 overflow-auto rounded-lg border border-ink/[0.07]">
          <table className="w-full text-left text-[11.5px]">
            <thead className="sticky top-0 bg-canvas text-ink/50">
              <tr>
                {['Timestamp (UTC)', 'Symbol', 'Open', 'High', 'Low', 'Close', 'Volume'].map((h) => (
                  <th key={h} className="whitespace-nowrap px-2.5 py-1.5 font-medium">
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="num">
              {rows.map((r, i) => (
                <tr key={i} className="border-t border-ink/[0.05] text-ink/75">
                  <td className="whitespace-nowrap px-2.5 py-1">{r.timestamp.replace('T', ' ').replace('Z', '')}</td>
                  <td className="px-2.5 py-1">{r.symbol}</td>
                  <td className="px-2.5 py-1">{r.open}</td>
                  <td className="px-2.5 py-1">{r.high}</td>
                  <td className="px-2.5 py-1">{r.low}</td>
                  <td className="px-2.5 py-1">{r.close}</td>
                  <td className="px-2.5 py-1">{r.volume}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <button type="button" className="btn-ghost text-[12px]" onClick={onToggleQuality}>
        {showQuality ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />} Sample quality report{q.rows_rejected || q.warnings.length ? ` (${int(q.rows_rejected)} rejected, ${int(q.warnings.length)} warnings)` : ''}
      </button>
      {showQuality && <QualityReport report={q} compact />}
    </div>
  );
}
