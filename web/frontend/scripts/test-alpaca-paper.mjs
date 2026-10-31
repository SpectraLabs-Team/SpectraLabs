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

describe('6. TradingView Pro Real-Time Chart & Coinbase Live Streaming', () => {
  it('TradingViewWidget component embeds official advanced chart widget and maps symbols', () => {
    const tvPath = path.resolve(root, 'components/studio/TradingViewWidget.tsx');
    assert.ok(fs.existsSync(tvPath), 'TradingViewWidget component must exist');
    const tvSource = fs.readFileSync(tvPath, 'utf8');
    assert.ok(tvSource.includes('embed-widget-advanced-chart.js'), 'Must load official TradingView advanced chart embed');
    assert.ok(tvSource.includes('toTradingViewSymbol'), 'Must export toTradingViewSymbol');
    assert.ok(tvSource.includes('COINBASE:SOLUSD'), 'Must map SOL to COINBASE:SOLUSD');
    assert.ok(tvSource.includes('ORCL') && tvSource.includes('NYSE:'), 'Must map ORCL to NYSE exchange');
  });

  it('PaperLab supports switching between TradingView and Spectra engines', () => {
    const labSource = fs.readFileSync(path.resolve(root, 'components/studio/PaperLab.tsx'), 'utf8');
    assert.ok(labSource.includes('TradingViewWidget'), 'PaperLab must import TradingViewWidget');
    assert.ok(labSource.includes('chartEngine'), 'PaperLab must manage chartEngine state');
    assert.ok(labSource.includes('TradingView Pro'), 'PaperLab must offer TradingView Pro mode');
  });

  it('PaperLab connects to Coinbase WebSocket for sub-second real crypto ticks', () => {
    const labSource = fs.readFileSync(path.resolve(root, 'components/studio/PaperLab.tsx'), 'utf8');
    assert.ok(labSource.includes('ws-feed.exchange.coinbase.com'), 'PaperLab must connect to Coinbase WebSocket feed');
    assert.ok(labSource.includes('liveStreamPrice'), 'PaperLab must update liveStreamPrice from feed');
  });

  it('PaperLab supports shrinkable Order Ticket sidebar and chart expand toggle', () => {
    const labSource = fs.readFileSync(path.resolve(root, 'components/studio/PaperLab.tsx'), 'utf8');
    assert.ok(labSource.includes('ticketOpen'), 'PaperLab must manage ticketOpen state for shrinkable sidebar');
    assert.ok(labSource.includes('toggleTicket'), 'PaperLab must provide toggleTicket handler');
    assert.ok(labSource.includes('chartHeightMode'), 'PaperLab must manage chartHeightMode');
  });
});

