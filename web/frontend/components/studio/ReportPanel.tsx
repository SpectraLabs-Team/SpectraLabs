'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { Download, History } from 'lucide-react';
import {
  REPORT_TYPE_LABEL,
  listGapReportVersions,
  listRunReports,
  reportHref,
  type GapReportVersions,
  type ReportVersion,
} from '@/lib/reports';

export const NO_REPORTS = 'No reports yet.';

/** A list of stored report versions, each with a link to the exact bytes that were exported. */
export function ReportVersionList({ versions }: { versions: ReportVersion[] }) {
  if (versions.length === 0) return <p className="mt-3 text-xs text-ink/50">{NO_REPORTS}</p>;
  return (
    <table className="mt-3 w-full text-xs">
      <thead>
        <tr className="text-left text-[11px] uppercase text-ink/50">
          <th className="py-1.5 font-medium">Report</th>
          <th className="py-1.5 font-medium">Version</th>
          <th className="py-1.5 font-medium">Created</th>
          <th className="py-1.5" />
        </tr>
      </thead>
      <tbody className="num">
        {versions.map((v) => (
          <tr key={`${v.report_id}-${v.version}`} className="border-t border-ink/[0.04]">
            <td className="py-1.5 text-ink">{REPORT_TYPE_LABEL[v.report_type] ?? v.report_type}</td>
            <td className="py-1.5 font-semibold text-ink">v{v.version}</td>
            <td className="py-1.5 text-ink/70">{new Date(v.created_at).toLocaleString()}</td>
            <td className="py-1.5 text-right">
              <a
                href={reportHref(v)}
                download
                className="inline-flex items-center gap-1 text-accent hover:underline"
                title={`Download version ${v.version} exactly as it was exported`}
              >
                <Download className="h-3.5 w-3.5" />
                Download
              </a>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Frame({ testId, children }: { testId: string; children: ReactNode }) {
  return (
    <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs" data-testid={testId}>
      <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
        <History className="h-4 w-4 text-accent" />
        Saved reports
      </h3>
      <p className="text-xs text-ink/50">Every export is saved as a numbered version. Older versions are never overwritten.</p>
      {children}
    </div>
  );
}

/** Loads with `load` whenever `key` changes; `data` is `null` while the current key is loading. */
function useLoaded<T>(load: () => Promise<T>, key: string): { data: T | null; error: string | null } {
  const [state, setState] = useState<{ key: string; data: T | null; error: string | null } | null>(null);
  useEffect(() => {
    let live = true;
    load()
      .then((data) => {
        if (live) setState({ key, data, error: null });
      })
      .catch((e: unknown) => {
        if (live) setState({ key, data: null, error: e instanceof Error ? e.message : 'Could not load reports.' });
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the key changes
  }, [key]);
  return state?.key === key ? state : { data: null, error: null };
}

function Status({ error }: { error: string | null }) {
  return error ? <p className="mt-3 text-xs text-rose-500">{error}</p> : <p className="mt-3 text-xs text-ink/50">Loading…</p>;
}

/** The stored tear-sheet versions of one backtest (stored under the backtest job id). `refreshKey` reloads the list after an export. */
export function BacktestReportPanel({ backtestId, refreshKey = 0 }: { backtestId: string; refreshKey?: number }) {
  const { data, error } = useLoaded(() => listRunReports('backtest', backtestId), `${backtestId}:${refreshKey}`);
  return <Frame testId="backtest-reports">{data ? <ReportVersionList versions={data} /> : <Status error={error} />}</Frame>;
}

/** The stored versions of each gap report, one group per gap report (stored under the gap report id). */
export function PaperReportPanel({ strategyId }: { strategyId?: string }) {
  const { data, error } = useLoaded<GapReportVersions[]>(() => listGapReportVersions(strategyId), strategyId ?? '');
  return (
    <Frame testId="paper-reports">
      {!data ? (
        <Status error={error} />
      ) : data.length === 0 ? (
        <p className="mt-3 text-xs text-ink/50">{NO_REPORTS}</p>
      ) : (
        data.map(({ gap, versions }) => (
          <div key={gap.id} className="mt-4 border-t border-ink/[0.06] pt-3" data-testid="gap-report-group">
            <p className="text-xs font-medium text-ink">
              Gap report <span className="font-mono text-ink/60">{gap.id.slice(0, 8)}</span>
              <span className="text-ink/50"> · {new Date(gap.created_at).toLocaleString()}</span>
            </p>
            <ReportVersionList versions={versions} />
          </div>
        ))
      )}
    </Frame>
  );
}
