// M11 Task 4: CSV export for P&L attribution panels.
// Builds UTF-8 BOM CSV exports for the waterfall and instruments tables with
// metadata headers and spreadsheet formula injection sanitization.

import { sortByShare, type Attribution } from './attribution';

/**
 * Escapes text cells for CSV output and spreadsheet safety.
 *
 * Cells starting with =, +, -, @, tab, or carriage return are prefixed with a
 * single quote (') so spreadsheets do not execute them as formulas.
 * Text with commas, quotes, newlines, or leading/trailing whitespace is enclosed in
 * double quotes, and existing double quotes are doubled ("").
 */
export function escapeText(text: string): string {
  let s = text;
  if (/^[=+\-@\t\r]/.test(s)) {
    s = `'${s}`;
  }
  if (/[",\r\n]/.test(s) || s !== s.trim()) {
    return `"${s.replace(/"/g, '""')}"`;
  }
  return s;
}

/**
 * Formats a number with a fixed decimal point and no thousands separators.
 * Numeric cells must NEVER be formula-escaped so spreadsheets keep them numeric.
 */
export function formatNumber(v: number | null | undefined, decimals = 2): string {
  if (v === null || v === undefined || Number.isNaN(v)) {
    return '';
  }
  return v.toFixed(decimals);
}

/**
 * Formats a 2D matrix of values into a CSV string prefixed with the UTF-8 BOM (\uFEFF).
 */
export function toCsv(rows: (string | number | boolean | null | undefined)[][]): string {
  const lines = rows.map((row) =>
    row
      .map((cell) => {
        if (cell === null || cell === undefined) return '';
        if (typeof cell === 'number') return formatNumber(cell);
        if (typeof cell === 'boolean') return cell ? 'true' : 'false';
        return escapeText(String(cell));
      })
      .join(',')
  );
  return '\uFEFF' + lines.join('\r\n') + '\r\n';
}

export interface ExportMetadata {
  id?: string;
  source?: 'backtest' | 'paper';
  currency?: string;
  generatedAt?: string;
  costAssumptions?: string;
}

function defaultMetadata(a: Attribution, meta?: ExportMetadata): (string | number | boolean | null | undefined)[][] {
  const id = meta?.id ?? 'n/a';
  const generatedAt = meta?.generatedAt ?? new Date().toISOString();
  const currency = meta?.currency ?? 'USD';
  const slippageMode = a.costs.slippage_included_in_fill_prices
    ? 'Slippage included in fill prices'
    : 'Slippage charged separately';
  const costs =
    meta?.costAssumptions ??
    `Total costs: ${formatNumber(a.costs.total_costs)} (${slippageMode}; commission: ${formatNumber(a.costs.commission_fees)})`;

  return [
    ['# Run / Account ID', id],
    ['# Generated at (UTC)', generatedAt],
    ['# Currency', currency],
    ['# Cost assumptions', costs],
    [],
  ];
}

/**
 * Builds the Waterfall CSV export.
 */
export function buildWaterfallCsv(a: Attribution, meta?: ExportMetadata): string {
  const rows: (string | number | boolean | null | undefined)[][] = defaultMetadata(a, meta);

  rows.push(['Step', 'Change', 'Running Total', 'Subtotal']);
  for (const step of a.waterfall) {
    rows.push([
      step.name,
      step.amount,
      step.running_total,
      step.subtotal,
    ]);
  }

  return toCsv(rows);
}

/**
 * Builds the Instruments CSV export, sorted by P&L share (largest first).
 */
export function buildInstrumentsCsv(a: Attribution, meta?: ExportMetadata): string {
  const rows: (string | number | boolean | null | undefined)[][] = defaultMetadata(a, meta);

  rows.push([
    'Instrument',
    'Symbol',
    'Trades',
    'Win Rate',
    'Gross P&L',
    'Fees',
    'Net P&L',
    'P&L Share',
    'Long P&L',
    'Short P&L',
  ]);

  const sorted = sortByShare(a.instruments);
  for (const inst of sorted) {
    rows.push([
      inst.instrument,
      inst.symbol ?? `#${inst.instrument}`,
      inst.trades_count,
      inst.win_rate,
      inst.gross_pnl,
      inst.fees,
      inst.net_pnl,
      inst.pnl_share,
      inst.long_pnl,
      inst.short_pnl,
    ]);
  }

  return toCsv(rows);
}
