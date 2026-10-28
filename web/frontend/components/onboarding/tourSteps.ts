import type { StudioTab } from '@/components/studio/StudioShell';

/**
 * Guided-tour steps. Each step names one or more `data-tour` targets in priority order; the tour
 * highlights the first one that is rendered and visible (desktop sidebar items are hidden on
 * phones, so those steps fall back to a later target or to a centred card). Steps with `tab`
 * switch the Studio to that tab first and wait for the target to render.
 *
 * Steps only describe and highlight. None of them clicks, submits or changes anything.
 */
export interface TourStep {
  id: string;
  title: string;
  body: string;
  targets: string[];
  tab?: StudioTab;
  side?: 'top' | 'right' | 'bottom' | 'left';
  /** Only include this step when the given Studio tab exists (features that may be absent). */
  requiresTab?: StudioTab;
  /** Only include this step for accounts with this role (the admin console). */
  role?: 'admin';
}

export const MAIN_TOUR: TourStep[] = [
  {
    id: 'dashboard',
    tab: 'overview',
    title: 'Your dashboard',
    body: 'The Workspace Overview gathers your recent backtests, risk posture, kill switches and pre-trade decisions in one place. The sidebar on the left takes you to every workspace.',
    targets: ['nav-overview', 'studio-main'],
    side: 'right',
  },
  {
    id: 'simulations',
    tab: 'backtests',
    requiresTab: 'backtests',
    title: 'Run trading simulations',
    body: 'Backtests replay a strategy over historical bars with realistic costs. Choose a dataset version, a strategy and its parameters, adjust costs and risk settings, then queue the run. For Monte Carlo risk simulations, open the Risk & Stress Lab.',
    targets: ['backtest-form', 'nav-backtests', 'studio-main'],
    side: 'bottom',
  },
  {
    id: 'strategies',
    tab: 'backtests',
    requiresTab: 'strategies',
    title: 'Choose or configure a strategy',
    body: 'Pick a built-in strategy (SMA crossover, z-score mean reversion, buy and hold, equal weight) and set its parameters here, or write your own rules in the Strategy Studio and run a saved version.',
    targets: ['strategy-picker', 'nav-strategies', 'backtest-form'],
    side: 'bottom',
  },
  {
    id: 'results',
    tab: 'backtests',
    requiresTab: 'backtests',
    title: 'Read simulation results',
    body: 'A finished run shows its equity and drawdown charts, monthly returns, performance, risk and trade statistics, and a benchmark comparison. Every metric has an ⓘ icon: hover or tap it for a short explanation and a link to the full lesson.',
    targets: ['backtest-results-header', 'backtest-results', 'studio-main'],
    side: 'bottom',
  },
  {
    id: 'risk',
    tab: 'risk',
    requiresTab: 'risk',
    title: 'Risk & Stress Lab',
    body: 'Stress-test a strategy with Monte Carlo simulation, historical scenarios and regime and volatility models. Each workspace starts with a How to use panel that explains its steps and controls.',
    targets: ['nav-risk', 'studio-main'],
    side: 'right',
  },
  {
    id: 'risk-center',
    tab: 'risk-center',
    requiresTab: 'risk-center',
    title: 'Risk Center & controls',
    body: 'Set pre-trade limits for exposure, leverage, drawdown, daily loss and VaR, watch their utilization, and use the kill switch to block new orders.',
    targets: ['nav-risk-center', 'studio-main'],
    side: 'right',
  },
  {
    id: 'portfolio',
    tab: 'portfolio',
    requiresTab: 'portfolio',
    title: 'Portfolio Studio',
    body: 'Combine positions or strategies into one portfolio and check how their risk and concentration add up.',
    targets: ['nav-portfolio', 'studio-main'],
    side: 'right',
  },
  {
    id: 'data',
    tab: 'data',
    requiresTab: 'data',
    title: 'Data',
    body: 'Upload price files or connect a provider, check the quality report, and save dataset versions so every run records exactly which data it used.',
    targets: ['nav-data', 'studio-main'],
    side: 'right',
  },
  {
    id: 'optimization',
    tab: 'optimization',
    requiresTab: 'optimization',
    title: 'Optimization Lab',
    body: 'Walk-forward analysis tests parameters on data they were not fitted to. Read the out-of-sample results and the overfitting statistics before trusting the best parameters.',
    targets: ['nav-optimization', 'studio-main'],
    side: 'right',
  },
  {
    id: 'benchmark',
    tab: 'benchmark',
    requiresTab: 'benchmark',
    title: 'Hardware benchmarks',
    body: 'Time the engine on this machine so you know which simulation sizes are practical.',
    targets: ['nav-benchmark', 'studio-main'],
    side: 'right',
  },
  {
    id: 'registry',
    tab: 'registry',
    requiresTab: 'registry',
    title: 'Experiment registry',
    body: 'Every run, its inputs and its results, kept as a record that cannot be edited after it is written. Use it to reproduce an earlier result.',
    targets: ['nav-registry', 'studio-main'],
    side: 'right',
  },
  {
    id: 'settings',
    tab: 'settings',
    requiresTab: 'settings',
    title: 'Market data & Alpaca settings',
    body: 'Connect your Alpaca paper account and choose the market data source. Keys are stored encrypted for this workspace.',
    targets: ['nav-settings', 'studio-main'],
    side: 'right',
  },
  {
    id: 'engine',
    title: 'The trading engine',
    body: 'Everything runs on the Spectra Rust engine: an event-driven backtester with no look-ahead, an exact accounting ledger, cost models and a Monte Carlo risk engine. Its status is shown here. Pre-trade risk limits and kill switches live in the Risk Center.',
    targets: ['engine-status', 'nav-risk-center'],
    side: 'right',
  },
  {
    id: 'paper',
    requiresTab: 'paper',
    title: 'Practise with paper trading',
    body: 'Connect an Alpaca paper account with its API keys, then search symbols, read candlestick charts and place simulated orders. You do not need to connect anything to finish this tour, and the tour never places orders.',
    targets: ['nav-paper'],
    side: 'right',
  },
  {
    id: 'learn',
    title: 'The Learning Center',
    body: 'Lessons on trading concepts and how simulations work, plus the Metric library: every metric in the Studio, with its formula, example and limitations. Open it from here or from the Help menu, where you can also take this tour again.',
    targets: ['nav-learn', 'help-menu'],
    side: 'right',
  },
  {
    id: 'account',
    title: 'Account settings',
    body: 'Your name and initials at the top of the sidebar open your account: profile, password, two-factor sign-in, signed-in devices and account deletion. It also explains the activity data Spectra keeps about your use of the platform.',
    targets: ['account-link', 'studio-main'],
    side: 'right',
  },
  {
    id: 'admin',
    role: 'admin',
    title: 'Admin console',
    body: 'Platform admins see the Admin console: overview figures, the user directory, analytics, engine workers and the audit log. Every view of the directory, analytics and an account is recorded.',
    targets: ['admin-link'],
    side: 'bottom',
  },
];

