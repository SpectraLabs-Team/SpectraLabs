/** Display helpers for the Data tab. */
export { ago, dash, int } from '../backtests/format';

export function bytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** "2024-01-02" for a whole-day timestamp, else "2024-01-02 09:15 UTC". */
export function day(iso: string | null | undefined): string {
  if (!iso) return '—';
  const [d, t = ''] = iso.replace('Z', '').split('T');
  return t && !/^00:00(:00)?(\.0+)?$/.test(t) ? `${d} ${t.slice(0, 5)} UTC` : d;
}

export function range(from: string | null | undefined, to: string | null | undefined): string {
  if (!from || !to) return '—';
  return from === to ? day(from) : `${day(from)} → ${day(to)}`;
}

/** A duration in nanoseconds as "1 d", "5 min", "2 h 30 min". */
export function duration(ns: number | null | undefined): string {
  if (ns == null || !Number.isFinite(ns) || ns <= 0) return '—';
  const s = ns / 1e9;
  if (s < 60) return `${+s.toFixed(2)} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  if (h < 48) return rm ? `${h} h ${rm} min` : `${h} h`;
  return `${+(h / 24).toFixed(1)} d`;
}

/** Rejection categories in words. */
export const CATEGORY: Record<string, string> = {
  column_count: 'Wrong number of fields',
  empty_symbol: 'Empty symbol',
  bad_timestamp: 'Unreadable timestamp',
  bad_number: 'Unreadable number',
  non_positive_price: 'Price ≤ 0',
  ohlc_inconsistent: 'High/low don’t bracket open/close',
  negative_volume: 'Negative volume',
  duplicate: 'Exact duplicate',
  duplicate_conflict: 'Conflicting duplicate',
};
