// Number formatting: a value shown as zero never carries a minus sign ("-$0", "-0.00%").
// Run: node scripts/test-format.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('format', { files: ['lib/utils.ts'] });
after(t.cleanup);
const u = t.load('lib/utils.js');

describe('zero is never shown as negative', () => {
  it('currency', () => {
    assert.equal(u.formatCurrency(-0), '$0');
    assert.equal(u.formatCurrency(-0.4), '$0');
    assert.equal(u.formatCurrency(-0.004, 2), '$0.00');
    assert.match(u.formatCompactCurrency(-0), /^\$0(\.0)?$/);
  });
  it('numbers, percentages and compact numbers', () => {
    assert.equal(u.formatNumber(-0), '0.00');
    assert.equal(u.formatNumber(-0.001, 2), '0.00');
    assert.equal(u.formatPercent(-0), '0.00%');
    assert.equal(u.formatPercent(-0.00001), '0.00%');
    assert.equal(u.formatCompact(-0), '0');
  });
});

describe('real values are unchanged', () => {
  it('keeps the sign of values that do not round to zero', () => {
    assert.equal(u.formatCurrency(-1), '-$1');
    assert.equal(u.formatCurrency(-0.5), '-$1');
    assert.equal(u.formatCurrency(-0.005, 2), '-$0.01');
    assert.equal(u.formatCurrency(1234.5, 2), '$1,234.50');
    assert.equal(u.formatPercent(-0.0123), '-1.23%');
    assert.match(u.formatCompactCurrency(-1_500_000), /^-\$1\.50?M$/);
    assert.equal(u.formatNumber(-2.5, 1), '-2.5');
    assert.equal(u.formatCurrency(Number.NaN), '—');
  });
});
