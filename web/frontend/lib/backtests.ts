/**
 * Typed client for backtests (`/api/v1/backtests`, `/api/v1/datasets`, `/api/v1/strategies/builtin`;
 * M3, docs/backtests.md). A backtest is a queued job of kind `backtest`: create it, poll it, then
 * read its summary, chart series and stored files. Calls go through the authenticated proxy
 * (lib/server/engineRoutes.ts): viewers read, editors create and cancel.
 *
 * The shapes mirror `BacktestView`, `BacktestListItem`, `DatasetVersionView` in
 * crates/spectra-api/src/backtests_api.rs and the summary document in
 * crates/spectra-backtest/src/output.rs (schema `spectra.backtest.summary/v1`);
 * lib/generated/engine-api.ts is the generated reference.
 */
import { API_BASE, ApiError, request } from './api';
import type { JobStatus } from './jobs';

// ---------------------------------------------------------------- catalogue

export type ParamType = 'int' | 'float' | 'bool';

export interface StrategyParam {
  name: string;
  type: ParamType;
  default: number | boolean;
  min?: number;
  max?: number;
  description: string;
}

export interface BuiltinStrategy {
  name: string;
  label: string;
  description: string;
  params: StrategyParam[];
  strategy_sha256: string;
}

/** One property of the configuration's JSON Schema. */
export interface ConfigField {
  description: string;
  type: string | string[];
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  enum?: string[];
  pattern?: string;
  default?: unknown;
}

export interface ConfigSchema {
  title: string;
  properties: Record<string, ConfigField>;
}

/** A configuration in the user form (every field optional; the preset fills the rest). */
export type UserConfig = Record<string, number | boolean | string | null>;

export interface Preset {
  name: string;
  label: string;
  config: UserConfig;
}

export interface Catalog {
  strategies: BuiltinStrategy[];
  config_schema: ConfigSchema;
  presets: Preset[];
}

export const getCatalog = () => request<Catalog>('/api/v1/strategies/builtin', { timeoutMs: 10_000 });

// ---------------------------------------------------------------- datasets

export interface DatasetVersion {
  id: string;
  dataset_name: string;
  version: number;
  checksum: string;
  row_count: number;
  byte_size: number;
  created_at: string;
}

export const listDatasets = () =>
  request<{ items: DatasetVersion[] }>('/api/v1/datasets', { timeoutMs: 10_000 }).then((r) => r.items);

// ---------------------------------------------------------------- requests

export type ParamValues = Record<string, number | boolean>;

/** One strategy of a multi-strategy run. */
export interface StrategyAllocation {
  strategy: string;
  params?: ParamValues;
  /** Share of the starting cash, in (0, 1]; the weights add up to at most 1. */
  weight: number;
}

export type CreateBacktest = {
  dataset_version_id: string;
  config?: UserConfig;
  priority?: number;
} & ({ strategy: string; params?: ParamValues; strategies?: never } | { strategies: StrategyAllocation[]; strategy?: never; params?: never });

/** Most strategies in one request (`MAX_REQUEST_STRATEGIES`). */
export const MAX_STRATEGIES = 10;

// ---------------------------------------------------------------- results

export interface Fingerprints {
  engine_version: string;
  dataset_sha256: string;
  config_sha256: string;
  result_sha256: string;
  strategy_sha256: string;
}

export interface StoredFile {
  kind: string;
  file_name: string;
  content_type: string;
  sha256: string;
  size: number;
}

/** Numbers the API could not represent (NaN, infinity) come back as null. */
type N = number | null;

export interface StrategySummary {
  index: number;
  strategy: string;
  strategy_sha256: string;
  params: ParamValues;
  weight: number;
  allocation: string;
  final_equity: string;
  profit: string;
  contribution: N;
  total_return: N;
  annualized_return: N;
  annualized_volatility: N;
  sharpe: N;
  max_drawdown: N;
  average_gross_exposure: N;
  order_count: number;
  fill_count: number;
  total_fees: string;
  financing: string;
  realized_pnl: string;
}

export interface InstrumentSummary {
  instrument: number;
  symbol: string | null;
  fills: number;
  traded_notional: N;
  final_qty: string;
  market_value: string;
  realized_pnl: string;
  unrealized_pnl: string;
  fees: string;
  borrow_fees: string;
  net_pnl: string;
}

