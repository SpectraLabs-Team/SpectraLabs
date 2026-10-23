// Response shapes of `spectra-api` (crates/spectra-api). Source of truth:
// crates/spectra-risk/src/engine.rs — RiskReport::to_json_value,
// StressResult::to_json_value and benchmark().

export type RiskMetric = 'var' | 'cvar' | 'volatility' | 'hhi' | 'downsidedeviation' | 'semideviation' | 'skewness' | 'kurtosis';
export type BackendRequest = 'auto' | 'cpu' | 'parallel';
/** Built-in scenarios of /api/stress (spectra-risk SCENARIOS). */
export type EngineScenario =
  | 'market-crash'
  | 'mild-correction'
  | 'covid-crash-2020'
  | 'gfc-2008'
  | 'tech-bubble-2000'
  | 'black-monday-1987';

export interface HealthResponse {
  status: string;
  engine: string;
  engine_version: string;
  cli_exists: boolean;
}

export interface PositionRiskContribution {
  symbol: string;
  position_value: number;
  weight: number;
  component_var: number;
  component_cvar: number;
  cvar_contribution_pct: number;
}

export interface RiskResult {
  /** Canonical name: "VaR", "CVaR", "Volatility", "HHI", … */
  metric: string;
  value: number;
  confidence: number;
  simulations_run: number;
  seed: number;
  /** Backend that actually ran: "CPU" | "Parallel". */
  backend: string;
  /** Backend that was asked for: "Auto" | "CPU" | "Parallel" | "CUDA". */
  requested_backend: string;
  horizon_years: number;
  portfolio_value: number;
  model: string;
  /** Aggregate annualized volatility (simple average of asset vols; 20% default). */
  model_volatility: number;
  library_version: string;
  git_commit: string;
  total_ms: number;
  compute_ms: number;
  /** Present only for adaptive runs. */
  convergence?: { converged: boolean; relative_std_error: number };
  /** M7: Cross-asset dependence model (`aggregate`, `gaussian_copula`, `t_copula`). */
  dependence_model?: string;
  /** M7: Euler component risk attribution by position. */
  position_contributions?: PositionRiskContribution[];
  /** M7: Extreme Value Theory (POT GPD) VaR at the same confidence level. */
  evt_var?: number;
  /** M7: Extreme Value Theory (POT GPD) CVaR at the same confidence level. */
  evt_cvar?: number;
}

/** /api/stress: the VaR report (always 99%) of the shocked book plus scenario fields. */
export interface StressResult extends RiskResult {
  scenario: string;
  shock_factor: number;
  value_before: number;
  value_after: number;
  scenario_loss: number;
}

export interface LatencyStats {
  min: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
}

export interface BenchmarkBackendEntry {
  requested_backend: 'CPU' | 'Parallel' | 'CUDA' | string;
  backend: string;
  /** CUDA is not built in this port, so it always reports "fallback" (ran on Parallel). */
  status: 'available' | 'fallback';
  simulations: number;
  repetitions: number;
  warmups: number;
  latency_ms: LatencyStats;
  compute_ms: { p50: number; p95: number };
  paths_per_second: number;
  samples_ms: number[];
}

export interface BenchmarkResult {
  simulations: number;
  requested_repetitions: number;
  warmups: number;
  /** Always three entries, in order: CPU, Parallel, CUDA. */
  benchmarks: BenchmarkBackendEntry[];
}

export interface CpuTelemetry {
  cpu_utilization: number;
  cpu_cores: number;
  cpu_threads: number;
  memory_utilization: number;
  memory_used_gb: number;
  memory_total_gb: number;
  source: string;
}

export interface Position {
  symbol: string;
  quantity: number;
  price: number;
  /** Annualized; undefined means the engine default (20%). */
  volatility?: number;
}

export interface RiskRequest {
  portfolio_csv: string;
  metric: RiskMetric;
  confidence: number;
  simulations: number;
  horizon_days: number;
  backend: BackendRequest;
  seed?: number;
  dependence?: 'aggregate' | 'gaussian_copula' | 't_copula';
  degrees_of_freedom?: number;
}
