import { API_BASE, ApiError, request } from './api';
import type { components } from './generated/engine-api';

export type RunSignalTestRequest = components['schemas']['RunSignalTestRequest'];
export type SignalTestDetailView = components['schemas']['SignalTestDetailView'];
export type SignalTestSummaryView = components['schemas']['SignalTestSummaryView'];
export type GateThresholdInput = components['schemas']['GateThresholdInput'];

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
