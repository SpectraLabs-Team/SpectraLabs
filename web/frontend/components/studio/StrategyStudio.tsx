'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  AlertCircle,
  CheckCircle2,
  ChevronRight,
  Code2,
  Copy,
  ExternalLink,
  FileCode2,
  GitCommit,
  GitCompare,
  History,
  Layers,
  Play,
  Plus,
  RefreshCw,
  Save,
  Sliders,
  Sparkles,
} from 'lucide-react';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import {
  createParameterSet,
  createStrategy,
  diffVersions,
  getDraft,
  listParameterSets,
  listStrategies,
  listVersions,
  saveDraft,
  saveVersion,
  validateStrategyCode,
  type InferredParameter,
  type ParameterSet,
  type StrategyDiff,
  type StrategySummary,
  type StrategyVersion,
  type ValidationErrorItem,
} from '@/lib/strategies';
import { listDatasets, type DatasetVersion } from '@/lib/backtests';

const DEFAULT_DSL_TEMPLATE = `// Restricted Safe Strategy DSL
param fast: int = 10, min = 2, max = 100;
param slow: int = 30, min = 5, max = 300;

on_bar {
    let fast_ma = sma(fast);
    let slow_ma = sma(slow);

    if fast_ma > slow_ma {
        target_shares(100);
    } else {
        target_shares(0);
    }
}
`;

