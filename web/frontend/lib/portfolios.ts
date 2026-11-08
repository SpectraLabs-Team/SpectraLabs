/**
 * Saved portfolios and portfolio backtests (M15). Portfolios are named target weights stored per
 * workspace; a run puts one through the rebalancing simulator on a dataset version and stores the
 * result, and its report is stored through M16 like every other report.
 *
 * Besides the API calls, this module holds the pure logic the Portfolio Lab view uses (which state
 * each panel is in, who may edit, and the chart series), so it is tested without rendering.
 */
import { API_BASE, ApiError, errorFrom, request } from './api';
import type { Series } from './backtests';

export type PortfolioMethod = 'fixed' | 'equal' | 'inverse_volatility' | 'risk_parity';
export type CalendarFrequency = 'monthly' | 'quarterly';
export type RunStatus = 'running' | 'succeeded' | 'failed';

/** A saved portfolio in the list (GET /api/v1/portfolios). */
export interface PortfolioSummary {
  id: string;
  name: string;
  method: PortfolioMethod;
  created_by: string | null;
  created_at: string;
}

export interface Holding {
  symbol: string;
  /** A fraction, not a percentage. */
  weight: number;
}

/** A saved portfolio with its holdings. */
export interface Portfolio extends PortfolioSummary {
  workspace_id: string;
  holdings: Holding[];
}

/** A holding as a save or edit sends it: `weight` for fixed, `volatility` for inverse volatility, the symbol alone for equal and risk parity. */
export interface HoldingInput {
  symbol: string;
  weight?: number;
  volatility?: number;
}

export interface SavePortfolioRequest {
  name: string;
  method: PortfolioMethod;
  holdings: HoldingInput[];
  /** Risk parity: the dataset version whose daily closes give the covariance. */
  dataset_version_id?: string;
  /** Risk parity: YYYY-MM-DD, only closes up to this day are used. Defaults to, and may not be after, the day of the save. */
  as_of?: string;
}

export interface RunRule {
  calendar?: CalendarFrequency | null;
  drift_threshold?: number | null;
}

export interface RunPortfolioRequest {
  dataset_version_id: string;
  /** YYYY-MM-DD. */
  start: string;
  end: string;
  rule: RunRule;
  initial_cash: number;
  /** Omitted: the M26 paper-default config. */
  cost_config?: { name?: string; slippage_bps?: number; min_fee?: number };
}

export interface PortfolioRunSummary {
  total_return: number;
  max_drawdown: number;
  total_turnover: number;
  total_cost: number;
  rebalance_count: number;
  initial_cash: number;
  final_equity: number;
}

export interface RebalanceWeight {
  symbol: string;
  before: number;
  after: number;
}

export interface RebalanceTrade {
  symbol: string;
  qty: number;
  price: number;
  exec_price: number;
  slippage_cost: number;
  fee: number;
}

export interface Rebalance {
  date: string;
  reasons: ('initial' | 'calendar' | 'drift')[];
  turnover: number;
  cost: number;
  weights: RebalanceWeight[];
  trades: RebalanceTrade[];
}

export interface PortfolioRunResult {
  summary: PortfolioRunSummary;
  equity: { date: string; equity: number }[];
  rebalances: Rebalance[];
  cost_config: { name: string; version: number; slippage_bps: number; min_fee: number };
  prices: { first: string; last: string; days: number; skipped_days: number };
}

/** One run (GET /api/v1/portfolio-runs/{id}). */
export interface PortfolioRun {
  id: string;
  portfolio_id: string | null;
  status: RunStatus;
  request: Record<string, unknown>;
  result: PortfolioRunResult | null;
  error: string | null;
  created_by: string | null;
  created_at: string;
  finished_at: string | null;
}

/** One run in a portfolio's list. */
export interface PortfolioRunListItem {
  id: string;
  portfolio_id: string | null;
  status: RunStatus;
  request: Record<string, unknown>;
  headline: PortfolioRunSummary | null;
  error: string | null;
  created_at: string;
  finished_at: string | null;
}

export interface PortfolioRunPage {
  items: PortfolioRunListItem[];
  next_cursor: string | null;
}

const seg = encodeURIComponent;

export const listPortfolios = () => request<PortfolioSummary[]>('/api/v1/portfolios');
export const getPortfolio = (id: string) => request<Portfolio>(`/api/v1/portfolios/${seg(id)}`);
export const createPortfolio = (body: SavePortfolioRequest) =>
  request<Portfolio>('/api/v1/portfolios', { method: 'POST', body: JSON.stringify(body) });
