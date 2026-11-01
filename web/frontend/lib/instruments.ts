import { request } from './api';

export type InstrumentType = 'equity' | 'future' | 'bond' | 'option' | 'swap';

export interface InstrumentItem {
  symbol: string;
  type: InstrumentType;
  currency: string;
  multiplier: string;
  tick_size: string;
  lot_size: string;
  expiry?: number | null;
  strike?: string | null;
  maturity?: number | null;
  coupon_rate?: string | null;
  underlying?: string | null;
  coupon_frequency?: number | null;
  day_count?: string | null;
  right?: 'call' | 'put' | null;
}

export interface InstrumentListResponse {
  base_currency: string;
  items: InstrumentItem[];
}

export interface CreateInstrumentPayload {
  symbol: string;
  type?: InstrumentType;
  currency?: string;
  multiplier?: number | string;
  tick_size?: number | string;
  lot_size?: number | string;
  expiry?: string | number;
  strike?: number | string;
  maturity?: string | number;
  coupon_rate?: number | string;
  underlying?: string;
  coupon_frequency?: number;
  day_count?: string;
  right?: 'call' | 'put';
}

/** Lists all instruments registered in the current workspace. */
export async function listInstruments(): Promise<InstrumentListResponse> {
  return request<InstrumentListResponse>('/api/v1/instruments', {
    method: 'GET',
    timeoutMs: 20_000,
  });
}

/** Adds or creates a new multi-asset instrument. */
export async function createInstrument(payload: CreateInstrumentPayload): Promise<InstrumentItem> {
  return request<InstrumentItem>('/api/v1/instruments', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
    timeoutMs: 20_000,
  });
}

/** Deletes an instrument record by symbol. */
export async function deleteInstrument(symbol: string): Promise<{ deleted: boolean }> {
  return request<{ deleted: boolean }>(`/api/v1/instruments/${encodeURIComponent(symbol)}`, {
    method: 'DELETE',
    timeoutMs: 20_000,
  });
}

/** Pre-packaged multi-asset contracts for instant 1-click seeding. */
export const SAMPLE_CONTRACTS: CreateInstrumentPayload[] = [
  {
    symbol: 'ESZ6',
    type: 'future',
    underlying: 'SPX',
    currency: 'USD',
    multiplier: 50,
    tick_size: '0.25',
    expiry: '2026-12-18',
  },
  {
    symbol: 'NQZ6',
    type: 'future',
    underlying: 'NDX',
    currency: 'USD',
    multiplier: 20,
    tick_size: '0.25',
    expiry: '2026-12-18',
  },
  {
    symbol: 'SPY261218C00550000',
    type: 'option',
    underlying: 'SPY',
    right: 'call',
    strike: '550.00',
    currency: 'USD',
    multiplier: 100,
    tick_size: '0.01',
    expiry: '2026-12-18',
  },
  {
    symbol: 'SPY261218P00500000',
    type: 'option',
    underlying: 'SPY',
    right: 'put',
    strike: '500.00',
    currency: 'USD',
    multiplier: 100,
    tick_size: '0.01',
    expiry: '2026-12-18',
  },
  {
    symbol: 'UST10Y',
    type: 'bond',
    currency: 'USD',
    multiplier: 1,
    tick_size: '0.01',
    coupon_rate: '0.0425',
    coupon_frequency: 2,
    day_count: 'act_act_icma',
    maturity: '2034-11-15',
  },
];
