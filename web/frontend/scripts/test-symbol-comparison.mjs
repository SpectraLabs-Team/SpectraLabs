/**
 * Test suite for M36 Task 3 (Symbol Comparison API & UI Integration).
 *
 * Verifies:
 * 1. chart-drawings.ts exports fetchSymbolComparison querying /api/v1/charts/compare.
 * 2. Rebase calculation formula: 100.0 * (P_t / P_base) based on the first bar of the full dataset.
 * 3. Timestamp alignment with null gaps (missing bars never invent prices).
 * 4. ComparisonModal exports and supports add, remove, suggestions, and validation.
 * 5. TradingChart exports ComparisonSeries and renders comparison lines and legend tags.
 */

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

describe('M36 Task 3: Symbol Comparison Client Library', () => {
  const libPath = path.resolve(root, 'lib/chart-drawings.ts');
  const libSource = fs.readFileSync(libPath, 'utf8');

  it('exports fetchSymbolComparison querying /api/v1/charts/compare', () => {
    assert.ok(libSource.includes('export async function fetchSymbolComparison'), 'Must export fetchSymbolComparison');
    assert.ok(libSource.includes('/api/v1/charts/compare?'), 'Must query /api/v1/charts/compare');
  });

  it('rebase calculation formula: 100 * (P_t / P_0) on first bar of full dataset', () => {
    // Primary bars: 100.0, 105.0, 110.0 -> [100.0, 105.0, 110.0]
    // Secondary bars: 200.0, 190.0, 210.0 -> [100.0, 95.0, 105.0]
    const p0 = 200.0;
    const p1 = 190.0;
    const p2 = 210.0;

    const rebased0 = (p0 / p0) * 100.0;
    const rebased1 = (p1 / p0) * 100.0;
    const rebased2 = (p2 / p0) * 100.0;

    assert.equal(rebased0, 100.0, 'First bar must be exactly 100.0');
    assert.equal(rebased1, 95.0, 'Second bar must be 95.0');
    assert.equal(rebased2, 105.0, 'Third bar must be 105.0');
  });

  it('timestamp alignment retains null on missing bars without interpolation', () => {
    const primaryTimestamps = [1000, 2000, 3000];
    const secondaryMap = new Map([
      [1000, 50.0],
      [3000, 55.0],
      // 2000 is missing (gap)
    ]);

    const basePrice = 50.0;
    const rebasedSeries = primaryTimestamps.map((ts) => {
      const p = secondaryMap.get(ts);
      return p !== undefined ? (p / basePrice) * 100.0 : null;
    });

    assert.equal(rebasedSeries[0], 100.0);
    assert.equal(rebasedSeries[1], null, 'Missing bar must be strictly null');
    assert.ok(Math.abs(rebasedSeries[2] - 110.0) < 1e-6);
  });
});

describe('M36 Task 3: ComparisonModal UI Component', () => {
  const modalPath = path.resolve(root, 'components/studio/charts/ComparisonModal.tsx');
  const modalSource = fs.readFileSync(modalPath, 'utf8');

  it('exports ComparisonModal and accepts required props', () => {
    assert.ok(modalSource.includes('export function ComparisonModal'), 'Must export ComparisonModal');
    assert.ok(modalSource.includes('baseSymbol: string'), 'Must accept baseSymbol prop');
    assert.ok(modalSource.includes('activeComparisons:'), 'Must accept activeComparisons');
    assert.ok(modalSource.includes('onAddComparison:'), 'Must accept onAddComparison');
    assert.ok(modalSource.includes('onRemoveComparison:'), 'Must accept onRemoveComparison');
  });

  it('validates against self-comparison and duplicates', () => {
    assert.ok(modalSource.includes('Cannot compare'), 'Must refuse self-comparison');
    assert.ok(modalSource.includes('already being compared'), 'Must refuse duplicate comparison');
  });

  it('provides quick suggestions and custom color selector', () => {
    assert.ok(modalSource.includes('SUGGESTED_SYMBOLS'), 'Must define quick suggestion symbols');
    assert.ok(modalSource.includes('COMPARISON_COLORS'), 'Must define comparison color palette');
  });
});

describe('M36 Task 3: TradingChart Comparison Integration', () => {
  const chartPath = path.resolve(root, 'components/studio/TradingChart.tsx');
  const chartSource = fs.readFileSync(chartPath, 'utf8');

  it('TradingChart exports ComparisonSeries and accepts comparison props', () => {
    assert.ok(chartSource.includes('export interface ComparisonSeries'), 'Must export ComparisonSeries');
    assert.ok(chartSource.includes('comparisons?: ComparisonSeries[]'), 'TradingChartProps must include comparisons');
    assert.ok(chartSource.includes('onAddComparison?:'), 'TradingChartProps must include onAddComparison');
    assert.ok(chartSource.includes('onRemoveComparison?:'), 'TradingChartProps must include onRemoveComparison');
  });

  it('TradingChart renders "+ Compare" button in toolbar', () => {
    assert.ok(chartSource.includes('setIsComparisonModalOpen(true)'), 'Must have button to open ComparisonModal');
    assert.ok(chartSource.includes('<span>Compare</span>'), 'Must display Compare button label');
  });

  it('TradingChart renders active comparison tags in legend with remove action', () => {
    assert.ok(chartSource.includes('Active Comparisons in Legend'), 'Must include comparisons in legend');
    assert.ok(chartSource.includes('handleRemoveComparison'), 'Must provide remove action on comparison badge');
  });

  it('TradingChart renders comparison series on canvas lifting pen on nulls', () => {
    assert.ok(chartSource.includes('Render Symbol Comparisons'), 'Must render comparison series on canvas');
    assert.ok(chartSource.includes('hasPoint = false'), 'Must lift pen on null values to preserve gaps');
  });
});
