// Report panel (M16): the backtest and paper pages list each run's stored report versions.
// Run: node scripts/test-report-panel.mjs
import { describe, it, after, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('report-panel', { files: ['lib/reports.ts', 'lib/api.ts', 'lib/server/engineRoutes.ts'] });
after(t.cleanup);
const r = t.load('lib/reports.js');
const routes = t.load('lib/server/engineRoutes.js');
const panel = t.source('components/studio/ReportPanel.tsx');
const runDetail = t.source('components/studio/backtests/RunDetail.tsx');
const paperLab = t.source('components/studio/PaperLab.tsx');

const STRATEGY = '11111111-1111-1111-1111-111111111111';
const GAP_A = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';
const GAP_B = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb';
const version = (runId, n) => ({
  report_id: `${runId}-report`,
  report_type: 'gap',
  run_kind: 'paper_gap',
  run_id: runId,
  version: n,
  format: 'pdf',
  content_type: 'application/pdf',
  sha256: 'ab'.repeat(32),
  size_bytes: 10,
  created_at: '2026-11-01T00:00:00Z',
  download_url: `/api/v1/reports/${runId}-report/versions/${n}`,
});

const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } });
const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('backtest panel', () => {
  it('lists the backtest by run_kind=backtest and its job id', async () => {
    const seen = [];
    globalThis.fetch = async (url) => {
      seen.push(url);
      return json([]);
    };
    await r.listRunReports('backtest', 'job-1');
    assert.equal(seen[0], '/api/engine/api/v1/reports?run_kind=backtest&run_id=job-1');
    assert.match(panel, /listRunReports\('backtest', backtestId\)/);
  });

  it('is mounted under the run sections with the backtest id and reloads after a PDF export', () => {
    assert.match(runDetail, /<BacktestReportPanel backtestId=\{backtestId\} refreshKey=\{reportsRefresh\} \/>/);
    assert.match(runDetail, /downloadTearSheetPdf\(backtestId\)\.finally\(\(\) => setReportsRefresh/);
  });
});

describe('paper panel', () => {
  it('queries each gap report by its own id, never by the strategy id', async () => {
    const seen = [];
    globalThis.fetch = async (url) => {
      seen.push(url);
      if (url.startsWith('/api/engine/api/v1/paper/gap/reports')) {
        return json([
          { id: GAP_A, strategy_id: STRATEGY, created_at: '2026-11-02T00:00:00Z' },
          { id: GAP_B, strategy_id: STRATEGY, created_at: '2026-11-01T00:00:00Z' },
        ]);
      }
      const runId = new URL(url, 'http://x').searchParams.get('run_id');
      return json(runId === GAP_A ? [version(GAP_A, 2), version(GAP_A, 1)] : []);
    };
    const groups = await r.listGapReportVersions(STRATEGY);
    assert.equal(seen[0], `/api/engine/api/v1/paper/gap/reports?strategy_id=${STRATEGY}`);
    const reportCalls = seen.filter((u) => u.includes('/api/v1/reports?'));
    assert.deepEqual(reportCalls.sort(), [
      `/api/engine/api/v1/reports?run_kind=paper_gap&run_id=${GAP_A}`,
      `/api/engine/api/v1/reports?run_kind=paper_gap&run_id=${GAP_B}`,
    ]);
    assert.ok(!reportCalls.some((u) => u.includes(STRATEGY)), 'no report query uses the strategy id');
    assert.deepEqual(groups.map((g) => [g.gap.id, g.versions.map((v) => v.version)]), [[GAP_A, [2, 1]], [GAP_B, []]]);
  });

  it('lists the whole workspace when no strategy is given, as the gap tab does', async () => {
    const seen = [];
    globalThis.fetch = async (url) => {
      seen.push(url);
      return json([]);
    };
    assert.deepEqual(await r.listGapReportVersions(), []);
    assert.deepEqual(seen, ['/api/engine/api/v1/paper/gap/reports']);
  });

  it('is mounted in the paper page gap tab, one group per gap report', () => {
    assert.match(paperLab, /<BacktestToPaperGap readOnly \/>\s*<div className="mt-5">\s*<PaperReportPanel \/>/);
    assert.match(panel, /data\.map\(\(\{ gap, versions \}\) =>/);
    assert.match(panel, /data-testid="gap-report-group"/);
  });
});

describe('shared version list', () => {
  it('links every version through its download_url', () => {
    assert.equal(r.reportHref(version(GAP_A, 3)), `/api/engine/api/v1/reports/${GAP_A}-report/versions/3`);
    assert.match(panel, /href=\{reportHref\(v\)\}/);
    assert.doesNotMatch(panel, /\/versions\/\$\{/, 'no hand-built version URL');
  });

  it('shows the type, version and created date, and an empty state', () => {
    assert.match(panel, /REPORT_TYPE_LABEL\[v\.report_type\]/);
    assert.match(panel, /v\{v\.version\}/);
    assert.match(panel, /new Date\(v\.created_at\)\.toLocaleString\(\)/);
    assert.match(panel, /export const NO_REPORTS = 'No reports yet\.';/);
    assert.match(panel, /Loading…/);
    assert.match(panel, /text-rose-500">\{error\}/);
  });

  it('never shows who created a report', () => {
    assert.doesNotMatch(panel, /created_by|user_id/);
  });
});

describe('engine proxy', () => {
  const allowed = (path) => Object.prototype.hasOwnProperty.call(routes.ENGINE_ROUTES, routes.routePolicyKey(path));
  it('forwards the report list and version downloads', () => {
    assert.ok(allowed('api/v1/reports'));
    assert.ok(allowed(`api/v1/reports/${GAP_A}/versions/2`));
    assert.ok(!allowed(`api/v1/reports/${GAP_A}/versions/0`));
    assert.ok(!allowed(`api/v1/reports/${GAP_A}/versions/x`));
  });
  it('forwards the tear sheet and its PDF export, which create the backtest versions', () => {
    assert.ok(allowed(`api/v1/backtests/${GAP_A}/tear-sheet`));
    assert.ok(allowed(`api/v1/backtests/${GAP_A}/tear-sheet/export`));
  });
});
