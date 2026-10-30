import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { getBaselineTickerPrice } from '@/lib/marketStatus';

export const dynamic = 'force-dynamic';

export interface CandleBar {
  t: string | number;
  o: number;
  h: number;
  l: number;
  c: number;
  v: number;
  n?: number;
  vw?: number;
}

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

function coinbaseGranularity(timeframe: string): number {
  switch (timeframe) {
    case '1Min':
      return 60;
    case '5Min':
      return 300;
    case '15Min':
      return 900;
    case '1Hour':
      return 3600;
    case '1Day':
      return 86400;
    case '1Week':
    case '1Month':
      return 86400;
    default:
      return 86400;
  }
}

function yahooParams(timeframe: string): { range: string; interval: string } {
  switch (timeframe) {
    case '1Min':
      return { range: '1d', interval: '1m' };
    case '5Min':
      return { range: '5d', interval: '5m' };
    case '15Min':
      return { range: '5d', interval: '15m' };
    case '1Hour':
      return { range: '1mo', interval: '1h' };
    case '1Day':
      return { range: '6mo', interval: '1d' };
    case '1Week':
      return { range: '2y', interval: '1wk' };
    case '1Month':
      return { range: '5y', interval: '1mo' };
    default:
      return { range: '6mo', interval: '1d' };
  }
}

export async function GET(req: Request) {
  const { res } = await requireWorkspace('viewer');
  if (res) return res;

  const url = new URL(req.url);
  const symbol = url.searchParams.get('symbol')?.trim().toUpperCase() || 'AAPL';
  const timeframe = url.searchParams.get('timeframe')?.trim() || '1Day';
  const limit = Math.min(parseInt(url.searchParams.get('limit') || '100', 10), 300);

  // 1. Crypto Live Candlesticks via Coinbase / Binance Public Feed (24/7)
  if (isCrypto(symbol)) {
    const { pair, base } = normalizeCryptoPair(symbol);
    try {
      const gran = coinbaseGranularity(timeframe);
      const cbResp = await fetch(`https://api.exchange.coinbase.com/products/${pair}/candles?granularity=${gran}`, {
        headers: { 'User-Agent': 'SpectraLabs-Market/1.0' },
        cache: 'no-store',
      });

      if (cbResp.ok) {
        const rawBars: number[][] = await cbResp.json();
        if (Array.isArray(rawBars) && rawBars.length > 0) {
          // Coinbase format: [ time, low, high, open, close, volume ] sorted descending
          const chronological = [...rawBars].reverse();
          const sliced = chronological.slice(-limit);
          const bars: CandleBar[] = sliced.map(([timeSec, low, high, open, close, volume]) => ({
            t: new Date(timeSec * 1000).toISOString(),
            o: Math.round(open * 100) / 100,
            h: Math.round(high * 100) / 100,
            l: Math.round(low * 100) / 100,
            c: Math.round(close * 100) / 100,
            v: Math.round(volume),
          }));
          return NextResponse.json({
            ok: true,
            bars,
            symbol: `${base}/USD`,
            source: 'Coinbase Pro (24/7 Feed)',
          });
        }
      }
    } catch (e) {
      console.warn('[Market API] Coinbase candles failed, falling back:', e);
    }

    // Fallback: Binance Public Klines
    try {
      const binanceSym = `${base}USDT`;
      const interval = timeframe.includes('Min') ? '5m' : timeframe.includes('Hour') ? '1h' : '1d';
      const binResp = await fetch(`https://api.binance.com/api/v3/klines?symbol=${binanceSym}&interval=${interval}&limit=${limit}`, {
        cache: 'no-store',
      });
      if (binResp.ok) {
        const data: any[][] = await binResp.json();
        if (Array.isArray(data) && data.length > 0) {
          const bars: CandleBar[] = data.map((d) => ({
            t: new Date(d[0]).toISOString(),
            o: parseFloat(d[1]),
            h: parseFloat(d[2]),
            l: parseFloat(d[3]),
            c: parseFloat(d[4]),
            v: parseFloat(d[5]),
          }));
          return NextResponse.json({
            ok: true,
            bars,
            symbol: `${base}/USD`,
            source: 'Binance Live (24/7 Feed)',
          });
        }
      }
    } catch (e) {
      console.warn('[Market API] Binance candles failed:', e);
    }
  }

  // 2. US Equities Live Market Feed via Yahoo Finance Public API
  try {
    const cleanSym = symbol.replace(/[^A-Z0-9.-]/g, '');
    const { range, interval } = yahooParams(timeframe);
    const yfResp = await fetch(`https://query1.finance.yahoo.com/v8/finance/chart/${cleanSym}?range=${range}&interval=${interval}`, {
      headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' },
      cache: 'no-store',
    });

    if (yfResp.ok) {
      const yfData = await yfResp.json();
      const result = yfData?.chart?.result?.[0];
      if (result && Array.isArray(result.timestamp) && result.indicators?.quote?.[0]) {
        const timestamps: number[] = result.timestamp;
        const q = result.indicators.quote[0];
        const bars: CandleBar[] = [];

        for (let i = 0; i < timestamps.length; i++) {
          if (q.close[i] == null || q.open[i] == null) continue;
          bars.push({
            t: new Date(timestamps[i] * 1000).toISOString(),
            o: Math.round(q.open[i] * 100) / 100,
            h: Math.round((q.high[i] || Math.max(q.open[i], q.close[i])) * 100) / 100,
            l: Math.round((q.low[i] || Math.min(q.open[i], q.close[i])) * 100) / 100,
            c: Math.round(q.close[i] * 100) / 100,
            v: q.volume?.[i] || 0,
          });
        }

        if (bars.length > 0) {
          return NextResponse.json({
            ok: true,
            bars: bars.slice(-limit),
            symbol: cleanSym,
            source: 'Real-Time Market Feed',
          });
        }
      }
    }
  } catch (e) {
    console.warn('[Market API] Yahoo Finance chart failed:', e);
  }

  // 3. Resilient Synthetic Baseline Fallback
  const baseline = getBaselineTickerPrice(symbol);
  const now = Date.now();
  const stepMs = timeframe.includes('Min') ? 300_000 : timeframe.includes('Hour') ? 3_600_000 : 86_400_000;
  const bars: CandleBar[] = [];
  let price = baseline;

  for (let i = 45; i >= 0; i--) {
    const t = new Date(now - i * stepMs).toISOString();
    const delta = (Math.random() - 0.49) * (price * 0.012);
    const c = Math.max(0.1, Math.round((price + delta) * 100) / 100);
    const o = price;
    const h = Math.round((Math.max(o, c) + Math.random() * (price * 0.005)) * 100) / 100;
    const l = Math.round((Math.min(o, c) - Math.random() * (price * 0.005)) * 100) / 100;
    bars.push({
      t,
      o,
      h,
      l,
      c,
      v: Math.floor(100_000 + Math.random() * 400_000),
    });
    price = c;
  }

  return NextResponse.json({
    ok: true,
    bars,
    symbol,
    source: 'Synthetic Market Feed',
  });
}
