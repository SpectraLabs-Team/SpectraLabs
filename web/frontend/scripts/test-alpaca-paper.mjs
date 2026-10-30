// Unit & integration tests for Alpaca Paper Trading, GitHub link removal, and Trading Dashboard.
// Run: node scripts/test-alpaca-paper.mjs
import { test, describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');
const require = createRequire(import.meta.url);

const ts = require(path.join(root, 'node_modules/typescript'));

function transpileAndLoad(relPath) {
  const tsSource = fs.readFileSync(path.join(root, relPath), 'utf8');
  const { outputText } = ts.transpileModule(tsSource, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  });
  const tempDir = path.join(root, 'node_modules', '.cache', 'alpaca-test');
  fs.mkdirSync(tempDir, { recursive: true });
  const tempFile = path.join(tempDir, path.basename(relPath).replace(/\.tsx?$/, '.cjs'));
  fs.writeFileSync(tempFile, outputText);
  return require(tempFile);
}

const site = transpileAndLoad('lib/site.ts');
const engineRoutes = transpileAndLoad('lib/server/engineRoutes.ts');

describe('1. Removal of GitHub References from the Public UI', () => {
  it('site.ts has no GITHUB_URL or github links', () => {
    assert.equal(site.GITHUB_URL, undefined, 'GITHUB_URL must be completely removed');
    const allLinks = [...site.NAV, ...site.DEV_LINKS];
    for (const link of allLinks) {
      assert.ok(!link.href.includes('github.com'), `No link may point to GitHub: ${link.href}`);
    }
  });

  it('Documentation button routes internally to /architecture, not GitHub', () => {
    const docLink = site.DEV_LINKS.find((l) => l.label === 'Docs');
    assert.ok(docLink, 'Docs link exists in DEV_LINKS');
    assert.equal(docLink.href, '/architecture', 'Docs link must route internally to /architecture');
  });

  it('components and app source trees contain zero github UI icons or external github URLs', () => {
    const checkDir = (dir) => {
      const entries = fs.readdirSync(dir, { withFileTypes: true });
      for (const e of entries) {
        const fullPath = path.join(dir, e.name);
        if (e.isDirectory()) {
          if (e.name !== 'node_modules' && e.name !== '.next') checkDir(fullPath);
        } else if (/\.(tsx|ts|jsx|js)$/.test(e.name)) {
          const content = fs.readFileSync(fullPath, 'utf8');
          assert.ok(
            !content.includes('github.com'),
            `Found unexpected github.com URL in UI file: ${fullPath}`
          );
          assert.ok(
            !content.includes('GithubIcon'),
            `Found unexpected GithubIcon component in UI file: ${fullPath}`
          );
        }
      }
    };

    checkDir(path.join(root, 'components'));
    checkDir(path.join(root, 'app'));
  });
});

