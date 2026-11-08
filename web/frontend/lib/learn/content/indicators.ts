import type { Lesson } from '../types';

export const indicators: Lesson[] = [
  // 1. SMA
  {
    slug: 'sma',
    category: 'indicators',
    title: 'Simple Moving Average (SMA)',
    level: 'Beginner',
    summary: 'The unweighted arithmetic mean of closing prices over a rolling window of N bars.',
    keywords: ['sma', 'simple moving average', 'sma trend'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Simple Moving Average (SMA) smooths out market price fluctuations by calculating the arithmetic mean of closing prices over a specified number of consecutive bars. It acts as an unweighted baseline trend filter, helping traders determine the broader direction of price movement without the distracting noise of day-to-day volatility.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{SMA}_n(t) = \frac{1}{n} \sum_{i=0}^{n-1} P_{t-i}`,
        caption: 'Arithmetic mean of the previous n closing prices.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The standard baseline parameter is **period = 20** for short-to-medium trend analysis, with **period = 50** and **period = 200** commonly employed for institutional trend regimes. When the current market price trades consistently above the SMA line, the market is in an established uptrend; when price trades below the SMA line, the market is in a downtrend.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because every bar within the window receives equal weighting, old price shocks produce a sudden drop or rise when they fall out of the calculation window, known as the drop-off effect. Furthermore, in sideways or consolidating markets, moving averages frequently whipsaw, causing false crossover signals and repetitive transaction losses.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'The Spectra engine computes SMAs strictly using historical bars up to bar t. The initial n - 1 positions evaluate to null during warm-up to prevent look-ahead bias.',
      },
    ],
    related: ['indicators/ema', 'strategies/moving-average-crossover'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 2. EMA
  {
    slug: 'ema',
    category: 'indicators',
    title: 'Exponential Moving Average (EMA)',
    level: 'Beginner',
    summary: 'A weighted moving average applying exponentially decaying weights to older price observations.',
    keywords: ['ema', 'exponential moving average', 'ema smoothing'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Exponential Moving Average (EMA) assigns greater weight and significance to the most recent data points while exponentially diminishing the influence of older bars. This weighting mechanism reduces lag compared to a simple moving average of identical length, making the EMA significantly more responsive to immediate price inflections and trend transitions.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{EMA}_t = \alpha \cdot P_t + (1 - \alpha) \cdot \text{EMA}_{t-1}, \quad \alpha = \frac{2}{n + 1}`,
        caption: 'Recursive exponential weighting with smoothing factor alpha.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The standard default parameter is **period = 20**, seeded on bar n - 1 with the initial arithmetic mean. Because recent price moves carry disproportionate weight, rapid departures from the EMA line signify acceleration in market momentum. Faster periods like 9 and 12 are widely used in oscillator calculations like the MACD.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Greater sensitivity to recent price changes comes with increased susceptibility to market noise and head-fakes. Relying on an isolated EMA cross without confirming volume or trend filters can produce frequent false entries during volatile consolidations.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Spectra seeds the EMA recursively starting from the SMA of the first n bars, ensuring full numerical parity with institutional benchmark datasets.',
      },
    ],
    related: ['indicators/sma', 'indicators/macd'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 3. WMA
  {
    slug: 'wma',
    category: 'indicators',
    title: 'Weighted Moving Average (WMA)',
    level: 'Intermediate',
    summary: 'A linearly weighted moving average where weights decrease in arithmetic progression.',
    keywords: ['wma', 'weighted moving average', 'linear weighting'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Weighted Moving Average (WMA) weights prices in linear arithmetic progression, giving the current bar weight n, the prior bar weight n - 1, down to weight 1 for the oldest bar in the window. This provides a balance between the lag of an SMA and the exponential tail of an EMA.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{WMA}_n(t) = \frac{\sum_{i=0}^{n-1} (n - i) \cdot P_{t-i}}{\frac{n(n + 1)}{2}}`,
        caption: 'Linearly weighted sum divided by the triangle number denominator.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The default parameter is **period = 20**. The denominator is the sum of integers from 1 to n, equal to n(n + 1)/2. The WMA responds more quickly to turning points than an equivalent SMA while preserving a finite memory window.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Like all moving averages, WMA will lag turning points during sudden reversals. In choppy regimes, the linear weighting on recent bars can amplify reactions to single erratic candles.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'The first n - 1 bars evaluate to null. All weights are strictly deterministic and verify zero look-ahead bias across all asset classes.',
      },
    ],
    related: ['indicators/sma', 'indicators/ema'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 4. DEMA
  {
    slug: 'dema',
    category: 'indicators',
    title: 'Double Exponential Moving Average (DEMA)',
    level: 'Intermediate',
    summary: 'A double-smoothed composite average engineered to eliminate trend-following lag.',
    keywords: ['dema', 'double exponential moving average'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Double Exponential Moving Average (DEMA) combines a single EMA and a smoothed double EMA to cancel out the mathematical lag inherent in standard moving averages. Developed by Patrick Mulloy, DEMA is designed to track price trends far more closely than an EMA of the same period without introducing erratic overshoot.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{DEMA}_t = 2 \cdot \text{EMA}_t - \text{EMA}(\text{EMA})_t`,
        caption: 'Double-smoothed lag reduction formula.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The default parameter is **period = 20**. DEMA tracks price turns aggressively; because it removes lag, traders use it to identify early trend reversals and as a faster trigger for breakout strategies.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'DEMA is not simply an EMA with double the period. Because it aggressively subtracts lag, sharp spikes can cause DEMA to react erratically, producing premature exit signals if volatility expands rapidly.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Requires warm-up of 2n - 1 bars to fully seed both internal recursive EMA filters before delivering valid numerical output.',
      },
    ],
    related: ['indicators/ema', 'indicators/tema'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 5. TEMA
  {
    slug: 'tema',
    category: 'indicators',
    title: 'Triple Exponential Moving Average (TEMA)',
    level: 'Intermediate',
    summary: 'A triple-smoothed average that further eliminates lag for ultra-fast trend tracking.',
    keywords: ['tema', 'triple exponential moving average'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Triple Exponential Moving Average (TEMA) takes lag reduction one step further than DEMA by calculating a triple-smoothed composite of three consecutive EMAs. It provides an exceptionally smooth curve that clings tightly to price action, making it suitable for fast momentum detection in high-frequency or volatile environments.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{TEMA}_t = 3 \cdot \text{EMA}_t - 3 \cdot \text{EMA}(\text{EMA})_t + \text{EMA}(\text{EMA}(\text{EMA}))_t`,
        caption: 'Third-order lag cancellation equation.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The default parameter is **period = 20**. TEMA crossovers offer ultra-responsive signals, changing slope almost immediately upon a momentum shift.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'The extreme responsiveness of TEMA makes it prone to frequent reversals in range-bound markets. It is best coupled with a volatility filter such as Average True Range (ATR) to avoid over-trading.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'TEMA enforces a strict warm-up period of 3n - 2 bars to ensure all three cascaded EMA stages reach steady-state accuracy.',
      },
    ],
    related: ['indicators/dema', 'indicators/ema'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 6. BB
  {
    slug: 'bb',
    category: 'indicators',
    title: 'Bollinger Bands (BB)',
    level: 'Beginner',
    summary: 'Volatility envelope consisting of a moving average and dynamic standard deviation bands.',
    keywords: ['bb', 'bollinger bands', 'bollinger squeeze'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Bollinger Bands measure market volatility and relative price extremity. Developed by John Bollinger, the indicator places dynamic upper and lower bands around an SMA based on rolling standard deviations. The bands expand during high-volatility expansions and contract during low-volatility consolidations.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{Upper} = \text{SMA}_n + k \cdot \sigma_n, \quad \text{Lower} = \text{SMA}_n - k \cdot \sigma_n, \quad \sigma_n = \sqrt{\frac{1}{n}\sum (P_i - \bar{P})^2}`,
        caption: 'Middle SMA band bracketed by k sample standard deviations.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard default parameters are **period = 20** and **mult = 2.0**. In a normal distribution, approximately 95% of observations occur within two standard deviations. When the bands contract tightly into a squeeze, an explosive volatility breakout frequently follows.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Touching the upper or lower band is not inherently an overbought or oversold sell/buy signal. In strong trending markets, prices can walk the bands for extended periods, causing severe losses for naive mean-reversion systems.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'The Spectra API returns three aligned series: middle, upper, and lower. Warm-up requires period - 1 null values across all three outputs.',
      },
    ],
    related: ['indicators/sma', 'indicators/keltner', 'strategies/mean-reversion'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 7. ICHIMOKU
  {
    slug: 'ichimoku',
    category: 'indicators',
    title: 'Ichimoku Kinko Hyo (Cloud)',
    level: 'Advanced',
    summary: 'Comprehensive equilibrium chart system providing support, resistance, momentum, and trend direction.',
    keywords: ['ichimoku', 'ichimoku cloud', 'kumo cloud', 'tenkan sen', 'kijun sen'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Ichimoku Kinko Hyo (Equilibrium Chart at a Glance) is a versatile system developed by Goichi Hosoda. It defines dynamic support and resistance levels, gauges trend direction and momentum, and provides actionable trade signals through five calculated lines and a shaded cloud (Kumo).',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{Tenkan} = \frac{\max(H_9) + \min(L_9)}{2}, \quad \text{Kijun} = \frac{\max(H_{26}) + \min(L_{26})}{2}`,
        caption: 'Tenkan-sen and Kijun-sen midpoints.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard parameters are **tenkan = 9**, **kijun = 26**, and **senkou_b = 52**. Senkou Span A is the average of Tenkan and Kijun; Senkou Span B is the 52-period midpoint. Price trading above the Kumo cloud indicates a bullish regime; price below the Kumo cloud indicates a bearish regime.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'The cloud components are traditionally plotted shifted forward into the future on chart platforms. However, in automated algorithmic backtesting, forward shifts must never introduce future look-ahead into current bar execution decisions.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Spectra returns five series: tenkan_sen, kijun_sen, senkou_span_a, senkou_span_b, and chikou_span, strictly aligned to historical time t.',
      },
    ],
    related: ['indicators/sma', 'indicators/psar', 'strategies/trend-following'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 8. PSAR
  {
    slug: 'psar',
    category: 'indicators',
    title: 'Parabolic SAR (Stop and Reverse)',
    level: 'Intermediate',
    summary: 'Trailing price-and-time stop system designed to identify trend continuation and exit thresholds.',
    keywords: ['psar', 'parabolic sar', 'stop and reverse'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Parabolic SAR (Stop and Reverse), developed by J. Welles Wilder, provides trailing price stops based on both price movement and the passage of time. As a trend matures, the dots accelerate toward the current price, narrowing the permissible adverse excursion and locking in accumulated profits.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{SAR}_{t+1} = \text{SAR}_t + \alpha \cdot (\text{EP} - \text{SAR}_t), \quad \alpha_{new} = \min(\alpha + \text{step}, \alpha_{max})`,
        caption: 'Recursive acceleration formula toward Extreme Price (EP).',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard default parameters are **af_step = 0.02** and **af_max = 0.20**. When dots are below candles, the trend is long; when price crosses through the SAR level, the indicator flips above candles, triggering a reverse stop.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Parabolic SAR assumes the market is constantly trending in one direction or the other. In range-bound sideways chop, it whipsaws aggressively, generating costly false exits.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Rendered on the price pane as discrete dots above or below candles, maintaining Wilder’s exact rule that SAR cannot penetrate the prior two bars’ extreme prices.',
      },
    ],
    related: ['indicators/atr', 'indicators/ema', 'strategies/trend-following'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 9. DONCHIAN
  {
    slug: 'donchian',
    category: 'indicators',
    title: 'Donchian Channels',
    level: 'Beginner',
    summary: 'Price envelope formed by highest high and lowest low over an N-bar lookback window.',
    keywords: ['donchian', 'donchian channels', 'donchian channel'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Donchian Channels, pioneered by Richard Donchian, plot the highest high and lowest low reached over the preceding N bars, along with their arithmetic median. It forms the backbone of classic trend-following breakout systems like the Turtle Trading strategy.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{Upper} = \max(H_{t-n+1..t}), \quad \text{Lower} = \min(L_{t-n+1..t}), \quad \text{Middle} = \frac{\text{Upper} + \text{Lower}}{2}`,
        caption: 'Highest high and lowest low extremes over N bars.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default lookback is **period = 20** (with 55 frequently used for multi-month breakouts). A candle closing above the upper channel signals a bullish breakout; a close below the lower channel signals a breakdown.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'In low-volatility regimes, breakout signals are vulnerable to false expansions. Turtle-style breakouts require wide risk budgets and disciplined sizing to weather multiple scratches before catching major trend runs.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Outputs upper, lower, and middle channel series, rendered as an overlay with translucent area shading between bounds.',
      },
    ],
    related: ['indicators/keltner', 'indicators/bb', 'strategies/breakout-trading'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 10. KELTNER
  {
    slug: 'keltner',
    category: 'indicators',
    title: 'Keltner Channels',
    level: 'Intermediate',
    summary: 'Volatility envelope based on an Exponential Moving Average bracketed by Average True Range bands.',
    keywords: ['keltner', 'keltner channels', 'keltner channel'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Keltner Channels construct a dynamic price envelope by centering on an EMA and extending upper and lower boundaries based on Average True Range (ATR). Unlike Bollinger Bands, which use standard deviation, Keltner Channels utilize ATR, producing smoother and less jittery volatility channels.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{Upper} = \text{EMA}_n(C) + m \cdot \text{ATR}_k, \quad \text{Lower} = \text{EMA}_n(C) - m \cdot \text{ATR}_k`,
        caption: 'EMA center line bounded by ATR volatility multiples.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard parameters are **period = 20**, **mult = 2.0**, and **atr_period = 10**. Channel penetration confirms strong underlying momentum, while price returning inside indicates consolidation.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because Keltner bands expand and contract more smoothly than Bollinger Bands, comparing Bollinger Bands inside Keltner Channels (the classic Squeeze setup) is required to gauge true consolidation tightness.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Returns middle, upper, and lower aligned series with zero look-ahead warm-up handling.',
      },
    ],
    related: ['indicators/bb', 'indicators/atr', 'indicators/ema'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 11. VWAP
  {
    slug: 'vwap',
    category: 'indicators',
    title: 'Volume Weighted Average Price (VWAP)',
    level: 'Intermediate',
    summary: 'Cumulative volume-weighted benchmark price reflecting true institutional execution value.',
    keywords: ['vwap', 'volume weighted average price', 'vwap benchmark'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Volume Weighted Average Price (VWAP) calculates the cumulative dollar value traded divided by total shares traded over a given time horizon. Institutional execution desks use VWAP as their primary execution efficiency benchmark to gauge whether their orders achieved favorable liquidity relative to market volume.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{VWAP} = \frac{\sum_{i=1}^t P_{\text{typical}, i} \cdot V_i}{\sum_{i=1}^t V_i}, \quad P_{\text{typical}} = \frac{H + L + C}{3}`,
        caption: 'Ratio of cumulative dollar volume to total shares traded.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'VWAP has no lookback period parameter; it resets at the session boundary or accumulates continuously over the dataset window. Buying below VWAP represents receiving liquidity at an above-average discount; trading above VWAP indicates institutional demand.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'VWAP becomes heavier and less responsive later in the trading session as cumulative volume accumulates. Early session spikes carry high volatility and should be interpreted with caution.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Calculated in single-pass linear time on bar datasets using verified 64-bit precision.',
      },
    ],
    related: ['indicators/typical', 'indicators/obv'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 12. TYPICAL
  {
    slug: 'typical',
    category: 'indicators',
    title: 'Typical Price',
    level: 'Beginner',
    summary: 'Single composite price representing the arithmetic average of High, Low, and Close.',
    keywords: ['typical', 'typical price', 'composite price'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Typical Price simplifies the bar by providing an unbiased composite price that weighs the bar high, low, and close equally. It is widely used as the underlying price input for indicators such as VWAP, Commodity Channel Index (CCI), and Money Flow Index (MFI).',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`P_{\text{typical}} = \frac{\text{High} + \text{Low} + \text{Close}}{3}`,
        caption: 'Tri-point arithmetic mean of candle boundaries.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Has no parameters. By accounting for the extremes of the bar, Typical Price avoids the single-point distortion of an anomalous close at the very tick of the market bell.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Typical Price is not a standalone trading signal; it is an intermediate valuation point intended for composite indicator pipelines.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Computed in O(1) time per bar without any warm-up delay.',
      },
    ],
    related: ['indicators/median', 'indicators/vwap', 'indicators/cci'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 13. MEDIAN
  {
    slug: 'median',
    category: 'indicators',
    title: 'Median Price',
    level: 'Beginner',
    summary: 'Midpoint between the High and Low of each bar.',
    keywords: ['median price', 'bar midpoint'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Median Price computes the exact midpoint between the highest and lowest trade prices of a bar. It strips away the close completely, focusing solely on the boundaries established by buyers and sellers during the session.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`P_{\text{median}} = \frac{\text{High} + \text{Low}}{2}`,
        caption: 'Arithmetic average of bar high and low extremes.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Has no parameters. Used in Bill Williams’ Alligator and Awesome Oscillator formulations as a balanced center-point reference.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Omitting the close means Median Price cannot convey directional conviction; an extreme hammer and inverted hammer with identical high and low will yield the exact same Median Price.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Evaluates instantaneously with zero lag or warm-up requirements.',
      },
    ],
    related: ['indicators/typical', 'indicators/donchian'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 14. RSI
  {
    slug: 'rsi',
    category: 'indicators',
    title: 'Relative Strength Index (RSI)',
    level: 'Beginner',
    summary: 'Bounded momentum oscillator measuring the speed and velocity of directional price movements.',
    keywords: ['relative strength index', 'rsi indicator', 'wilders rsi'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Relative Strength Index (RSI), developed by J. Welles Wilder, is a bounded momentum oscillator operating on a scale from 0 to 100. It measures the velocity and magnitude of recent price gains relative to recent losses, identifying momentum saturation and trend divergence.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{RSI} = 100 - \frac{100}{1 + \text{RS}}, \quad \text{RS} = \frac{\text{WilderEMA}(\text{Gains}, n)}{\text{WilderEMA}(\text{Losses}, n)}`,
        caption: 'Normalized ratio of smoothed average gains to average losses.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard default is **period = 14**. Readings above 70 signify overbought conditions, while readings below 30 indicate oversold territory. In strong bull markets, RSI typically oscillates between 40 and 80; in bear markets, between 20 and 60.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'RSI can remain pinned in overbought territory during powerful sustained momentum rallies. Shorting solely because RSI crosses above 70 is one of the most common and costly retail trading mistakes.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Follows Wilder’s exact smoothing formulation. The first n bars evaluate to null warm-up positions.',
      },
    ],
    related: ['strategies/rsi-strategies', 'indicators/stoch', 'indicators/macd'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 15. MACD
  {
    slug: 'macd',
    category: 'indicators',
    title: 'Moving Average Convergence Divergence (MACD)',
    level: 'Intermediate',
    summary: 'Trend-following momentum indicator displaying the relationship between two exponential moving averages.',
    keywords: ['macd', 'moving average convergence divergence', 'macd histogram'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Moving Average Convergence Divergence (MACD), developed by Gerald Appel, translates moving average crossovers into an unbounded momentum oscillator. It reveals changes in trend strength, direction, momentum, and duration by subtracting a slow 26-period EMA from a fast 12-period EMA.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{MACD} = \text{EMA}_{12} - \text{EMA}_{26}, \quad \text{Signal} = \text{EMA}_9(\text{MACD}), \quad \text{Hist} = \text{MACD} - \text{Signal}`,
        caption: 'Fast minus slow EMA with a 9-period signal smoothing line.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard parameters are **fast = 12**, **slow = 26**, and **signal = 9**. A bullish crossover occurs when the MACD line crosses above the signal line (histogram turns positive). Divergence between MACD peaks and price peaks often foreshadows significant reversals.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because MACD is unbounded, absolute values are not comparable across different stock prices. Furthermore, histogram zero-line crossovers lag actual momentum inflection peaks.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'The API returns three distinct series: macd, signal, and histogram, rendered with green/red directional bar coloring in a dedicated sub-pane.',
      },
    ],
    related: ['indicators/ema', 'indicators/rsi', 'strategies/momentum'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 16. STOCH
  {
    slug: 'stoch',
    category: 'indicators',
    title: 'Stochastic Oscillator (STOCH)',
    level: 'Intermediate',
    summary: 'Momentum indicator comparing a closing price to its high-low range over a given time period.',
    keywords: ['stoch', 'stochastic oscillator', 'fast stochastic'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Stochastic Oscillator, developed by George Lane, tracks the location of the close relative to the high-low range over N periods. The premise is that in upward-trending markets, prices tend to close near the upper extreme of their range, whereas in downtrends, closes cluster near the low.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\%K = 100 \cdot \frac{C - \min(L_n)}{\max(H_n) - \min(L_n)}, \quad \%D = \text{SMA}_d(\%K)`,
        caption: 'Normalized position of close within rolling high-low bounds.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Standard settings are **k_period = 14**, **d_period = 3**, and **slowing = 3**. Values above 80 represent overbought conditions; values below 20 indicate oversold status. A %K line crossing above %D while below 20 is a classic bullish entry signal.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'In volatile markets, Stochastics can produce frequent premature turns. It is most effective when trading within verified support/resistance ranges rather than chasing runaway trends.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Outputs two aligned series: k and d, bounded strictly between 0 and 100 with standard 80/20 threshold lines.',
      },
    ],
    related: ['indicators/rsi', 'indicators/willr', 'indicators/donchian'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 17. CCI
  {
    slug: 'cci',
    category: 'indicators',
    title: 'Commodity Channel Index (CCI)',
    level: 'Intermediate',
    summary: 'Versatile cyclical oscillator measuring price deviation from its statistical mean.',
    keywords: ['cci', 'commodity channel index'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Commodity Channel Index (CCI), developed by Donald Lambert, assesses price variation from its statistical average. Although originally created for commodities, it is extensively used across equities and crypto to identify cyclical turns and emergent new trends.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{CCI} = \frac{P_{\text{typical}} - \text{SMA}_n(P_{\text{typical}})}{0.015 \cdot \text{MeanDeviation}}, \quad \text{MD} = \frac{1}{n} \sum |P_{\text{typical}} - \bar{P}|`,
        caption: 'Difference from mean normalized by mean absolute deviation.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The default parameter is **period = 20**. Lambert designed the 0.015 scaling constant so approximately 70-80% of values fall between -100 and +100. Surging past +100 signifies strong breakout momentum; dropping below -100 indicates heavy selling.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because CCI is unbounded, values can occasionally spike to +300 or -300 during regime shifts. Assuming +100 is an immediate reversal point often leads to shorting an accelerating trend.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Evaluates in a dedicated sub-pane with standard reference lines at +100 and -100.',
      },
    ],
    related: ['indicators/typical', 'indicators/rsi'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 18. WILLR
  {
    slug: 'willr',
    category: 'indicators',
    title: 'Williams %R',
    level: 'Intermediate',
    summary: 'Negative-bounded momentum indicator reflecting the level of the close relative to the high-low period.',
    keywords: ['willr', 'williams %r', 'williams percent r'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Williams %R, developed by legendary trader Larry Williams, is a negative-scale momentum indicator that measures where the market closed relative to the highest high over the lookback window. It is the inverted mirror of the Fast Stochastic Oscillator.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\%R = -100 \cdot \frac{\max(H_n) - C}{\max(H_n) - \min(L_n)}`,
        caption: 'Distance from period high normalized to a scale of -100 to 0.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The default parameter is **period = 14**. Readings between 0 and -20 indicate overbought conditions; readings between -80 and -100 indicate oversold conditions. A failure to reach above -20 during a rally indicates waning buyer conviction.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because the scale is inverted (0 is the top and -100 is the bottom), beginners frequently mix up bullish and bearish thresholds. An indicator pinned near 0 demonstrates persistent strength, not an automatic shorting opportunity.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Renders in a dedicated sub-pane bounded between -100 and 0 with threshold lines at -20 and -80.',
      },
    ],
    related: ['indicators/stoch', 'indicators/rsi'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 19. MOM
  {
    slug: 'mom',
    category: 'indicators',
    title: 'Momentum (MOM)',
    level: 'Beginner',
    summary: 'Direct velocity indicator measuring the absolute price change between the current bar and N bars ago.',
    keywords: ['mom', 'momentum indicator', 'price momentum indicator'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Momentum indicator measures the pure velocity of directional price moves by calculating the absolute difference between the current closing price and the closing price N periods in the past. It shows whether price progress is accelerating or decelerating.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{MOM}_n(t) = P_t - P_{t-n}`,
        caption: 'Absolute difference between current close and close n bars prior.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default lookback is **period = 10**. Positive readings signify that price is higher than N bars ago; negative readings indicate price is lower. When momentum crosses through the zero line, a directional regime transition is confirmed.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Because MOM is an absolute dollar difference, its magnitude scales directly with the nominal share price. Comparing MOM across different instruments without normalization leads to misleading conclusions.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Enforces a strict zero-baseline reference in its sub-pane display.',
      },
    ],
    related: ['indicators/roc', 'strategies/momentum'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 20. ROC
  {
    slug: 'roc',
    category: 'indicators',
    title: 'Rate of Change (ROC)',
    level: 'Beginner',
    summary: 'Percentage price momentum oscillator measuring percentage return over an N-bar window.',
    keywords: ['roc', 'rate of change indicator', 'percentage rate of change'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Rate of Change (ROC) is the percentage-normalized companion to the Momentum indicator. By dividing the price change by the past price, ROC creates a dimensionless percentage return metric that allows consistent cross-asset comparison.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{ROC}_n(t) = 100 \cdot \frac{P_t - P_{t-n}}{P_{t-n}}`,
        caption: 'Percentage change over n bars.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default setting is **period = 10**. A rising ROC above zero confirms accelerating upside momentum; dropping below zero indicates accelerating downside pressure. Extreme positive or negative readings indicate potential exhaustion peaks.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'A sharp one-day price spike will cause ROC to elevate for exactly N days, then abruptly drop when that spike rolls out of the calculation window, even if current prices remain elevated.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Renders in a dedicated sub-pane with a highlighted 0.0% equilibrium line.',
      },
    ],
    related: ['indicators/mom', 'strategies/momentum'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 21. ATR
  {
    slug: 'atr',
    category: 'indicators',
    title: 'Average True Range (ATR)',
    level: 'Intermediate',
    summary: 'Volatility indicator measuring the true trading range including inter-bar price gaps.',
    keywords: ['atr', 'average true range', 'true range indicator'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Average True Range (ATR), developed by J. Welles Wilder, is the definitive measure of market volatility. Unlike standard price range (High - Low), True Range accounts for overnight gaps by comparing the current high and low to the prior bar’s close.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{TR} = \max(H - L, |H - C_{prev}|, |L - C_{prev}|), \quad \text{ATR}_n = \text{WilderEMA}(\text{TR}, n)`,
        caption: 'Maximum of high-low and gap distances, smoothed with Wilder’s method.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'The standard setting is **period = 14**. ATR is strictly positive and non-directional; it does not indicate whether price will move up or down, but rather how much price is expected to move. It is the premier tool for volatility-adjusted position sizing and trailing stops.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'ATR expands during sharp market drops because panic generates wider candle ranges. Do not interpret rising ATR as a bullish expansion; it simply signals elevated market turbulence.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Calculated strictly in points/dollars. Sub-pane auto-scales with a zero baseline to ensure truthful visual volatility representation.',
      },
    ],
    related: ['indicators/keltner', 'indicators/psar'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 22. ADX
  {
    slug: 'adx',
    category: 'indicators',
    title: 'Average Directional Index (ADX)',
    level: 'Advanced',
    summary: 'Trend strength indicator quantifying whether a market is trending, irrespective of direction.',
    keywords: ['adx', 'average directional index', 'directional movement index'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Average Directional Index (ADX), designed by J. Welles Wilder, quantifies the strength of a trend regardless of whether it is an uptrend or a downtrend. It is calculated by smoothing the Directional Movement System (+DI and -DI).',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{DX} = 100 \cdot \frac{|{+\text{DI}} - {-\text{DI}}|}{{+\text{DI}} + {-\text{DI}}}, \quad \text{ADX} = \text{WilderEMA}(\text{DX}, n)`,
        caption: 'Normalized directional divergence smoothed by Wilder’s EMA.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default parameter is **period = 14**. ADX values above 25 indicate a strong, tradeable trend; values below 20 indicate trendless consolidation where mean-reversion strategies are preferred. A bullish signal occurs when +DI crosses above -DI while ADX is rising.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'ADX is non-directional. A surging ADX in a collapsing market does not mean an uptrend is beginning; it indicates the downward crash is extremely forceful.',
      },
      {
        type: 'callout',
        tone: 'tip',
        title: 'Spectra Engine Implementation',
        text: 'Returns three aligned series: adx, plus_di, and minus_di, rendered with standard green/red and bold trend line styling.',
      },
    ],
    related: ['indicators/atr', 'strategies/trend-following'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 23. OBV
  {
    slug: 'obv',
    category: 'indicators',
    title: 'On-Balance Volume (OBV)',
    level: 'Intermediate',
    summary: 'Cumulative volume indicator measuring institutional accumulation and distribution pressure.',
    keywords: ['obv', 'on balance volume', 'on-balance volume'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'On-Balance Volume (OBV), created by Joe Granville, relates volume flow to price changes. It posits that smart money volume precedes price moves, accumulating shares during quiet consolidations and distributing them near euphoric tops.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{OBV}_t = \text{OBV}_{t-1} + \begin{cases} V_t & \text{if } C_t > C_{t-1} \\ 0 & \text{if } C_t = C_{t-1} \\ -V_t & \text{if } C_t < C_{t-1} \end{cases}`,
        caption: 'Cumulative addition or subtraction of volume depending on close direction.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Has no lookback parameters; it accumulates continuously from the beginning of the series. The absolute numeric value is meaningless; what matters is the slope and direction of the OBV line. When OBV breaks out to new highs ahead of price, it provides strong bullish confirmation.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'A massive outlier volume day (such as index rebalancing or triple witching) can skew the OBV line for weeks. Traders should look for persistent multi-week slopes rather than single-session jumps.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Maintains verified 64-bit integer cumulative volume precision to eliminate numerical overflow.',
      },
    ],
    related: ['indicators/vwap', 'indicators/cmf', 'indicators/mfi'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 24. CMF
  {
    slug: 'cmf',
    category: 'indicators',
    title: 'Chaikin Money Flow (CMF)',
    level: 'Intermediate',
    summary: 'Volume-weighted momentum indicator measuring accumulation and distribution over a fixed window.',
    keywords: ['cmf', 'chaikin money flow'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'Chaikin Money Flow (CMF), developed by Marc Chaikin, calculates the accumulation-distribution volume over a rolling window. It evaluates where the close occurred within the high-low range and weights that position by the bar’s trading volume.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{MFV} = \frac{(C - L) - (H - C)}{H - L} \cdot V, \quad \text{CMF}_n = \frac{\sum_{i=0}^{n-1} \text{MFV}_{t-i}}{\sum_{i=0}^{n-1} V_{t-i}}`,
        caption: 'Rolling sum of Money Flow Volume divided by rolling total volume.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default parameter is **period = 20**. Values range between -1 and +1. CMF readings above +0.05 indicate institutional buying accumulation; readings below -0.05 signify active distribution.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'If a stock gaps up significantly but trades lower during the day to close near its low, CMF will register negative money flow for that bar despite the positive net gain. Comparing CMF alongside OBV helps reconcile opening gap dynamics.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Renders in a dedicated sub-pane with a centered 0.0 neutral reference line.',
      },
    ],
    related: ['indicators/mfi', 'indicators/obv'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },

  // 25. MFI
  {
    slug: 'mfi',
    category: 'indicators',
    title: 'Money Flow Index (MFI)',
    level: 'Intermediate',
    summary: 'Volume-weighted Relative Strength Index measuring buying and selling pressure.',
    keywords: ['mfi', 'money flow index', 'volume weighted rsi'],
    blocks: [
      { type: 'h', text: 'What it measures' },
      {
        type: 'p',
        text: 'The Money Flow Index (MFI) is essentially a volume-weighted RSI. By incorporating trading volume into the typical price momentum equation, MFI assesses the enthusiasm and conviction behind price movements, identifying overbought and oversold conditions with greater fidelity.',
      },
      { type: 'h', text: 'Mathematical formula' },
      {
        type: 'formula',
        tex: String.raw`\text{MFI} = 100 - \frac{100}{1 + \text{MFR}}, \quad \text{MFR} = \frac{\sum \text{Positive Money Flow}}{\sum \text{Negative Money Flow}}`,
        caption: 'Ratio of positive typical price volume flow to negative volume flow.',
      },
      { type: 'h', text: 'Default parameters and interpretation' },
      {
        type: 'p',
        text: 'Default setting is **period = 14**. Values above 80 indicate overbought conditions; values below 20 indicate oversold territory. Bullish divergences (where price makes lower lows but MFI makes higher lows) are respected trend reversal indicators.',
      },
      { type: 'h', text: 'Common misreading and pitfalls' },
      {
        type: 'p',
        text: 'Like RSI, MFI can linger at extremes during powerful speculative runs. Always look for divergence or a confirmed cross back inside the 80/20 bands before expecting a mean-reversion move.',
      },
      {
        type: 'callout',
        tone: 'info',
        title: 'Spectra Engine Implementation',
        text: 'Outputs values strictly bounded between 0 and 100 with standard 80/20 oscillator reference bands.',
      },
    ],
    related: ['indicators/rsi', 'indicators/cmf', 'indicators/typical'],
    platformLinks: [{ label: 'Paper Lab Chart', href: '/studio?tab=paper' }],
  },
];
