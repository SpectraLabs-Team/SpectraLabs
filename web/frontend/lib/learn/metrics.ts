/**
 * Metric registry: the one place where a metric's short explanation lives.
 *
 * Every ⓘ icon in the Studio (backtest results, risk lab, paper trading, optimization lab, sidebar)
 * reads its text from here, and every `learnPath` points at a real lesson in the Learning Center
 * (scripts/test-learn.mjs fails if one does not). Short descriptions follow the engine's actual
 * definitions: see each lesson's "How Spectra calculates it" note for the exact formula.
 */

export type MetricCategory = 'returns' | 'risk' | 'risk-adjusted' | 'trades' | 'costs' | 'exposure' | 'benchmark' | 'validation' | 'account';

export interface MetricDefinition {
  /** Display name. */
  name: string;
  /** One or two sentences, shown in the tooltip. */
  shortDescription: string;
  /** Route of the detailed lesson. */
  learnPath: string;
  category: MetricCategory;
  /** Compact plain-text formula for the tooltip (the lesson renders the full one). */
  formula?: string;
  /** One line on reading the value. */
  interpretation?: string;
}

const m = (slug: string) => `/learn/metrics/${slug}`;

export const metricDefinitions = {
  // Returns & P&L
  total_return: {
    name: 'Total Return',
    shortDescription: 'Growth of equity over the whole run, net of all costs: final equity divided by starting equity, minus 1.',
    learnPath: m('total-return'),
    category: 'returns',
    formula: 'E_end / E_start − 1',
    interpretation: 'Not annualized: compare runs of different lengths with CAGR.',
  },
  final_equity: {
    name: 'Final Equity',
    shortDescription: 'Cash plus open positions marked at the last close. The difference from the starting cash is the run’s net profit.',
    learnPath: m('net-profit'),
    category: 'returns',
    formula: 'cash + Σ qty × last close',
  },
  annualized_return: {
    name: 'Annualized Return',
    shortDescription: 'The constant yearly rate that compounds the starting equity into the final equity over the run’s length.',
    learnPath: m('annualized-return'),
    category: 'returns',
    formula: '(E_end / E_start)^(ppy / n) − 1',
  },
  cagr: {
    name: 'CAGR',
    shortDescription: 'Compound annual growth rate implied by the run’s period returns.',
    learnPath: m('cagr'),
    category: 'returns',
    formula: '(Π(1 + r))^(ppy / n) − 1',
    interpretation: 'Short runs annualize into unreliable numbers.',
  },
  equity_curve: {
    name: 'Equity Curve',
    shortDescription: 'Account value at every timestamp: cash plus positions marked at each bar’s close. Every metric here is computed from it.',
    learnPath: m('equity-curve'),
    category: 'returns',
  },
  monthly_returns: {
    name: 'Monthly Returns',
    shortDescription: 'The compounded return of the equity curve in each calendar month.',
    learnPath: m('monthly-returns'),
    category: 'returns',
  },
  net_pnl: {
    name: 'Net P&L',
    shortDescription: 'Realized plus unrealized P&L, minus fees and borrow fees, plus dividends.',
    learnPath: m('profit-and-loss'),
    category: 'returns',
    formula: 'realized + unrealized − fees − borrow + dividends',
  },
  realized_pnl: {
    name: 'Realized P&L',
    shortDescription: 'Profit or loss locked in by reducing or closing positions, at average cost, before fees.',
    learnPath: m('realized-unrealized-pnl'),
    category: 'returns',
  },
  unrealized_pnl: {
    name: 'Unrealized P&L',
    shortDescription: 'Gain or loss on positions that are still open: market value minus cost. It changes with every price move.',
    learnPath: m('realized-unrealized-pnl'),
    category: 'returns',
    formula: 'qty × (price − average cost)',
  },
  contribution: {
    name: 'Contribution',
    shortDescription: 'A strategy’s profit on its own books as a share of the account’s starting cash; contributions add up to the account’s return less idle-cash interest.',
    learnPath: '/learn/engine/portfolio-management',
    category: 'returns',
  },

  // Risk
  annualized_volatility: {
    name: 'Annualized Volatility',
    shortDescription: 'Sample standard deviation of the period returns, scaled to a year by √(periods per year).',
    learnPath: m('volatility'),
    category: 'risk',
    formula: 'stdev(r) × √ppy',
  },
  max_drawdown: {
    name: 'Maximum Drawdown',
    shortDescription: 'The largest peak-to-trough decline in portfolio value during the run, as a fraction of the peak.',
    learnPath: m('maximum-drawdown'),
    category: 'risk',
    formula: 'max (peak − equity) / peak',
    interpretation: 'A 50% drawdown needs a 100% gain to recover.',
  },
  current_drawdown: {
    name: 'Current Drawdown',
    shortDescription: 'How far the final equity sits below the highest equity reached.',
    learnPath: m('drawdown-duration'),
    category: 'risk',
  },
  longest_underwater: {
    name: 'Longest Underwater',
    shortDescription: 'The longest run of consecutive periods spent below a previous equity peak.',
    learnPath: m('drawdown-duration'),
    category: 'risk',
  },
  drawdown_episodes: {
    name: 'Drawdown Episodes',
    shortDescription: 'How many separate stretches the equity spent below its running peak.',
    learnPath: m('drawdown-duration'),
    category: 'risk',
  },
  ulcer_index: {
    name: 'Ulcer Index',
    shortDescription: 'Root-mean-square of the drawdown at every point: rises with both the depth and the length of drawdowns.',
    learnPath: m('ulcer-index'),
    category: 'risk',
    formula: '√mean(drawdown²)',
  },
  var_historical: {
    name: 'Value at Risk (historical)',
    shortDescription: 'The loss exceeded by only the worst 5% of the run’s period returns (nearest-rank quantile). Needs at least 20 returns.',
    learnPath: m('value-at-risk'),
    category: 'risk',
    interpretation: 'Not the worst case: see expected shortfall for the average beyond it.',
  },
  expected_shortfall_historical: {
    name: 'Expected Shortfall',
    shortDescription: 'The average loss over the worst 5% of the run’s period returns.',
    learnPath: m('expected-shortfall'),
    category: 'risk',
  },
  worst_period: {
    name: 'Worst Period',
    shortDescription: 'The single worst return between two consecutive equity points.',
    learnPath: m('worst-period'),
    category: 'risk',
  },
  var_monte_carlo: {
    name: 'Value at Risk (Monte Carlo)',
    shortDescription: 'The loss over the horizon that only the worst (1 − confidence) share of simulated paths exceed.',
    learnPath: m('value-at-risk'),
    category: 'risk',
    interpretation: 'The model treats the portfolio as one position and ignores correlations.',
  },
  cvar_monte_carlo: {
    name: 'Expected Shortfall (CVaR)',
    shortDescription: 'The average simulated loss at or beyond the Value at Risk.',
    learnPath: m('expected-shortfall'),
    category: 'risk',
  },
  risk_volatility: {
    name: 'Model Volatility',
    shortDescription: 'The annualized volatility the Monte Carlo model uses for the portfolio; the subtitle shows the standard deviation of the simulated horizon P&L.',
    learnPath: m('volatility'),
    category: 'risk',
  },
  evt_var: {
    name: 'EVT VaR',
    shortDescription: 'Value at Risk from a Generalized Pareto fit to the largest simulated losses (peaks over the 85th-percentile threshold).',
    learnPath: m('evt-tail-risk'),
    category: 'risk',
  },
  evt_tail_ratio: {
    name: 'Tail Ratio (EVT / MC)',
    shortDescription: 'EVT VaR divided by Monte Carlo VaR. Above 1 means the tail is heavier than the Gaussian model suggests.',
    learnPath: m('evt-tail-risk'),
    category: 'risk',
  },
  garch_forecast_vol: {
    name: '1-Day Volatility Forecast',
    shortDescription: 'GARCH forecast of tomorrow\u2019s volatility, annualized. It reacts to recent shocks and decays towards the long-run level.',
    learnPath: m('garch-volatility'),
    category: 'risk',
  },
  garch_unconditional_vol: {
    name: 'Unconditional Volatility',
    shortDescription: 'The long-run volatility the GARCH model reverts to (set equal to the sample volatility by variance targeting), annualized.',
    learnPath: m('garch-volatility'),
    category: 'risk',
  },
  garch_persistence: {
    name: 'Volatility Persistence',
    shortDescription: 'α + β (plus γ/2 for GJR): how slowly a volatility shock fades. Near 1 means shocks last a long time.',
    learnPath: m('garch-volatility'),
    category: 'risk',
  },
  garch_leverage: {
    name: 'Asymmetric Leverage (γ)',
    shortDescription: 'Extra variance added by negative shocks in GJR-GARCH. γ > 0 means falls raise volatility more than rises.',
    learnPath: m('garch-volatility'),
    category: 'risk',
  },
  regime: {
    name: 'Detected Regime',
    shortDescription: 'The market state (calm or turbulent) a hidden Markov model assigns to the most recent data.',
    learnPath: m('market-regimes'),
    category: 'risk',
  },
  regime_var: {
    name: 'Regime-Conditional VaR',
    shortDescription: 'Value at Risk using the mean and volatility of the current regime instead of the whole history.',
    learnPath: m('market-regimes'),
    category: 'risk',
  },
  regime_transitions: {
    name: 'Transition Probability Matrix',
    shortDescription: 'P(next regime = j | current regime = i). High values on the diagonal mean regimes persist.',
    learnPath: m('market-regimes'),
    category: 'risk',
  },
  stress_pnl: {
    name: 'Stress P&L',
    shortDescription: 'The immediate profit or loss if every price moved by the scenario\u2019s shock factor.',
    learnPath: '/learn/simulations/stress-testing',
    category: 'risk',
  },
  stress_post_var: {
    name: 'Post-Shock VaR',
    shortDescription: 'Value at Risk of the portfolio after the scenario shock has been applied.',
    learnPath: '/learn/simulations/stress-testing',
    category: 'risk',
  },
  skewness: {
    name: 'Skewness',
    shortDescription: 'Asymmetry of the return distribution: negative when large losses are more common than large gains.',
    learnPath: m('skewness-and-kurtosis'),
    category: 'risk',
  },
  excess_kurtosis: {
    name: 'Excess Kurtosis',
    shortDescription: 'How fat the return distribution’s tails are compared with a normal distribution (0 = normal).',
    learnPath: m('skewness-and-kurtosis'),
    category: 'risk',
  },
  gross_exposure: {
    name: 'Gross Exposure',
    shortDescription: 'Total absolute market value of every open position, long and short, in currency.',
    learnPath: m('exposure'),
    category: 'exposure',
    formula: 'Σ |quantity × price|',
  },
  hhi: {
    name: 'Concentration (HHI)',
    shortDescription: 'Herfindahl index of the portfolio’s gross weights: 1 means one position holds everything, 1/N means N equal positions.',
    learnPath: m('concentration-hhi'),
    category: 'risk',
    formula: 'Σ wᵢ², where wᵢ = |vᵢ| / Σ|vⱼ|',
    interpretation: 'Below 0.10 is unconcentrated, 0.10 to 0.18 moderately concentrated, above 0.18 highly concentrated (the Portfolio Studio bands).',
  },
  gross_leverage: {
    name: 'Gross Leverage',
    shortDescription: 'Total absolute value of all positions divided by equity, shown against the account’s leverage limit.',
    learnPath: m('leverage'),
    category: 'risk',
    formula: 'Σ |position value| / equity',
  },

  // Risk-adjusted
  sharpe_ratio: {
    name: 'Sharpe Ratio',
    shortDescription: 'Measures risk-adjusted returns by comparing the average excess return with the volatility of returns, annualized.',
    learnPath: m('sharpe-ratio'),
    category: 'risk-adjusted',
    formula: '(mean(r) − rf/ppy) / stdev(r) × √ppy',
    interpretation: 'Higher generally means better risk-adjusted performance, but it does not guarantee future returns.',
  },
  sortino_ratio: {
    name: 'Sortino Ratio',
    shortDescription: 'Like the Sharpe ratio, but only returns below the risk-free target count as risk.',
    learnPath: m('sortino-ratio'),
    category: 'risk-adjusted',
    formula: '(mean(r) − rf/ppy) / downside dev × √ppy',
  },
  calmar_ratio: {
    name: 'Calmar Ratio',
    shortDescription: 'CAGR divided by the maximum drawdown: yearly growth per unit of worst-case decline.',
    learnPath: m('calmar-ratio'),
    category: 'risk-adjusted',
    formula: 'CAGR / max drawdown',
  },
  omega_ratio: {
    name: 'Omega Ratio',
    shortDescription: 'Sum of period gains above zero divided by the sum of losses below zero.',
    learnPath: m('omega-ratio'),
    category: 'risk-adjusted',
  },

  // Trades
  trade_count: {
    name: 'Trades (Round Trips)',
    shortDescription: 'Completed position lifecycles: a trade opens when a position leaves zero and ends when it returns to zero or flips sign.',
    learnPath: m('number-of-trades'),
    category: 'trades',
    interpretation: 'The sample size for every trade statistic.',
  },
  win_rate: {
    name: 'Win Rate',
    shortDescription: 'The share of completed trades with positive net P&L after fees.',
    learnPath: m('win-rate'),
    category: 'trades',
    formula: 'winning trades / all trades',
    interpretation: 'Says nothing about profitability on its own: pair it with the payoff ratio.',
  },
  profit_factor: {
    name: 'Profit Factor',
    shortDescription: 'Gross profit of winning trades divided by the gross loss of losing trades.',
    learnPath: m('profit-factor'),
    category: 'trades',
    formula: 'Σ wins / |Σ losses|',
    interpretation: 'Above 1 means closed trades made money. Shown as — with no losing trades.',
  },
  payoff_ratio: {
    name: 'Payoff Ratio',
    shortDescription: 'Average winning trade divided by average losing trade: the realized risk-reward ratio.',
    learnPath: m('risk-reward-ratio'),
    category: 'trades',
    formula: 'average win / average loss',
  },
  expectancy: {
    name: 'Expectancy',
    shortDescription: 'Average net P&L per closed trade, in account currency.',
    learnPath: m('expectancy'),
    category: 'trades',
    formula: 'total net P&L / trades',
  },
  best_worst_trade: {
    name: 'Best / Worst Trade',
    shortDescription: 'The net P&L of the single most profitable and least profitable closed trades.',
    learnPath: m('average-trade-return'),
    category: 'trades',
  },
  avg_holding: {
    name: 'Average Holding',
    shortDescription: 'Average calendar time from a trade’s first fill to its closing fill, in days.',
    learnPath: m('holding-period'),
    category: 'trades',
  },
  open_positions: {
    name: 'Open Positions',
    shortDescription: 'Positions still held at the end of the run: marked to market but not counted as closed trades.',
    learnPath: m('orders-and-fills'),
    category: 'trades',
  },
  orders_fills: {
    name: 'Orders / Fills',
    shortDescription: 'Orders placed and the executions they produced. One order can fill in several pieces, and some never fill.',
    learnPath: m('orders-and-fills'),
    category: 'trades',
  },

  // Costs & exposure
  fees: {
    name: 'Fees',
    shortDescription: 'Commissions charged on every fill (per unit, basis points of notional, with a minimum), deducted from cash.',
    learnPath: m('transaction-costs'),
    category: 'costs',
  },
  slippage: {
    name: 'Slippage',
    shortDescription: 'Adverse price move applied to market and stop fills: flat bps + half the spread + volume-based bps.',
    learnPath: m('slippage'),
    category: 'costs',
  },
  financing: {
    name: 'Financing',
    shortDescription: 'Interest earned on cash minus margin interest and borrow fees paid.',
    learnPath: m('financing'),
    category: 'costs',
  },
  borrow_fees: {
    name: 'Borrow Fees',
    shortDescription: 'The cost of holding short positions, charged every bar on their market value.',
    learnPath: m('financing'),
    category: 'costs',
  },
  turnover: {
    name: 'Turnover (annual)',
    shortDescription: 'Traded notional divided by average equity, scaled to a year.',
    learnPath: m('turnover'),
    category: 'costs',
    formula: 'Σ |notional| / mean equity × ppy / n',
  },
  average_gross_exposure: {
    name: 'Average Gross Exposure',
    shortDescription: 'Average of the absolute value of all positions as a share of equity.',
    learnPath: m('exposure'),
    category: 'exposure',
  },
  average_net_exposure: {
    name: 'Average Net Exposure',
    shortDescription: 'Average of long minus short position value as a share of equity.',
    learnPath: m('exposure'),
    category: 'exposure',
  },
  time_in_market: {
    name: 'Time in Market',
    shortDescription: 'Share of bars on which the strategy held any position.',
    learnPath: m('exposure'),
    category: 'exposure',
  },

  // Benchmark
  benchmark_return: {
    name: 'Benchmark Return',
    shortDescription: 'Total return of an equal-weight buy-and-hold of every instrument in the dataset, with no costs.',
    learnPath: m('benchmark-excess-return'),
    category: 'benchmark',
  },
  excess_return: {
    name: 'Excess Return',
    shortDescription: 'The strategy’s total return minus the benchmark’s.',
    learnPath: m('benchmark-excess-return'),
    category: 'benchmark',
  },
  beta: {
    name: 'Beta',
    shortDescription: 'Sensitivity to the benchmark: covariance of returns divided by the benchmark’s variance.',
    learnPath: m('beta'),
    category: 'benchmark',
    formula: 'cov(r_s, r_b) / var(r_b)',
  },
  alpha: {
    name: 'Alpha (annual)',
    shortDescription: 'Average return not explained by beta to the benchmark, annualized simply.',
    learnPath: m('alpha'),
    category: 'benchmark',
    formula: '(mean r_s − β · mean r_b) × ppy',
  },
  correlation: {
    name: 'Correlation',
    shortDescription: 'How closely strategy and benchmark returns move together, from −1 to +1.',
    learnPath: m('correlation'),
    category: 'benchmark',
  },
  tracking_error: {
    name: 'Tracking Error',
    shortDescription: 'How far the strategy’s returns stray from the benchmark’s: the volatility of the return gap, annualized.',
    learnPath: m('information-ratio'),
    category: 'benchmark',
    formula: 'σ(rₛ − r_b) · √ppy',
  },
  information_ratio: {
    name: 'Information Ratio',
    shortDescription: 'Average return above the benchmark per unit of tracking error, annualized.',
    learnPath: m('information-ratio'),
    category: 'benchmark',
  },
  capture_ratios: {
    name: 'Up / Down Capture',
    shortDescription: 'The strategy’s average return in benchmark up periods (and down periods) relative to the benchmark’s.',
    learnPath: m('capture-ratios'),
    category: 'benchmark',
  },

  // Validation
  best_sharpe: {
    name: 'Best Observed Sharpe',
    shortDescription: 'The highest Sharpe ratio among the trials of a search. Inflated by selection: compare it with the Deflated Sharpe Ratio.',
    learnPath: m('sharpe-ratio'),
    category: 'validation',
  },
  psr: {
    name: 'Probabilistic Sharpe Ratio',
    shortDescription: 'Probability that the true Sharpe ratio is above the benchmark, given the record length, skewness and kurtosis.',
    learnPath: m('probabilistic-sharpe-ratio'),
    category: 'validation',
  },
  dsr: {
    name: 'Deflated Sharpe Ratio',
    shortDescription: 'PSR measured against the Sharpe ratio expected from the best of N skill-less trials: corrects for searching.',
    learnPath: m('deflated-sharpe-ratio'),
    category: 'validation',
  },
  pbo: {
    name: 'Probability of Backtest Overfitting',
    shortDescription: 'Share of CSCV splits where the in-sample winner ranks in the bottom half out-of-sample.',
    learnPath: m('probability-of-backtest-overfitting'),
    category: 'validation',
    interpretation: 'Near 0.5 or above: in-sample selection carries no information.',
  },
  min_trl: {
    name: 'Minimum Track Record Length',
    shortDescription: 'Observations needed before the observed Sharpe is significantly above the benchmark at the chosen confidence.',
    learnPath: m('minimum-track-record-length'),
    category: 'validation',
  },
  is_oos_sharpe: {
    name: 'In-Sample / Out-of-Sample Sharpe',
    shortDescription: 'Average Sharpe of the chosen parameters on their train windows versus the test windows that followed. A large drop signals overfitting.',
    learnPath: '/learn/simulations/walk-forward-analysis',
    category: 'validation',
  },

  // Account (paper trading)
  total_equity: {
    name: 'Total Equity',
    shortDescription: 'Cash plus the market value of every position in the paper account.',
    learnPath: '/learn/paper-trading/account-balances',
    category: 'account',
  },
  cash_balance: {
    name: 'Cash Balance',
    shortDescription: 'Money in the paper account that is not invested; negative when using margin.',
    learnPath: '/learn/paper-trading/account-balances',
    category: 'account',
  },
  buying_power: {
    name: 'Buying Power',
    shortDescription: 'How much the paper account can buy now, including margin.',
    learnPath: '/learn/paper-trading/account-balances',
    category: 'account',
  },
  portfolio_value: {
    name: 'Portfolio Value',
    shortDescription: 'Net market value of the loaded portfolio’s positions, the value the risk simulation starts from.',
    learnPath: '/learn/simulations/monte-carlo-risk-simulation',
    category: 'account',
  },
  pnl_gap: {
    name: 'Backtest-to-Paper P&L Gap',
    shortDescription: 'Paper-trading P&L minus the backtest\u2019s P&L on matched trades: how far live execution departed from the simulation.',
    learnPath: '/learn/paper-trading/tracking-portfolio-performance',
    category: 'account',
  },
  slippage_drift: {
    name: 'Slippage Drift',
    shortDescription: 'Average difference between paper fill prices and the prices the backtest assumed: realized slippage beyond the model.',
    learnPath: m('slippage'),
    category: 'costs',
  },
  timing_latency: {
    name: 'Timing Latency',
    shortDescription: 'Average delay between when the backtest would have filled and when the paper order actually filled.',
    learnPath: '/learn/simulations/simulation-assumptions',
    category: 'costs',
  },
  missed_fills: {
    name: 'Missed / Unfilled',
    shortDescription: 'Orders the backtest filled but paper trading did not (rejected, expired or only partly filled).',
    learnPath: m('orders-and-fills'),
    category: 'trades',
  },
  order_status: {
    name: 'Order Status',
    shortDescription: 'Where an order is in its lifecycle: working (new, accepted, partially filled) or final (filled, canceled, expired, rejected).',
    learnPath: '/learn/paper-trading/order-statuses',
    category: 'account',
  },
} satisfies Record<string, MetricDefinition>;

export type MetricId = keyof typeof metricDefinitions;

export function getMetric(id: MetricId): MetricDefinition {
  return metricDefinitions[id];
}

export const METRIC_CATEGORY_LABELS: Record<MetricCategory, string> = {
  returns: 'Returns & P&L',
  risk: 'Risk',
  'risk-adjusted': 'Risk-adjusted',
  trades: 'Trade statistics',
  costs: 'Costs',
  exposure: 'Exposure',
  benchmark: 'Benchmark',
  validation: 'Overfitting & validation',
  account: 'Account',
};
