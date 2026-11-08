/**
 * Indicator Registry and API client for M35 Charting & Indicators.
 *
 * Implements client-side definition, parameter schemas, defaults, and API client
 * for all 25 technical indicators specified in docs/models/m35-indicators-spec.md.
 */

import { request } from './api';

export type IndicatorCategory = 'overlay' | 'subpane';

export type IndicatorName =
  | 'SMA'
  | 'EMA'
  | 'WMA'
  | 'DEMA'
  | 'TEMA'
  | 'BB'
  | 'ICHIMOKU'
  | 'PSAR'
  | 'DONCHIAN'
  | 'KELTNER'
  | 'VWAP'
  | 'TYPICAL'
  | 'MEDIAN'
  | 'RSI'
  | 'MACD'
  | 'STOCH'
  | 'CCI'
  | 'WILLR'
  | 'MOM'
  | 'ROC'
  | 'ATR'
  | 'ADX'
  | 'OBV'
  | 'CMF'
  | 'MFI'
  | 'STDDEV';

export interface ParamField {
  name: string;
  label: string;
  type: 'integer' | 'float';
  default: number;
  min?: number;
  max?: number;
  step?: number;
}

export interface IndicatorMeta {
  id: IndicatorName;
  name: string;
  shortName: string;
  category: IndicatorCategory;
  description: string;
  params: ParamField[];
  defaultParams: Record<string, number>;
  defaultColor: string;
  outputs: string[];
}

/**
 * Core 12 technical indicators prioritized in picker (Decision 2).
 * 8 Overlays + 4 Sub-Panes.
 */
export const CORE_INDICATOR_IDS: IndicatorName[] = [
  'SMA',
  'EMA',
  'BB',
  'VWAP',
  'ICHIMOKU',
  'PSAR',
  'DONCHIAN',
  'KELTNER',
  'RSI',
  'MACD',
  'STOCH',
  'ATR',
];

