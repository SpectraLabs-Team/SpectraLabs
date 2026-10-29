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

describe('extra sample datasets', () => {
  it('trend kind is exactly the original sample', () => {
    assert.equal(s.buildSampleCsvFor('trend'), s.buildSampleCsv());
  });
  for (const kind of ['crash', 'range']) {
    describe(kind, () => {
      const info = s.sampleInfo(kind);
      const csv = s.buildSampleCsvFor(kind);
      const [header, ...data] = csv.trim().split('\n');
      it('is seeded and has the shared header and bar count', () => {
        assert.equal(s.buildSampleCsvFor(kind), csv);
        assert.equal(header, 'symbol,timestamp,open,high,low,close,volume');
        assert.equal(data.length, 504 * info.symbols.length);
      });
      it('every bar is valid OHLC with positive prices and volume', () => {
        for (const r of data) {
          const [O, H, L, C, V] = r.split(',').slice(2).map(Number);
          assert.ok(L > 0 && V > 0 && H >= Math.max(O, C) && L <= Math.min(O, C), r);
        }
      });
      it('says it is synthetic, uses DEMO symbols, and has a unique name', () => {
        assert.match(info.name, /synthetic/i);
        assert.match(info.filename, /synthetic/i);
        for (const x of info.symbols) assert.match(x.symbol, /^DEMO/);
        assert.equal(new Set(s.SAMPLE_DATASETS.map((d) => d.name)).size, s.SAMPLE_DATASETS.length);
      });
    });
  }
  it('crash sample has a deep drawdown; range sample stays near its start', () => {
    const closes = (kind, sym) => s.buildSampleCsvFor(kind).trim().split('\n').slice(1).filter((r) => r.startsWith(sym + ',')).map((r) => Number(r.split(',')[5]));
    const x = closes('crash', 'DEMOX');
    let peak = 0, dd = 0;
    for (const c of x) { peak = Math.max(peak, c); dd = Math.max(dd, 1 - c / peak); }
    assert.ok(dd > 0.2, `max drawdown only ${dd}`);
    const r = closes('range', 'DEMOS');
    assert.ok(Math.abs(r[r.length - 1] / 150 - 1) < 0.3, `range drifted to ${r[r.length - 1]}`);
  });
});
