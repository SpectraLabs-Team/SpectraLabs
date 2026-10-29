import { request } from './api';

export interface OptimizationSweep {
  id: string;
  workspace_id: string;
  job_id?: string | null;
  strategy_id?: string | null;
  strategy_version_id?: string | null;
  dataset_version_id: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  method: string;
  objective: string;
  param_grid: Record<string, unknown>;
  trial_count: number;
  best_trial_num?: number | null;
  best_params?: Record<string, unknown>;
  best_metrics?: Record<string, unknown>;
  best_objective?: number | null;
  dsr?: number | null;
  psr?: number | null;
  min_trl?: number | null;
  pbo?: number | null;
  created_at: string;
  finished_at?: string | null;
}

export interface SweepTrial {
  id: string;
  sweep_id: string;
  trial_num: number;
  params: Record<string, number | string>;
  metrics: {
    sharpe?: number;
    total_return?: number;
    annualized_return?: number;
    annualized_volatility?: number;
    max_drawdown?: number;
    [key: string]: unknown;
  };
  objective_value: number;
  returns_mean?: number | null;
  returns_vol?: number | null;
  created_at: string;
}

export interface SensitivityMatrix {
  axis_x: string;
  values_x: number[];
  axis_y: string;
  values_y: number[];
  matrix: (number | null)[][];
}

export interface SweepDetail {
  sweep: OptimizationSweep;
  trials: SweepTrial[];
  sensitivity?: SensitivityMatrix;
}

export interface WalkForwardFold {
  fold: number;
  train_begin: number;
  train_end: number;
  test_begin: number;
  test_end: number;
  in_sample_objective: number;
  out_of_sample_objective: number;
  best_params: Record<string, number | string>;
  oos_sharpe?: number;
  oos_return?: number;
  oos_drawdown?: number;
}

export interface WalkForwardReport {
  id: string;
  workspace_id: string;
  job_id?: string | null;
  strategy_id?: string | null;
  strategy_version_id?: string | null;
  dataset_version_id: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  mode: string;
  train_size: number;
  test_size: number;
  step_size: number;
  embargo: number;
  objective: string;
  param_grid: Record<string, unknown>;
  folds: WalkForwardFold[];
  efficiency_ratio?: number | null;
  in_sample_sharpe?: number | null;
  out_of_sample_sharpe?: number | null;
  created_at: string;
  finished_at?: string | null;
}

export interface OverfittingMetrics {
  psr: number;
  dsr: number;
  min_trl: number;
  pbo: number;
  expected_max_sharpe: number;
  sharpe_variance: number;
  best_sharpe: number;
  num_trials: number;
  num_observations: number;
}

export interface HoldoutVault {
  id: string;
  workspace_id: string;
  name: string;
  dataset_version_id: string;
  budget: number;
  evaluations_used: number;
  notes: string;
  sealed_by?: string | null;
  sealed_at: string;
}

export interface HoldoutEvaluation {
  id: string;
  holdout_id: string;
  workspace_id: string;
  strategy_version_id?: string | null;
  evaluation_num: number;
  metrics: Record<string, unknown>;
  dsr?: number | null;
  warnings: string[];
  evaluated_by?: string | null;
  evaluated_at: string;
}

export interface HoldoutDetail {
  holdout: HoldoutVault;
  evaluations: HoldoutEvaluation[];
}

export interface CreateSweepPayload {
  dataset_version_id: string;
  strategy_version_id?: string;
  strategy?: string;
  dsl_code?: string;
  param_grid: Record<string, unknown>;
  method?: 'grid' | 'random';
  max_trials?: number;
  seed?: number;
  objective?: 'sharpe' | 'total_return';
}

export interface CreateWalkForwardPayload {
  dataset_version_id: string;
  strategy_version_id?: string;
  strategy?: string;
  dsl_code?: string;
  mode?: 'rolling' | 'anchored';
  train_size: number;
  test_size: number;
  step_size?: number;
  embargo?: number;
  param_grid: Record<string, unknown>;
  objective?: 'sharpe' | 'total_return';
}

export const queueSweep = (data: CreateSweepPayload) =>
  request<OptimizationSweep>('/api/v1/optimization/sweep', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const listSweeps = () =>
  request<{ items: OptimizationSweep[] }>('/api/v1/optimization/sweeps').then((r) => r.items);

export const getSweep = (id: string) =>
  request<SweepDetail>(`/api/v1/optimization/sweep/${id}`);

export const queueWalkForward = (data: CreateWalkForwardPayload) =>
  request<WalkForwardReport>('/api/v1/optimization/walkforward', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const listWalkForwards = () =>
  request<{ items: WalkForwardReport[] }>('/api/v1/optimization/walkforward').then((r) => r.items);

export const getWalkForward = (id: string) =>
  request<WalkForwardReport>(`/api/v1/optimization/walkforward/${id}`);

export const computeOverfitting = (data: {
  returns_matrix: number[][];
  benchmark_sharpe?: number;
  confidence?: number;
}) =>
  request<OverfittingMetrics>('/api/v1/optimization/overfitting', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const sealHoldout = (data: {
  name: string;
  dataset_version_id: string;
  budget: number;
  notes?: string;
}) =>
  request<HoldoutVault>('/api/v1/optimization/holdout/seal', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const listHoldouts = () =>
  request<{ items: HoldoutVault[] }>('/api/v1/optimization/holdout').then((r) => r.items);

export const getHoldout = (id: string) =>
  request<HoldoutDetail>(`/api/v1/optimization/holdout/${id}`);

export const evaluateHoldout = (
  id: string,
  data: {
    strategy_version_id?: string;
    strategy?: string;
    dsl_code?: string;
    parameters?: Record<string, unknown>;
  }
) =>
  request<HoldoutEvaluation>(`/api/v1/optimization/holdout/${id}/evaluate`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
