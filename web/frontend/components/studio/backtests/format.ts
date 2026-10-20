/** Display helpers for backtest numbers. Null (a value the engine could not compute) shows as "—". */

export const dash = '—';

export function pct(v: number | null | undefined, digits = 2): string {
  return v == null || !Number.isFinite(v) ? dash : `${(v * 100).toFixed(digits)}%`;
}

/** A signed percentage: +4.20% / −1.05%. */
export function signedPct(v: number | null | undefined, digits = 2): string {
  if (v == null || !Number.isFinite(v)) return dash;
  const s = `${Math.abs(v * 100).toFixed(digits)}%`;
  return v > 0 ? `+${s}` : v < 0 ? `−${s}` : s;
}

export function ratio(v: number | null | undefined, digits = 2): string {
  return v == null || !Number.isFinite(v) ? dash : v.toFixed(digits);
}

export function int(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) ? dash : Math.round(v).toLocaleString('en-US');
}

/** A money value (decimal string or number) with thousands separators and 2 decimals. */
export function cash(v: string | number | null | undefined, digits = 2): string {
  const n = typeof v === 'string' ? Number(v) : v;
  if (n == null || !Number.isFinite(n)) return dash;
  return n.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** Compact money: 1.25M, 980k. */
export function compactCash(v: string | number | null | undefined): string {
  const n = typeof v === 'string' ? Number(v) : v;
  if (n == null || !Number.isFinite(n)) return dash;
  return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 2 }).format(n);
}

/** Tone for a signed number. */
export function tone(v: number | null | undefined): 'profit' | 'loss' | 'neutral' {
  if (v == null || !Number.isFinite(v) || v === 0) return 'neutral';
  return v > 0 ? 'profit' : 'loss';
}

/** Date label for a chart axis: day precision for long series, minutes for short ones. */
export function axisDate(ms: number, spanMs: number): string {
  const d = new Date(ms);
  const iso = d.toISOString();
  if (spanMs > 5 * 86_400_000) return spanMs > 2 * 365 * 86_400_000 ? iso.slice(0, 7) : iso.slice(0, 10);
  return `${iso.slice(5, 10)} ${iso.slice(11, 16)}`;
}

export function fullDate(ms: number): string {
  return new Date(ms).toISOString().replace('T', ' ').slice(0, 16) + ' UTC';
}

/** "3 min ago", "2 h ago", "Oct 18". */
export function ago(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return '';
  const s = Math.max(0, (Date.now() - t) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(t).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

export function shortHash(h: string | null | undefined, n = 12): string {
  return h ? h.slice(0, n) : dash;
}
