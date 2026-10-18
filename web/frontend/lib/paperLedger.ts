import type { PortfolioStats } from './portfolio';

// A simulated paper account, computed in the browser. There is no brokerage connection:
// `/api/alpaca/*` answers 501 in the Rust API. The book is assumed bought at the CSV
// prices from `startingCapital`, with no market data to mark it afterwards, so P&L is zero
// and equity equals starting capital.

export interface PaperSettings {
  startingCapital: number;
  /** Gross leverage limit, as enforced by the backtester's ledger (default 1.0×). */
  leverageLimit: number;
}

export const DEFAULT_PAPER: PaperSettings = { startingCapital: 1_000_000, leverageLimit: 1 };

export interface PaperAccount {
  equity: number;
  /** Cash after buying longs (and receiving proceeds from shorts). */
  cash: number;
  grossExposure: number;
  grossLeverage: number;
  buyingPower: number;
  withinLimit: boolean;
}

export function paperAccount(stats: PortfolioStats, s: PaperSettings): PaperAccount {
  const equity = s.startingCapital;
  const cash = equity - stats.netValue;
  const grossLeverage = equity > 0 ? stats.grossExposure / equity : Infinity;
  return {
    equity,
    cash,
    grossExposure: stats.grossExposure,
    grossLeverage,
    buyingPower: Math.max(0, s.leverageLimit * equity - stats.grossExposure),
    withinLimit: grossLeverage <= s.leverageLimit + 1e-12,
  };
}
