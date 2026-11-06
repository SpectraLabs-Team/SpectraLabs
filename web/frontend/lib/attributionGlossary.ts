// M11 Task 5: Help text and glossary for P&L attribution panels.
// Provides plain-language explanations, tooltip summaries, and worked examples
// for waterfall steps, cost items, and attribution metrics.
// Following M22 guidance: copy avoids return promises and speculative claims.

export interface AttributionGlossaryEntry {
  term: string;
  short: string;
  description: string;
  example: string;
  category: 'waterfall' | 'costs' | 'metrics';
  aliases?: string[];
}

export const ATTRIBUTION_GLOSSARY: AttributionGlossaryEntry[] = [
  // --- Waterfall Steps ---
  {
    term: 'Gross Trading P&L',
    short: 'Total realized gain or loss across closed trades before deducting transaction fees.',
    description:
      'The raw trading result computed from entry and exit execution prices across all completed round-trip trades. Does not include broker commissions, financing charges, or borrow fees.',
    example:
      'Buying 100 shares at $50.00 and selling at $55.00 produces a Gross Trading P&L of +$500.00.',
    category: 'waterfall',
    aliases: ['gross trading pnl', 'gross p&l', 'trade gross'],
  },
  {
    term: 'Fees on Closed Trades',
    short: 'Direct brokerage commissions and exchange fees charged on completed trades.',
    description:
      'Total transaction costs paid to brokers and trading venues to execute the entry and exit fills of closed positions.',
    example:
      'Executing 2 trades with a fixed $1.00 minimum commission incurs -$2.00 in Fees on Closed Trades.',
    category: 'waterfall',
    aliases: ['fees', 'trade fees', 'closed trade fees'],
  },
  {
    term: 'Trading Net P&L',
    short: 'Realized net profit or loss from closed trades (Gross minus closed trade fees).',
    description:
      'The net trading result from completed positions, matching the total_net_pnl in backtest and paper summaries.',
    example:
      'With +$500.00 Gross Trading P&L and -$2.00 in fees, Trading Net P&L is +$498.00.',
    category: 'waterfall',
    aliases: ['trading net pnl', 'trading net', 'total net pnl'],
  },
  {
    term: 'Open Positions (Realized So Far)',
    short: 'Mark-to-market unrealized P&L on holdings that remain open in the portfolio.',
    description:
      'The valuation difference between open position entry costs and the latest mark price. Reflects unrealized equity change.',
    example:
      'Holding 50 shares bought at $100.00 currently marked at $104.00 contributes +$200.00 in Open Position P&L.',
    category: 'waterfall',
    aliases: ['open positions', 'open position pnl', 'open p&l'],
  },
  {
    term: 'Net Account P&L',
    short: 'Final reconciled account P&L incorporating trading net, open positions, financing, and fees.',
    description:
      'The comprehensive portfolio bottom line (total_account_pnl) reconciling closed trading results, open mark-to-market positions, borrow fees, financing costs, and interest income.',
    example:
      'Trading Net ($498.00) + Open P&L ($200.00) - Borrow Fees ($1.00) + Interest ($3.00) = Net Account P&L of +$700.00.',
    category: 'waterfall',
    aliases: ['net account pnl', 'total account pnl', 'account pnl'],
  },

  // --- Costs Categories ---
  {
    term: 'Commissions',
    short: 'Execution and order routing charges assessed by the broker.',
    description:
      'Transaction fees incurred on order executions, based on per-share, per-contract, or minimum ticket pricing models.',
    example:
      'Trading 5 option contracts with a $0.65 fee per contract incurs $3.25 in commissions.',
    category: 'costs',
    aliases: ['commission fees', 'commission'],
  },
  {
    term: 'Borrow fees',
    short: 'Daily custody fee charged for borrowing shares to maintain short positions.',
    description:
      'Fee assessed on short stock positions based on the annualized borrow rate and market value of the borrowed shares.',
    example:
      'Maintaining a $10,000 short position with a 3.65% annualized borrow rate incurs $1.00 per calendar day in borrow fees.',
    category: 'costs',
    aliases: ['borrow fee', 'borrow', 'short borrow fees'],
  },
  {
    term: 'Financing interest',
    short: 'Interest charged on debit balances or leverage borrowed to fund positions overnight.',
    description:
      'Brokerage debit interest charged on cash borrowed to maintain positions exceeding account cash balance.',
    example:
      'Borrowing $25,000 on margin at a 6% annual rate incurs approximately $4.11 in daily financing interest.',
    category: 'costs',
    aliases: ['financing', 'margin interest', 'financing cost'],
  },
  {
    term: 'Interest income',
    short: 'Interest credited to the account on uninvested cash and collateral balances.',
    description:
      'Yield paid by the broker or custodian on unallocated cash balances held in the trading account.',
    example:
      'Holding $60,000 in uninvested cash earning a 4.5% annual credit yield generates ~$7.40 per day in interest income.',
    category: 'costs',
    aliases: ['interest', 'cash yield', 'credit interest'],
  },
  {
    term: 'Dividends (in trade gross)',
    short: 'Cash dividend distributions received or paid out during position holding periods.',
    description:
      'Memo line showing cash dividends credited to long shareholdings or debited from short positions. Already included within gross trade P&L.',
    example:
      'Holding 100 shares across an ex-dividend date paying $0.75 per share credits $75.00 in dividend cash flow.',
    category: 'costs',
    aliases: ['dividends', 'dividend'],
  },
  {
    term: 'Coupons (in trade gross)',
    short: 'Periodic interest payments received from bond holdings.',
    description:
      'Memo line showing bond coupon distributions credited during holding periods, reflected within bond gross trading cash flows.',
    example:
      'Holding 10 bonds with a 4% annual coupon paid semi-annually generates $200.00 per payment period.',
    category: 'costs',
    aliases: ['coupons', 'coupon'],
  },
  {
    term: 'Slippage estimate (in fill prices)',
    short: 'Estimated execution drag between decision price and actual execution fill price.',
    description:
      'Memo line estimating market impact and bid-ask spread friction. Since fill prices reflect slippage directly, it is not charged as an additional fee.',
    example:
      'Buying at an effective fill price of $100.05 vs. a mid-market price of $100.00 represents 5 basis points ($0.05/share) of slippage drag.',
    category: 'costs',
    aliases: ['slippage', 'slippage costs', 'execution drag'],
  },

  // --- Metrics & Distribution ---
  {
    term: 'Win rate',
    short: 'Percentage of completed closed trades that ended with a positive gross profit.',
    description:
      'The ratio of winning closed trades to total closed trades in the evaluated period.',
    example:
      'A strategy completing 6 winning trades and 4 losing trades has a 60% win rate.',
    category: 'metrics',
    aliases: ['winrate', 'overall win rate'],
  },
  {
    term: 'Share of |net|',
    short: 'Signed contribution of an individual instrument to total absolute portfolio net P&L.',
    description:
      'Relative attribution weight calculated as the instrument net P&L divided by the sum of absolute net P&Ls across all instruments.',
    example:
      'If AAPL gained +$300 and MSFT lost -$100, the sum of absolute net is $400. AAPL has a +75% share and MSFT has a -25% share.',
    category: 'metrics',
    aliases: ['pnl share', 'share', 'pnl_share'],
  },
  {
    term: 'Long / short',
    short: 'Performance and trade breakdown segmented by trade side (long vs. short).',
    description:
      'Separates trade count, win rate, gross P&L, fees, and net P&L according to whether positions were opened long or short.',
    example:
      'A market-neutral strategy might show +$400 net on long trades and +$150 net on short trades, summing to +$550.',
    category: 'metrics',
    aliases: ['long short', 'direction', 'side split'],
  },
];

/**
 * Resolves a glossary entry by exact name, lowercased name, or alias.
 */
export function getGlossaryEntry(term: string): AttributionGlossaryEntry | undefined {
  const norm = term.trim().toLowerCase();
  return ATTRIBUTION_GLOSSARY.find((e) => {
    if (e.term.toLowerCase() === norm) return true;
    if (e.aliases?.some((a) => a.toLowerCase() === norm)) return true;
    return false;
  });
}

/**
 * Returns all registered glossary entries.
 */
export function getAllGlossaryEntries(): AttributionGlossaryEntry[] {
  return [...ATTRIBUTION_GLOSSARY];
}

/**
 * Returns entries matching the requested category.
 */
export function getEntriesByCategory(category: AttributionGlossaryEntry['category']): AttributionGlossaryEntry[] {
  return ATTRIBUTION_GLOSSARY.filter((e) => e.category === category);
}
