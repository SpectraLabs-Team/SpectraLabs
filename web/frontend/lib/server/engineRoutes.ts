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
  'api/v1/backtests': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/backtests/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/series': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/datasets': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  // Data Studio (M1). A preview stores nothing, but it is the first step of an upload: editors only.
  'api/v1/datasets/preview': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/datasets/ingest': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  // Resumable uploads (M1b): start, check, complete or abandon; chunks go through app/api/engine-upload/sessions.
  'api/v1/uploads/limits': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/uploads/sessions': { method: 'POST', minRole: 'editor', timeoutMs: 20_000 },
  'api/v1/uploads/sessions/{id}': { method: ['GET', 'DELETE'], minRole: 'editor', timeoutMs: 20_000 },
  // Hashes and stores the whole file (up to SPECTRA_MAX_UPLOAD_BYTES).
  'api/v1/uploads/sessions/{id}/complete': { method: 'POST', minRole: 'editor', timeoutMs: 300_000 },
  'api/v1/datasets/{id}': { method: ['GET', 'PATCH'], minRole: 'viewer', methodRoles: { PATCH: 'editor' }, timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  'api/v1/datasets/versions/{id}/derive': { method: 'POST', minRole: 'editor', timeoutMs: 60_000 },
  'api/v1/datasets/versions/{id}/runs': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/datasets/versions/{id}/bars': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  // Corporate actions & backward adjustments (M1b)
  'api/v1/corporate-actions': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/corporate-actions/{id}': { method: ['GET', 'DELETE'], minRole: 'viewer', methodRoles: { DELETE: 'editor' }, timeoutMs: 20_000 },
  // Symbol changes, delistings & survivorship-free universes (M1b task 12)
  'api/v1/symbol-changes': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/symbol-changes/{id}': { method: 'DELETE', minRole: 'editor', timeoutMs: 20_000 },
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

/**
 * File uploads don't go through the JSON proxy (2 MB bodies): app/api/engine-upload streams the file
 * (up to 100 MiB) to POST /api/v1/uploads. Editors only, signed-in sessions only.
 */
export const ENGINE_UPLOAD_ROUTE = { path: 'api/v1/uploads', minRole: 'editor' as WorkspaceRole, timeoutMs: 300_000 };

/** Chunks of a resumable upload (app/api/engine-upload/sessions/[id]): at most 8 MiB each. Editors only. */
export const ENGINE_CHUNK_ROUTE = { minRole: 'editor' as WorkspaceRole, timeoutMs: 120_000, maxBytes: 8 * 1024 * 1024 };

/** Map UUID job URLs to their allowlist policy while preserving the real path for signing. */
export function routePolicyKey(path: string): string {
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}\/cancel$/i.test(path)) return 'api/v1/jobs/{id}/cancel';
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/jobs/{id}';
  for (const [pattern, key] of REGISTRY_PATTERNS) if (pattern.test(path)) return key;
  // Only the stored files the API serves; any other name stays unlisted (a 404 here).
  if (/^api\/v1\/backtests\/[0-9a-f-]{36}\/artifacts\/(summary|request|equity_csv|orders_csv|fills_csv|returns_csv|strategies_csv)$/i.test(path)) {
    return 'api/v1/backtests/{id}/artifacts/{kind}';
  }
  const backtest = /^api\/v1\/backtests\/[0-9a-f-]{36}(\/cancel|\/series)?$/i.exec(path);
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
  const version = /^api\/v1\/datasets\/versions\/[0-9a-f-]{36}(\/runs|\/bars|\/derive)?$/i.exec(path);
  if (version) return `api/v1/datasets/versions/{id}${version[1] ?? ''}`;
  if (/^api\/v1\/datasets\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/datasets/{id}';
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
