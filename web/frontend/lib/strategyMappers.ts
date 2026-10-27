/**
 * Pure mappers from the Strategy engine's wire shapes (crates/spectra-api/src/strategies_api.rs) to
 * the shapes the Strategy Studio uses. No network or browser APIs here, so they are unit-tested
 * directly (scripts/test-strategies.mjs). lib/strategies.ts re-exports them.
 */
import type { StrategyVersion, StrategyDiff, DiffLine, ParameterChange, ParameterSet, StrategySummary } from './strategies';

// The engine's wire shapes (crates/spectra-api/src/strategies_api.rs) differ from the names the
// Studio uses. These types describe what the engine sends; the mappers below convert them, so the
// components only ever see the UI shapes above.

export interface EngineVersion {
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

export interface EngineDiffLine {
  kind: 'added' | 'removed' | 'unchanged';
  line_from: number | null;
  line_to: number | null;
  text: string;
}

export interface EngineDiff {
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

export interface EngineParameterSet {
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

export interface EngineStrategy {
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

