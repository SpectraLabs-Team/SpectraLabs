'use client';

import { useState, useEffect, useCallback, useId } from 'react';
import {
  Sliders,
  Play,
  RotateCcw,
  ShieldCheck,
  Lock,
  Flame,
  CheckCircle2,
  AlertTriangle,
  Info,
  Clock,
  Layers,
  Sparkles,
  TrendingUp,
  Percent,
  Download,
  FileText,
} from 'lucide-react';
import {
  listSweeps,
  getSweep,
  queueSweep,
  listWalkForwards,
  getWalkForward,
  queueWalkForward,
  computeOverfitting,
  listHoldouts,
  getHoldout,
  sealHoldout,
  evaluateHoldout,
  type OptimizationSweep,
  type SweepDetail,
  type WalkForwardReport,
  type OverfittingMetrics,
  type HoldoutVault,
  type HoldoutDetail,
} from '@/lib/optimization';
import { listDatasets, type DatasetVersion } from '@/lib/backtests';
import { listStrategies, type StrategySummary } from '@/lib/strategies';
import { cn } from '@/lib/utils';

export function OptimizationLab() {
  const [activeTab, setActiveTab] = useState<'sweeps' | 'walkforward' | 'overfitting' | 'holdout'>('sweeps');
  const [datasets, setDatasets] = useState<DatasetVersion[]>([]);
  const [strategies, setStrategies] = useState<StrategySummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Sweeps State
  const [sweeps, setSweeps] = useState<OptimizationSweep[]>([]);
  const [selectedSweepId, setSelectedSweepId] = useState<string | null>(null);
  const [selectedSweepDetail, setSelectedSweepDetail] = useState<SweepDetail | null>(null);
  const [selectedCell, setSelectedCell] = useState<{ x: number; y: number } | null>(null);
  const [sweepDatasetId, setSweepDatasetId] = useState<string>('');
  const [sweepStrategy, setSweepStrategy] = useState<string>('sma_cross');
  const [sweepParamGridText, setSweepParamGridText] = useState<string>(
    JSON.stringify({ fast: [5, 10, 15, 20], slow: [25, 30, 40, 50] }, null, 2)
  );
  const [sweepMethod, setSweepMethod] = useState<'grid' | 'random'>('grid');
  const [sweepMaxTrials, setSweepMaxTrials] = useState<number>(16);

  // Walk-Forward State
  const [reports, setReports] = useState<WalkForwardReport[]>([]);
  const [selectedReportId, setSelectedReportId] = useState<string | null>(null);
  const [selectedReport, setSelectedReport] = useState<WalkForwardReport | null>(null);
  const [wfTrainSize, setWfTrainSize] = useState<number>(200);
  const [wfTestSize, setWfTestSize] = useState<number>(50);
  const [wfMode, setWfMode] = useState<'rolling' | 'anchored'>('rolling');

  // Overfitting Diagnostic State
  const [overfittingResult, setOverfittingResult] = useState<OverfittingMetrics | null>(null);
  const [simTrials, setSimTrials] = useState<number>(25);
  const [simObs, setSimObs] = useState<number>(120);

  // Holdout Vault State
  const [holdouts, setHoldouts] = useState<HoldoutVault[]>([]);
  const [selectedHoldoutId, setSelectedHoldoutId] = useState<string | null>(null);
  const [selectedHoldoutDetail, setSelectedHoldoutDetail] = useState<HoldoutDetail | null>(null);
  const [sealName, setSealName] = useState<string>('');
  const [sealBudget, setSealBudget] = useState<number>(5);
  const [sealNotes, setSealNotes] = useState<string>('');

  const sweepDatasetSelectId = useId();
  const sweepStrategySelectId = useId();
  const sweepSearchMethodSelectId = useId();
  const sweepMaxTrialsInputId = useId();
  const sweepParamGridTextareaId = useId();
  const wfDatasetSelectId = useId();
  const wfTrainSizeInputId = useId();
  const wfTestSizeInputId = useId();
  const wfModeSelectId = useId();
  const simTrialsInputId = useId();
  const simObsInputId = useId();
  const sealNameInputId = useId();
  const sealDatasetSelectId = useId();
  const sealBudgetInputId = useId();
  const sealNotesTextareaId = useId();

  const refreshSweeps = useCallback(() => {
    listSweeps()
      .then((items) => {
        setSweeps(items);
        if (items.length > 0 && !selectedSweepId) {
          setSelectedSweepId(items[0].id);
        }
      })
      .catch((e: Error) => setError(e.message));
  }, [selectedSweepId]);

  const refreshWalkForwards = useCallback(() => {
    listWalkForwards()
      .then((items) => {
        setReports(items);
        if (items.length > 0 && !selectedReportId) {
          setSelectedReportId(items[0].id);
        }
      })
      .catch(() => {});
  }, [selectedReportId]);

  const refreshHoldouts = useCallback(() => {
    listHoldouts()
      .then((items) => {
        setHoldouts(items);
        if (items.length > 0 && !selectedHoldoutId) {
          setSelectedHoldoutId(items[0].id);
        }
      })
      .catch(() => {});
  }, [selectedHoldoutId]);

  // Initial load
  useEffect(() => {
    listDatasets().then((items) => {
      setDatasets(items);
      if (items.length > 0) {
        setSweepDatasetId(items[0].id);
      }
    }).catch(() => {});

    listStrategies().then((items) => {
      setStrategies(items);
    }).catch(() => {});

    refreshSweeps();
    refreshWalkForwards();
    refreshHoldouts();
  }, [refreshSweeps, refreshWalkForwards, refreshHoldouts]);

  useEffect(() => {
    if (selectedSweepId) {
      getSweep(selectedSweepId)
        .then(setSelectedSweepDetail)
        .catch(() => setSelectedSweepDetail(null));
    }
  }, [selectedSweepId]);

  useEffect(() => {
    if (selectedReportId) {
      getWalkForward(selectedReportId)
        .then(setSelectedReport)
        .catch(() => setSelectedReport(null));
    }
  }, [selectedReportId]);

  useEffect(() => {
    if (selectedHoldoutId) {
      getHoldout(selectedHoldoutId)
        .then(setSelectedHoldoutDetail)
        .catch(() => setSelectedHoldoutDetail(null));
    }
  }, [selectedHoldoutId]);

  // Queue parameter sweep
  const handleQueueSweep = async () => {
    setLoading(true);
    setError(null);
    try {
      const grid = JSON.parse(sweepParamGridText);
      await queueSweep({
        dataset_version_id: sweepDatasetId,
        strategy: sweepStrategy,
        param_grid: grid,
        method: sweepMethod,
        max_trials: sweepMethod === 'random' ? sweepMaxTrials : undefined,
        objective: 'sharpe',
      });
      refreshSweeps();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Invalid parameters');
    } finally {
      setLoading(false);
    }
  };

  // Queue walk forward
  const handleQueueWalkForward = async () => {
    setLoading(true);
    setError(null);
    try {
      const grid = JSON.parse(sweepParamGridText);
      await queueWalkForward({
        dataset_version_id: sweepDatasetId,
        strategy: sweepStrategy,
        mode: wfMode,
        train_size: Number(wfTrainSize),
        test_size: Number(wfTestSize),
        param_grid: grid,
        objective: 'sharpe',
      });
      refreshWalkForwards();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Invalid walk-forward parameters');
    } finally {
      setLoading(false);
    }
  };

  // Run live overfitting simulation
  const handleRunOverfitting = async () => {
    setLoading(true);
    try {
      // Generate synthetic trials return matrix
      const matrix: number[][] = [];
      for (let t = 0; t < simTrials; t++) {
        const row: number[] = [];
        const trueMean = (Math.random() - 0.45) * 0.002;
        const vol = 0.015 + Math.random() * 0.005;
        for (let i = 0; i < simObs; i++) {
          row.push(trueMean + (Math.random() - 0.5) * vol);
        }
        matrix.push(row);
      }
      const res = await computeOverfitting({
        returns_matrix: matrix,
        benchmark_sharpe: 0.0,
        confidence: 0.95,
      });
      setOverfittingResult(res);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Could not compute overfitting statistics');
    } finally {
      setLoading(false);
    }
  };

  // Seal holdout
  const handleSealHoldout = async () => {
    if (!sealName.trim()) return;
    setLoading(true);
    try {
      await sealHoldout({
        name: sealName.trim(),
        dataset_version_id: sweepDatasetId,
        budget: Number(sealBudget),
        notes: sealNotes.trim(),
      });
      setSealName('');
      refreshHoldouts();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to seal holdout');
    } finally {
      setLoading(false);
    }
  };

  // Evaluate holdout
  const handleEvaluateHoldout = async (holdoutId: string) => {
    setLoading(true);
    try {
      await evaluateHoldout(holdoutId, { strategy: 'buy_and_hold' });
      if (selectedHoldoutId === holdoutId) {
        getHoldout(holdoutId).then(setSelectedHoldoutDetail);
      }
      refreshHoldouts();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Holdout evaluation failed or budget exhausted');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* Top Header */}
      <div className="flex flex-col gap-4 border-b border-ink/[0.08] pb-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-ink">Optimization & Statistical Validation Lab</h1>
          <p className="text-sm text-ink/60">
            Multi-parameter sweeps, walk-forward out-of-sample efficiency, and Bailey &amp; López de Prado overfitting defense.
          </p>
        </div>

        {/* Tab navigation pills */}
        <div className="flex flex-wrap gap-1 rounded-xl border border-ink/[0.08] bg-ink/[0.02] p-1">
          <button
            type="button"
            onClick={() => setActiveTab('sweeps')}
            className={cn(
              'flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition',
              activeTab === 'sweeps' ? 'bg-accent text-onaccent shadow-sm' : 'text-ink/60 hover:text-ink'
            )}
          >
            <Sliders className="h-3.5 w-3.5" />
            Parameter Sweeps
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('walkforward')}
            className={cn(
              'flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition',
              activeTab === 'walkforward' ? 'bg-accent text-onaccent shadow-sm' : 'text-ink/60 hover:text-ink'
            )}
          >
            <Layers className="h-3.5 w-3.5" />
            Walk-Forward
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('overfitting')}
            className={cn(
              'flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition',
              activeTab === 'overfitting' ? 'bg-accent text-onaccent shadow-sm' : 'text-ink/60 hover:text-ink'
            )}
          >
            <ShieldCheck className="h-3.5 w-3.5" />
            Overfitting (PSR / DSR)
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('holdout')}
            className={cn(
              'flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition',
              activeTab === 'holdout' ? 'bg-accent text-onaccent shadow-sm' : 'text-ink/60 hover:text-ink'
            )}
          >
            <Lock className="h-3.5 w-3.5" />
            Holdout Vault
          </button>
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-2 rounded-xl border border-rose-500/20 bg-rose-500/10 p-3 text-xs text-rose-600 dark:text-rose-400">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {/* TAB 1: PARAMETER SWEEPS */}
      {activeTab === 'sweeps' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          {/* Controls & Queue form */}
          <div className="space-y-4 lg:col-span-4">
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
              <h2 className="text-sm font-semibold text-ink">New Parameter Sweep</h2>
              <p className="mt-1 text-xs text-ink/50">Run multi-trial grid or Monte Carlo random search across parameters.</p>

              <div className="mt-4 space-y-3 text-xs">
                <div>
                  <label htmlFor={sweepDatasetSelectId} className="font-medium text-ink/80">Target Dataset</label>
                  <select
                    id={sweepDatasetSelectId}
                    aria-label="Target Dataset"
                    value={sweepDatasetId}
                    onChange={(e) => setSweepDatasetId(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink outline-none focus:border-accent"
                  >
                    {datasets.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.dataset_name} (v{d.version} · {d.row_count} bars)
                      </option>
                    ))}
                  </select>
                </div>

                <div>
                  <label htmlFor={sweepStrategySelectId} className="font-medium text-ink/80">Strategy</label>
                  <select
                    id={sweepStrategySelectId}
                    aria-label="Strategy"
                    value={sweepStrategy}
                    onChange={(e) => setSweepStrategy(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink outline-none focus:border-accent"
                  >
                    <option value="sma_cross">sma_cross (Dual SMA Moving Average)</option>
                    <option value="ema_cross">ema_cross (Exponential Moving Average)</option>
                    <option value="rsi_reversion">rsi_reversion (RSI Mean Reversion)</option>
                    <option value="buy_and_hold">buy_and_hold (Benchmark)</option>
                    {strategies.map((s) => (
                      <option key={s.id} value={`custom:${s.id}`}>Custom: {s.name}</option>
                    ))}
                  </select>
                </div>

                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <label htmlFor={sweepSearchMethodSelectId} className="font-medium text-ink/80">Search Method</label>
                    <select
                      id={sweepSearchMethodSelectId}
                      aria-label="Search Method"
                      value={sweepMethod}
                      onChange={(e) => setSweepMethod(e.target.value as 'grid' | 'random')}
                      className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2 py-1.5 text-xs text-ink outline-none focus:border-accent"
                    >
                      <option value="grid">Grid Search</option>
                      <option value="random">Random Sampling</option>
                    </select>
                  </div>
                  <div>
                    <label htmlFor={sweepMaxTrialsInputId} className="font-medium text-ink/80">Max Trials</label>
                    <input
                      id={sweepMaxTrialsInputId}
                      aria-label="Max Trials"
                      type="number"
                      value={sweepMaxTrials}
                      onChange={(e) => setSweepMaxTrials(Number(e.target.value))}
                      className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink outline-none focus:border-accent"
                    />
                  </div>
                </div>

                <div>
                  <label htmlFor={sweepParamGridTextareaId} className="font-medium text-ink/80">Parameter Grid (JSON array of values)</label>
                  <textarea
                    id={sweepParamGridTextareaId}
                    aria-label="Parameter Grid"
                    rows={4}
                    value={sweepParamGridText}
                    onChange={(e) => setSweepParamGridText(e.target.value)}
                    className="mt-1 w-full font-mono text-[11px] rounded-lg border border-ink/[0.12] bg-ink/[0.02] p-2 text-ink outline-none focus:border-accent"
                  />
                </div>

                <button
                  type="button"
                  onClick={handleQueueSweep}
                  disabled={loading}
                  className="flex w-full items-center justify-center gap-2 rounded-xl bg-accent py-2 font-medium text-onaccent shadow-sm transition hover:opacity-90 disabled:opacity-50"
                >
                  <Play className="h-3.5 w-3.5 fill-current" />
                  {loading ? 'Queuing...' : 'Launch Parameter Sweep'}
                </button>
              </div>
            </div>

            {/* Sweep History List */}
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4 shadow-xs">
              <div className="flex items-center justify-between pb-2">
                <span className="text-xs font-semibold text-ink">Sweep History</span>
                <button type="button" onClick={refreshSweeps} className="text-ink/40 hover:text-ink">
                  <RotateCcw className="h-3.5 w-3.5" />
                </button>
              </div>
              <div className="space-y-1.5 max-h-60 overflow-y-auto">
                {sweeps.map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => setSelectedSweepId(s.id)}
                    className={cn(
                      'w-full text-left rounded-xl p-2.5 text-xs transition border',
                      selectedSweepId === s.id
                        ? 'border-accent bg-accent/[0.06] text-accent font-medium'
                        : 'border-ink/[0.05] hover:bg-ink/[0.02] text-ink'
                    )}
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-semibold uppercase tracking-wider text-[10px]">{s.method}</span>
                      <span className={cn('px-1.5 py-0.5 rounded text-[10px]', s.status === 'succeeded' ? 'bg-emerald-500/10 text-emerald-600' : 'bg-amber-500/10 text-amber-600')}>
                        {s.status}
                      </span>
                    </div>
                    <div className="mt-1 flex items-center justify-between text-ink/60 text-[11px]">
                      <span>{s.trial_count} trials</span>
                      {s.best_objective !== undefined && (
                        <span>Best SR: {s.best_objective.toFixed(2)}</span>
                      )}
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Results: Sensitivity Heatmap & Trials Table */}
          <div className="space-y-6 lg:col-span-8">
            {selectedSweepDetail ? (
              <>
                {/* Headline Overfitting Metrics for this sweep */}
                <div className="flex items-center justify-between gap-3">
                  <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 flex-1">
                    <div className="rounded-xl border border-ink/[0.08] bg-surface p-3.5">
                      <span className="text-[11px] font-medium text-ink/50 uppercase">Best Sharpe</span>
                      <p className="mt-1 text-lg font-bold text-ink">
                        {selectedSweepDetail.sweep.best_objective?.toFixed(2) ?? '—'}
                      </p>
                      <span className="text-[10px] text-emerald-600">Trial #{selectedSweepDetail.sweep.best_trial_num ?? 1}</span>
                    </div>
                    <div className="rounded-xl border border-ink/[0.08] bg-surface p-3.5">
                      <span className="text-[11px] font-medium text-ink/50 uppercase">Deflated Sharpe (DSR)</span>
                      <p className="mt-1 text-lg font-bold text-accent">
                        {selectedSweepDetail.sweep.dsr !== undefined ? `${(selectedSweepDetail.sweep.dsr * 100).toFixed(1)}%` : '—'}
                      </p>
                      <span className="text-[10px] text-ink/50">Multi-testing penalized</span>
                    </div>
                    <div className="rounded-xl border border-ink/[0.08] bg-surface p-3.5">
                      <span className="text-[11px] font-medium text-ink/50 uppercase">Probabilistic SR (PSR)</span>
                      <p className="mt-1 text-lg font-bold text-ink">
                        {selectedSweepDetail.sweep.psr !== undefined ? `${(selectedSweepDetail.sweep.psr * 100).toFixed(1)}%` : '—'}
                      </p>
                      <span className="text-[10px] text-ink/50">Non-normal confidence</span>
                    </div>
                    <div className="rounded-xl border border-ink/[0.08] bg-surface p-3.5">
                      <span className="text-[11px] font-medium text-ink/50 uppercase">CSCV PBO</span>
                      <p className="mt-1 text-lg font-bold text-rose-500">
                        {selectedSweepDetail.sweep.pbo !== undefined ? `${(selectedSweepDetail.sweep.pbo * 100).toFixed(1)}%` : '—'}
                      </p>
                      <span className="text-[10px] text-ink/50">Overfitting probability</span>
                    </div>
                  </div>
                </div>

                <div className="flex items-center justify-between">
                  <div className="text-xs text-ink/60">
                    Sweep status: <span className="font-semibold text-ink">{selectedSweepDetail.sweep.status}</span> · Method: <span className="font-mono text-ink">{selectedSweepDetail.sweep.method}</span> · Trials: <span className="font-bold text-ink">{selectedSweepDetail.sweep.trial_count}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    <a
                      href={`/api/v1/optimization/sweep/${selectedSweepDetail.sweep.id}/export?format=pdf`}
                      download
                      className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white shadow-xs hover:bg-accent/90"
                      title="Export 1-page validation report as PDF"
                    >
                      <FileText className="h-3.5 w-3.5" />
                      Export Report (PDF)
                    </a>
                    <a
                      href={`/api/v1/optimization/sweep/${selectedSweepDetail.sweep.id}/export?format=csv`}
                      download
                      className="btn-secondary inline-flex items-center gap-1.5 rounded-lg border border-ink/[0.12] bg-surface px-3 py-1.5 text-xs text-ink hover:bg-ink/[0.04]"
                      title="Export validation report as CSV"
                    >
                      <Download className="h-3.5 w-3.5 text-accent" />
                      Export (CSV)
                    </a>
                  </div>
                </div>

                {/* 2D SENSITIVITY HEATMAP */}
                {selectedSweepDetail.sensitivity ? (
                  <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
                    <div className="flex items-center justify-between">
                      <div>
                        <h3 className="text-sm font-semibold text-ink flex items-center gap-2">
                          <Flame className="h-4 w-4 text-accent" />
                          2D Parameter Sensitivity Heatmap
                        </h3>
                        <p className="text-xs text-ink/50">
                          Parameter stability matrix ({selectedSweepDetail.sensitivity.axis_x} vs {selectedSweepDetail.sensitivity.axis_y}). Click any cell to drill down to its child trials.
                        </p>
                      </div>
                      {selectedCell && (
                        <button
                          type="button"
                          onClick={() => setSelectedCell(null)}
                          className="rounded-md border border-accent/30 bg-accent/10 px-2 py-1 text-[11px] font-medium text-accent hover:bg-accent/20"
                        >
                          Clear drill-down ({selectedSweepDetail.sensitivity.axis_x}={selectedCell.x}, {selectedSweepDetail.sensitivity.axis_y}={selectedCell.y})
                        </button>
                      )}
                    </div>

                    <div className="mt-5 overflow-x-auto">
                      <div className="inline-block min-w-full">
                        <div className="flex flex-col items-center">
                          {/* Y-axis label */}
                          <div className="text-[11px] font-semibold text-ink/60 mb-1">
                            ↑ {selectedSweepDetail.sensitivity.axis_y}
                          </div>

                          <div className="flex">
                            {/* Y-axis values */}
                            <div className="flex flex-col justify-around pr-2 text-[10px] font-mono text-ink/50">
                              {selectedSweepDetail.sensitivity.values_y.map((y) => (
                                <div key={y} className="h-9 flex items-center">{y}</div>
                              ))}
                            </div>

                            {/* Heatmap Grid */}
                            <div className="flex flex-col gap-1">
                              {selectedSweepDetail.sensitivity.matrix.map((row, rIdx) => (
                                <div key={rIdx} className="flex gap-1">
                                  {row.map((val, cIdx) => {
                                    const x_val = selectedSweepDetail.sensitivity?.values_x[cIdx];
                                    const y_val = selectedSweepDetail.sensitivity?.values_y[rIdx];
                                    const isSelected = selectedCell && selectedCell.x === x_val && selectedCell.y === y_val;
                                    const score = val ?? 0;
                                    const bgClass =
                                      val === null
                                        ? 'bg-ink/[0.03]'
                                        : score > 2.0
                                        ? 'bg-emerald-500 text-white'
                                        : score > 1.2
                                        ? 'bg-emerald-400/80 text-black'
                                        : score > 0.5
                                        ? 'bg-amber-300 text-black'
                                        : score > 0.0
                                        ? 'bg-amber-100 text-black'
                                        : 'bg-rose-400 text-white';
                                    return (
                                      <button
                                        type="button"
                                        key={cIdx}
                                        onClick={() => {
                                          if (x_val !== undefined && y_val !== undefined) {
                                            setSelectedCell(isSelected ? null : { x: x_val, y: y_val });
                                          }
                                        }}
                                        title={`X (${selectedSweepDetail.sensitivity?.axis_x}): ${x_val}, Y (${selectedSweepDetail.sensitivity?.axis_y}): ${y_val} → Sharpe: ${val?.toFixed(2) ?? 'N/A'} (Click to drill down)`}
                                        className={cn(
                                          'h-9 w-12 rounded flex items-center justify-center font-mono text-[10px] font-medium transition hover:scale-105 cursor-pointer',
                                          bgClass,
                                          isSelected && 'ring-2 ring-accent ring-offset-2 scale-105 shadow-md font-bold'
                                        )}
                                      >
                                        {val !== null ? val.toFixed(1) : '·'}
                                      </button>
                                    );
                                  })}
                                </div>
                              ))}
                            </div>
                          </div>

                          {/* X-axis values */}
                          <div className="flex gap-1 pl-6 pt-1 text-[10px] font-mono text-ink/50">
                            {selectedSweepDetail.sensitivity.values_x.map((x) => (
                              <div key={x} className="w-12 text-center">{x}</div>
                            ))}
                          </div>
                          {/* X-axis label */}
                          <div className="text-[11px] font-semibold text-ink/60 mt-1">
                            {selectedSweepDetail.sensitivity.axis_x} →
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                ) : null}

                {/* Trials Ranking Table */}
                {(() => {
                  const filteredTrials = selectedSweepDetail.trials.filter((t) => {
                    if (!selectedCell || !selectedSweepDetail.sensitivity) return true;
                    const xParam = selectedSweepDetail.sensitivity.axis_x;
                    const yParam = selectedSweepDetail.sensitivity.axis_y;
                    return (
                      t.params[xParam] === selectedCell.x && t.params[yParam] === selectedCell.y
                    );
                  });

                  return (
                    <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
                      <div className="flex items-center justify-between">
                        <div>
                          <h3 className="text-sm font-semibold text-ink">
                            Trials Ledger ({filteredTrials.length} {selectedCell ? 'filtered' : 'of ' + selectedSweepDetail.trials.length} trials)
                          </h3>
                          {selectedCell && (
                            <p className="text-xs text-accent mt-0.5">
                              Drill-down active: showing trials with {selectedSweepDetail.sensitivity?.axis_x}={selectedCell.x} and {selectedSweepDetail.sensitivity?.axis_y}={selectedCell.y}
                            </p>
                          )}
                        </div>
                      </div>
                      <div className="mt-3 overflow-x-auto">
                        <table className="w-full text-left text-xs">
                          <thead>
                            <tr className="border-b border-ink/[0.08] text-ink/50 text-[11px]">
                              <th className="py-2">#</th>
                              <th className="py-2">Parameters</th>
                              <th className="py-2 text-right">Sharpe</th>
                              <th className="py-2 text-right">Return</th>
                              <th className="py-2 text-right">Max DD</th>
                              <th className="py-2 text-right">Mean Return</th>
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-ink/[0.04]">
                            {filteredTrials.slice(0, 50).map((t) => (
                              <tr key={t.id} className="hover:bg-ink/[0.02]">
                                <td className="py-2 font-mono font-medium text-ink/80">{t.trial_num}</td>
                                <td className="py-2 font-mono text-[11px] text-ink/70">
                                  {Object.entries(t.params).map(([k, v]) => `${k}=${v}`).join(', ')}
                                </td>
                                <td className="py-2 text-right font-mono font-bold text-ink">
                                  {t.objective_value.toFixed(2)}
                                </td>
                                <td className="py-2 text-right font-mono text-ink/70">
                                  {t.metrics.total_return ? `${(t.metrics.total_return * 100).toFixed(1)}%` : '—'}
                                </td>
                                <td className="py-2 text-right font-mono text-rose-500">
                                  {t.metrics.max_drawdown ? `-${(t.metrics.max_drawdown * 100).toFixed(1)}%` : '—'}
                                </td>
                                <td className="py-2 text-right font-mono text-ink/60">
                                  {t.returns_mean !== undefined ? `${(t.returns_mean * 100).toFixed(3)}%` : '—'}
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    </div>
                  );
                })()}
              </>
            ) : (
              <div className="flex h-64 items-center justify-center rounded-2xl border border-dashed border-ink/[0.12] text-xs text-ink/40">
                Select or queue a parameter sweep to inspect sensitivity heatmap and trial statistics.
              </div>
            )}
          </div>
        </div>
      )}

      {/* TAB 2: WALK-FORWARD VALIDATION */}
      {activeTab === 'walkforward' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          {/* Walk-forward Form */}
          <div className="space-y-4 lg:col-span-4">
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
              <h2 className="text-sm font-semibold text-ink">Walk-Forward Validation</h2>
              <p className="mt-1 text-xs text-ink/50">Stitch non-overlapping out-of-sample test windows to compute Walk-Forward Efficiency (WFE).</p>

              <div className="mt-4 space-y-3 text-xs">
                <div>
                  <label htmlFor={wfDatasetSelectId} className="font-medium text-ink/80">Target Dataset</label>
                  <select
                    id={wfDatasetSelectId}
                    aria-label="Target Dataset"
                    value={sweepDatasetId}
                    onChange={(e) => setSweepDatasetId(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink outline-none"
                  >
                    {datasets.map((d) => (
                      <option key={d.id} value={d.id}>{d.dataset_name} (v{d.version})</option>
                    ))}
                  </select>
                </div>

                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <label htmlFor={wfTrainSizeInputId} className="font-medium text-ink/80">Train Window (bars)</label>
                    <input
                      id={wfTrainSizeInputId}
                      aria-label="Train Window"
                      type="number"
                      value={wfTrainSize}
                      onChange={(e) => setWfTrainSize(Number(e.target.value))}
                      className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2 py-1.5 text-xs text-ink"
                    />
                  </div>
                  <div>
                    <label htmlFor={wfTestSizeInputId} className="font-medium text-ink/80">Test Window (bars)</label>
                    <input
                      id={wfTestSizeInputId}
                      aria-label="Test Window"
                      type="number"
                      value={wfTestSize}
                      onChange={(e) => setWfTestSize(Number(e.target.value))}
                      className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2 py-1.5 text-xs text-ink"
                    />
                  </div>
                </div>

                <div>
                  <label htmlFor={wfModeSelectId} className="font-medium text-ink/80">Window Mode</label>
                  <select
                    id={wfModeSelectId}
                    aria-label="Window Mode"
                    value={wfMode}
                    onChange={(e) => setWfMode(e.target.value as 'rolling' | 'anchored')}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2 py-1.5 text-xs text-ink"
                  >
                    <option value="rolling">Rolling (Sliding Train Window)</option>
                    <option value="anchored">Anchored (Expanding Train Window)</option>
                  </select>
                </div>

                <button
                  type="button"
                  onClick={handleQueueWalkForward}
                  disabled={loading}
                  className="flex w-full items-center justify-center gap-2 rounded-xl bg-accent py-2 font-medium text-onaccent shadow-sm transition hover:opacity-90 disabled:opacity-50"
                >
                  <Layers className="h-3.5 w-3.5" />
                  {loading ? 'Running...' : 'Run Walk-Forward Analysis'}
                </button>
              </div>
            </div>

            {/* Reports List */}
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4 shadow-xs">
              <span className="text-xs font-semibold text-ink">Walk-Forward Reports</span>
              <div className="mt-2 space-y-1.5 max-h-60 overflow-y-auto">
                {reports.map((r) => (
                  <button
                    key={r.id}
                    type="button"
                    onClick={() => setSelectedReportId(r.id)}
                    className={cn(
                      'w-full text-left rounded-xl p-2.5 text-xs transition border',
                      selectedReportId === r.id
                        ? 'border-accent bg-accent/[0.06] text-accent font-medium'
                        : 'border-ink/[0.05] hover:bg-ink/[0.02] text-ink'
                    )}
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-semibold uppercase text-[10px]">{r.mode}</span>
                      <span className="text-[10px] text-emerald-600">WFE: {(r.efficiency_ratio ? r.efficiency_ratio * 100 : 0).toFixed(0)}%</span>
                    </div>
                    <div className="mt-1 flex items-center justify-between text-ink/50 text-[11px]">
                      <span>Folds: {r.folds?.length ?? 0}</span>
                      <span>OOS SR: {r.out_of_sample_sharpe?.toFixed(2) ?? '—'}</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Report Detail View */}
          <div className="space-y-6 lg:col-span-8">
            {selectedReport ? (
              <>
                <div className="grid grid-cols-3 gap-3">
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-xs text-ink/50">Walk-Forward Efficiency (WFE)</span>
                    <p className="mt-1 text-2xl font-bold text-accent">
                      {selectedReport.efficiency_ratio !== undefined ? `${(selectedReport.efficiency_ratio * 100).toFixed(1)}%` : '—'}
                    </p>
                    <span className="text-[10px] text-emerald-600 font-medium">
                      {(selectedReport.efficiency_ratio ?? 0) >= 0.5 ? 'Robust (≥ 50%)' : 'Overfit Decay (< 50%)'}
                    </span>
                  </div>
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-xs text-ink/50">In-Sample Mean Sharpe</span>
                    <p className="mt-1 text-2xl font-bold text-ink">
                      {selectedReport.in_sample_sharpe?.toFixed(2) ?? '—'}
                    </p>
                    <span className="text-[10px] text-ink/50">Training optimum</span>
                  </div>
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-xs text-ink/50">Out-of-Sample Mean Sharpe</span>
                    <p className="mt-1 text-2xl font-bold text-emerald-600">
                      {selectedReport.out_of_sample_sharpe?.toFixed(2) ?? '—'}
                    </p>
                    <span className="text-[10px] text-ink/50">Realized forward performance</span>
                  </div>
                </div>

                {/* Walk-Forward Timeline Chart (Task 9) */}
                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <div>
                      <h3 className="text-sm font-semibold text-ink flex items-center gap-2">
                        <Layers className="h-4 w-4 text-accent" />
                        Walk-Forward Window Timeline ({selectedReport.folds?.length ?? 0} Windows)
                      </h3>
                      <p className="text-xs text-ink/50">
                        Progression across sliding windows. In-sample optimization (blue) paired with out-of-sample forward evaluation (emerald).
                      </p>
                    </div>
                    <div className="flex items-center gap-3 text-[11px]">
                      <span className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-accent/70" />In-Sample (Train)</span>
                      <span className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-emerald-500" />Out-of-Sample (Test)</span>
                    </div>
                  </div>

                  {/* Visual timeline bars */}
                  <div className="mt-5 space-y-4">
                    {selectedReport.folds?.map((f) => {
                      const maxExtent = Math.max(...(selectedReport.folds?.map((x) => x.test_end) ?? [100]));
                      const leftTrain = `${(f.train_begin / maxExtent) * 100}%`;
                      const widthTrain = `${Math.max(2, ((f.train_end - f.train_begin) / maxExtent) * 100)}%`;
                      const leftTest = `${(f.test_begin / maxExtent) * 100}%`;
                      const widthTest = `${Math.max(2, ((f.test_end - f.test_begin) / maxExtent) * 100)}%`;

                      return (
                        <div key={f.fold} className="rounded-xl border border-ink/[0.06] bg-ink/[0.02] p-3 text-xs space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-semibold text-ink">Window / Fold #{f.fold}</span>
                            <div className="flex items-center gap-4">
                              <span className="text-ink/60">IS Sharpe: <strong className="text-ink">{f.in_sample_objective.toFixed(2)}</strong></span>
                              <span className="text-ink/60">OOS Sharpe: <strong className="text-emerald-600 font-bold">{f.out_of_sample_objective.toFixed(2)}</strong></span>
                            </div>
                          </div>

                          {/* Graphical timeline track */}
                          <div className="relative h-6 w-full rounded-md bg-ink/[0.06] overflow-hidden">
                            <div
                              style={{ left: leftTrain, width: widthTrain }}
                              className="absolute top-0 bottom-0 bg-accent/40 border-r border-accent/60 flex items-center px-1.5 text-[9px] font-mono text-ink truncate"
                              title={`Window #${f.fold} Train: [${f.train_begin}..${f.train_end}] (IS Sharpe: ${f.in_sample_objective.toFixed(2)})`}
                            >
                              Train [{f.train_begin}..{f.train_end}]
                            </div>
                            <div
                              style={{ left: leftTest, width: widthTest }}
                              className="absolute top-0 bottom-0 bg-emerald-500 flex items-center px-1.5 text-[9px] font-mono text-white truncate font-medium"
                              title={`Window #${f.fold} Test: [${f.test_begin}..${f.test_end}] (OOS Sharpe: ${f.out_of_sample_objective.toFixed(2)})`}
                            >
                              Test [{f.test_begin}..{f.test_end}]
                            </div>
                          </div>

                          <div className="flex items-center justify-between text-[11px] text-ink/50 pt-0.5">
                            <span className="font-mono">Fitted: {JSON.stringify(f.best_params)}</span>
                            <span>Efficiency: {(f.in_sample_objective > 0 ? (f.out_of_sample_objective / f.in_sample_objective * 100).toFixed(0) : 0)}%</span>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              </>
            ) : (
              <div className="flex h-64 items-center justify-center rounded-2xl border border-dashed border-ink/[0.12] text-xs text-ink/40">
                Launch or select a walk-forward analysis to review fold progression and efficiency ratio.
              </div>
            )}
          </div>
        </div>
      )}

      {/* TAB 3: OVERFITTING DIAGNOSTICS */}
      {activeTab === 'overfitting' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          <div className="space-y-4 lg:col-span-4">
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
              <h2 className="text-sm font-semibold text-ink">Overfitting Simulation Lab</h2>
              <p className="mt-1 text-xs text-ink/50">Calculate Probabilistic Sharpe, Deflated Sharpe, and CSCV Probability of Backtest Overfitting (PBO).</p>

              <div className="mt-4 space-y-3 text-xs">
                <div>
                  <div className="flex items-center justify-between">
                    <label htmlFor={simTrialsInputId} className="font-medium text-ink/80">Number of Tested Trials (N)</label>
                    <span className="font-mono text-[10px] text-ink/40">trial count</span>
                  </div>
                  <input
                    id={simTrialsInputId}
                    aria-label="Number of Tested Trials"
                    type="number"
                    value={simTrials}
                    onChange={(e) => setSimTrials(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink"
                  />
                  <p className="mt-1 text-[10.5px] leading-4 text-ink/40">
                    Total candidate strategies tested (parameter N in López de Prado&apos;s Deflated Sharpe Ratio to discount for multiple-testing selection bias).
                  </p>
                </div>
                <div>
                  <label htmlFor={simObsInputId} className="font-medium text-ink/80">Observations Per Trial</label>
                  <input
                    id={simObsInputId}
                    aria-label="Observations Per Trial"
                    type="number"
                    value={simObs}
                    onChange={(e) => setSimObs(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink"
                  />
                </div>

                <button
                  type="button"
                  onClick={handleRunOverfitting}
                  disabled={loading}
                  className="flex w-full items-center justify-center gap-2 rounded-xl bg-accent py-2 font-medium text-onaccent shadow-sm transition hover:opacity-90 disabled:opacity-50"
                >
                  <Sparkles className="h-3.5 w-3.5" />
                  {loading ? 'Evaluating...' : 'Compute Overfitting Statistics'}
                </button>
              </div>
            </div>

            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4 text-xs text-ink/60 space-y-2">
              <span className="font-semibold text-ink flex items-center gap-1.5">
                <Info className="h-3.5 w-3.5 text-accent" />
                Bailey &amp; López de Prado Formulas
              </span>
              <p className="text-[11px] leading-relaxed">
                <strong>Deflated Sharpe Ratio (DSR)</strong> penalizes for the number of trials tested and the variance of the Sharpe estimates, neutralizing selection bias under multiple testing.
              </p>
              <p className="text-[11px] leading-relaxed">
                <strong>Combinatorially Symmetric Cross-Validation (CSCV)</strong> generates 16 combinatorial partitionings to directly measure the probability that in-sample winner underperforms the median out-of-sample (PBO).
              </p>
            </div>
          </div>

          <div className="space-y-6 lg:col-span-8">
            {overfittingResult ? (
              <div className="space-y-6">
                <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-[11px] text-ink/50 uppercase">DSR (Deflated Sharpe)</span>
                    <p className="mt-1 text-2xl font-bold text-accent">{(overfittingResult.dsr * 100).toFixed(1)}%</p>
                    <span className="text-[10px] text-emerald-600">Confidence above zero</span>
                  </div>
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-[11px] text-ink/50 uppercase">PSR (Probabilistic SR)</span>
                    <p className="mt-1 text-2xl font-bold text-ink">{(overfittingResult.psr * 100).toFixed(1)}%</p>
                    <span className="text-[10px] text-ink/50">Higher moments adjusted</span>
                  </div>
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-[11px] text-ink/50 uppercase">Min Track Record (MinTRL)</span>
                    <p className="mt-1 text-2xl font-bold text-ink">{overfittingResult.min_trl.toFixed(1)}</p>
                    <span className="text-[10px] text-ink/50">Obs periods required</span>
                  </div>
                  <div className="rounded-xl border border-ink/[0.08] bg-surface p-4">
                    <span className="text-[11px] text-ink/50 uppercase">CSCV PBO</span>
                    <p className="mt-1 text-2xl font-bold text-rose-500">{(overfittingResult.pbo * 100).toFixed(1)}%</p>
                    <span className="text-[10px] text-rose-500 font-medium">Overfitting risk</span>
                  </div>
                </div>

                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs space-y-4">
                  <h3 className="text-sm font-semibold text-ink">Statistical Diagnostics Summary</h3>
                  <div className="grid grid-cols-2 gap-4 text-xs font-mono">
                    <div className="rounded-lg bg-ink/[0.02] p-3">
                      <span className="text-ink/50 block text-[10px] uppercase font-sans">Best Observed Sharpe</span>
                      <strong className="text-base text-ink">{overfittingResult.best_sharpe.toFixed(3)}</strong>
                    </div>
                    <div className="rounded-lg bg-ink/[0.02] p-3">
                      <span className="text-ink/50 block text-[10px] uppercase font-sans">Expected Max Sharpe (E[max])</span>
                      <strong className="text-base text-ink">{overfittingResult.expected_max_sharpe.toFixed(3)}</strong>
                    </div>
                    <div className="rounded-lg bg-ink/[0.02] p-3">
                      <span className="text-ink/50 block text-[10px] uppercase font-sans">Sharpe Estimator Variance</span>
                      <strong className="text-base text-ink">{overfittingResult.sharpe_variance.toFixed(4)}</strong>
                    </div>
                    <div className="rounded-lg bg-ink/[0.02] p-3">
                      <span className="text-ink/50 block text-[10px] uppercase font-sans">Total Trials Tested</span>
                      <strong className="text-base text-ink">{overfittingResult.num_trials}</strong>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex h-64 items-center justify-center rounded-2xl border border-dashed border-ink/[0.12] text-xs text-ink/40">
                Click &ldquo;Compute Overfitting Statistics&rdquo; to evaluate Deflated Sharpe and CSCV PBO.
              </div>
            )}
          </div>
        </div>
      )}

      {/* TAB 4: HOLDOUT VAULT */}
      {activeTab === 'holdout' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
          {/* Seal Holdout Form */}
          <div className="space-y-4 lg:col-span-4">
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
              <h2 className="text-sm font-semibold text-ink flex items-center gap-2">
                <Lock className="h-4 w-4 text-accent" />
                Seal New Holdout Dataset
              </h2>
              <p className="mt-1 text-xs text-ink/50">
                Metered out-of-sample data with an immutable evaluation budget. Once exhausted, further runs are permanently blocked.
              </p>

              <div className="mt-4 space-y-3 text-xs">
                <div>
                  <label htmlFor={sealNameInputId} className="font-medium text-ink/80">Holdout Name</label>
                  <input
                    id={sealNameInputId}
                    aria-label="Holdout Name"
                    type="text"
                    placeholder="e.g. 2026-Q1 Blind Holdout"
                    value={sealName}
                    onChange={(e) => setSealName(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink"
                  />
                </div>

                <div>
                  <label htmlFor={sealDatasetSelectId} className="font-medium text-ink/80">Source Dataset Version</label>
                  <select
                    id={sealDatasetSelectId}
                    aria-label="Source Dataset Version"
                    value={sweepDatasetId}
                    onChange={(e) => setSweepDatasetId(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink"
                  >
                    {datasets.map((d) => (
                      <option key={d.id} value={d.id}>{d.dataset_name} (v{d.version})</option>
                    ))}
                  </select>
                </div>

                <div>
                  <label htmlFor={sealBudgetInputId} className="font-medium text-ink/80">Evaluation Budget (Times)</label>
                  <input
                    id={sealBudgetInputId}
                    aria-label="Evaluation Budget"
                    type="number"
                    min={1}
                    max={20}
                    value={sealBudget}
                    onChange={(e) => setSealBudget(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface px-2.5 py-1.5 text-xs text-ink"
                  />
                </div>

                <div>
                  <label htmlFor={sealNotesTextareaId} className="font-medium text-ink/80">Vault Notes</label>
                  <textarea
                    id={sealNotesTextareaId}
                    aria-label="Vault Notes"
                    rows={2}
                    placeholder="Verification purpose, lockdown reason..."
                    value={sealNotes}
                    onChange={(e) => setSealNotes(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/[0.12] bg-surface p-2 text-xs text-ink"
                  />
                </div>

                <button
                  type="button"
                  onClick={handleSealHoldout}
                  disabled={loading || !sealName.trim()}
                  className="flex w-full items-center justify-center gap-2 rounded-xl bg-accent py-2 font-medium text-onaccent shadow-sm transition hover:opacity-90 disabled:opacity-50"
                >
                  <Lock className="h-3.5 w-3.5" />
                  Seal Holdout Dataset
                </button>
              </div>
            </div>

            {/* Holdouts List */}
            <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4 shadow-xs">
              <span className="text-xs font-semibold text-ink">Sealed Holdouts</span>
              <div className="mt-2 space-y-1.5 max-h-60 overflow-y-auto">
                {holdouts.map((h) => {
                  const exhausted = h.evaluations_used >= h.budget;
                  return (
                    <button
                      key={h.id}
                      type="button"
                      onClick={() => setSelectedHoldoutId(h.id)}
                      className={cn(
                        'w-full text-left rounded-xl p-2.5 text-xs transition border',
                        selectedHoldoutId === h.id
                          ? 'border-accent bg-accent/[0.06] text-accent font-medium'
                          : 'border-ink/[0.05] hover:bg-ink/[0.02] text-ink'
                      )}
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-semibold text-ink">{h.name}</span>
                        <span className={cn('px-1.5 py-0.5 rounded text-[10px]', exhausted ? 'bg-rose-500/10 text-rose-600' : 'bg-emerald-500/10 text-emerald-600')}>
                          {h.evaluations_used} / {h.budget} used
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Holdout Detail & Evaluations */}
          <div className="space-y-6 lg:col-span-8">
            {selectedHoldoutDetail ? (
              <div className="space-y-6">
                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs flex items-center justify-between">
                  <div>
                    <h3 className="text-base font-bold text-ink">{selectedHoldoutDetail.holdout.name}</h3>
                    <p className="text-xs text-ink/50 mt-0.5">
                      Sealed: {new Date(selectedHoldoutDetail.holdout.sealed_at).toLocaleDateString()} · Budget: {selectedHoldoutDetail.holdout.evaluations_used} of {selectedHoldoutDetail.holdout.budget} consumed
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => handleEvaluateHoldout(selectedHoldoutDetail.holdout.id)}
                    disabled={loading || selectedHoldoutDetail.holdout.evaluations_used >= selectedHoldoutDetail.holdout.budget}
                    className="flex items-center gap-2 rounded-xl bg-accent px-4 py-2 text-xs font-semibold text-onaccent shadow-sm transition hover:opacity-90 disabled:opacity-50"
                  >
                    <Play className="h-3.5 w-3.5 fill-current" />
                    {selectedHoldoutDetail.holdout.evaluations_used >= selectedHoldoutDetail.holdout.budget ? 'Budget Exhausted' : 'Evaluate Strategy'}
                  </button>
                </div>

                {/* Audit trail */}
                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
                  <h4 className="text-xs font-semibold text-ink uppercase tracking-wider">Evaluation Audit Trail</h4>
                  <div className="mt-3 space-y-2">
                    {selectedHoldoutDetail.evaluations.length === 0 ? (
                      <p className="text-xs text-ink/40">No evaluations run yet. Holdout remains pristine.</p>
                    ) : (
                      selectedHoldoutDetail.evaluations.map((ev) => (
                        <div key={ev.id} className="rounded-xl border border-ink/[0.06] bg-ink/[0.02] p-3 text-xs flex items-center justify-between">
                          <div>
                            <span className="font-semibold text-ink">Evaluation #{ev.evaluation_num}</span>
                            <span className="ml-2 text-[11px] text-ink/40">{new Date(ev.evaluated_at).toLocaleString()}</span>
                          </div>
                          <div className="font-mono text-xs">
                            Sharpe: <strong className="text-ink">{typeof (ev.metrics as Record<string, unknown>)?.sharpe === 'number' ? ((ev.metrics as Record<string, unknown>).sharpe as number).toFixed(2) : '—'}</strong>
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex h-64 items-center justify-center rounded-2xl border border-dashed border-ink/[0.12] text-xs text-ink/40">
                Select or seal a holdout dataset to monitor metered access and evaluation logs.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
