import type { WorkspaceRole } from './workspaces';

/**
 * Allowlist of engine routes the /api/engine proxy forwards: method, timeout, and the minimum
 * workspace role (in the caller's active workspace) needed to call it. Anything not listed is a
 * 404, so new engine routes stay private until added here.
 *
 *   viewer → reads (sample data, telemetry, datasets, backtests, and the research registry)
 *   editor → computations ("create and run"), data uploads, registry writes and broker account data
 *   owner  → whole-registry operations (verify the chain, import a registry.jsonl)
 */
export type EngineMethod = 'GET' | 'POST' | 'PATCH' | 'DELETE' | 'PUT';
export type EngineRoute = {
  method: EngineMethod | EngineMethod[];
  public?: boolean;
  minRole: WorkspaceRole;
  methodRoles?: Partial<Record<EngineMethod, WorkspaceRole>>;
  timeoutMs: number;
  heavy?: boolean;
  maxBodyBytes?: number;
};

const MB = 1024 * 1024;

export const ENGINE_ROUTES: Record<string, EngineRoute> = {
  health: { method: 'GET', public: true, minRole: 'viewer', timeoutMs: 4_000 },
  'api/sample-portfolio': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/telemetry/cpu': { method: 'GET', minRole: 'viewer', timeoutMs: 4_000 },
  'api/risk': { method: 'POST', minRole: 'editor', timeoutMs: 60_000 },
  'api/stress': { method: 'POST', minRole: 'editor', timeoutMs: 60_000 },
  'api/benchmark': { method: 'POST', minRole: 'editor', timeoutMs: 130_000, heavy: true },
  'api/alpaca/account': { method: 'GET', minRole: 'editor', timeoutMs: 15_000 },
  'api/alpaca/positions': { method: 'GET', minRole: 'editor', timeoutMs: 15_000 },
  'api/alpaca/orders': { method: ['GET', 'POST'], minRole: 'editor', timeoutMs: 20_000 },
  'api/alpaca/orders/{id}': { method: 'DELETE', minRole: 'editor', timeoutMs: 15_000 },
  'api/alpaca/sync': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/alpaca/reconcile': { method: 'GET', minRole: 'editor', timeoutMs: 20_000 },
  'api/alpaca/assets': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/alpaca/stocks/{symbol}/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/alpaca/crypto/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/alpaca/crypto/latest/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/alpaca/account': { method: 'GET', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/alpaca/positions': { method: 'GET', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/alpaca/orders': { method: ['GET', 'POST'], minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/alpaca/orders/{id}': { method: 'DELETE', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/alpaca/sync': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/alpaca/reconcile': { method: 'GET', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/alpaca/assets': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/alpaca/stocks/{symbol}/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/alpaca/crypto/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/alpaca/crypto/latest/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/jobs': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/jobs/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/jobs/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  // M5 experiment registry, always scoped to the caller's active workspace by the engine.
  'api/v1/experiments': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/experiments/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/experiments/{id}/lineage': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/experiments/{id}/runs': { method: 'POST', minRole: 'editor', timeoutMs: 60_000, maxBodyBytes: 11 * MB },
  'api/v1/runs': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 60_000, maxBodyBytes: 11 * MB },
  'api/v1/runs/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/runs/{id}/notes': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/runs/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/runs/{id}/survival': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/survival/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/survival/{id}/text': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/survival/presets': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/survival/presets/{id}': { method: 'PATCH', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/registry/verify': { method: 'GET', minRole: 'owner', timeoutMs: 60_000 },
  'api/v1/registry/export': { method: 'GET', minRole: 'viewer', timeoutMs: 60_000 },
  'api/v1/registry/import': { method: 'POST', minRole: 'owner', timeoutMs: 120_000, maxBodyBytes: 51 * MB },
  'api/v1/backtests': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/backtests/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/series': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/attribution': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/monthly-returns': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/datasets': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/datasets/preview': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/datasets/ingest': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/uploads/limits': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/uploads/sessions': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/uploads/sessions/{id}': { method: ['GET', 'DELETE'], minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/uploads/sessions/{id}/complete': { method: 'POST', minRole: 'editor', timeoutMs: 300_000 },
  'api/v1/datasets/{id}': { method: ['GET', 'PATCH'], minRole: 'viewer', methodRoles: { PATCH: 'editor' }, timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/datasets/versions/{id}/derive': { method: 'POST', minRole: 'editor', timeoutMs: 60_000 },
  'api/v1/datasets/versions/{id}/export': { method: 'GET', minRole: 'viewer', timeoutMs: 60_000 },
  'api/v1/datasets/versions/{id}/runs': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  // Corporate actions & backward adjustments (M1b)
  'api/v1/corporate-actions': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/corporate-actions/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  // Symbol changes, delistings & survivorship-free universes (M1b task 12)
  'api/v1/symbol-changes': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/symbol-changes/{id}': { method: 'DELETE', minRole: 'editor', timeoutMs: 20_000 },
  // Multi-asset instruments, base currency, FX rates, curves and pricing (M24)
  'api/v1/instruments': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/instruments/{symbol}': { method: 'DELETE', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/base-currency': { method: ['GET', 'PUT'], minRole: 'viewer', methodRoles: { PUT: 'owner' }, timeoutMs: 10_000 },
  'api/v1/fx-rates': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000, maxBodyBytes: 5 * MB },
  'api/v1/curves': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000, maxBodyBytes: 5 * MB },
  'api/v1/curves/{name}': { method: 'DELETE', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/pricing/bond': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/pricing/option': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/pricing/curve': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/pricing/swap': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/delistings': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/delistings/{id}': { method: 'DELETE', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/universes': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/universes/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/universes/{id}/members': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/universes/{id}/as-of': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  // Market data providers & automated refresh schedules (M1b task 13)
  'api/v1/providers/configs': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/providers/configs/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/datasets/{id}/refresh-schedule': { method: ['GET', 'POST', 'DELETE'], minRole: 'viewer', methodRoles: { POST: 'editor', DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/datasets/{id}/refresh': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/calendars': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/strategies/builtin': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  // Saved strategies and their immutable versions (M2): reads are viewer, saving is editor.
  'api/v1/strategies': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/strategies/validate': { method: 'POST', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/strategies/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/strategies/{id}/draft': { method: ['GET', 'PUT'], minRole: 'viewer', methodRoles: { PUT: 'editor' }, timeoutMs: 15_000 },
  'api/v1/strategies/{id}/diff': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/strategies/{id}/versions': { method: 'POST', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/strategies/versions/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/strategies/{id}/versions/{versionId}/parameter_sets': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/strategies/parameter_sets/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  // Module 4: Parameter Sweeps, Walk-Forward Validation, Overfitting, Holdout Vault
  'api/v1/optimization/sweep': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/optimization/sweeps': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/optimization/sweep/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/optimization/walkforward': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 30_000 },
  'api/v1/optimization/walkforward/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/optimization/overfitting': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/optimization/holdout': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/optimization/holdout/seal': { method: 'POST', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/optimization/holdout/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/optimization/holdout/{id}/evaluate': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  // Module 8: Risk Center and Enforcement
  'api/v1/risk/limits': { method: ['GET', 'PUT'], minRole: 'viewer', methodRoles: { PUT: 'editor' }, timeoutMs: 15_000 },
  'api/v1/risk/check': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/risk/kill-switch': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/risk/audit': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/risk/metrics': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/risk/positions': { method: ['GET', 'PUT'], minRole: 'viewer', methodRoles: { PUT: 'editor' }, timeoutMs: 15_000 },
  'api/v1/risk/balance': { method: ['GET', 'PUT'], minRole: 'viewer', methodRoles: { PUT: 'editor' }, timeoutMs: 15_000 },
  'api/v1/risk/exposure-history': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/risk/alerts': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/risk/replay/{id}': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/risk/regimes': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/risk/garch': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/risk/evt': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  // Module 9: Paper Trading & Live Ledger
  'api/v1/paper/account': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/paper/attribution': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/paper/account/reset': { method: 'POST', minRole: 'editor', timeoutMs: 15_000 },
  'api/v1/paper/orders': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/paper/orders/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/paper/step': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/paper/sync': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/paper/reconcile': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  // Module 10: Backtest-to-Paper Gap Analysis
  'api/v1/paper/gap/analyze': { method: 'POST', minRole: 'editor', timeoutMs: 30_000 },
  'api/v1/paper/gap/reports': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/paper/gap/reports/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  // Per-user state and sandbox trades (M25)
  'api/v1/me/state': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/state/import': { method: 'POST', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/state/{key}': { method: 'PUT', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/sandbox/orders': { method: ['GET', 'POST', 'DELETE'], minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/sandbox/orders/{id}': { method: 'PATCH', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/sandbox/transact': { method: 'POST', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/runs': { method: 'GET', minRole: 'viewer', timeoutMs: 15_000 },
  'api/v1/me/runs/{id}/hide': { method: 'POST', minRole: 'viewer', timeoutMs: 15_000 },
};

export const ENGINE_UPLOAD_ROUTE = { path: 'api/v1/uploads', minRole: 'editor' as WorkspaceRole, timeoutMs: 300_000 };
export const ENGINE_CHUNK_ROUTE = { minRole: 'editor' as WorkspaceRole, timeoutMs: 120_000, maxBytes: 8 * MB };

const ID = '(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[a-z]{2,8}-[0-9a-f]{16})';
const REGISTRY_PATTERNS: [RegExp, string][] = [
  [new RegExp(`^api/v1/experiments/${ID}$`, 'i'), 'api/v1/experiments/{id}'],
  [new RegExp(`^api/v1/experiments/${ID}/lineage$`, 'i'), 'api/v1/experiments/{id}/lineage'],
  [new RegExp(`^api/v1/experiments/${ID}/runs$`, 'i'), 'api/v1/experiments/{id}/runs'],
  [new RegExp(`^api/v1/runs/${ID}$`, 'i'), 'api/v1/runs/{id}'],
  [new RegExp(`^api/v1/runs/${ID}/notes$`, 'i'), 'api/v1/runs/{id}/notes'],
  [new RegExp(`^api/v1/runs/${ID}/artifacts/[a-z][a-z0-9_]{0,63}$`, 'i'), 'api/v1/runs/{id}/artifacts/{kind}'],
  [new RegExp(`^api/v1/runs/${ID}/survival$`, 'i'), 'api/v1/runs/{id}/survival'],
  [new RegExp(`^api/v1/survival/${ID}$`, 'i'), 'api/v1/survival/{id}'],
  [new RegExp(`^api/v1/survival/${ID}/text$`, 'i'), 'api/v1/survival/{id}/text'],
  [new RegExp(`^api/v1/survival/presets/${ID}$`, 'i'), 'api/v1/survival/presets/{id}'],
];

export function routePolicyKey(path: string): string {
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}\/cancel$/i.test(path)) return 'api/v1/jobs/{id}/cancel';
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/jobs/{id}';
  if (/^api\/(?:v1\/)?alpaca\/orders\/[^/]+$/i.test(path)) {
    return path.startsWith('api/v1/') ? 'api/v1/alpaca/orders/{id}' : 'api/alpaca/orders/{id}';
  }
  if (/^api\/(?:v1\/)?alpaca\/stocks\/[^/]+\/bars$/i.test(path)) {
    return path.startsWith('api/v1/') ? 'api/v1/alpaca/stocks/{symbol}/bars' : 'api/alpaca/stocks/{symbol}/bars';
  }
  if (/^api\/(?:v1\/)?alpaca\/crypto\/bars$/i.test(path)) {
    return path.startsWith('api/v1/') ? 'api/v1/alpaca/crypto/bars' : 'api/alpaca/crypto/bars';
  }
  if (/^api\/(?:v1\/)?alpaca\/crypto\/latest\/bars$/i.test(path)) {
    return path.startsWith('api/v1/') ? 'api/v1/alpaca/crypto/latest/bars' : 'api/alpaca/crypto/latest/bars';
  }
  if (/^api\/v1\/paper\/orders\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/paper/orders/{id}';
  if (/^api\/v1\/paper\/gap\/reports\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/paper/gap/reports/{id}';
  if (/^api\/v1\/me\/state\/import$/i.test(path)) return 'api/v1/me/state/import';
  if (/^api\/v1\/me\/state\/[^/]+$/i.test(path)) return 'api/v1/me/state/{key}';
  if (/^api\/v1\/me\/sandbox\/orders\/[^/]+$/i.test(path)) return 'api/v1/me/sandbox/orders/{id}';
  if (/^api\/v1\/me\/runs\/[^/]+\/hide$/i.test(path)) return 'api/v1/me/runs/{id}/hide';
  if (/^api\/v1\/me\/runs$/i.test(path)) return 'api/v1/me/runs';
  for (const [pattern, key] of REGISTRY_PATTERNS) if (pattern.test(path)) return key;
  // Only the stored files the API serves; any other name stays unlisted (a 404 here).
  if (/^api\/v1\/backtests\/[0-9a-f-]{36}\/artifacts\/(summary|request|equity_csv|orders_csv|fills_csv|returns_csv|strategies_csv)$/i.test(path)) {
    return 'api/v1/backtests/{id}/artifacts/{kind}';
  }
  const backtest = /^api\/v1\/backtests\/[0-9a-f-]{36}(\/cancel|\/series|\/monthly-returns|\/attribution)?$/i.exec(path);
  if (backtest) return `api/v1/backtests/{id}${backtest[1] ?? ''}`;
  const session = /^api\/v1\/uploads\/sessions\/[0-9a-f-]{36}(\/complete)?$/i.exec(path);
  if (session) return `api/v1/uploads/sessions/{id}${session[1] ?? ''}`;
  if (/^api\/v1\/corporate-actions\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/corporate-actions/{id}';
  if (/^api\/v1\/symbol-changes\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/symbol-changes/{id}';
  if (/^api\/v1\/delistings\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/delistings/{id}';
  if (/^api\/v1\/providers\/configs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/providers/configs/{id}';
  const refresh = /^api\/v1\/datasets\/[0-9a-f-]{36}(\/refresh-schedule|\/refresh)?$/i.exec(path);
  if (refresh && refresh[1]) return `api/v1/datasets/{id}${refresh[1]}`;
  const universe = /^api\/v1\/universes\/[0-9a-f-]{36}(\/members|\/as-of)?$/i.exec(path);
  if (universe) return `api/v1/universes/{id}${universe[1] ?? ''}`;
  if (/^api\/v1\/strategies\/validate$/i.test(path)) return 'api/v1/strategies/validate';
  if (/^api\/v1\/strategies\/parameter_sets\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/strategies/parameter_sets/{id}';
  if (/^api\/v1\/strategies\/versions\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/strategies/versions/{id}';
  const paramSets = /^api\/v1\/strategies\/[0-9a-f-]{36}\/versions\/[0-9a-f-]{36}\/parameter_sets$/i.exec(path);
  if (paramSets) return 'api/v1/strategies/{id}/versions/{versionId}/parameter_sets';
  const strategy = /^api\/v1\/strategies\/[0-9a-f-]{36}(\/versions|\/draft|\/diff)?$/i.exec(path);
  if (strategy) return `api/v1/strategies/{id}${strategy[1] ?? ''}`;
  if (/^api\/v1\/optimization\/sweep\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/optimization/sweep/{id}';
  if (/^api\/v1\/optimization\/walkforward\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/optimization/walkforward/{id}';
  const holdout = /^api\/v1\/optimization\/holdout\/[0-9a-f-]{36}(\/evaluate)?$/i.exec(path);
  if (holdout) return `api/v1/optimization/holdout/{id}${holdout[1] ?? ''}`;
  const version = /^api\/v1\/datasets\/versions\/[0-9a-f-]{36}(\/runs|\/bars|\/derive|\/export)?$/i.exec(path);
  if (version) return `api/v1/datasets/versions/{id}${version[1] ?? ''}`;
  if (/^api\/v1\/datasets\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/datasets/{id}';
  return path;
}

export function allowsMethod(route: EngineRoute, method: EngineMethod): boolean {
  return Array.isArray(route.method) ? route.method.includes(method) : route.method === method;
}
export function maxBodyBytes(route: EngineRoute): number { return route.maxBodyBytes ?? 2 * MB; }
export function minimumRole(route: EngineRoute, method: EngineMethod): WorkspaceRole { return route.methodRoles?.[method] ?? route.minRole; }
