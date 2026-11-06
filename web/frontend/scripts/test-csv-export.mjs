// M11 Task 4: CSV export for attribution panels. Run: node scripts/test-csv-export.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const { load, cleanup } = transpileForTest('csv-export', {
  files: ['lib/csvExport.ts', 'lib/attribution.ts'],
});
after(cleanup);
const { escapeText, formatNumber, toCsv, buildWaterfallCsv, buildInstrumentsCsv } = load('lib/csvExport.js');

const sampleAttribution = {
  total_gross_pnl: 1000,
  total_fees: 10,
  total_net_pnl: 990,
  open_position_pnl: 0,
  total_account_pnl: 990,
  total_trades: 2,
  overall_win_rate: 0.5,
  costs: {
    commission_fees: 10,
    slippage_costs: 5,
    slippage_included_in_fill_prices: true,
    borrow_fees: 0,
    financing_interest: 0,
    interest_income: 0,
    dividends: 0,
    coupons: 0,
    total_costs: 10,
  },
  direction: {
    long_gross_pnl: 1000,
    long_fees: 10,
    long_net_pnl: 990,
    long_trades: 2,
    long_wins: 1,
    long_win_rate: 0.5,
    short_gross_pnl: 0,
    short_fees: 0,
    short_net_pnl: 0,
    short_trades: 0,
    short_wins: 0,
    short_win_rate: 0,
  },
  instruments: [
    {
      instrument: 1,
      symbol: '=MSFT',
      gross_pnl: -200,
      fees: 5,
      net_pnl: -205,
      trades_count: 1,
      win_trades: 0,
      loss_trades: 1,
      win_rate: 0,
      long_pnl: -205,
      short_pnl: 0,
      pnl_share: -0.25,
    },
    {
      instrument: 0,
      symbol: 'AAPL',
      gross_pnl: 1200,
      fees: 5,
      net_pnl: 1195,
      trades_count: 1,
      win_trades: 1,
      loss_trades: 0,
      win_rate: 1,
      long_pnl: 1195,
      short_pnl: 0,
      pnl_share: 0.75,
    },
  ],
  waterfall: [
    { name: 'Gross Trading P&L', amount: 1000, running_total: 1000, subtotal: true },
    { name: 'Fees on Closed Trades', amount: -10, running_total: 990, subtotal: false },
    { name: 'Trading Net P&L', amount: 990, running_total: 990, subtotal: true },
    { name: 'Net Account P&L', amount: 990, running_total: 990, subtotal: true },
  ],
};

describe('CSV sanitization and formatting', () => {
  it('escapeText prefixes formula trigger characters with a single quote', () => {
    assert.equal(escapeText('=SUM(A1:B1)'), "'=SUM(A1:B1)");
    assert.equal(escapeText('+123'), "'+123");
    assert.equal(escapeText('-cmd'), "'-cmd");
    assert.equal(escapeText('@admin'), "'@admin");
    assert.equal(escapeText('\ttabbed'), "'\ttabbed");
    assert.equal(escapeText('\rreturn'), `"'\rreturn"`);
  });

  it('escapeText quotes strings containing commas, quotes, newlines, or whitespace and doubles internal quotes', () => {
    assert.equal(escapeText('hello,world'), '"hello,world"');
    assert.equal(escapeText('with "quotes" inside'), '"with ""quotes"" inside"');
    assert.equal(escapeText(' padded '), '" padded "');
    assert.equal(escapeText('line1\nline2'), '"line1\nline2"');
    assert.equal(escapeText('line1\r\nline2'), '"line1\r\nline2"');
  });

  it('escapeText leaves safe text unquoted and unescaped', () => {
    assert.equal(escapeText('Gross Trading P&L'), 'Gross Trading P&L');
    assert.equal(escapeText('AAPL'), 'AAPL');
    assert.equal(escapeText('regular text without special chars'), 'regular text without special chars');
  });

  it('formatNumber writes numbers with a fixed decimal point and no thousands separators', () => {
    assert.equal(formatNumber(1234.5), '1234.50');
    assert.equal(formatNumber(0), '0.00');
    assert.equal(formatNumber(1234567.891), '1234567.89');
    assert.equal(formatNumber(null), '');
    assert.equal(formatNumber(undefined), '');
  });

  it('formatNumber preserves negative numbers without formula escaping', () => {
    const formatted = formatNumber(-123.45);
    assert.equal(formatted, '-123.45');
    assert.equal(formatted.startsWith("'"), false, 'must not be prefixed with quote');
  });

  it('toCsv prepends the UTF-8 BOM and formats rows with CRLF', () => {
    const csv = toCsv([
      ['Item', 'Amount'],
      ['Loss', -50.25],
      ['=Formula', 10],
    ]);
    assert.ok(csv.startsWith('\uFEFF'), 'must start with UTF-8 BOM');
    assert.ok(csv.includes('\r\n'), 'must use CRLF line breaks');
    assert.ok(csv.includes('Loss,-50.25'), 'negative numbers must stay numeric');
    assert.ok(csv.includes("'=Formula,10.00"), 'formula text must be escaped');
  });
});

