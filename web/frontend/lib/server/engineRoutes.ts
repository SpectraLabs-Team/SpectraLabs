import type { WorkspaceRole } from './workspaces';

/**
 * Allowlist of engine routes the /api/engine proxy forwards: method, timeout, and the minimum
 * workspace role (in the caller's active workspace) needed to call it. Anything not listed is a
 * 404, so new engine routes stay private until added here.
 *
 *   viewer → reads (sample data, telemetry, the research registry)
 *   editor → computations ("create and run"), registry writes and broker account data
 *   owner  → whole-registry operations (verify the chain, import a registry.jsonl)
 */
export type EngineMethod = 'GET' | 'POST';
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

/** Map UUID job URLs to their allowlist policy while preserving the real path for signing. */
export function routePolicyKey(path: string): string {
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}\/cancel$/i.test(path)) return 'api/v1/jobs/{id}/cancel';
  if (/^api\/v1\/jobs\/[0-9a-f-]{36}$/i.test(path)) return 'api/v1/jobs/{id}';
  for (const [pattern, key] of REGISTRY_PATTERNS) if (pattern.test(path)) return key;
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
