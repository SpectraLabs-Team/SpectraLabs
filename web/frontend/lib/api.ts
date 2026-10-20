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
  constructor(message: string, public status: number) {
    super(message);
    this.name = 'ApiError';
  }
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
