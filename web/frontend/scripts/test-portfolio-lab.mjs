// Portfolio Lab (M15 Task 6): saved portfolios, portfolio backtests and their reports in the Studio.
// Run: node scripts/test-portfolio-lab.mjs
import { describe, it, after, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('portfolio-lab', { files: ['lib/portfolios.ts', 'lib/reports.ts', 'lib/api.ts', 'lib/server/engineRoutes.ts'] });
after(t.cleanup);
const p = t.load('lib/portfolios.js');
const routes = t.load('lib/server/engineRoutes.js');
const lab = t.source('components/studio/PortfolioLab.tsx');
const workspace = t.source('components/studio/PortfolioWorkspace.tsx');
const studio = t.source('components/studio/StudioApp.tsx');
const panel = t.source('components/studio/ReportPanel.tsx');

const ID = '0192f3a4-0000-7000-8000-00000000002a';
const RUN = '0192f3a4-0000-7000-8000-00000000002b';
const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** Records every request and answers with `body`. */
function capture(body = {}) {
  const seen = [];
  globalThis.fetch = async (url, init = {}) => {
    seen.push({ url, method: init.method ?? 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    return json(body);
  };
  return seen;
}

const result = (rebalances) => ({
  summary: { total_return: 0.1, max_drawdown: 0.02, total_turnover: 1.2, total_cost: 4, rebalance_count: rebalances.length, initial_cash: 1000, final_equity: 1100 },
  equity: [
    { date: '2026-01-02', equity: 1000 },
    { date: '2026-02-02', equity: 1100 },
  ],
  rebalances,
  cost_config: { name: 'paper-default', version: 1, slippage_bps: 5, min_fee: 1 },
  prices: { first: '2026-01-02', last: '2026-02-02', days: 2, skipped_days: 0 },
});
const rebalance = { date: '2026-01-02', reasons: ['initial'], turnover: 1, cost: 2, weights: [{ symbol: 'AAA', before: 0, after: 0.5 }, { symbol: 'BBB', before: 0, after: 0.5 }], trades: [] };
const run = (status, res = null, error = null) => ({ id: RUN, portfolio_id: ID, status, request: {}, result: res, error, created_by: null, created_at: '2026-11-08T00:00:00Z', finished_at: null });

describe('API client', () => {
  it('reads, saves, edits and deletes portfolios through the engine proxy', async () => {
    const seen = capture({ id: ID });
    await p.listPortfolios();
    await p.getPortfolio(ID);
    const body = { name: 'Core', method: 'fixed', holdings: [{ symbol: 'AAA', weight: 1 }] };
    await p.createPortfolio(body);
    await p.updatePortfolio(ID, body);
    await p.deletePortfolio(ID);
    assert.deepEqual(
      seen.map((s) => [s.method, s.url]),
      [
        ['GET', '/api/engine/api/v1/portfolios'],
        ['GET', `/api/engine/api/v1/portfolios/${ID}`],
        ['POST', '/api/engine/api/v1/portfolios'],
        ['PUT', `/api/engine/api/v1/portfolios/${ID}`],
        ['DELETE', `/api/engine/api/v1/portfolios/${ID}`],
      ],
    );
    assert.deepEqual(seen[3].body, body, 'an edit sends the same body as a save');
  });

  it('runs a portfolio, lists its runs and reads one', async () => {
    const seen = capture({ items: [], next_cursor: null });
    const body = { dataset_version_id: ID, start: '2026-01-01', end: '2026-06-30', rule: { calendar: 'monthly' }, initial_cash: 100000 };
    await p.runPortfolio(ID, body);
    await p.listPortfolioRuns(ID);
    await p.getPortfolioRun(RUN);
    assert.deepEqual(
      seen.map((s) => [s.method, s.url]),
      [
        ['POST', `/api/engine/api/v1/portfolios/${ID}/runs`],
        ['GET', `/api/engine/api/v1/portfolios/${ID}/runs`],
        ['GET', `/api/engine/api/v1/portfolio-runs/${RUN}`],
      ],
    );
    assert.deepEqual(seen[0].body, body);
    assert.equal(p.portfolioReportPath(RUN), `/api/v1/portfolio-runs/${RUN}/report`);
  });

  it('shows the API message when a run is refused (a holding with no bars)', async () => {
    globalThis.fetch = async () => json({ message: 'No bars for CCC between 2026-01-01 and 2026-06-30 in this dataset version.' }, 422);
    await assert.rejects(p.runPortfolio(ID, {}), (e) => e.status === 422 && /No bars for CCC/.test(e.message));
  });
});

describe('roles', () => {
  it('lets owners and editors write; viewers and unknown roles only read', () => {
    assert.equal(p.canEditPortfolios('owner'), true);
    assert.equal(p.canEditPortfolios('editor'), true);
    assert.equal(p.canEditPortfolios('viewer'), false);
    assert.equal(p.canEditPortfolios(undefined), false);
    assert.equal(p.canEditPortfolios(null), false);
  });

  it('hides every edit and run control from viewers and says the view is read-only', () => {
    assert.match(lab, /\{!canEdit && \(\s*<p[^>]*data-testid="portfolio-read-only"/);
    assert.match(lab, /\{canEdit && \(\s*<button[^>]*onClick=\{\(\) => setEditing\('new'\)\}/, 'New portfolio');
    assert.match(lab, /\{editing && canEdit && \(/, 'editor form');
    assert.match(lab, /canEdit && portfolio \? \(/, 'Edit and Delete');
    assert.match(lab, /\{canEdit && portfolio && <RunForm /, 'run form');
    assert.match(lab, /canEdit && run\?\.status === 'succeeded' \?/, 'report export');
    assert.match(lab, /setCanEdit\(canEditPortfolios\(s\.workspace\?\.role\)\)/, 'role from the session');
  });
});

describe('panel states', () => {
  it('lists: error, running, loading, empty and loaded', () => {
    assert.equal(p.listState(null, 'boom'), 'error');
    assert.equal(p.listState([], 'boom'), 'error', 'an error wins');
    assert.equal(p.listState([], null, true), 'running');
    assert.equal(p.listState(null, null), 'loading');
    assert.equal(p.listState([], null), 'empty');
    assert.equal(p.listState([{ id: ID }], null), 'loaded');
  });

  it('a run: loading, running, error, empty (no rebalances) and loaded', () => {
    assert.equal(p.runState(null, null), 'loading');
    assert.equal(p.runState(null, null, true), 'running', 'a run being started');
    assert.equal(p.runState(run('running'), null), 'running');
    assert.equal(p.runState(null, 'No bars for CCC'), 'error');
    assert.equal(p.runState(run('failed', null, 'No bars for CCC'), null), 'error');
    assert.equal(p.runState(run('succeeded', result([])), null), 'empty');
    assert.equal(p.runState(run('succeeded', result([rebalance])), null), 'loaded');
  });

  it('every panel renders each state it can be in, and exposes it for tests', () => {
    for (const id of ['portfolio-list', 'portfolio-runs', 'portfolio-run-result']) {
      assert.match(lab, new RegExp(`testId="${id}"`), id);
    }
    assert.match(lab, /data-testid=\{testId\} data-state=\{state\}/);
    for (const [state, text] of [
      ['loading', /Loading…/],
      ['error', /<ErrorNote>\{error\}<\/ErrorNote>/],
      ['running', /\{RUNNING\}/],
      ['empty', /\{empty\}/],
    ]) {
      assert.match(lab, new RegExp(`state === '${state}'`), `StateNote handles ${state}`);
      assert.match(lab, text, `StateNote shows ${state}`);
    }
    assert.match(lab, /empty=\{NO_PORTFOLIOS\}/);
    assert.match(lab, /empty=\{NO_RUNS\}/);
    assert.match(lab, /empty=\{NO_REBALANCES\}/);
    assert.match(lab, /runState\(run, startError \?\? error, starting\)/);
  });
});

describe('results', () => {
  it('draws the equity curve with the backtest chart ([ns, value] points)', () => {
    const s = p.equitySeries(result([rebalance]));
    assert.equal(s.kind, 'equity');
    assert.deepEqual(s.points[0], [Date.UTC(2026, 0, 2) * 1_000_000, 1000]);
    assert.match(lab, /<EquityChart series=\{series\} start=\{result\.summary\.initial_cash\}/);
  });

  it('shows weights over time, turnover and cost per rebalance, and the stored reports', () => {
    assert.deepEqual(p.runSymbols(result([rebalance])), ['AAA', 'BBB']);
    assert.match(lab, /testId="portfolio-weights"/);
    assert.match(lab, /`\$\{pct\(w\.before\)\} → \$\{pct\(w\.after\)\}`/);
    assert.match(lab, /testId="portfolio-rebalances"/);
    assert.match(lab, /pct\(r\.turnover\)/);
    assert.match(lab, /cash\(r\.cost\)/);
    assert.match(lab, /<PortfolioRunReportPanel runId=\{run\.id\} refreshKey=\{reportsRefresh\} \/>/);
    assert.match(lab, /downloadPortfolioReport\(run\.id\)[\s\S]*setReportsRefresh/);
    assert.match(panel, /listRunReports\('portfolio_backtest', runId\)/);
  });

  it('checks fixed weights add up to 100% before saving', () => {
    assert.deepEqual(p.weightTotal([{ symbol: 'A', weight: 0.6 }, { symbol: 'B', weight: 0.4 }]), { total: 1, ok: true });
    assert.equal(p.weightTotal([{ symbol: 'A', weight: 0.6 }, { symbol: 'B', weight: 0.3 }]).ok, false);
    assert.equal(p.weightTotal([{ symbol: 'A', weight: 0.6 }, { symbol: 'B' }]).ok, false, 'a missing weight counts as 0');
  });
});

describe('route policy', () => {
  // [concrete path, policy key, method, minimum role]
  const table = [
    ['api/v1/portfolios', 'api/v1/portfolios', 'GET', 'viewer'],
    ['api/v1/portfolios', 'api/v1/portfolios', 'POST', 'editor'],
    [`api/v1/portfolios/${ID}`, 'api/v1/portfolios/{id}', 'GET', 'viewer'],
    [`api/v1/portfolios/${ID}`, 'api/v1/portfolios/{id}', 'PUT', 'editor'],
    [`api/v1/portfolios/${ID}`, 'api/v1/portfolios/{id}', 'DELETE', 'editor'],
    [`api/v1/portfolios/${ID}/runs`, 'api/v1/portfolios/{id}/runs', 'GET', 'viewer'],
    [`api/v1/portfolios/${ID}/runs`, 'api/v1/portfolios/{id}/runs', 'POST', 'editor'],
    [`api/v1/portfolio-runs/${RUN}`, 'api/v1/portfolio-runs/{id}', 'GET', 'viewer'],
    [`api/v1/portfolio-runs/${RUN}/report`, 'api/v1/portfolio-runs/{id}/report', 'GET', 'editor'],
  ];

  it('gives every portfolio route its policy key and role, following the backtest rules', () => {
    for (const [path, key, method, role] of table) {
      assert.equal(routes.routePolicyKey(path), key, path);
      const route = routes.ENGINE_ROUTES[key];
      assert.ok(route, `${key} is listed`);
      assert.ok(routes.allowsMethod(route, method), `${method} ${key} is allowed`);
      assert.equal(routes.minimumRole(route, method), role, `${method} ${key}`);
    }
    // The same roles as the backtest routes they mirror.
    const bt = routes.ENGINE_ROUTES;
    assert.equal(routes.minimumRole(bt['api/v1/backtests'], 'POST'), routes.minimumRole(bt['api/v1/portfolios/{id}/runs'], 'POST'));
    assert.equal(routes.minimumRole(bt['api/v1/backtests/{id}'], 'GET'), routes.minimumRole(bt['api/v1/portfolio-runs/{id}'], 'GET'));
    assert.equal(routes.minimumRole(bt['api/v1/backtests/{id}/tear-sheet/export'], 'GET'), routes.minimumRole(bt['api/v1/portfolio-runs/{id}/report'], 'GET'));
  });

  it('allows no other methods and keeps other paths unlisted', () => {
    assert.ok(!routes.allowsMethod(routes.ENGINE_ROUTES['api/v1/portfolio-runs/{id}'], 'DELETE'));
    assert.ok(!routes.allowsMethod(routes.ENGINE_ROUTES['api/v1/portfolio-runs/{id}/report'], 'POST'));
    assert.equal(routes.routePolicyKey(`api/v1/portfolios/${ID}/runs/x`), `api/v1/portfolios/${ID}/runs/x`);
    assert.equal(routes.routePolicyKey(`api/v1/portfolio-runs/${RUN}/other`), `api/v1/portfolio-runs/${RUN}/other`);
    assert.equal(routes.routePolicyKey('api/v1/portfolios/not-an-id'), 'api/v1/portfolios/not-an-id');
  });
});

describe('Studio wiring', () => {
  it('opens saved portfolios as a sub-view of the Portfolio tab, kept in the URL', () => {
    assert.match(studio, /\{tab === 'portfolio' && <PortfolioWorkspace \/>\}/);
    assert.match(workspace, /param === 'saved' \? 'saved' : 'positions'/);
    assert.match(workspace, /view === 'saved' \? <PortfolioLab \/> : <PortfolioStudio \/>/);
    assert.match(workspace, /\?tab=portfolio\$\{v === 'saved' \? '&view=saved' : ''\}/);
  });
});