export function StrategyStudio() {
  const [strategies, setStrategies] = useState<StrategySummary[]>([]);
  const [selectedStrategy, setSelectedStrategy] = useState<StrategySummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  // Editor state
  const [code, setCode] = useState(DEFAULT_DSL_TEMPLATE);
  const [parameters, setParameters] = useState<Record<string, unknown>>({});
  const [inferredParams, setInferredParams] = useState<InferredParameter[]>([]);
  const [validationErrors, setValidationErrors] = useState<ValidationErrorItem[]>([]);
  const [isValid, setIsValid] = useState(true);
  const [draftStatus, setDraftStatus] = useState<'saved' | 'saving' | 'dirty' | 'idle'>('idle');
  const [isReadOnly, setIsReadOnly] = useState(false);

  // Sub-tabs: 'code' | 'history' | 'diff' | 'presets'
  const [activeSubTab, setActiveSubTab] = useState<'code' | 'history' | 'diff' | 'presets'>('code');

  // Versions and diffs
  const [versions, setVersions] = useState<StrategyVersion[]>([]);
  const [selectedVersionForDiffFrom, setSelectedVersionForDiffFrom] = useState<number>(1);
  const [selectedVersionForDiffTo, setSelectedVersionForDiffTo] = useState<number>(2);
  const [diffResult, setDiffResult] = useState<StrategyDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);

  // Parameter sets
  const [paramSets, setParamSets] = useState<ParameterSet[]>([]);
  const [newPresetName, setNewPresetName] = useState('');

  // Modals / Dialogs
  const [showSaveModal, setShowSaveModal] = useState(false);
  const [showNewStrategyModal, setShowNewStrategyModal] = useState(false);
  const [newStrategyName, setNewStrategyName] = useState('');
  const [newStrategyDesc, setNewStrategyDesc] = useState('');
  const [isSavingVersion, setIsSavingVersion] = useState(false);
  const [versionSavedNotice, setVersionSavedNotice] = useState<string | null>(null);

  // Quick Run Backtest Modal
  const [showRunModal, setShowRunModal] = useState(false);
  const [datasets, setDatasets] = useState<DatasetVersion[]>([]);
  const [selectedDatasetId, setSelectedDatasetId] = useState('');
  const [selectedVersionForRun, setSelectedVersionForRun] = useState<string>('');
  const [isRunning, setIsRunning] = useState(false);
  const [runMessage, setRunMessage] = useState<string | null>(null);

  // Debounce ref for auto-saving drafts and validating
  const debounceTimerRef = useRef<NodeJS.Timeout | null>(null);

  // Load strategies list
  const loadStrategies = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const items = await listStrategies();
      if (items.length > 0) {
        setStrategies(items);
        if (!selectedStrategy || !items.some((it) => it.id === selectedStrategy.id)) {
          setSelectedStrategy(items[0]);
        }
      } else {
        // No strategies exist yet in this workspace: create default Dual SMA crossover strategy
        try {
          const defaultStrat = await createStrategy({
            name: 'Dual SMA Moving Average',
            kind: 'custom',
            description: 'Fast and slow simple moving average crossover strategy with parameter tuning.',
          });
          setStrategies([defaultStrat]);
          setSelectedStrategy(defaultStrat);
        } catch {
          setStrategies([]);
        }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load strategies.');
    } finally {
      setLoading(false);
    }
  }, [selectedStrategy]);

  useEffect(() => {
    loadStrategies();
  }, [loadStrategies]);

  // Load strategy details (draft, versions, param sets) when selectedStrategy changes
  useEffect(() => {
    if (!selectedStrategy) return;
    const stratId = selectedStrategy.id;

    // Load draft
    getDraft(stratId)
      .then((draft) => {
        if (draft && draft.code) {
          setCode(draft.code);
          setParameters(draft.parameters || {});
        } else {
          setCode(DEFAULT_DSL_TEMPLATE);
          setParameters({ fast: 10, slow: 30 });
        }
      })
      .catch(() => {
        setCode(DEFAULT_DSL_TEMPLATE);
        setParameters({ fast: 10, slow: 30 });
      });

    // Load versions
    listVersions(stratId)
      .then((vers) => {
        setVersions(vers);
        if (vers.length >= 2) {
          setSelectedVersionForDiffFrom(vers[vers.length - 2].version_num);
          setSelectedVersionForDiffTo(vers[vers.length - 1].version_num);
        } else if (vers.length === 1) {
          setSelectedVersionForDiffFrom(vers[0].version_num);
          setSelectedVersionForDiffTo(vers[0].version_num);
        }
        if (vers.length > 0) {
          setSelectedVersionForRun(vers[vers.length - 1].id);
          // Parameter sets belong to a saved version: load the latest one's presets.
          listParameterSets(stratId, vers[vers.length - 1].id)
            .then((ps) => setParamSets(ps))
            .catch(() => setParamSets([]));
        } else {
          setParamSets([]);
        }
      })
      .catch(() => setVersions([]));

    // Load datasets for quick backtest
    listDatasets()
      .then((ds) => {
        setDatasets(ds);
        if (ds.length > 0) setSelectedDatasetId(ds[0].id);
      })
      .catch(() => {});
  }, [selectedStrategy]);

  // Validation & Draft autosave effect
  useEffect(() => {
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
    setDraftStatus('dirty');

    debounceTimerRef.current = setTimeout(async () => {
      // 1. Validate code
      try {
        const valRes = await validateStrategyCode(code);
        setIsValid(valRes.valid);
        setValidationErrors(valRes.errors || []);
        setInferredParams(valRes.parameters || []);

        // Sync parameters default if not set
        if (valRes.parameters) {
          setParameters((prev) => {
            const next = { ...prev };
            for (const p of valRes.parameters) {
              if (next[p.name] === undefined) {
                next[p.name] = p.default;
              }
            }
            return next;
          });
        }
      } catch (err) {
        setIsValid(false);
        setValidationErrors([
          { line: 1, col: 1, message: err instanceof Error ? err.message : 'Validation request failed' },
        ]);
      }

      // 2. Autosave draft if a strategy is selected
      if (selectedStrategy) {
        setDraftStatus('saving');
        try {
          await saveDraft(selectedStrategy.id, code, parameters);
          setDraftStatus('saved');
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : String(err);
          if (msg.includes('viewer_read_only') || msg.includes('403')) {
            setIsReadOnly(true);
            setDraftStatus('idle');
          } else {
            setDraftStatus('dirty');
          }
        }
      }
    }, 600);

    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, selectedStrategy]);

  // Handle Save Version
  const handleSaveVersion = async () => {
    if (!selectedStrategy) return;
    setIsSavingVersion(true);
    setVersionSavedNotice(null);
    try {
      const v = await saveVersion(selectedStrategy.id, code, parameters);
      setShowSaveModal(false);
      setVersionSavedNotice(`Saved as Version ${v.version_num} (Hash: ${v.code_sha256.slice(0, 10)}...)`);

      // Refresh versions
      const vers = await listVersions(selectedStrategy.id);
      setVersions(vers);
      if (vers.length >= 2) {
        setSelectedVersionForDiffFrom(vers[vers.length - 2].version_num);
        setSelectedVersionForDiffTo(vers[vers.length - 1].version_num);
      }
      setSelectedVersionForRun(v.id);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to save version.';
      if (msg.includes('viewer_read_only') || msg.includes('403')) {
        setIsReadOnly(true);
      }
      alert(msg);
    } finally {
      setIsSavingVersion(false);
    }
  };

  // Handle Fetch Diff
  const handleFetchDiff = async () => {
    if (!selectedStrategy) return;
    setDiffLoading(true);
    try {
      const diff = await diffVersions(
        selectedStrategy.id,
        selectedVersionForDiffFrom,
        selectedVersionForDiffTo
      );
      setDiffResult(diff);
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Failed to fetch diff.');
    } finally {
      setDiffLoading(false);
    }
  };

  // Handle Create Preset
  const handleCreatePreset = async () => {
    if (!selectedStrategy || !newPresetName.trim()) return;
    const latest = versions[versions.length - 1];
    if (!latest) {
      alert('Save a version of this strategy first: presets are stored against a saved version.');
      return;
    }
    try {
      const created = await createParameterSet(selectedStrategy.id, latest.id, {
        name: newPresetName.trim(),
        parameters,
      });
      setParamSets((prev) => [created, ...prev]);
      setNewPresetName('');
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Failed to create parameter set.');
    }
  };

  // Handle Create New Strategy
  const handleCreateStrategy = async () => {
    if (!newStrategyName.trim()) return;
    try {
      const strat = await createStrategy({
        name: newStrategyName.trim(),
        description: newStrategyDesc.trim(),
      });
      setStrategies((prev) => [strat, ...prev]);
      setSelectedStrategy(strat);
      setShowNewStrategyModal(false);
      setNewStrategyName('');
      setNewStrategyDesc('');
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Failed to create strategy.');
    }
  };

  // Handle Quick Run Backtest
  const handleRunBacktest = async () => {
    if (!selectedDatasetId || !selectedVersionForRun) return;
    setIsRunning(true);
    setRunMessage(null);
    try {
      const res = await fetch('/api/v1/backtests', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          dataset_version_id: selectedDatasetId,
          strategy_version_id: selectedVersionForRun,
        }),
      });
      if (!res.ok) {
        const errJson = await res.json().catch(() => ({}));
        throw new Error(errJson.message || `Request failed with status ${res.status}`);
      }
      const data = await res.json();
      setRunMessage(`Backtest Job queued successfully! Job ID: ${data.id}`);
    } catch (e) {
      setRunMessage(`Failed to run: ${e instanceof Error ? e.message : 'Unknown error'}`);
    } finally {
      setIsRunning(false);
    }
  };

  if (loading && strategies.length === 0) {
    return (
      <div className="flex h-96 items-center justify-center">
        <Spinner className="h-8 w-8 text-accent" />
      </div>
    );
  }

  const linesCount = code.split('\n').length;
  const errorLineSet = new Set(validationErrors.map((e) => e.line));

  return (
    <div className="space-y-6">
      {/* Top Header: Strategy Switcher & Primary Actions */}
      <GlassCard className="p-4 sm:p-6">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-accent/10 text-accent">
              <FileCode2 className="h-6 w-6" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <select
                  value={selectedStrategy?.id || ''}
                  onChange={(e) => {
                    const found = strategies.find((s) => s.id === e.target.value);
                    if (found) setSelectedStrategy(found);
                  }}
                  className="rounded-lg border border-ink/15 bg-white/70 px-3 py-1.5 text-base font-semibold text-ink shadow-sm dark:bg-ink/5"
                >
                  {strategies.length === 0 ? (
                    <option value="" disabled>No strategies found</option>
                  ) : (
                    strategies.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} ({s.kind})
                      </option>
                    ))
                  )}
                </select>
                <button
                  type="button"
                  onClick={() => setShowNewStrategyModal(true)}
                  className="inline-flex items-center gap-1 rounded-lg border border-ink/15 bg-ink/5 px-2.5 py-1.5 text-xs font-medium text-ink transition hover:bg-ink/10"
                >
                  <Plus className="h-3.5 w-3.5" /> New Strategy
                </button>
              </div>
              <p className="mt-1 text-xs text-ink/60">
                {selectedStrategy?.description || 'Custom Strategy Studio & Restricted DSL Runtime'}
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2.5">
            {isReadOnly && (
              <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/30 bg-amber-500/10 px-2.5 py-1 text-xs font-semibold text-amber-600 dark:text-amber-400">
                Shared · Read-only (Viewer)
              </span>
            )}

            {/* Draft status indicator */}
            <div className="flex items-center gap-1.5 rounded-full border border-ink/10 bg-ink/[0.03] px-3 py-1 text-xs font-mono text-ink/70">
              {draftStatus === 'saving' && (
                <>
                  <RefreshCw className="h-3 w-3 animate-spin text-accent" />
                  <span>Autosaving draft...</span>
                </>
              )}
              {draftStatus === 'saved' && (
                <>
                  <CheckCircle2 className="h-3 w-3 text-emerald-500" />
                  <span>Draft saved</span>
                </>
              )}
              {draftStatus === 'dirty' && (
                <>
                  <span className="h-2 w-2 rounded-full bg-amber-400" />
                  <span>Unsaved changes</span>
                </>
              )}
              {draftStatus === 'idle' && <span>Ready</span>}
            </div>

            <button
              type="button"
              onClick={() => setShowSaveModal(true)}
              disabled={isReadOnly}
              title={isReadOnly ? 'Viewers cannot modify or save strategy versions' : 'Save Version'}
              className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-2 text-xs font-semibold text-onaccent shadow-sm transition hover:brightness-110 active:scale-95 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <Save className="h-4 w-4" /> Save Version
            </button>

            <button
              type="button"
              onClick={() => setShowRunModal(true)}
              disabled={versions.length === 0}
              className="inline-flex items-center gap-1.5 rounded-lg border border-accent/30 bg-accent/10 px-3.5 py-2 text-xs font-semibold text-accent transition hover:bg-accent/20 active:scale-95 disabled:opacity-50"
            >
              <Play className="h-4 w-4 fill-current" /> Run Backtest
            </button>
          </div>
        </div>

        {versionSavedNotice && (
          <div className="mt-4 flex items-center justify-between rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-4 py-2 text-xs font-medium text-emerald-600 dark:text-emerald-400">
            <span>{versionSavedNotice}</span>
            <button
              type="button"
              onClick={() => setVersionSavedNotice(null)}
              className="text-emerald-700 hover:underline dark:text-emerald-300"
            >
              Dismiss
            </button>
          </div>
        )}
      </GlassCard>

      {/* Sub-tabs: Code Editor | Versions & History | Diff Inspector | Parameter Presets */}
      <div className="flex border-b border-ink/10">
        <button
          type="button"
          onClick={() => setActiveSubTab('code')}
          className={`flex items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-semibold transition ${
            activeSubTab === 'code'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          }`}
        >
          <Code2 className="h-4 w-4" /> Code Editor & AST Validation
        </button>
        <button
          type="button"
          onClick={() => setActiveSubTab('history')}
          className={`flex items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-semibold transition ${
            activeSubTab === 'history'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          }`}
        >
          <History className="h-4 w-4" /> Version History ({versions.length})
        </button>
        <button
          type="button"
          onClick={() => {
            setActiveSubTab('diff');
            handleFetchDiff();
          }}
          className={`flex items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-semibold transition ${
            activeSubTab === 'diff'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          }`}
        >
          <GitCompare className="h-4 w-4" /> Version Diff
        </button>
        <button
          type="button"
          onClick={() => setActiveSubTab('presets')}
          className={`flex items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-semibold transition ${
            activeSubTab === 'presets'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          }`}
        >
          <Sliders className="h-4 w-4" /> Parameter Sets ({paramSets.length})
        </button>
      </div>

      {/* Sub-tab 1: Code Editor & AST Validation */}
      {activeSubTab === 'code' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
          {/* Editor Workspace (2 cols) */}
          <div className="space-y-4 lg:col-span-2">
            <div className="overflow-hidden rounded-xl border border-ink/15 bg-ink/[0.02] shadow-inner dark:bg-black/30">
              {/* Editor Bar */}
              <div className="flex items-center justify-between border-b border-ink/10 bg-ink/[0.04] px-4 py-2 text-xs font-mono text-ink/60">
                <span className="flex items-center gap-2 font-medium">
                  <span className="h-2 w-2 rounded-full bg-emerald-500" /> main.dsl (Restricted Safe AST)
                </span>
                <span>{linesCount} lines · utf-8</span>
              </div>

              {/* Code input with line numbers */}
              <div className="flex min-h-[420px] font-mono text-xs">
                {/* Line numbers gutter */}
                <div className="select-none border-r border-ink/10 bg-ink/[0.03] py-3 text-right text-ink/30">
                  {Array.from({ length: linesCount }).map((_, i) => {
                    const lineNo = i + 1;
                    const hasErr = errorLineSet.has(lineNo);
                    return (
                      <div
                        key={lineNo}
                        className={`px-3 leading-5 ${
                          hasErr ? 'bg-rose-500/20 font-bold text-rose-500' : ''
                        }`}
                      >
                        {hasErr ? '!' : lineNo}
                      </div>
                    );
                  })}
                </div>

                {/* Editor Textarea */}
                <textarea
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  placeholder="// Type restricted strategy code here..."
                  spellCheck={false}
                  className="flex-1 resize-y bg-transparent p-3 font-mono leading-5 text-ink outline-none selection:bg-accent/20 dark:text-zinc-200"
                  rows={20}
                />
              </div>
            </div>

            {/* Diagnostics Panel */}
            <div className="rounded-xl border border-ink/10 bg-ink/[0.02] p-4 text-xs">
              <div className="mb-2 flex items-center justify-between font-semibold">
                <span className="flex items-center gap-1.5">
                  {isValid ? (
                    <>
                      <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                      <span className="text-emerald-600 dark:text-emerald-400">
                        Syntax & Safety Validation Passed
                      </span>
                    </>
                  ) : (
                    <>
                      <AlertCircle className="h-4 w-4 text-rose-500" />
                      <span className="text-rose-600 dark:text-rose-400">
                        Validation Diagnostic Errors ({validationErrors.length})
                      </span>
                    </>
                  )}
                </span>
                <span className="font-mono text-[11px] text-ink/50">
                  Guarantees: No lookahead · No unbounded loops · Pure AST
                </span>
              </div>

              {!isValid && validationErrors.length > 0 && (
                <div className="mt-2 space-y-1.5 font-mono">
                  {validationErrors.map((err, idx) => (
                    <div
                      key={idx}
                      className="flex items-start gap-2 rounded bg-rose-500/10 p-2 text-rose-700 dark:text-rose-300"
                    >
                      <span className="shrink-0 rounded bg-rose-500/20 px-1 py-0.5 text-[10px] font-bold">
                        Line {err.line}:{err.col}
                      </span>
                      <span>{err.message}</span>
                    </div>
                  ))}
                </div>
              )}

              {isValid && (
                <p className="text-ink/60">
                  Code conforms to the Spectra safe DSL runtime. Safe to execute in backtest and optimization engines.
                </p>
              )}
            </div>
          </div>

          {/* Right Rail: Inferred Parameters & Tuning */}
          <div className="space-y-4">
            <GlassCard className="p-4">
              <div className="flex items-center justify-between border-b border-ink/10 pb-3">
                <h3 className="text-xs font-bold uppercase tracking-wider text-ink/70">
                  Inferred Parameters ({inferredParams.length})
                </h3>
                <span className="rounded bg-accent/10 px-2 py-0.5 text-[10px] font-semibold text-accent">
                  Live Bindings
                </span>
              </div>

              {inferredParams.length === 0 ? (
                <div className="py-8 text-center text-xs text-ink/50">
                  No `param` declarations found. Add e.g.:
                  <code className="mt-2 block rounded bg-ink/5 p-2 font-mono text-[11px]">
                    param fast: int = 10;
                  </code>
                </div>
              ) : (
                <div className="mt-3 space-y-4">
                  {inferredParams.map((p) => {
                    const currentVal = parameters[p.name] ?? p.default;
                    return (
                      <div key={p.name} className="space-y-1.5">
                        <div className="flex items-center justify-between text-xs">
                          <span className="font-mono font-medium text-ink">{p.name}</span>
                          <span className="font-mono text-ink/60">
                            {String(currentVal)}{p.type ? ` (${p.type})` : ''}
                          </span>
                        </div>

                        {p.type === 'bool' ? (
                          <button
                            type="button"
                            onClick={() =>
                              setParameters((prev) => ({ ...prev, [p.name]: !prev[p.name] }))
                            }
                            className={`w-full rounded border px-3 py-1.5 text-xs font-semibold ${
                              currentVal
                                ? 'border-accent bg-accent/10 text-accent'
                                : 'border-ink/15 bg-ink/5 text-ink/70'
                            }`}
                          >
                            {currentVal ? 'true' : 'false'}
                          </button>
                        ) : (
                          <div className="space-y-1">
                            <input
                              type="number"
                              min={p.min}
                              max={p.max}
                              value={Number(currentVal)}
                              onChange={(e) => {
                                const n = Number(e.target.value);
                                setParameters((prev) => ({ ...prev, [p.name]: n }));
                              }}
                              className="w-full rounded-md border border-ink/15 bg-white/70 px-2.5 py-1 text-xs font-mono text-ink dark:bg-ink/5"
                            />
                            {p.min !== undefined && p.max !== undefined && (
                              <input
                                type="range"
                                min={p.min}
                                max={p.max}
                                value={Number(currentVal)}
                                onChange={(e) => {
                                  const n = Number(e.target.value);
                                  setParameters((prev) => ({ ...prev, [p.name]: n }));
                                }}
                                className="w-full accent-accent"
                              />
                            )}
                          </div>
                        )}
                        <p className="text-[11px] text-ink/40">
                          Default: {String(p.default)}
                          {p.min !== undefined && ` · Range: [${p.min}, ${p.max}]`}
                        </p>
                      </div>
                    );
                  })}
                </div>
              )}
            </GlassCard>
          </div>
        </div>
      )}

      {/* Sub-tab 2: Version History */}
      {activeSubTab === 'history' && (
        <div className="space-y-4">
          <div className="overflow-hidden rounded-xl border border-ink/10 bg-white/60 shadow-sm dark:bg-ink/[0.02]">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-ink/10 bg-ink/[0.03] text-[11px] font-semibold uppercase tracking-wider text-ink/60">
                <tr>
                  <th className="px-4 py-3">Version</th>
                  <th className="px-4 py-3">Code Fingerprint</th>
                  <th className="px-4 py-3">Parameters</th>
                  <th className="px-4 py-3">Created</th>
                  <th className="px-4 py-3 text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-ink/10 font-mono">
                {versions.length === 0 ? (
                  <tr>
                    <td colSpan={6} className="px-4 py-8 text-center text-ink/40">
                      No versions saved yet. Click &quot;Save Version&quot; to create the first immutable release.
                    </td>
                  </tr>
                ) : (
                  versions.map((v) => (
                    <tr key={v.id} className="hover:bg-ink/[0.02]">
                      <td className="px-4 py-3 font-bold text-accent">v{v.version_num}</td>
                      <td className="px-4 py-3 text-ink/70" title={v.code_sha256}>
                        {v.code_sha256.slice(0, 12)}...
                      </td>
                      <td className="px-4 py-3 text-ink/60">
                        {JSON.stringify(v.parameters || {})}
                      </td>
                      <td className="px-4 py-3 font-sans text-ink/50">
                        {new Date(v.created_at).toLocaleString()}
                      </td>
                      <td className="px-4 py-3 text-right font-sans">
                        <button
                          type="button"
                          onClick={() => {
                            if (v.code) {
                              setCode(v.code);
                              setParameters(v.parameters || {});
                              setActiveSubTab('code');
                            }
                          }}
                          className="rounded border border-ink/15 px-2 py-1 text-[11px] font-semibold text-ink hover:bg-ink/5"
                        >
                          Load in Editor
                        </button>
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Sub-tab 3: Version Diff */}
      {activeSubTab === 'diff' && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center gap-4 rounded-xl border border-ink/10 bg-ink/[0.02] p-4 text-xs">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-ink">Compare:</span>
              <select
                value={selectedVersionForDiffFrom}
                onChange={(e) => setSelectedVersionForDiffFrom(Number(e.target.value))}
                className="rounded border border-ink/15 bg-white/70 px-2.5 py-1 text-ink dark:bg-ink/5"
              >
                {versions.map((v) => (
                  <option key={v.id} value={v.version_num}>
                    v{v.version_num} ({v.code_sha256.slice(0, 8)})
                  </option>
                ))}
              </select>
            </div>
            <span className="text-ink/40">→</span>
            <div className="flex items-center gap-2">
              <span className="font-semibold text-ink">With:</span>
              <select
                value={selectedVersionForDiffTo}
                onChange={(e) => setSelectedVersionForDiffTo(Number(e.target.value))}
                className="rounded border border-ink/15 bg-white/70 px-2.5 py-1 text-ink dark:bg-ink/5"
              >
                {versions.map((v) => (
                  <option key={v.id} value={v.version_num}>
                    v{v.version_num} ({v.code_sha256.slice(0, 8)})
                  </option>
                ))}
              </select>
            </div>
            <button
              type="button"
              onClick={handleFetchDiff}
              className="inline-flex items-center gap-1 rounded-lg bg-accent px-3 py-1.5 font-semibold text-onaccent hover:brightness-110"
            >
              <RefreshCw className={`h-3 w-3 ${diffLoading ? 'animate-spin' : ''}`} /> Refresh Diff
            </button>
          </div>

          {diffResult && (
            <div className="space-y-4">
              {/* Parameter Differences */}
              {diffResult.parameters_changed.length > 0 && (
                <div className="rounded-xl border border-amber-500/20 bg-amber-500/5 p-4 text-xs">
                  <h4 className="font-bold text-amber-700 dark:text-amber-400">Parameter Changes</h4>
                  <div className="mt-2 divide-y divide-amber-500/10 font-mono">
                    {diffResult.parameters_changed.map((pc, idx) => (
                      <div key={idx} className="flex items-center justify-between py-1">
                        <span className="font-semibold">{pc.name}</span>
                        <span>
                          <span className="text-rose-600 line-through">
                            {JSON.stringify(pc.old_value)}
                          </span>{' '}
                          →{' '}
                          <span className="text-emerald-600 font-bold">
                            {JSON.stringify(pc.new_value)}
                          </span>
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Code Unified Diff */}
              <div className="overflow-hidden rounded-xl border border-ink/15 bg-ink/[0.02] font-mono text-xs shadow-inner">
                <div className="border-b border-ink/10 bg-ink/[0.04] px-4 py-2 text-[11px] text-ink/60">
                  Unified Code Diff (v{diffResult.from_version} → v{diffResult.to_version})
                </div>
                <div className="max-h-[500px] overflow-y-auto">
                  {diffResult.lines.map((l, idx) => {
                    const isAdded = l.change_type === 'added';
                    const isDeleted = l.change_type === 'deleted';
                    return (
                      <div
                        key={idx}
                        className={`flex px-3 py-0.5 leading-5 ${
                          isAdded
                            ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300'
                            : isDeleted
                            ? 'bg-rose-500/15 text-rose-700 dark:text-rose-300 line-through'
                            : 'text-ink/80'
                        }`}
                      >
                        <span className="w-8 shrink-0 select-none text-ink/30">
                          {l.line_num_from ?? ''}
                        </span>
                        <span className="w-8 shrink-0 select-none text-ink/30">
                          {l.line_num_to ?? ''}
                        </span>
                        <span className="mr-2 select-none font-bold">
                          {isAdded ? '+' : isDeleted ? '-' : ' '}
                        </span>
                        <span className="whitespace-pre">{l.text}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Sub-tab 4: Parameter Presets */}
      {activeSubTab === 'presets' && (
        <div className="space-y-6">
          <GlassCard className="p-4 sm:p-6">
            <h3 className="text-sm font-bold text-ink">Save Parameter Set</h3>
            <p className="mt-1 text-xs text-ink/60">
              Create a named parameter configuration from the current active values.
            </p>
            <div className="mt-4 flex gap-2">
              <input
                type="text"
                placeholder="e.g. Aggressive Trend, Low Volatility"
                value={newPresetName}
                onChange={(e) => setNewPresetName(e.target.value)}
                className="flex-1 rounded-lg border border-ink/15 bg-white/70 px-3 py-2 text-xs text-ink dark:bg-ink/5"
              />
              <button
                type="button"
                onClick={handleCreatePreset}
                disabled={!newPresetName.trim()}
                className="rounded-lg bg-accent px-4 py-2 text-xs font-semibold text-onaccent hover:brightness-110 disabled:opacity-50"
              >
                Save Preset
              </button>
            </div>
          </GlassCard>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {paramSets.map((ps) => (
              <GlassCard key={ps.id} className="p-4">
                <div className="flex items-center justify-between">
                  <h4 className="font-bold text-sm text-ink">{ps.name}</h4>
                  <button
                    type="button"
                    onClick={() => {
                      setParameters(ps.parameters);
                      setActiveSubTab('code');
                    }}
                    className="rounded border border-accent/30 bg-accent/10 px-2 py-1 text-xs font-semibold text-accent hover:bg-accent/20"
                  >
                    Apply
                  </button>
                </div>
                <div className="mt-3 rounded bg-ink/5 p-2 font-mono text-xs text-ink/70">
                  {Object.entries(ps.parameters).map(([k, v]) => (
                    <div key={k} className="flex justify-between">
                      <span>{k}:</span>
                      <span className="font-bold">{String(v)}</span>
                    </div>
                  ))}
                </div>
                <p className="mt-2 text-[10px] text-ink/40">
                  Created {new Date(ps.created_at).toLocaleDateString()}
                </p>
              </GlassCard>
            ))}
          </div>
        </div>
      )}

      {/* Save Version Modal */}
      {showSaveModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="w-full max-w-md rounded-2xl border border-ink/15 bg-white p-6 shadow-2xl dark:bg-zinc-900">
            <h3 className="text-base font-bold text-ink">Save Strategy Version</h3>
            <p className="mt-1 text-xs text-ink/60">
              Creates an immutable release with fingerprint deduplication. If no changes occurred
              since the last version, the existing version will be returned.
            </p>


            <div className="mt-6 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowSaveModal(false)}
                className="rounded-lg border border-ink/15 px-3 py-1.5 text-xs font-medium text-ink/70 hover:bg-ink/5"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleSaveVersion}
                disabled={isSavingVersion}
                className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-xs font-semibold text-onaccent hover:brightness-110 disabled:opacity-50"
              >
                {isSavingVersion ? <Spinner className="h-3.5 w-3.5" /> : <Save className="h-3.5 w-3.5" />}
                Confirm & Save
              </button>
            </div>
          </div>
        </div>
      )}

      {/* New Strategy Modal */}
      {showNewStrategyModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="w-full max-w-md rounded-2xl border border-ink/15 bg-white p-6 shadow-2xl dark:bg-zinc-900">
            <h3 className="text-base font-bold text-ink">Create New Strategy</h3>
            <div className="mt-4 space-y-3">
              <div>
                <label className="text-xs font-medium text-ink">Strategy Name</label>
                <input
                  type="text"
                  value={newStrategyName}
                  onChange={(e) => setNewStrategyName(e.target.value)}
                  placeholder="e.g. Mean Reversion Bollinger"
                  className="mt-1 w-full rounded-lg border border-ink/15 bg-white/70 p-2 text-xs text-ink dark:bg-ink/5"
                />
              </div>
              <div>
                <label className="text-xs font-medium text-ink">Description</label>
                <textarea
                  value={newStrategyDesc}
                  onChange={(e) => setNewStrategyDesc(e.target.value)}
                  placeholder="Strategy concept and objective"
                  rows={2}
                  className="mt-1 w-full rounded-lg border border-ink/15 bg-white/70 p-2 text-xs text-ink dark:bg-ink/5"
                />
              </div>
            </div>

            <div className="mt-6 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowNewStrategyModal(false)}
                className="rounded-lg border border-ink/15 px-3 py-1.5 text-xs font-medium text-ink/70 hover:bg-ink/5"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleCreateStrategy}
                disabled={!newStrategyName.trim()}
                className="rounded-lg bg-accent px-4 py-1.5 text-xs font-semibold text-onaccent hover:brightness-110 disabled:opacity-50"
              >
                Create Strategy
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Quick Run Backtest Modal */}
      {showRunModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="w-full max-w-md rounded-2xl border border-ink/15 bg-white p-6 shadow-2xl dark:bg-zinc-900">
            <h3 className="text-base font-bold text-ink">Queue Backtest from Version</h3>
            <p className="mt-1 text-xs text-ink/60">
              Run this strategy version on historical data using the deterministic worker engine.
            </p>

            <div className="mt-4 space-y-3">
              <div>
                <label className="text-xs font-medium text-ink">Dataset Version</label>
                <select
                  value={selectedDatasetId}
                  onChange={(e) => setSelectedDatasetId(e.target.value)}
                  className="mt-1 w-full rounded-lg border border-ink/15 bg-white/70 p-2 text-xs text-ink dark:bg-ink/5"
                >
                  {datasets.map((ds) => (
                    <option key={ds.id} value={ds.id}>
                      {ds.dataset_name} (v{ds.version} · {ds.row_count} bars)
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="text-xs font-medium text-ink">Strategy Release</label>
                <select
                  value={selectedVersionForRun}
                  onChange={(e) => setSelectedVersionForRun(e.target.value)}
                  className="mt-1 w-full rounded-lg border border-ink/15 bg-white/70 p-2 text-xs text-ink dark:bg-ink/5"
                >
                  {versions.map((v) => (
                    <option key={v.id} value={v.id}>
                      v{v.version_num} ({v.code_sha256.slice(0, 8)})
                    </option>
                  ))}
                </select>
              </div>

              {runMessage && (
                <div className="rounded-lg bg-accent/10 p-2 text-xs font-medium text-accent">
                  {runMessage}
                </div>
              )}
            </div>

            <div className="mt-6 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => {
                  setShowRunModal(false);
                  setRunMessage(null);
                }}
                className="rounded-lg border border-ink/15 px-3 py-1.5 text-xs font-medium text-ink/70 hover:bg-ink/5"
              >
                Close
              </button>
              <button
                type="button"
                onClick={handleRunBacktest}
                disabled={isRunning || !selectedDatasetId || !selectedVersionForRun}
                className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-xs font-semibold text-onaccent hover:brightness-110 disabled:opacity-50"
              >
                {isRunning ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5 fill-current" />}
                Queue Run
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
