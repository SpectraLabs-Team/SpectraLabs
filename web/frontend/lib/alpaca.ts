/**
 * Alpaca Paper Trading API Client (Workspace-Scoped)
 *
 * Provides typed methods for credential status, connecting/disconnecting,
 * querying account balance, positions, active/historical orders, assets,
 * and historical market bars through the authenticated Next.js proxy.
 *
 * All requests route through the backend:
 * - Credentials management: /api/paper/alpaca/*
 * - Broker order execution & data: /api/engine/api/v1/alpaca/*
 */

export interface AlpacaConnectionStatus {
  connected: boolean;
  keyIdHint?: string;
  updatedAt?: number;
}

export interface AlpacaAccount {
  id: string;
  account_number: string;
  status: string;
  currency: string;
  buying_power: string;
  regt_buying_power?: string;
  daytrading_buying_power?: string;
  cash: string;
  portfolio_value: string;
  equity: string;
  last_equity?: string;
  long_market_value?: string;
  short_market_value?: string;
  initial_margin?: string;
  maintenance_margin?: string;
  sma?: string;
  daytrade_count?: number;
  pattern_day_trader?: boolean;
}

export interface AlpacaPosition {
  asset_id: string;
  symbol: string;
  exchange: string;
  asset_class: string;
  avg_entry_price: string;
  qty: string;
  side: 'long' | 'short';
  market_value: string;
  cost_basis: string;
  unrealized_pl: string;
  unrealized_plpc: string;
  unrealized_intraday_pl?: string;
  unrealized_intraday_plpc?: string;
  current_price: string;
  lastday_price?: string;
  change_today?: string;
}

export interface AlpacaOrder {
  id: string;
  client_order_id: string;
  created_at: string;
  updated_at?: string;
  submitted_at?: string;
  filled_at?: string | null;
  expired_at?: string | null;
  canceled_at?: string | null;
  failed_at?: string | null;
  asset_id?: string;
  symbol: string;
  asset_class?: string;
  notional?: string | null;
  qty: string;
  filled_qty: string;
  filled_avg_price?: string | null;
  order_class?: string;
  order_type?: string;
  type: 'market' | 'limit' | 'stop' | 'stop_limit' | 'trailing_stop';
  side: 'buy' | 'sell';
  time_in_force: 'day' | 'gtc' | 'opg' | 'cls' | 'ioc' | 'fok';
  limit_price?: string | null;
  stop_price?: string | null;
  status:
    | 'new'
    | 'partially_filled'
    | 'filled'
    | 'done_for_day'
    | 'canceled'
    | 'expired'
    | 'replaced'
    | 'pending_cancel'
    | 'pending_replace'
    | 'accepted'
    | 'pending_new'
    | 'accepted_for_bidding'
    | 'stopped'
    | 'rejected'
    | 'suspended'
    | 'calculated';
  extended_hours?: boolean;
}

export interface AlpacaAsset {
  id: string;
  class: string;
  exchange: string;
  symbol: string;
  name: string;
  status: 'active' | 'inactive';
  tradable: boolean;
  marginable?: boolean;
  shortable?: boolean;
  fractionable?: boolean;
}

export interface CandleBar {
  t: string | number; // timestamp RFC3339 or epoch ms
  o: number; // open
  h: number; // high
  l: number; // low
  c: number; // close
  v: number; // volume
  n?: number; // trade count
  vw?: number; // vwap
}

export interface CreateOrderParams {
  symbol: string;
  qty: number | string;
  side: 'buy' | 'sell';
  type: 'market' | 'limit' | 'stop' | 'stop_limit';
  time_in_force?: 'day' | 'gtc';
  limit_price?: number | string;
  stop_price?: number | string;
  client_order_id?: string;
}

/* ==================== CREDENTIALS LIFECYCLE ==================== */

export async function getAlpacaStatus(): Promise<AlpacaConnectionStatus> {
  const res = await fetch('/api/paper/alpaca/status', {
    method: 'GET',
    credentials: 'same-origin',
    cache: 'no-store',
  });
  if (!res.ok) {
    return { connected: false };
  }
  const data = await res.json().catch(() => ({ connected: false }));
  return {
    connected: Boolean(data.connected),
    keyIdHint: data.keyIdHint || data.hint,
    updatedAt: data.updatedAt,
  };
}

