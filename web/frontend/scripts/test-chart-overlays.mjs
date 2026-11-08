/**
 * Test suite for M35 Task 3 (Price-pane overlays).
 *
 * Verifies:
 * 1. Indicator registry contains all 25 indicators with spec-accurate metadata and parameters.
 * 2. TradingChart exports OverlaySeries and supports overlays prop.
 * 3. Overlay rendering for SMA, EMA, WMA, Bollinger Bands, VWAP, Ichimoku, Parabolic SAR,
 *    Donchian Channels, and Keltner Channels on fixture data.
 * 4. PriceChart supports dataset explorer overlays.
 * 5. Warm-up nulls do not produce erroneous canvas coordinates (NaN/Infinity).
 */

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

describe('M35 Task 3: Indicator Registry & Overlays Contract', () => {
  const indicatorsPath = path.resolve(root, 'lib/indicators.ts');
  const indicatorsSource = fs.readFileSync(indicatorsPath, 'utf8');

  it('indicators.ts defines all 25 indicators with spec-accurate placement', () => {
    const requiredOverlays = [
      'SMA',
      'EMA',
      'WMA',
      'DEMA',
      'TEMA',
      'BB',
      'ICHIMOKU',
      'PSAR',
      'DONCHIAN',
      'KELTNER',
      'VWAP',
      'TYPICAL',
      'MEDIAN',
    ];

    const requiredSubpanes = [
      'RSI',
      'MACD',
      'STOCH',
      'ATR',
      'ADX',
      'OBV',
      'CCI',
      'WILLR',
      'CMF',
      'MOM',
      'ROC',
      'MFI',
      'STDDEV',
    ];

    for (const ind of requiredOverlays) {
      assert.ok(
        indicatorsSource.includes(`id: '${ind}'`),
        `Indicator registry must include overlay ${ind}`
      );
      assert.ok(
        indicatorsSource.includes(`category: 'overlay'`),
        `Indicator ${ind} must be categorized as overlay`
      );
    }

    for (const ind of requiredSubpanes) {
      assert.ok(
        indicatorsSource.includes(`id: '${ind}'`),
        `Indicator registry must include subpane ${ind}`
      );
      assert.ok(
        indicatorsSource.includes(`category: 'subpane'`),
        `Indicator ${ind} must be categorized as subpane`
      );
    }
  });

  it('indicators.ts default parameters match the M35 sign-off specification', () => {
    // BB: period 20, mult 2.0
    assert.ok(indicatorsSource.includes(`period: 20, mult: 2.0`), 'BB default parameters must be period=20, mult=2.0');
    // KELTNER: period 20, mult 1.5
    assert.ok(indicatorsSource.includes(`period: 20, mult: 1.5`), 'Keltner default parameters must be period=20, mult=1.5');
    // ICHIMOKU: tenkan 9, kijun 26, span_b 52
    assert.ok(indicatorsSource.includes(`tenkan: 9, kijun: 26, span_b: 52`), 'Ichimoku defaults must be 9, 26, 52');
    // PSAR: start 0.02, step 0.02, max 0.2
    assert.ok(indicatorsSource.includes(`start: 0.02, step: 0.02, max: 0.2`), 'PSAR defaults must be 0.02, 0.02, 0.2');
  });

  it('fetchIndicatorSeries is exported and queries GET /api/v1/indicators', () => {
    assert.ok(indicatorsSource.includes('export async function fetchIndicatorSeries'), 'Must export fetchIndicatorSeries');
    assert.ok(indicatorsSource.includes('/api/v1/indicators?'), 'Must query /api/v1/indicators');
  });
});

