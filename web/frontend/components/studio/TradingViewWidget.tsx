'use client';

import React, { useEffect, useRef, memo } from 'react';

export interface TradingViewWidgetProps {
  symbol: string;
  theme?: 'dark' | 'light';
  interval?: string;
  className?: string;
  height?: number | string;
  hideSideToolbar?: boolean;
}

export function toTradingViewSymbol(symbol: string): string {
  const s = symbol.toUpperCase().trim();

  // Crypto pairs (Coinbase / Binance live feed)
  if (s.includes('SOL')) return 'COINBASE:SOLUSD';
  if (s.includes('BTC')) return 'COINBASE:BTCUSD';
  if (s.includes('ETH')) return 'COINBASE:ETHUSD';
  if (s.includes('DOGE')) return 'COINBASE:DOGEUSD';
  if (s.includes('AVAX')) return 'COINBASE:AVAXUSD';
  if (s.includes('LINK')) return 'COINBASE:LINKUSD';
  if (s.includes('LTC')) return 'COINBASE:LTCUSD';
  if (s.includes('ADA')) return 'COINBASE:ADAUSD';

  // Specific NYSE Stocks
  if (
    [
      'ORCL',
      'IBM',
      'DIS',
      'JNJ',
      'JPM',
      'V',
      'PG',
      'UNH',
      'HD',
      'BAC',
      'XOM',
      'CVX',
      'WMT',
      'KO',
      'PEP',
      'PFE',
      'ABT',
      'NKE',
      'MCD',
      'CRM',
      'BA',
      'GE',
      'GS',
      'MS',
      'C',
      'BMY',
      'T',
      'VZ',
      'UBER',
    ].includes(s)
  ) {
    return `NYSE:${s}`;
  }

  // Specific AMEX / Arca ETFs
  if (['SPY', 'IWM', 'GLD', 'VXX', 'DIA', 'XLF', 'XLE', 'XLK'].includes(s)) {
    return `AMEX:${s}`;
  }

  // Specific NASDAQ Stocks & ETFs
  if (
    [
      'AAPL',
      'MSFT',
      'NVDA',
      'TSLA',
      'AMZN',
      'GOOGL',
      'GOOG',
      'META',
      'QQQ',
      'AMD',
      'ARM',
      'COIN',
      'MSTR',
      'SMCI',
      'PLTR',
      'NFLX',
      'INTC',
    ].includes(s)
  ) {
    return `NASDAQ:${s}`;
  }

  // If already formatted with exchange prefix (e.g. "NASDAQ:AAPL")
  if (s.includes(':')) return s;

  // Default to NASDAQ
  return `NASDAQ:${s}`;
}

export const TradingViewWidget = memo(function TradingViewWidget({
  symbol,
  theme = 'dark',
  interval = 'D',
  className = '',
  height = 620,
  hideSideToolbar = false,
}: TradingViewWidgetProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // Clear any previous widget DOM
    container.innerHTML = '';

    const widgetHolder = document.createElement('div');
    widgetHolder.className = 'tradingview-widget-container__widget';
    widgetHolder.style.height = 'calc(100% - 32px)';
    widgetHolder.style.width = '100%';
    container.appendChild(widgetHolder);

    const script = document.createElement('script');
    script.src = 'https://s3.tradingview.com/external-embedding/embed-widget-advanced-chart.js';
    script.type = 'text/javascript';
    script.async = true;

    const tvSymbol = toTradingViewSymbol(symbol);
    const tvInterval =
      interval === '1Min'
        ? '1'
        : interval === '5Min'
        ? '5'
        : interval === '15Min'
        ? '15'
        : interval === '1Hour'
        ? '60'
        : interval === '1Week'
        ? 'W'
        : interval === '1Month'
        ? 'M'
        : 'D';

    script.innerHTML = JSON.stringify({
      autosize: true,
      symbol: tvSymbol,
      interval: tvInterval,
      timezone: 'exchange',
      theme: theme === 'dark' ? 'dark' : 'light',
      style: '1',
      locale: 'en',
      enable_publishing: false,
      allow_symbol_change: true,
      calendar: false,
      hide_top_toolbar: false,
      hide_legend: false,
      hide_side_toolbar: hideSideToolbar,
      save_image: true,
      withdateranges: true,
      hide_volume: false,
      backgroundColor: theme === 'dark' ? '#0b0f19' : '#ffffff',
      gridColor: theme === 'dark' ? 'rgba(242, 242, 242, 0.06)' : 'rgba(0, 0, 0, 0.06)',
      studies: ['STD;SMA'],
      support_host: 'https://www.tradingview.com',
    });

    container.appendChild(script);

    return () => {
      if (container) container.innerHTML = '';
    };
  }, [symbol, theme, interval, hideSideToolbar]);

  return (
    <div
      className={`tradingview-widget-container relative w-full rounded-2xl overflow-hidden border border-ink/10 shadow-sm ${className}`}
      style={{
        height: typeof height === 'number' ? `${height}px` : height,
        minHeight: '520px',
        width: '100%',
      }}
      ref={containerRef}
    />
  );
});
