'use client';

import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  BarChart,
  Bar,
  Cell,
} from 'recharts';
import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  Download,
  FileCode2,
  FileSpreadsheet,
  FileText,
  FlaskConical,
  History,
  Layers,
  Play,
  RotateCcw,
  BookmarkPlus,
  BookmarkCheck,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  TrendingUp,
} from 'lucide-react';
import {
  exportSignalTest,
  getSignalTest,
  listSignalTests,
  runSignalTest,
  listSignals,
  saveSignal,
  type SavedSignalView,
  type SignalTestDetailView,
  type SignalTestSummaryView,
} from '@/lib/alpha';
import { chartColors, useTheme } from '@/lib/theme';
import { cn } from '@/lib/utils';
import { SignalTestReportPanel } from './ReportPanel';
import { WorkspaceGuide } from './WorkspaceGuide';

interface HorizonRow {
  horizon: number;
  mean_ic: number;
  ic_stdev: number;
  ic_t_stat: number;
}

interface QuantileRow {
  quantile: number;
  mean_forward_return: number;
  observation_count: number;
}

export function AlphaWorkbench() {
  const { theme } = useTheme();
  const colors = chartColors(theme);

  const [tests, setTests] = useState<SignalTestSummaryView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedTest, setSelectedTest] = useState<SignalTestDetailView | null>(null);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [exporting, setExporting] = useState<'pdf' | 'json' | 'csv' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reportsRefresh, setReportsRefresh] = useState(0);

  // Form State
  const [signalType, setSignalType] = useState<'momentum_5d' | 'mean_reversion_5d' | 'sma_crossover_10_30' | 'sentiment' | 'custom'>('momentum_5d');
  const [customSignalName, setCustomSignalName] = useState('');
  const [universe, setUniverse] = useState('AAPL, MSFT, GOOG, AMZN, NVDA, META, TSLA, JPM, UNH, V');
  const [datasetVersionId, setDatasetVersionId] = useState('');
  const [primaryHorizon, setPrimaryHorizon] = useState(5);
  const [quantileCount, setQuantileCount] = useState(5);
  // Owner approved default gate thresholds: Mean IC >= 0.05, t-stat >= 2.0, Half-life <= 20 days.
  const [minMeanIc, setMinMeanIc] = useState('0.05');
  const [minTStat, setMinTStat] = useState('2.0');
  const [maxHalfLife, setMaxHalfLife] = useState('20.0');

  // Signal Library State
  const [savedSignals, setSavedSignals] = useState<SavedSignalView[]>([]);
  const [selectedSignalId, setSelectedSignalId] = useState<string>('');
  const [signalSaveName, setSignalSaveName] = useState('');
  const [signalSaveDesc, setSignalSaveDesc] = useState('');
  const [savingSignal, setSavingSignal] = useState(false);
  const [saveSuccessMessage, setSaveSuccessMessage] = useState<string | null>(null);

  const effectiveSignalName = signalType === 'custom' ? customSignalName.trim() || 'custom_signal' : signalType;

  // Load existing signal tests
  const loadTests = async (selectLatest = false) => {
    try {
      setLoading(true);
      setError(null);
      const list = await listSignalTests();
      setTests(list);
      if (selectLatest && list.length > 0) {
        setSelectedId(list[0].id);
      } else if (!selectedId && list.length > 0) {
        setSelectedId(list[0].id);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to list signal tests');
    } finally {
      setLoading(false);
    }
  };

  const loadSavedSignals = async () => {
    try {
      const list = await listSignals();
      setSavedSignals(list);
    } catch {
      // ignore
    }
  };

  useEffect(() => {
    loadTests();
    loadSavedSignals();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Fetch detail when selection changes
  useEffect(() => {
    if (!selectedId) {
      setSelectedTest(null);
      return;
    }
    let live = true;
    getSignalTest(selectedId)
      .then((data) => {
        if (live) setSelectedTest(data);
      })
      .catch((err) => {
        if (live) setError(err instanceof Error ? err.message : 'Failed to get signal test');
      });
    return () => {
      live = false;
    };
  }, [selectedId, reportsRefresh]);

  const handleSaveToLibrary = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!signalSaveName.trim()) return;
    try {
      setSavingSignal(true);
      setError(null);
      const parsedMinIc = minMeanIc ? parseFloat(minMeanIc) : null;
      const parsedMinT = minTStat ? parseFloat(minTStat) : null;
      const parsedMaxHalf = maxHalfLife ? parseFloat(maxHalfLife) : null;

      const res = await saveSignal({
        name: signalSaveName.trim(),
        description: signalSaveDesc.trim() || undefined,
        signal_type: effectiveSignalName,
        parameters: {
          universe,
          datasetVersionId,
          primaryHorizon,
          quantileCount,
        },
        threshold: {
          min_mean_ic: parsedMinIc,
          min_t_stat: parsedMinT,
          max_half_life_days: parsedMaxHalf,
        },
      });

      setSaveSuccessMessage(`Saved version ${res.version} to Signal Library!`);
      setTimeout(() => setSaveSuccessMessage(null), 4000);
      setSignalSaveName('');
      setSignalSaveDesc('');
      await loadSavedSignals();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save signal to library');
    } finally {
      setSavingSignal(false);
    }
  };

  const handleReloadSavedSignal = (sig: SavedSignalView) => {
    setSelectedSignalId(sig.id);
    if (['momentum_5d', 'mean_reversion_5d', 'sma_crossover_10_30', 'sentiment'].includes(sig.signal_type)) {
      setSignalType(sig.signal_type as typeof signalType);
    } else {
      setSignalType('custom');
      setCustomSignalName(sig.signal_type);
    }
    const params = sig.parameters as Record<string, unknown>;
    if (typeof params?.universe === 'string') setUniverse(params.universe);
    if (typeof params?.datasetVersionId === 'string') setDatasetVersionId(params.datasetVersionId);
    if (typeof params?.primaryHorizon === 'number') setPrimaryHorizon(params.primaryHorizon);
    if (typeof params?.quantileCount === 'number') setQuantileCount(params.quantileCount);
    if (sig.threshold) {
      if (sig.threshold.min_mean_ic !== undefined && sig.threshold.min_mean_ic !== null) {
        setMinMeanIc(sig.threshold.min_mean_ic.toString());
      }
      if (sig.threshold.min_t_stat !== undefined && sig.threshold.min_t_stat !== null) {
        setMinTStat(sig.threshold.min_t_stat.toString());
      }
      if (sig.threshold.max_half_life_days !== undefined && sig.threshold.max_half_life_days !== null) {
        setMaxHalfLife(sig.threshold.max_half_life_days.toString());
      }
    }
  };

  const handleRunTest = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      setRunning(true);
      setError(null);
      const parsedMinIc = minMeanIc ? parseFloat(minMeanIc) : null;
      const parsedMinT = minTStat ? parseFloat(minTStat) : null;
      const parsedMaxHalf = maxHalfLife ? parseFloat(maxHalfLife) : null;

      const symbols = universe
        .split(',')
        .map((s) => s.trim().toUpperCase())
        .filter(Boolean);

      const res = await runSignalTest({
        signal_name: effectiveSignalName,
        signal_id: selectedSignalId || undefined,
        symbols: symbols.length > 0 ? symbols : undefined,
        dataset_version_id: datasetVersionId.trim() || undefined,
        primary_horizon: primaryHorizon,
        quantile_count: quantileCount,
        threshold:
          parsedMinIc !== null || parsedMinT !== null || parsedMaxHalf !== null
            ? {
                min_mean_ic: parsedMinIc,
                min_t_stat: parsedMinT,
                max_half_life_days: parsedMaxHalf,
              }
            : null,
      });

      await loadTests();
      setSelectedId(res.id);
      setSelectedTest(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to run signal test');
    } finally {
      setRunning(false);
    }
  };

  const handleExport = async (format: 'pdf' | 'json' | 'csv') => {
    if (!selectedTest) return;
    try {
      setExporting(format);
      await exportSignalTest(selectedTest.id, format);
      setReportsRefresh((k) => k + 1);
    } catch (err) {
      setError(err instanceof Error ? err.message : `Export failed for ${format}`);
    } finally {
      setExporting(null);
    }
  };

  // Extract decay and quantile structures from report_json
  const reportData = selectedTest?.report_json as {
    decay?: { horizon_ics?: HorizonRow[]; half_life_days?: number; r_squared?: number };
    quantiles?: { quantile_returns?: QuantileRow[]; top_bottom_spread?: number; top_quantile_turnover?: number };
    gate_evaluation?: { passed?: boolean; reasons?: string[]; threshold?: Record<string, unknown> };
  } | undefined;

  const horizonIcs: HorizonRow[] = useMemo(() => {
    if (reportData?.decay?.horizon_ics) {
      return reportData.decay.horizon_ics;
    }
    return [];
  }, [reportData]);

  const quantileReturns: QuantileRow[] = useMemo(() => {
    if (reportData?.quantiles?.quantile_returns) {
      return reportData.quantiles.quantile_returns;
    }
    return [];
  }, [reportData]);

  return (
    <div className="space-y-6 p-4 sm:p-6 lg:p-8" data-testid="alpha-workbench">
      <WorkspaceGuide tab="alpha" />

      {/* Prominent Research Disclaimer Banner */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-2xl border border-amber-500/30 bg-amber-500/[0.08] p-4 text-amber-500">
        <div className="flex items-center gap-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-amber-500/20 text-amber-500 font-bold">
            <FlaskConical className="h-5 w-5" />
          </span>
          <div>
            <div className="flex items-center gap-2">
              <span className="inline-block rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-0.5 font-mono text-[11px] font-bold uppercase tracking-wider text-amber-500">
                research only, not a prediction
              </span>
              <span className="text-xs font-semibold uppercase tracking-wider opacity-75">M12 Alpha Workbench</span>
            </div>
            <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
              Evaluates historical predictive power via Information Coefficients and decay half-life. Past statistical correlation does not predict future market returns.
            </p>
          </div>
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-3 rounded-xl border border-loss/30 bg-loss/10 p-4 text-xs text-loss">
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
        {/* Left Sidebar: Run Form & Test History */}
        <div className="space-y-6 lg:col-span-4">
          {/* New Signal Evaluation Form */}
          <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
            <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
              <Sparkles className="h-4 w-4 text-accent" />
              Evaluate Signal
            </h3>
            <p className="mt-1 text-xs text-ink/50">Run multi-horizon rank IC and decay metrics against point-in-time data.</p>

            <form onSubmit={handleRunTest} className="mt-4 space-y-4">
              {savedSignals.length > 0 && (
                <div>
                  <label className="block text-xs font-medium text-ink">Reload Saved Signal (Signal Library)</label>
                  <select
                    value={selectedSignalId}
                    onChange={(e) => {
                      const found = savedSignals.find((s) => s.id === e.target.value);
                      if (found) handleReloadSavedSignal(found);
                    }}
                    className="mt-1.5 block w-full rounded-lg border border-accent/40 bg-accent/[0.04] px-3 py-2 text-xs font-medium text-ink focus:border-accent focus:outline-none"
                  >
                    <option value="">-- Choose saved signal to reload --</option>
                    {savedSignals.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} (v{s.latest_version} · {s.signal_type})
                      </option>
                    ))}
                  </select>
                </div>
              )}

              <div>
                <label className="block text-xs font-medium text-ink">Signal Generator</label>
                <select
                  value={signalType}
                  onChange={(e) => setSignalType(e.target.value as typeof signalType)}
                  className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 text-xs text-ink focus:border-accent focus:outline-none"
                >
                  <option value="momentum_5d">5-Day Momentum (momentum_5d)</option>
                  <option value="mean_reversion_5d">5-Day Mean Reversion (mean_reversion_5d)</option>
                  <option value="sma_crossover_10_30">10/30 SMA Crossover (sma_crossover_10_30)</option>
                  <option value="sentiment">M42 Sentiment Score (sentiment)</option>
                  <option value="custom">Custom Signal Name...</option>
                </select>
              </div>

              {signalType === 'custom' && (
                <div>
                  <label className="block text-xs font-medium text-ink">Custom Signal Name</label>
                  <input
                    type="text"
                    value={customSignalName}
                    onChange={(e) => setCustomSignalName(e.target.value)}
                    placeholder="e.g. analyst_revisions_score"
                    className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 text-xs text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
              )}

              <div>
                <label className="block text-xs font-medium text-ink">Universe Symbols (comma separated)</label>
                <input
                  type="text"
                  value={universe}
                  onChange={(e) => setUniverse(e.target.value)}
                  className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 font-mono text-xs text-ink focus:border-accent focus:outline-none"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-ink">Dataset Version ID (Optional)</label>
                <input
                  type="text"
                  value={datasetVersionId}
                  onChange={(e) => setDatasetVersionId(e.target.value)}
                  placeholder="UUID binding point-in-time universe"
                  className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 font-mono text-xs text-ink focus:border-accent focus:outline-none"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-medium text-ink">Primary Horizon</label>
                  <select
                    value={primaryHorizon}
                    onChange={(e) => setPrimaryHorizon(parseInt(e.target.value, 10))}
                    className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 text-xs text-ink focus:border-accent focus:outline-none"
                  >
                    <option value={1}>1 Day</option>
                    <option value={5}>5 Days</option>
                    <option value={10}>10 Days</option>
                    <option value={20}>20 Days</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs font-medium text-ink">Quantile Count</label>
                  <select
                    value={quantileCount}
                    onChange={(e) => setQuantileCount(parseInt(e.target.value, 10))}
                    className="mt-1.5 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-3 py-2 text-xs text-ink focus:border-accent focus:outline-none"
                  >
                    <option value={3}>3 (Tertiaries)</option>
                    <option value={5}>5 (Quintiles)</option>
                    <option value={10}>10 (Deciles)</option>
                  </select>
                </div>
              </div>

              <div className="border-t border-ink/[0.06] pt-3">
                <p className="font-mono text-[10px] uppercase tracking-wider text-ink/50">Owner Gate Thresholds (M12)</p>
                <div className="mt-2 grid grid-cols-3 gap-2">
                  <div>
                    <label className="block text-[10px] text-ink/70">Min Mean IC</label>
                    <input
                      type="number"
                      step="0.005"
                      value={minMeanIc}
                      onChange={(e) => setMinMeanIc(e.target.value)}
                      className="mt-1 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-2 py-1.5 text-xs text-ink"
                    />
                  </div>
                  <div>
                    <label className="block text-[10px] text-ink/70">Min t-stat</label>
                    <input
                      type="number"
                      step="0.1"
                      value={minTStat}
                      onChange={(e) => setMinTStat(e.target.value)}
                      className="mt-1 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-2 py-1.5 text-xs text-ink"
                    />
                  </div>
                  <div>
                    <label className="block text-[10px] text-ink/70">Max Half-Life (d)</label>
                    <input
                      type="number"
                      step="1.0"
                      value={maxHalfLife}
                      onChange={(e) => setMaxHalfLife(e.target.value)}
                      className="mt-1 block w-full rounded-lg border border-ink/[0.12] bg-canvas px-2 py-1.5 text-xs text-ink"
                    />
                  </div>
                </div>
              </div>

              <button
                type="submit"
                disabled={running}
                className="mt-2 flex w-full items-center justify-center gap-2 rounded-xl bg-accent px-4 py-2.5 text-xs font-medium text-onaccent shadow-glow transition hover:opacity-90 disabled:opacity-50"
              >
                {running ? (
                  <>
                    <RotateCcw className="h-4 w-4 animate-spin" />
                    Running Signal Test…
                  </>
                ) : (
                  <>
                    <Play className="h-4 w-4" />
                    Run Signal Evaluation
                  </>
                )}
              </button>

              <div className="border-t border-ink/[0.06] pt-3">
                <p className="font-mono text-[10px] uppercase tracking-wider text-ink/50">Save Signal to Library (M12)</p>
                <div className="mt-2 space-y-2">
                  <input
                    type="text"
                    placeholder="Signal Name (e.g. momentum_v1)"
                    value={signalSaveName}
                    onChange={(e) => setSignalSaveName(e.target.value)}
                    className="block w-full rounded-lg border border-ink/[0.12] bg-canvas px-2.5 py-1.5 text-xs text-ink"
                  />
                  <input
                    type="text"
                    placeholder="Description / Hypothesis"
                    value={signalSaveDesc}
                    onChange={(e) => setSignalSaveDesc(e.target.value)}
                    className="block w-full rounded-lg border border-ink/[0.12] bg-canvas px-2.5 py-1.5 text-xs text-ink"
                  />
                  <button
                    type="button"
                    onClick={handleSaveToLibrary}
                    disabled={savingSignal || !signalSaveName.trim()}
                    className="flex w-full items-center justify-center gap-1.5 rounded-lg border border-ink/[0.12] bg-canvas px-3 py-1.5 text-xs font-medium text-ink hover:bg-ink/[0.04] disabled:opacity-50"
                  >
                    <BookmarkPlus className="h-3.5 w-3.5 text-accent" />
                    {savingSignal ? 'Saving...' : 'Save New Version to Library'}
                  </button>
                  {saveSuccessMessage && (
                    <p className="text-[11px] text-profit font-medium">{saveSuccessMessage}</p>
                  )}
                </div>
              </div>
            </form>
          </div>

          {/* Test History */}
          <div className="rounded-2xl border border-ink/[0.08] bg-surface p-5 shadow-xs">
            <div className="flex items-center justify-between">
              <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
                <History className="h-4 w-4 text-accent" />
                Signal Tests
              </h3>
              <button
                type="button"
                onClick={() => loadTests()}
                className="text-xs text-accent hover:underline"
              >
                Refresh
              </button>
            </div>
            {tests.length === 0 ? (
              <p className="mt-3 text-xs text-ink/50">No signal tests recorded yet.</p>
            ) : (
              <div className="mt-3 divide-y divide-ink/[0.04]">
                {tests.map((t) => {
                  const active = t.id === selectedId;
                  return (
                    <button
                      key={t.id}
                      type="button"
                      onClick={() => setSelectedId(t.id)}
                      className={cn(
                        'flex w-full items-center justify-between py-2.5 text-left transition',
                        active ? 'text-accent' : 'text-ink hover:text-accent',
                      )}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-xs font-medium">{t.signal_name}</span>
                          <span
                            className={cn(
                              'inline-flex items-center rounded-md px-1.5 py-0.5 text-[10px] font-semibold uppercase',
                              t.passed_gate
                                ? 'bg-profit/10 text-profit border border-profit/30'
                                : 'bg-loss/10 text-loss border border-loss/30',
                            )}
                          >
                            {t.passed_gate ? 'Passed' : 'Failed'}
                          </span>
                        </div>
                        <p className="mt-0.5 font-mono text-[11px] text-ink/50">
                          Mean IC: {(t.mean_ic ?? 0).toFixed(4)} · t: {(t.ic_t_stat ?? 0).toFixed(2)}
                        </p>
                      </div>
                      <span className="font-mono text-[10px] text-ink/40">
                        {new Date(t.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* Right Main Panel: Detailed Metrics & Visualizations */}
        <div className="space-y-6 lg:col-span-8">
          {selectedTest ? (
            <>
              {/* Header card with export actions */}
              <div className="rounded-2xl border border-ink/[0.08] bg-surface p-6 shadow-xs">
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
                  <div>
                    <div className="flex items-center gap-2.5">
                      <h2 className="text-xl font-bold text-ink">{selectedTest.signal_name}</h2>
                      <span
                        className={cn(
                          'inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold',
                          selectedTest.passed_gate
                            ? 'border border-profit/30 bg-profit/10 text-profit'
                            : 'border border-loss/30 bg-loss/10 text-loss',
                        )}
                      >
                        {selectedTest.passed_gate ? (
                          <>
                            <ShieldCheck className="h-3.5 w-3.5" /> Gate Passed
                          </>
                        ) : (
                          <>
                            <ShieldAlert className="h-3.5 w-3.5" /> Gate Failed
                          </>
                        )}
                      </span>
                    </div>
                    <p className="mt-1 font-mono text-xs text-ink/50">
                      ID: {selectedTest.id} · Evaluated {new Date(selectedTest.created_at).toLocaleString()}
                    </p>
                  </div>

                  {/* Export Buttons */}
                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      disabled={exporting !== null}
                      onClick={() => handleExport('pdf')}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-accent/30 bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent transition hover:bg-accent/20 disabled:opacity-50"
                    >
                      <Download className="h-3.5 w-3.5" />
                      {exporting === 'pdf' ? 'Exporting…' : 'PDF Report'}
                    </button>
                    <button
                      type="button"
                      disabled={exporting !== null}
                      onClick={() => handleExport('json')}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-ink/[0.12] bg-canvas px-3 py-1.5 text-xs font-medium text-ink transition hover:border-ink/20 disabled:opacity-50"
                    >
                      <FileCode2 className="h-3.5 w-3.5" />
                      JSON
                    </button>
                    <button
                      type="button"
                      disabled={exporting !== null}
                      onClick={() => handleExport('csv')}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-ink/[0.12] bg-canvas px-3 py-1.5 text-xs font-medium text-ink transition hover:border-ink/20 disabled:opacity-50"
                    >
                      <FileSpreadsheet className="h-3.5 w-3.5" />
                      CSV
                    </button>
                  </div>
                </div>

                {/* Overfitting Guard Display */}
                <div className="mt-4 border-t border-ink/[0.06] pt-4">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium text-ink">
                      Overfitting Guard: Tests run on this dataset
                    </span>
                    <span
                      className={cn(
                        'num font-mono text-xs font-semibold px-2 py-0.5 rounded-md',
                        selectedTest.tests_run_count > 5
                          ? 'bg-amber-500/10 text-amber-500 border border-amber-500/30'
                          : 'bg-ink/[0.05] text-ink/80',
                      )}
                    >
                      {selectedTest.tests_run_count} {selectedTest.tests_run_count === 1 ? 'test' : 'tests'}
                    </span>
                  </div>
                  {selectedTest.tests_run_count > 5 && (
                    <div className="mt-2.5 flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-600 dark:text-amber-400">
                      <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
                      <div>
                        <p className="font-semibold">Elevated Test Count Notice</p>
                        <p className="mt-0.5 opacity-90">
                          {selectedTest.tests_run_count} signal evaluations have been run against this dataset version. High iteration counts inflate false discovery rates and rank correlation significance. Consider holdout validation before deployment.
                        </p>
                      </div>
                    </div>
                  )}
                </div>
              </div>

              {/* KPI Summary Grid */}
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4">
                  <p className="text-[11px] uppercase tracking-wider text-ink/50">Mean IC ({selectedTest.primary_horizon}d)</p>
                  <p className={cn('num mt-1 text-2xl font-bold', selectedTest.mean_ic >= 0 ? 'text-profit' : 'text-loss')}>
                    {selectedTest.mean_ic.toFixed(4)}
                  </p>
                  <p className="mt-0.5 text-[10px] text-ink/40">Spearman rank corr</p>
                </div>

                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4">
                  <p className="text-[11px] uppercase tracking-wider text-ink/50">t-Statistic</p>
                  <p className={cn('num mt-1 text-2xl font-bold', Math.abs(selectedTest.ic_t_stat) >= 2 ? 'text-ink' : 'text-ink/60')}>
                    {selectedTest.ic_t_stat.toFixed(2)}
                  </p>
                  <p className="mt-0.5 text-[10px] text-ink/40">Stdev: {selectedTest.ic_stdev.toFixed(4)}</p>
                </div>

                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4">
                  <p className="text-[11px] uppercase tracking-wider text-ink/50">Decay Half-Life</p>
                  <p className="num mt-1 text-2xl font-bold text-accent">
                    {selectedTest.half_life_days.toFixed(1)} <span className="text-sm font-normal text-ink/60">days</span>
                  </p>
                  <p className="mt-0.5 text-[10px] text-ink/40">Exponential fit (t₁/₂)</p>
                </div>

                <div className="rounded-2xl border border-ink/[0.08] bg-surface p-4">
                  <p className="text-[11px] uppercase tracking-wider text-ink/50">Q{quantileCount} - Q1 Spread</p>
                  <p className={cn('num mt-1 text-2xl font-bold', selectedTest.top_bottom_spread >= 0 ? 'text-profit' : 'text-loss')}>
                    {(selectedTest.top_bottom_spread * 10_000).toFixed(0)} <span className="text-xs font-normal text-ink/60">bps</span>
                  </p>
                  <p className="mt-0.5 text-[10px] text-ink/40">Turnover: {(selectedTest.top_quantile_turnover * 100).toFixed(1)}%</p>
                </div>
              </div>

              {/* Multi-Horizon Decay Section */}
              <div className="rounded-2xl border border-ink/[0.08] bg-surface p-6 shadow-xs">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
                      <TrendingUp className="h-4 w-4 text-accent" />
                      Multi-Horizon Decay Curve
                    </h3>
                    <p className="text-xs text-ink/50">
                      Predictive power decay over 1, 5, 10, and 20 trading days: IC(h) ≈ IC₀ · 2^(-h / t₁/₂)
                    </p>
                  </div>
                </div>

                {horizonIcs.length > 0 && (
                  <div className="mt-6 h-60 w-full">
                    <ResponsiveContainer width="100%" height="100%">
                      <LineChart data={horizonIcs} margin={{ top: 10, right: 20, left: 0, bottom: 5 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke={colors.grid} />
                        <XAxis dataKey="horizon" unit="d" stroke={colors.axis} tick={{ fill: colors.tick, fontSize: 11 }} />
                        <YAxis stroke={colors.axis} tick={{ fill: colors.tick, fontSize: 11 }} domain={['auto', 'auto']} />
                        <Tooltip
                          contentStyle={colors.tooltip}
                          formatter={(val: unknown) => [typeof val === 'number' ? val.toFixed(4) : String(val), 'Mean IC']}
                          labelFormatter={(label) => `Horizon: ${label} Days`}
                        />
                        <Line
                          type="monotone"
                          dataKey="mean_ic"
                          name="Observed Mean IC"
                          stroke={colors.accent}
                          strokeWidth={2.5}
                          dot={{ fill: colors.accent, r: 4 }}
                          activeDot={{ r: 6 }}
                        />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                )}

                {/* Horizon IC Table */}
                <div className="mt-4 overflow-x-auto">
                  <table className="w-full text-left text-xs">
                    <thead>
                      <tr className="border-b border-ink/[0.06] text-[11px] uppercase tracking-wider text-ink/50">
                        <th className="py-2">Horizon</th>
                        <th className="py-2">Mean IC</th>
                        <th className="py-2">IC Stdev</th>
                        <th className="py-2">t-Statistic</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-ink/[0.04] font-mono">
                      {horizonIcs.map((h) => (
                        <tr key={h.horizon}>
                          <td className="py-2 text-ink font-semibold">{h.horizon} Day{h.horizon > 1 ? 's' : ''}</td>
                          <td className={cn('py-2', h.mean_ic >= 0 ? 'text-profit' : 'text-loss')}>
                            {h.mean_ic.toFixed(4)}
                          </td>
                          <td className="py-2 text-ink/70">{h.ic_stdev.toFixed(4)}</td>
                          <td className="py-2 text-ink/70">{h.ic_t_stat.toFixed(2)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>

              {/* Quantile Performance & Turnover */}
              <div className="rounded-2xl border border-ink/[0.08] bg-surface p-6 shadow-xs">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
                      <Layers className="h-4 w-4 text-accent" />
                      Quantile Spread & Monotonicity
                    </h3>
                    <p className="text-xs text-ink/50">
                      Forward return sorting across quantiles (Q1 lowest score to Q{quantileCount} highest score).
                    </p>
                  </div>
                </div>

                {quantileReturns.length > 0 && (
                  <div className="mt-6 h-52 w-full">
                    <ResponsiveContainer width="100%" height="100%">
                      <BarChart data={quantileReturns} margin={{ top: 10, right: 20, left: 0, bottom: 5 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke={colors.grid} />
                        <XAxis dataKey="quantile" tickFormatter={(q) => `Q${q}`} stroke={colors.axis} tick={{ fill: colors.tick, fontSize: 11 }} />
                        <YAxis stroke={colors.axis} tick={{ fill: colors.tick, fontSize: 11 }} tickFormatter={(v) => `${(v * 100).toFixed(1)}%`} />
                        <Tooltip
                          contentStyle={colors.tooltip}
                          formatter={(v: unknown) => [
                            typeof v === 'number' ? `${(v * 10_000).toFixed(1)} bps (${(v * 100).toFixed(2)}%)` : String(v),
                            'Mean Return',
                          ]}
                          labelFormatter={(label) => `Quantile Q${label}`}
                        />
                        <Bar dataKey="mean_forward_return" radius={[4, 4, 0, 0]}>
                          {quantileReturns.map((entry, index) => (
                            <Cell
                              key={`cell-${index}`}
                              fill={entry.mean_forward_return >= 0 ? colors.accent : colors.loss}
                            />
                          ))}
                        </Bar>
                      </BarChart>
                    </ResponsiveContainer>
                  </div>
                )}

                <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <div className="rounded-xl border border-ink/[0.06] bg-ink/[0.02] p-3 text-xs">
                    <span className="text-ink/50">Top-Minus-Bottom Spread</span>
                    <p className="num mt-1 font-mono text-base font-bold text-ink">
                      {(selectedTest.top_bottom_spread * 10_000).toFixed(1)} bps
                    </p>
                    <p className="mt-0.5 text-[11px] text-ink/40">Spread between top and bottom quintile forward returns</p>
                  </div>
                  <div className="rounded-xl border border-ink/[0.06] bg-ink/[0.02] p-3 text-xs">
                    <span className="text-ink/50">Top Quantile Turnover</span>
                    <p className="num mt-1 font-mono text-base font-bold text-ink">
                      {(selectedTest.top_quantile_turnover * 100).toFixed(1)}%
                    </p>
                    <p className="mt-0.5 text-[11px] text-ink/40">Fraction of constituents replaced each consecutive rebalance date</p>
                  </div>
                </div>
              </div>

              {/* Versioned Reports Panel (M16 Integration) */}
              <SignalTestReportPanel signalTestId={selectedTest.id} refreshKey={reportsRefresh} />
            </>
          ) : (
            <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-ink/10 p-12 text-center">
              <FlaskConical className="h-10 w-10 text-ink/30" />
              <h3 className="mt-4 text-sm font-semibold text-ink">No signal test selected</h3>
              <p className="mt-1 max-w-sm text-xs text-ink/50">
                Choose an existing test from the sidebar or run a new signal evaluation using the form.
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
