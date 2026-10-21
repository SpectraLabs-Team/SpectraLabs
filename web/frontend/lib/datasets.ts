/**
 * Typed client for the Studio's Data tab (M1 Data Studio; docs/datasets.md): preview a file with
 * ingest settings, upload it, ingest it into a dataset version, then browse, chart, rename and
 * delete datasets. Calls go through the authenticated proxy (lib/server/engineRoutes.ts) except
 * the upload, which streams to app/api/engine-upload. Viewers read; editors upload, ingest,
 * rename and delete.
 *
 * Shapes mirror crates/spectra-api/src/datasets_api.rs; lib/generated/engine-api.ts is the
 * generated reference.
 */
import { API_BASE, ApiError, errorFrom, request } from './api';
import type { components } from './generated/engine-api';
import { getJob, type Job } from './jobs';

type S = components['schemas'];

export type IngestSettings = S['IngestSettings'];
export type IngestColumns = S['IngestColumns'];
export type Suggestions = S['Suggestions'];
export type Preview = S['Preview'];
export type PreviewRow = S['PreviewRow'];
export type PreviewResponse = S['PreviewResponse'];
export type QualityReport = S['QualityReport'];
export type RowIssue = S['RowIssue'];
export type SymbolStats = S['SymbolStats'];
export type Upload = S['UploadView'];
export type DatasetSummary = S['DatasetSummaryView'];
export type DatasetDetail = S['DatasetDetailView'];
export type VersionEntry = S['VersionEntryView'];
export type VersionRun = S['VersionRunView'];
export type Bars = S['BarsView'];
export type BarPoint = S['BarPoint'];
export type CalendarView = S['CalendarView'];
export type CalendarsResponse = S['CalendarsResponse'];

/** A version in full. Its quality report has every field for Studio uploads, only some for command-line ones. */
export type VersionDetail = Omit<S['VersionDetailView'], 'quality_report' | 'source'> & {
  quality_report: Partial<QualityReport>;
  source: { upload_id?: string; file_sha256?: string; filename?: string; settings?: IngestSettings } | null;
};

/** The largest upload (the dashboard route and the engine both refuse more). */
export const MAX_UPLOAD_BYTES = 100 * 1024 * 1024;
/** The preview sample: the first 1 MB of the file (the engine's limit). */
export const PREVIEW_SAMPLE_BYTES = 1024 * 1024;

export const TIME_FORMATS = [
  { value: 'iso8601', label: 'ISO 8601 text (2024-01-02, 2024-01-02T09:15:00+05:30)' },
  { value: 'epoch_s', label: 'Unix seconds' },
  { value: 'epoch_ms', label: 'Unix milliseconds' },
  { value: 'epoch_us', label: 'Unix microseconds' },
  { value: 'epoch_ns', label: 'Unix nanoseconds' },
] as const;

/** Common fixed offsets, in minutes east of UTC. (Fixed: daylight saving is not modelled yet.) */
export const UTC_OFFSETS = [
  { value: 0, label: 'UTC+00:00 (UTC, London winter)' },
  { value: 60, label: 'UTC+01:00 (Central Europe winter)' },
  { value: 120, label: 'UTC+02:00' },
  { value: 330, label: 'UTC+05:30 (India)' },
  { value: 480, label: 'UTC+08:00 (Singapore, Hong Kong)' },
  { value: 540, label: 'UTC+09:00 (Tokyo)' },
  { value: -240, label: 'UTC−04:00 (New York summer)' },
  { value: -300, label: 'UTC−05:00 (New York winter)' },
  { value: -360, label: 'UTC−06:00 (Chicago winter)' },
] as const;

export const SUPPORTED_TIMEZONES = [
  { value: 'America/New_York', label: 'America/New_York (US Eastern, DST)' },
  { value: 'America/Chicago', label: 'America/Chicago (US Central, DST)' },
  { value: 'Europe/London', label: 'Europe/London (UK GMT/BST, DST)' },
  { value: 'Europe/Berlin', label: 'Europe/Berlin (Central Europe, DST)' },
  { value: 'Asia/Tokyo', label: 'Asia/Tokyo (JST, UTC+09:00)' },
  { value: 'Asia/Hong_Kong', label: 'Asia/Hong_Kong (HKT, UTC+08:00)' },
  { value: 'Asia/Kolkata', label: 'Asia/Kolkata (IST, UTC+05:30)' },
  { value: 'UTC', label: 'UTC (Coordinated Universal Time)' },
] as const;

