import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * A value that would be displayed as zero, returned as +0. Without this, -0 and small negatives
 * such as -0.004 format as "-$0", "-0.00%" or "-0".
 */
export function zeroIfRoundsToZero(value: number, decimals: number): number {
  return Math.abs(value) < 0.5 * 10 ** -decimals ? 0 : value;
}

export function formatNumber(value: number, decimals = 2): string {
  value = zeroIfRoundsToZero(value, decimals);
  return value.toLocaleString('en-US', { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
}

export function formatCurrency(value: number, decimals = 0): string {
  if (!Number.isFinite(value)) return '—';
  value = zeroIfRoundsToZero(value, decimals);
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(value);
}

/** $478.5k, $1.24M */
export function formatCompactCurrency(value: number): string {
  value = zeroIfRoundsToZero(value, 1);
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    notation: 'compact',
    maximumFractionDigits: Math.abs(value) >= 1e6 ? 2 : 1,
  }).format(value);
}

export function formatPercent(value: number, decimals = 2): string {
  if (!Number.isFinite(value)) return '—';
  return `${zeroIfRoundsToZero(value * 100, decimals).toFixed(decimals)}%`;
}

export function formatDuration(ms: number): string {
  if (ms < 1) return `${(ms * 1000).toFixed(0)} µs`;
  if (ms < 1000) return `${ms.toFixed(2)} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(2)} s`;
  return `${(ms / 60000).toFixed(2)} min`;
}

/** 32.1M, 950k */
export function formatCompact(value: number, digits = 1): string {
  value = zeroIfRoundsToZero(value, digits);
  return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: digits }).format(value);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

export function confidenceLabel(c: number): string {
  const pct = c * 100;
  return `${Number.isInteger(pct) ? pct.toFixed(0) : pct.toFixed(1)}%`;
}
