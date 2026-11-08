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
