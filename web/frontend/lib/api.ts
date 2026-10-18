import { RiskResult, BenchmarkResult, StressResult } from './types';

const API_BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? 'http://localhost:8000';

export async function calculateRisk(
  portfolioCsv: string,
  metric: string = 'var',
  confidence: number = 0.99,
  simulations: number = 1000000
): Promise<RiskResult> {
  const response = await fetch(`${API_BASE}/api/risk`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      portfolio_csv: portfolioCsv,
      metric,
      confidence,
      simulations,
    }),
  });

  if (!response.ok) throw new Error(await readApiError(response, 'Failed to calculate risk'));

  return response.json();
}

export async function runBenchmark(
  portfolioCsv: string,
  simulations: number = 100000,
  repetitions: number = 5,
  warmups: number = 1
): Promise<BenchmarkResult> {
  const response = await fetch(`${API_BASE}/api/benchmark`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      portfolio_csv: portfolioCsv,
      simulations,
      repetitions,
      warmups,
    }),
  });

  if (!response.ok) throw new Error(await readApiError(response, 'Failed to run benchmark'));

  return response.json();
}

export async function runStressTest(
  portfolioCsv: string,
  scenario: string = 'market-crash'
): Promise<StressResult> {
  const response = await fetch(`${API_BASE}/api/stress`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      portfolio_csv: portfolioCsv,
      scenario,
    }),
  });

  if (!response.ok) throw new Error(await readApiError(response, 'Failed to run stress test'));

  return response.json();
}

async function readApiError(response: Response, fallback: string): Promise<string> {
  try {
    const body = await response.json();
    return body.detail || fallback;
  } catch {
    return fallback;
  }
}

async function getResource<T>(path: string): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`);
  if (!response.ok) throw new Error(await readApiError(response, `Could not load ${path}`));
  return response.json();
}

export const getCpuTelemetry = () => getResource<{
  cpu_utilization: number; cpu_cores: number; cpu_threads: number;
  memory_utilization: number; memory_used_gb: number; memory_total_gb: number; source: string;
}>('/api/telemetry/cpu');

export const getWorkerTelemetry = () => getResource<{
  workers: Array<{ worker_id: number; utilization: number }>;
  total_workers: number; average_utilization: number; source: string;
}>('/api/telemetry/workers');

export const getGpuTelemetry = () => getResource<{
  available: boolean; utilization: number; memory_used_mb: number;
  memory_total_mb: number; temperature_c: number; message?: string;
}>('/api/telemetry/gpu');

export async function getSamplePortfolio(): Promise<string> {
  const response = await fetch(`${API_BASE}/api/sample-portfolio`);
  if (!response.ok) {
    throw new Error('Failed to get sample portfolio');
  }
  const data = await response.json();
  return data.csv;
}

export async function getHealth(): Promise<{ status: string; cli_exists: boolean }> {
  const response = await fetch(`${API_BASE}/health`);
  if (!response.ok) {
    throw new Error('Failed to get health status');
  }
  return response.json();
}