describe('2. Engine Routes & Role-Based Access for Alpaca Paper Trading', () => {
  it('routePolicyKey maps Alpaca endpoints including parameter variations', () => {
    assert.equal(engineRoutes.routePolicyKey('api/v1/alpaca/account'), 'api/v1/alpaca/account');
    assert.equal(engineRoutes.routePolicyKey('api/alpaca/account'), 'api/alpaca/account');
    assert.equal(engineRoutes.routePolicyKey('api/v1/alpaca/positions'), 'api/v1/alpaca/positions');
    assert.equal(engineRoutes.routePolicyKey('api/v1/alpaca/orders'), 'api/v1/alpaca/orders');
    assert.equal(
      engineRoutes.routePolicyKey('api/v1/alpaca/orders/61a85a40-342a-40e7-9646-bb62a01f441c'),
      'api/v1/alpaca/orders/{id}'
    );
    assert.equal(
      engineRoutes.routePolicyKey('api/alpaca/orders/61a85a40-342a-40e7-9646-bb62a01f441c'),
      'api/alpaca/orders/{id}'
    );
    assert.equal(
      engineRoutes.routePolicyKey('api/v1/alpaca/stocks/AAPL/bars'),
      'api/v1/alpaca/stocks/{symbol}/bars'
    );
    assert.equal(
      engineRoutes.routePolicyKey('api/alpaca/stocks/TSLA/bars'),
      'api/alpaca/stocks/{symbol}/bars'
    );
    assert.equal(
      engineRoutes.routePolicyKey('api/v1/alpaca/crypto/bars'),
      'api/v1/alpaca/crypto/bars'
    );
    assert.equal(
      engineRoutes.routePolicyKey('api/alpaca/crypto/bars'),
      'api/alpaca/crypto/bars'
    );
  });

  it('all Alpaca mutation endpoints require editor role or higher', () => {
    const r = engineRoutes.ENGINE_ROUTES;
    assert.equal(engineRoutes.minimumRole(r['api/v1/alpaca/orders'], 'POST'), 'editor');
    assert.equal(engineRoutes.minimumRole(r['api/v1/alpaca/orders/{id}'], 'DELETE'), 'editor');
    assert.equal(engineRoutes.minimumRole(r['api/v1/alpaca/sync'], 'POST'), 'editor');
    assert.equal(engineRoutes.minimumRole(r['api/alpaca/orders'], 'POST'), 'editor');
    assert.equal(engineRoutes.minimumRole(r['api/alpaca/orders/{id}'], 'DELETE'), 'editor');
  });

  it('market data and assets endpoints allow viewer role', () => {
    const r = engineRoutes.ENGINE_ROUTES;
    assert.equal(engineRoutes.minimumRole(r['api/v1/alpaca/assets'], 'GET'), 'viewer');
    assert.equal(engineRoutes.minimumRole(r['api/v1/alpaca/stocks/{symbol}/bars'], 'GET'), 'viewer');
    assert.equal(engineRoutes.minimumRole(r['api/alpaca/assets'], 'GET'), 'viewer');
    assert.equal(engineRoutes.minimumRole(r['api/alpaca/stocks/{symbol}/bars'], 'GET'), 'viewer');
  });
});

describe('3. Paper Trading Environment Protection', () => {
  it('Rust engine uses paper trading base URL and never routes to live', () => {
    const rustSource = fs.readFileSync(
      path.resolve(root, '../../crates/spectra-api/src/broker_api.rs'),
      'utf8'
    );
    assert.ok(
      rustSource.includes('https://paper-api.alpaca.markets'),
      'broker_api.rs must route to https://paper-api.alpaca.markets'
    );
    assert.ok(
      !rustSource.includes('https://api.alpaca.markets/'),
      'broker_api.rs must NOT use live trading URL https://api.alpaca.markets'
    );
  });

  it('Next.js connect route validates paper API exclusively', () => {
    const connectRoute = fs.readFileSync(
      path.resolve(root, 'app/api/paper/alpaca/connect/route.ts'),
      'utf8'
    );
    assert.ok(
      connectRoute.includes('https://paper-api.alpaca.markets'),
      'connect/route.ts must test credentials against Alpaca paper endpoint'
    );
    assert.ok(
      connectRoute.includes('/v2/account'),
      'connect/route.ts must validate account endpoint'
    );
    assert.ok(
      !connectRoute.includes('https://api.alpaca.markets'),
      'connect/route.ts must NEVER test against live endpoint'
    );
  });
});

