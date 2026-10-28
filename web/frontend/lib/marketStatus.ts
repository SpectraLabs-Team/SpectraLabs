/**
 * US Equities Market Session & Curated Watchlists.
 * Calculates Eastern Time (America/New_York) market hours including
 * Pre-Market, Regular Trading Hours, and After-Hours.
 */

export type MarketSessionType = 'open' | 'pre-market' | 'after-hours' | 'closed';

export interface MarketSessionInfo {
  session: MarketSessionType;
  label: string;
  badgeTone: 'profit' | 'caution' | 'neutral';
  description: string;
  isRegularHours: boolean;
  formattedTimeEt: string;
}

export interface CuratedAsset {
  symbol: string;
  name: string;
  category: 'tech' | 'etfs' | 'volatility';
  defaultPrice: number;
  changePct: number;
}

export const CURATED_WATCHLIST_ITEMS: CuratedAsset[] = [
  // Mega-Cap Tech
  { symbol: 'AAPL', name: 'Apple Inc.', category: 'tech', defaultPrice: 185.0, changePct: 1.24 },
  { symbol: 'NVDA', name: 'NVIDIA Corporation', category: 'tech', defaultPrice: 180.0, changePct: 3.42 },
  { symbol: 'MSFT', name: 'Microsoft Corporation', category: 'tech', defaultPrice: 425.0, changePct: 0.88 },
  { symbol: 'GOOGL', name: 'Alphabet Inc.', category: 'tech', defaultPrice: 178.5, changePct: -0.42 },
  { symbol: 'AMZN', name: 'Amazon.com Inc.', category: 'tech', defaultPrice: 210.0, changePct: 1.15 },
  { symbol: 'META', name: 'Meta Platforms Inc.', category: 'tech', defaultPrice: 650.0, changePct: 2.18 },
  { symbol: 'TSLA', name: 'Tesla Inc.', category: 'tech', defaultPrice: 330.0, changePct: -1.75 },

  // Indices & ETFs
  { symbol: 'SPY', name: 'SPDR S&P 500 ETF Trust', category: 'etfs', defaultPrice: 602.5, changePct: 0.65 },
  { symbol: 'QQQ', name: 'Invesco QQQ Trust', category: 'etfs', defaultPrice: 520.0, changePct: 1.12 },
  { symbol: 'IWM', name: 'iShares Russell 2000 ETF', category: 'etfs', defaultPrice: 220.0, changePct: -0.28 },
  { symbol: 'TLT', name: 'iShares 20+ Year Treasury', category: 'etfs', defaultPrice: 90.0, changePct: -0.52 },
  { symbol: 'GLD', name: 'SPDR Gold Shares', category: 'etfs', defaultPrice: 300.0, changePct: 0.42 },
  { symbol: 'VXX', name: 'iPath Series B S&P 500 VIX', category: 'etfs', defaultPrice: 14.2, changePct: -2.15 },

  // High Beta & Volatility
  { symbol: 'COIN', name: 'Coinbase Global Inc.', category: 'volatility', defaultPrice: 305.0, changePct: 4.85 },
  { symbol: 'MSTR', name: 'MicroStrategy Inc.', category: 'volatility', defaultPrice: 350.0, changePct: 6.25 },
  { symbol: 'SMCI', name: 'Super Micro Computer', category: 'volatility', defaultPrice: 45.0, changePct: -3.45 },
  { symbol: 'ARM', name: 'Arm Holdings plc', category: 'volatility', defaultPrice: 142.0, changePct: 2.92 },
  { symbol: 'AMD', name: 'Advanced Micro Devices', category: 'volatility', defaultPrice: 162.0, changePct: 1.48 },
  { symbol: 'PLTR', name: 'Palantir Technologies', category: 'volatility', defaultPrice: 55.0, changePct: 3.12 },
];

export const WATCHLIST_CATEGORY_LABELS: Record<string, string> = {
  my: 'My Watchlist',
  tech: 'Mega Tech',
  etfs: 'Indices & ETFs',
  volatility: 'High Beta',
};

/**
 * Returns current US Equity market session information in America/New_York timezone.
 */
export function getMarketSession(now = new Date()): MarketSessionInfo {
  // Extract ET components
  const formatter = new Intl.DateTimeFormat('en-US', {
    timeZone: 'America/New_York',
    weekday: 'short',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
    hour12: false,
  });

  const parts = formatter.formatToParts(now);
  const getPart = (type: string) => parts.find((p) => p.type === type)?.value || '';

  const weekday = getPart('weekday');
  const hour = parseInt(getPart('hour'), 10) || 0;
  const minute = parseInt(getPart('minute'), 10) || 0;
  const second = parseInt(getPart('second'), 10) || 0;

  const timeFormatted = `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}:${String(second).padStart(2, '0')} ET`;
  const minutesSinceMidnight = hour * 60 + minute;

  // Weekends
  if (weekday === 'Sat' || weekday === 'Sun') {
    return {
      session: 'closed',
      label: 'Market Closed',
      badgeTone: 'neutral',
      description: 'Weekend · Reopens Monday 9:30 AM ET',
      isRegularHours: false,
      formattedTimeEt: timeFormatted,
    };
  }

  // Pre-Market: 04:00 to 09:30 (240 to 570 mins)
  if (minutesSinceMidnight >= 240 && minutesSinceMidnight < 570) {
    return {
      session: 'pre-market',
      label: 'Pre-Market Session',
      badgeTone: 'caution',
      description: 'Pre-market trading · Regular open at 9:30 AM ET',
      isRegularHours: false,
      formattedTimeEt: timeFormatted,
    };
  }

  // Regular Trading Hours: 09:30 to 16:00 (570 to 960 mins)
  if (minutesSinceMidnight >= 570 && minutesSinceMidnight < 960) {
    return {
      session: 'open',
      label: 'Market Open',
      badgeTone: 'profit',
      description: 'Regular Trading Hours (9:30 AM – 4:00 PM ET)',
      isRegularHours: true,
      formattedTimeEt: timeFormatted,
    };
  }

  // After-Hours: 16:00 to 20:00 (960 to 1200 mins)
  if (minutesSinceMidnight >= 960 && minutesSinceMidnight < 1200) {
    return {
      session: 'after-hours',
      label: 'After-Hours Session',
      badgeTone: 'neutral',
      description: 'Extended hours trading (closes 8:00 PM ET)',
      isRegularHours: false,
      formattedTimeEt: timeFormatted,
    };
  }

  // Closed: 20:00 to 04:00
  return {
    session: 'closed',
    label: 'Market Closed',
    badgeTone: 'neutral',
    description: 'Closed · Pre-market begins 4:00 AM ET',
    isRegularHours: false,
    formattedTimeEt: timeFormatted,
  };
}

/**
 * Returns baseline fallback price for any known ticker.
 */
export function getBaselineTickerPrice(symbol: string): number {
  const match = CURATED_WATCHLIST_ITEMS.find((a) => a.symbol.toUpperCase() === symbol.toUpperCase());
  return match?.defaultPrice ?? 175.0;
}
