import type {
  BenchmarkResult,
  CpuTelemetry,
  EngineScenario,
  HealthResponse,
  RiskRequest,
  RiskResult,
  StressResult,
} from './types';

/**
 * All engine calls go through the authenticated Next.js proxy (app/api/engine/[...path]).
 * The Rust API's own address is server-side config: ENGINE_URL, default http://127.0.0.1:8000.
 */
export const API_BASE = '/api/engine';

export class ApiError extends Error {
  /** `details` is the API's structured detail, when it sends one (e.g. `{errors: [{field, message}]}`). */
  constructor(message: string, public status: number, public details?: unknown, public code?: string) {
    super(message);
    this.name = 'ApiError';
  }
}

/** The per-field errors of a 422 (`details.errors`), keyed by field path; '' for the request as a whole. */
export function detailErrors(e: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  const list = e instanceof ApiError ? (e.details as { errors?: { field?: unknown; message?: unknown }[] } | undefined)?.errors : undefined;
  if (!Array.isArray(list)) return out;
  for (const item of list) {
    const field = typeof item?.field === 'string' ? item.field : '';
    const message = typeof item?.message === 'string' ? item.message : '';
    if (message) out[field] = out[field] ? `${out[field]}; ${message}` : message;
  }
  return out;
}

/** The {@link ApiError} for a failed response: the API's message, code and details when it sent JSON. */
export async function errorFrom(response: Response): Promise<ApiError> {
  let detail = `${response.status} ${response.statusText}`;
  let details: unknown;
  let code: string | undefined;
  try {
    const body = await response.json();
    if (typeof body?.message === 'string') detail = body.message;
    else if (typeof body?.detail === 'string') detail = body.detail;
    if (body?.details) details = body.details;
    if (typeof body?.code === 'string') code = body.code;
  } catch {
    /* non-JSON error body */
  }
  return new ApiError(detail, response.status, details, code);
}

/** One engine call through the proxy; throws {@link ApiError} with the API's message on failure. */
export async function request<T>(path: string, init?: RequestInit & { timeoutMs?: number }): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), init?.timeoutMs ?? 30_000);
  let response: Response;
  try {
    response = await fetch(`${API_BASE}${path}`, {
      ...init,
      signal: init?.signal ?? controller.signal,
      headers: init?.body ? { 'Content-Type': 'application/json' } : undefined,
      credentials: 'same-origin',
    });
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw new ApiError('The engine took too long to respond.', 0);
    throw new ApiError('Cannot reach the Spectra server. Check your connection and try again.', 0);
  } finally {
    clearTimeout(timer);
  }
  if (!response.ok) throw await errorFrom(response);
  return response.json() as Promise<T>;
}

const post = <T>(path: string, body: unknown, timeoutMs?: number) =>
  request<T>(path, { method: 'POST', body: JSON.stringify(body), timeoutMs });

export const getHealth = () => request<HealthResponse>('/health', { timeoutMs: 4_000 });

export const getSamplePortfolio = () => request<{ csv: string }>('/api/sample-portfolio').then((r) => r.csv);

export const getCpuTelemetry = () => request<CpuTelemetry>('/api/telemetry/cpu', { timeoutMs: 4_000 });

export const calculateRisk = (body: RiskRequest) => post<RiskResult>('/api/risk', body);

export const runStressTest = (body: {
  portfolio_csv: string;
  scenario: EngineScenario;
  simulations: number;
  horizon_days: number;
  backend?: string;
}) => post<StressResult>('/api/stress', body);

/** The API refuses runs whose simulations × (repetitions + warmups) × 3 backends exceed 10M paths. */
export const BENCHMARK_PATH_BUDGET = 10_000_000;

export const benchmarkFits = (simulations: number, repetitions: number, warmups: number) =>
  simulations * (repetitions + warmups) * 3 <= BENCHMARK_PATH_BUDGET;

export const runBenchmark = (body: {
  portfolio_csv: string;
  simulations: number;
  repetitions: number;
  warmups: number;
}) => post<BenchmarkResult>('/api/benchmark', body, 120_000);
