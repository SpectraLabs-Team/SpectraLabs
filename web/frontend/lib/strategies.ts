import { request } from './api';

export interface ValidationErrorItem {
  line: number;
  col: number;
  message: string;
}

export interface InferredParameter {
  name: string;
  type: string;
  default: number | boolean;
  min?: number;
  max?: number;
  description: string;
}

export interface StrategyValidationResult {
  valid: boolean;
  errors: ValidationErrorItem[];
  parameters: InferredParameter[];
}

export interface StrategySummary {
  id: string;
  workspace_id: string;
  name: string;
  kind: string;
  description: string;
  created_at: string;
  updated_at: string;
}

export interface StrategyVersion {
  id: string;
  workspace_id: string;
  strategy_id: string;
  version_num: number;
  code_sha256: string;
  code?: string;
  parameters: Record<string, unknown>;
  notes: string;
  created_at: string;
}

export interface StrategyDraft {
  strategy_id: string;
  code: string;
  parameters: Record<string, unknown>;
  updated_at: string;
}

export interface DiffLine {
  line_num_from?: number;
  line_num_to?: number;
  change_type: 'equal' | 'added' | 'deleted';
  text: string;
}

export interface ParameterChange {
  name: string;
  old_value: unknown;
  new_value: unknown;
}

export interface StrategyDiff {
  strategy_id: string;
  from_version: number;
  to_version: number;
  lines: DiffLine[];
  parameters_changed: ParameterChange[];
}

export interface ParameterSet {
  id: string;
  workspace_id: string;
  strategy_id: string;
  strategy_version_id?: string;
  name: string;
  parameters: Record<string, unknown>;
  created_at: string;
}

// The engine's wire shapes (crates/spectra-api/src/strategies_api.rs) differ from the names the
// Studio uses. These types describe what the engine sends; the mappers below convert them, so the
// components only ever see the UI shapes above.

interface EngineVersion {
  id: string;
  strategy_id: string;
  version: number;
  strategy_sha256: string;
  config_hash?: string;
  params?: Record<string, unknown>;
  code?: string;
  notes?: string;
  created_at: string;
  workspace_id?: string;
}

interface EngineDiffLine {
  kind: 'added' | 'removed' | 'unchanged';
  line_from: number | null;
  line_to: number | null;
  text: string;
}

interface EngineDiff {
  strategy_id: string;
  from_version: number;
  to_version: number;
  code_diff: EngineDiffLine[];
  parameter_diff: {
    changed: Record<string, { from: unknown; to: unknown }>;
    added: Record<string, unknown>;
    removed: Record<string, unknown>;
  };
}

interface EngineParameterSet {
  id: string;
  workspace_id: string;
  strategy_version_id: string;
  name: string;
  parameters: Record<string, unknown>;
  created_at: string;
}

export function toStrategyVersion(v: EngineVersion, strategyId?: string): StrategyVersion {
  return {
    id: v.id,
    workspace_id: v.workspace_id ?? '',
    strategy_id: v.strategy_id ?? strategyId ?? '',
    version_num: v.version,
    code_sha256: v.strategy_sha256 ?? '',
    code: v.code,
    parameters: v.params ?? {},
    notes: v.notes ?? '',
    created_at: v.created_at,
  };
}

export function toStrategyDiff(d: EngineDiff): StrategyDiff {
  const lines: DiffLine[] = (d.code_diff ?? []).map((l) => ({
    change_type: l.kind === 'removed' ? 'deleted' : l.kind === 'added' ? 'added' : 'equal',
    line_num_from: l.line_from ?? undefined,
    line_num_to: l.line_to ?? undefined,
    text: l.text,
  }));
  const pd = d.parameter_diff ?? { changed: {}, added: {}, removed: {} };
  const parameters_changed: ParameterChange[] = [
    ...Object.entries(pd.changed ?? {}).map(([name, c]) => ({ name, old_value: c.from, new_value: c.to })),
    ...Object.entries(pd.added ?? {}).map(([name, v]) => ({ name, old_value: undefined, new_value: v })),
    ...Object.entries(pd.removed ?? {}).map(([name, v]) => ({ name, old_value: v, new_value: undefined })),
  ];
  return { strategy_id: d.strategy_id, from_version: d.from_version, to_version: d.to_version, lines, parameters_changed };
}