export const EXCHANGE_CALENDARS = [
  { value: 'XNYS', label: 'XNYS — New York Stock Exchange (09:30–16:00 ET)' },
  { value: 'XCME', label: 'XCME — Chicago Mercantile Exchange (Globex)' },
  { value: 'XLON', label: 'XLON — London Stock Exchange (08:00–16:30 GMT/BST)' },
  { value: 'XTKS', label: 'XTKS — Tokyo Stock Exchange (09:00–11:30, 12:30–15:30 JST)' },
  { value: 'XNSE', label: 'XNSE — National Stock Exchange of India (09:15–15:30 IST)' },
  { value: '24/7', label: '24/7 — Continuous (Crypto / FX)' },
] as const;

export const DELIMITERS = [
  { value: ',', label: 'Comma ,' },
  { value: ';', label: 'Semicolon ;' },
  { value: '\t', label: 'Tab' },
  { value: '|', label: 'Pipe |' },
] as const;

export const formatOffset = (minutes: number) => {
  const sign = minutes < 0 ? '−' : '+';
  const m = Math.abs(minutes);
  return `UTC${sign}${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
};

/** The start of a file for the preview: at most 1 MB, cut at the last whole line. */
export async function readSample(file: Blob): Promise<{ text: string; truncated: boolean }> {
  const truncated = file.size > PREVIEW_SAMPLE_BYTES;
  const text = await file.slice(0, PREVIEW_SAMPLE_BYTES).text();
  return { text, truncated };
}

export const preview = (sample: string, truncated: boolean, settings?: IngestSettings) =>
  request<PreviewResponse>('/api/v1/datasets/preview', {
    method: 'POST',
    body: JSON.stringify({ sample, truncated, ...(settings ? { settings } : {}) }),
    timeoutMs: 20_000,
  });

/**
 * Uploads `file` to app/api/engine-upload, reporting the bytes sent. Uses XMLHttpRequest because
 * fetch can't report upload progress.
 */
export function uploadFile(file: File, onProgress: (sent: number, total: number) => void, signal?: AbortSignal): Promise<Upload> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', `/api/engine-upload?filename=${encodeURIComponent(file.name)}`);
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');
    xhr.withCredentials = true;
    xhr.upload.onprogress = (e) => onProgress(e.loaded, e.lengthComputable ? e.total : file.size);
    xhr.onerror = () => reject(new ApiError('The upload failed; check your connection and try again.', 0));
    xhr.onabort = () => reject(new ApiError('Upload cancelled.', 0));
    xhr.onload = () => {
      const response = new Response(xhr.responseText, { status: xhr.status, headers: { 'Content-Type': 'application/json' } });
      if (xhr.status === 201) {
        response.json().then(resolve, () => reject(new ApiError('The server answered oddly; try again.', xhr.status)));
      } else {
        errorFrom(response).then(reject);
      }
    };
    signal?.addEventListener('abort', () => xhr.abort());
    xhr.send(file);
  });
}

export const ingest = (body: { upload_id: string; dataset_name: string; settings: IngestSettings }) =>
  request<Job>('/api/v1/datasets/ingest', { method: 'POST', body: JSON.stringify(body), timeoutMs: 20_000 });

/** What a finished ingest job's result holds. */
export type IngestResult = {
  dataset_version_id: string;
  dataset_name: string;
  version: number;
  existed: boolean;
  checksum: string;
  rows: number;
  symbols: string[];
  quality_report: QualityReport;
};

/** What a refused ingest's result holds. */
export type IngestRejection = { rejected: true; error: string; filename: string; quality_report: QualityReport | null };

export const getIngestJob = (id: string) => getJob<IngestResult | IngestRejection>(id);

export const listDatasetSummaries = () =>
  request<{ datasets?: DatasetSummary[] }>('/api/v1/datasets?view=grouped', { timeoutMs: 10_000 }).then((r) => r.datasets ?? []);

export const getDataset = (id: string) => request<DatasetDetail>(`/api/v1/datasets/${id}`, { timeoutMs: 10_000 });

export const updateDataset = (id: string, body: { name?: string; description?: string }) =>
  request<DatasetDetail>(`/api/v1/datasets/${id}`, { method: 'PATCH', body: JSON.stringify(body), timeoutMs: 10_000 });

export const getVersion = (id: string) => request<VersionDetail>(`/api/v1/datasets/versions/${id}`, { timeoutMs: 20_000 });

export const deleteVersion = (id: string) =>
  request<{ deleted: boolean; dataset_deleted: boolean }>(`/api/v1/datasets/versions/${id}`, { method: 'DELETE', timeoutMs: 20_000 });

export const getVersionRuns = (id: string) =>
  request<{ items: VersionRun[] }>(`/api/v1/datasets/versions/${id}/runs`, { timeoutMs: 10_000 }).then((r) => r.items);

export const getBars = (id: string, q: { symbol?: string; from?: string; to?: string; points?: number } = {}) => {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) if (v !== undefined && v !== '') params.set(k, String(v));
  const qs = params.toString();
  return request<Bars>(`/api/v1/datasets/versions/${id}/bars${qs ? `?${qs}` : ''}`, { timeoutMs: 30_000 });
};

export const listCalendars = () =>
  request<CalendarsResponse>('/api/v1/calendars', { timeoutMs: 10_000 });

/** Settings the person has filled in so far: every required field may still be missing. */
export type DraftSettings = {
  delimiter: string;
  time_format: string;
  utc_offset_minutes: number | null;
  timezone: string;
  exchange: string;
  columns: { timestamp: string; open: string; high: string; low: string; close: string; volume: string; symbol: string };
  default_symbol: string;
  skip_bad_rows: boolean;
  quality: { gap_factor: number; outlier_z: number; stale_run: number };
};

export const emptyDraft = (): DraftSettings => ({
  delimiter: '',
  time_format: '',
  utc_offset_minutes: null,
  timezone: '',
  exchange: '',
  columns: { timestamp: '', open: '', high: '', low: '', close: '', volume: '', symbol: '' },
  default_symbol: '',
  skip_bad_rows: false,
  quality: { gap_factor: 5, outlier_z: 10, stale_run: 5 },
});

/**
 * The settings document for a draft. Nothing is filled in for the person: an empty choice stays
 * missing and the engine reports it. Optional columns left empty mean "the file has none".
 */
export function settingsFromDraft(d: DraftSettings): IngestSettings {
  const symbol = d.columns.symbol || null;
  return {
    delimiter: d.delimiter,
    time_format: d.time_format,
    utc_offset_minutes: d.utc_offset_minutes,
    timezone: d.timezone || null,
    exchange: d.exchange || null,
    columns: {
      timestamp: d.columns.timestamp,
      open: d.columns.open,
      high: d.columns.high,
      low: d.columns.low,
      close: d.columns.close,
      volume: d.columns.volume || null,
      symbol,
    },
    default_symbol: symbol ? null : d.default_symbol || null,
    skip_bad_rows: d.skip_bad_rows,
    quality: { ...d.quality },
  };
}

/** The suggestions as a draft: applied only when the person asks ("Use suggestions"). */
export function applySuggestions(d: DraftSettings, s: Suggestions): DraftSettings {
  const c = s.columns;
  return {
    ...d,
    delimiter: s.delimiter ?? d.delimiter,
    time_format: s.time_format ?? d.time_format,
    columns: {
      timestamp: c.timestamp ?? d.columns.timestamp,
      open: c.open ?? d.columns.open,
      high: c.high ?? d.columns.high,
      low: c.low ?? d.columns.low,
      close: c.close ?? d.columns.close,
      volume: c.volume ?? d.columns.volume,
      symbol: c.symbol ?? d.columns.symbol,
    },
  };
}

/** Polls an ingest job until it finishes (or `signal` aborts), reporting each state. */
export async function followIngest(id: string, onUpdate: (job: Job<IngestResult | IngestRejection>) => void, signal?: AbortSignal) {
  for (;;) {
    if (signal?.aborted) return;
    const job = await getIngestJob(id);
    onUpdate(job);
    if (job.status === 'succeeded' || job.status === 'failed' || job.status === 'cancelled') return job;
    await new Promise((r) => setTimeout(r, 1000));
  }
}

export { API_BASE };
