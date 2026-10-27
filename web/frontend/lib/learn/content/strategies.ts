import { lesson } from '../build';
import type { Lesson } from '../types';
import { BREAKOUT_DSL, MOMENTUM_DSL, RSI_DSL, SMA_DSL, TREND_DSL } from './strategySnippets';

const L = (l: Omit<Lesson, 'category'>) => lesson('strategies', l);
const STUDIO = { label: 'Open Strategy Studio', href: '/studio?tab=strategies' };
const BT = { label: 'Open Backtests', href: '/studio?tab=backtests' };

const HOW_TO_RUN =
  'Paste it into a new strategy in the **Strategy Studio**, validate it (the editor reports errors by line), save a version, and backtest that version. The rule language has no loops, no recursion and no access to future bars, so every script is bounded and free of look-ahead.';

export const strategies: Lesson[] = [
  L({
    slug: 'moving-average-crossover',
    title: 'Moving Average Crossover',
    level: 'Beginner',
    featured: true,
    summary: 'Hold the instrument while a fast moving average is above a slow one; step aside (or go short) when it falls below. A built-in strategy in Backtests.',
    keywords: ['sma', 'moving average', 'crossover', 'golden cross', 'sma_cross'],
    blocks: [
      { type: 'p', text: 'A simple moving average (SMA) is the mean of the last N closes. A short (fast) average reacts quickly to price; a long (slow) one reacts slowly. When the fast average is above the slow one, recent prices are higher than older ones: an uptrend.' },
      { type: 'diagram', id: 'sma-crossover', caption: 'Long while the fast average is above the slow one; flat when it is below.' },
      { type: 'formula', tex: String.raw`\text{SMA}_N(t) = \frac{1}{N}\sum_{k=0}^{N-1} \text{Close}_{t-k}` },
      { type: 'h', text: 'The built-in version' },
      { type: 'table', head: ['Parameter', 'Default', 'Meaning'], rows: [
        ['fast', '10', 'Fast moving-average length, in bars'],
        ['slow', '30', 'Slow moving-average length (must be longer than fast)'],
        ['qty', '100', 'Units held per instrument while in a position'],
        ['allow_short', 'false', 'Go short instead of flat when fast < slow (the configuration must allow shorting)'],
      ] },
      { type: 'p', text: 'It decides on each bar’s close and trades at the next bar’s open. Its runs reproduce the engine’s reference SMA demo fingerprint for fingerprint.' },
      { type: 'h', text: 'The same idea in the rule language' },
      { type: 'code', text: SMA_DSL, caption: 'A minimal long-only crossover for the Strategy Studio.' },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Assumes trends persist long enough to pay for the lag of the averages.', 'In sideways markets it whipsaws: many small losing trades as the averages cross back and forth.', 'Win rates are typically low (30–45%); profits come from a few long trends.', 'Results are sensitive to the two lengths. Check neighbouring values in the Survival Lab’s parameter-stability test.'] },
    ],
    related: ['strategies/trend-following', 'strategies/momentum', 'metrics/win-rate', 'engine/trade-management'],
    platformLinks: [BT, STUDIO],
  }),

  L({
    slug: 'momentum',
    title: 'Momentum',
    level: 'Intermediate',
    summary: 'Buy what has risen over a lookback window, on the evidence that recent winners tend to keep winning for a while.',
    keywords: ['momentum', 'rate of change', 'roc', 'time series momentum', 'relative strength'],
    blocks: [
      { type: 'p', text: 'Momentum strategies rank or filter instruments by their past return. **Time-series** momentum holds an instrument while its own return over the lookback is positive; **cross-sectional** momentum holds the best performers relative to the others.' },
      { type: 'formula', tex: String.raw`\text{Momentum}_t = \frac{\text{Close}_t}{\text{Close}_{t-L}} - 1` },
      { type: 'h', text: 'In the rule language' },
      { type: 'p', text: '`close[lookback]` reads the close `lookback` bars ago. Indexing only looks backwards; the validator rejects any reference to future bars.' },
      { type: 'code', text: MOMENTUM_DSL, caption: 'Time-series momentum: hold while the return over the lookback beats the threshold.' },
      { type: 'p', text: HOW_TO_RUN },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Assumes under-reaction to news and herding keep trends going.', '**Momentum crashes**: sharp reversals after market bottoms can produce large, fast losses.', 'Lookbacks are usually months (for daily bars, 60–250 bars); very short lookbacks behave more like noise.', 'Cross-sectional momentum needs many instruments and is not expressible with a single-instrument script.'] },
    ],
    related: ['strategies/trend-following', 'strategies/moving-average-crossover', 'metrics/skewness-and-kurtosis'],
    platformLinks: [STUDIO, BT],
  }),

  L({
    slug: 'mean-reversion',
    title: 'Mean Reversion (Z-Score)',
    level: 'Intermediate',
    summary: 'Buy when price is unusually far below its recent average and exit when it returns. The built-in z-score reversion strategy does exactly this.',
    keywords: ['mean reversion', 'z score', 'bollinger', 'zscore_reversion', 'statistical arbitrage'],
    blocks: [
      { type: 'p', text: 'Mean reversion assumes that prices oscillate around a moving centre and that large deviations tend to shrink. The z-score measures the deviation in standard deviations, so the same rule works across volatile and quiet instruments.' },
      { type: 'formula', tex: String.raw`z_t = \frac{\text{Close}_t - \mu_t}{\sigma_t}` },
      { type: 'p', text: '\\(\\mu_t\\) and \\(\\sigma_t\\) are the mean and standard deviation of the last `lookback` closes.' },
      { type: 'diagram', id: 'mean-reversion' },
      { type: 'h', text: 'The built-in version' },
      { type: 'table', head: ['Parameter', 'Default', 'Meaning'], rows: [
        ['lookback', '20', 'Window for the rolling mean and standard deviation, in bars'],
        ['entry_z', '2.0', 'Enter when the close is this many standard deviations from the mean'],
        ['exit_z', '0.5', 'Exit once it is back within this many (must be below entry_z)'],
        ['qty', '100', 'Units per position'],
        ['allow_short', 'false', 'Also short above +entry_z (the configuration must allow shorting)'],
      ] },
      { type: 'p', text: 'Long below −entry_z, out once z rises above −exit_z; with shorting on, short above +entry_z and out once z falls below +exit_z.' },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Assumes the mean is stable. In a strong trend price can stay “extreme” for a long time, and losses grow while you wait.', 'High win rates with occasional large losses (negative skew): check the worst trade and the drawdown, not just the win rate.', 'Many short trades: sensitive to costs. Borrow fees matter if you short.'] },
    ],
    related: ['strategies/rsi-strategies', 'metrics/win-rate', 'metrics/skewness-and-kurtosis', 'metrics/financing'],
    platformLinks: [BT],
  }),

  L({
    slug: 'breakout-trading',
    title: 'Breakout Trading',
    level: 'Intermediate',
    summary: 'Buy when price closes above the recent range, betting that the move continues. Written in the Strategy Studio rule language.',
    keywords: ['breakout', 'donchian', 'channel', 'range', 'new high'],
    blocks: [
      { type: 'p', text: 'A breakout strategy waits for price to leave a range (for example, close above the highest high of the last N bars) and enters in the direction of the break. Classic versions use 20- or 55-bar channels.' },
      { type: 'formula', tex: String.raw`\text{Enter long when } \text{Close}_t > \max(\text{High}_{t-1}, \dots, \text{High}_{t-N})` },
      { type: 'h', text: 'In the rule language' },
      { type: 'p', text: 'The language has no loops and no rolling-maximum function, so a short channel is written with nested `max` over `high[1]` … `high[N]`. This example uses a 5-bar channel and exits below a moving average.' },
      { type: 'code', text: BREAKOUT_DSL, caption: 'Five-bar breakout with a moving-average exit.' },
      { type: 'p', text: HOW_TO_RUN },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Assumes ranges are broken by real shifts in supply and demand, not noise.', '**False breakouts** are common: price pokes above the range and falls back. Exits and costs decide profitability.', 'Breakouts often happen on gaps; market orders then fill at the next open, which may be well above the breakout level.', 'Longer channels need more nested terms; keep them readable or use a moving-average band instead.'] },
    ],
    related: ['strategies/trend-following', 'strategies/momentum', 'fundamentals/slippage'],
    platformLinks: [STUDIO, BT],
  }),

  L({
    slug: 'trend-following',
    title: 'Trend Following',
    level: 'Intermediate',
    summary: 'Stay with the prevailing direction using long-term filters, accepting many small losses for a few large gains.',
    keywords: ['trend following', 'ema', 'trend filter', 'cta', 'managed futures'],
    blocks: [
      { type: 'p', text: 'Trend following is a family of rules (moving-average crossovers, breakouts, momentum filters) that share one belief: large moves tend to continue. They do not predict; they react, and they cut positions when the trend ends.' },
      { type: 'formula', tex: String.raw`\text{EMA}_t = k\,\text{Close}_t + (1 - k)\,\text{EMA}_{t-1}, \quad k = \frac{2}{N+1}` },
      { type: 'h', text: 'In the rule language' },
      { type: 'code', text: TREND_DSL, caption: 'Long while price is above a long exponential average and the medium-term average agrees.' },
      { type: 'p', text: HOW_TO_RUN },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Typical profile: win rate below 50%, payoff ratio well above 1, positive skew.', 'Long flat or losing stretches in range-bound markets; the longest-underwater statistic matters.', 'Works best across many diversified instruments; a single-instrument test understates both its risk and its diversification.', 'Exponential averages put more weight on recent bars than simple ones, so they react faster.'] },
    ],
    related: ['strategies/moving-average-crossover', 'strategies/breakout-trading', 'metrics/risk-reward-ratio', 'metrics/drawdown-duration'],
    platformLinks: [STUDIO, BT],
  }),

  L({
    slug: 'rsi-strategies',
    title: 'RSI-Based Strategies',
    level: 'Intermediate',
    summary: 'The Relative Strength Index scales recent gains against losses from 0 to 100; low readings are often traded as oversold bounces.',
    keywords: ['rsi', 'relative strength index', 'oversold', 'overbought', 'oscillator'],
    blocks: [
      { type: 'p', text: 'RSI compares the size of recent up-moves with recent down-moves. Readings below 30 are traditionally called oversold and above 70 overbought.' },
      { type: 'formula', tex: String.raw`\text{RSI} = 100 - \frac{100}{1 + RS}, \qquad RS = \frac{\text{average gain over } N}{\text{average loss over } N}` },
      { type: 'callout', tone: 'info', title: 'How the rule language computes RSI', text: '`rsi(N)` uses simple averages of the close-to-close gains and losses over the last N bars (not Wilder’s smoothed averages), returns 50 until there are more than N bars, and 100 when there were no losses. Values can differ slightly from charting packages that use Wilder smoothing.' },
      { type: 'code', text: RSI_DSL, caption: 'Buy oversold readings, exit when RSI recovers.' },
      { type: 'p', text: HOW_TO_RUN },
      { type: 'h', text: 'Assumptions and limitations' },
      { type: 'list', items: ['Used this way RSI is a mean-reversion tool: it assumes oversold conditions bounce.', 'In strong downtrends RSI can stay below 30 for weeks; consider a trend filter (only buy oversold readings above a long moving average).', 'Thresholds (30/70) are conventions, not laws. Test neighbours rather than optimizing to a single value.'] },
    ],
    related: ['strategies/mean-reversion', 'strategies/trend-following', 'simulations/overfitting-and-validation'],
    platformLinks: [STUDIO, BT],
  }),

  L({
    slug: 'buy-and-hold-and-equal-weight',
    title: 'Buy-and-Hold & Equal Weight',
    level: 'Beginner',
    summary: 'Two built-in benchmark strategies: buy once and never trade, or keep every instrument at the same weight with periodic rebalancing.',
    keywords: ['buy and hold', 'equal weight', 'rebalancing', 'benchmark', 'passive'],
    blocks: [
      { type: 'p', text: 'Every active strategy should be compared with doing almost nothing. Spectra ships two built-in baselines.' },
      { type: 'h', text: 'Buy and hold' },
      { type: 'p', text: 'On the first bar it splits a share of the cash (`allocation`) equally across the instruments present and buys whole units at the next open. It never trades again. The unallocated share covers costs and opening gaps.' },
      { type: 'h', text: 'Equal weight' },
      { type: 'p', text: 'Every instrument seen so far gets the same share of equity. The strategy sets target weights and the engine’s rebalancer turns them into orders, either every `rebalance_every` bars or, with that set to 0, whenever any weight drifts more than `drift` from its target. `gross` sets the share of the leverage limit to invest; `lot_size` and `min_trade` avoid tiny trades.' },
      { type: 'formula', tex: String.raw`w_i = \frac{\text{gross}}{N}, \qquad \text{trade}_i = w_i E - V_i` },
      { type: 'h', text: 'Why they matter' },
      { type: 'list', items: ['Each backtest’s benchmark card already compares your strategy with a cost-free equal-weight buy-and-hold of the same instruments.', 'Running them as real backtests (with costs) gives a fairer baseline.', 'Equal weight systematically sells winners and buys losers at each rebalance: a mild mean-reversion tilt.'] },
    ],
    related: ['metrics/benchmark-excess-return', 'engine/portfolio-management', 'metrics/alpha'],
    platformLinks: [BT],
  }),
];
