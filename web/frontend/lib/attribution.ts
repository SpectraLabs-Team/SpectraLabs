// M11: P&L attribution for a backtest or a paper account. The shapes mirror the engine's JSON
// (`crates/spectra-research/src/attribution.rs`, `PnlAttribution::to_json`). Pure helpers only:
// the panel renders them, and `scripts/test-attribution.mjs` checks them.

export interface AttributionWaterfallStep {
  name: string;
  /** The step's change; for a subtotal, the subtotal itself. */
  amount: number;
  running_total: number;
  /** True for a level drawn from zero (the start and the subtotals). */
  subtotal: boolean;
}

export interface AttributionInstrument {
  instrument: number;
  symbol: string | null;
  gross_pnl: number;
  fees: number;
  net_pnl: number;
  trades_count: number;
  win_trades: number;
  loss_trades: number;
  win_rate: number;
  long_pnl: number;
  short_pnl: number;
  pnl_share: number;
}

export interface AttributionDirection {
  long_gross_pnl: number;
  long_fees: number;
  long_net_pnl: number;
  long_trades: number;
  long_wins: number;
  long_win_rate: number;
  short_gross_pnl: number;
  short_fees: number;
  short_net_pnl: number;
  short_trades: number;
  short_wins: number;
  short_win_rate: number;
}

export interface AttributionCosts {
  commission_fees: number;
  slippage_costs: number;
  slippage_included_in_fill_prices: boolean;
  borrow_fees: number;
  financing_interest: number;
  interest_income: number;
  dividends: number;
  coupons: number;
  total_costs: number;
}

export interface Attribution {
  total_gross_pnl: number;
  total_fees: number;
  total_net_pnl: number;
  open_position_pnl: number;
  total_account_pnl: number;
  total_trades: number;
  overall_win_rate: number;
  costs: AttributionCosts;
  direction: AttributionDirection;
  instruments: AttributionInstrument[];
  waterfall: AttributionWaterfallStep[];
}

/** The combined attribution of every paper strategy in a workspace (M11 Task 7). */
export interface WorkspaceAttribution extends Attribution {
  /** How many paper strategies were combined. */
  strategy_count: number;
}

/** One bar of the waterfall chart, as fractions of the chart's full width (0 to 1). */
export interface WaterfallBar {
  name: string;
  amount: number;
  running_total: number;
  subtotal: boolean;
  /** Where the bar starts and ends on the value axis, in money. */
  from: number;
  to: number;
  /** Left edge and width, as fractions of the chart width. */
  left: number;
  width: number;
  tone: 'profit' | 'loss' | 'neutral';
}

/** True when the run has no fills at all: there is nothing to attribute. */
export function hasNoFills(a: Attribution): boolean {
  return a.total_trades === 0 && a.instruments.length === 0 && a.costs.commission_fees === 0 && a.open_position_pnl === 0;
}

/** Instruments by P&L share, largest first (signed, so the biggest winner leads). */
export function sortByShare(instruments: AttributionInstrument[]): AttributionInstrument[] {
  return [...instruments].sort((x, y) => y.pnl_share - x.pnl_share || x.instrument - y.instrument);
}

/**
 * Bar geometry for the waterfall. Subtotals are drawn from zero; a change is drawn from the
 * previous running total to the new one. The axis spans the lowest and highest value reached,
 * always including zero, so negative totals draw correctly.
 */
export function waterfallBars(steps: AttributionWaterfallStep[]): WaterfallBar[] {
  if (steps.length === 0) return [];
  const levels: number[] = [0];
  let previous = 0;
  const spans = steps.map((s) => {
    const from = s.subtotal ? 0 : previous;
    const to = s.running_total;
    levels.push(from, to);
    previous = to;
    return { from, to };
  });
  const lo = Math.min(...levels);
  const hi = Math.max(...levels);
  const span = hi - lo || 1;
  return steps.map((s, i) => {
    const { from, to } = spans[i];
    const a = Math.min(from, to);
    const b = Math.max(from, to);
    const isPositive = s.subtotal ? s.running_total > 0 : s.amount > 0;
    const isZero = s.subtotal ? s.running_total === 0 : s.amount === 0;
    return {
      name: s.name,
      amount: s.amount,
      running_total: s.running_total,
      subtotal: s.subtotal,
      from,
      to,
      left: (a - lo) / span,
      width: (b - a) / span,
      tone: isZero ? 'neutral' : isPositive ? 'profit' : 'loss',
    };
  });
}

/** Share of the absolute net P&L, as a percentage string. */
export function sharePct(share: number): string {
  if (!Number.isFinite(share)) return '—';
  return `${(share * 100).toFixed(1)}%`;
}
