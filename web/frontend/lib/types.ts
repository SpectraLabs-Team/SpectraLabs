export interface RiskResult {
  metric: string;
  value: number;
  confidence: number;
  simulations_run: number;
  seed: number;
  backend: string;
  library_version: string;
  git_commit: string;
  total_ms: number;
  compute_ms: number;
}

export interface BenchmarkResult {
  simulations: number;
  requested_repetitions: number;
  warmups: number;
  benchmarks: Array<{
    requested_backend: string;
    backend: string | null;
    status: 'available' | 'fallback' | 'unavailable';
    simulations: number;
    repetitions: number;
    latency_ms?: { min: number; mean: number; p50: number; p95: number; p99: number; max: number };
    compute_ms?: { p50: number; p95: number };
    paths_per_second?: number;
    samples_ms: number[];
  }>;
}

export interface StressResult {
  metric: string;
  value: number;
  confidence: number;
  simulations_run: number;
  seed: number;
  backend: string;
  library_version: string;
  git_commit: string;
  total_ms: number;
  compute_ms: number;
}

export interface Portfolio {
  symbol: string;
  quantity: number;
  price: number;
}