export function toParameterSet(p: EngineParameterSet, strategyId: string): ParameterSet {
  return {
    id: p.id,
    workspace_id: p.workspace_id,
    strategy_id: strategyId,
    strategy_version_id: p.strategy_version_id,
    name: p.name,
    parameters: p.parameters ?? {},
    created_at: p.created_at,
  };
}

export const validateStrategyCode = (code: string) =>
  request<StrategyValidationResult>('/api/v1/strategies/validate', {
    method: 'POST',
    body: JSON.stringify({ code }),
  });

interface EngineStrategy {
  id: string;
  name: string;
  description: string;
  latest_builtin?: string | null;
  created_at: string;
}

/** The engine's strategy list uses its own field names; map them to the Studio's shape. */
export function toStrategySummary(s: EngineStrategy, workspaceId = ''): StrategySummary {
  return {
    id: s.id,
    workspace_id: workspaceId,
    name: s.name,
    kind: s.latest_builtin ? 'builtin' : 'custom',
    description: s.description ?? '',
    created_at: s.created_at,
    updated_at: s.created_at,
  };
}

export const listStrategies = () =>
  request<{ items: EngineStrategy[] }>('/api/v1/strategies').then((r) => r.items.map((s) => toStrategySummary(s)));

export const createStrategy = (data: { name: string; kind?: string; description?: string }) =>
  request<StrategySummary>('/api/v1/strategies', {
    method: 'POST',
    body: JSON.stringify({
      name: data.name,
      kind: data.kind ?? 'custom',
      description: data.description ?? '',
    }),
  });

export const getStrategy = (id: string) =>
  request<StrategySummary>(`/api/v1/strategies/${id}`);

export const getDraft = (strategyId: string) =>
  request<StrategyDraft>(`/api/v1/strategies/${strategyId}/draft`);

export const saveDraft = (strategyId: string, code: string, parameters: Record<string, unknown> = {}) =>
  request<{ saved: boolean }>(`/api/v1/strategies/${strategyId}/draft`, {
    method: 'PUT',
    body: JSON.stringify({ code, parameters }),
  });

/** Versions come from the strategy detail endpoint (`GET /api/v1/strategies/{id}` returns `versions`, newest first). */
export const listVersions = (strategyId: string) =>
  request<{ strategy: EngineStrategy; versions: EngineVersion[] }>(`/api/v1/strategies/${strategyId}`).then((r) =>
    r.versions.map((v) => toStrategyVersion(v, strategyId)),
  );

/** Saving answers `{ created, version }`; `created` is false when the same code and parameters were already saved. */
export const saveVersion = (
  strategyId: string,
  code: string,
  parameters: Record<string, unknown> = {},
  notes: string = ''
) =>
  request<{ created: boolean; version: EngineVersion }>(`/api/v1/strategies/${strategyId}/versions`, {
    method: 'POST',
    body: JSON.stringify({ code, parameters, notes }),
  }).then((r) => toStrategyVersion(r.version, strategyId));

export const diffVersions = (strategyId: string, fromVersion: number, toVersion: number) =>
  request<EngineDiff>(`/api/v1/strategies/${strategyId}/diff?from_version=${fromVersion}&to_version=${toVersion}`).then(toStrategyDiff);

/** Parameter sets belong to one saved version (the engine route is nested under the version). */
export const listParameterSets = (strategyId: string, versionId: string) =>
  request<{ parameter_sets: EngineParameterSet[] }>(`/api/v1/strategies/${strategyId}/versions/${versionId}/parameter_sets`).then((r) =>
    r.parameter_sets.map((p) => toParameterSet(p, strategyId)),
  );

/** The engine rejects unknown fields, so only `name`, `description` and `parameters` are sent. */
export const createParameterSet = (
  strategyId: string,
  versionId: string,
  data: { name: string; parameters: Record<string, unknown>; description?: string }
) =>
  request<EngineParameterSet>(`/api/v1/strategies/${strategyId}/versions/${versionId}/parameter_sets`, {
    method: 'POST',
    body: JSON.stringify({ name: data.name, parameters: data.parameters, description: data.description ?? '' }),
  }).then((p) => toParameterSet(p, strategyId));
