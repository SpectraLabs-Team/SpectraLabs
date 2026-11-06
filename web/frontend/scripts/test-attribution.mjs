// M11 attribution panel: pure helpers and the panel's wiring. Run: node scripts/test-attribution.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const { load, cleanup } = transpileForTest('attribution', { files: ['lib/attribution.ts'] });
after(cleanup);
const { hasNoFills, sortByShare, waterfallBars, sharePct } = load('lib/attribution.js');

const step = (name, amount, running_total, subtotal = false) => ({ name, amount, running_total, subtotal });

describe('attribution helpers', () => {
  it('detects a run with no fills, and not one with an open position', () => {
    const empty = { total_trades: 0, instruments: [], costs: { commission_fees: 0 }, open_position_pnl: 0 };
    assert.equal(hasNoFills(empty), true);
    assert.equal(hasNoFills({ ...empty, open_position_pnl: -5 }), false);
    assert.equal(hasNoFills({ ...empty, costs: { commission_fees: 1 } }), false);
    assert.equal(hasNoFills({ ...empty, total_trades: 1 }), false);
  });

  it('sorts instruments by signed P&L share, largest first, ties by id', () => {
    const rows = [
      { instrument: 2, pnl_share: -0.2 },
      { instrument: 0, pnl_share: 0.5 },
      { instrument: 1, pnl_share: 0.5 },
    ];
    assert.deepEqual(sortByShare(rows).map((r) => r.instrument), [0, 1, 2]);
    assert.equal(rows[0].instrument, 2, 'the input is not mutated');
  });

  it('draws subtotals from zero and changes from the previous total', () => {
    const bars = waterfallBars([
      step('Gross Trading P&L', 1000, 1000, true),
      step('Fees on Closed Trades', -10, 990),
      step('Trading Net P&L', 990, 990, true),
      step('Financing Interest', -20, 970),
      step('Net Account P&L', 970, 970, true),
    ]);
    assert.deepEqual(bars.map((b) => [b.from, b.to]), [[0, 1000], [1000, 990], [0, 990], [990, 970], [0, 970]]);
    assert.equal(bars[1].tone, 'loss');
    assert.equal(bars[0].tone, 'profit');
  });

  it('keeps every bar inside 0..1 when totals go negative', () => {
    const bars = waterfallBars([
      step('Gross Trading P&L', -50, -50, true),
      step('Fees on Closed Trades', -5, -55),
      step('Trading Net P&L', -55, -55, true),
      step('Interest Income', 80, 25),
      step('Net Account P&L', 25, 25, true),
    ]);
    for (const b of bars) {
      assert.ok(b.left >= -1e-12 && b.left <= 1 + 1e-12, `left ${b.left}`);
      assert.ok(b.left + b.width <= 1 + 1e-12, `right edge ${b.left + b.width}`);
      assert.ok(b.width >= 0, 'non-negative width');
    }
  });

  it('gives an empty waterfall no bars', () => {
    assert.deepEqual(waterfallBars([]), []);
  });

  it('formats shares as percentages and refuses non-numbers', () => {
    assert.equal(sharePct(0.4567), '45.7%');
    assert.equal(sharePct(-1), '-100.0%');
    assert.equal(sharePct(Number.NaN), '—');
  });
});

describe('attribution panel wiring', () => {
  const card = fs.readFileSync(path.join(root, 'components/studio/backtests/AttributionCard.tsx'), 'utf8');
  const run = fs.readFileSync(path.join(root, 'components/studio/backtests/RunDetail.tsx'), 'utf8');
  const api = fs.readFileSync(path.join(root, 'lib/backtests.ts'), 'utf8');

  it('the API client fetches the attribution endpoint for a backtest', () => {
    assert.match(api, /getAttribution = \(backtestId: string\)/);
    assert.match(api, /\/api\/v1\/backtests\/\$\{id\(backtestId\)\}\/attribution/);
  });

  it('the card labels backtest numbers as simulated and shows all four blocks', () => {
    assert.match(card, /simulated/i);
    assert.match(card, /Waterfall/);
    assert.match(card, /Instruments/);
    assert.match(card, /Long \/ short/);
    assert.match(card, /Costs/);
  });

  it('the run page shows the panel for the run, and the panel loads that run\'s attribution', () => {
    assert.match(run, /<AttributionPanel key=\{b\.id\}/);
    assert.match(card, /getAttribution\(backtestId\)/);
  });
});
