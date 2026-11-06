// M11 Task 5: Help text and glossary tests. Run: node scripts/test-attribution-glossary.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const { load, cleanup } = transpileForTest('attribution-glossary', {
  files: ['lib/attributionGlossary.ts'],
});
after(cleanup);

const {
  ATTRIBUTION_GLOSSARY,
  getGlossaryEntry,
  getAllGlossaryEntries,
  getEntriesByCategory,
} = load('lib/attributionGlossary.js');

const cardSource = fs.readFileSync(
  path.join(root, 'components/studio/backtests/AttributionCard.tsx'),
  'utf8'
);

describe('attribution glossary and help text (M11 Task 5)', () => {
  it('covers all waterfall steps with plain-language definitions', () => {
    const expectedWaterfallSteps = [
      'Gross Trading P&L',
      'Fees on Closed Trades',
      'Trading Net P&L',
      'Open Positions (Realized So Far)',
      'Net Account P&L',
    ];
    for (const step of expectedWaterfallSteps) {
      const entry = getGlossaryEntry(step);
      assert.ok(entry, `missing entry for waterfall step: ${step}`);
      assert.equal(entry.category, 'waterfall');
      assert.ok(entry.description.length > 20, `description too brief for: ${step}`);
    }
  });

  it('covers all cost categories with plain-language definitions', () => {
    const expectedCostCategories = [
      'Commissions',
      'Borrow fees',
      'Financing interest',
      'Interest income',
      'Dividends (in trade gross)',
      'Coupons (in trade gross)',
      'Slippage estimate (in fill prices)',
    ];
    for (const cost of expectedCostCategories) {
      const entry = getGlossaryEntry(cost);
      assert.ok(entry, `missing entry for cost category: ${cost}`);
      assert.equal(entry.category, 'costs');
      assert.ok(entry.description.length > 20, `description too brief for: ${cost}`);
    }
  });

  it('covers instrument and direction metrics with plain-language definitions', () => {
    const expectedMetrics = ['Win rate', 'Share of |net|', 'Long / short'];
    for (const metric of expectedMetrics) {
      const entry = getGlossaryEntry(metric);
      assert.ok(entry, `missing entry for metric: ${metric}`);
      assert.equal(entry.category, 'metrics');
      assert.ok(entry.description.length > 20, `description too brief for: ${metric}`);
    }
  });

  it('every glossary entry has a concrete worked example', () => {
    const all = getAllGlossaryEntries();
    assert.ok(all.length >= 15, `expected at least 15 entries, got ${all.length}`);
    for (const entry of all) {
      assert.ok(entry.example, `entry ${entry.term} lacks example`);
      assert.ok(
        /\d+/.test(entry.example),
        `example for ${entry.term} must contain concrete numbers: "${entry.example}"`
      );
      assert.ok(
        entry.example.length >= 15,
        `example for ${entry.term} too short: "${entry.example}"`
      );
    }
  });

  it('every glossary entry has a concise short summary for tooltips', () => {
    const all = getAllGlossaryEntries();
    for (const entry of all) {
      assert.ok(entry.short, `entry ${entry.term} lacks short summary`);
      assert.ok(
        entry.short.length >= 10,
        `short summary for ${entry.term} too brief: "${entry.short}"`
      );
      assert.ok(
        entry.short.length <= 120,
        `short summary for ${entry.term} exceeds 120 chars: "${entry.short}"`
      );
    }
  });

  it('copy avoids return promises and guaranteed performance claims', () => {
    const all = getAllGlossaryEntries();
    const banned = /\b(guarantee|guaranteed|risk-free|promise|sure-fire|will always profit)\b/i;
    for (const entry of all) {
      assert.ok(
        !banned.test(entry.description),
        `description for ${entry.term} contains speculative claim: "${entry.description}"`
      );
      assert.ok(
        !banned.test(entry.example),
        `example for ${entry.term} contains speculative claim: "${entry.example}"`
      );
      assert.ok(
        !banned.test(entry.short),
        `short summary for ${entry.term} contains speculative claim: "${entry.short}"`
      );
    }
  });

  it('getGlossaryEntry resolves terms case-insensitively and handles aliases', () => {
    assert.equal(getGlossaryEntry('gross trading pnl')?.term, 'Gross Trading P&L');
    assert.equal(getGlossaryEntry('gross p&l')?.term, 'Gross Trading P&L');
    assert.equal(getGlossaryEntry('FEES')?.term, 'Fees on Closed Trades');
    assert.equal(getGlossaryEntry('closed trade fees')?.term, 'Fees on Closed Trades');
    assert.equal(getGlossaryEntry('winrate')?.term, 'Win rate');
    assert.equal(getGlossaryEntry('slippage')?.term, 'Slippage estimate (in fill prices)');
    assert.equal(getGlossaryEntry('margin interest')?.term, 'Financing interest');
    assert.equal(getGlossaryEntry('totally non-existent term'), undefined);
  });

  it('getEntriesByCategory partitions terms into waterfall, costs, and metrics', () => {
    const waterfall = getEntriesByCategory('waterfall');
    const costs = getEntriesByCategory('costs');
    const metrics = getEntriesByCategory('metrics');

    assert.equal(waterfall.length, 5, 'expected 5 waterfall entries');
    assert.equal(costs.length, 7, 'expected 7 cost entries');
    assert.equal(metrics.length, 3, 'expected 3 metric entries');
    assert.equal(
      waterfall.length + costs.length + metrics.length,
      ATTRIBUTION_GLOSSARY.length,
      'sum of category lengths must match total glossary count'
    );
  });

  it('AttributionCard exports HelpTip component', () => {
    assert.match(
      cardSource,
      /export function HelpTip\s*\(/,
      'AttributionCard must export HelpTip component'
    );
    assert.match(
      cardSource,
      /onOpenGlossary\?:\s*\(term:\s*string\)\s*=>\s*void/,
      'HelpTip must accept onOpenGlossary prop'
    );
  });

  it('AttributionCard exports Glossary component', () => {
    assert.match(
      cardSource,
      /export function Glossary\s*\(/,
      'AttributionCard must export Glossary component'
    );
    assert.match(
      cardSource,
      /initialTerm\?: string/,
      'Glossary component must accept initialTerm'
    );
    assert.match(
      cardSource,
      /isOpen:\s*boolean/,
      'Glossary component must accept isOpen'
    );
  });

  it('AttributionCard renders help buttons and glossary trigger', () => {
    assert.match(
      cardSource,
      /Glossary/,
      'AttributionCard must render a Glossary button'
    );
    assert.match(
      cardSource,
      /<Glossary[\s\S]*?isOpen=\{glossaryOpen\}/,
      'AttributionCard must mount the Glossary modal'
    );
  });

  it('Waterfall and Costs tables embed HelpTip components on their labels', () => {
    assert.match(
      cardSource,
      /<HelpTip term=\{b\.name\} onOpenGlossary=\{onOpenGlossary\} \/>/,
      'Waterfall step rows must embed HelpTip'
    );
    assert.match(
      cardSource,
      /<HelpTip term=\{label\} onOpenGlossary=\{onOpenGlossary\} \/>/,
      'Costs rows must embed HelpTip'
    );
    assert.match(
      cardSource,
      /<HelpTip term="Win rate" onOpenGlossary=\{onOpenGlossary\} \/>/,
      'Instruments headers must embed HelpTip for Win rate'
    );
  });

  it('Open Positions term does not contain mark-to-market or unrealized text', () => {
    const entry = getGlossaryEntry('Open Positions (Realized So Far)');
    assert.ok(entry, 'missing Open Positions (Realized So Far) entry');
    const text = `${entry.short} ${entry.description} ${entry.example}`.toLowerCase();
    assert.ok(!text.includes('mark-to-market'), 'must not contain mark-to-market');
    assert.ok(!text.includes('unrealized'), 'must not contain unrealized');
  });

  it('Net Account P&L example ends with arithmetic reconciliation to +$540.00', () => {
    const entry = getGlossaryEntry('Net Account P&L');
    assert.ok(entry, 'missing Net Account P&L entry');
    assert.ok(
      entry.example.endsWith('= Net Account P&L of +$540.00.'),
      `example must end with '= Net Account P&L of +$540.00.', got: "${entry.example}"`
    );
    assert.equal(498 + 40 - 1 + 3, 540);
  });
});
