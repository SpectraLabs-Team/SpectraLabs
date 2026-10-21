'use client';

import { AlertTriangle, CheckCircle2, Info } from 'lucide-react';
import type { QualityReport as Report } from '@/lib/datasets';
import { Badge } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { CATEGORY, duration, int, range } from './format';

function Tile({ label, value, tone }: { label: string; value: string; tone?: 'loss' | 'caution' | 'profit' }) {
  return (
    <div className="rounded-lg border border-ink/[0.07] bg-canvas/50 px-3 py-2">
      <div className="text-[10.5px] uppercase tracking-wider text-ink/45">{label}</div>
      <div className={cn('num mt-0.5 text-[15px] font-semibold text-ink', tone === 'loss' && 'text-loss', tone === 'caution' && 'text-caution', tone === 'profit' && 'text-profit')}>{value}</div>
    </div>
  );
}

const hot = (n: number | undefined) => (n ? 'font-semibold text-caution' : 'text-ink/55');

/**
 * A dataset's quality report: what was read, what was refused and why, and per-symbol checks
 * (gaps, outlier returns, stale prices, zero volume), with anything worth a look highlighted.
 * Command-line versions carry only part of the report; missing parts are left out.
 */
export function QualityReport({ report, compact }: { report: Partial<Report>; compact?: boolean }) {
  const warnings = report.warnings ?? [];
  const rejected = report.rows_rejected ?? 0;
  const categories = Object.entries(report.rejected_by_category ?? {}).sort((a, b) => b[1] - a[1]);
  const issues = report.sample_issues ?? [];
  const stats = report.symbol_stats ?? [];
  const symbols = report.symbols ?? [];
  return (
    <div className="space-y-4">
      <div className={cn('grid gap-2', compact ? 'grid-cols-2 sm:grid-cols-4' : 'grid-cols-2 sm:grid-cols-3 lg:grid-cols-6')}>
        <Tile label="Rows read" value={int(report.rows_total)} />
        <Tile label="Bars" value={int(report.rows_accepted)} tone="profit" />
        <Tile label="Rejected" value={int(rejected)} tone={rejected ? 'loss' : undefined} />
        <Tile label="Symbols" value={int(symbols.length)} />
        {!compact && <Tile label="Range" value={range(report.first_ts, report.last_ts)} />}
        {!compact && <Tile label="Warnings" value={int(warnings.length)} tone={warnings.length ? 'caution' : undefined} />}
      </div>

      {warnings.length > 0 ? (
        <ul className="space-y-1.5" aria-label="Warnings">
          {warnings.map((w, i) => (
            <li key={i} className="flex gap-2 rounded-lg border border-caution/30 bg-caution/[0.07] px-3 py-2 text-[12px] leading-5 text-ink/80">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-caution" />
              <span>{w}</span>
            </li>
          ))}
        </ul>
      ) : (
        report.rows_total !== undefined && (
          <p className="flex items-center gap-2 text-[12px] text-profit">
            <CheckCircle2 className="h-3.5 w-3.5" /> No warnings.
          </p>
        )
      )}

      {categories.length > 0 && (
        <div>
          <h4 className="mb-1.5 text-[11.5px] font-semibold uppercase tracking-wider text-ink/50">Rejected rows by category</h4>
          <div className="flex flex-wrap gap-1.5">
            {categories.map(([c, n]) => (
              <Badge key={c} tone="loss">
                {CATEGORY[c] ?? c} · {int(n)}
              </Badge>
            ))}
          </div>
        </div>
      )}

      {issues.length > 0 && (
        <div>
          <h4 className="mb-1.5 text-[11.5px] font-semibold uppercase tracking-wider text-ink/50">
            {issues.length < rejected ? `First ${issues.length} of ${int(rejected)} rejected rows` : 'Rejected rows'}
          </h4>
          <div className="max-h-56 overflow-auto rounded-lg border border-ink/[0.07]">
            <table className="w-full text-left text-[11.5px]">
              <thead className="sticky top-0 bg-canvas text-ink/50">
                <tr>
                  <th className="px-2.5 py-1.5 font-medium">Line</th>
                  <th className="px-2.5 py-1.5 font-medium">Problem</th>
                  <th className="px-2.5 py-1.5 font-medium">Detail</th>
                </tr>
              </thead>
              <tbody>
                {issues.map((it, i) => (
                  <tr key={i} className="border-t border-ink/[0.05]">
                    <td className="num px-2.5 py-1.5 text-ink/70">{it.line}</td>
                    <td className="whitespace-nowrap px-2.5 py-1.5 text-loss">{CATEGORY[it.category] ?? it.category}</td>
                    <td className="px-2.5 py-1.5 text-ink/70">{it.message}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {stats.length > 0 && (
        <div>
          <h4 className="mb-1.5 text-[11.5px] font-semibold uppercase tracking-wider text-ink/50">Per symbol</h4>
          <div className="overflow-x-auto rounded-lg border border-ink/[0.07]">
            <table className="w-full min-w-[640px] text-left text-[11.5px]">
              <thead className="bg-canvas/60 text-ink/50">
                <tr>
                  {['Symbol', 'Bars', 'Range', 'Median interval', 'Gaps', 'Largest gap', 'Outlier returns', 'Stale runs', 'Zero volume'].map((h) => (
                    <th key={h} className="whitespace-nowrap px-2.5 py-1.5 font-medium">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="num">
                {stats.map((s) => (
                  <tr key={s.symbol} className="border-t border-ink/[0.05]">
                    <td className="px-2.5 py-1.5 font-medium text-ink">{s.symbol}</td>
                    <td className="px-2.5 py-1.5 text-ink/75">{int(s.bars)}</td>
                    <td className="whitespace-nowrap px-2.5 py-1.5 text-ink/60">{range(s.first_ts, s.last_ts)}</td>
                    <td className="px-2.5 py-1.5 text-ink/60">{duration(s.median_interval_ns)}</td>
                    <td className={cn('px-2.5 py-1.5', hot(s.gaps))}>{int(s.gaps)}</td>
                    <td className="px-2.5 py-1.5 text-ink/60">{s.gaps ? duration(s.largest_gap_ns) : '—'}</td>
                    <td className={cn('px-2.5 py-1.5', hot(s.outlier_returns))}>{int(s.outlier_returns)}</td>
                    <td className={cn('px-2.5 py-1.5', hot(s.stale_runs))}>{int(s.stale_runs)}</td>
                    <td className={cn('px-2.5 py-1.5', hot(s.zero_volume_bars))}>{int(s.zero_volume_bars)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {report.thresholds && (
            <p className="mt-1.5 text-[10.5px] text-ink/45">
              A gap is longer than {report.thresholds.gap_factor ?? 5}× the median interval; an outlier return is beyond {report.thresholds.outlier_z ?? 10} robust σ; a stale run is {report.thresholds.stale_run ?? 5}+ bars with an unchanged price.
            </p>
          )}
        </div>
      )}

      {!compact && (report.notes?.length ?? 0) > 0 && (
        <ul className="space-y-1">
          {report.notes!.map((n, i) => (
            <li key={i} className="flex gap-2 text-[11px] leading-5 text-ink/50">
              <Info className="mt-0.5 h-3 w-3 shrink-0" />
              {n}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