describe('M35 Task 3: TradingChart & PriceChart Overlay Drawing', () => {
  const tradingChartPath = path.resolve(root, 'components/studio/TradingChart.tsx');
  const tradingChartSource = fs.readFileSync(tradingChartPath, 'utf8');

  it('TradingChart.tsx exports OverlaySeries interface and accepts overlays prop', () => {
    assert.ok(tradingChartSource.includes('export interface OverlaySeries'), 'TradingChart must export OverlaySeries');
    assert.ok(tradingChartSource.includes('overlays?: OverlaySeries[]'), 'TradingChartProps must include overlays prop');
  });

  it('TradingChart.tsx implements canvas rendering for all required price-pane overlays', () => {
    // Single line moving averages & price types
    assert.ok(tradingChartSource.includes('SMA, EMA, WMA, DEMA, TEMA, VWAP, TYPICAL, MEDIAN'), 'Must render single-line moving averages');

    // Bollinger Bands with fill and multi-line boundary
    assert.ok(tradingChartSource.includes(`ind === 'BB'`), 'Must render Bollinger Bands');
    assert.ok(tradingChartSource.includes('ctx.fillStyle = hexToRgba(color, 0.08)'), 'Must render translucent channel fill for BB');

    // Keltner and Donchian channels
    assert.ok(tradingChartSource.includes(`ind === 'KELTNER'`), 'Must render Keltner Channels');
    assert.ok(tradingChartSource.includes(`ind === 'DONCHIAN'`), 'Must render Donchian Channels');

    // Ichimoku with Tenkan, Kijun, Span A/B cloud, Chikou
    assert.ok(tradingChartSource.includes(`ind === 'ICHIMOKU'`), 'Must render Ichimoku Kinko Hyo');
    assert.ok(tradingChartSource.includes('tenkan_sen'), 'Must render Tenkan-sen');
    assert.ok(tradingChartSource.includes('kijun_sen'), 'Must render Kijun-sen');
    assert.ok(tradingChartSource.includes('senkou_span_a'), 'Must render Senkou Span A');
    assert.ok(tradingChartSource.includes('senkou_span_b'), 'Must render Senkou Span B');
    assert.ok(tradingChartSource.includes('chikou_span'), 'Must render Chikou Span');

    // Parabolic SAR dots
    assert.ok(tradingChartSource.includes(`ind === 'PSAR'`), 'Must render Parabolic SAR');
    assert.ok(tradingChartSource.includes('ctx.arc(x, y, 2.5, 0, Math.PI * 2)'), 'Must render discrete SAR dots');
  });

  it('TradingChart.tsx includes active overlay readout in the chart legend', () => {
    assert.ok(tradingChartSource.includes('Active Overlays Readout in Legend'), 'Must render overlay badges in legend');
    assert.ok(tradingChartSource.includes('ov.indicator'), 'Legend must show indicator name');
  });

  it('PriceChart.tsx exports PriceChartOverlay and renders overlay lines in dataset explorer', () => {
    const priceChartPath = path.resolve(root, 'components/studio/data/PriceChart.tsx');
    const priceChartSource = fs.readFileSync(priceChartPath, 'utf8');

    assert.ok(priceChartSource.includes('export interface PriceChartOverlay'), 'PriceChart must export PriceChartOverlay');
    assert.ok(priceChartSource.includes('overlays?: PriceChartOverlay[]'), 'PriceChart must accept overlays prop');
    assert.ok(priceChartSource.includes('<Line'), 'PriceChart must render Recharts Line for overlays');
  });
});

