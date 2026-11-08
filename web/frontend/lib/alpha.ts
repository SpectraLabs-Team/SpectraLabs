import { API_BASE, ApiError, request } from './api';
import type { components } from './generated/engine-api';

export type SignalTestDetailView = components['schemas']['SignalTestDetailView'];
export type SignalTestSummaryView = components['schemas']['SignalTestSummaryView'];

export interface GateThresholdInput {
  min_mean_ic?: number | null;
  min_t_stat?: number | null;
  max_turnover?: number | null;
  min_half_life_days?: number | null;
  max_half_life_days?: number | null;
}

export interface RunSignalTestRequest {
  signal_name: string;
  signal_id?: string;
  signal_version?: number;
  dataset_version_id?: string;
  symbols?: string[];
  horizons?: number[];
  primary_horizon?: number;
  quantile_count?: number;
  custom_signals?: Array<{ symbol: string; date: string; value: number }>;
  custom_bars?: Array<{ symbol: string; date: string; close: number; timestamp_ns?: number }>;
  threshold?: GateThresholdInput | null;
}

export interface SaveSignalRequest {
  name: string;
  description?: string;
  signal_type: string;
  parameters?: Record<string, unknown>;
  threshold?: GateThresholdInput | null;
}

export interface SavedSignalView {
  id: string;
  workspace_id: string;
  name: string;
  description: string;
  latest_version: number;
  signal_type: string;
  parameters: Record<string, unknown>;
  threshold?: GateThresholdInput | null;
  created_at: string;
  updated_at: string;
}

export interface SavedSignalVersionView {
  id: string;
  signal_id: string;
  workspace_id: string;
  version: number;
  signal_type: string;
  parameters: Record<string, unknown>;
  threshold?: GateThresholdInput | null;
  created_at: string;
}

export const OWNER_DEFAULT_GATE_THRESHOLDS: GateThresholdInput = {
  min_mean_ic: 0.05,
  min_t_stat: 2.0,
  max_half_life_days: 20.0,
};

export async function listSignals(): Promise<SavedSignalView[]> {
  return request<SavedSignalView[]>('/api/v1/alpha/signals');
}

export async function getSignal(signalId: string): Promise<SavedSignalView> {
  return request<SavedSignalView>(`/api/v1/alpha/signals/${encodeURIComponent(signalId)}`);
}

export async function listSignalVersions(signalId: string): Promise<SavedSignalVersionView[]> {
  return request<SavedSignalVersionView[]>(`/api/v1/alpha/signals/${encodeURIComponent(signalId)}/versions`);
}

export async function getSignalVersion(signalId: string, version: number): Promise<SavedSignalVersionView> {
  return request<SavedSignalVersionView>(
    `/api/v1/alpha/signals/${encodeURIComponent(signalId)}/versions/${version}`,
  );
}

export async function saveSignal(req: SaveSignalRequest): Promise<SavedSignalVersionView> {
  return request<SavedSignalVersionView>('/api/v1/alpha/signals', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function listSignalTests(signalName?: string): Promise<SignalTestSummaryView[]> {
  const query = signalName ? `?signal_name=${encodeURIComponent(signalName)}` : '';
  return request<SignalTestSummaryView[]>(`/api/v1/alpha/signal-tests${query}`);
}

export async function getSignalTest(id: string): Promise<SignalTestDetailView> {
  return request<SignalTestDetailView>(`/api/v1/alpha/signal-tests/${encodeURIComponent(id)}`);
}

export async function runSignalTest(req: RunSignalTestRequest): Promise<SignalTestDetailView> {
  return request<SignalTestDetailView>('/api/v1/alpha/signal-tests', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function exportSignalTest(id: string, format: 'pdf' | 'json' | 'csv'): Promise<void> {
  let response: Response;
  try {
    response = await fetch(`${API_BASE}/api/v1/alpha/signal-tests/${encodeURIComponent(id)}/export?format=${format}`, {
      credentials: 'same-origin',
    });
  } catch {
    throw new ApiError('Cannot reach the Spectra server. Check your connection and try again.', 0);
  }
  if (!response.ok) {
    throw new ApiError(`Could not export signal test (${response.status}).`, response.status);
  }

  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `signal-test-${id.slice(0, 8)}.${format}`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