export const INDICATOR_REGISTRY: Record<IndicatorName, IndicatorMeta> = {
  SMA: {
    id: 'SMA',
    name: 'Simple Moving Average',
    shortName: 'SMA',
    category: 'overlay',
    description: 'Unweighted arithmetic mean of closing prices over n bars.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#3B82F6',
    outputs: ['values'],
  },
  EMA: {
    id: 'EMA',
    name: 'Exponential Moving Average',
    shortName: 'EMA',
    category: 'overlay',
    description: 'Weighted moving average giving exponentially greater weight to recent bars.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#F59E0B',
    outputs: ['values'],
  },
  WMA: {
    id: 'WMA',
    name: 'Weighted Moving Average',
    shortName: 'WMA',
    category: 'overlay',
    description: 'Linearly weighted moving average with weights increasing toward recent bars.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#8B5CF6',
    outputs: ['values'],
  },
  DEMA: {
    id: 'DEMA',
    name: 'Double Exponential Moving Average',
    shortName: 'DEMA',
    category: 'overlay',
    description: 'Zero-lag combination of single and double smoothed EMA.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#EC4899',
    outputs: ['values'],
  },
  TEMA: {
    id: 'TEMA',
    name: 'Triple Exponential Moving Average',
    shortName: 'TEMA',
    category: 'overlay',
    description: 'Ultra-low-lag triple smoothed exponential moving average.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#06B6D4',
    outputs: ['values'],
  },
  BB: {
    id: 'BB',
    name: 'Bollinger Bands',
    shortName: 'BB',
    category: 'overlay',
    description: 'Volatility envelope around an SMA with upper and lower standard deviation bands.',
    params: [
      { name: 'period', label: 'Period', type: 'integer', default: 20, min: 2, max: 500, step: 1 },
      { name: 'mult', label: 'Multiplier', type: 'float', default: 2.0, min: 0.1, max: 10.0, step: 0.1 },
    ],
    defaultParams: { period: 20, mult: 2.0 },
    defaultColor: '#6366F1',
    outputs: ['upper', 'middle', 'lower', 'percent_b', 'bandwidth'],
  },
  ICHIMOKU: {
    id: 'ICHIMOKU',
    name: 'Ichimoku Kinko Hyo',
    shortName: 'Ichimoku',
    category: 'overlay',
    description: 'Equilibrium chart indicator combining trend, momentum, and support/resistance cloud.',
    params: [
      { name: 'tenkan', label: 'Conversion (Tenkan)', type: 'integer', default: 9, min: 1, max: 200, step: 1 },
      { name: 'kijun', label: 'Base Line (Kijun)', type: 'integer', default: 26, min: 1, max: 200, step: 1 },
      { name: 'span_b', label: 'Leading Span B', type: 'integer', default: 52, min: 1, max: 500, step: 1 },
    ],
    defaultParams: { tenkan: 9, kijun: 26, span_b: 52 },
    defaultColor: '#10B981',
    outputs: ['tenkan_sen', 'kijun_sen', 'senkou_span_a', 'senkou_span_b', 'chikou_span'],
  },
  PSAR: {
    id: 'PSAR',
    name: 'Parabolic SAR',
    shortName: 'PSAR',
    category: 'overlay',
    description: 'Stop-and-reverse trailing indicator showing price acceleration dots.',
    params: [
      { name: 'start', label: 'Start AF', type: 'float', default: 0.02, min: 0.001, max: 1.0, step: 0.01 },
      { name: 'step', label: 'Step AF', type: 'float', default: 0.02, min: 0.001, max: 1.0, step: 0.01 },
      { name: 'max', label: 'Max AF', type: 'float', default: 0.2, min: 0.01, max: 2.0, step: 0.05 },
    ],
    defaultParams: { start: 0.02, step: 0.02, max: 0.2 },
    defaultColor: '#EAB308',
    outputs: ['values'],
  },
  DONCHIAN: {
    id: 'DONCHIAN',
    name: 'Donchian Channels',
    shortName: 'Donchian',
    category: 'overlay',
    description: 'Channel formed by the highest high and lowest low over the lookback period.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#14B8A6',
    outputs: ['upper', 'middle', 'lower'],
  },
  KELTNER: {
    id: 'KELTNER',
    name: 'Keltner Channels',
    shortName: 'Keltner',
    category: 'overlay',
    description: 'Volatility-based channel around an EMA with Average True Range bands.',
    params: [
      { name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 500, step: 1 },
      { name: 'mult', label: 'Multiplier', type: 'float', default: 1.5, min: 0.1, max: 10.0, step: 0.1 },
    ],
    defaultParams: { period: 20, mult: 1.5 },
    defaultColor: '#F97316',
    outputs: ['upper', 'middle', 'lower'],
  },
  VWAP: {
    id: 'VWAP',
    name: 'Volume-Weighted Average Price',
    shortName: 'VWAP',
    category: 'overlay',
    description: 'Cumulative volume-weighted price representing the true average traded price.',
    params: [],
    defaultParams: {},
    defaultColor: '#A855F7',
    outputs: ['values'],
  },
  TYPICAL: {
    id: 'TYPICAL',
    name: 'Typical Price',
    shortName: 'Typical',
    category: 'overlay',
    description: 'Mean of High, Low, and Close for each bar: (H + L + C) / 3.',
    params: [],
    defaultParams: {},
    defaultColor: '#64748B',
    outputs: ['values'],
  },
  MEDIAN: {
    id: 'MEDIAN',
    name: 'Median Price',
    shortName: 'Median',
    category: 'overlay',
    description: 'Midpoint of High and Low for each bar: (H + L) / 2.',
    params: [],
    defaultParams: {},
    defaultColor: '#94A3B8',
    outputs: ['values'],
  },

  // Sub-pane indicators
  RSI: {
    id: 'RSI',
    name: 'Relative Strength Index',
    shortName: 'RSI',
    category: 'subpane',
    description: 'Momentum oscillator measuring the speed and change of price movements between 0 and 100.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 14, min: 1, max: 500, step: 1 }],
    defaultParams: { period: 14 },
    defaultColor: '#8B5CF6',
    outputs: ['values'],
  },
  MACD: {
    id: 'MACD',
    name: 'Moving Average Convergence Divergence',
    shortName: 'MACD',
    category: 'subpane',
    description: 'Trend-following momentum indicator showing relationship between two moving averages.',
    params: [
      { name: 'fast', label: 'Fast Period', type: 'integer', default: 12, min: 1, max: 200, step: 1 },
      { name: 'slow', label: 'Slow Period', type: 'integer', default: 26, min: 1, max: 200, step: 1 },
      { name: 'signal', label: 'Signal Period', type: 'integer', default: 9, min: 1, max: 200, step: 1 },
    ],
    defaultParams: { fast: 12, slow: 26, signal: 9 },
    defaultColor: '#3B82F6',
    outputs: ['macd', 'signal', 'histogram'],
  },
  STOCH: {
    id: 'STOCH',
    name: 'Stochastic Oscillator',
    shortName: 'Stoch',
    category: 'subpane',
    description: 'Location of the close relative to the high-low range over a given period (%K, %D).',
    params: [
      { name: 'k', label: '%K Period', type: 'integer', default: 14, min: 1, max: 200, step: 1 },
      { name: 'd', label: '%D Period', type: 'integer', default: 3, min: 1, max: 200, step: 1 },
      { name: 'slow_d', label: 'Slowing', type: 'integer', default: 3, min: 1, max: 200, step: 1 },
    ],
    defaultParams: { k: 14, d: 3, slow_d: 3 },
    defaultColor: '#10B981',
    outputs: ['k', 'd'],
  },
  ATR: {
    id: 'ATR',
    name: 'Average True Range',
    shortName: 'ATR',
    category: 'subpane',
    description: 'Technical analysis volatility indicator measuring market volatility over n periods.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 14, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 14 },
    defaultColor: '#EF4444',
    outputs: ['values'],
  },
  ADX: {
    id: 'ADX',
    name: 'Average Directional Index',
    shortName: 'ADX',
    category: 'subpane',
    description: 'Measures trend strength regardless of trend direction, along with +DI and -DI lines.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 14, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 14 },
    defaultColor: '#F59E0B',
    outputs: ['adx', 'plus_di', 'minus_di'],
  },
  OBV: {
    id: 'OBV',
    name: 'On-Balance Volume',
    shortName: 'OBV',
    category: 'subpane',
    description: 'Cumulative volume indicator using volume flow to predict changes in stock price.',
    params: [],
    defaultParams: {},
    defaultColor: '#06B6D4',
    outputs: ['values'],
  },
  CCI: {
    id: 'CCI',
    name: 'Commodity Channel Index',
    shortName: 'CCI',
    category: 'subpane',
    description: 'Versatile indicator that can be used to identify a new trend or warn of extreme conditions.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#EC4899',
    outputs: ['values'],
  },
  WILLR: {
    id: 'WILLR',
    name: 'Williams %R',
    shortName: 'Williams %R',
    category: 'subpane',
    description: 'Momentum indicator that moves between 0 and -100 to gauge overbought and oversold levels.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 14, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 14 },
    defaultColor: '#F97316',
    outputs: ['values'],
  },
  CMF: {
    id: 'CMF',
    name: 'Chaikin Money Flow',
    shortName: 'CMF',
    category: 'subpane',
    description: 'Measures the amount of Money Flow Volume over a specific period (typically 20 or 21 days).',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#14B8A6',
    outputs: ['values'],
  },
  MOM: {
    id: 'MOM',
    name: 'Momentum',
    shortName: 'MOM',
    category: 'subpane',
    description: 'Absolute difference between current price and price n bars ago.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 10, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 10 },
    defaultColor: '#3B82F6',
    outputs: ['values'],
  },
  ROC: {
    id: 'ROC',
    name: 'Rate of Change',
    shortName: 'ROC',
    category: 'subpane',
    description: 'Percentage change between current price and price n bars ago.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 12, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 12 },
    defaultColor: '#6366F1',
    outputs: ['values'],
  },
  MFI: {
    id: 'MFI',
    name: 'Money Flow Index',
    shortName: 'MFI',
    category: 'subpane',
    description: 'Volume-weighted RSI that measures the inflow and outflow of money over a specified period.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 14, min: 1, max: 200, step: 1 }],
    defaultParams: { period: 14 },
    defaultColor: '#10B981',
    outputs: ['values'],
  },
  STDDEV: {
    id: 'STDDEV',
    name: 'Rolling Standard Deviation',
    shortName: 'StdDev',
    category: 'subpane',
    description: 'Sample standard deviation of closing prices measuring statistical dispersion.',
    params: [{ name: 'period', label: 'Period', type: 'integer', default: 20, min: 2, max: 200, step: 1 }],
    defaultParams: { period: 20 },
    defaultColor: '#EAB308',
    outputs: ['values'],
  },
};