/** Short optional tours for complex pages, offered from the Help menu only. */
export const PAGE_TOURS: Partial<Record<StudioTab, TourStep[]>> = {
  backtests: [
    { id: 'bt-form', title: 'New backtest', body: 'Pick a dataset version and a strategy, set its parameters, then costs, risk settings and periods per year (used to annualize every metric).', targets: ['backtest-form'], side: 'bottom' },
    { id: 'bt-strategy', title: 'Strategy and parameters', body: 'Built-in strategies or a saved Strategy Studio version. Several strategies can share one account in a multi-strategy run.', targets: ['strategy-picker'], side: 'bottom' },
    { id: 'bt-runs', title: 'Your runs', body: 'Queued, running and finished backtests. Select one to see its results; finished runs can be re-run or downloaded.', targets: ['backtest-runs'], side: 'left' },
    { id: 'bt-results', title: 'Results', body: 'Headline numbers, charts and statistics. Use the ⓘ icons to learn what each metric means and how it is calculated.', targets: ['backtest-results'], side: 'top' },
  ],
  paper: [
    { id: 'pp-connect', title: 'Connect Alpaca', body: 'Paste the Key ID and Secret Key of an Alpaca paper account. Spectra checks them with Alpaca and stores them encrypted for this workspace.', targets: ['paper-connect'], side: 'right' },
    { id: 'pp-account', title: 'Account summary', body: 'Equity, cash, buying power and unrealized P&L of your paper account. The ⓘ icons explain each figure.', targets: ['paper-account'], side: 'bottom' },
    { id: 'pp-search', title: 'Search and watchlist', body: 'Find a tradable symbol and add it to your watchlist.', targets: ['paper-search'], side: 'right' },
    { id: 'pp-chart', title: 'Candlestick chart', body: 'Each candle is one bar: open, high, low and close. Switch the timeframe above the chart.', targets: ['paper-chart'], side: 'bottom' },
    { id: 'pp-ticket', title: 'Order ticket', body: 'Choose buy or sell, market or limit, and a quantity. Every order is reviewed in a confirmation dialog before anything is sent.', targets: ['paper-ticket'], side: 'left' },
    { id: 'pp-positions', title: 'Positions and orders', body: 'Open positions with their unrealized P&L, and your orders with their status. Working orders can be cancelled.', targets: ['paper-positions'], side: 'top' },
  ],
};
