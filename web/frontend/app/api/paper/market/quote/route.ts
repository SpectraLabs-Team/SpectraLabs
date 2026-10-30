import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { getBaselineTickerPrice } from '@/lib/marketStatus';

export const dynamic = 'force-dynamic';

function isCrypto(symbol: string): boolean {
  const s = symbol.toUpperCase().trim();
  return (
    s.includes('/') ||
    s.endsWith('USD') ||
    s.endsWith('USDT') ||
    ['SOL', 'BTC', 'ETH', 'DOGE', 'AVAX', 'LINK', 'ADA', 'LTC', 'SHIB', 'UNI', 'DOT', 'NEAR'].includes(s)
  );
}

function normalizeCryptoPair(symbol: string): { pair: string; base: string } {
  let s = symbol.toUpperCase().trim();
  if (s.includes('/')) {
    const [base, quote] = s.split('/');
    return { pair: `${base}-${quote}`, base };
  }
  if (s.endsWith('USD')) {
    const base = s.slice(0, -3);
    return { pair: `${base}-USD`, base };
  }
  if (s.endsWith('USDT')) {
    const base = s.slice(0, -4);
    return { pair: `${base}-USD`, base };
  }
  return { pair: `${s}-USD`, base: s };
}

export async function GET(req: Request) {
  const { res } = await requireWorkspace('viewer');
  if (res) return res;

  const url = new URL(req.url);
  const symbol = url.searchParams.get('symbol')?.trim().toUpperCase() || 'AAPL';

  // 1. Crypto Live Quote via Coinbase / Binance (24/7)
  if (isCrypto(symbol)) {
    const { pair, base } = normalizeCryptoPair(symbol);
    try {
      const [tickerResp, statsResp] = await Promise.allSettled([
        fetch(`https://api.exchange.coinbase.com/products/${pair}/ticker`, {
          headers: { 'User-Agent': 'SpectraLabs-Market/1.0' },
          cache: 'no-store',
        }),
        fetch(`https://api.exchange.coinbase.com/products/${pair}/stats`, {
          headers: { 'User-Agent': 'SpectraLabs-Market/1.0' },
          cache: 'no-store',
        }),
      ]);

      if (tickerResp.status === 'fulfilled' && tickerResp.value.ok) {
        const ticker = await tickerResp.value.json();
        const price = parseFloat(ticker.price);
        let openPrice = price;
        let high = price;
        let low = price;
        let volume = parseFloat(ticker.volume || '0');

        if (statsResp.status === 'fulfilled' && statsResp.value.ok) {
          const stats = await statsResp.value.json();
          openPrice = parseFloat(stats.open || ticker.price);
          high = parseFloat(stats.high || ticker.price);
          low = parseFloat(stats.low || ticker.price);
          if (stats.volume) volume = parseFloat(stats.volume);
        }

        const change = price - openPrice;
        const changePct = openPrice > 0 ? (change / openPrice) * 100 : 0;

        return NextResponse.json({
          ok: true,
          symbol: `${base}/USD`,
          price: Math.round(price * 100) / 100,
          change: Math.round(change * 100) / 100,
          changePct: Math.round(changePct * 100) / 100,
          high: Math.round(high * 100) / 100,
          low: Math.round(low * 100) / 100,
          volume: Math.round(volume),
          source: 'Coinbase Pro (24/7 Feed)',
          timestamp: Date.now(),
        });
      }
    } catch (e) {
      console.warn('[Market API] Coinbase quote error:', e);
    }

    // Fallback: Binance 24hr ticker
    try {
      const binResp = await fetch(`https://api.binance.com/api/v3/ticker/24hr?symbol=${base}USDT`, {
        cache: 'no-store',
      });
      if (binResp.ok) {
        const b = await binResp.json();
        const price = parseFloat(b.lastPrice);
        const change = parseFloat(b.priceChange);
        const changePct = parseFloat(b.priceChangePercent);
        const high = parseFloat(b.highPrice);
        const low = parseFloat(b.lowPrice);
        const volume = parseFloat(b.volume);

        return NextResponse.json({
          ok: true,
          symbol: `${base}/USD`,
          price: Math.round(price * 100) / 100,
          change: Math.round(change * 100) / 100,
          changePct: Math.round(changePct * 100) / 100,
          high: Math.round(high * 100) / 100,
          low: Math.round(low * 100) / 100,
          volume: Math.round(volume),
          source: 'Binance Live (24/7 Feed)',
          timestamp: Date.now(),
        });
      }
    } catch (e) {
      console.warn('[Market API] Binance quote error:', e);
    }
  }

  // 2. US Equities Live Quote via Yahoo Finance
  try {
    const cleanSym = symbol.replace(/[^A-Z0-9.-]/g, '');
    const yfResp = await fetch(`https://query1.finance.yahoo.com/v8/finance/chart/${cleanSym}?range=1d&interval=1m`, {
      headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' },
      cache: 'no-store',
    });

    if (yfResp.ok) {
      const yfData = await yfResp.json();
      const meta = yfData?.chart?.result?.[0]?.meta;
      if (meta) {
        const price = meta.regularMarketPrice || meta.chartPreviousClose || 100;
        const prevClose = meta.chartPreviousClose || meta.previousClose || price;
        const change = price - prevClose;
        const changePct = prevClose > 0 ? (change / prevClose) * 100 : 0;
        const high = meta.regularMarketDayHigh || meta.dayHigh || price;
        const low = meta.regularMarketDayLow || meta.dayLow || price;
        const volume = meta.regularMarketVolume || 0;

        return NextResponse.json({
          ok: true,
          symbol: cleanSym,
          price: Math.round(price * 100) / 100,
          change: Math.round(change * 100) / 100,
          changePct: Math.round(changePct * 100) / 100,
          high: Math.round(high * 100) / 100,
          low: Math.round(low * 100) / 100,
          volume,
          source: 'Real-Time Market Feed',
          timestamp: Date.now(),
        });
      }
    }
  } catch (e) {
    console.warn('[Market API] Yahoo Finance quote error:', e);
  }

  // 3. Fallback Quote
  const baseline = getBaselineTickerPrice(symbol);
  return NextResponse.json({
    ok: true,
    symbol,
    price: baseline,
    change: 0,
    changePct: 0,
    high: baseline * 1.01,
    low: baseline * 0.99,
    volume: 100000,
    source: 'Synthetic Market Feed',
    timestamp: Date.now(),
  });
}