export const CORE_12_INDICATORS: IndicatorName[] = [
  'SMA',
  'EMA',
  'BB',
  'VWAP',
  'ICHIMOKU',
  'PSAR',
  'RSI',
  'MACD',
  'STOCH',
  'ATR',
  'ADX',
  'OBV',
];

export interface ActiveIndicator {
  instanceId: string;
  indicator: IndicatorName;
  params: Record<string, number>;
  color: string;
  visible: boolean;
}

export interface IndicatorSeriesResponse {
  symbol: string;
  indicator: string;
  timeframe: string;
  parameters: Record<string, number | string>;
  values?: (number | null)[];
  series?: Record<string, (number | null)[]>;
}

/**
 * Fetch indicator aligned series from the engine.
 */
export async function fetchIndicatorSeries(options: {
  dataset_version_id: string;
  symbol: string;
  timeframe: string;
  indicator: IndicatorName;
  params?: Record<string, number>;
  signal?: AbortSignal;
}): Promise<IndicatorSeriesResponse> {
  const query = new URLSearchParams({
    dataset_version_id: options.dataset_version_id,
    symbol: options.symbol,
    timeframe: options.timeframe,
    indicator: options.indicator,
  });

  if (options.params) {
    for (const [k, v] of Object.entries(options.params)) {
      query.set(k, String(v));
    }
  }

  return request<IndicatorSeriesResponse>(`/api/v1/indicators?${query.toString()}`, {
    method: 'GET',
    timeoutMs: 30_000,
    signal: options.signal,
  });
}

