import { metricLesson } from '../build';

const BT = [{ label: 'Open Backtests', href: '/studio?tab=backtests' }];
const RISK = [{ label: 'Open Risk & Stress Lab', href: '/studio?tab=risk' }];

export const riskMetrics = [
  metricLesson({
    slug: 'sharpe-ratio',
    title: 'Sharpe Ratio',
    level: 'Intermediate',
    featured: true,
    summary: 'Risk-adjusted return: the average excess return per unit of volatility, annualized.',
    keywords: ['sharpe', 'risk adjusted', 'reward to risk', 'sr'],
    what: [
      'The Sharpe ratio divides the strategy’s average return above the risk-free rate by the standard deviation of its returns. It asks how much extra return you earned for each unit of variability you accepted.',
      'Spectra computes it on the period returns of the equity curve and annualizes it with the run’s periods-per-year setting.',
    ],
    why: [
      'Raw return rewards taking more risk. Sharpe lets you compare a quiet strategy and a volatile one on the same scale, and it is the basis of the Probabilistic and Deflated Sharpe ratios used to judge whether a backtest result is likely to be real.',
    ],
    formulas: [{ tex: String.raw`\text{Sharpe} = \frac{\overline{r} - r_f/\text{ppy}}{\sigma_r}\sqrt{\text{ppy}}` }],
    where: [
      '\\(\\overline{r}\\) is the mean of the period returns \\(r_t = E_t/E_{t-1} - 1\\).',
      '\\(\\sigma_r\\) is their sample standard deviation (divides by \\(n-1\\)).',
      '\\(r_f\\) is the annual risk-free rate from the run configuration (0 unless you set it).',
      '\\(\\text{ppy}\\) is periods per year (252 for daily bars by default).',
    ],
    engine:
      'Exactly the formula above (`spectra_engine::metrics`). The ratio is 0 when there are fewer than two returns or the returns never vary. It uses simple, not log, returns.',
    example: {
      intro: 'A daily strategy averages +0.05% a day with a daily standard deviation of 1.0%. The risk-free rate is 4% a year and ppy is 252.',
      steps: [
        'Daily risk-free rate: 0.04 / 252 = 0.0159%.',
        'Daily excess return: 0.0500% − 0.0159% = 0.0341%.',
        'Per-period Sharpe: 0.000341 / 0.010 = 0.0341.',
        'Annualize: 0.0341 × √252 = 0.0341 × 15.87 = 0.54.',
      ],
      result: 'The Sharpe ratio is **0.54**. With a risk-free rate of 0 it would be 0.79.',
    },
    high: 'More return per unit of volatility. Above about 1 is good for a single strategy after costs; above 2 or 3 on a backtest is unusual and worth checking for overfitting or look-ahead.',
    low: 'Little reward for the risk taken. A negative Sharpe means the strategy earned less than the risk-free rate.',
    misconceptions: [
      'A high Sharpe ratio does not guarantee future returns; it describes one sample of history.',
      'It treats upside and downside volatility the same. A strategy with rare large losses can have a fine Sharpe until the loss arrives; check the Sortino ratio, skewness and drawdown too.',
      'Sharpe ratios computed on different bar frequencies or with a wrong periods-per-year setting are not comparable.',
      'Trying many parameter sets and keeping the best Sharpe inflates it. The Deflated Sharpe Ratio corrects for that.',
    ],
    related: ['sortino-ratio', 'volatility', 'probabilistic-sharpe-ratio', 'deflated-sharpe-ratio', 'calmar-ratio'],
    platformLinks: [...BT, { label: 'Open Optimization Lab', href: '/studio?tab=optimization' }],
    interactive: 'equity-metrics',
  }),

  metricLesson({
    slug: 'sortino-ratio',
    title: 'Sortino Ratio',
    level: 'Intermediate',
    summary: 'Like the Sharpe ratio, but it only counts returns below the target as risk.',
    keywords: ['sortino', 'downside deviation', 'downside risk'],
    what: [
      'The Sortino ratio divides the average excess return by the **downside deviation**: how far, on average, returns fall below the target. Up-moves do not count against the strategy.',
    ],
    why: [
      'Investors mind losses more than gains. A strategy with occasional big up-days is penalized by Sharpe but not by Sortino, so comparing the two tells you whether volatility is mostly on the upside or the downside.',
    ],
    formulas: [
      { tex: String.raw`\text{Sortino} = \frac{\overline{r} - \tau}{\text{DD}_\tau}\sqrt{\text{ppy}}, \quad \tau = r_f/\text{ppy}` },
      { tex: String.raw`\text{DD}_\tau = \sqrt{\frac{1}{n}\sum_{t=1}^{n} \min(r_t - \tau,\ 0)^2}` },
    ],
    where: ['The downside deviation divides by **all** \\(n\\) returns, not only the negative ones.', '\\(\\tau\\) is the per-period risk-free rate, used as the minimum acceptable return.'],
    engine:
      'Computed in `spectra_engine::performance::sortino` with the run’s risk-free rate. It is shown as “—” when no return falls below the target (the downside deviation is zero and the ratio is undefined).',
    example: {
      intro: 'Daily returns average +0.05%, the risk-free rate is 0, and the downside deviation of the daily returns is 0.6%.',
      steps: ['Per-period ratio: 0.0005 / 0.006 = 0.0833.', 'Annualize: 0.0833 × √252 = 1.32.'],
      result: 'The Sortino ratio is **1.32**. If the same strategy’s Sharpe were 0.79, most of its volatility would be on the upside.',
    },
    high: 'Good return relative to downside moves.',
    low: 'Downside moves are large relative to the average return.',
    misconceptions: [
      'Sortino and Sharpe are not on directly comparable scales: Sortino is usually higher for the same strategy.',
      'An undefined Sortino on a short run (no losing periods) is not “infinitely good”; there is just too little data.',
    ],
    related: ['sharpe-ratio', 'volatility', 'skewness-and-kurtosis', 'maximum-drawdown'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'calmar-ratio',
    title: 'Calmar Ratio',
    level: 'Intermediate',
    summary: 'Compound annual growth divided by the maximum drawdown: return per unit of worst-case pain.',
    keywords: ['calmar', 'return over drawdown', 'mar ratio'],
    what: ['The Calmar ratio compares how fast the strategy compounds (CAGR) with the deepest peak-to-trough fall it suffered (maximum drawdown).'],
    why: [
      'Drawdowns are what make people abandon strategies. Calmar tells you how many years of typical growth one worst drawdown represents: a Calmar of 0.5 means the worst fall was two years of growth.',
    ],
    formulas: [{ tex: String.raw`\text{Calmar} = \frac{\text{CAGR}}{\text{Max drawdown}}` }],
    engine:
      'Computed from the period returns (`performance::calmar`): CAGR over the deepest drawdown of the compounded return path. It is “—” when there was no drawdown at all.',
    example: {
      intro: 'A strategy compounds at 12% a year and its worst drawdown was 20%.',
      steps: ['Divide 0.12 by 0.20.'],
      result: 'The Calmar ratio is **0.6**.',
    },
    high: 'Growth is large compared with the worst fall. Above 1 is strong for a long backtest.',
    low: 'The worst drawdown is large compared with the growth; the strategy may be hard to hold through bad periods.',
    misconceptions: [
      'Maximum drawdown grows with the length of the history, so Calmar ratios from runs of different lengths are not comparable.',
      'A high Calmar on a short run may simply mean the bad period has not happened yet.',
    ],
    related: ['cagr', 'maximum-drawdown', 'sharpe-ratio', 'ulcer-index'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'omega-ratio',
    title: 'Omega Ratio',
    level: 'Advanced',
    summary: 'The sum of gains above a threshold divided by the sum of losses below it, using the whole return distribution.',
    keywords: ['omega', 'gain loss ratio', 'threshold'],
    what: [
      'Omega splits every period return at a threshold (0 in Spectra) and compares the total size of the gains with the total size of the losses. It uses every return, not just the mean and standard deviation.',
    ],
    why: ['Because it does not assume returns are normally distributed, Omega captures skew and fat tails that Sharpe ignores.'],
    formulas: [{ tex: String.raw`\Omega(\theta) = \frac{\sum_t \max(r_t - \theta,\ 0)}{\sum_t \max(\theta - r_t,\ 0)}, \quad \theta = 0` }],
    engine: 'Computed by `performance::omega` at a zero threshold. It is “—” when no period return was below zero.',
    example: {
      intro: 'Over a run, the positive daily returns add up to 18.4% and the negative ones to −11.5%.',
      steps: ['Gains above 0: 18.4%.', 'Losses below 0: 11.5%.', 'Divide: 18.4 / 11.5.'],
      result: 'Omega is **1.6**: for every $1 lost in down periods, $1.60 was gained in up periods.',
    },
    high: 'Above 1 means gains outweigh losses at the threshold.',
    low: 'Below 1 means total losses exceed total gains: the strategy lost money on a simple-sum basis.',
    misconceptions: ['Omega at 0 is not the same as profit factor: Omega sums period returns of the equity curve, profit factor sums closed-trade P&L.'],
    related: ['profit-factor', 'sharpe-ratio', 'skewness-and-kurtosis'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'volatility',
    title: 'Annualized Volatility',
    level: 'Beginner',
    featured: true,
    summary: 'How much returns vary from period to period, expressed as an annual standard deviation.',
    keywords: ['standard deviation', 'annualized volatility', 'risk', 'sigma', 'vol'],
    what: [
      'Volatility measures the spread of returns around their average. In a backtest it is the standard deviation of the equity curve’s period returns, scaled to a year. In the Risk & Stress Lab it is the model volatility used for the Monte Carlo simulation.',
    ],
    why: [
      'It is the most common measure of risk and the denominator of the Sharpe ratio. Higher volatility means a wider range of outcomes over any horizon and larger swings in account value.',
    ],
    formulas: [{ tex: String.raw`\sigma_{\text{ann}} = \sqrt{\frac{1}{n-1}\sum_{t=1}^{n}(r_t - \overline{r})^2}\ \times \sqrt{\text{ppy}}` }],
    where: ['Volatility scales with the square root of time, so daily volatility × √252 gives annual volatility.'],
    engine:
      'Backtests: sample standard deviation (n − 1) of the period returns times √ppy (`annualized_volatility`). It is 0 with fewer than two returns.',
    example: {
      intro: 'Daily returns have a standard deviation of 1.0% and ppy is 252.',
      steps: ['√252 = 15.87.', '1.0% × 15.87 = 15.87%.'],
      result: 'Annualized volatility is about **15.9%**, close to that of a broad equity index.',
    },
    high: 'Larger swings; returns are less predictable over short horizons. Not necessarily bad if the return is high enough.',
    low: 'Smoother returns. Very low volatility combined with steady gains can hide rare large losses (selling options, for example).',
    misconceptions: [
      'Volatility is not the same as loss: it counts up-moves and down-moves alike.',
      'Annualizing assumes returns are independent from one period to the next. Strategies with trending or mean-reverting returns break that assumption.',
    ],
    related: ['sharpe-ratio', 'sortino-ratio', 'value-at-risk', 'fundamentals/volatility'],
    platformLinks: [...BT, ...RISK],
    interactive: 'equity-metrics',
  }),

  metricLesson({
    slug: 'maximum-drawdown',
    title: 'Maximum Drawdown',
    level: 'Beginner',
    featured: true,
    summary: 'The largest peak-to-trough decline in portfolio value during the run.',
    keywords: ['max dd', 'drawdown', 'peak to trough', 'mdd'],
    what: [
      'At every point of the equity curve, the drawdown is how far equity sits below the highest value reached so far. The maximum drawdown is the deepest of those falls, as a positive fraction of the peak.',
    ],
    why: [
      'It is the clearest measure of how painful a strategy can be. A 50% drawdown needs a 100% gain to recover. Many traders set a maximum acceptable drawdown and reject strategies that exceed it, and the Risk Center can enforce a drawdown limit on paper trading.',
    ],
    diagram: { id: 'drawdown', caption: 'The drawdown is the gap between the running peak and current equity.' },
    formulas: [{ tex: String.raw`\text{MDD} = \max_t \frac{P_t - E_t}{P_t}, \quad P_t = \max_{s \le t} E_s` }],
    engine:
      'Computed on every point of the equity curve (`max_drawdown`, reported as a positive fraction). The drawdown chart is calculated the same way, so its lowest point equals the reported figure exactly.',
    example: {
      intro: 'Equity rises from $100,000 to a peak of $130,000, falls to $97,500, then recovers to $140,000.',
      steps: ['Peak before the fall: $130,000.', 'Trough: $97,500.', 'Fall: ($130,000 − $97,500) / $130,000 = 0.25.'],
      result: 'Maximum drawdown is **25%**, even though the run ended at a new high.',
    },
    high: 'A deeper worst fall: more capital at risk and a longer road back.',
    low: 'Shallower falls. A very low drawdown on a short run may just mean a bad period has not happened yet.',
    misconceptions: [
      'It is not the worst single-day loss; it can build over months of small losses.',
      'It is measured from a peak, not from the starting value: a run can be up overall and still have a large drawdown.',
      'Past maximum drawdown is a floor, not a ceiling, for future drawdowns.',
    ],
    related: ['drawdown-duration', 'calmar-ratio', 'ulcer-index', 'equity-curve', 'engine/risk-management'],
    platformLinks: [...BT, { label: 'Open Risk Center', href: '/studio?tab=risk-center' }],
    interactive: 'equity-metrics',
  }),

  metricLesson({
    slug: 'drawdown-duration',
    title: 'Current Drawdown, Underwater Time & Episodes',
    level: 'Intermediate',
    summary: 'How far below its peak the account ends, how long it stayed below a peak, and how many separate drawdowns it had.',
    keywords: ['current drawdown', 'longest underwater', 'drawdown episodes', 'recovery time', 'average drawdown'],
    what: [
      '**Current drawdown** is how far the final equity sits below the highest equity reached. **Longest underwater** is the longest run of consecutive periods spent below a previous peak. **Drawdown episodes** counts the separate stretches below a peak.',
    ],
    why: [
      'Depth is only half of the story. A 15% drawdown that recovers in two weeks feels very different from one that lasts two years. Long underwater periods are where strategies get abandoned.',
    ],
    formulas: [{ tex: String.raw`\text{DD}_t = \frac{P_t - E_t}{P_t}, \qquad \text{current} = \text{DD}_n` }],
    where: ['Underwater periods are periods where \\(E_t < P_t\\); the longest consecutive run of them is reported in periods (bars).'],
    engine:
      'Current drawdown and longest underwater come from `compute_drawdown_stats` on the equity curve. Episode counts and average episode depth come from `performance::drawdown_episodes` on the compounded returns: each separate stretch below the running peak is one episode, measured by its deepest point.',
    example: {
      intro: 'Equity: 100, 120, 90, 95, 130, 117.',
      steps: [
        'Running peak: 100, 120, 120, 120, 130, 130.',
        'Below the peak at 90 and 95 (two periods), then again at 117.',
        'Current drawdown: (130 − 117) / 130 = 10%.',
      ],
      result: 'Current drawdown **10%**, longest underwater **2 periods**, **2** drawdown episodes (the second still open).',
    },
    high: 'Long or frequent drawdowns: the strategy spends a lot of time below its best level.',
    low: 'Quick recoveries; the strategy rarely stays below its high-water mark.',
    misconceptions: ['Underwater time is counted in bars, so a value from hourly bars is not comparable with one from daily bars without converting.'],
    related: ['maximum-drawdown', 'ulcer-index', 'equity-curve'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'ulcer-index',
    title: 'Ulcer Index',
    level: 'Advanced',
    summary: 'The root-mean-square of drawdowns: it rises with both the depth and the duration of time spent below a peak.',
    keywords: ['ulcer', 'drawdown risk', 'rms drawdown'],
    what: ['The Ulcer index squares the drawdown at every point, averages the squares and takes the square root. Deep drawdowns weigh heavily, and long ones count at every bar.'],
    why: ['Maximum drawdown looks at one point. The Ulcer index summarizes the whole drawdown experience in one number.'],
    formulas: [{ tex: String.raw`\text{UI} = \sqrt{\frac{1}{N}\sum_{t=1}^{N} \text{DD}_t^2}` }],
    where: ['\\(N\\) is the number of equity points and \\(\\text{DD}_t\\) the drawdown fraction at each.'],
    engine: 'Computed by `compute_drawdown_stats` over every equity point, with drawdowns as fractions (shown to four decimals).',
    example: {
      intro: 'Four equity points with drawdowns 0, 0.10, 0.05 and 0.',
      steps: ['Squares: 0, 0.01, 0.0025, 0.', 'Mean: 0.0125 / 4 = 0.003125.', 'Square root: 0.0559.'],
      result: 'The Ulcer index is **0.0559** (about 5.6%).',
    },
    high: 'Deep and/or long drawdowns.',
    low: 'Shallow, short drawdowns.',
    misconceptions: ['It is not a percentage loss you can expect; it is an average-like measure of how far below the peak you typically are.'],
    related: ['maximum-drawdown', 'drawdown-duration', 'calmar-ratio'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'value-at-risk',
    title: 'Value at Risk (VaR)',
    level: 'Intermediate',
    featured: true,
    summary: 'A loss threshold: at a confidence level such as 95%, the loss that is exceeded only in the worst 5% of cases.',
    keywords: ['var', 'value at risk', 'tail risk', 'monte carlo var', 'historical var', 'confidence level'],
    what: [
      'VaR answers “how bad is a bad day (or week)?” A 95% one-day VaR of $20,000 means that on 95% of days the loss should be smaller than $20,000, and on the remaining 5% it is larger.',
      'Spectra reports VaR in two places with two methods. **Backtests** report a *historical* VaR of the strategy’s period returns. The **Risk & Stress Lab** reports a *Monte Carlo* VaR of the current portfolio over a chosen horizon.',
    ],
    why: [
      'It turns a distribution of outcomes into one number that can be compared with capital and limits. The Risk Center can enforce a maximum VaR as a percentage of equity.',
    ],
    formulas: [
      { tex: String.raw`\text{VaR}^{\text{hist}}_{95\%} = -\,r_{(k)}, \quad k = \lceil 0.05\,n \rceil`, caption: 'Backtests: the k-th smallest period return (nearest rank)' },
      { tex: String.raw`\text{VaR}^{\text{MC}}_{c} = Q_c(L_1, \dots, L_N)`, caption: 'Risk & Stress Lab: the c-quantile of N simulated losses, interpolated between order statistics' },
    ],
    where: ['\\(r_{(k)}\\) is the \\(k\\)-th smallest of the \\(n\\) returns; VaR is reported as a positive loss fraction.', '\\(L_i\\) is the simulated loss of path \\(i\\) over the horizon.'],
    engine:
      'Historical VaR (`compute_tail_stats`) needs at least 20 returns and never exceeds the worst loss in the sample. Monte Carlo VaR (`spectra_risk::metrics::value_at_risk`) sorts the simulated horizon losses and linearly interpolates the quantile at rank `c × (N − 1)`. The Monte Carlo model is a single aggregate position with geometric Brownian motion and zero drift, so it ignores correlations between assets; the lab shows how far the simulated value is from the exact closed-form value of the same model.',
    example: {
      intro: 'Risk & Stress Lab: a $1,000,000 portfolio with 20% annual volatility, a one-day horizon (1/252 of a year) and 95% confidence.',
      steps: [
        'One-day volatility: 20% × √(1/252) = 1.26%.',
        'The 5th-percentile normal draw is −1.645, so the 5%-worst one-day return is about e^(−0.00008 − 1.645 × 0.0126) − 1 = −2.06%.',
        'Loss at that point: 2.06% × $1,000,000.',
      ],
      result: 'The one-day 95% VaR is about **$20,600**. A 10,000-path Monte Carlo run lands within a small fraction of this.',
    },
    high: 'Larger potential losses at the chosen confidence.',
    low: 'Smaller potential losses, as far as the model or history can tell.',
    misconceptions: [
      'VaR is not the worst case. It says nothing about how big the losses beyond it are; expected shortfall does.',
      'A 99% VaR is not “safer” than a 95% one; it is a different, larger threshold for the same portfolio.',
      'VaR from a model is only as good as the model: the lab’s single-position GBM ignores correlations, gaps and fat tails (see EVT VaR).',
    ],
    related: ['expected-shortfall', 'evt-tail-risk', 'volatility', 'worst-period'],
    platformLinks: [...RISK, ...BT],
  }),

  metricLesson({
    slug: 'expected-shortfall',
    title: 'Expected Shortfall (CVaR)',
    level: 'Intermediate',
    summary: 'The average loss in the worst cases: the mean of all losses at or beyond the VaR threshold.',
    keywords: ['cvar', 'conditional value at risk', 'expected shortfall', 'es', 'tail loss'],
    what: ['Where VaR marks the edge of the tail, expected shortfall averages what happens inside it. A 95% expected shortfall is the average of the worst 5% of outcomes.'],
    why: ['Two portfolios can share a VaR while one has a much fatter tail. Expected shortfall sees the difference, which is why regulators and risk managers increasingly prefer it.'],
    formulas: [
      { tex: String.raw`\text{ES}^{\text{hist}}_{95\%} = -\frac{1}{k}\sum_{j=1}^{k} r_{(j)}`, caption: 'Backtests: mean of the k worst returns' },
      { tex: String.raw`\text{CVaR}_c = \operatorname{mean}\{L_i : L_i \ge \text{VaR}_c\}`, caption: 'Risk & Stress Lab' },
    ],
    engine:
      'Backtests average the returns up to and including the VaR rank (`expected_shortfall`). The Risk & Stress Lab averages every simulated loss at or beyond the Monte Carlo VaR (`conditional_var`).',
    example: {
      intro: 'A backtest has 100 daily returns. The five worst are −4.2%, −3.6%, −3.1%, −2.8% and −2.5%.',
      steps: ['k = ⌈0.05 × 100⌉ = 5, so VaR is the 5th worst: 2.5%.', 'Average the five: (4.2 + 3.6 + 3.1 + 2.8 + 2.5) / 5 = 3.24%.'],
      result: 'The 95% VaR is **2.5%** and the expected shortfall is **3.24%**.',
    },
    high: 'Bad periods are very bad on average.',
    low: 'The tail is thin: when losses exceed VaR they do not exceed it by much.',
    misconceptions: ['Expected shortfall is always at least as large as VaR at the same level; if a tool reports otherwise, something is wrong.'],
    related: ['value-at-risk', 'evt-tail-risk', 'worst-period', 'skewness-and-kurtosis'],
    platformLinks: [...RISK, ...BT],
  }),

  metricLesson({
    slug: 'evt-tail-risk',
    title: 'EVT VaR (Extreme Value Theory)',
    level: 'Advanced',
    summary: 'A tail estimate that fits a Generalized Pareto distribution to the largest simulated losses instead of relying on the normal shape.',
    keywords: ['evt', 'extreme value theory', 'gpd', 'peaks over threshold', 'fat tails'],
    what: [
      'Extreme Value Theory models only the extreme part of a distribution. The peaks-over-threshold method takes every loss beyond a high threshold (the 85th percentile in the Risk & Stress Lab) and fits a Generalized Pareto distribution (GPD) to the excesses. VaR and expected shortfall are then read from the fitted tail.',
    ],
    why: ['Real losses have fatter tails than the normal distribution. When the EVT estimate is well above the Monte Carlo VaR, the lab flags fat-tail risk.'],
    formulas: [{ tex: String.raw`\text{VaR}_c = u + \frac{\beta}{\xi}\left[\left(\frac{n}{N_u}(1-c)\right)^{-\xi} - 1\right]` }],
    where: ['\\(u\\) is the threshold, \\(N_u\\) the number of losses above it out of \\(n\\).', '\\(\\xi\\) (shape) and \\(\\beta\\) (scale) are the fitted GPD parameters; \\(\\xi > 0\\) means a fat tail.'],
    engine: 'The Risk & Stress Lab shows EVT VaR, EVT expected shortfall and the ratio of EVT VaR to Monte Carlo VaR when the fit succeeds.',
    example: {
      intro: 'A run reports Monte Carlo VaR $20,600 and EVT VaR $24,700.',
      steps: ['Tail ratio: 24,700 / 20,600 = 1.20.'],
      result: 'The EVT tail is **20% heavier** than the Gaussian estimate, and the lab badges the run as fat-tailed.',
    },
    high: 'A ratio well above 1 means the extreme losses are larger than the normal model suggests.',
    low: 'A ratio near or below 1 means the tail is within normal bounds.',
    misconceptions: ['EVT needs many tail observations; on few simulations or a short history the fitted shape parameter is very uncertain.'],
    related: ['value-at-risk', 'expected-shortfall', 'skewness-and-kurtosis'],
    platformLinks: RISK,
  }),

  metricLesson({
    slug: 'worst-period',
    title: 'Worst Period Return',
    level: 'Beginner',
    summary: 'The single worst bar-to-bar return of the equity curve.',
    keywords: ['worst day', 'worst return', 'largest loss'],
    what: ['The worst period is the most negative single return between two consecutive equity points: the worst day for daily bars.'],
    why: ['It shows the sharpest shock the strategy took, which matters for leverage, margin and stop-loss settings.'],
    formulas: [{ tex: String.raw`r_{\min} = \min_t \left(\frac{E_t}{E_{t-1}} - 1\right)` }],
    engine: 'Reported with the tail statistics (`worst_return`). Like VaR it needs at least 20 returns.',
    example: {
      intro: 'Equity is $104,000 one day and $99,840 the next.',
      steps: ['99,840 / 104,000 − 1 = −0.04.'],
      result: 'If no other day was worse, the worst period is **−4.0%**.',
    },
    high: 'A value near zero: no sharp single-period losses.',
    low: 'A large negative value: the strategy is exposed to sudden shocks such as gaps.',
    misconceptions: ['The worst period is not the maximum drawdown, which can build up over many periods.'],
    related: ['value-at-risk', 'maximum-drawdown', 'expected-shortfall'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'skewness-and-kurtosis',
    title: 'Skewness & Excess Kurtosis',
    level: 'Advanced',
    summary: 'Shape statistics of the return distribution: skewness measures asymmetry, excess kurtosis measures how fat the tails are.',
    keywords: ['skew', 'kurtosis', 'fat tails', 'distribution shape', 'moments'],
    what: [
      '**Skewness** is negative when the left tail (losses) is longer than the right, and positive when large gains are more common than large losses. **Excess kurtosis** is 0 for a normal distribution and positive when extreme returns are more frequent than a normal distribution predicts.',
    ],
    why: [
      'Sharpe and volatility only use the first two moments. Strategies that earn small steady gains with rare large losses (negative skew, high kurtosis) look better on those numbers than they are. The Probabilistic and Deflated Sharpe ratios use skewness and kurtosis directly.',
    ],
    formulas: [
      { tex: String.raw`\text{Skew} = \frac{m_3}{m_2^{3/2}}, \qquad \text{Excess kurtosis} = \frac{m_4}{m_2^{2}} - 3` },
      { tex: String.raw`m_k = \frac{1}{n}\sum_{t=1}^{n}(r_t - \overline{r})^k` },
    ],
    engine:
      'Population moments (divide by n) of the equity curve’s period returns (`performance::skewness`, `performance::excess_kurtosis`). Skewness needs 3 returns and kurtosis 4; both are “—” if returns never vary.',
    example: {
      intro: 'A strategy has many small positive days and a few large negative ones.',
      steps: ['The few large negative deviations dominate the cubed terms, so \\(m_3\\) is negative.', 'The same rare large moves dominate the fourth powers, so \\(m_4 / m_2^2\\) is well above 3.'],
      result: 'You would see something like skewness **−1.2** and excess kurtosis **+6**: the classic profile of a strategy that picks up pennies in front of a steamroller.',
    },
    high: 'Positive skew: occasional big wins. High excess kurtosis: extreme moves (either way) are common.',
    low: 'Negative skew: occasional big losses. Excess kurtosis near 0: tails close to normal.',
    misconceptions: ['Both statistics are very noisy on short samples; one outlier can flip the sign of skewness.'],
    related: ['sharpe-ratio', 'sortino-ratio', 'probabilistic-sharpe-ratio', 'expected-shortfall'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'leverage',
    title: 'Gross Leverage',
    level: 'Intermediate',
    summary: 'The total size of all positions, long and short, as a multiple of equity.',
    keywords: ['leverage', 'gross exposure', 'margin', 'buying power'],
    what: [
      'Gross leverage adds up the absolute market value of every position and divides it by equity. 1.0× means positions are worth as much as the account; 2.0× means they are worth twice as much, financed by borrowing or shorting.',
      'The Studio sidebar shows the paper account’s gross leverage against its limit.',
    ],
    why: ['Leverage multiplies both returns and losses. The backtest ledger refuses fills that would exceed the configured gross leverage limit (1.0× by default), and the Risk Center can enforce a leverage limit before orders reach the broker.'],
    formulas: [{ tex: String.raw`\text{Gross leverage} = \frac{\sum_i |q_i \times p_i|}{\text{Equity}}` }],
    engine:
      'In backtests the ledger checks every fill against `max_gross_leverage`; a fill that would breach it is rejected and the order is marked Rejected. Set the limit to null to disable the check.',
    example: {
      intro: 'Equity is $100,000, you hold $90,000 of stock A and are short $40,000 of stock B.',
      steps: ['Gross exposure: |90,000| + |−40,000| = $130,000.', 'Divide by equity: 130,000 / 100,000.'],
      result: 'Gross leverage is **1.3×**. With the default 1.0× limit the engine would have refused the fill that took it past 1.0×.',
    },
    high: 'More exposure per dollar of capital: bigger swings and margin risk.',
    low: 'Less exposure; part of the capital is idle in cash.',
    misconceptions: ['Net exposure can be near zero while gross leverage is high (a long/short book); hedged does not mean unlevered.'],
    related: ['exposure', 'engine/risk-management', 'paper-trading/account-balances'],
    platformLinks: [{ label: 'Open Risk Center', href: '/studio?tab=risk-center' }, ...BT],
  }),
  metricLesson({
    slug: 'concentration-hhi',
    title: 'Concentration (HHI)',
    level: 'Intermediate',
    summary: 'How much of the portfolio sits in a few positions: the Herfindahl index of its gross weights, and the effective number of holdings it implies.',
    keywords: ['hhi', 'herfindahl', 'concentration', 'diversification', 'effective n', 'position weight'],
    what: [
      'The Herfindahl–Hirschman index (HHI) squares each position’s share of the portfolio’s gross exposure and adds the squares. It is 1 when one position holds everything and falls toward 0 as the exposure spreads over more positions.',
      'The Portfolio Studio shows HHI in points (HHI × 10,000) and the effective number of holdings, 1 / HHI.',
    ],
    why: ['A concentrated portfolio can lose heavily on one bad name even when it holds many tickers. HHI shows that in one number, and the Monte Carlo risk simulation is only as informative as the positions it is given.'],
    formulas: [
      { tex: String.raw`w_i = \frac{|q_i\,p_i|}{\sum_j |q_j\,p_j|}` },
      { tex: String.raw`\text{HHI} = \sum_i w_i^2, \qquad N_{\text{eff}} = \frac{1}{\text{HHI}}` },
    ],
    where: ['Positions with zero value add nothing; the sums run over the positions that have exposure.'],
    engine:
      'Computed by `Portfolio::hhi` in `spectra-risk` from the gross weights (the Risk Lab metric `hhi`). The Portfolio Studio computes the same value in the browser and shows whether the two agree. Bands: below 1,000 points unconcentrated, 1,000 to 1,800 moderate, above 1,800 highly concentrated.',
    example: {
      intro: 'Three positions with market values $50,000, $30,000 and $20,000 (gross exposure $100,000).',
      steps: ['Weights: 0.50, 0.30 and 0.20.', 'Squares: 0.25 + 0.09 + 0.04 = 0.38.', 'Effective holdings: 1 / 0.38 = 2.63.'],
      result: 'HHI is **0.38** (3,800 points), which is highly concentrated, and the portfolio behaves like **2.6** equal holdings. Three equal positions would give 0.33, so this book is more concentrated than an equal split.',
    },
    high: 'Exposure sits in a few names, so one position dominates the risk.',
    low: 'Exposure is spread over many names. The lowest possible value for N positions is 1/N.',
    misconceptions: [
      'HHI uses gross weights, so a short position counts as much as a long one of the same size.',
      'A low HHI does not mean the positions are unrelated: ten stocks that move together are still one concentrated risk.',
    ],
    related: ['leverage', 'exposure', 'value-at-risk'],
    platformLinks: [{ label: 'Open Portfolio Studio', href: '/studio?tab=portfolio' }, { label: 'Open Risk & Stress Lab', href: '/studio?tab=risk' }],
  }),
];
