/**
 * Institutional preset books for the Portfolio Studio. CSV format matches the engine:
 * symbol,quantity,price,volatility (annualized). Prices and vols are illustrative round
 * numbers for demonstration, not live quotes. Negative quantities are short hedges.
 */
export interface PortfolioPreset {
  id: string;
  name: string;
  blurb: string;
  csv: string;
}

export const PRESETS: PortfolioPreset[] = [
  {
    id: 'mag7',
    name: 'Magnificent 7 · Tech Concentrated',
    blurb: 'Seven mega-cap growth names: high beta, high single-sector concentration.',
    csv: [
      'symbol,quantity,price,volatility',
      'NVDA,1500,180.00,0.50',
      'AAPL,1200,225.00,0.28',
      'MSFT,600,450.00,0.25',
      'GOOGL,1400,185.00,0.30',
      'AMZN,1200,210.00,0.33',
      'META,400,650.00,0.38',
      'TSLA,700,330.00,0.60',
    ].join('\n'),
  },
  {
    id: 'macro-6040',
    name: 'Macro Balanced 60/40',
    blurb: 'Equity index core, duration ballast, gold and oil, with short small-cap and credit hedges.',
    csv: [
      'symbol,quantity,price,volatility',
      'SPY,1000,600.00,0.17',
      'QQQ,400,520.00,0.22',
      'TLT,4000,90.00,0.16',
      'IEF,2500,95.00,0.07',
      'GLD,800,300.00,0.15',
      'USO,1000,75.00,0.35',
      'IWM,-800,220.00,0.24',
      'HYG,-1000,80.00,0.08',
    ].join('\n'),
  },
  {
    id: 'all-weather',
    name: 'All-Weather Classic · Risk Parity',
    blurb: 'Multi-asset weather-proofing: equities, intermediate & long treasuries, physical gold and commodities.',
    csv: [
      'symbol,quantity,price,volatility',
      'SPY,1500,600.00,0.16',
      'TLT,4500,90.00,0.15',
      'IEF,3000,95.00,0.08',
      'GLD,750,300.00,0.14',
      'DBC,3000,22.00,0.20',
    ].join('\n'),
  },
  {
    id: 'benchmark-sp500',
    name: 'S&P 500 Benchmark · Core Equity',
    blurb: 'Pure broad-market benchmark exposure for relative VaR and alpha backtesting.',
    csv: [
      'symbol,quantity,price,volatility',
      'SPY,2500,600.00,0.17',
    ].join('\n'),
  },
  {
    id: 'tail-risk',
    name: 'Tail-Risk · High-Beta Volatile',
    blurb: 'Crypto proxies and AI semis: fat tails, vols of 55–90%.',
    csv: [
      'symbol,quantity,price,volatility',
      'SMCI,3000,45.00,0.85',
      'COIN,800,300.00,0.75',
      'MSTR,500,350.00,0.90',
      'ARM,1000,140.00,0.65',
      'AMD,1200,160.00,0.55',
    ].join('\n'),
  },
];
