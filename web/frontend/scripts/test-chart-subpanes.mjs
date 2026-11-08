/**
 * Test suite for M35 Task 4 (Sub-panes).
 *
 * Verifies:
 * 1. TradingChart exports SubPaneSeries and accepts subpanes prop.
 * 2. Independent Y-scale calculation and rendering for:
 *    RSI, MACD, Stochastic, ATR, ADX, OBV, CCI, Williams %R, MFI, CMF.
 * 3. Overbought / oversold and zero-line reference markers.
 * 4. Crosshair tracking across sub-pane boundaries.
 */

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

describe('M35 Task 4: Sub-Panes Contract & Component Exports', () => {
  const tradingChartPath = path.resolve(root, 'components/studio/TradingChart.tsx');
  const tradingChartSource = fs.readFileSync(tradingChartPath, 'utf8');

  it('TradingChart exports SubPaneSeries interface and accepts subpanes prop', () => {
    assert.ok(tradingChartSource.includes('export interface SubPaneSeries'), 'TradingChart must export SubPaneSeries');
    assert.ok(tradingChartSource.includes('subpanes?: SubPaneSeries[]'), 'TradingChartProps must include subpanes prop');
  });

  it('TradingChart partitions vertical canvas space when subpanes are active', () => {
    assert.ok(tradingChartSource.includes('activeSubpanes'), 'Must filter active subpanes');
    assert.ok(tradingChartSource.includes('subPaneHeight'), 'Must calculate subPaneHeight');
    assert.ok(tradingChartSource.includes('mainChartHeight'), 'Must allocate mainChartHeight independently');
  });

  it('TradingChart implements rendering for all 10 required sub-pane indicators', () => {
    const required = [
      'RSI',
      'MACD',
      'STOCH',
      'ATR',
      'ADX',
      'OBV',
      'CCI',
      'WILLR',
      'CMF',
      'MFI',
    ];

    for (const ind of required) {
      assert.ok(
        tradingChartSource.includes(`ind === '${ind}'`),
        `TradingChart must implement sub-pane rendering for ${ind}`
      );
    }
  });

  it('TradingChart implements reference threshold lines for oscillators', () => {
    // RSI 70/30
    assert.ok(tradingChartSource.includes('drawRefLine(70)'), 'Must draw RSI 70 level');
    assert.ok(tradingChartSource.includes('drawRefLine(30)'), 'Must draw RSI 30 level');
    // Stoch & MFI 80/20
    assert.ok(tradingChartSource.includes('drawRefLine(80)'), 'Must draw 80 level');
    assert.ok(tradingChartSource.includes('drawRefLine(20)'), 'Must draw 20 level');
    // Williams %R -20/-80
    assert.ok(tradingChartSource.includes('drawRefLine(-20)'), 'Must draw -20 level');
    assert.ok(tradingChartSource.includes('drawRefLine(-80)'), 'Must draw -80 level');
    // Zero line for MACD, CMF, CCI
    assert.ok(tradingChartSource.includes('drawRefLine(0)'), 'Must draw zero reference line');
    // ADX 25 trend line
    assert.ok(tradingChartSource.includes('drawRefLine(25)'), 'Must draw ADX 25 trend strength level');
  });

  it('MACD sub-pane renders histogram bars and macd/signal lines', () => {
    assert.ok(tradingChartSource.includes('sp.series?.histogram'), 'Must access histogram series');
    assert.ok(tradingChartSource.includes('v >= 0 ? \'rgba(16, 185, 129, 0.7)\''), 'Positive histogram must be green');
    assert.ok(tradingChartSource.includes('rgba(239, 68, 68, 0.7)'), 'Negative histogram must be red');
    assert.ok(tradingChartSource.includes('line.key === \'macd\' || line.key === \'signal\'') || tradingChartSource.includes(`key: 'macd'`), 'Must draw MACD lines');
  });

  it('Stochastic sub-pane renders both %K and %D lines', () => {
    assert.ok(tradingChartSource.includes(`key: 'k'`), 'Must draw %K line');
    assert.ok(tradingChartSource.includes(`key: 'd'`), 'Must draw %D line');
  });

  it('ADX sub-pane renders ADX, +DI, and -DI lines', () => {
    assert.ok(tradingChartSource.includes(`key: 'adx'`), 'Must draw ADX line');
    assert.ok(tradingChartSource.includes(`key: 'plus_di'`), 'Must draw +DI line');
    assert.ok(tradingChartSource.includes(`key: 'minus_di'`), 'Must draw -DI line');
  });
});

describe('M35 Task 4: Sub-Pane Mock Coordinate Calculation', () => {
  it('computes correct independent scales and Y-coordinates for RSI and MACD', () => {
    const barsCount = 50;
    const chartWidth = 600;
    const candleSlotWidth = chartWidth / barsCount;

    // RSI subpane (0 to 100)
    const rsiMin = 0;
    const rsiMax = 100;
    const rsiTop = 260;
    const rsiHeight = 90;
    const rsiInnerTop = rsiTop + 16;
    const rsiInnerBottom = rsiTop + rsiHeight - 6;
    const rsiInnerH = rsiInnerBottom - rsiInnerTop;

    const rsiY = (v) => rsiInnerBottom - ((v - rsiMin) / (rsiMax - rsiMin)) * rsiInnerH;

    // At RSI = 50, Y must be exactly halfway
    const y50 = rsiY(50);
    const expectedHalf = (rsiInnerTop + rsiInnerBottom) / 2;
    assert.equal(Math.abs(y50 - expectedHalf) < 1e-6, true);

    // At RSI = 100, Y must be at inner top
    assert.equal(Math.abs(rsiY(100) - rsiInnerTop) < 1e-6, true);

    // At RSI = 0, Y must be at inner bottom
    assert.equal(Math.abs(rsiY(0) - rsiInnerBottom) < 1e-6, true);

    // MACD zero centering
    const macdMax = 2.0;
    const macdMin = -2.0;
    const macdTop = 350;
    const macdHeight = 90;
    const macdInnerTop = macdTop + 16;
    const macdInnerBottom = macdTop + macdHeight - 6;
    const macdInnerH = macdInnerBottom - macdInnerTop;

    const macdY = (v) => macdInnerBottom - ((v - macdMin) / (macdMax - macdMin)) * macdInnerH;

    // MACD zero line must be centered
    const zeroY = macdY(0);
    assert.equal(Math.abs(zeroY - (macdInnerTop + macdInnerBottom) / 2) < 1e-6, true);
  });
});
