/**
 * Curated Common Quantitative Strategies for the Strategy Studio.
 * Tested against the spectra_engine DSL parser and engine runtime.
 */

export interface CommonStrategyTemplate {
  id: string;
  name: string;
  category: 'Trend Following' | 'Momentum' | 'Breakout' | 'Mean Reversion';
  description: string;
  code: string;
  defaultParams: Record<string, unknown>;
}

export const COMMON_STRATEGY_TEMPLATES: CommonStrategyTemplate[] = [
  {
    id: 'sma-cross',
    name: 'Dual Moving Average (SMA Cross)',
    category: 'Trend Following',
    description: 'Classic trend-following system entering long when the fast 10-bar SMA crosses above the slow 30-bar SMA, and flat otherwise.',
    defaultParams: { fast: 10, slow: 30 },
    code: `// Dual Moving Average (SMA Cross)
param fast: int = 10, min = 2, max = 100, desc = "Fast moving average period";
param slow: int = 30, min = 5, max = 300, desc = "Slow moving average period";
param qty: float = 100.0, min = 1.0, max = 1000000.0, desc = "Target order shares";

on_bar {
    let fast_ma = sma(fast);
    let slow_ma = slow > 0 ? sma(slow) : close;

    if fast_ma > slow_ma {
        target_shares(qty);
    } else {
        target_shares(0);
    }
}
`,
  },
  {
    id: 'momentum',
    name: 'Time-Series Momentum',
    category: 'Momentum',
    description: 'Lookback return filter holding positive price momentum assets over a 60-bar lookback window.',
    defaultParams: { lookback: 60, threshold: 0.0, qty: 100.0 },
    code: `// Time-Series Momentum
param lookback: int = 60, min = 2, max = 500, desc = "Bars to measure momentum over";
param threshold: float = 0.0, min = -1.0, max = 1.0, desc = "Minimum return over lookback to hold";
param qty: float = 100.0, min = 1.0, max = 1000000.0;

on_bar {
    if bars_count <= lookback {
        return;
    }
    let past = close[lookback];
    let momentum = close / past - 1.0;
    if momentum > threshold {
        target_shares(qty);
    } else {
        target_shares(0);
    }
}
`,
  },
  {
    id: 'donchian-breakout',
    name: 'Donchian Breakout',
    category: 'Breakout',
    description: 'Enters long when the close breaks above the recent 5-bar high, with a trailing 20-period moving average stop.',
    defaultParams: { qty: 100.0, exit_len: 20 },
    code: `// Donchian Breakout
param qty: float = 100.0, min = 1.0, max = 1000000.0;
param exit_len: int = 20, min = 2, max = 500, desc = "Exit when close falls below moving average";

on_bar {
    if bars_count < 6 {
        return;
    }
    let recent_high = max(max(max(high[1], high[2]), max(high[3], high[4])), high[5]);
    if position == 0 and close > recent_high {
        target_shares(qty);
    } else if position > 0 and close < sma(exit_len) {
        target_shares(0);
    }
}
`,
  },
  {
    id: 'ema-trend',
    name: 'Dual EMA Trend Following',
    category: 'Trend Following',
    description: 'Long-term exponential trend filter combining the 20-period and 100-period EMAs to stay with strong macro trends.',
    defaultParams: { trend_len: 100, qty: 100.0 },
    code: `// Dual EMA Trend Following
param trend_len: int = 100, min = 10, max = 1000, desc = "Long-term trend filter";
param qty: float = 100.0, min = 1.0, max = 1000000.0;

on_bar {
    if bars_count < trend_len {
        return;
    }
    if close > ema(trend_len) and ema(20) > ema(trend_len) {
        target_shares(qty);
    } else {
        target_shares(0);
    }
}
`,
  },
  {
    id: 'rsi-reversion',
    name: 'RSI Mean Reversion',
    category: 'Mean Reversion',
    description: 'Counter-trend mean reversion strategy buying oversold RSI (<30) and exiting when RSI recovers above 55.',
    defaultParams: { period: 14, oversold: 30.0, exit_level: 55.0, qty: 100.0 },
    code: `// RSI Mean Reversion
param period: int = 14, min = 2, max = 100;
param oversold: float = 30.0, min = 1.0, max = 50.0;
param exit_level: float = 55.0, min = 30.0, max = 90.0;
param qty: float = 100.0, min = 1.0, max = 1000000.0;

on_bar {
    if bars_count <= period {
        return;
    }
    let r = rsi(period);
    if position == 0 and r < oversold {
        target_shares(qty);
    } else if position > 0 and r > exit_level {
        target_shares(0);
    }
}
`,
  },
];
