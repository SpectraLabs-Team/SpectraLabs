import type { WorkspaceRole } from './workspaces';

/**
 * Allowlist of engine routes the /api/engine proxy forwards: method, timeout, and the minimum
 * workspace role (in the caller's active workspace) needed to call it. Anything not listed is a
 * 404, so new engine routes stay private until added here.
 *
 *   viewer → reads (sample data, telemetry)
 *   editor → computations ("create and run") and broker account data
 */
export type EngineMethod = 'GET' | 'POST';
export type EngineRoute = {
  method: EngineMethod | EngineMethod[];
  public?: boolean;
  minRole: WorkspaceRole;
  methodRoles?: Partial<Record<EngineMethod, WorkspaceRole>>;
  timeoutMs: number;
  heavy?: boolean;
};

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
  'api/v1/backtests': { method: ['GET', 'POST'], minRole: 'viewer', methodRoles: { POST: 'editor' }, timeoutMs: 20_000 },
  'api/v1/backtests/{id}': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/cancel': { method: 'POST', minRole: 'editor', timeoutMs: 10_000 },
  'api/v1/backtests/{id}/series': { method: 'GET', minRole: 'viewer', timeoutMs: 20_000 },
  'api/v1/backtests/{id}/artifacts/{kind}': { method: 'GET', minRole: 'viewer', timeoutMs: 30_000 },
  'api/v1/datasets': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
  'api/v1/strategies/builtin': { method: 'GET', minRole: 'viewer', timeoutMs: 10_000 },
};

/** Map UUID job URLs to their allowlist policy while preserving the real path for signing. */
export function routePolicyKey(path: string): string {
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}\/cancel$/i.test(path)) return 'api/v1/jobs/{id}/cancel';
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/jobs/{id}';
  // Only the stored files the API serves; any other name stays unlisted (a 404 here).
  if (/^api\/v1\/backtests\/[0-9a-f-]{36}\/artifacts\/(summary|request|equity_csv|orders_csv|fills_csv|returns_csv|strategies_csv)$/i.test(path)) {
    return 'api/v1/backtests/{id}/artifacts/{kind}';
  }
  const backtest = /^api\/v1\/backtests\/[0-9a-f-]{36}(\/cancel|\/series)?$/i.exec(path);
  if (backtest) return `api/v1/backtests/{id}${backtest[1] ?? ''}`;
  return path;
}

export function allowsMethod(route: EngineRoute, method: EngineMethod): boolean {
  return Array.isArray(route.method) ? route.method.includes(method) : route.method === method;
}

export function minimumRole(route: EngineRoute, method: EngineMethod): WorkspaceRole {
  return route.methodRoles?.[method] ?? route.minRole;
}
