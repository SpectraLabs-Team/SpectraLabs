import { lesson } from '../build';
import type { Lesson } from '../types';

const L = (l: Omit<Lesson, 'category'>) => lesson('paper-trading', l);
const PAPER = { label: 'Open Paper Trading', href: '/studio?tab=paper' };

const DISCLAIMER =
  'Paper trading uses **simulated funds**. Fills are simulated against live quotes and do not account for everything that happens to real orders (queue position, market impact, some partial fills), so paper results do not guarantee real-market performance.';

export const paper: Lesson[] = [
  L({
    slug: 'what-is-paper-trading',
    title: 'What Is Paper Trading?',
    level: 'Beginner',
    featured: true,
    summary: 'Trading live markets with a simulated account: real prices, real order types, no real money.',
    keywords: ['paper trading', 'demo account', 'simulated', 'practice', 'forward test'],
    blocks: [
      { type: 'p', text: 'Paper trading lets you place orders in a simulated brokerage account that tracks real market prices. You learn the mechanics (order tickets, statuses, positions, buying power) and you can forward-test a strategy on data it has never seen, without risking capital.' },
      { type: 'callout', tone: 'warning', title: 'Simulated funds', text: DISCLAIMER },
      { type: 'h', text: 'What it is good for' },
      { type: 'list', items: ['Learning how orders behave before using real money.', 'Validating a backtested strategy out of sample, in real time.', 'Measuring the gap between backtest and live behaviour (the Backtest-to-Paper Gap panel on a backtest’s results).', 'Testing risk limits and kill switches in the Risk Center.'] },
      { type: 'h', text: 'What it is not' },
      { type: 'list', items: ['Not proof of profitability: real fills can be worse.', 'Not emotionally equivalent: losing simulated money is easy to shrug off.', 'Not instant evidence: a few weeks of paper results are statistically weak (see Minimum Track Record Length).'] },
    ],
    related: ['paper-trading/how-alpaca-paper-trading-works', 'simulations/simulation-vs-backtesting-vs-paper-trading', 'metrics/minimum-track-record-length'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'how-alpaca-paper-trading-works',
    title: 'How Alpaca Paper Trading Works',
    level: 'Beginner',
    summary: 'Alpaca runs a simulated brokerage account against live market data. Spectra connects to it with your paper API keys and routes orders through its risk checks.',
    keywords: ['alpaca', 'broker', 'paper api', 'iex', 'market data'],
    blocks: [
      { type: 'p', text: 'Alpaca is a US broker with an API-first design. Every Alpaca account comes with a **paper** environment (`paper-api.alpaca.markets`) that has its own keys, its own simulated balance and its own order history, completely separate from any live account.' },
      { type: 'list', ordered: true, items: [
        'You place an order in Spectra’s Paper Trading tab and review it in a confirmation dialog.',
        'Spectra’s engine runs **pre-trade risk checks** (kill switches and the limits set in the Risk Center). If the checks cannot run, the order is refused: the path fails closed.',
        'Approved orders are sent to Alpaca’s paper API with your workspace’s stored keys.',
        'Alpaca’s simulator accepts the order and fills it against live market quotes.',
        'Spectra refreshes the account, positions and orders from Alpaca.',
      ] },
      { type: 'p', text: 'Charts use Alpaca’s market data API with the free **IEX** feed, which covers trades on one exchange; prices and volumes can therefore differ slightly from consolidated data shown elsewhere.' },
      { type: 'callout', tone: 'info', text: DISCLAIMER },
    ],
    related: ['paper-trading/connecting-alpaca', 'engine/risk-management', 'paper-trading/placing-orders'],
    platformLinks: [PAPER, { label: 'Open Risk Center', href: '/studio?tab=risk-center' }],
  }),

  L({
    slug: 'connecting-alpaca',
    title: 'Connecting an Alpaca Paper Account',
    level: 'Beginner',
    summary: 'Generate paper API keys on Alpaca, paste them into the Paper Trading tab, and Spectra verifies and stores them encrypted for your workspace.',
    keywords: ['connect', 'api key', 'secret key', 'credentials', 'setup'],
    blocks: [
      { type: 'list', ordered: true, items: [
        'Create a free account at alpaca.markets and switch the dashboard to **Paper Trading**.',
        'Generate an API key. You get a **Key ID** (starts with PK) and a **Secret Key**; the secret is shown once.',
        'In Spectra, open **Paper Trading** and paste both into the connection form.',
        'Spectra checks the keys live against Alpaca’s paper API before saving anything. Wrong keys are rejected with Alpaca’s message.',
        'Valid keys are stored with AES-256-GCM envelope encryption bound to your workspace, and the change is written to the audit log.',
      ] },
      { type: 'table', head: ['Requirement', 'Why'], rows: [
        ['Editor or Owner role in the workspace', 'Viewers can see the connection status but cannot change keys'],
        ['Paper keys, not live keys', 'The connection only talks to the paper endpoint'],
      ] },
      { type: 'callout', tone: 'tip', text: 'You can disconnect at any time from the Paper Trading tab. Disconnecting removes the stored keys; it does not change anything in your Alpaca account.' },
      { type: 'callout', tone: 'warning', text: 'Never paste live-trading keys into any tool you do not control, and never share your secret key.' },
    ],
    related: ['paper-trading/how-alpaca-paper-trading-works', 'paper-trading/searching-for-stocks'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'searching-for-stocks',
    title: 'Searching for Stocks',
    level: 'Beginner',
    summary: 'Find tradable symbols, add them to your watchlist and open their chart.',
    keywords: ['search', 'symbol', 'ticker', 'watchlist', 'assets'],
    blocks: [
      { type: 'p', text: 'The left panel of the Paper Trading tab has a **symbol search** over Alpaca’s list of tradable US assets: listed stocks and ETFs. Type a ticker (AAPL) or part of a company name.' },
      { type: 'h', text: 'Using the results' },
      { type: 'list', items: ['Click a result to open its chart and load it into the order ticket.', 'Use the + button to add it to your **watchlist**; the × removes it.', 'Your watchlist is kept in this browser, so it is there when you come back on the same device.'] },
      { type: 'h', text: 'Before you trade a symbol' },
      { type: 'list', items: [
        'Look at its chart on a daily timeframe first, then zoom into intraday bars.',
        'Check liquidity: thinly traded symbols have wide spreads and gaps between bars, so market orders can fill far from the last price.',
        'Remember that the chart uses the IEX feed, which covers one exchange; volume looks lower than consolidated volume.',
        'If you plan to backtest the same symbol, upload its history in the Data tab so the backtest and the paper account see comparable bars.',
      ] },
      { type: 'callout', tone: 'tip', text: 'Check that the symbol is the security you mean: share classes (BRK.A / BRK.B) and ETFs with similar names are easy to confuse.' },
    ],
    related: ['paper-trading/reading-candlestick-charts', 'paper-trading/placing-orders', 'fundamentals/liquidity'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'reading-candlestick-charts',
    title: 'Reading Candlestick Charts',
    level: 'Beginner',
    featured: true,
    summary: 'Each candle shows one bar’s open, high, low and close. The body is open to close; the wicks reach the high and low.',
    keywords: ['candlestick', 'ohlc', 'chart', 'wick', 'timeframe', 'volume'],
    blocks: [
      { type: 'diagram', id: 'candlestick', caption: 'A rising (green) and a falling (red) candle.' },
      { type: 'list', items: [
        '**Body**: from the open to the close. Green (or hollow) when the close is above the open; red when below.',
        '**Upper wick**: from the top of the body to the high.',
        '**Lower wick**: from the bottom of the body to the low.',
        '**Volume** bars beneath show how much traded in each candle.',
      ] },
      { type: 'h', text: 'Timeframes' },
      { type: 'p', text: 'The chart can show 1-minute, 5-minute, 15-minute, hourly, daily, weekly and monthly candles. A daily candle summarizes a whole session; switching timeframe changes the story: a strong daily uptrend can contain many red 5-minute candles.' },
      { type: 'h', text: 'Common readings' },
      { type: 'list', items: ['Long lower wick: sellers pushed the price down, buyers pushed it back.', 'Small body with long wicks: indecision.', 'A gap between candles: the price jumped between sessions (common at the open).'] },
      { type: 'callout', tone: 'info', text: 'Candle patterns on their own have weak predictive power. Treat them as a description of what happened, and test any rule built on them with a backtest.' },
    ],
    related: ['paper-trading/searching-for-stocks', 'fundamentals/how-stock-markets-work', 'fundamentals/trading-sessions'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'placing-orders',
    title: 'Placing Buy and Sell Orders',
    level: 'Beginner',
    summary: 'Use the order ticket to choose side, type and quantity, review the order, then submit it to your Alpaca paper account.',
    keywords: ['order ticket', 'buy', 'sell', 'market', 'limit', 'submit', 'review'],
    blocks: [
      { type: 'list', ordered: true, items: [
        'Pick a symbol from search or your watchlist.',
        'Choose **Buy** or **Sell**.',
        'Choose **Market** (fill now at the best available price) or **Limit** (fill only at your price or better) and, for a limit, enter the price.',
        'Enter the quantity. The ticket shows the estimated cost and your buying power.',
        'Click **Review**. A confirmation dialog shows exactly what will be sent.',
        'Click **Submit to Alpaca**. The order passes Spectra’s pre-trade risk checks and, if approved, goes to Alpaca. Orders are sent as day orders.',
      ] },
      { type: 'p', text: 'Selling shares you do not own opens a short position if your Alpaca paper account allows it. To close a position, use the close action in the positions table; it also asks for confirmation.' },
      { type: 'callout', tone: 'warning', text: 'A market order placed outside regular hours waits for the next open, when the price may have gapped. Use a limit order if the price matters to you.' },
      { type: 'callout', tone: 'info', text: 'The guided tour of Spectra never places, reviews or submits orders. Every order is your explicit action.' },
    ],
    related: ['fundamentals/market-and-limit-orders', 'paper-trading/order-statuses', 'engine/risk-management'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'order-statuses',
    title: 'Understanding Order Statuses',
    level: 'Beginner',
    summary: 'What new, accepted, partially filled, filled, canceled, expired and rejected mean for a paper order, and what to do about each.',
    keywords: ['order status', 'filled', 'partially filled', 'canceled', 'rejected', 'expired', 'accepted', 'pending'],
    blocks: [
      { type: 'table', head: ['Alpaca status', 'Meaning', 'What to do'], rows: [
        ['new / accepted / pending_new', 'Received and working (or queued until the market opens)', 'Wait, or cancel it'],
        ['partially_filled', 'Some shares filled, the rest still working', 'Wait, or cancel the remainder'],
        ['filled', 'Completely executed', 'Check the average fill price and your position'],
        ['canceled', 'Cancelled by you or the system', 'Nothing; no further fills'],
        ['expired', 'A day order that did not fill by the close', 'Re-enter if you still want it'],
        ['rejected', 'Refused (buying power, risk checks, invalid symbol)', 'Read the error message'],
      ] },
      { type: 'p', text: 'The orders table shows each order’s status, filled quantity out of the total, and the average fill price. Working orders (new, accepted, pending_new, partially_filled) have a **Cancel** button.' },
      { type: 'p', text: 'Orders refused by Spectra’s own pre-trade risk checks never reach Alpaca; you see the Risk Center’s reason instead of an Alpaca status.' },
    ],
    related: ['engine/order-lifecycle', 'paper-trading/placing-orders', 'paper-trading/monitoring-positions'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'monitoring-positions',
    title: 'Monitoring Positions',
    level: 'Beginner',
    summary: 'The positions table shows each holding’s quantity, average entry, current price, market value and unrealized P&L.',
    keywords: ['positions', 'holdings', 'unrealized', 'average entry', 'close position'],
    blocks: [
      { type: 'table', head: ['Column', 'Meaning'], rows: [
        ['Side', 'Long or short'],
        ['Shares', 'Quantity held'],
        ['Avg entry', 'Average cost per share'],
        ['Current price', 'Latest price'],
        ['Market value', 'Shares × current price'],
        ['Unrealized P&L', 'Market value − cost, in dollars and percent'],
        ['Action', 'Close the position with a market order (asks for confirmation)'],
      ] },
      { type: 'p', text: 'The Unrealized P&L card at the top adds up every position’s open P&L. It changes with every price move until you close the position, at which point it becomes realized.' },
      { type: 'formula', tex: String.raw`\text{Unrealized \%} = \frac{\text{Price} - \text{Avg entry}}{\text{Avg entry}}`, caption: 'for a long position' },
      { type: 'h', text: 'A routine for checking positions' },
      { type: 'list', ordered: true, items: [
        'Compare each position\u2019s market value with total equity: anything above your intended weight needs a decision.',
        'Look at unrealized P&L in percent, not only dollars, so small and large positions are comparable.',
        'Check working orders in the Open Orders tab: a forgotten limit order can open a position you no longer want.',
        'Close or reduce with the Action button; it sends a market order and asks for confirmation first.',
      ] },
      { type: 'callout', tone: 'tip', text: 'Watch concentration: one position that is a large share of equity dominates your risk. The Risk Center can cap it.' },
    ],
    related: ['metrics/realized-unrealized-pnl', 'paper-trading/tracking-portfolio-performance', 'paper-trading/account-balances'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'account-balances',
    title: 'Equity, Cash & Buying Power',
    level: 'Beginner',
    summary: 'Equity is what the account is worth; cash is what is not invested; buying power is how much more you can buy, including margin.',
    keywords: ['equity', 'cash', 'buying power', 'margin', 'regt', 'last equity'],
    blocks: [
      { type: 'table', head: ['Figure', 'Meaning'], rows: [
        ['Total equity', 'Cash + market value of all positions'],
        ['Cash balance', 'Money not invested (can be negative on margin)'],
        ['Buying power', 'How much you can buy now, including margin; Alpaca also reports Reg T buying power'],
        ['Last equity', 'Equity at the previous close, the base for today’s change'],
      ] },
      { type: 'formula', tex: String.raw`\text{Equity} = \text{Cash} + \sum \text{position market values}` },
      { type: 'formula', tex: String.raw`\text{Day change} = \text{Equity} - \text{Last equity}` },
      { type: 'p', text: 'Buying power can exceed equity on a margin account. Using it increases leverage: gains and losses grow faster than the account.' },
      { type: 'h', text: 'Worked example' },
      { type: 'p', text: 'You hold $60,000 of stock and $40,000 of cash, so equity is $100,000. Yesterday\u2019s close equity (last equity) was $98,500, so today\u2019s change is +$1,500 (+1.5%). Buying a further $30,000 of stock moves cash to $10,000; equity is unchanged at the moment of the fill except for costs, because cash was swapped for shares of equal value.' },
      { type: 'p', text: 'The **Portfolio Metrics** tab of the bottom panel also shows Reg T and day-trading buying power and the maintenance margin Alpaca reports for the account.' },
      { type: 'callout', tone: 'info', text: 'Buying power in a paper account follows Alpaca\u2019s simulated margin rules; it is not an offer of credit, and the funds are simulated.' },
    ],
    related: ['metrics/leverage', 'paper-trading/monitoring-positions', 'paper-trading/tracking-portfolio-performance'],
    platformLinks: [PAPER],
  }),

  L({
    slug: 'tracking-portfolio-performance',
    title: 'Tracking Portfolio Performance',
    level: 'Intermediate',
    summary: 'Turn a paper account’s history into the same metrics you use for backtests, and compare the two honestly.',
    keywords: ['performance', 'tracking', 'paper results', 'backtest gap', 'journal'],
    blocks: [
      { type: 'p', text: 'Treat a paper account like a backtest that runs in real time. Record equity at the same frequency as your backtest bars and compute the same metrics: total return, maximum drawdown, Sharpe ratio and trade statistics.' },
      { type: 'list', items: [
        'Compare daily change (equity − last equity) with the backtest’s daily returns over the same days.',
        'Compare average fill prices with the backtest’s assumed fills to estimate real slippage.',
        'Use the **Backtest-to-Paper Gap** panel on a backtest’s results to see where live behaviour departs from the simulation.',
        'Keep notes in the Experiment Registry so the paper period is linked to the strategy version it tested.',
      ] },
      { type: 'callout', tone: 'warning', text: DISCLAIMER },
      { type: 'p', text: 'A few weeks is rarely enough. Use the [Minimum Track Record Length](/learn/metrics/minimum-track-record-length) to estimate how long a paper record must be before its Sharpe ratio means something.' },
    ],
    related: ['metrics/total-return', 'metrics/sharpe-ratio', 'metrics/minimum-track-record-length', 'simulations/historical-vs-real-time-simulations'],
    platformLinks: [PAPER, { label: 'Open Backtests', href: '/studio?tab=backtests' }],
  }),
];
