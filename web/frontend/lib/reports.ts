/**
 * Versioned reports (M16). Every server-rendered report export is stored as a numbered version
 * linked to the run it came from; these helpers list a run's reports and link to each version.
 */
import { API_BASE, request } from './api';

export type ReportType = 'validation' | 'tear_sheet' | 'gap';
export type RunKind = 'sweep' | 'backtest' | 'paper_gap';

/** One stored version of a report (GET /api/v1/reports). */
export interface ReportVersion {
  report_id: string;
  report_type: ReportType;
  run_kind: RunKind;
  run_id: string;
  version: number;
  format: 'pdf' | 'csv';
  content_type: string;
  sha256: string;
  size_bytes: number;
  created_by: string | null;
  created_at: string;
  /** Engine path of the stored bytes, e.g. /api/v1/reports/{id}/versions/2. */
  download_url: string;
}

export const REPORT_TYPE_LABEL: Record<ReportType, string> = {
  validation: 'Validation report',
  tear_sheet: 'Tear sheet',
  gap: 'Gap report',
};

/** Every report version for a run, grouped by type and newest first. */
export const listRunReports = (runKind: RunKind, runId: string) =>
  request<ReportVersion[]>(
    `/api/v1/reports?run_kind=${encodeURIComponent(runKind)}&run_id=${encodeURIComponent(runId)}`,
  );

/** Browser link to download a stored version through the authenticated engine proxy. */
export const reportHref = (v: Pick<ReportVersion, 'download_url'>) => `${API_BASE}${v.download_url}`;

/** "12.3 KB"-style size. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
