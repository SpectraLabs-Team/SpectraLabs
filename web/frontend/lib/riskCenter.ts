import { request } from './api';

export interface RiskLimits {
  id?: string | null;
  workspace_id: string;
  strategy_id?: string | null;
  max_gross_exposure: number;
  max_net_exposure: number;
  max_position_size: number;
  max_concentration_pct: number;
  max_leverage: number;
  max_drawdown_pct: number;
  max_daily_loss: number;
  max_var_pct: number;
  size_reduction_enabled: boolean;
  updated_by?: string | null;
  updated_at?: string | null;
}

export interface UpsertLimitsPayload {
  strategy_id?: string | null;
  max_gross_exposure: number;
  max_net_exposure: number;
  max_position_size: number;
  max_concentration_pct: number;
  max_leverage: number;
  max_drawdown_pct: number;
  max_daily_loss: number;
  max_var_pct: number;
  size_reduction_enabled: boolean;
}

export interface KillSwitch {
  id: string;
  workspace_id: string;
  strategy_id?: string | null;
  active: boolean;
  reason: string;
  triggered_by?: string | null;
  triggered_at: string;
  cleared_at?: string | null;
  cleared_by?: string | null;
}

export interface SetKillSwitchPayload {
  strategy_id?: string | null;
  active: boolean;
  reason?: string;
}

export interface PositionSnapshot {
  qty: number;
  price: number;
}

export interface PortfolioSnapshot {
  cash: number;
  equity: number;
  peak_equity: number;
  daily_start_equity: number;
  positions: Record<string, PositionSnapshot>;
}

export interface PreTradeCheckPayload {
  strategy_id?: string | null;
  symbol: string;
  side: 'buy' | 'sell';
  qty: number;
  price: number;
  portfolio?: PortfolioSnapshot;
}

export interface RiskBreach {
  code: string;
  limit: number;
  projected: number;
  message: string;
}

export interface ProjectedMetrics {
  gross_exposure: number;
  net_exposure: number;
  leverage: number;
  position_size: number;
  concentration_pct: number;
  drawdown_pct: number;
  daily_loss: number;
  var_pct?: number;
}

export interface PortfolioPositionView {
  symbol: string;
  qty: number;
  price: number;
  updated_at: string;
}

export interface UpsertPositionPayload {
  strategy_id?: string | null;
  symbol: string;
  qty: number;
  price: number;
}

export interface PortfolioBalanceView {
  cash: number;
  equity: number;
  peak_equity: number;
  daily_start_equity: number;
  updated_at: string;
}

export interface UpsertBalancePayload {
  strategy_id?: string | null;
  cash: number;
  equity: number;
  peak_equity: number;
  daily_start_equity: number;
}

export interface EquitySnapshotView {
  step: number;
  ts_ns: number;
  equity: number;
  cash: number;
  created_at: string;
}

export interface RiskAlertView {
  severity: 'warning' | 'breach' | string;
  metric: string;
  current_value: number;
  limit_value: number;
  utilization_pct: number;
  message: string;
}

export interface ReplayDecisionView {
  decision_id: string;
  logged_verdict: string;
  replay_verdict: string;
  matches: boolean;
  breaches: RiskBreach[];
  projected_metrics: ProjectedMetrics;
}

export interface PreTradeResult {
  verdict: 'approved' | 'reduced' | 'rejected';
  requested_qty: number;
  approved_qty: number;
  breaches: RiskBreach[];
  projected_metrics: ProjectedMetrics;
  decision_id?: string | null;
}

export interface RiskDecision {
  id: string;
  workspace_id: string;
  strategy_id?: string | null;
  symbol: string;
  side: string;
  requested_qty: number;
  price: number;
  approved_qty: number;
  verdict: string;
  breaches: RiskBreach[] | unknown;
  reasons: string[];
  metrics: ProjectedMetrics | unknown;
  actor_id?: string | null;
  created_at: string;
}

export interface RiskMetricsSummary {
  workspace_id: string;
  strategy_id?: string | null;
  kill_switch_active: boolean;
  kill_switch_reason?: string | null;
  gross_exposure: number;
  gross_exposure_limit: number;
  gross_exposure_utilization_pct: number;
  net_exposure: number;
  net_exposure_limit: number;
  net_exposure_utilization_pct: number;
  leverage: number;
  leverage_limit: number;
  leverage_utilization_pct: number;
  drawdown_pct: number;
  drawdown_limit_pct: number;
  drawdown_utilization_pct: number;
  daily_loss: number;
  daily_loss_limit: number;
  daily_loss_utilization_pct: number;
  var_pct?: number;
  var_limit_pct?: number;
  var_utilization_pct?: number;
}

export const getRiskLimits = (strategyId?: string): Promise<RiskLimits> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<RiskLimits>(`/api/v1/risk/limits${q}`);
};

export const upsertRiskLimits = (data: UpsertLimitsPayload): Promise<RiskLimits> =>
  request<RiskLimits>('/api/v1/risk/limits', {
    method: 'PUT',
    body: JSON.stringify(data),
  });

export const getKillSwitches = (strategyId?: string): Promise<KillSwitch[]> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<KillSwitch[]>(`/api/v1/risk/kill-switch${q}`);
};

export const setKillSwitch = (data: SetKillSwitchPayload): Promise<KillSwitch> =>
  request<KillSwitch>('/api/v1/risk/kill-switch', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const preTradeCheck = (data: PreTradeCheckPayload): Promise<PreTradeResult> =>
  request<PreTradeResult>('/api/v1/risk/check', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const listAuditDecisions = (limit = 100): Promise<RiskDecision[]> =>
  request<RiskDecision[]>(`/api/v1/risk/audit?limit=${limit}`);

export const getRiskMetrics = (strategyId?: string): Promise<RiskMetricsSummary> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<RiskMetricsSummary>(`/api/v1/risk/metrics${q}`);
};

export const getPositions = (strategyId?: string): Promise<PortfolioPositionView[]> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<PortfolioPositionView[]>(`/api/v1/risk/positions${q}`);
};

export const upsertPosition = (data: UpsertPositionPayload): Promise<{ status: string; symbol: string; qty: number }> =>
  request<{ status: string; symbol: string; qty: number }>('/api/v1/risk/positions', {
    method: 'PUT',
    body: JSON.stringify(data),
  });

export const getBalance = (strategyId?: string): Promise<PortfolioBalanceView | null> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<PortfolioBalanceView | null>(`/api/v1/risk/balance${q}`);
};

export const upsertBalance = (data: UpsertBalancePayload): Promise<{ status: string; equity: number }> =>
  request<{ status: string; equity: number }>('/api/v1/risk/balance', {
    method: 'PUT',
    body: JSON.stringify(data),
  });

export const getExposureHistory = (strategyId?: string, limit = 100): Promise<EquitySnapshotView[]> => {
  const params = new URLSearchParams();
  if (strategyId) params.set('strategy_id', strategyId);
  params.set('limit', limit.toString());
  return request<EquitySnapshotView[]>(`/api/v1/risk/exposure-history?${params.toString()}`);
};

export const getRiskAlerts = (strategyId?: string): Promise<RiskAlertView[]> => {
  const q = strategyId ? `?strategy_id=${encodeURIComponent(strategyId)}` : '';
  return request<RiskAlertView[]>(`/api/v1/risk/alerts${q}`);
};

export const replayDecision = (decisionId: string): Promise<ReplayDecisionView> =>
  request<ReplayDecisionView>(`/api/v1/risk/replay/${encodeURIComponent(decisionId)}`, {
    method: 'POST',
  });