/** The run's summary document. Money is a decimal string (exact); ratios are numbers. */
export interface Summary {
  schema: string;
  run: {
    engine_version: string;
    dataset_sha256: string;
    config_sha256: string;
    result_sha256: string;
    seed: string;
    bar_count: number;
    strategy: string;
    strategy_sha256: string;
    params?: ParamValues;
    config: UserConfig;
  };
  metrics: { valid: boolean; periods: number; total_return: N; annualized_return: N; annualized_volatility: N; sharpe: N; max_drawdown: N };
  performance: { cagr: N; sortino: N; calmar: N; omega: N; downside_deviation: N; skewness: N; excess_kurtosis: N; drawdown_episodes: number; average_drawdown: N };
  drawdown: { valid: boolean; max_drawdown: N; current_drawdown: N; longest_underwater_periods: number; ulcer_index: N };
  tail: { valid: boolean; level: N; var: N; expected_shortfall: N; worst_return: N };
  exposure: { valid: boolean; average_gross?: N; max_gross?: N; average_net?: N; min_net?: N; max_net?: N; time_in_market?: N };
  activity: {
    order_count: number;
    orders_by_state: Record<string, number>;
    fill_count: number;
    turnover_annualized: N;
    total_fees: string;
    financing: string;
    realized_pnl: string;
    final_cash: string;
    final_equity: string;
    open_positions: number;
  };
  trades: {
    count: number;
    wins: number;
    losses: number;
    win_rate: N;
    avg_win: N;
    avg_loss: N;
    payoff_ratio: N;
    expectancy: N;
    profit_factor: N;
    total_net: N;
    best: N;
    worst: N;
    avg_holding_days: N;
  };
  instruments: InstrumentSummary[];
  benchmark: {
    valid: boolean;
    name?: string;
    total_return?: N;
    excess_total_return?: N;
    beta?: N;
    alpha_annualized?: N;
    correlation?: N;
    tracking_error?: N;
    information_ratio?: N;
    up_capture?: N;
    down_capture?: N;
  };
  strategies?: {
    list: StrategySummary[];
    unallocated: { allocation: string; final_cash: string; financing: string } | null;
  };
}

/** The job payload as stored: the request plus the dataset version. */
export interface StoredRequest {
  dataset_version_id?: string;
  strategy?: string;
  params?: ParamValues;
  strategies?: StrategyAllocation[];
  config?: UserConfig;
}

