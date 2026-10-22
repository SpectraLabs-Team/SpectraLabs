'use client';

import { useMemo } from 'react';
import type { MonthlyReturn } from '@/lib/backtests';
import { cash, signedPct } from './format';
import { cn } from '@/lib/utils';

const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'] as const;

export interface MonthlyHeatmapProps {
  months: MonthlyReturn[] | null | undefined;
  className?: string;
}

interface YearRow {
  year: number;
  months: (MonthlyReturn | null)[];
  ytdReturn: number | null;
  cellCount: number;
}

/**
 * Computes cell styling using a diverging scale around zero.
 * Positive returns scale into --profit (green), negative into --loss (red).
 */
function getCellVisuals(r: number | null | undefined, maxAbs: number) {
  if (r == null || !Number.isFinite(r)) {
    return {
      style: {},
      className: 'bg-ink/[0.02] text-ink/30',
      label: '—',
    };
  }

  const label = signedPct(r, 1);

  if (Math.abs(r) < 0.0001) {
    return {
      style: {},
      className: 'bg-ink/[0.04] text-ink/70',
      label: '0.0%',
    };
  }

  // Normalize intensity between 0.15 and 0.85 opacity for clear visual contrast
  const fraction = Math.min(1, Math.max(0, Math.abs(r) / maxAbs));
  const alpha = 0.15 + fraction * 0.70;

  if (r > 0) {
    return {
      style: { backgroundColor: `rgb(var(--profit) / ${alpha.toFixed(2)})` },
      className: alpha >= 0.52 ? 'text-white font-medium' : 'text-emerald-800 dark:text-emerald-200 font-medium',
      label,
    };
  } else {
    return {
      style: { backgroundColor: `rgb(var(--loss) / ${alpha.toFixed(2)})` },
      className: alpha >= 0.52 ? 'text-white font-medium' : 'text-rose-800 dark:text-rose-200 font-medium',
      label,
    };
  }
}

/**
 * Year by month return heatmap with diverging color scale around zero.
 * Renders monthly returns with sign, partial month flag, and annual YTD.
 */
export function MonthlyHeatmap({ months, className }: MonthlyHeatmapProps) {
  const { rows, maxAbs, hasPartial } = useMemo(() => {
    if (!months || months.length === 0) {
      return { rows: [], maxAbs: 0.10, hasPartial: false };
    }

    let highestAbs = 0.05;
    let anyPartial = false;
    const yearMap = new Map<number, (MonthlyReturn | null)[]>();

    for (const m of months) {
      const absVal = Math.abs(m.return);
      if (absVal > highestAbs) highestAbs = absVal;
      if (m.partial) anyPartial = true;

      if (!yearMap.has(m.year)) {
        yearMap.set(m.year, Array(12).fill(null));
      }
      const arr = yearMap.get(m.year)!;
      if (m.month >= 1 && m.month <= 12) {
        arr[m.month - 1] = m;
      }
    }

    const sortedYears = Array.from(yearMap.keys()).sort((a, b) => a - b);
    const resultRows: YearRow[] = sortedYears.map((year) => {
      const monthArr = yearMap.get(year)!;
      const validMonths = monthArr.filter((m): m is MonthlyReturn => m !== null);
      const ytd = validMonths.length > 0
        ? validMonths.reduce((acc, m) => acc * (1 + m.return), 1) - 1
        : null;

      return {
        year,
        months: monthArr,
        ytdReturn: ytd,
        cellCount: validMonths.length,
      };
    });

    return { rows: resultRows, maxAbs: highestAbs, hasPartial: anyPartial };
  }, [months]);

  if (!months || months.length === 0 || rows.length === 0) {
    return (
      <div className={cn('flex h-32 items-center justify-center rounded-xl border border-ink/[0.06] bg-ink/[0.01] text-xs text-ink/45', className)}>
        No monthly returns available for this run.
      </div>
    );
  }

  return (
    <div className={cn('space-y-3', className)}>
      <div className="w-full overflow-x-auto">
        <table className="w-full min-w-[720px] border-collapse text-center text-xs">
          <thead>
            <tr className="border-b border-ink/[0.06] text-[10.5px] uppercase tracking-wider text-ink/50">
              <th className="py-2 pl-3 pr-2 text-left font-medium">Year</th>
              {MONTH_NAMES.map((m) => (
                <th key={m} className="px-1.5 py-2 font-medium">
                  {m}
                </th>
              ))}
              <th className="py-2 pl-2 pr-3 text-right font-medium">YTD</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-ink/[0.04]">
            {rows.map((row) => {
              const ytdVisual = getCellVisuals(row.ytdReturn, maxAbs);
              return (
                <tr key={row.year} className="group">
                  <td className="py-2 pl-3 pr-2 text-left font-mono text-[11px] font-semibold text-ink/80">
                    {row.year}
                  </td>
                  {row.months.map((ret, idx) => {
                    const cell = getCellVisuals(ret?.return, maxAbs);
                    return (
                      <td key={idx} className="p-1">
                        {ret ? (
                          <div
                            style={cell.style}
                            className={cn(
                              'relative flex h-8 w-full min-w-[46px] items-center justify-center rounded-md text-[11px] font-mono transition-transform duration-100 hover:scale-[1.05] hover:shadow-sm',
                              cell.className
                            )}
                            title={`${MONTH_NAMES[idx]} ${row.year}: ${signedPct(ret.return, 2)}${ret.partial ? ' (partial month)' : ''}\nStart: $${cash(ret.start_equity, 0)}\nEnd: $${cash(ret.end_equity, 0)}`}
                          >
                            <span>
                              {cell.label}
                              {ret.partial && (
                                <sup className="ml-0.5 text-[8.5px] font-bold opacity-80" title="Partial month">*</sup>
                              )}
                            </span>
                          </div>
                        ) : (
                          <div className="flex h-8 w-full min-w-[46px] items-center justify-center rounded-md bg-ink/[0.015] text-[11px] font-mono text-ink/20">
                            —
                          </div>
                        )}
                      </td>
                    );
                  })}
                  <td className="py-1 pl-2 pr-3 text-right">
                    {row.ytdReturn !== null ? (
                      <div
                        style={ytdVisual.style}
                        className={cn(
                          'inline-flex h-8 min-w-[56px] items-center justify-center rounded-md px-2 text-[11px] font-mono font-semibold shadow-xs',
                          ytdVisual.className
                        )}
                        title={`Year ${row.year} compounded return: ${signedPct(row.ytdReturn, 2)}`}
                      >
                        {ytdVisual.label}
                      </div>
                    ) : (
                      <span className="text-[11px] font-mono text-ink/25">—</span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2 px-1 text-[11px] text-ink/50">
        <div className="flex items-center gap-1.5 font-mono text-[10.5px]">
          <span className="h-2 w-2 rounded-full bg-loss" />
          <span>Loss</span>
          <span className="mx-1 text-ink/25">•</span>
          <span className="h-2 w-2 rounded-full bg-ink/30" />
          <span>0.0%</span>
          <span className="mx-1 text-ink/25">•</span>
          <span className="h-2 w-2 rounded-full bg-profit" />
          <span>Profit</span>
        </div>
        {hasPartial && (
          <div className="text-[10.5px] text-ink/45">
            <span className="font-mono font-bold text-accent">*</span> Partial calendar month
          </div>
        )}
      </div>
    </div>
  );
}
