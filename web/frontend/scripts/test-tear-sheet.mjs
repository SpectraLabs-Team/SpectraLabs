import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendDir = path.resolve(__dirname, '..');

test('tear-sheet and PDF/CSV report exports', async (t) => {
  await t.test('TearSheetModal component exists with required export capabilities', () => {
    const file = path.join(frontendDir, 'components/studio/TearSheetModal.tsx');
    assert.ok(fs.existsSync(file), 'TearSheetModal.tsx should exist');
    const content = fs.readFileSync(file, 'utf8');

    assert.ok(content.includes('export function TearSheetModal'), 'Exports TearSheetModal component');
    assert.ok(content.includes('window.print()'), 'Includes browser print invocation for PDF export');
    assert.ok(content.includes('print:hidden'), 'Hides navigation/action chrome in print layout');
    assert.ok(content.includes('print:shadow-none'), 'Optimizes layout for clean paper printing');
    assert.ok(content.includes('handleDownloadCsv'), 'Includes 1-click CSV export');
    assert.ok(content.includes('handleDownloadJson'), 'Includes 1-click JSON export');
    assert.ok(content.includes('reportType'), 'Accepts configurable reportType prop');
    assert.ok(content.includes('sections'), 'Renders metric sections');
    assert.ok(content.includes('table'), 'Renders tabular breakdown');
  });

  await t.test('RiskLab integrates TearSheetModal with audit metadata', () => {
    const file = path.join(frontendDir, 'components/studio/RiskLab.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.ok(content.includes('import { TearSheetModal }'), 'Imports TearSheetModal');
    assert.ok(content.includes('tearSheetOpen'), 'Manages tearSheetOpen modal state');
    assert.ok(content.includes('Tear-Sheet & PDF'), 'Renders Tear-Sheet & PDF button');
    assert.ok(content.includes('reportType="Risk Audit"'), 'Passes Risk Audit report type');
    assert.ok(content.includes('Value at Risk & Capital Protection'), 'Contains VaR & Capital metrics section');
    assert.ok(content.includes('Concentration & Exposure Analytics'), 'Contains concentration analytics');
  });

  await t.test('RunDetail integrates TearSheetModal for quantitative backtests', () => {
    const file = path.join(frontendDir, 'components/studio/backtests/RunDetail.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.ok(content.includes('import { TearSheetModal }'), 'Imports TearSheetModal');
    assert.ok(content.includes('tearSheetOpen'), 'Manages tearSheetOpen modal state');
    assert.ok(content.includes('Tear-Sheet & PDF'), 'Renders Tear-Sheet & PDF button');
    assert.ok(content.includes('reportType="Quantitative Backtest"'), 'Passes Quantitative Backtest report type');
    assert.ok(content.includes('Return & Capital Dynamics'), 'Contains return dynamics section');
    assert.ok(content.includes('Risk-Adjusted Performance Ratios'), 'Contains risk ratios section');
  });

  await t.test('CompareRuns integrates TearSheetModal for comparative strategy analysis', () => {
    const file = path.join(frontendDir, 'components/studio/backtests/CompareRuns.tsx');
    const content = fs.readFileSync(file, 'utf8');

    assert.ok(content.includes('import { TearSheetModal }'), 'Imports TearSheetModal');
    assert.ok(content.includes('tearSheetOpen'), 'Manages tearSheetOpen modal state');
    assert.ok(content.includes('Tear-Sheet & PDF'), 'Renders Tear-Sheet & PDF button');
    assert.ok(content.includes('reportType="Comparative Analysis"'), 'Passes Comparative Analysis report type');
    assert.ok(content.includes('Performance Champions'), 'Contains performance champions summary');
    assert.ok(content.includes('Side-by-Side Strategy Comparison Scorecard'), 'Contains side-by-side scorecard table');
  });
});