describe('Attribution CSV export builders', () => {
  it('buildWaterfallCsv includes metadata header, BOM, and each waterfall step', () => {
    const csv = buildWaterfallCsv(sampleAttribution, {
      id: 'backtest-uuid-42',
      currency: 'USD',
      generatedAt: '2026-11-06T12:00:00Z',
    });
    assert.ok(csv.startsWith('\uFEFF'), 'must have UTF-8 BOM');
    assert.ok(csv.includes('# Run / Account ID,backtest-uuid-42'));
    assert.ok(csv.includes('# Currency,USD'));
    assert.ok(csv.includes('# Generated at (UTC),2026-11-06T12:00:00Z'));
    assert.ok(csv.includes('# Cost assumptions,'));
    assert.ok(csv.includes('Step,Change,Running Total,Subtotal'));
    assert.ok(csv.includes('Gross Trading P&L,1000.00,1000.00,true'));
    assert.ok(csv.includes('Fees on Closed Trades,-10.00,990.00,false'));
    assert.ok(csv.includes('Trading Net P&L,990.00,990.00,true'));
  });

  it('buildInstrumentsCsv includes metadata header, BOM, and instruments sorted by share', () => {
    const csv = buildInstrumentsCsv(sampleAttribution, {
      id: 'backtest-uuid-42',
      currency: 'USD',
    });
    assert.ok(csv.startsWith('\uFEFF'), 'must have UTF-8 BOM');
    assert.ok(csv.includes('# Run / Account ID,backtest-uuid-42'));
    assert.ok(csv.includes('Instrument,Symbol,Trades,Win Rate,Gross P&L,Fees,Net P&L,P&L Share,Long P&L,Short P&L'));

    // AAPL (share 0.75) must appear before =MSFT (share -0.25)
    const aaplIndex = csv.indexOf('AAPL');
    const msftIndex = csv.indexOf("'=MSFT");
    assert.ok(aaplIndex > 0, 'AAPL should be present');
    assert.ok(msftIndex > 0, '=MSFT should be present and formula-escaped');
    assert.ok(aaplIndex < msftIndex, 'AAPL (share 0.75) should precede MSFT (share -0.25)');
    assert.ok(csv.includes('-205.00'), 'negative net pnl must stay numeric');
  });
});

describe('AttributionCard export button wiring', () => {
  const cardSource = fs.readFileSync(
    path.join(root, 'components/studio/backtests/AttributionCard.tsx'),
    'utf8'
  );

  it('AttributionCard wires ExportButtons with downloadCsv and exportId', () => {
    assert.match(cardSource, /export function ExportButtons/);
    assert.match(cardSource, /export function downloadCsv/);
    assert.match(cardSource, /Waterfall CSV/);
    assert.match(cardSource, /Instruments CSV/);
    assert.match(cardSource, /exportId\?: string/);
    assert.match(cardSource, /<ExportButtons a=\{a\} exportId=\{exportId\} currency=\{currency\} \/>/);
    assert.match(cardSource, /exportId=\{backtestId\}/);
  });
});