export async function connectAlpaca(
  keyId: string,
  secretKey: string
): Promise<{ ok: boolean; error?: string; keyIdHint?: string; account?: AlpacaAccount }> {
  const res = await fetch('/api/paper/alpaca/connect', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ keyId: keyId.trim(), secretKey: secretKey.trim() }),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok) {
    return { ok: false, error: data.error || 'Failed to connect Alpaca credentials.' };
  }
  return { ok: true, keyIdHint: data.keyIdHint || data.hint, account: data.account };
}

export async function disconnectAlpaca(): Promise<{ ok: boolean; error?: string }> {
  const res = await fetch('/api/paper/alpaca/disconnect', {
    method: 'POST',
    credentials: 'same-origin',
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok) {
    return { ok: false, error: data.error || 'Failed to disconnect Alpaca account.' };
  }
  return { ok: true };
}

/* ==================== BROKER OPERATIONS ==================== */

async function engineRequest<T>(subpath: string, init?: RequestInit): Promise<T> {
  const clean = subpath.replace(/^\/+/, '');
  const res = await fetch(`/api/engine/api/v1/alpaca/${clean}`, {
    ...init,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: {
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...(init?.headers || {}),
    },
  });

  if (!res.ok) {
    let msg = `Alpaca request failed (${res.status})`;
    try {
      const err = await res.json();
      if (err.message) msg = err.message;
      else if (err.detail) msg = err.detail;
    } catch {
      // non-JSON body
    }
    throw new Error(msg);
  }

  // Handle 204 No Content
  if (res.status === 204) {
    return null as unknown as T;
  }

  return res.json();
}

export async function getAlpacaAccount(): Promise<AlpacaAccount> {
  return engineRequest<AlpacaAccount>('account');
}

export async function getAlpacaPositions(): Promise<AlpacaPosition[]> {
  const res = await engineRequest<AlpacaPosition[]>('positions');
  return Array.isArray(res) ? res : [];
}

export async function getAlpacaOrders(status = 'all', limit = 100): Promise<AlpacaOrder[]> {
  const res = await engineRequest<AlpacaOrder[]>(`orders?status=${status}&limit=${limit}`);
  return Array.isArray(res) ? res : [];
}

export interface RealtimeQuote {
  symbol: string;
  price: number;
  change: number;
  changePct: number;
  high: number;
  low: number;
  volume: number;
  source: string;
  timestamp: number;
}

export function isCryptoPair(symbol: string): boolean {
  const s = symbol.toUpperCase().trim();
  return (
    s.includes('/') ||
    s.endsWith('USD') ||
    s.endsWith('USDT') ||
    ['SOL', 'BTC', 'ETH', 'DOGE', 'AVAX', 'LINK', 'ADA', 'LTC', 'SHIB', 'UNI', 'DOT', 'NEAR'].includes(s)
  );
}

export async function createAlpacaOrder(params: CreateOrderParams): Promise<AlpacaOrder> {
  const sym = params.symbol.toUpperCase().trim();
  const isCrypto = isCryptoPair(sym);
  return engineRequest<AlpacaOrder>('orders', {
    method: 'POST',
    body: JSON.stringify({
      symbol: sym,
      qty: String(params.qty),
      side: params.side,
      type: params.type,
      time_in_force: isCrypto ? 'gtc' : params.time_in_force || 'day',
      limit_price: params.limit_price !== undefined ? String(params.limit_price) : undefined,
      stop_price: params.stop_price !== undefined ? String(params.stop_price) : undefined,
      client_order_id: params.client_order_id || `sl-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
    }),
  });
}

export async function cancelAlpacaOrder(orderId: string): Promise<boolean> {
  await engineRequest<void>(`orders/${encodeURIComponent(orderId)}`, {
    method: 'DELETE',
  });
  return true;
}

export async function getAlpacaAssets(): Promise<AlpacaAsset[]> {
  try {
    const [equities, cryptos] = await Promise.allSettled([
      engineRequest<AlpacaAsset[]>('assets?status=active&asset_class=us_equity'),
      engineRequest<AlpacaAsset[]>('assets?status=active&asset_class=crypto'),
    ]);
    const list: AlpacaAsset[] = [];
    if (equities.status === 'fulfilled' && Array.isArray(equities.value)) {
      list.push(...equities.value);
    }
    if (cryptos.status === 'fulfilled' && Array.isArray(cryptos.value)) {
      list.push(...cryptos.value);
    }
    return list;
  } catch {
    return [];
  }
}

export function computeAlpacaStartTime(timeframe = '1Day', limit = 100): string {
  const now = Date.now();
  let ms: number;
  switch (timeframe) {
    case '1Min':
      ms = Math.max(limit * 60 * 1000 * 2, 4 * 86_400_000);
      break;
    case '5Min':
      ms = Math.max(limit * 5 * 60 * 1000 * 2, 7 * 86_400_000);
      break;
    case '15Min':
      ms = Math.max(limit * 15 * 60 * 1000 * 2, 14 * 86_400_000);
      break;
    case '1Hour':
      ms = Math.max(limit * 60 * 60 * 1000 * 2, 45 * 86_400_000);
      break;
    case '1Day':
      ms = Math.max(limit * 86_400_000 * 1.8, 180 * 86_400_000);
      break;
    case '1Week':
      ms = Math.max(limit * 7 * 86_400_000 * 1.5, 730 * 86_400_000);
      break;
    case '1Month':
      ms = Math.max(limit * 30 * 86_400_000 * 1.5, 1825 * 86_400_000);
      break;
    default:
      ms = 180 * 86_400_000;
  }
  return new Date(now - ms).toISOString();
}

export async function getRealtimeQuote(symbol: string): Promise<RealtimeQuote | null> {
  try {
    const res = await fetch(`/api/paper/market/quote?symbol=${encodeURIComponent(symbol.toUpperCase().trim())}`, {
      method: 'GET',
      credentials: 'same-origin',
      cache: 'no-store',
    });
    if (res.ok) {
      const data = await res.json();
      if (data && data.ok) {
        return data as RealtimeQuote;
      }
    }
  } catch {
    // ignore
  }
  return null;
}

export async function getAlpacaBars(
  symbol: string,
  timeframe = '1Day',
  limit = 100
): Promise<{ bars: CandleBar[]; symbol: string; source: string }> {
  const sym = symbol.toUpperCase().trim();
  const isCrypto = isCryptoPair(sym);
  const start = computeAlpacaStartTime(timeframe, limit);

  // 1. Try Alpaca Data API
  try {
    const endpoint = isCrypto
      ? `crypto/bars?symbols=${encodeURIComponent(sym)}&timeframe=${timeframe}&limit=${limit}&start=${encodeURIComponent(start)}&sort=asc`
      : `stocks/${encodeURIComponent(sym)}/bars?timeframe=${timeframe}&limit=${limit}&start=${encodeURIComponent(start)}&feed=iex&sort=asc`;

    const res = await engineRequest<{ bars?: Record<string, any[]> | any[] }>(endpoint);

    let rawList: any[] = [];
    if (res && res.bars) {
      if (Array.isArray(res.bars)) {
        rawList = res.bars;
      } else if (res.bars[sym]) {
        rawList = res.bars[sym];
      }
    }

    if (rawList && rawList.length >= 25) {
      const bars: CandleBar[] = rawList.map((b) => ({
        t: b.t,
        o: Number(b.o),
        h: Number(b.h),
        l: Number(b.l),
        c: Number(b.c),
        v: Number(b.v),
        n: b.n ? Number(b.n) : undefined,
        vw: b.vw ? Number(b.vw) : undefined,
      }));
      return { bars, symbol: sym, source: isCrypto ? 'Alpaca Crypto' : 'Alpaca Paper (IEX Feed)' };
    }
  } catch (err) {
    console.warn(`[Alpaca] Direct bars request for ${sym} failed or returned sparse data, querying real-time market service:`, err);
  }

  // 2. Query Dedicated Real-Time Market Data Provider (Coinbase 24/7 for Crypto, Yahoo Finance for Stocks)
  try {
    const res = await fetch(
      `/api/paper/market/bars?symbol=${encodeURIComponent(sym)}&timeframe=${encodeURIComponent(timeframe)}&limit=${limit}`,
      {
        method: 'GET',
        credentials: 'same-origin',
        cache: 'no-store',
      }
    );
    if (res.ok) {
      const data = await res.json();
      if (data && data.ok && Array.isArray(data.bars) && data.bars.length > 0) {
        return {
          bars: data.bars,
          symbol: data.symbol || sym,
          source: data.source || (isCrypto ? 'Coinbase Pro (24/7 Feed)' : 'Real-Time Market Feed'),
        };
      }
    }
  } catch (marketErr) {
    console.warn(`[Market API] Fallback bars request for ${sym} failed:`, marketErr);
  }

  return { bars: [], symbol: sym, source: 'unavailable' };
}