export interface Backtest {
  id: string;
  status: JobStatus;
  progress: number;
  cancel_requested: boolean;
  error: string | null;
  request: StoredRequest;
  run_id: string | null;
  fingerprints: Fingerprints | null;
  summary: Summary | null;
  artifacts: StoredFile[] | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface BacktestListItem {
  id: string;
  status: JobStatus;
  progress: number;
  cancel_requested: boolean;
  error: string | null;
  request: StoredRequest;
  run_id: string | null;
  headline: { total_return: N; sharpe: N; max_drawdown: N; final_equity: string | null; fill_count: number | null } | null;
  created_at: string;
  finished_at: string | null;
}

export interface BacktestPage {
  items: BacktestListItem[];
  next_cursor: string | null;
}

export type SeriesKind = 'equity' | 'drawdown';

export interface Series {
  kind: SeriesKind;
  total_points: number;
  downsampled: boolean;
  /** [timestamp in ns, value]. */
  points: [number, number | null][];
}

/** Stored files `/artifacts/{kind}` serves. */
export const DOWNLOADS = [
  { kind: 'equity_csv', label: 'Equity', file: 'equity.csv' },
  { kind: 'orders_csv', label: 'Orders', file: 'orders.csv' },
  { kind: 'fills_csv', label: 'Fills', file: 'fills.csv' },
  { kind: 'returns_csv', label: 'Returns', file: 'returns.csv' },
  { kind: 'strategies_csv', label: 'Per strategy', file: 'strategies.csv' },
  { kind: 'summary', label: 'Summary', file: 'summary.json' },
  { kind: 'request', label: 'Request', file: 'request.json' },
] as const;

export type DownloadKind = (typeof DOWNLOADS)[number]['kind'];

// ---------------------------------------------------------------- calls

const id = (s: string) => encodeURIComponent(s);

export const createBacktest = (body: CreateBacktest) =>
  request<Backtest>('/api/v1/backtests', { method: 'POST', body: JSON.stringify(body), timeoutMs: 20_000 });

export const getBacktest = (backtestId: string) => request<Backtest>(`/api/v1/backtests/${id(backtestId)}`, { timeoutMs: 10_000 });

export const cancelBacktest = (backtestId: string) =>
  request<Backtest>(`/api/v1/backtests/${id(backtestId)}/cancel`, { method: 'POST', timeoutMs: 10_000 });

export const listBacktests = (opts: { status?: JobStatus; cursor?: string; limit?: number } = {}) => {
  const q = new URLSearchParams();
  if (opts.status) q.set('status', opts.status);
  if (opts.cursor) q.set('cursor', opts.cursor);
  if (opts.limit !== undefined) q.set('limit', String(opts.limit));
  const qs = q.toString();
  return request<BacktestPage>(`/api/v1/backtests${qs ? `?${qs}` : ''}`, { timeoutMs: 20_000 });
};

export const getSeries = (backtestId: string, kind: SeriesKind) =>
  request<Series>(`/api/v1/backtests/${id(backtestId)}/series?kind=${kind}`, { timeoutMs: 20_000 });

async function fetchArtifact(backtestId: string, kind: DownloadKind): Promise<Response> {
  let response: Response;
  try {
    response = await fetch(`${API_BASE}/api/v1/backtests/${id(backtestId)}/artifacts/${kind}`, { credentials: 'same-origin' });
  } catch {
    throw new ApiError('Cannot reach the Spectra server. Check your connection and try again.', 0);
  }
  if (!response.ok) {
    let detail = `${response.status} ${response.statusText}`;
    try {
      const body = await response.json();
      if (typeof body?.message === 'string') detail = body.message;
      else if (typeof body?.detail === 'string') detail = body.detail;
    } catch {
      /* non-JSON error body */
    }
    throw new ApiError(detail, response.status);
  }
  return response;
}

/** A stored file's text (e.g. strategies.csv for the per-strategy chart). */
export const getArtifactText = (backtestId: string, kind: DownloadKind) => fetchArtifact(backtestId, kind).then((r) => r.text());

/** Fetches a stored file and hands it to the browser as a download. */
export async function downloadArtifact(backtestId: string, kind: DownloadKind): Promise<void> {
  const entry = DOWNLOADS.find((d) => d.kind === kind);
  const blob = await (await fetchArtifact(backtestId, kind)).blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `backtest-${backtestId.slice(0, 8)}-${entry?.file ?? kind}`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export const isFinishedStatus = (s: JobStatus) => s === 'succeeded' || s === 'failed' || s === 'cancelled';

/**
 * Poll until the backtest finishes or the signal aborts, calling `onUpdate` with each poll.
 * Short outages (a restarting API or proxy) are retried; other errors reject.
 */
export async function waitForBacktest(
  backtestId: string,
  opts: { intervalMs?: number; signal?: AbortSignal; onUpdate?: (b: Backtest) => void } = {},
): Promise<Backtest> {
  const interval = opts.intervalMs ?? 1_500;
  for (;;) {
    if (opts.signal?.aborted) throw new ApiError('Stopped waiting for the backtest.', 0);
    let b: Backtest;
    try {
      b = await getBacktest(backtestId);
    } catch (error) {
      const retryable = error instanceof ApiError && (error.status === 0 || [502, 503, 504].includes(error.status));
      if (!retryable) throw error;
      await sleep(interval * 2, opts.signal);
      continue;
    }
    opts.onUpdate?.(b);
    if (isFinishedStatus(b.status)) return b;
    await sleep(interval, opts.signal);
  }
}

function sleep(ms: number, signal?: AbortSignal) {
  return new Promise<void>((resolve) => {
    const t = setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => {
      clearTimeout(t);
      resolve();
    });
  });
}

// ---------------------------------------------------------------- helpers

/**
 * Splits a 422 message ("invalid backtest configuration: params.fast: must be ...; config.x: ...")
 * into per-field messages. Unattributed problems land under "".
 */
export function fieldErrors(message: string): Record<string, string> {
  const out: Record<string, string> = {};
  const body = message.replace(/^invalid backtest configuration:\s*/i, '');
  for (const part of body.split(/;\s+/)) {
    const m = /^([A-Za-z_][\w.[\]]*):\s+(.*)$/.exec(part.trim());
    if (m) out[m[1]] = out[m[1]] ? `${out[m[1]]}; ${m[2]}` : m[2];
    else if (part.trim()) out[''] = out[''] ? `${out['']}; ${part.trim()}` : part.trim();
  }
  return out;
}

/** The default value of every parameter of a strategy. */
export function defaultParams(s: BuiltinStrategy): ParamValues {
  return Object.fromEntries(s.params.map((p) => [p.name, p.default]));
}

/** A strategy name, or "multi-strategy" for a strategies list, from a stored request. */
export function describeRequest(r: StoredRequest, catalog?: Catalog | null): string {
  const label = (name: string) => catalog?.strategies.find((s) => s.name === name)?.label ?? name;
  if (r.strategies?.length) return r.strategies.map((s) => label(s.strategy)).join(' + ');
  return r.strategy ? label(r.strategy) : 'backtest';
}

/** One strategy's equity over time, from strategies.csv (`ts_ns,strategy,equity,cash,gross_exposure`). */
export interface StrategyCurve {
  strategy: number;
  points: [number, number][];
}

/** Parses strategies.csv, keeping at most `maxPoints` evenly spaced points per strategy (always the last). */
export function parseStrategyCurves(csv: string, maxPoints = 600): StrategyCurve[] {
  const by = new Map<number, [number, number][]>();
  for (const line of csv.split('\n').slice(1)) {
    if (!line) continue;
    const [ts, strategy, equity] = line.split(',');
    const k = Number(strategy);
    if (!by.has(k)) by.set(k, []);
    by.get(k)!.push([Number(ts), Number(equity)]);
  }
  return [...by.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([strategy, pts]) => {
      if (pts.length <= maxPoints) return { strategy, points: pts };
      const step = (pts.length - 1) / (maxPoints - 1);
      const out: [number, number][] = [];
      for (let i = 0; i < maxPoints; i++) out.push(pts[Math.round(i * step)]);
      return { strategy, points: out };
    });
}

/** Nanosecond timestamp → milliseconds for Date. */
export const nsToMs = (ns: number) => ns / 1e6;

/** A money string from the summary as a number (for display only; the string is exact). */
export const money = (s: string | null | undefined) => (s == null ? NaN : Number(s));