export const updatePortfolio = (id: string, body: SavePortfolioRequest) =>
  request<Portfolio>(`/api/v1/portfolios/${seg(id)}`, { method: 'PUT', body: JSON.stringify(body) });
export const deletePortfolio = (id: string) =>
  request<{ id: string; deleted: boolean }>(`/api/v1/portfolios/${seg(id)}`, { method: 'DELETE' });
/** Runs the backtest in the request; it returns once the run has finished. */
export const runPortfolio = (id: string, body: RunPortfolioRequest) =>
  request<PortfolioRun>(`/api/v1/portfolios/${seg(id)}/runs`, { method: 'POST', body: JSON.stringify(body), timeoutMs: 120_000 });
export const listPortfolioRuns = (id: string) => request<PortfolioRunPage>(`/api/v1/portfolios/${seg(id)}/runs`);
export const getPortfolioRun = (runId: string) => request<PortfolioRun>(`/api/v1/portfolio-runs/${seg(runId)}`);

/** The engine path that stores (or reuses) the run's report version and returns the PDF. */
export const portfolioReportPath = (runId: string) => `/api/v1/portfolio-runs/${seg(runId)}/report`;

/** Stores the run's report as a version (or reuses an identical one) and downloads the PDF. */
export async function downloadPortfolioReport(runId: string): Promise<void> {
  let response: Response;
  try {
    response = await fetch(`${API_BASE}${portfolioReportPath(runId)}`, { credentials: 'same-origin' });
  } catch {
    throw new ApiError('Cannot reach the Spectra server. Check your connection and try again.', 0);
  }
  if (!response.ok) throw await errorFrom(response);
  const url = URL.createObjectURL(await response.blob());
  const a = document.createElement('a');
  a.href = url;
  a.download = `portfolio-run-${runId.slice(0, 8)}-report.pdf`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/** Owners and editors may save, edit, delete and run portfolios; viewers only read. */
export function canEditPortfolios(role: string | null | undefined): boolean {
  return role === 'owner' || role === 'editor';
}

/** What a panel shows. */
export type PanelState = 'loading' | 'error' | 'running' | 'empty' | 'loaded';

/** A list panel (portfolios, runs): an error wins, then loading, then empty or loaded. */
export function listState<T>(items: T[] | null, error: string | null, running = false): PanelState {
  if (error) return 'error';
  if (running) return 'running';
  if (items === null) return 'loading';
  return items.length === 0 ? 'empty' : 'loaded';
}

/**
 * The run result panel: a run being started or still `running` shows as running, a failed run as
 * an error, and a finished run with no rebalances as empty.
 */
export function runState(run: PortfolioRun | null, error: string | null, starting = false): PanelState {
  if (error) return 'error';
  if (starting || run?.status === 'running') return 'running';
  if (run === null) return 'loading';
  if (run.status === 'failed') return 'error';
  return run.result && run.result.rebalances.length > 0 ? 'loaded' : 'empty';
}

/** Milliseconds since the epoch of a YYYY-MM-DD day (UTC midnight). */
export function dayMs(date: string): number {
  const [y, m, d] = date.split('-').map(Number);
  return Date.UTC(y, m - 1, d);
}

/** The run's equity curve as the backtest chart takes it ([ns, value] points). */
export function equitySeries(result: PortfolioRunResult): Series {
  return {
    kind: 'equity',
    total_points: result.equity.length,
    downsampled: false,
    points: result.equity.map((p) => [dayMs(p.date) * 1_000_000, p.equity]),
  };
}

/** Every holding's symbol across the run's rebalances, in first-seen order. */
export function runSymbols(result: PortfolioRunResult): string[] {
  const out: string[] = [];
  for (const r of result.rebalances) for (const w of r.weights) if (!out.includes(w.symbol)) out.push(w.symbol);
  return out;
}

/** Sum of the weights a fixed-weight form would save, and whether it is within 1e-6 of 1. */
export function weightTotal(holdings: HoldingInput[]): { total: number; ok: boolean } {
  const total = holdings.reduce((s, h) => s + (Number.isFinite(h.weight) ? (h.weight as number) : 0), 0);
  return { total, ok: Math.abs(total - 1) <= 1e-6 };
}