describe('4. TradingView-Style Interactive Chart & Symbols', () => {
  it('TradingChart component supports all required timeframes (1m, 5m, 15m, 1h, 1D, 1W, 1M)', () => {
    const chartSource = fs.readFileSync(
      path.resolve(root, 'components/studio/TradingChart.tsx'),
      'utf8'
    );
    const required = ['1m', '5m', '15m', '1h', '1D', '1W', '1M'];
    for (const tf of required) {
      assert.ok(chartSource.includes(`'${tf}'`), `TradingChart must support timeframe: ${tf}`);
    }
  });

  it('TradingChart provides crosshair, price scale, and volume histogram', () => {
    const chartSource = fs.readFileSync(
      path.resolve(root, 'components/studio/TradingChart.tsx'),
      'utf8'
    );
    assert.ok(chartSource.includes('crosshair'), 'TradingChart must track and draw crosshair');
    assert.ok(chartSource.includes('priceScaleWidth'), 'TradingChart must render dedicated price scale');
    assert.ok(chartSource.includes('volumeHeight'), 'TradingChart must render volume bars');
    assert.ok(chartSource.includes('handleZoom'), 'TradingChart must support zooming');
  });

  it('PaperLab supports symbol search and dynamic watchlist', () => {
    const labSource = fs.readFileSync(
      path.resolve(root, 'components/studio/PaperLab.tsx'),
      'utf8'
    );
    assert.ok(labSource.includes('searchQuery'), 'PaperLab must support dynamic stock search');
    assert.ok(labSource.includes('addToWatchlist'), 'PaperLab must allow adding symbols to watchlist');
    assert.ok(labSource.includes('removeFromWatchlist'), 'PaperLab must allow removing symbols from watchlist');
  });

  it('TradingChart clamps candle width and uses minSlots so single bars never expand to giant blocks', () => {
    const chartSource = fs.readFileSync(
      path.resolve(root, 'components/studio/TradingChart.tsx'),
      'utf8'
    );
    assert.ok(chartSource.includes('minSlots'), 'TradingChart must define minSlots for sparse data');
    assert.ok(chartSource.includes('candleSlotWidth'), 'TradingChart must space candles on a slot grid');
    assert.ok(chartSource.includes('LIVE'), 'TradingChart must display live real-time status pill');
  });

  it('alpaca.ts calculates start timestamps and PaperLab implements real-time auto-refresh', () => {
    const alpacaSource = fs.readFileSync(
      path.resolve(root, 'lib/alpaca.ts'),
      'utf8'
    );
    assert.ok(
      alpacaSource.includes('computeAlpacaStartTime'),
      'alpaca.ts must define computeAlpacaStartTime to prevent single-day truncation'
    );
    assert.ok(
      alpacaSource.includes('start='),
      'getAlpacaBars must pass start parameter to Alpaca Data API'
    );

    const labSource = fs.readFileSync(
      path.resolve(root, 'components/studio/PaperLab.tsx'),
      'utf8'
    );
    assert.ok(
      labSource.includes('tickInterval') || labSource.includes('setInterval'),
      'PaperLab must have real-time interval polling'
    );
    assert.ok(
      labSource.includes('backfillHistoricalBars'),
      'PaperLab must backfill historical bars if live feed returns sparse bars'
    );
    assert.ok(
      labSource.includes('SOL/USD'),
      'PaperLab must support Solana SOL/USD'
    );
    assert.ok(
      labSource.includes('getRealtimeQuote'),
      'PaperLab must call getRealtimeQuote for active live price ticks'
    );
    assert.ok(
      labSource.includes('priceTickFlash') || labSource.includes('priceFlash'),
      'PaperLab must implement price flash animations on live ticks'
    );
  });
});

describe('5. Order Execution Flow, Confirmation & Idempotency', () => {
  it('PaperLab implements confirmation modal before submitting orders', () => {
    const labSource = fs.readFileSync(
      path.resolve(root, 'components/studio/PaperLab.tsx'),
      'utf8'
    );
    assert.ok(labSource.includes('confirmModalOpen'), 'PaperLab must have confirmation modal state');
    assert.ok(labSource.includes('Confirm Paper Order'), 'PaperLab must show confirmation dialog');
  });

  it('prevents duplicate order submissions with disabled button & loading state', () => {
    const labSource = fs.readFileSync(
      path.resolve(root, 'components/studio/PaperLab.tsx'),
      'utf8'
    );
    assert.ok(
      labSource.includes('disabled={submittingOrder'),
      'Order submit button must be disabled during submission'
    );
  });

  it('supports closing positions with a single click', () => {
    const labSource = fs.readFileSync(
      path.resolve(root, 'components/studio/PaperLab.tsx'),
      'utf8'
    );
    assert.ok(labSource.includes('handleClosePosition'), 'PaperLab must support closing positions');
    assert.ok(labSource.includes('handleCancelOrder'), 'PaperLab must support cancelling open orders');
  });
});
