// Built-in sample dataset (lib/sampleCsv.ts, ingested by lib/sampleData.ts): deterministic, well-formed OHLCV, clearly synthetic.
// Run: node scripts/test-sample-data.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('sample-data', { files: ['lib/sampleCsv.ts'] });
after(t.cleanup);
const s = t.load('lib/sampleCsv.js');

describe('sample dataset', () => {
  const csv = s.buildSampleCsv();
  const rows = csv.trim().split('\n');
  const [header, ...data] = rows;

  it('is identical every time (seeded)', () => {
    assert.equal(s.buildSampleCsv(), csv);
    assert.notEqual(s.buildSampleCsv(7), csv);
  });

  it('has the columns the ingest settings name', () => {
    assert.equal(header, 'symbol,timestamp,open,high,low,close,volume');
    const c = s.SAMPLE_COLUMNS;
    for (const k of ['timestamp', 'open', 'high', 'low', 'close', 'volume', 'symbol']) assert.ok(header.split(',').includes(c[k]), k);
  });

  it('has two years of weekday bars for each made-up symbol', () => {
    assert.equal(data.length, 504 * s.SAMPLE_SYMBOLS.length);
    for (const r of data) {
      const day = new Date(r.split(',')[1]).getUTCDay();
      assert.ok(day >= 1 && day <= 5, `weekend bar: ${r}`);
    }
  });

  it('every bar is a valid OHLC bar with positive prices and volume', () => {
    for (const r of data) {
      const [, , o, h, l, c, v] = r.split(',');
      const [O, H, L, C, V] = [o, h, l, c, v].map(Number);
      assert.ok(L > 0 && V > 0, r);
      assert.ok(H >= Math.max(O, C) && L <= Math.min(O, C), r);
    }
  });

  it('says it is synthetic, and uses no real tickers', () => {
    assert.match(s.SAMPLE_DATASET_NAME, /synthetic/i);
    assert.match(s.SAMPLE_FILENAME, /synthetic/i);
    for (const x of s.SAMPLE_SYMBOLS) assert.match(x.symbol, /^DEMO/);
  });
});