export interface BarInput {
  o: number;
  h: number;
  l: number;
  c: number;
  v?: number;
}

/**
 * Client-side calculation of aligned indicator series on OHLCV bars.
 * Used for live Alpaca charts and instant interactive preview.
 */
export function computeIndicatorSeries(
  indicator: IndicatorName | string,
  bars: BarInput[],
  customParams?: Record<string, number>
): { values?: (number | null)[]; series?: Record<string, (number | null)[]> } {
  const n = bars.length;
  if (n === 0) return {};

  const ind = indicator.toUpperCase() as IndicatorName;
  const meta = INDICATOR_REGISTRY[ind];
  const p = { ...(meta?.defaultParams || {}), ...(customParams || {}) };

  const closes = bars.map((b) => b.c);
  const highs = bars.map((b) => b.h);
  const lows = bars.map((b) => b.l);
  const volumes = bars.map((b) => b.v ?? 0);

  // Helper: Simple Moving Average
  const calcSMA = (data: number[], period: number): (number | null)[] => {
    const res: (number | null)[] = new Array(data.length).fill(null);
    let sum = 0;
    for (let i = 0; i < data.length; i++) {
      sum += data[i];
      if (i >= period) sum -= data[i - period];
      if (i >= period - 1) res[i] = sum / period;
    }
    return res;
  };

  // Helper: Exponential Moving Average
  const calcEMA = (data: number[], period: number): (number | null)[] => {
    const res: (number | null)[] = new Array(data.length).fill(null);
    if (data.length < period) return res;
    let sum = 0;
    for (let i = 0; i < period; i++) sum += data[i];
    let prev = sum / period;
    res[period - 1] = prev;
    const k = 2 / (period + 1);
    for (let i = period; i < data.length; i++) {
      prev = (data[i] - prev) * k + prev;
      res[i] = prev;
    }
    return res;
  };

  switch (ind) {
    case 'SMA': {
      const period = p.period || 20;
      return { values: calcSMA(closes, period) };
    }
    case 'EMA': {
      const period = p.period || 20;
      return { values: calcEMA(closes, period) };
    }
    case 'WMA': {
      const period = p.period || 20;
      const res: (number | null)[] = new Array(n).fill(null);
      const denom = (period * (period + 1)) / 2;
      for (let i = period - 1; i < n; i++) {
        let sum = 0;
        for (let j = 0; j < period; j++) {
          sum += closes[i - period + 1 + j] * (j + 1);
        }
        res[i] = sum / denom;
      }
      return { values: res };
    }
    case 'BB': {
      const period = p.period || 20;
      const mult = p.mult || 2.0;
      const mid = calcSMA(closes, period);
      const upper: (number | null)[] = new Array(n).fill(null);
      const lower: (number | null)[] = new Array(n).fill(null);
      for (let i = period - 1; i < n; i++) {
        const m = mid[i];
        if (m === null) continue;
        let variance = 0;
        for (let j = 0; j < period; j++) {
          const diff = closes[i - j] - m;
          variance += diff * diff;
        }
        const std = Math.sqrt(variance / period);
        upper[i] = m + mult * std;
        lower[i] = m - mult * std;
      }
      return { series: { middle: mid, upper, lower } };
    }
    case 'VWAP': {
      const res: (number | null)[] = new Array(n).fill(null);
      let cumVwap = 0;
      let cumVol = 0;
      for (let i = 0; i < n; i++) {
        const typ = (highs[i] + lows[i] + closes[i]) / 3;
        const v = volumes[i] > 0 ? volumes[i] : 1;
        cumVwap += typ * v;
        cumVol += v;
        res[i] = cumVwap / cumVol;
      }
      return { values: res };
    }
    case 'ICHIMOKU': {
      const tenkanP = p.tenkan_period || 9;
      const kijunP = p.kijun_period || 26;
      const senkouBP = p.senkou_b_period || 52;
      const tenkan: (number | null)[] = new Array(n).fill(null);
      const kijun: (number | null)[] = new Array(n).fill(null);
      const senkouA: (number | null)[] = new Array(n).fill(null);
      const senkouB: (number | null)[] = new Array(n).fill(null);
      const chikou: (number | null)[] = new Array(n).fill(null);

      const rangeMid = (start: number, end: number) => {
        let hi = -Infinity;
        let lo = Infinity;
        for (let i = start; i <= end; i++) {
          if (highs[i] > hi) hi = highs[i];
          if (lows[i] < lo) lo = lows[i];
        }
        return (hi + lo) / 2;
      };

      for (let i = 0; i < n; i++) {
        if (i >= tenkanP - 1) tenkan[i] = rangeMid(i - tenkanP + 1, i);
        if (i >= kijunP - 1) kijun[i] = rangeMid(i - kijunP + 1, i);
        if (tenkan[i] !== null && kijun[i] !== null) {
          senkouA[i] = (tenkan[i]! + kijun[i]!) / 2;
        }
        if (i >= senkouBP - 1) senkouB[i] = rangeMid(i - senkouBP + 1, i);
        chikou[i] = closes[i];
      }
      return {
        series: {
          tenkan_sen: tenkan,
          kijun_sen: kijun,
          senkou_span_a: senkouA,
          senkou_span_b: senkouB,
          chikou_span: chikou,
        },
      };
    }
    case 'PSAR': {
      const afStep = p.af_step || 0.02;
      const afMax = p.af_max || 0.2;
      const res: (number | null)[] = new Array(n).fill(null);
      if (n < 2) return { values: res };
      let isLong = closes[1] >= closes[0];
      let sar = isLong ? lows[0] : highs[0];
      let ep = isLong ? highs[1] : lows[1];
      let af = afStep;
      res[0] = sar;
      for (let i = 1; i < n; i++) {
        sar = sar + af * (ep - sar);
        if (isLong) {
          if (i >= 1 && sar > lows[i - 1]) sar = lows[i - 1];
          if (i >= 2 && sar > lows[i - 2]) sar = lows[i - 2];
          if (lows[i] < sar) {
            isLong = false;
            sar = ep;
            ep = lows[i];
            af = afStep;
          } else {
            if (highs[i] > ep) {
              ep = highs[i];
              af = Math.min(af + afStep, afMax);
            }
          }
        } else {
          if (i >= 1 && sar < highs[i - 1]) sar = highs[i - 1];
          if (i >= 2 && sar < highs[i - 2]) sar = highs[i - 2];
          if (highs[i] > sar) {
            isLong = true;
            sar = ep;
            ep = highs[i];
            af = afStep;
          } else {
            if (lows[i] < ep) {
              ep = lows[i];
              af = Math.min(af + afStep, afMax);
            }
          }
        }
        res[i] = sar;
      }
      return { values: res };
    }
    case 'DONCHIAN': {
      const period = p.period || 20;
      const upper: (number | null)[] = new Array(n).fill(null);
      const lower: (number | null)[] = new Array(n).fill(null);
      const middle: (number | null)[] = new Array(n).fill(null);
      for (let i = period - 1; i < n; i++) {
        let hi = -Infinity;
        let lo = Infinity;
        for (let j = 0; j < period; j++) {
          if (highs[i - j] > hi) hi = highs[i - j];
          if (lows[i - j] < lo) lo = lows[i - j];
        }
        upper[i] = hi;
        lower[i] = lo;
        middle[i] = (hi + lo) / 2;
      }
      return { series: { upper, lower, middle } };
    }
    case 'KELTNER': {
      const period = p.period || 20;
      const mult = p.mult || 2.0;
      const mid = calcEMA(closes, period);
      // ATR
      const tr: number[] = new Array(n);
      tr[0] = highs[0] - lows[0];
      for (let i = 1; i < n; i++) {
        tr[i] = Math.max(
          highs[i] - lows[i],
          Math.abs(highs[i] - closes[i - 1]),
          Math.abs(lows[i] - closes[i - 1])
        );
      }
      const atr = calcEMA(tr, period);
      const upper: (number | null)[] = new Array(n).fill(null);
      const lower: (number | null)[] = new Array(n).fill(null);
      for (let i = 0; i < n; i++) {
        if (mid[i] !== null && atr[i] !== null) {
          upper[i] = mid[i]! + mult * atr[i]!;
          lower[i] = mid[i]! - mult * atr[i]!;
        }
      }
      return { series: { middle: mid, upper, lower } };
    }
    case 'RSI': {
      const period = p.period || 14;
      const res: (number | null)[] = new Array(n).fill(null);
      if (n <= period) return { values: res };
      let gainSum = 0;
      let lossSum = 0;
      for (let i = 1; i <= period; i++) {
        const diff = closes[i] - closes[i - 1];
        if (diff >= 0) gainSum += diff;
        else lossSum -= diff;
      }
      let avgGain = gainSum / period;
      let avgLoss = lossSum / period;
      res[period] = avgLoss === 0 ? 100 : 100 - 100 / (1 + avgGain / avgLoss);
      for (let i = period + 1; i < n; i++) {
        const diff = closes[i] - closes[i - 1];
        const gain = diff >= 0 ? diff : 0;
        const loss = diff < 0 ? -diff : 0;
        avgGain = (avgGain * (period - 1) + gain) / period;
        avgLoss = (avgLoss * (period - 1) + loss) / period;
        res[i] = avgLoss === 0 ? 100 : 100 - 100 / (1 + avgGain / avgLoss);
      }
      return { values: res };
    }
    case 'MACD': {
      const fastP = p.fast_period || 12;
      const slowP = p.slow_period || 26;
      const sigP = p.signal_period || 9;
      const fastEma = calcEMA(closes, fastP);
      const slowEma = calcEMA(closes, slowP);
      const macdLine: (number | null)[] = new Array(n).fill(null);
      const macdValues: number[] = [];
      const macdIndices: number[] = [];
      for (let i = 0; i < n; i++) {
        if (fastEma[i] !== null && slowEma[i] !== null) {
          const val = fastEma[i]! - slowEma[i]!;
          macdLine[i] = val;
          macdValues.push(val);
          macdIndices.push(i);
        }
      }
      const sigEma = calcEMA(macdValues, sigP);
      const sigLine: (number | null)[] = new Array(n).fill(null);
      const hist: (number | null)[] = new Array(n).fill(null);
      for (let j = 0; j < macdValues.length; j++) {
        const idx = macdIndices[j];
        if (sigEma[j] !== null) {
          sigLine[idx] = sigEma[j];
          hist[idx] = macdLine[idx]! - sigEma[j]!;
        }
      }
      return { series: { macd: macdLine, signal: sigLine, histogram: hist } };
    }
    case 'STOCH': {
      const kP = p.k_period || 14;
      const dP = p.d_period || 3;
      const slowP = p.slowing || 3;
      const rawK: number[] = [];
      const kIndices: number[] = [];
      for (let i = kP - 1; i < n; i++) {
        let hi = -Infinity;
        let lo = Infinity;
        for (let j = 0; j < kP; j++) {
          if (highs[i - j] > hi) hi = highs[i - j];
          if (lows[i - j] < lo) lo = lows[i - j];
        }
        const denom = hi - lo;
        const val = denom === 0 ? 50 : ((closes[i] - lo) / denom) * 100;
        rawK.push(val);
        kIndices.push(i);
      }
      const smoothK = calcSMA(rawK, slowP);
      const kLine: (number | null)[] = new Array(n).fill(null);
      const smoothedKValues: number[] = [];
      const smoothKIndices: number[] = [];
      for (let j = 0; j < rawK.length; j++) {
        const idx = kIndices[j];
        if (smoothK[j] !== null) {
          kLine[idx] = smoothK[j];
          smoothedKValues.push(smoothK[j]!);
          smoothKIndices.push(idx);
        }
      }
      const dSmooth = calcSMA(smoothedKValues, dP);
      const dLine: (number | null)[] = new Array(n).fill(null);
      for (let m = 0; m < smoothedKValues.length; m++) {
        const idx = smoothKIndices[m];
        if (dSmooth[m] !== null) dLine[idx] = dSmooth[m];
      }
      return { series: { k: kLine, d: dLine } };
    }
    case 'ATR': {
      const period = p.period || 14;
      const res: (number | null)[] = new Array(n).fill(null);
      if (n === 0) return { values: res };
      const tr: number[] = new Array(n);
      tr[0] = highs[0] - lows[0];
      for (let i = 1; i < n; i++) {
        tr[i] = Math.max(
          highs[i] - lows[i],
          Math.abs(highs[i] - closes[i - 1]),
          Math.abs(lows[i] - closes[i - 1])
        );
      }
      let sum = 0;
      for (let i = 0; i < Math.min(period, n); i++) sum += tr[i];
      if (n >= period) {
        let prev = sum / period;
        res[period - 1] = prev;
        for (let i = period; i < n; i++) {
          prev = (prev * (period - 1) + tr[i]) / period;
          res[i] = prev;
        }
      }
      return { values: res };
    }
    case 'OBV': {
      const res: (number | null)[] = new Array(n).fill(null);
      let obv = 0;
      res[0] = obv;
      for (let i = 1; i < n; i++) {
        const v = volumes[i] || 0;
        if (closes[i] > closes[i - 1]) obv += v;
        else if (closes[i] < closes[i - 1]) obv -= v;
        res[i] = obv;
      }
      return { values: res };
    }
    case 'CCI': {
      const period = p.period || 20;
      const res: (number | null)[] = new Array(n).fill(null);
      const tp = bars.map((b) => (b.h + b.l + b.c) / 3);
      const tpSma = calcSMA(tp, period);
      for (let i = period - 1; i < n; i++) {
        const m = tpSma[i];
        if (m === null) continue;
        let md = 0;
        for (let j = 0; j < period; j++) {
          md += Math.abs(tp[i - j] - m);
        }
        md = md / period;
        res[i] = md === 0 ? 0 : (tp[i] - m) / (0.015 * md);
      }
      return { values: res };
    }
    default: {
      // Fallback: 20-period SMA
      return { values: calcSMA(closes, 20) };
    }
  }
}

