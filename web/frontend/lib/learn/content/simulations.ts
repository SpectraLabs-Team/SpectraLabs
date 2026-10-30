import { lesson } from '../build';
import type { Lesson } from '../types';

const L = (l: Omit<Lesson, 'category'>) => lesson('simulations', l);
const BT = { label: 'Open Backtests', href: '/studio?tab=backtests' };

export const simulations: Lesson[] = [
  L({
    slug: 'what-is-a-trading-simulation',
    title: 'What Is a Trading Simulation?',
    level: 'Beginner',
    featured: true,
    summary: 'A simulation replays a strategy’s rules against market data to see what would have happened, without risking money.',
    keywords: ['simulation', 'backtest', 'introduction'],
    blocks: [
      { type: 'p', text: 'A trading simulation runs a strategy’s rules against market data and an imitation of the market’s execution, then records every order, fill and change in the account. The output is a full history of what the strategy would have done and how the account would have moved.' },
      { type: 'p', text: 'Spectra has three kinds of simulation:' },
      { type: 'list', items: [
        '**Backtests** replay historical bars through the event-driven engine (the Backtests tab).',
        '**Monte Carlo risk simulations** draw thousands of possible future outcomes for the current portfolio to estimate VaR and expected shortfall (the Risk & Stress Lab).',
        '**Paper trading** executes orders in Alpaca’s simulated account against live market prices (the Paper Trading tab).',
      ] },
      { type: 'diagram', id: 'simulation-loop', caption: 'A backtest loops over time: fill earlier orders, mark the books, ask the strategy, queue its orders.' },
      { type: 'callout', tone: 'warning', text: 'A simulation is a model. It can tell you that an idea is bad far more reliably than it can tell you that an idea is good.' },
    ],
    related: ['simulations/why-simulations-are-useful', 'simulations/simulation-vs-backtesting-vs-paper-trading', 'simulations/what-happens-when-a-simulation-runs'],
    platformLinks: [BT],
  }),

  L({
    slug: 'why-simulations-are-useful',
    title: 'Why Simulations Are Useful',
    level: 'Beginner',
    summary: 'Simulations let you reject bad ideas cheaply, size risk before it is real, and understand how a strategy behaves in different markets.',
    keywords: ['benefits', 'testing', 'research'],
    blocks: [
      { type: 'list', items: [
        '**Cheap failure**: most trading ideas do not work. Finding that out in a backtest costs seconds, not money.',
        '**Behaviour, not just results**: you see drawdowns, losing streaks, turnover and costs before you live through them.',
        '**Sensitivity**: change one assumption (costs, delay, parameters) and see how fragile the result is. The Survival Lab runs these tests systematically.',
        '**Risk sizing**: Monte Carlo simulations estimate how much a portfolio could lose over a horizon at a given confidence.',
        '**Reproducibility**: every Spectra run records its data, configuration and code fingerprints, so a result can be reproduced and audited in the Experiment Registry.',
      ] },
      { type: 'h', text: 'What simulations cannot do' },
      { type: 'list', items: ['Predict regimes that are not in the data.', 'Model your own market impact perfectly.', 'Account for every real-world friction (outages, halts, borrow recalls).', 'Undo the bias of having tested many ideas on the same data.'] },
    ],
    related: ['simulations/simulation-assumptions', 'simulations/overfitting-and-validation', 'simulations/what-is-a-trading-simulation'],
    platformLinks: [BT, { label: 'Open Experiment Registry', href: '/studio?tab=registry' }],
  }),

  L({
    slug: 'how-simulated-trades-execute',
    title: 'How Simulated Trades Are Executed',
    level: 'Intermediate',
    summary: 'Orders placed on one bar fill on a later bar, at prices the bar actually traded, after costs.',
    keywords: ['fill model', 'simulated execution', 'next bar', 'look ahead', 'costs'],
    blocks: [
      { type: 'p', text: 'The Spectra backtester follows a strict timing contract so results never use information the strategy could not have had.' },
      { type: 'list', ordered: true, items: [
        'At bar T, the strategy sees history up to and including T’s close and may place orders.',
        'Those orders cannot fill until the next timestamp with a bar for the instrument (later if `delay_bars` is set).',
        'Market orders fill at that bar’s **open**, moved against you by slippage and half the spread.',
        'Limit orders fill at the open if it is already through the limit, else at the limit if the bar’s range reaches it.',
        'Commission is charged; the ledger checks cash, shorting and leverage rules and may reject the fill.',
      ] },
      { type: 'callout', tone: 'info', title: 'Why the next open?', text: 'If a strategy computed a signal from bar T’s close and filled at that same close, it would be trading at a price that had already printed before its decision: look-ahead bias. Filling at the next open is the honest default.' },
      { type: 'p', text: 'The fill rules for each order type are in the [order types lesson](/learn/fundamentals/market-and-limit-orders) and the cost formulas in the [slippage metric lesson](/learn/metrics/slippage).' },
    ],
    related: ['engine/order-execution-engine', 'simulations/simulation-assumptions', 'metrics/slippage'],
    platformLinks: [BT],
  }),

  L({
    slug: 'historical-vs-real-time-simulations',
    title: 'Historical vs Real-Time Simulations',
    level: 'Beginner',
    summary: 'Historical simulations replay the past quickly and reproducibly; real-time simulations run forward on live data and cannot be rerun.',
    keywords: ['historical', 'real time', 'live', 'forward test', 'out of sample'],
    blocks: [
      { type: 'table', head: ['', 'Historical (backtest)', 'Real-time (paper trading)'], rows: [
        ['Data', 'Stored bars you uploaded', 'Live market data'],
        ['Speed', 'Years in seconds', 'One day per day'],
        ['Reproducible', 'Exactly (same fingerprints)', 'No: the market only happens once'],
        ['Execution', 'Engine fill rules', 'Alpaca’s simulator against live quotes'],
        ['Look-ahead risk', 'Possible in data and design', 'None: the future is unknown'],
        ['Best for', 'Research, parameter testing', 'Out-of-sample validation, practice'],
      ] },
      { type: 'p', text: 'A strategy should pass both: historical simulation to show the idea has merit over many conditions, then real-time paper trading to show it still works on data it has never seen. The gap between the two is itself informative; the Backtest-to-Paper Gap panel on a backtest’s results compares them.' },
    ],
    related: ['simulations/simulation-vs-backtesting-vs-paper-trading', 'paper-trading/what-is-paper-trading', 'simulations/walk-forward-analysis'],
    platformLinks: [BT, { label: 'Open Paper Trading', href: '/studio?tab=paper' }],
  }),

  L({
    slug: 'simulation-assumptions',
    title: 'How Simulation Assumptions Affect Results',
    level: 'Intermediate',
    featured: true,
    summary: 'Costs, fills, data quality, annualization and leverage settings can each turn a winning backtest into a losing one.',
    keywords: ['assumptions', 'cost assumptions', 'sensitivity', 'bias', 'survivorship', 'look ahead'],
    blocks: [
      { type: 'p', text: 'Every simulation embeds assumptions. The important ones, and where you set them in a Spectra backtest:' },
      { type: 'table', head: ['Assumption', 'Setting', 'Effect if too optimistic'], rows: [
        ['Commission', '`commission_bps`, `commission_per_unit`, `commission_min`', 'Overstates net returns, especially for frequent trading'],
        ['Slippage & spread', '`slippage_bps`, `spread_bps`, `volume_slippage_bps`', 'Overstates every market-order strategy'],
        ['Liquidity', '`max_participation`', 'Assumes you can trade any size'],
        ['Latency', '`delay_bars`', 'Assumes instant reaction to signals'],
        ['Leverage & shorting', '`max_gross_leverage`, `allow_short`, `allow_negative_cash`', 'Allows exposure you could not get'],
        ['Carry', '`borrow_fee_rate`, `margin_interest_rate`', 'Ignores the cost of shorts and margin'],
        ['Annualization', '`periods_per_year`', 'Wrong Sharpe, volatility and CAGR'],
      ] },
      { type: 'h', text: 'Biases in the data and process' },
      { type: 'list', items: ['**Survivorship bias**: testing only on stocks that exist today ignores those that went bankrupt.', '**Look-ahead bias**: using information before it was available (the engine prevents it in execution; you must prevent it in data, such as restated fundamentals).', '**Selection bias**: trying many variants and keeping the best. Use the Deflated Sharpe Ratio and PBO.'] },
      { type: 'callout', tone: 'tip', text: 'Run the Strategy Survival Lab on any result you like. It re-runs the strategy with extra costs, delays, neighbouring parameters, sub-periods and bootstraps, and shows which assumption breaks it first.' },
    ],
    related: ['metrics/slippage', 'metrics/transaction-costs', 'simulations/overfitting-and-validation', 'metrics/deflated-sharpe-ratio'],
    platformLinks: [BT],
  }),

  L({
    slug: 'what-happens-when-a-simulation-runs',
    title: 'What Happens When a Simulation Runs',
    level: 'Intermediate',
    summary: 'From pressing Run to reading results: queueing, the event loop, accounting, metrics, artifacts and the registry.',
    keywords: ['run', 'job', 'worker', 'queue', 'artifacts', 'fingerprint'],
    blocks: [
      { type: 'list', ordered: true, items: [
        '**Validate**: the form checks the strategy, its parameters and the configuration against the engine’s schema.',
        '**Queue**: the backtest becomes a job in Postgres; a worker picks it up. You can follow it, or cancel it, from the run list.',
        '**Load**: the worker loads the exact dataset version (by its hash) and builds the strategy.',
        '**Event loop**: bar by bar, fill earlier orders, mark positions, record equity, call the strategy, queue its new orders.',
        '**Account**: every fill, fee, interest charge, dividend and split goes through the ledger, which checks its invariants.',
        '**Summarize**: metrics, drawdown and tail statistics, trade statistics, exposure, attribution and the benchmark comparison.',
        '**Store**: CSV artifacts (equity, orders, fills, returns), chart series and the summary; fingerprints of data, configuration, strategy and result.',
        '**Register**: the run is recorded in the hash-chained experiment registry, linked to its experiment.',
      ] },
      { type: 'diagram', id: 'simulation-loop' },
      { type: 'p', text: 'Because the engine is deterministic and the inputs are fingerprinted, rerunning the same request produces the same `result_sha256`.' },
    ],
    related: ['simulations/interpreting-simulation-output', 'engine/order-execution-engine', 'engine/what-is-a-trading-engine'],
    platformLinks: [BT, { label: 'Open Experiment Registry', href: '/studio?tab=registry' }],
  }),

  L({
    slug: 'interpreting-simulation-output',
    title: 'How to Interpret Simulation Output',
    level: 'Intermediate',
    featured: true,
    summary: 'A step-by-step way to read a backtest’s results, from the headline numbers to the details that tell you whether to trust them.',
    keywords: ['results', 'interpretation', 'checklist', 'metrics', 'dashboard'],
    blocks: [
      { type: 'p', text: 'Every metric on the results page has an ⓘ icon: hover or tap it for a short definition and a link to its full lesson. Read the results in this order:' },
      { type: 'list', ordered: true, items: [
        '**Is it valid?** Check the number of periods and trades. Fewer than ~30 trades or a few months of data makes every statistic unreliable.',
        '**Did it make money after costs?** Total return, CAGR and final equity, then fees and financing.',
        '**At what risk?** Maximum drawdown, volatility, VaR and the drawdown chart.',
        '**Was the risk worth it?** Sharpe, Sortino and Calmar; compare with the benchmark card.',
        '**How was it earned?** Trade statistics (win rate, payoff, expectancy), monthly returns and per-instrument P&L. Is it a few big trades or many small ones? One instrument or many?',
        '**Is it robust?** Run the Survival Lab and look at the Deflated Sharpe Ratio in the Optimization Lab if you chose parameters by searching.',
      ] },
      { type: 'interactive', id: 'equity-metrics' },
      { type: 'callout', tone: 'warning', title: 'Red flags', text: 'A Sharpe above 3, a nearly straight equity curve, a profit factor above 3 from few trades, results that collapse with a little more slippage, or one instrument producing most of the profit.' },
    ],
    related: ['metrics/total-return', 'metrics/sharpe-ratio', 'metrics/maximum-drawdown', 'metrics/win-rate', 'simulations/overfitting-and-validation'],
    platformLinks: [BT],
  }),

  L({
    slug: 'simulation-vs-backtesting-vs-paper-trading',
    title: 'Simulation vs Backtesting vs Paper Trading',
    level: 'Beginner',
    featured: true,
    summary: 'Simulation is the umbrella term; backtesting replays history; paper trading runs forward in real time with simulated money.',
    keywords: ['difference', 'comparison', 'backtest', 'paper trade', 'forward test'],
    blocks: [
      { type: 'table', head: ['', 'Simulation (general)', 'Backtesting', 'Paper trading'], rows: [
        ['What it is', 'Any model of trading outcomes', 'Replaying rules on past data', 'Trading live markets with simulated funds'],
        ['Data', 'Historical, generated or live', 'Historical bars', 'Live quotes and bars'],
        ['Time', 'Any', 'Past, compressed', 'Present, real time'],
        ['Money', 'None', 'None', 'Simulated balance (no real money)'],
        ['In Spectra', 'Monte Carlo in the Risk & Stress Lab', 'Backtests tab', 'Paper Trading tab via Alpaca'],
      ] },
      { type: 'p', text: 'A sensible path for a new idea: backtest it, stress it (Survival Lab, Optimization Lab), then paper trade it for long enough that its live statistics mean something (see Minimum Track Record Length).' },
      { type: 'callout', tone: 'info', text: 'Paper trading uses simulated funds and does not guarantee real-market performance: real orders can face different fills, partial fills, queue position and market impact.' },
    ],
    related: ['simulations/what-is-a-trading-simulation', 'paper-trading/what-is-paper-trading', 'metrics/minimum-track-record-length'],
    platformLinks: [BT, { label: 'Open Paper Trading', href: '/studio?tab=paper' }],
  }),

  L({
    slug: 'monte-carlo-risk-simulation',
    title: 'Monte Carlo Risk Simulation',
    level: 'Intermediate',
    summary: 'The Risk & Stress Lab draws thousands of possible horizon outcomes for the portfolio and reads VaR and expected shortfall off the distribution.',
    keywords: ['monte carlo', 'gbm', 'risk lab', 'paths', 'stress test'],
    blocks: [
      { type: 'p', text: 'Instead of replaying the past, a Monte Carlo simulation samples many possible futures from a model. Each path gives one loss over the horizon; with enough paths, the histogram of losses approximates the model’s loss distribution.' },
      { type: 'formula', tex: String.raw`L = V - V\exp\!\left(\left(\mu - \tfrac{\sigma^2}{2}\right)T + \sigma\sqrt{T}\,Z\right), \quad Z \sim N(0,1)` },
      { type: 'list', items: ['\\(V\\) is the portfolio value, \\(\\sigma\\) its volatility, \\(T\\) the horizon in years, \\(\\mu\\) the drift (0 by default).', 'You choose the confidence level, the number of simulations and the horizon (1 day to 1 year).', 'Results are reproducible: the random number generator is seeded, and parallel runs give the same answer regardless of thread count.'] },
      { type: 'p', text: 'The lab compares the simulated VaR with the exact closed-form value of the same model, so you can see the Monte Carlo error, and shows an EVT tail estimate when the tail looks heavier than the model. Stress tests apply historical-style shocks (such as a market crash) to the portfolio directly.' },
      { type: 'callout', tone: 'warning', text: 'The engine’s model treats the portfolio as one aggregate position, so it ignores correlations between assets. It is a defensible first model, not a production market-risk model.' },
    ],
    related: ['metrics/value-at-risk', 'metrics/expected-shortfall', 'metrics/evt-tail-risk'],
    platformLinks: [{ label: 'Open Risk & Stress Lab', href: '/studio?tab=risk' }],
  }),

  L({
    slug: 'stress-testing',
    title: 'Stress Testing',
    level: 'Intermediate',
    summary: 'Apply a named shock (a market crash, a 2008-sized fall) to the portfolio and see the immediate loss and the risk that remains.',
    keywords: ['stress test', 'scenario', 'crash', 'lehman', 'covid', 'shock', 'post var'],
    blocks: [
      { type: 'p', text: 'A stress test asks \u201cwhat if?\u201d directly instead of estimating probabilities. Every price is multiplied by the scenario\u2019s price factor and, for most scenarios, every asset\u2019s volatility by a multiplier. The Stress panel reports the **P&L** of the shock, the **value after** it, and the **Post VaR**: the engine\u2019s 99% Monte Carlo VaR of the shocked portfolio.' },
      { type: 'table', head: ['Scenario', 'Prices', 'Volatility', 'Source'], rows: [
        ['COVID crash', '−35%', '×2', 'Sized after Feb–Mar 2020'],
        ['2008 Lehman', '−45%', '×2.5', 'Sized after Sep 2008–Mar 2009'],
        ['Tech selloff', '−20%', '×1.5', 'Hypothetical'],
        ['Market crash', '−30%', 'unchanged', 'Engine built-in scenario'],
        ['Mild correction', '−10%', 'unchanged', 'Engine built-in scenario'],
        ['Rates spike', '−12%', '×1.3', 'Duration-led repricing'],
      ] },
      { type: 'formula', tex: String.raw`\text{P\&L} = V\times(\text{price factor} - 1), \qquad V_{\text{after}} = V \times \text{price factor}` },
      { type: 'p', text: 'Example: a $1,000,000 long portfolio under **2008 Lehman** (price factor 0.55) is worth $550,000 afterwards, a P&L of −$450,000. Its Post VaR is then computed with volatilities 2.5 times higher, so the remaining risk is larger as a share of the smaller book. Short positions gain from the same price shock.' },
      { type: 'callout', tone: 'warning', text: 'Scenarios apply one uniform shock to every position: they are hypothetical what-ifs sized after historical episodes, not replays. Real crises hit sectors differently and change correlations.' },
    ],
    related: ['simulations/monte-carlo-risk-simulation', 'metrics/value-at-risk', 'metrics/maximum-drawdown'],
    platformLinks: [{ label: 'Open Risk & Stress Lab', href: '/studio?tab=risk' }],
  }),

  L({
    slug: 'walk-forward-analysis',
    title: 'Walk-Forward Analysis & In/Out-of-Sample Sharpe',
    level: 'Advanced',
    summary: 'Optimize on one window, test on the next, roll forward and repeat: the honest way to evaluate a parameter search.',
    keywords: ['walk forward', 'in sample', 'out of sample', 'oos', 'train test', 'rolling', 'anchored'],
    blocks: [
      { type: 'p', text: 'Walk-forward analysis splits history into consecutive **train** and **test** windows. Parameters are chosen on each train window and evaluated, unchanged, on the following test window. Stitching the test windows together gives an out-of-sample record of the whole procedure.' },
      { type: 'list', items: ['**Rolling** windows keep a fixed train length; **anchored** windows grow from the start of the data.', '**In-sample mean Sharpe**: average Sharpe of the chosen parameters on their train windows.', '**Out-of-sample mean Sharpe**: average Sharpe of the same parameters on the test windows that followed.'] },
      { type: 'p', text: 'A large drop from in-sample to out-of-sample Sharpe is the signature of overfitting: the parameters fitted noise in the train window. A small drop suggests the selection procedure generalizes.' },
      { type: 'callout', tone: 'tip', text: 'The holdout vault in the Optimization Lab keeps a final slice of data untouched, with a limited evaluation budget, so you can test the finished strategy once on data the search never saw.' },
    ],
    related: ['simulations/overfitting-and-validation', 'metrics/probability-of-backtest-overfitting', 'metrics/sharpe-ratio'],
    platformLinks: [{ label: 'Open Optimization Lab', href: '/studio?tab=optimization' }],
  }),

  L({
    slug: 'overfitting-and-validation',
    title: 'Overfitting and Validation',
    level: 'Advanced',
    summary: 'Why the best backtest from a search is usually too good, and the tools Spectra gives you to measure by how much.',
    keywords: ['overfitting', 'curve fitting', 'data snooping', 'validation', 'dsr', 'pbo'],
    blocks: [
      { type: 'p', text: '**Overfitting** is tuning a strategy so closely to past data that it captures noise instead of a repeatable pattern. The more parameters and variants you try, the better the best one will look by chance.' },
      { type: 'h', text: 'Spectra’s validation toolkit' },
      { type: 'list', items: [
        '[Deflated Sharpe Ratio](/learn/metrics/deflated-sharpe-ratio): is the best Sharpe higher than the best of N worthless trials?',
        '[Probability of Backtest Overfitting](/learn/metrics/probability-of-backtest-overfitting): does the in-sample winner hold up out of sample across many splits?',
        '[Minimum Track Record Length](/learn/metrics/minimum-track-record-length): is there enough data for the Sharpe to be significant?',
        '[Walk-forward analysis](/learn/simulations/walk-forward-analysis): how does the selection procedure perform on unseen windows?',
        '**Survival Lab**: neighbouring-parameter stability, sub-periods, regimes, costs and bootstraps.',
        '**Holdout vault**: one final test on untouched data.',
      ] },
      { type: 'callout', tone: 'warning', text: 'Every look at the holdout data, and every idea you discard after seeing results, is a trial. Record them; the experiment registry exists so the trial count is honest.' },
    ],
    related: ['metrics/deflated-sharpe-ratio', 'metrics/probability-of-backtest-overfitting', 'simulations/walk-forward-analysis', 'simulations/simulation-assumptions'],
    platformLinks: [{ label: 'Open Optimization Lab', href: '/studio?tab=optimization' }, { label: 'Open Experiment Registry', href: '/studio?tab=registry' }],
  }),
];