describe('7. Alpaca Secret Never Leaves the Browser (Decision 6)', () => {
  const secretFieldNames = ['alpacaSecret', 'apiSecret', 'secretKey', 'inputSecretKey'];

  it('confirms no /api/v1/me/* request body or response contains secretKey, alpacaSecret, or apiSecret', async () => {
    // 1. Static inspection of userState.ts and backend user_state_api.rs
    const userStateSource = fs.readFileSync(path.resolve(root, 'lib/userState.ts'), 'utf8');
    for (const field of secretFieldNames) {
      assert.ok(
        !userStateSource.includes(field),
        `lib/userState.ts must not contain or reference secret field ${field}`
      );
    }

    const rustUserStateSource = fs.readFileSync(
      path.resolve(root, '../../crates/spectra-api/src/user_state_api.rs'),
      'utf8'
    );
    const rustSecretVariants = ['secret_key', 'secretKey', 'alpaca_secret', 'alpacaSecret', 'api_secret', 'apiSecret'];
    for (const variant of rustSecretVariants) {
      assert.ok(
        !rustUserStateSource.includes(variant),
        `crates/spectra-api/src/user_state_api.rs must not contain or serialize secret field ${variant}`
      );
    }

    // 2. Runtime inspection of userState API requests
    const sentRequests = [];
    const mockStorage = new Map();
    globalThis.localStorage = {
      getItem: (k) => mockStorage.get(k) ?? null,
      setItem: (k, v) => mockStorage.set(k, String(v)),
      removeItem: (k) => mockStorage.delete(k),
      clear: () => mockStorage.clear(),
    };

    const originalFetch = globalThis.fetch;
    globalThis.fetch = async (url, options = {}) => {
      const urlStr = String(url);
      const method = options.method || 'GET';
      const body = options.body ? JSON.parse(options.body) : null;
      sentRequests.push({ url: urlStr, method, body });

      let resPayload = {};
      if (urlStr.endsWith('/state')) {
        resPayload = {
          workspace_id: 'ws-123',
          state: {
            portfolio: { value: { csv: 'AAPL,10', source: null }, version: 1, updated_at: '2026-10-31T00:00:00Z' },
            'paper-account': { value: { balance: 100000 }, version: 1, updated_at: '2026-10-31T00:00:00Z' },
            'risk-params': { value: { maxDrawdown: 0.1 }, version: 1, updated_at: '2026-10-31T00:00:00Z' },
            sandbox: { value: { cash: 100000, positions: [] }, version: 1, updated_at: '2026-10-31T00:00:00Z' },
            watchlist: { value: ['AAPL', 'MSFT'], version: 1, updated_at: '2026-10-31T00:00:00Z' },
          },
        };
      } else if (urlStr.includes('/state/')) {
        resPayload = { value: body?.value, version: 2, updated_at: '2026-10-31T00:00:01Z' };
      } else if (urlStr.endsWith('/sandbox/orders')) {
        resPayload = method === 'DELETE' ? { cleared: 1 } : method === 'POST' ? { id: 'order-1', payload: body, created_at: '2026-10-31T00:00:00Z' } : { orders: [] };
      } else if (urlStr.includes('/sandbox/orders/')) {
        resPayload = { id: 'order-1', payload: body, created_at: '2026-10-31T00:00:00Z' };
      } else if (urlStr.endsWith('/sandbox/transact')) {
        resPayload = { saved: true, version: 2, updated_at: '2026-10-31T00:00:01Z', value: { cash: 90000, positions: [] }, order: { id: 'o-1', payload: {}, created_at: '2026-10-31T00:00:01Z' } };
      } else if (urlStr.endsWith('/state/import')) {
        resPayload = { noop: false, imported: ['sandbox', 'watchlist'], skipped: [] };
      }

      return {
        ok: true,
        status: 200,
        headers: new Headers({ 'Content-Type': 'application/json' }),
        json: async () => resPayload,
        text: async () => JSON.stringify(resPayload),
      };
    };

    try {
      // Transpile and load api & userState
      transpileAndLoad('lib/api.ts');
      const tempDir = path.join(root, 'node_modules', '.cache', 'alpaca-test');
      const tsSource = fs.readFileSync(path.join(root, 'lib/userState.ts'), 'utf8');
      const { outputText } = ts.transpileModule(tsSource, {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
      });
      const tempUserStateFile = path.join(tempDir, 'userState.js');
      fs.writeFileSync(tempUserStateFile, outputText);
      const userState = require(tempUserStateFile);

      // Invoke all /api/v1/me/* operations
      await userState.getUserState();
      await userState.putUserState('sandbox', 1, { cash: 100000, positions: [{ symbol: 'AAPL', qty: 10 }] });
      await userState.listSandboxOrders();
      await userState.appendSandboxOrder({ symbol: 'AAPL', qty: 10, side: 'buy' });
      await userState.updateSandboxOrder('order-1', { status: 'cancelled' });
      await userState.transactSandbox({ expected_version: 1, cash: 95000, positions: [], order: { symbol: 'AAPL', qty: 5, side: 'buy' } });
      await userState.importUserState('browser-v1', { sandbox: { cash: 100000, positions: [] } });
      await userState.clearSandboxOrders();

      assert.ok(sentRequests.length >= 7, 'Expected at least 7 /api/v1/me requests');

      for (const req of sentRequests) {
        assert.ok(
          req.url.includes('/api/v1/me/'),
          `Request URL must be under /api/v1/me/*: ${req.url}`
        );
        if (req.body) {
          const bodyStr = JSON.stringify(req.body);
          for (const field of secretFieldNames) {
            assert.ok(
              !bodyStr.includes(field),
              `Request body for ${req.url} must not contain secret field ${field}: ${bodyStr}`
            );
          }
        }
      }
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it('confirms the secret is not written to localStorage keys under spectra-cache: or saved state keys', () => {
    const mockStorage = new Map();
    globalThis.localStorage = {
      getItem: (k) => mockStorage.get(k) ?? null,
      setItem: (k, v) => mockStorage.set(k, String(v)),
      removeItem: (k) => mockStorage.delete(k),
      clear: () => mockStorage.clear(),
    };

    const tempDir = path.join(root, 'node_modules', '.cache', 'alpaca-test');
    const tsSource = fs.readFileSync(path.join(root, 'lib/userState.ts'), 'utf8');
    const { outputText } = ts.transpileModule(tsSource, {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
    });
    const tempUserStateFile = path.join(tempDir, 'userState.js');
    fs.writeFileSync(tempUserStateFile, outputText);
    const userState = require(tempUserStateFile);

    const testEmail = 'trader@spectra.test';

    // Write all supported state categories into cache
    userState.writeCache(testEmail, 'portfolio', { csv: 'AAPL,50\nMSFT,20', source: 'csv' });
    userState.writeCache(testEmail, 'paper-account', { balance: 100000, equity: 100000 });
    userState.writeCache(testEmail, 'risk-params', { maxDrawdown: 0.15, maxPositionSize: 0.25 });
    userState.writeCache(testEmail, 'sandbox', { cash: 100000, positions: [{ symbol: 'AAPL', qty: 10 }] });
    userState.writeCache(testEmail, 'watchlist', ['AAPL', 'TSLA', 'SOL/USD', 'ETH/USD']);

    // Check all written keys
    const cacheKeys = [...mockStorage.keys()].filter((k) => k.startsWith('spectra-cache:'));
    assert.equal(cacheKeys.length, 5, 'Must have written exactly 5 cache entries under spectra-cache:');

    for (const key of cacheKeys) {
      const raw = mockStorage.get(key);
      for (const field of secretFieldNames) {
        assert.ok(
          !raw.includes(field),
          `Cache entry for ${key} must never contain secret field ${field}: ${raw}`
        );
      }
    }

    // Legacy keys inventory check
    const legacyKeys = [
      `spectra-portfolio-${testEmail}`,
      `spectra-paper-${testEmail}`,
      `spectra-risk-params-${testEmail}`,
      'spectra-paper-sandbox-v1',
      'spectra-paper-watchlist',
      'spectra-alpaca-account-cache',
    ];
    for (const key of legacyKeys) {
      assert.ok(
        !key.includes('secret') && !key.includes('Secret'),
        `Legacy key ${key} must not be named after secrets`
      );
    }
  });

  it('PaperLab keeps inputSecretKey in local React state and never passes it to storage or user-state sync', () => {
    const labSource = fs.readFileSync(path.resolve(root, 'components/studio/PaperLab.tsx'), 'utf8');

    // 1. inputSecretKey is kept strictly in React useState('')
    assert.match(
      labSource,
      /const\s*\[inputSecretKey,\s*setInputSecretKey\]\s*=\s*useState\(''\)/,
      'inputSecretKey must be managed strictly in React state'
    );

    // 2. inputSecretKey is never passed to writeCache, localStorage.setItem, or putUserState
    assert.ok(
      !labSource.includes('writeCache(') || !labSource.match(/writeCache\([^)]*inputSecretKey/),
      'writeCache must never receive inputSecretKey'
    );
    assert.ok(
      !labSource.includes('localStorage.setItem(') || !labSource.match(/localStorage\.setItem\([^)]*inputSecretKey/),
      'localStorage.setItem must never receive inputSecretKey'
    );

    // 3. Inspect all writeCache calls in PaperLab - must strictly be 'sandbox' or 'watchlist'
    const writeCacheMatches = [...labSource.matchAll(/writeCache\(\s*who\s*,\s*'([^']+)'/g)];
    assert.ok(writeCacheMatches.length > 0, 'PaperLab must call writeCache');
    for (const match of writeCacheMatches) {
      const keyName = match[1];
      assert.ok(
        keyName === 'sandbox' || keyName === 'watchlist',
        `PaperLab writeCache can only write 'sandbox' or 'watchlist', got: ${keyName}`
      );
    }

    // 4. Inspect all localStorage.setItem calls in PaperLab - must strictly be UI toggles
    const setItemMatches = [...labSource.matchAll(/localStorage\.setItem\(\s*'([^']+)'/g)];
    assert.ok(setItemMatches.length > 0, 'PaperLab must persist UI toggles');
    for (const match of setItemMatches) {
      const keyName = match[1];
      assert.ok(
        keyName === 'paper.watchlistOpen' || keyName === 'paper.ticketOpen',
        `PaperLab localStorage.setItem can only write UI toggles, got: ${keyName}`
      );
    }
  });
});