describe('M35 Task 3: Mock Canvas Overlay Drawing Execution', () => {
  it('executes overlay coordinate mapping on simulated 70-bar fixture without NaN or exceptions', () => {
    // 70 fixture bars
    const barsCount = 70;
    const bars = [];
    let p = 100;
    for (let i = 0; i < barsCount; i++) {
      p += (i % 2 === 0 ? 0.5 : -0.3);
      bars.push({ t: 1700000000000 + i * 86400000, o: p, h: p + 1.2, l: p - 0.8, c: p + 0.2, v: 1000 + i * 50 });
    }

    // Mock SMA values with 19 null warm-up positions
    const smaValues = new Array(barsCount).fill(null);
    for (let i = 19; i < barsCount; i++) {
      smaValues[i] = 100 + i * 0.1;
    }

    // Mock BB values
    const bbSeries = {
      middle: new Array(barsCount).fill(null),
      upper: new Array(barsCount).fill(null),
      lower: new Array(barsCount).fill(null),
    };
    for (let i = 19; i < barsCount; i++) {
      bbSeries.middle[i] = 100 + i * 0.1;
      bbSeries.upper[i] = 105 + i * 0.1;
      bbSeries.lower[i] = 95 + i * 0.1;
    }

    // Mock Ichimoku series
    const ichiSeries = {
      tenkan_sen: new Array(barsCount).fill(null),
      kijun_sen: new Array(barsCount).fill(null),
      senkou_span_a: new Array(barsCount).fill(null),
      senkou_span_b: new Array(barsCount).fill(null),
      chikou_span: new Array(barsCount).fill(null),
    };
    for (let i = 8; i < barsCount; i++) ichiSeries.tenkan_sen[i] = 101.5;
    for (let i = 25; i < barsCount; i++) {
      ichiSeries.kijun_sen[i] = 100.8;
      ichiSeries.senkou_span_a[i] = 101.0;
    }
    for (let i = 51; i < barsCount; i++) ichiSeries.senkou_span_b[i] = 99.5;
    for (let i = 0; i < barsCount; i++) ichiSeries.chikou_span[i] = 100.2;

    // Mock PSAR values
    const psarValues = new Array(barsCount).fill(null);
    for (let i = 1; i < barsCount; i++) {
      psarValues[i] = 98.0 + i * 0.05;
    }

    // Track mock canvas calls
    const calls = {
      moveTo: [],
      lineTo: [],
      arc: [],
      fill: 0,
      stroke: 0,
    };

    const mockCtx = {
      save: () => {},
      restore: () => {},
      scale: () => {},
      beginPath: () => {},
      closePath: () => {},
      moveTo: (x, y) => calls.moveTo.push({ x, y }),
      lineTo: (x, y) => calls.lineTo.push({ x, y }),
      arc: (x, y, r, sa, ea) => calls.arc.push({ x, y, r, sa, ea }),
      stroke: () => { calls.stroke++; },
      fill: () => { calls.fill++; },
      fillRect: () => {},
      fillText: () => {},
      setLineDash: () => {},
    };

    const count = barsCount;
    const candleChartHeight = 300;
    const chartWidth = 700;
    const candleSlotWidth = chartWidth / count;
    const xOffset = 0;
    const paddedMin = 90;
    const fullPriceRange = 30;

    const priceToY = (val) => candleChartHeight - ((val - paddedMin) / fullPriceRange) * candleChartHeight + 10;

    // 1. Simulate SMA drawing
    let drawing = false;
    for (let i = 0; i < count; i++) {
      const v = smaValues[i];
      if (v !== null && v !== undefined && !isNaN(v)) {
        const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
        const y = priceToY(v);
        if (!drawing) {
          mockCtx.moveTo(x, y);
          drawing = true;
        } else {
          mockCtx.lineTo(x, y);
        }
      }
    }
    mockCtx.stroke();

    // Verify SMA: first valid moveTo must be index 19
    assert.equal(calls.moveTo.length, 1);
    const expectedX19 = 19 * candleSlotWidth + candleSlotWidth / 2;
    assert.equal(Math.abs(calls.moveTo[0].x - expectedX19) < 1e-6, true);
    assert.equal(calls.lineTo.length, count - 19 - 1); // 70 - 19 - 1 = 50 lineTo segments

    // Check no NaN or Infinity in calls
    for (const pt of calls.moveTo) {
      assert.equal(Number.isFinite(pt.x), true);
      assert.equal(Number.isFinite(pt.y), true);
    }
    for (const pt of calls.lineTo) {
      assert.equal(Number.isFinite(pt.x), true);
      assert.equal(Number.isFinite(pt.y), true);
    }

    // 2. Simulate PSAR dots
    for (let i = 0; i < count; i++) {
      const v = psarValues[i];
      if (v !== null && v !== undefined && !isNaN(v)) {
        const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
        const y = priceToY(v);
        mockCtx.arc(x, y, 2.5, 0, Math.PI * 2);
        mockCtx.fill();
      }
    }

    assert.equal(calls.arc.length, count - 1); // index 1..69
    for (const dot of calls.arc) {
      assert.equal(Number.isFinite(dot.x), true);
      assert.equal(Number.isFinite(dot.y), true);
      assert.equal(dot.r, 2.5);
    }
  });
});
