import type { WorkspaceRole } from './workspaces';

/**
 * Allowlist of engine routes the /api/engine proxy forwards: method, timeout, and the minimum
 * workspace role (in the caller's active workspace) needed to call it. Anything not listed is a
 * 404, so new engine routes stay private until added here.
 *
<<<<<<< HEAD
 *   viewer → reads (sample data, telemetry, the research registry)
 *   editor → computations ("create and run"), registry writes and broker account data
 *   owner  → whole-registry operations (verify the chain, import a registry.jsonl)
=======
 *   viewer → reads (sample data, telemetry, datasets and backtests)
 *   editor → computations ("create and run"), data uploads, renaming and deleting datasets, and
 *            broker account data
>>>>>>> origin/main
 */
export type EngineMethod = 'GET' | 'POST' | 'PATCH' | 'DELETE';
export type EngineRoute = {
  method: EngineMethod | EngineMethod[];
  public?: boolean;
  minRole: WorkspaceRole;
  methodRoles?: Partial<Record<EngineMethod, WorkspaceRole>>;
  timeoutMs: number;
  heavy?: boolean;
  /** Largest request body the proxy forwards (default 2 MB). */
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
  'api/alpaca/account': { method: 'GET', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/jobs': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/jobs/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/jobs/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
<<<<<<< HEAD
  // M5 experiment registry, always scoped to the caller's active workspace by the engine.
  'api/v1/experiments': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 15_000 },
  'api/v1/experiments/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/experiments/{id}/lineage': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/experiments/{id}/runs': { method: 'POST', minRole: 'editor', timeoutMs: 60_000, maxBodyBytes: 11 * MB },
  'api/v1/runs': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 60_000, maxBodyBytes: 11 * MB },
  'api/v1/runs/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/runs/{id}/notes': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/runs/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/registry/verify': { method: 'GET', minRole: 'owner', timeoutMs: 60_000 },
  'api/v1/registry/export': { method: 'GET', minRole: 'viewer', timeoutMs: 60_000 },
  'api/v1/registry/import': { method: 'POST', minRole: 'owner', timeoutMs: 120_000, maxBodyBytes: 51 * MB },
};

// Registry ids: UUIDs for records made here, exp-/run-/art-<16 hex> for ones imported from a
// file registry. Artifact kinds are short snake_case words.
const ID = '(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[a-z]{2,8}-[0-9a-f]{16})';
const REGISTRY_PATTERNS: [RegExp, string][] = [
  [new RegExp(`^api/v1/experiments/${ID}$`, 'i'), 'api/v1/experiments/{id}'],
  [new RegExp(`^api/v1/experiments/${ID}/lineage$`, 'i'), 'api/v1/experiments/{id}/lineage'],
  [new RegExp(`^api/v1/experiments/${ID}/runs$`, 'i'), 'api/v1/experiments/{id}/runs'],
  [new RegExp(`^api/v1/runs/${ID}$`, 'i'), 'api/v1/runs/{id}'],
  [new RegExp(`^api/v1/runs/${ID}/notes$`, 'i'), 'api/v1/runs/{id}/notes'],
  [new RegExp(`^api/v1/runs/${ID}/artifacts/[a-z][a-z0-9_]{0,63}$`, 'i'), 'api/v1/runs/{id}/artifacts/{kind}'],
];
=======
  'api/v1/backtests': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/backtests/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/series': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/datasets': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  // Data Studio (M1). A preview stores nothing, but it is the first step of an upload: editors only.
  'api/v1/datasets/preview': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/datasets/ingest': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/datasets/{id}': { method: ['GET', 'PATCH'], minRole: 'viewer', methodRoles: { PATCH: 'editor' }, timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/datasets/versions/{id}/runs': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/calendars': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/strategies/builtin': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
};

/**
 * File uploads don't go through the JSON proxy (2 MB bodies): app/api/engine-upload streams the file
 * (up to 100 MiB) to POST /api/v1/uploads. Editors only, signed-in sessions only.
 */
export const ENGINE_UPLOAD_ROUTE = { path: 'api/v1/uploads', minRole: 'editor' as WorkspaceRole, timeoutMs: 300_000 };
>>>>>>> origin/main

/** Map UUID job URLs to their allowlist policy while preserving the real path for signing. */
export function routePolicyKey(path: string): string {
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}\/cancel$/i.test(path)) return 'api/v1/jobs/{id}/cancel';
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/jobs/{id}';
<<<<<<< HEAD
  for (const [pattern, key] of REGISTRY_PATTERNS) if (pattern.test(path)) return key;
=======
  // Only the stored files the API serves; any other name stays unlisted (a 404 here).
  if (/^api\/v1\/backtests\/[0-9a-f-]{36}\/artifacts\/(summary|request|equity_csv|orders_csv|fills_csv|returns_csv|strategies_csv)$/i.test(path)) {
    return 'api/v1/backtests/{id}/artifacts/{kind}';
  }
  const backtest = /^api\/v1\/backtests\/[0-9a-f-]{36}(\/cancel|\/series)?$/i.exec(path);
  if (backtest) return `api/v1/backtests/{id}${backtest[1] ?? ''}`;
  const version = /^api\/v1\/datasets\/versions\/[0-9a-f-]{36}(\/runs|\/bars)?$/i.exec(path);
  if (version) return `api/v1/datasets/versions/{id}${version[1] ?? ''}`;
  if (/^api\/v1\/datasets\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/datasets/{id}';
>>>>>>> origin/main
  return path;
}

export function allowsMethod(route: EngineRoute, method: EngineMethod): boolean {
  return Array.isArray(route.method) ? route.method.includes(method) : route.method === method;
}

export function maxBodyBytes(route: EngineRoute): number {
  return route.maxBodyBytes ?? 2 * MB;
}

export function minimumRole(route: EngineRoute, method: EngineMethod): WorkspaceRole {
  return route.methodRoles?.[method] ?? route.minRole;
}
