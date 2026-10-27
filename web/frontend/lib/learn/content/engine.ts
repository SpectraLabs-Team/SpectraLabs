import { lesson } from '../build';
import type { Lesson } from '../types';

const L = (l: Omit<Lesson, 'category'>) => lesson('engine', l);

export const engine: Lesson[] = [
  L({
    slug: 'what-is-a-trading-engine',
    title: 'What Is a Trading Engine?',
    level: 'Beginner',
    featured: true,
    summary: 'The software that turns strategy decisions into orders, fills, positions and accounting, consistently and without peeking into the future.',
    keywords: ['engine', 'backtester', 'architecture', 'rust', 'event driven'],
    blocks: [
      { type: 'p', text: 'A trading engine is the machinery between a strategy’s decision (“buy 100 AAPL”) and the account’s books. It receives market data, asks strategies what they want to do, manages orders through their lifecycle, simulates or routes executions, updates positions and cash, enforces risk limits, and records everything.' },
      { type: 'diagram', id: 'engine-pipeline', caption: 'The main components of the Spectra engine and how data flows between them.' },
      { type: 'h', text: 'The Spectra engine' },
      { type: 'list', items: [
        '**Event-driven backtester** (`spectra-engine`): feeds bars one timestamp at a time and fills orders against later bars only, so there is no look-ahead by construction.',
        '**Ledger** (`spectra-core`): fixed-point money, average-cost positions, fees, financing and a journal of every entry. Results are bit-identical across machines.',
        '**Cost models**: commission, slippage, spread, volume slippage and market impact.',
        '**Research layer** (`spectra-research`): sweeps, walk-forward analysis, PSR/DSR, PBO, trade analysis and the Survival Lab.',
        '**Risk engine** (`spectra-risk`): Monte Carlo VaR/CVaR and stress tests, used by the Risk & Stress Lab.',
        '**Risk Center**: pre-trade checks and kill switches applied before paper orders are sent.',
      ] },
      { type: 'p', text: 'The engine and research crates use only Rust’s standard library, so a dependency upgrade can never silently change a result. Every run records fingerprints (dataset, configuration, strategy and result hashes) so it can be reproduced exactly.' },
    ],
    related: ['engine/order-lifecycle', 'engine/market-data-processing', 'simulations/what-happens-when-a-simulation-runs'],
    platformLinks: [{ label: 'Read the architecture', href: '/architecture' }, { label: 'Open Backtests', href: '/studio?tab=backtests' }],
  }),

  L({
    slug: 'order-lifecycle',
    title: 'The Order Lifecycle',
    level: 'Beginner',
    featured: true,
    summary: 'Every order moves through a small set of states, from pending to filled, cancelled, rejected or expired.',
    keywords: ['order states', 'pending', 'filled', 'partially filled', 'cancelled', 'rejected', 'expired'],
    blocks: [
      { type: 'diagram', id: 'order-lifecycle', caption: 'Order states in the Spectra backtester.' },
      { type: 'table', head: ['State', 'Meaning'], rows: [
        ['Pending', 'Accepted and waiting to fill (at the earliest on the next bar)'],
        ['Partially filled', 'Some quantity filled; the rest is still open (participation cap)'],
        ['Filled', 'Completely filled'],
        ['Cancelled', 'Cancelled by the strategy before it filled completely'],
        ['Rejected', 'Malformed, or the ledger refused the fill (for example the leverage limit)'],
        ['Expired', 'Its time in force ran out (a Day order not completely filled on its first eligible bar, or a good-till-date order past its date)'],
      ] },
      { type: 'p', text: 'Filled, cancelled, rejected and expired are **terminal**: nothing more can happen to the order. Every order and its final state are written to `orders.csv` with the run, and the summary counts orders by state.' },
      { type: 'callout', tone: 'info', title: 'Paper trading', text: 'Alpaca uses a larger set of statuses (new, accepted, pending_new, partially_filled, filled, canceled, expired, rejected and more). The Paper Trading lesson on order statuses maps them.' },
    ],
    related: ['engine/order-matching', 'engine/order-execution-engine', 'paper-trading/order-statuses', 'metrics/orders-and-fills'],
  }),

  L({
    slug: 'order-matching',
    title: 'Order Matching',
    level: 'Intermediate',
    summary: 'How an exchange pairs buy and sell orders using price-time priority, and how a bar-based backtester approximates it.',
    keywords: ['matching', 'price time priority', 'fifo', 'crossing'],
    blocks: [
      { type: 'p', text: 'Exchanges match orders by **price-time priority**: the best price trades first, and among orders at the same price, the one that arrived first trades first. A new buy order at or above the best ask is *marketable* and matches immediately; one below it rests on the book.' },
      { type: 'list', ordered: true, items: ['A buy limit at $10.02 arrives. The best ask is $10.01 for 300 shares.', 'It matches 300 shares at $10.01 (the resting order’s price).', 'If it wanted 500, the remaining 200 rest as the new best bid at $10.02.'] },
      { type: 'h', text: 'Matching in a bar-based backtest' },
      { type: 'p', text: 'With only open, high, low, close and volume per bar, the engine cannot see the queue. It uses conservative rules instead: market orders fill at the next open (plus slippage); a limit buy fills at the open if the open is already through the limit, otherwise at the limit if the low touches it. It never fills at a price the bar did not trade.' },
      { type: 'p', text: 'Open orders are kept in a **price-indexed book**, so each step only visits orders that could fill on that bar. This avoids the quadratic slowdown of re-scanning every order ever placed.' },
      { type: 'callout', tone: 'warning', text: 'Touching a limit is not a guarantee of a fill in real markets: you may be behind other orders in the queue. Treat backtest limit-order fill rates as optimistic.' },
    ],
    related: ['engine/order-book-fundamentals', 'fundamentals/market-and-limit-orders', 'simulations/how-simulated-trades-execute'],
  }),

  L({
    slug: 'order-book-fundamentals',
    title: 'Order Book Fundamentals',
    level: 'Intermediate',
    summary: 'The order book lists resting buy and sell orders by price. Its shape tells you about liquidity and likely price impact.',
    keywords: ['order book', 'level 2', 'bids', 'asks', 'depth', 'top of book'],
    blocks: [
      { type: 'diagram', id: 'order-book' },
      { type: 'list', items: ['**Bids** (buy orders) are sorted from highest price down; the top bid is the best bid.', '**Asks** (sell orders) are sorted from lowest price up; the top ask is the best ask.', 'The **spread** is the gap between them; the **mid** is halfway.', '**Depth** is the quantity at each level.'] },
      { type: 'p', text: 'Marketable orders remove liquidity from the book; resting limit orders add it. Imbalance between total bid and ask size near the top is often a short-term signal of direction.' },
      { type: 'p', text: 'Spectra’s backtester keeps its own book of the strategy’s open orders, indexed by price, but not a book of the whole market: market liquidity is approximated from bar volume.' },
    ],
    related: ['fundamentals/market-depth', 'engine/order-matching', 'fundamentals/bid-ask-spread'],
  }),

  L({
    slug: 'order-execution-engine',
    title: 'The Order Execution Engine',
    level: 'Intermediate',
    summary: 'The part of the engine that decides when and at what price an order fills, applying costs and participation limits.',
    keywords: ['execution engine', 'fill rules', 'delay', 'participation', 'costs'],
    blocks: [
      { type: 'p', text: 'At each timestamp T, the Spectra backtester runs the execution step **before** the strategy sees the bar:' },
      { type: 'list', ordered: true, items: [
        'Fill orders submitted at earlier timestamps against the bars of T, using the fill rules for their type.',
        'Apply costs: commission on every fill; slippage, half spread and volume slippage on market and stop fills.',
        'Respect the participation cap: an order takes at most `max_participation` of the bar’s volume; the rest stays open.',
        'Respect `delay_bars`: orders can be made to wait extra bars before they may fill, to model latency.',
        'Hand every fill to the ledger, which may reject it (cash, shorting or leverage rules).',
        'Mark positions at T’s close and record an equity point; then call the strategy’s `on_fill` and `on_bar`.',
      ] },
      { type: 'callout', tone: 'tip', title: 'Why this order matters', text: 'Because the strategy is called after fills and marking, an order it places while handling T can only fill on a later bar. That is the engine’s guarantee against look-ahead bias.' },
    ],
    related: ['engine/order-lifecycle', 'simulations/how-simulated-trades-execute', 'metrics/slippage'],
    platformLinks: [{ label: 'Open Backtests', href: '/studio?tab=backtests' }],
  }),

  L({
    slug: 'trade-management',
    title: 'The Trade Management Engine',
    level: 'Intermediate',
    summary: 'How fills are grouped into trades (round trips), and how stops, targets and exits are managed.',
    keywords: ['trade management', 'round trip', 'stop loss', 'take profit', 'exits'],
    blocks: [
      { type: 'p', text: 'Fills are low-level events. **Trade management** groups them into trades and manages each trade’s exit. In Spectra a trade is one position lifecycle in one instrument: it opens when the position leaves zero and ends when it returns to zero or flips sign.' },
      { type: 'h', text: 'Exit tools' },
      { type: 'list', items: ['**Stop-loss**: a sell stop below a long entry (or buy stop above a short) to cap the loss.', '**Take-profit**: a limit order at a target price.', '**Signal exits**: the strategy’s own rules close the position (an SMA crossover reversing, a z-score returning to normal).', '**Time exits**: close after N bars.'] },
      { type: 'p', text: 'In the Strategy Studio rule language you can place `stop_sell`, `limit_sell` and `cancel_order` orders, or simply set a target with `target_shares(0)`. After the run, the trade statistics (win rate, payoff ratio, holding period) summarize how the trades were managed.' },
    ],
    related: ['metrics/number-of-trades', 'metrics/win-rate', 'engine/position-management', 'strategies/moving-average-crossover'],
    platformLinks: [{ label: 'Open Strategy Studio', href: '/studio?tab=strategies' }],
  }),

  L({
    slug: 'risk-management',
    title: 'Risk Management',
    level: 'Intermediate',
    featured: true,
    summary: 'Limits that stop a strategy from taking more risk than intended: leverage, position size, concentration, drawdown, daily loss and VaR.',
    keywords: ['risk management', 'limits', 'kill switch', 'pre trade', 'risk center'],
    blocks: [
      { type: 'p', text: 'Risk management decides how much can be lost and enforces it mechanically, before emotions or bugs do damage. Spectra applies it at two levels.' },
      { type: 'h', text: 'In backtests: the ledger' },
      { type: 'p', text: 'The ledger refuses fills that would break the run’s rules: a gross leverage limit (`max_gross_leverage`, 1.0× by default), no shorting unless allowed, no negative cash unless margin is allowed. Refused fills mark the order Rejected.' },
      { type: 'h', text: 'In paper trading: the Risk Center' },
      { type: 'table', head: ['Limit', 'What it caps'], rows: [
        ['Max gross / net exposure', 'Total and directional position value'],
        ['Max position size', 'One position’s value'],
        ['Max concentration', 'One position as a share of equity'],
        ['Max leverage', 'Gross exposure ÷ equity'],
        ['Max drawdown', 'Fall from peak equity'],
        ['Max daily loss', 'Loss since the start of the day'],
        ['Max VaR', 'Value at Risk as a share of equity'],
      ] },
      { type: 'p', text: 'Every proposed order gets a pre-trade verdict: **approved**, **reduced** (if size reduction is enabled, the order is shrunk to fit) or **rejected**. **Kill switches** halt trading for a workspace or a strategy until cleared. The Overview shows the current risk posture and recent decisions.' },
    ],
    related: ['metrics/leverage', 'metrics/maximum-drawdown', 'metrics/value-at-risk', 'engine/portfolio-management'],
    platformLinks: [{ label: 'Open Risk Center', href: '/studio?tab=risk-center' }],
  }),

  L({
    slug: 'position-management',
    title: 'Position Management',
    level: 'Beginner',
    summary: 'How the engine tracks quantity, average cost and P&L for each position as fills arrive.',
    keywords: ['positions', 'average cost', 'mark to market', 'flip'],
    blocks: [
      { type: 'p', text: 'A position is the signed quantity you hold in one instrument: positive for long, negative for short. For each position the ledger keeps the quantity, the total cost, the realized P&L and the fees.' },
      { type: 'h', text: 'Average cost accounting' },
      { type: 'list', items: ['Adding to a position updates the average cost: (old cost + new quantity × price) ÷ new quantity.', 'Reducing it realizes (price − average cost) × quantity reduced (reversed for shorts); the average of what remains is unchanged.', 'A fill larger than the position closes it and opens one on the other side at the fill price.'] },
      { type: 'formula', tex: String.raw`\bar{c}_{\text{new}} = \frac{q_{\text{old}}\bar{c}_{\text{old}} + q_{\text{fill}}\,p_{\text{fill}}}{q_{\text{old}} + q_{\text{fill}}}` },
      { type: 'p', text: 'At every bar close positions are **marked to market**: market value = quantity × close, unrealized P&L = market value − cost. Corporate actions (splits and dividends) are applied to positions on their dates.' },
    ],
    related: ['metrics/realized-unrealized-pnl', 'engine/portfolio-management', 'fundamentals/buy-and-sell-orders'],
  }),

  L({
    slug: 'portfolio-management',
    title: 'Portfolio Management',
    level: 'Intermediate',
    summary: 'Combining positions and strategies into one account: allocation, rebalancing, exposure and concentration.',
    keywords: ['portfolio', 'allocation', 'rebalancing', 'multi strategy', 'concentration', 'hhi'],
    blocks: [
      { type: 'p', text: 'Portfolio management decides how capital is split across instruments and strategies and keeps the split on target as prices move.' },
      { type: 'h', text: 'In Spectra' },
      { type: 'list', items: [
        '**Target weights**: the equal-weight strategy sets weights and the engine’s rebalancer turns them into orders, on a schedule or when weights drift too far.',
        '**Multi-strategy runs**: several strategies share one account. Each gets a share of the starting cash and keeps its own books; the account is their sum, and each strategy’s contribution is reported.',
        '**Portfolio Studio**: load a portfolio of positions to analyse its value, weights and concentration (Herfindahl index), and feed it into the Risk & Stress Lab.',
      ] },
      { type: 'formula', tex: String.raw`\text{HHI} = \sum_i w_i^2`, caption: 'Herfindahl concentration of gross weights: 1/N for N equal positions, 1 for a single position.' },
    ],
    related: ['engine/risk-management', 'strategies/buy-and-hold-and-equal-weight', 'metrics/exposure'],
    platformLinks: [{ label: 'Open Portfolio Studio', href: '/studio?tab=portfolio' }],
  }),

  L({
    slug: 'market-data-processing',
    title: 'Market Data Processing',
    level: 'Intermediate',
    summary: 'How price data gets into the engine: ingestion, quality checks, content-addressed versions and corporate actions.',
    keywords: ['market data', 'csv', 'ingest', 'data quality', 'dataset version', 'corporate actions'],
    blocks: [
      { type: 'p', text: 'Results can only be as good as the data. Spectra treats data as versioned, verifiable input.' },
      { type: 'list', ordered: true, items: [
        '**Upload** bars as CSV in the Data tab (timestamp, symbol, open, high, low, close, volume).',
        '**Validation and quality report**: prices are parsed exactly as fixed-point decimals; by default any invalid row aborts the load (skipping bad rows is opt-in and every skipped row is counted). Duplicate rows are detected, and the report flags gaps longer than 5× the median interval, outliers and stale prices.',
        '**Content-addressed storage**: each dataset version is identified by the SHA-256 of its contents, so a run records exactly which data it used.',
        '**Alignment**: datasets whose instruments have different median bar intervals are refused, because mixing them would leak a bar’s close to strategies trading inside it.',
        '**Corporate actions**: splits and dividends can be applied so positions and cash stay correct.',
      ] },
      { type: 'p', text: 'Paper trading uses a different source: Alpaca’s market data API (the free IEX feed) for charts and quotes.' },
    ],
    related: ['engine/what-is-a-trading-engine', 'simulations/simulation-assumptions', 'paper-trading/reading-candlestick-charts'],
    platformLinks: [{ label: 'Open Data', href: '/studio?tab=data' }],
  }),
];
