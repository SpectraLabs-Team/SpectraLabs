import type { StudioTab } from '@/components/studio/StudioShell';

/**
 * Workspace guide registry: one entry per Studio workspace, shown as the "How to use" panel at the
 * top of that workspace and used by the guided tour and tests. Typed as Record<StudioTab, …>, so a
 * new workspace cannot be added without a guide.
 *
 * `lessons` links only to real Learning Center lessons (test-workspaces.mjs checks every link).
 */
export interface LessonLink {
  title: string;
  href: string;
}

export interface WorkspaceGuide {
  /** Plain-language purpose: what the workspace is for. */
  purpose: string;
  /** Steps in the order a person normally does them. */
  steps: string[];
  /** What the person provides. */
  inputs: string[];
  /** Buttons and options, and what each one does. */
  controls: string[];
  /** What the workspace shows or produces. */
  outputs: string[];
  /** Common mistakes and what to check. */
  pitfalls: string[];
  /** Lessons that explain the concepts used here. */
  lessons: LessonLink[];
  /** Admin-only workspaces are hidden for other users. */
  adminOnly?: boolean;
}

export const WORKSPACES: Record<StudioTab, WorkspaceGuide> = {
  overview: {
    purpose: 'A one-page summary of your workspace: recent backtests, risk posture, kill switches and pre-trade decisions.',
    steps: [
      'Read the headline figures to see what changed since you last looked.',
      'Open any panel that shows a warning, then follow its link to the workspace that controls it.',
      'Use the links in the sidebar to move to the workspace you need.',
    ],
    inputs: ['Nothing. This page only reads your workspace.'],
    controls: ['Each card links to the workspace where you can act on it.'],
    outputs: ['Exposure, leverage, drawdown and daily loss gauges; recent runs; kill switch state; the last pre-trade decisions.'],
    pitfalls: ['Figures come from the last saved state. Refresh the page after a run finishes if a number looks stale.'],
    lessons: [{ title: 'Interpreting simulation output', href: '/learn/simulations/interpreting-simulation-output' }],
  },
  risk: {
    purpose: 'Stress-test a strategy with Monte Carlo simulation and historical scenarios, and fit volatility and regime models to its returns.',
    steps: [
      'Choose the return series to test, or a saved backtest run.',
      'Set the number of simulated paths and the confidence level.',
      'Run the Monte Carlo simulation, then the stress scenarios.',
      'Compare the tail measures (VaR and expected shortfall) with your loss limits.',
    ],
    inputs: ['A return series from a finished backtest or an uploaded dataset.', 'Number of paths, horizon, confidence level and random seed.'],
    controls: ['Simulation settings: start the simulation as a background job.', 'Seed: fixes the random numbers so a result can be reproduced.', 'Regime and GARCH panels: fit the models on the same series.'],
    outputs: ['Value at Risk and expected shortfall, the distribution of outcomes, scenario losses and model parameters.'],
    pitfalls: ['A fixed seed reproduces the same result; change it to see how much the answer depends on luck.', 'Tail estimates from short histories are unreliable. Check the number of observations first.'],
    lessons: [
      { title: 'Monte Carlo risk simulation', href: '/learn/simulations/monte-carlo-risk-simulation' },
      { title: 'Value at risk', href: '/learn/metrics/value-at-risk' },
      { title: 'Market regimes', href: '/learn/metrics/market-regimes' },
    ],
  },
  'risk-center': {
    purpose: 'Set pre-trade risk limits and kill switches, and watch current exposure against them.',
    steps: [
      'Set a limit for each measure: gross and net exposure, leverage, drawdown, daily loss and VaR.',
      'Check the utilization gauges. Amber means at least 80% of a limit is used; red means it is breached.',
      'Turn on a kill switch if you want new orders blocked for the workspace.',
    ],
    inputs: ['Limit values for each measure.', 'Kill switch on or off.'],
    controls: ['Limit fields: set the value each measure must stay within.', 'Kill switch: blocks new orders for the workspace until it is turned off.'],
    outputs: ['Current value, limit and utilization for each measure, and the pre-trade decision for each order.'],
    pitfalls: ['A limit only applies to orders placed after it is saved.', 'A kill switch stops new orders, not open positions. Close positions separately if needed.'],
    lessons: [
      { title: 'Risk management', href: '/learn/engine/risk-management' },
      { title: 'Leverage', href: '/learn/metrics/leverage' },
    ],
  },
  paper: {
    purpose: 'Trade on an Alpaca paper account: connect it, look at prices, and place simulated orders with the same checks as live trading.',
    steps: [
      'Connect an Alpaca paper account with its Key ID and Secret Key.',
      'Search for a symbol and open its chart.',
      'Fill in the order ticket and review the confirmation.',
      'Watch your open positions and orders until they fill or you cancel them.',
    ],
    inputs: ['Alpaca paper API keys (stored encrypted for this workspace).', 'A symbol, side, order type and quantity.'],
    controls: ['Connection form: checks the keys with Alpaca before saving them.', 'Order ticket: buy or sell, market or limit, quantity.', 'Working orders: can be cancelled from the orders list.'],
    outputs: ['Account equity, cash and buying power; positions with unrealized P&L; orders with their status.'],
    pitfalls: ['Only paper accounts are supported here. Keys from a live account are rejected.', 'Market orders fill at the next available price, which can differ from the last price you saw.'],
    lessons: [
      { title: 'Connecting Alpaca', href: '/learn/paper-trading/connecting-alpaca' },
      { title: 'Placing orders', href: '/learn/paper-trading/placing-orders' },
      { title: 'Account balances', href: '/learn/paper-trading/account-balances' },
    ],
  },
  portfolio: {
    purpose: 'Combine several strategies or positions into one portfolio and see how their risk and returns add up.',
    steps: [
      'Add the strategies or positions you want to combine.',
      'Set a weight for each, or let the workspace weight them equally.',
      'Review the combined return, drawdown and concentration.',
    ],
    inputs: ['Strategies or positions, and their weights.'],
    controls: ['Weights: set the share of each position; check that they add up to the portfolio you intend.'],
    outputs: ['Combined performance, gross weights and concentration (HHI).'],
    pitfalls: ['Weights that look balanced can still be concentrated in one position. Check the HHI figure.'],
    lessons: [
      { title: 'Portfolio management', href: '/learn/engine/portfolio-management' },
      { title: 'Concentration (HHI)', href: '/learn/metrics/concentration-hhi' },
    ],
  },
  data: {
    purpose: 'Bring price data into the workspace, check its quality, and record exactly which version each simulation used.',
    steps: [
      'Upload a CSV file or pick a provider.',
      'Map its columns and check the quality report.',
      'Save the dataset version and use it in a simulation.',
    ],
    inputs: ['A CSV file of bars (date, open, high, low, close, volume), or provider settings.'],
    controls: ['Upload wizard: maps the file\u2019s columns and previews the first rows.', 'Quality report: lists gaps, duplicates and suspicious values.'],
    outputs: ['A saved dataset version with a fingerprint, so every run can name the exact data it used.'],
    pitfalls: ['Check the time zone and the bar interval. A daily file loaded as hourly bars gives wrong results.', 'Fix gaps before you run a simulation; the quality report says where they are.'],
    lessons: [{ title: 'Market data processing', href: '/learn/engine/market-data-processing' }],
  },
  strategies: {
    purpose: 'Write and save strategy rules. Each save creates a version, so a result can always be traced to the exact code it used.',
    steps: [
      'Write the rules in the editor, or start from a built-in example.',
      'Set the parameters in the parameters panel.',
      'Save a version. Saving identical code and parameters returns the existing version.',
      'Use the saved version in a backtest.',
    ],
    inputs: ['Strategy code and its parameters.'],
    controls: ['Save version: creates a numbered version.', 'Version diff: shows the line and parameter differences between two versions.', 'Parameter sets: named sets of values for one saved version.'],
    outputs: ['A list of versions, diffs between them, and parameter sets you can reuse.'],
    pitfalls: ['Presets are stored against a saved version. Save a version first, then add a preset.', 'Changing a parameter creates a new version; the old results stay tied to the old version.'],
    lessons: [
      { title: 'Moving average crossover', href: '/learn/strategies/moving-average-crossover' },
      { title: 'Momentum', href: '/learn/strategies/momentum' },
    ],
  },
  backtests: {
    purpose: 'Replay a strategy over historical data with realistic costs, then read its results.',
    steps: [
      'Pick a dataset version and a strategy, with its parameters.',
      'Set costs, risk settings and the periods per year used to annualize results.',
      'Queue the run and wait for it to finish.',
      'Open the run to read its equity curve, drawdowns, statistics and benchmark comparison.',
    ],
    inputs: ['A dataset version.', 'A built-in strategy or a saved strategy version, with parameters.', 'Costs (commission, spread, slippage) and risk settings.'],
    controls: ['Queue run: starts the backtest as a background job.', 'Re-run and download: available on a finished run.', 'ⓘ icons: explain each metric and link to its lesson.'],
    outputs: ['Total and annualized return, Sharpe and Sortino ratios, drawdown, trade statistics, monthly returns and a benchmark comparison.'],
    pitfalls: ['Set periods per year to match your bar frequency; the annualized figures depend on it.', 'A strong in-sample result is not evidence on its own. Check the validation statistics in the Optimization Lab.'],
    lessons: [
      { title: 'Interpreting simulation output', href: '/learn/simulations/interpreting-simulation-output' },
      { title: 'Transaction costs', href: '/learn/metrics/transaction-costs' },
      { title: 'Simulation assumptions', href: '/learn/simulations/simulation-assumptions' },
    ],
  },
  optimization: {
    purpose: 'Test whether a strategy’s results survive out-of-sample testing, parameter sweeps and multiple-testing corrections.',
    steps: [
      'Choose a strategy and the parameter ranges to sweep.',
      'Run a walk-forward analysis: fit on one window, test on the next.',
      'Read the in-sample and out-of-sample results side by side.',
      'Check the overfitting statistics (PSR, DSR, PBO) before trusting the best parameters.',
    ],
    inputs: ['A strategy, parameter ranges, window lengths and the objective to optimize.'],
    controls: ['Run Walk-Forward Analysis: starts the job.', 'Window settings: set the length of each fit and test window.'],
    outputs: ['Walk-forward efficiency, per-window in-sample and out-of-sample Sharpe, and the overfitting statistics.'],
    pitfalls: ['The best in-sample parameters are usually the most overfitted. Judge them on the out-of-sample results.', 'Many trials make a good result more likely by chance. The deflated Sharpe and PBO correct for this.'],
    lessons: [
      { title: 'Walk-forward analysis', href: '/learn/simulations/walk-forward-analysis' },
      { title: 'Overfitting and validation', href: '/learn/simulations/overfitting-and-validation' },
      { title: 'Deflated Sharpe ratio', href: '/learn/metrics/deflated-sharpe-ratio' },
    ],
  },
  benchmark: {
    purpose: 'Measure how fast the engine runs on this machine, so you know what workloads are practical.',
    steps: ['Choose the benchmark workload.', 'Run it and wait for the timing results.', 'Compare the timings across machines or engine versions.'],
    inputs: ['The workload and its size.'],
    controls: ['Benchmark run: times the chosen workload.'],
    outputs: ['Timings and throughput for each workload, with the hardware they ran on.'],
    pitfalls: ['Run it on an idle machine. Background work changes the timings.'],
    lessons: [{ title: 'What is a trading engine', href: '/learn/engine/what-is-a-trading-engine' }],
  },
  registry: {
    purpose: 'The experiment registry: every run, its inputs and its results, kept as a tamper-evident record.',
    steps: ['Find the experiment you want in the list.', 'Open it to see its exact inputs, data version and results.', 'Use the same inputs to reproduce it.'],
    inputs: ['A search term or filter.'],
    controls: ['Selecting an experiment: shows its full record.'],
    outputs: ['A list of experiments and their full records.'],
    pitfalls: ['A record cannot be edited after it is written. Create a new run to change an experiment.'],
    lessons: [{ title: 'Simulation assumptions', href: '/learn/simulations/simulation-assumptions' }],
  },
  settings: {
    purpose: 'Connect market data sources and your Alpaca paper account, and set the defaults used across the workspace.',
    steps: ['Choose the market data source.', 'Add or replace your Alpaca paper keys.', 'Check the connection status before you trade.'],
    inputs: ['Data source settings.', 'Alpaca paper Key ID and Secret Key.'],
    controls: ['Settings fields: store the data source and broker keys for this workspace.', 'Connection status: shows whether the saved keys work.'],
    outputs: ['Connection status for each source and the active defaults.'],
    pitfalls: ['Keys are stored encrypted and are never shown again after saving. Replace them if you lose them.'],
    lessons: [{ title: 'Connecting Alpaca', href: '/learn/paper-trading/connecting-alpaca' }],
  },
};

/** Tabs a given role may see: admin-only workspaces are hidden from everyone else. */
export function workspacesFor(role: 'admin' | 'user' | undefined): StudioTab[] {
  return (Object.keys(WORKSPACES) as StudioTab[]).filter((t) => !WORKSPACES[t].adminOnly || role === 'admin');
}
