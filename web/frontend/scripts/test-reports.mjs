// Versioned reports (M16): the client helpers and the run page's saved-report list.
// Run: node scripts/test-reports.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const src = (rel) => fs.readFileSync(path.join(root, rel), 'utf8');
const t = transpileForTest('reports', { files: ['lib/reports.ts', 'lib/api.ts'] });
after(t.cleanup);
const r = t.load('lib/reports.js');

describe('report client helpers', () => {
  it('links a version through the authenticated engine proxy', () => {
    assert.equal(r.reportHref({ download_url: '/api/v1/reports/abc/versions/2' }), '/api/engine/api/v1/reports/abc/versions/2');
  });

  it('lists a run by kind and id, escaping both', async () => {
    const seen = [];
    globalThis.fetch = async (url) => {
      seen.push(url);
      return new Response('[]', { status: 200, headers: { 'content-type': 'application/json' } });
    };
    assert.deepEqual(await r.listRunReports('sweep', 'a b'), []);
    assert.equal(seen[0], '/api/engine/api/v1/reports?run_kind=sweep&run_id=a%20b');
  });

  it('formats sizes', () => {
    assert.equal(r.formatBytes(512), '512 B');
    assert.equal(r.formatBytes(12_595), '12.3 KB');
    assert.equal(r.formatBytes(3 * 1024 * 1024), '3.0 MB');
    assert.equal(r.formatBytes(-1), '—');
  });

  it('names every report type the API can return', () => {
    const api = src('../../crates/spectra-db/migrations/20261101000002_reports.sql');
    const types = api.match(/report_type IN \(([^)]*)\)/)[1].match(/'([a-z_]+)'/g).map((s) => s.slice(1, -1));
    assert.deepEqual(Object.keys(r.REPORT_TYPE_LABEL).sort(), types.sort());
  });
});

describe('the sweep page lists its saved reports', () => {
  const lab = src('components/studio/OptimizationLab.tsx');
  const list = src('components/studio/RunReports.tsx');
  it('renders the run reports for the selected sweep', () => {
    assert.match(lab, /<RunReports runKind="sweep" runId=\{selectedSweepDetail\.sweep\.id\} refreshKey=\{reportsRefresh\} \/>/);
  });
  it('reloads the list after either export', () => {
    assert.equal((lab.match(/onClick=\{refreshReportsSoon\}/g) ?? []).length, 2);
  });
  it('downloads each version through the stored link and shows its hash', () => {
    assert.match(list, /href=\{reportHref\(v\)\}/);
    assert.match(list, /v\.sha256\.slice\(0, 12\)/);
    assert.match(list, /\[runKind, runId, refreshKey\]/, 'reloads when the run or refresh key changes');
  });
});
