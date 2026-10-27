/**
 * Strategy Studio rule-language examples used in the strategy lessons.
 * Each one was checked with `spectra_engine::dsl::parse_and_validate` and run through a backtest
 * when it was written; keep them valid if the language changes.
 */

export const SMA_DSL = `param fast: int = 10, min = 2, max = 100;
param slow: int = 30, min = 5, max = 300;

on_bar {
    let fast_ma = sma(fast);
    let slow_ma = sma(slow);

    if fast_ma > slow_ma {
        target_shares(100);
    } else {
        target_shares(0);
    }
}`;

export const MOMENTUM_DSL = `param lookback: int = 60, min = 2, max = 500, desc = "Bars to measure momentum over";
param threshold: float = 0.0, min = -1.0, max = 1.0, desc = "Minimum return over the lookback to hold";
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
}`;

export const BREAKOUT_DSL = `param qty: float = 100.0, min = 1.0, max = 1000000.0;
param exit_len: int = 20, min = 2, max = 500, desc = "Exit when the close falls below this moving average";

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
}`;

export const TREND_DSL = `param trend_len: int = 100, min = 10, max = 1000, desc = "Long-term trend filter";
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
}`;

export const RSI_DSL = `param period: int = 14, min = 2, max = 100;
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
}`;
