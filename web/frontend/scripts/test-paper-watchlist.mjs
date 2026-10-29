import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendDir = path.resolve(__dirname, '..');

const { load, cleanup } = transpileForTest('paper-watchlist', {
  files: ['lib/marketStatus.ts'],
});

const {
  getMarketSession,
  getBaselineTickerPrice,
  CURATED_WATCHLIST_ITEMS,
  WATCHLIST_CATEGORY_LABELS,
} = load('lib/marketStatus.js');

test('paper trading watchlist and market status', async (t) => {
  await t.test('calculates market session correctly for regular trading hours', () => {
    // Wednesday 14:00 UTC (10:00 AM EDT)
    const midDay = new Date('2026-10-28T14:00:00Z');
    const res = getMarketSession(midDay);
    assert.equal(res.session, 'open');
    assert.equal(res.isRegularHours, true);
    assert.equal(res.badgeTone, 'profit');
  });

  await t.test('calculates market session correctly for pre-market and after-hours', () => {
    // Wednesday 10:00 UTC (6:00 AM EDT) -> pre-market
    const preMarket = new Date('2026-10-28T10:00:00Z');
    const resPre = getMarketSession(preMarket);
    assert.equal(resPre.session, 'pre-market');
    assert.equal(resPre.isRegularHours, false);

    // Wednesday 21:00 UTC (17:00 EDT) -> after-hours
    const afterHours = new Date('2026-10-28T21:00:00Z');
    const resAfter = getMarketSession(afterHours);
    assert.equal(resAfter.session, 'after-hours');
  });

  await t.test('detects weekend market closure', () => {
    // Sunday 18:00 UTC
    const sunday = new Date('2026-10-25T18:00:00Z');
    const res = getMarketSession(sunday);
    assert.equal(res.session, 'closed');
    assert.ok(res.description.includes('Weekend'));
  });

  await t.test('curated watchlist items cover required asset categories', () => {
    assert.ok(CURATED_WATCHLIST_ITEMS.length >= 15, 'Has extensive curated catalog');
    const categories = new Set(CURATED_WATCHLIST_ITEMS.map((c) => c.category));
    assert.ok(categories.has('tech'), 'Includes mega-cap tech');
    assert.ok(categories.has('etfs'), 'Includes index ETFs');
    assert.ok(categories.has('volatility'), 'Includes high-beta names');

    assert.ok(getBaselineTickerPrice('AAPL') > 150);
    assert.ok(getBaselineTickerPrice('SPY') > 500);
    assert.equal(getBaselineTickerPrice('UNKNOWN_TICKER'), 175);
  });

  await t.test('PaperLab component integrates live status and quick-pick categories', () => {
    const file = path.join(frontendDir, 'components/studio/PaperLab.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.ok(content.includes('getMarketSession'), 'Uses getMarketSession helper');
    assert.ok(content.includes('marketSession.label'), 'Renders market session badge in header');
    assert.ok(content.includes('CURATED_WATCHLIST_ITEMS'), 'Imports curated items');
    assert.ok(content.includes('watchlistCategory'), 'Tracks category state');
    assert.ok(content.includes('Star'), 'Provides star toggle for watchlist customization');
  });
});
