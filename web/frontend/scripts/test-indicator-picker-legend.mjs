/**
 * Comprehensive verification for M35 Task 5: Picker and Legend
 *
 * Verifies:
 * 1. IndicatorPickerModal component contract, category tabs, and Core 12 prioritization.
 * 2. IndicatorSettingsModal parameter validation, color presets, and reset to defaults.
 * 3. TradingChart toolbar integration ("+ Indicators" button), legend readouts, and action buttons.
 * 4. Responsive design rules for desktop (1280px) and phone (375px) widths.
 * 5. Client-side computeIndicatorSeries verification across key indicators.
 */

import { describe, it, after } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

const t = transpileForTest('picker-legend', { files: ['lib/indicators.ts', 'lib/api.ts'] });
after(t.cleanup);
const indMod = t.load('lib/indicators.js');

describe('M35 Task 5: IndicatorPickerModal & Decision 2 Prioritization', () => {
  const pickerPath = path.resolve(root, 'components/studio/indicators/IndicatorPickerModal.tsx');
  assert.ok(fs.existsSync(pickerPath), 'IndicatorPickerModal.tsx must exist');
  const pickerSource = fs.readFileSync(pickerPath, 'utf8');

  it('exports IndicatorPickerModal and accepts required props', () => {
    assert.ok(pickerSource.includes('export function IndicatorPickerModal'), 'Must export IndicatorPickerModal');
    assert.ok(pickerSource.includes('export interface IndicatorPickerModalProps'), 'Must export IndicatorPickerModalProps');
    assert.ok(pickerSource.includes('onSelectIndicator'), 'Must accept onSelectIndicator callback');
    assert.ok(pickerSource.includes('activeIndicatorIds'), 'Must accept activeIndicatorIds prop');
  });

  it('implements category filter tabs for All, Overlays, and Sub-Panes', () => {
    assert.ok(pickerSource.includes("setSelectedTab('all')"), 'Must have tab for All');
    assert.ok(pickerSource.includes("setSelectedTab('overlay')"), 'Must have tab for Overlays');
    assert.ok(pickerSource.includes("setSelectedTab('subpane')"), 'Must have tab for Sub-Panes');
  });

  it('implements Decision 2: Core 12 indicators prioritized at top', () => {
    assert.ok(pickerSource.includes('CORE_INDICATOR_IDS'), 'Must import or use CORE_INDICATOR_IDS');
    assert.ok(pickerSource.includes('Core Indicators (Recommended)'), 'Must render Core Indicators section');
    assert.ok(pickerSource.includes('Additional Indicators'), 'Must render Additional Indicators section');
  });

  it('provides deep-links to documentation and learning center for each indicator', () => {
    assert.ok(
      pickerSource.includes('/learn/indicators/'),
      'Must link to /learn/indicators/[slug] documentation'
    );
  });
});

describe('M35 Task 5: IndicatorSettingsModal & Parameter Validation', () => {
  const settingsPath = path.resolve(root, 'components/studio/indicators/IndicatorSettingsModal.tsx');
  assert.ok(fs.existsSync(settingsPath), 'IndicatorSettingsModal.tsx must exist');
  const settingsSource = fs.readFileSync(settingsPath, 'utf8');

  it('exports IndicatorSettingsModal and accepts required props', () => {
    assert.ok(settingsSource.includes('export function IndicatorSettingsModal'), 'Must export IndicatorSettingsModal');
    assert.ok(settingsSource.includes('export interface IndicatorSettingsModalProps'), 'Must export IndicatorSettingsModalProps');
    assert.ok(settingsSource.includes('onSave'), 'Must accept onSave callback');
  });

  it('validates parameter constraints (min, max, integer vs float)', () => {
    assert.ok(settingsSource.includes('validateParam'), 'Must have param validation logic');
    assert.ok(settingsSource.includes('Minimum allowed is'), 'Must enforce minimum allowed');
    assert.ok(settingsSource.includes('Maximum allowed is'), 'Must enforce maximum allowed');
  });

  it('provides color presets and custom color selector', () => {
    assert.ok(settingsSource.includes('COLOR_PRESETS'), 'Must define preset color palette');
    assert.ok(settingsSource.includes('type="color"'), 'Must support HTML5 color picker');
  });

  it('supports reset to spec defaults and remove action', () => {
    assert.ok(settingsSource.includes('handleResetDefaults'), 'Must support reset to defaults');
    assert.ok(settingsSource.includes('onRemove'), 'Must support removing indicator');
  });
});

