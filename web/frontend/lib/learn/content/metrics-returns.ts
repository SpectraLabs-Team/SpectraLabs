import { metricLesson } from '../build';

const BT = [{ label: 'Open Backtests', href: '/studio?tab=backtests' }];

export const returnMetrics = [
  metricLesson({
    slug: 'total-return',
    title: 'Total Return',
    level: 'Beginner',
    featured: true,
    summary: 'How much the account grew or shrank over the whole simulation, as a percentage of where it started.',
    keywords: ['return', 'performance', 'percent gain', 'growth'],
    what: [
      'Total return compares the final equity of a run with its starting equity. Equity is cash plus the market value of every open position, marked at each bar’s close, so total return includes gains that have not been realized yet.',
      'It answers the simplest question about a strategy: if you had run it over this period, how much richer or poorer would the account be?',
    ],
    why: [
      'It is the headline number every other metric qualifies. A high Sharpe ratio on a 0.5% total return still made very little money, and a large total return with a 60% drawdown may have been impossible to sit through.',
      'Because it is a single ratio of two equity values it ignores the path in between and the length of the period. Use annualized return or CAGR to compare runs of different lengths.',
    ],
    formulas: [{ tex: String.raw`\text{Total return} = \frac{E_n}{E_0} - 1` }],
    where: ['\\(E_0\\) is the first point of the equity curve (the starting cash).', '\\(E_n\\) is the last point: cash plus open positions marked at the final close, after every fee and financing charge.'],
    engine:
      'The engine reads the first and last points of the equity curve: `total_return = E_n / E_0 - 1`. Fees, slippage, borrow fees and dividends are already inside the equity values, so the return is net of costs. If the starting equity is not positive the metric is left at 0.',
    example: {
      intro: 'A run starts with $100,000 of cash and ends with $112,500 of equity.',
      steps: ['Divide final by starting equity: 112,500 / 100,000 = 1.125.', 'Subtract 1: 1.125 − 1 = 0.125.'],
      result: 'Total return is **+12.5%**. The same 12.5% earned over one year or over ten years reads identically, which is why the period length matters.',
    },
    high: 'The strategy grew the account more over this period. Check the drawdown and the number of trades before trusting it: one lucky trade can produce a large total return.',
    low: 'The strategy grew the account less, or lost money (negative values). A low return with very low risk can still be useful as part of a portfolio.',
    misconceptions: [
      'It is not an annual figure. A 40% total return over eight years is about 4.3% a year.',
      'It is not the sum of the period returns. Returns compound: +10% then −10% is −1%, not 0%.',
      'A positive total return does not mean every trade was profitable, or that the strategy will be profitable in a different period.',
    ],
    related: ['annualized-return', 'cagr', 'net-profit', 'equity-curve', 'maximum-drawdown'],
    platformLinks: BT,
    interactive: 'equity-metrics',
  }),

  metricLesson({
    slug: 'net-profit',
    title: 'Net Profit & Final Equity',
    level: 'Beginner',
    summary: 'The dollar result of a run: final equity minus the starting cash, after every cost the engine charged.',
    keywords: ['final equity', 'profit', 'dollars', 'net p&l'],
    what: [
      '**Final equity** is the value of the account on the last bar: cash plus every open position marked at that bar’s close. **Net profit** is the change from the starting cash to that final equity.',
      'The Backtests results show final equity with the starting cash beneath it (“from $100,000”), so the net profit is the difference between the two.',
    ],
    why: [
      'Percentages hide scale. A 2% return on $10 million is $200,000; on $10,000 it is $200. Net profit tells you what the strategy actually earned in money, which is what pays for data, infrastructure and time.',
      'It is also the number that should reconcile with the per-instrument table: the sum of each instrument’s net P&L plus interest on idle cash.',
    ],
    formulas: [{ tex: String.raw`\text{Net profit} = E_{\text{final}} - \text{Initial cash}` }],
    where: ['\\(E_{\\text{final}}\\) = final cash + market value of open positions at the last close.'],
    engine:
      'Final equity comes straight from the accounting ledger (`final_equity` in the run summary), which holds money as exact fixed-point decimals. Fees, financing (cash interest and margin interest), borrow fees and dividends have all been booked to cash by then.',
    example: {
      intro: 'You start with $100,000. At the end you hold $61,000 of cash and 400 shares of a stock that closed at $129.',
      steps: ['Value the open position: 400 × $129 = $51,600.', 'Final equity: $61,000 + $51,600 = $112,600.', 'Net profit: $112,600 − $100,000 = $12,600.'],
      result: 'Net profit is **$12,600**, of which part is unrealized because the 400 shares are still open.',
    },
    high: 'More money made. Compare it with the capital and risk used: the same profit with half the drawdown is a better result.',
    low: 'Less money made, or a loss if negative. Check fees and slippage: a strategy that is profitable before costs can lose money after them.',
    misconceptions: [
      'Net profit is not all cash in hand: open positions are marked to market and could fall before they are sold.',
      'Two runs with the same net profit are not equally good if one took far more risk or far more trades to get there.',
    ],
    related: ['total-return', 'profit-and-loss', 'realized-unrealized-pnl', 'transaction-costs'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'profit-and-loss',
    title: 'Profit and Loss (P&L)',
    level: 'Beginner',
    summary: 'P&L is the money gained or lost by a position, a strategy or an account, broken down into realized gains, open gains and costs.',
    keywords: ['pnl', 'p&l', 'net p&l', 'per instrument', 'attribution'],
    what: [
      'Profit and loss is the change in value caused by trading. In Spectra it is reported for the whole account and for every instrument in the **Per instrument** table of a backtest.',
      'Net P&L for an instrument combines what has been locked in by closing trades (realized), what is still open (unrealized), and the costs and income attached to that instrument.',
    ],
    why: [
      'Breaking P&L down shows where the result came from. A strategy can look profitable overall while most instruments lose money and one carries the run, or while fees eat most of the gross gains.',
    ],
    formulas: [{ tex: String.raw`\text{Net P\&L} = \text{Realized} + \text{Unrealized} - \text{Fees} - \text{Borrow fees} + \text{Dividends}` }],
    where: [
      '**Realized**: profit or loss booked when a position was reduced or closed, at average cost.',
      '**Unrealized**: market value of the remaining position minus its cost.',
      '**Fees**: commissions charged on that instrument’s fills. **Borrow fees**: the cost of holding it short.',
    ],
    engine:
      'Per-instrument figures come from the ledger’s journal: realized P&L uses the average-cost method, unrealized P&L is `market value − cost`, and every fee on the instrument’s fills is charged to it. Interest on idle cash belongs to the account, not to any instrument, so the instrument rows add up to the account result less that interest.',
    example: {
      intro: 'You buy 100 shares at $50 (fee $1), sell 60 at $55 (fee $1), and the stock closes at $52.',
      steps: [
        'Realized: 60 × ($55 − $50) = $300.',
        'Unrealized: 40 shares are left at a cost of $50; marked at $52 they show 40 × $2 = $80.',
        'Fees: $1 + $1 = $2.',
        'Net P&L: $300 + $80 − $2 = $378.',
      ],
      result: 'The instrument’s net P&L is **$378**: $298 locked in after fees, $80 still open.',
    },
    high: 'The instrument or strategy contributed more to the result. Check whether most of it is unrealized; that part can still disappear.',
    low: 'It contributed little or lost money. Negative P&L concentrated in a few instruments can point to a data problem or a market the strategy does not suit.',
    misconceptions: [
      'P&L and return are different units: P&L is money, return is a percentage of capital.',
      'A large gross P&L can turn into a small net P&L once fees, slippage and borrow fees are included. Spectra always shows the net figure.',
    ],
    related: ['realized-unrealized-pnl', 'net-profit', 'transaction-costs', 'financing'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'realized-unrealized-pnl',
    title: 'Realized and Unrealized P&L',
    level: 'Beginner',
    summary: 'Realized P&L is profit locked in by closing a position; unrealized P&L is the gain or loss on positions that are still open.',
    keywords: ['realized', 'unrealized', 'open p&l', 'closed p&l', 'mark to market', 'average cost'],
    what: [
      '**Realized P&L** is booked when you sell shares you own (or buy back shares you are short). It no longer depends on the market.',
      '**Unrealized P&L** (also called open P&L) is what an open position would make or lose if it were closed at the current price. It changes with every price move.',
    ],
    why: [
      'A strategy that shows a large profit which is almost all unrealized has not proven anything yet: the gain depends on prices at one moment. Realized P&L shows what the strategy has actually captured.',
      'In paper trading, Alpaca reports unrealized P&L per position; the Paper Trading tab sums it across positions.',
    ],
    formulas: [
      { tex: String.raw`\text{Realized} = q_{\text{closed}} \times (p_{\text{exit}} - \bar{c})`, caption: 'for a long position' },
      { tex: String.raw`\text{Unrealized} = q_{\text{open}} \times (p_{\text{mark}} - \bar{c})` },
    ],
    where: ['\\(\\bar{c}\\) is the average cost per unit of the position.', '\\(p_{\\text{mark}}\\) is the latest price: the bar’s close in a backtest, the latest trade in paper trading.'],
    engine:
      'The ledger uses average cost. Buying more shares updates the average; selling some realizes the difference between the sale price and the average, without changing the average of the shares left. Realized P&L in the run summary excludes fees, which are reported separately.',
    example: {
      intro: 'You buy 100 shares at $40 and later 100 more at $50. The price is now $48 and you sell 50.',
      steps: [
        'Average cost: (100 × 40 + 100 × 50) / 200 = $45.',
        'Realized on the 50 sold: 50 × ($48 − $45) = $150.',
        'Unrealized on the 150 left: 150 × ($48 − $45) = $450.',
      ],
      result: 'Realized P&L is **$150** and unrealized P&L is **$450**; together they equal the total gain on the 200 shares, 200 × $3 = $600.',
    },
    high: 'High realized P&L means gains were captured. High unrealized P&L means gains are on paper and still exposed to the market.',
    low: 'Low or negative realized P&L with positive unrealized P&L can mean the strategy lets winners run and cuts losers, or simply that it has not closed anything yet.',
    misconceptions: [
      'Unrealized P&L is not “fake”: it is already in equity and in every return metric.',
      'Realizing a loss does not make it worse, and holding a loser does not avoid it: the loss is already in equity either way.',
    ],
    related: ['profit-and-loss', 'net-profit', 'paper-trading/monitoring-positions'],
    platformLinks: [...BT, { label: 'Open Paper Trading', href: '/studio?tab=paper' }],
  }),

  metricLesson({
    slug: 'annualized-return',
    title: 'Annualized Return',
    level: 'Intermediate',
    summary: 'The constant yearly growth rate that would turn the starting equity into the final equity over the run’s length.',
    keywords: ['annual return', 'yearly return', 'geometric return'],
    what: [
      'Annualized return converts a total return earned over any number of bars into an equivalent yearly rate. It is geometric: it assumes the growth compounds, not that it adds up.',
      'The number of bars per year comes from the run’s **periods per year** setting (252 for daily US equity bars by default).',
    ],
    why: [
      'You can only compare runs fairly when they are on the same time scale. Two years at +21% and six months at +10% look similar as total returns, but their annual rates are about 10% and 21%.',
    ],
    formulas: [{ tex: String.raw`R_{\text{ann}} = \left(\frac{E_n}{E_0}\right)^{\text{ppy}/n} - 1` }],
    where: ['\\(n\\) is the number of return periods (bars minus one).', '\\(\\text{ppy}\\) is periods per year from the run configuration.'],
    engine:
      'Computed as `(1 + total_return)^(ppy / n) − 1`. If the account was wiped out (final equity not positive) it is reported as −100%.',
    example: {
      intro: 'A daily run covers 504 return periods (two years at 252 bars a year) and makes +12.5% in total.',
      steps: ['Exponent: 252 / 504 = 0.5.', 'Growth factor: 1.125^0.5 = 1.0607.', 'Subtract 1: 0.0607.'],
      result: 'Annualized return is **+6.07%** a year, roughly half the total return, as expected for two years.',
    },
    high: 'Faster growth per year. Very high values from short runs are unreliable: a few good weeks annualize into an impossible number.',
    low: 'Slower growth, or a yearly loss if negative.',
    misconceptions: [
      'Annualizing a two-week backtest does not tell you what a year would look like; it just extrapolates two weeks.',
      'It is not the average of yearly returns. Volatility drags the compounded rate below the arithmetic average.',
      'A wrong periods-per-year setting (for example 252 on hourly bars) makes every annualized metric wrong.',
    ],
    related: ['cagr', 'total-return', 'volatility', 'sharpe-ratio'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'cagr',
    title: 'CAGR (Compound Annual Growth Rate)',
    level: 'Intermediate',
    summary: 'The yearly growth rate implied by compounding every period return of the run.',
    keywords: ['compound annual growth rate', 'annualized', 'geometric mean'],
    what: [
      'CAGR is the steady yearly rate that, compounded, produces the same growth as the strategy’s actual period returns. It is shown on the CAGR card of a backtest, with the run’s annualized volatility beneath it.',
    ],
    why: [
      'It is the standard way to compare growth across strategies, funds and benchmarks with different histories, and it is the numerator of the Calmar ratio.',
    ],
    formulas: [{ tex: String.raw`\text{CAGR} = \left(\prod_{t=1}^{n} (1 + r_t)\right)^{\text{ppy}/n} - 1` }],
    where: ['\\(r_t = E_t / E_{t-1} - 1\\) are the period returns of the equity curve.', '\\(n\\) is the number of returns and \\(\\text{ppy}\\) the periods per year.'],
    engine:
      'Computed in `spectra_engine::performance::cagr` from the period returns. For a normal equity curve this equals the annualized return (both compound from start to end); they differ only when a period with non-positive equity had to be skipped. CAGR is shown as “—” when it is undefined (no returns, or total growth not positive).',
    example: {
      intro: 'An account grows from $10,000 to $13,310 over three years of daily bars (756 returns, ppy 252).',
      steps: ['Total growth factor: 13,310 / 10,000 = 1.331.', 'Exponent: 252 / 756 = 1/3.', '1.331^(1/3) = 1.10.'],
      result: 'CAGR is **10% a year**: 10% compounded three times is 1.1 × 1.1 × 1.1 = 1.331.',
    },
    high: 'Faster compounding. Pair it with maximum drawdown: CAGR / max drawdown is the Calmar ratio.',
    low: 'Slower compounding, or shrinking capital if negative.',
    misconceptions: [
      'CAGR smooths the path: two strategies with the same CAGR can have very different drawdowns.',
      'It is not the average return per year. With returns of +50% and −50% the average is 0% but CAGR is about −13.4%.',
    ],
    related: ['annualized-return', 'total-return', 'calmar-ratio', 'volatility'],
    platformLinks: BT,
  }),

  metricLesson({
    slug: 'equity-curve',
    title: 'Equity Curve',
    level: 'Beginner',
    featured: true,
    summary: 'The chart of account value over time. Every return and risk metric in a backtest is computed from it.',
    keywords: ['equity chart', 'account value', 'performance chart', 'path'],
    what: [
      'The equity curve plots the account’s equity (cash plus marked positions) at every timestamp of the run. The Backtests results show it with the drawdown chart beneath and the monthly-returns heatmap below that.',
      'Long runs are downsampled for the chart, but each bucket keeps its lowest and highest point, so the deepest drawdown and the peak are always visible.',
    ],
    why: [
      'Numbers summarize; the curve shows the path. A smooth rising line, a staircase of a few big jumps, and a long flat stretch can all end at the same total return.',
      'Looking at it is the fastest way to spot a run that only worked in one period, a regime change, or a data problem such as a single bad price.',
    ],
    formulas: [{ tex: String.raw`E_t = \text{Cash}_t + \sum_i q_{i,t} \times \text{Close}_{i,t}` }],
    where: ['\\(q_{i,t}\\) is the position in instrument \\(i\\) after the fills at time \\(t\\) (negative when short).'],
    engine:
      'At each timestamp the engine first fills orders placed earlier, then marks every position at that bar’s close and records an equity point. The full curve is stored as `equity.csv` with the run, so you can download every point.',
    example: {
      intro: 'Six daily equity points (five daily returns): $10,000 → $10,200 → $10,098 → $10,404 → $10,196 → $10,500.',
      steps: [
        'Period returns: +2.00%, −1.00%, +3.03%, −2.00%, +2.98%.',
        'Running peak: 10,000, 10,200, 10,200, 10,404, 10,404, 10,500.',
        'The largest fall from a peak is from $10,404 to $10,196: about 2.0%.',
      ],
      result: 'Total return is **+5%** and the maximum drawdown about **2%**, both read straight off the curve.',
    },
    high: 'A curve that rises steadily with shallow dips is what most people look for.',
    low: 'A curve that is flat for long stretches or falls sharply shows when the strategy did not work, which is often more informative than the total.',
    misconceptions: [
      'A smooth curve on past data is not evidence of a smooth future: an optimized strategy can fit the past very closely.',
      'Log and linear scales change how a curve looks; on long runs a linear scale exaggerates late moves.',
    ],
    related: ['total-return', 'maximum-drawdown', 'monthly-returns', 'simulations/interpreting-simulation-output'],
    platformLinks: BT,
    diagram: { id: 'drawdown', caption: 'An equity curve with its running peak; the shaded gap is the drawdown.' },
  }),

  metricLesson({
    slug: 'monthly-returns',
    title: 'Monthly Returns Heatmap',
    level: 'Beginner',
    summary: 'A grid of the strategy’s return in each calendar month, coloured by sign and size, to show when it made and lost money.',
    keywords: ['heatmap', 'calendar returns', 'seasonality', 'monthly'],
    what: [
      'Each cell is the compounded return of the equity curve within one calendar month. Rows are years, columns months, and the colour shows whether the month was positive or negative and by how much.',
    ],
    why: [
      'It shows consistency. A strategy that makes 12% a year from one great month and eleven flat ones is very different from one that makes 1% most months.',
      'Clusters of red months point to regimes the strategy does not handle, which you can test separately in the Survival Lab.',
    ],
    formulas: [{ tex: String.raw`R_{\text{month}} = \frac{E_{\text{last bar of month}}}{E_{\text{last bar of previous month}}} - 1` }],
    engine:
      'Monthly returns are computed by the engine from the stored equity curve and served with the run (the `monthly-returns` endpoint of a backtest). The first month is measured from the first equity point, and a month the run covers only in part is flagged as partial.',
    example: {
      intro: 'Equity is $100,000 at the end of January and $103,000 at the end of February.',
      steps: ['Divide: 103,000 / 100,000 = 1.03.', 'Subtract 1.'],
      result: 'February’s cell shows **+3.0%**.',
    },
    high: 'Mostly positive months suggest a steady edge.',
    low: 'Long runs of negative months suggest a regime dependency or decay.',
    misconceptions: ['Months do not add up to the year: they compound.', 'A pattern of strong months in the same calendar month across a few years is rarely real seasonality.'],
    related: ['equity-curve', 'total-return', 'volatility'],
    platformLinks: BT,
  }),
];
