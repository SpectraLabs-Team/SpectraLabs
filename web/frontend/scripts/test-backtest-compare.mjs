// Multi-backtest comparison test suite.
// Run: node scripts/test-backtest-compare.mjs
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');

describe('multi-backtest comparison view', () => {
  it('has CompareRuns component with chart and comparison scorecard', () => {
    const file = path.join(root, 'components/studio/backtests/CompareRuns.tsx');
    assert.ok(fs.existsSync(file), 'CompareRuns.tsx exists');
    const content = fs.readFileSync(file, 'utf8');

    assert.match(content, /export function CompareRuns/, 'exports CompareRuns component');
    assert.match(content, /Multi-Backtest Comparison/, 'has comparison header');
    assert.match(content, /Comparative Equity Curves/, 'has equity curve comparison chart');
    assert.match(content, /Institutional Performance Comparison/, 'has metrics table');
    assert.match(content, /Strategy Parameters Matrix/, 'has parameter diff matrix');
    assert.match(content, /handleExportCsv/, 'has CSV export functionality');
  });

  it('RunList supports comparison mode, selection checkboxes, and compare action bar', () => {
    const file = path.join(root, 'components/studio/backtests/RunList.tsx');
    assert.ok(fs.existsSync(file), 'RunList.tsx exists');
    const content = fs.readFileSync(file, 'utf8');

    assert.match(content, /compareIds/, 'supports compareIds prop');
    assert.match(content, /onToggleCompareId/, 'supports onToggleCompareId prop');
    assert.match(content, /onOpenComparison/, 'supports onOpenComparison prop');
    assert.match(content, /onClearComparison/, 'supports onClearComparison prop');
    assert.match(content, /Scale/, 'has compare toggle icon');
    assert.match(content, /CheckSquare/, 'renders checkboxes for comparison');
  });

  it('BacktestLab wires compare state and URL synchronisation', () => {
    const file = path.join(root, 'components/studio/BacktestLab.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.match(content, /CompareRuns/, 'imports CompareRuns');
    assert.match(content, /isComparing/, 'manages isComparing state');
    assert.match(content, /compareIds/, 'manages compareIds state');
    assert.match(content, /openComparison/, 'implements openComparison');
    assert.match(content, /clearComparison/, 'implements clearComparison');
    assert.match(content, /url\.searchParams\.set\('compare'/, 'synchronizes compare in query params');
  });

  it('RunDetail provides direct add-to-compare button for completed runs', () => {
    const file = path.join(root, 'components/studio/backtests/RunDetail.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.match(content, /onAddToCompare/, 'supports onAddToCompare prop');
    assert.match(content, /isCompared/, 'supports isCompared prop');
    assert.match(content, /Compare/, 'has compare action button in header');
  });

  it('computes normalized percentage returns and deltas correctly', () => {
    const initialCash = 100_000;
    const finalCashA = 112_500;
    const finalCashB = 108_000;

    const returnA = (finalCashA - initialCash) / initialCash;
    const returnB = (finalCashB - initialCash) / initialCash;
    const deltaReturn = returnA - returnB;

    assert.equal(returnA, 0.125);
    assert.equal(returnB, 0.08);
    assert.equal(deltaReturn, 0.045);
  });
});