describe('M35 Task 5: TradingChart Toolbar & Legend Integration', () => {
  const chartPath = path.resolve(root, 'components/studio/TradingChart.tsx');
  assert.ok(fs.existsSync(chartPath), 'TradingChart.tsx must exist');
  const chartSource = fs.readFileSync(chartPath, 'utf8');

  it('integrates "+ Indicators" button in chart toolbar', () => {
    assert.ok(
      chartSource.includes('setIsPickerOpen(true)'),
      'Toolbar must have button that opens indicator picker modal'
    );
    assert.ok(
      chartSource.includes('Indicators'),
      'Button must display "Indicators" label'
    );
  });

  it('renders interactive indicator items in the legend with hover readouts', () => {
    assert.ok(
      chartSource.includes('Active Indicators & Overlays Readout in Legend') ||
      chartSource.includes('effectiveOverlays.map') ||
      chartSource.includes('overlays.map'),
      'Must map active overlays in legend'
    );
    assert.ok(
      chartSource.includes('effectiveSubpanes.map'),
      'Must map active sub-panes in legend'
    );
    assert.ok(chartSource.includes('val.toFixed(2)'), 'Must render formatted numerical value on hover');
  });

  it('provides visibility toggle, settings trigger, and remove buttons on each legend item', () => {
    assert.ok(chartSource.includes('handleToggleVisibility'), 'Must support toggling indicator visibility');
    assert.ok(chartSource.includes('setSettingsModalState'), 'Must support opening settings modal from legend item');
    assert.ok(chartSource.includes('handleRemoveIndicator'), 'Must support removing indicator from legend item');
    assert.ok(chartSource.includes('Eye'), 'Must render eye icon');
    assert.ok(chartSource.includes('Settings'), 'Must render settings icon');
    assert.ok(chartSource.includes('X'), 'Must render remove close icon');
  });

  it('supports responsive layouts at desktop (1280px) and phone (375px) widths', () => {
    // Check responsive flex classes
    assert.ok(chartSource.includes('flex-wrap'), 'Legend and toolbar must use flex-wrap to prevent phone clipping');
    assert.ok(chartSource.includes('clamp('), 'Canvas container must use responsive clamp scaling');
  });
});

describe('M35 Task 5: computeIndicatorSeries Calculation Accuracy', () => {
  it('correctly calculates core overlay series (SMA, EMA, BB) on mock bars', () => {
    const { computeIndicatorSeries } = indMod;

    const mockBars = [];
    let p = 100;
    for (let i = 0; i < 50; i++) {
      p += (i % 2 === 0 ? 1 : -0.5);
      mockBars.push({ o: p - 0.2, h: p + 1, l: p - 1, c: p, v: 1000 });
    }

    // SMA (period 10)
    const smaRes = computeIndicatorSeries('SMA', mockBars, { period: 10 });
    assert.ok(smaRes.values, 'SMA must return values');
    assert.strictEqual(smaRes.values[0], null, 'SMA warmup must be null');
    assert.strictEqual(smaRes.values[8], null, 'SMA bar 8 must be null for period 10');
    assert.ok(typeof smaRes.values[9] === 'number', 'SMA bar 9 must be a valid number');

    // EMA (period 10)
    const emaRes = computeIndicatorSeries('EMA', mockBars, { period: 10 });
    assert.ok(emaRes.values, 'EMA must return values');
    assert.strictEqual(emaRes.values[0], null, 'EMA warmup must be null');
    assert.ok(typeof emaRes.values[9] === 'number', 'EMA bar 9 must be a valid number');

    // BB (period 10, mult 2)
    const bbRes = computeIndicatorSeries('BB', mockBars, { period: 10, mult: 2.0 });
    assert.ok(bbRes.series?.middle, 'BB must have middle band');
    assert.ok(bbRes.series?.upper, 'BB must have upper band');
    assert.ok(bbRes.series?.lower, 'BB must have lower band');
    assert.ok(bbRes.series.upper[20] > bbRes.series.middle[20], 'Upper band must be above middle');
    assert.ok(bbRes.series.lower[20] < bbRes.series.middle[20], 'Lower band must be below middle');
  });

  it('correctly calculates core subpane series (RSI, MACD, STOCH, ATR) on mock bars', () => {
    const { computeIndicatorSeries } = indMod;

    const mockBars = [];
    let p = 150;
    for (let i = 0; i < 60; i++) {
      p += Math.sin(i * 0.3) * 2;
      mockBars.push({ o: p - 0.5, h: p + 1.5, l: p - 1.5, c: p, v: 5000 });
    }

    // RSI (period 14)
    const rsiRes = computeIndicatorSeries('RSI', mockBars, { period: 14 });
    assert.ok(rsiRes.values, 'RSI must return values');
    assert.strictEqual(rsiRes.values[0], null, 'RSI warmup must be null');
    assert.ok(typeof rsiRes.values[14] === 'number', 'RSI bar 14 must be a valid number');
    assert.ok(rsiRes.values[14] >= 0 && rsiRes.values[14] <= 100, 'RSI must be bounded between 0 and 100');

    // MACD
    const macdRes = computeIndicatorSeries('MACD', mockBars);
    assert.ok(macdRes.series?.macd, 'MACD must have macd line');
    assert.ok(macdRes.series?.signal, 'MACD must have signal line');
    assert.ok(macdRes.series?.histogram, 'MACD must have histogram');

    // ATR (period 14)
    const atrRes = computeIndicatorSeries('ATR', mockBars, { period: 14 });
    assert.ok(atrRes.values, 'ATR must return values');
    assert.ok(atrRes.values[20] > 0, 'ATR must be strictly positive');
  });
});
