'use client';

import { useEffect, useState } from 'react';
import { Download, History } from 'lucide-react';
import {
  REPORT_TYPE_LABEL,
  formatBytes,
  listRunReports,
  reportHref,
  type ReportVersion,
  type RunKind,
} from '@/lib/reports';

/**
 * The stored report versions of one run (M16). Each export is kept as a numbered version; every
 * row links to the exact bytes that were exported. `refreshKey` reloads the list after an export.
 */
export function RunReports({ runKind, runId, refreshKey = 0 }: { runKind: RunKind; runId: string; refreshKey?: number }) {
  const [versions, setVersions] = useState<ReportVersion[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    listRunReports(runKind, runId)
      .then((v) => {
        if (!live) return;
        setVersions(v);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(e instanceof Error ? e.message : 'Could not load reports.');
      });
    return () => {
      live = false;
    };
  }, [runKind, runId, refreshKey]);

  return (
    <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs" data-testid="run-reports">
      <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
        <History className="h-4 w-4 text-accent" />
        Saved reports
      </h3>
      <p className="text-xs text-ink/50">Every export is saved as a numbered version. Older versions are never overwritten.</p>
      {error ? (
        <p className="mt-3 text-xs text-rose-500">{error}</p>
      ) : versions === null ? (
        <p className="mt-3 text-xs text-ink/50">Loading…</p>
      ) : versions.length === 0 ? (
        <p className="mt-3 text-xs text-ink/50">No reports yet. Export one above and it will be listed here.</p>
      ) : (
        <table className="mt-3 w-full text-xs">
          <thead>
            <tr className="text-left text-[11px] uppercase text-ink/50">
              <th className="py-1.5 font-medium">Report</th>
              <th className="py-1.5 font-medium">Version</th>
              <th className="py-1.5 font-medium">Format</th>
              <th className="py-1.5 font-medium">Created</th>
              <th className="py-1.5 text-right font-medium">Size</th>
              <th className="py-1.5 font-medium">SHA-256</th>
              <th className="py-1.5" />
            </tr>
          </thead>
          <tbody className="num">
            {versions.map((v) => (
              <tr key={`${v.report_id}-${v.version}`} className="border-t border-ink/[0.04]">
                <td className="py-1.5 text-ink">{REPORT_TYPE_LABEL[v.report_type] ?? v.report_type}</td>
                <td className="py-1.5 font-semibold text-ink">v{v.version}</td>
                <td className="py-1.5 uppercase text-ink/70">{v.format}</td>
                <td className="py-1.5 text-ink/70">{new Date(v.created_at).toLocaleString()}</td>
                <td className="py-1.5 text-right text-ink/70">{formatBytes(v.size_bytes)}</td>
                <td className="py-1.5 font-mono text-ink/50" title={v.sha256}>
                  {v.sha256.slice(0, 12)}
                </td>
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
      )}
    </div>
  );
}
