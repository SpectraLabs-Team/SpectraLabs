import { request } from './api';
import { toStrategyVersion, toStrategyDiff, toParameterSet, toStrategySummary, type EngineStrategy, type EngineVersion, type EngineDiff, type EngineParameterSet } from './strategyMappers';

export { toStrategyVersion, toStrategyDiff, toParameterSet, toStrategySummary };

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

export const validateStrategyCode = (code: string) =>
  request<StrategyValidationResult>('/api/v1/strategies/validate', {
    method: 'POST',
    body: JSON.stringify({ code }),
  });

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

/**
 * Saving answers `{ created, version }`; `created` is false when the same code and parameters were already saved.
 * The engine does not store version notes, so none are sent.
 */
export const saveVersion = (strategyId: string, code: string, parameters: Record<string, unknown> = {}) =>
  request<{ created: boolean; version: EngineVersion }>(`/api/v1/strategies/${strategyId}/versions`, {
    method: 'POST',
    body: JSON.stringify({ code, parameters }),
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
