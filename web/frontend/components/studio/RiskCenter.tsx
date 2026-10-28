'use client';

import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';
import { RiskSourceNote } from './RiskSourceNote';
import { useState, useEffect, useCallback, useId } from 'react';
import {
  ShieldAlert,
  ShieldCheck,
  Power,
  RotateCcw,
  AlertTriangle,
  CheckCircle2,
  Sliders,
  Play,
  Layers,
  History,
  Activity,
  ArrowRight,
  TrendingDown,
  Info,
} from 'lucide-react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from 'recharts';
import {
  getRiskLimits,
  upsertRiskLimits,
  getKillSwitches,
  setKillSwitch,
  preTradeCheck,
  listAuditDecisions,
  getRiskMetrics,
  getPositions,
  getExposureHistory,
  getRiskAlerts,
  replayDecision,
  type RiskLimits,
  type KillSwitch,
  type RiskDecision,
  type RiskMetricsSummary,
  type PreTradeResult,
  type PortfolioPositionView,
  type EquitySnapshotView,
  type RiskAlertView,
  type ReplayDecisionView,
} from '@/lib/riskCenter';
import { cn, formatCurrency } from '@/lib/utils';

export function RiskCenter() {
  const [activeTab, setActiveTab] = useState<'overview' | 'simulator' | 'limits' | 'audit'>('overview');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Live Metrics & Status
  const [metrics, setMetrics] = useState<RiskMetricsSummary | null>(null);
  const [killSwitches, setKillSwitches] = useState<KillSwitch[]>([]);
  const [limits, setLimits] = useState<RiskLimits | null>(null);
  const [auditLogs, setAuditLogs] = useState<RiskDecision[]>([]);
  const [alerts, setAlerts] = useState<RiskAlertView[]>([]);
  const [exposureHistory, setExposureHistory] = useState<EquitySnapshotView[]>([]);
  const [positions, setPositions] = useState<PortfolioPositionView[]>([]);
  const [replayingIds, setReplayingIds] = useState<Record<string, boolean>>({});
  const [replayResults, setReplayResults] = useState<Record<string, ReplayDecisionView>>({});

  // Kill Switch Action Modal / Prompt
  const [killReason, setKillReason] = useState('Emergency risk halt triggered by operator.');
  const [killSubmitting, setKillSubmitting] = useState(false);

  // Limits Form State
  const [limitsForm, setLimitsForm] = useState({
    max_gross_exposure: 1_000_000,
    max_net_exposure: 500_000,
    max_position_size: 100_000,
    max_concentration_pct: 0.25,
    max_leverage: 3.0,
    max_drawdown_pct: 0.15,
    max_daily_loss: 25_000,
    max_var_pct: 0.05,
    size_reduction_enabled: true,
  });

  // Simulator Form State
  const [simSymbol, setSimSymbol] = useState('AAPL');
  const [simSide, setSimSide] = useState<'buy' | 'sell'>('buy');
  const [simQty, setSimQty] = useState(100);
  const [simPrice, setSimPrice] = useState(150);
  const [simCash, setSimCash] = useState(100_000);
  const [simEquity, setSimEquity] = useState(100_000);
  const [simResult, setSimResult] = useState<PreTradeResult | null>(null);
  const [simEvaluating, setSimEvaluating] = useState(false);

  // Unique Form IDs for Accessibility
  const reasonInputId = useId();
  const grossExposureId = useId();
  const netExposureId = useId();
  const positionSizeId = useId();
  const concentrationId = useId();
  const leverageId = useId();
  const drawdownId = useId();
  const dailyLossId = useId();
  const reductionToggleId = useId();
  const simSymbolId = useId();
  const simSideId = useId();
  const simQtyId = useId();
  const simPriceId = useId();
  const simCashId = useId();
  const simEquityId = useId();

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [m, ks, lims, audit, alts, expo, pos] = await Promise.all([
        getRiskMetrics(),
        getKillSwitches(),
        getRiskLimits(),
        listAuditDecisions(50),
        getRiskAlerts().catch(() => []),
        getExposureHistory(undefined, 50).catch(() => []),
        getPositions().catch(() => []),
      ]);
      setMetrics(m);
      setKillSwitches(ks);
      setLimits(lims);
      setLimitsForm({
        max_gross_exposure: lims.max_gross_exposure,
        max_net_exposure: lims.max_net_exposure,
        max_position_size: lims.max_position_size,
        max_concentration_pct: lims.max_concentration_pct,
        max_leverage: lims.max_leverage,
        max_drawdown_pct: lims.max_drawdown_pct,
        max_daily_loss: lims.max_daily_loss,
        max_var_pct: lims.max_var_pct,
        size_reduction_enabled: lims.size_reduction_enabled,
      });
      setAuditLogs(audit);
      setAlerts(alts);
      setExposureHistory(expo);
      setPositions(pos);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load risk center data.');
    } finally {
      setLoading(false);
    }
  }, []);

  const handleReplayDecision = async (decisionId: string) => {
    setReplayingIds((prev) => ({ ...prev, [decisionId]: true }));
    try {
      const res = await replayDecision(decisionId);
      setReplayResults((prev) => ({ ...prev, [decisionId]: res }));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Replay decision failed');
    } finally {
      setReplayingIds((prev) => ({ ...prev, [decisionId]: false }));
    }
  };

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleToggleKillSwitch = async (activate: boolean) => {
    setKillSubmitting(true);
    setError(null);
    setSuccess(null);
    try {
      await setKillSwitch({
        strategy_id: null,
        active: activate,
        reason: killReason,
      });
      setSuccess(
        activate
          ? 'Emergency kill switch ACTIVATED: all trading halted for workspace.'
          : 'Emergency kill switch CLEARED: trading resumed.'
      );
      await loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to update kill switch state.');
    } finally {
      setKillSubmitting(false);
    }
  };

  const handleSaveLimits = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);
    setSuccess(null);
    try {
      const updated = await upsertRiskLimits({
        strategy_id: null,
        ...limitsForm,
      });
      setLimits(updated);
      setSuccess('Risk limits updated successfully.');
      await loadData();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save risk limits.');
    } finally {
      setLoading(false);
    }
  };

  const handleRunSimulation = async (e: React.FormEvent) => {
    e.preventDefault();
    setSimEvaluating(true);
    setError(null);
    try {
      const res = await preTradeCheck({
        symbol: simSymbol,
        side: simSide,
        qty: Number(simQty),
        price: Number(simPrice),
        portfolio: {
          cash: Number(simCash),
          equity: Number(simEquity),
          peak_equity: Number(simEquity),
          daily_start_equity: Number(simEquity),
          positions: {},
        },
      });
      setSimResult(res);
      // Refresh audit logs
      const updatedAudit = await listAuditDecisions(50);
      setAuditLogs(updatedAudit);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Pre-trade check failed.');
    } finally {
      setSimEvaluating(false);
    }
  };

  const isHalted = metrics?.kill_switch_active ?? false;

  return (
    <div className="space-y-6">
      {/* Top Header & Emergency Kill Switch Bar */}
      <div className={cn(
        'rounded-2xl border p-6 transition shadow-sm',
        isHalted
          ? 'border-rose-500/30 bg-rose-500/10 dark:border-rose-500/20 dark:bg-rose-950/20'
          : 'border-emerald-500/20 bg-emerald-500/5 dark:border-emerald-500/10 dark:bg-emerald-950/10'
      )}>
        <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-3">
            <div className={cn(
              'flex h-12 w-12 items-center justify-center rounded-xl font-bold shadow-sm',
              isHalted ? 'bg-rose-500 text-white animate-pulse' : 'bg-emerald-500 text-white'
            )}>
              {isHalted ? <ShieldAlert className="h-6 w-6" /> : <ShieldCheck className="h-6 w-6" />}
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h1 className="text-xl font-bold tracking-tight text-ink">Risk Center & Pre-Trade Enforcement</h1>
                <span className={cn(
                  'rounded-full px-2.5 py-0.5 text-xs font-semibold uppercase tracking-wider',
                  isHalted ? 'bg-rose-500/20 text-rose-600 dark:text-rose-400' : 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400'
                )}>
                  {isHalted ? 'Trading Halted' : 'Normal Operations'}
                </span>
              </div>
              <p className="text-sm text-ink/60">
                {isHalted
                  ? `Active kill switch halt: ${metrics?.kill_switch_reason || 'Trading halted'}`
                  : 'Pre-trade checks, hard risk limits, leverage monitors, and decision audit trail.'}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={loadData}
              disabled={loading}
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-surface px-3 py-2 text-xs font-medium text-ink/70 hover:bg-ink/5"
            >
              <RotateCcw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} />
              Refresh
            </button>

            {isHalted ? (
              <button
                type="button"
                onClick={() => handleToggleKillSwitch(false)}
                disabled={killSubmitting}
                className="inline-flex items-center gap-2 rounded-xl bg-emerald-600 px-4 py-2.5 text-sm font-semibold text-white shadow hover:bg-emerald-500 disabled:opacity-50"
              >
                <Power className="h-4 w-4" />
                Clear Kill Switch
              </button>
            ) : (
              <button
                type="button"
                onClick={() => handleToggleKillSwitch(true)}
                disabled={killSubmitting}
                className="inline-flex items-center gap-2 rounded-xl bg-rose-600 px-4 py-2.5 text-sm font-semibold text-white shadow hover:bg-rose-500 disabled:opacity-50"
              >
                <Power className="h-4 w-4" />
                Emergency Halt (Kill Switch)
              </button>
            )}
          </div>
        </div>

        {/* Reason field when emergency halt action is prepared */}
        <div className="mt-4 flex flex-col gap-2 pt-3 border-t border-ink/10 sm:flex-row sm:items-center">
          <label htmlFor={reasonInputId} className="text-xs font-medium text-ink/60 shrink-0">
            Halt / Clear Reason:
          </label>
          <input
            id={reasonInputId}
            type="text"
            value={killReason}
            onChange={(e) => setKillReason(e.target.value)}
            className="flex-1 rounded-lg border border-ink/10 bg-surface px-3 py-1.5 text-xs text-ink placeholder:text-ink/40 focus:border-accent focus:outline-none"
            placeholder="Audit explanation for kill switch toggle..."
          />
        </div>
      </div>

      {/* Educational Explainer: What is the Kill Switch? */}
      <div className="rounded-xl border border-ink/[0.08] bg-canvas/40 p-4 text-xs text-ink/75">
        <div className="flex items-center gap-2 font-semibold text-ink">
          <ShieldAlert className="h-4 w-4 text-rose-500" />
          <span>What is the Kill Switch?</span>
        </div>
        <p className="mt-1 leading-relaxed">
          The <strong>Kill Switch</strong> is an emergency risk circuit-breaker for quantitative trading systems. When activated (either manually by an operator or automatically when hard loss limits or VaR limits are breached), the engine <strong>instantly halts all automated order routing</strong>, rejects any new buy/sell orders at the pre-trade check stage, and freezes active executions. Clearing the kill switch requires entering an explicit audit reason to preserve institutional compliance and full audit trail history.
        </p>
      </div>

      {error && (
        <div className="rounded-xl border border-rose-500/20 bg-rose-500/5 p-4 text-sm text-rose-600 dark:text-rose-400">
          <div className="flex items-center gap-2">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            <span>{error}</span>
          </div>
        </div>
      )}

      {success && (
        <div className="rounded-xl border border-emerald-500/20 bg-emerald-500/5 p-4 text-sm text-emerald-600 dark:text-emerald-400">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="h-4 w-4 shrink-0" />
            <span>{success}</span>
          </div>
        </div>
      )}

      {/* Navigation Tabs */}
      <div className="flex border-b border-ink/10">
        <button
          type="button"
          onClick={() => setActiveTab('overview')}
          className={cn(
            'flex items-center gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition',
            activeTab === 'overview'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          )}
        >
          <Activity className="h-4 w-4" />
          Live Metrics & Gauges
        </button>
        <button
          type="button"
          onClick={() => setActiveTab('simulator')}
          className={cn(
            'flex items-center gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition',
            activeTab === 'simulator'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          )}
        >
          <Play className="h-4 w-4" />
          Pre-Trade Simulator
        </button>
        <button
          type="button"
          onClick={() => setActiveTab('limits')}
          className={cn(
            'flex items-center gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition',
            activeTab === 'limits'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          )}
        >
          <Sliders className="h-4 w-4" />
          Limit Configuration
        </button>
        <button
          type="button"
          onClick={() => setActiveTab('audit')}
          className={cn(
            'flex items-center gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition',
            activeTab === 'audit'
              ? 'border-accent text-accent'
              : 'border-transparent text-ink/60 hover:text-ink'
          )}
        >
          <History className="h-4 w-4" />
          Audit Trail ({auditLogs.length})
        </button>
      </div>

      {/* Tab 1: Live Metrics Gauges */}
      {activeTab === 'overview' && (
        <div className="space-y-6">
          {/* Active Alerts Banner (Task 11) */}
          {alerts.length > 0 && (
            <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 dark:border-amber-500/20 dark:bg-amber-950/20">
              <div className="flex items-center gap-2 font-semibold text-amber-600 dark:text-amber-400">
                <AlertTriangle className="h-5 w-5 shrink-0" />
                <span>Active Risk Warnings & Threshold Alerts ({alerts.length})</span>
              </div>
              <div className="mt-3 grid gap-2.5 sm:grid-cols-2">
                {alerts.map((a, idx) => (
                  <div
                    key={idx}
                    className="flex items-start justify-between rounded-lg bg-surface/80 p-3 text-xs border border-ink/5 shadow-xs"
                  >
                    <div>
                      <span className="font-semibold text-ink uppercase tracking-wider">{a.metric}</span>
                      <p className="text-ink/70 mt-0.5">{a.message}</p>
                    </div>
                    <span
                      className={cn(
                        'ml-2 shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px] font-bold uppercase',
                        a.severity === 'breach'
                          ? 'bg-rose-500/20 text-rose-600 dark:text-rose-400'
                          : 'bg-amber-500/20 text-amber-600 dark:text-amber-400'
                      )}
                    >
                      {a.utilization_pct.toFixed(0)}% limit
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          <RiskSourceNote grossExposure={metrics?.gross_exposure} />

          {/* Metric Gauges Grid */}
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {/* Gross Exposure Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="gross_exposure_utilization">Gross Exposure</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.gross_exposure_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {formatCurrency(metrics?.gross_exposure ?? 0)}
                </span>
                <InfoTip metric="gross_exposure" label="Gross exposure" className="self-center" />
                <span className="text-xs font-mono text-ink/40">
                  / {formatCurrency(metrics?.gross_exposure_limit ?? 1_000_000)}
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.gross_exposure_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.gross_exposure_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.gross_exposure_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>

            {/* Net Exposure Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="net_exposure_utilization">Net Exposure</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.net_exposure_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {formatCurrency(metrics?.net_exposure ?? 0)}
                </span>
                <InfoTip metric="net_exposure" label="Net exposure" className="self-center" />
                <span className="text-xs font-mono text-ink/40">
                  / {formatCurrency(metrics?.net_exposure_limit ?? 500_000)}
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.net_exposure_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.net_exposure_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.net_exposure_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>

            {/* Leverage Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="leverage_utilization">Leverage</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.leverage_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {metrics?.leverage?.toFixed(2) ?? '0.00'}x
                </span>
                <span className="text-xs font-mono text-ink/40">
                  / {metrics?.leverage_limit?.toFixed(2) ?? '3.00'}x max
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.leverage_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.leverage_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.leverage_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>

            {/* Drawdown Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="drawdown_utilization">Drawdown</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.drawdown_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {((metrics?.drawdown_pct ?? 0) * 100).toFixed(2)}%
                </span>
                <span className="text-xs font-mono text-ink/40">
                  / {((metrics?.drawdown_limit_pct ?? 0.15) * 100).toFixed(1)}% limit
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.drawdown_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.drawdown_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.drawdown_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>

            {/* Daily Loss Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="daily_loss_utilization">Daily Loss</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.daily_loss_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {formatCurrency(metrics?.daily_loss ?? 0)}
                </span>
                <span className="text-xs font-mono text-ink/40">
                  / {formatCurrency(metrics?.daily_loss_limit ?? 25_000)}
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.daily_loss_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.daily_loss_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.daily_loss_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>

            {/* VaR Gauge */}
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <div className="flex items-center justify-between text-xs font-semibold text-ink/60 uppercase tracking-wider">
                <span><MetricLabel metric="var_utilization">Value at Risk (99% 1-Day)</MetricLabel></span>
                <span className="font-mono text-ink/90">
                  {metrics?.var_utilization_pct?.toFixed(1) ?? '0.0'}%
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-mono text-ink">
                  {((metrics?.var_pct ?? 0) * 100).toFixed(2)}%
                </span>
                <span className="text-xs font-mono text-ink/40">
                  / {((metrics?.var_limit_pct ?? 0.05) * 100).toFixed(1)}% limit
                </span>
              </div>
              <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full transition-all',
                    (metrics?.var_utilization_pct ?? 0) > 90
                      ? 'bg-rose-500'
                      : (metrics?.var_utilization_pct ?? 0) > 70
                      ? 'bg-amber-500'
                      : 'bg-emerald-500'
                  )}
                  style={{ width: `${Math.min(100, Math.max(0, metrics?.var_utilization_pct ?? 0))}%` }}
                />
              </div>
            </div>
          </div>

          {/* Exposure & Equity History Chart (Task 10) */}
          <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-sm font-semibold text-ink">Portfolio Exposure & Equity History</h3>
                <p className="text-xs text-ink/60">Time series of recorded portfolio equity & cash levels</p>
              </div>
              <span className="text-xs font-mono text-ink/50">{exposureHistory.length} snapshots</span>
            </div>
            <div className="mt-4 h-64 w-full">
              {exposureHistory.length > 0 ? (
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart
                    data={exposureHistory.map((s) => ({
                      time: new Date(s.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
                      equity: s.equity,
                      cash: s.cash,
                    }))}
                  >
                    <CartesianGrid strokeDasharray="3 3" opacity={0.15} />
                    <XAxis dataKey="time" tick={{ fontSize: 10, fill: '#64748B' }} />
                    <YAxis tick={{ fontSize: 10, fill: '#64748B' }} tickFormatter={(v) => `$${(v / 1000).toFixed(0)}k`} />
                    <Tooltip formatter={(value: unknown) => formatCurrency(Number(value))} />
                    <Legend wrapperStyle={{ fontSize: 11 }} />
                    <Line type="monotone" dataKey="equity" stroke="#10b981" strokeWidth={2} dot={false} name="Equity" />
                    <Line type="monotone" dataKey="cash" stroke="#6366f1" strokeWidth={1.5} dot={false} strokeDasharray="4 4" name="Cash" />
                  </LineChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-full items-center justify-center text-xs text-ink/40">
                  No equity history snapshots recorded yet.
                </div>
              )}
            </div>
          </div>

          {/* Active Positions Table (Task 1) */}
          {positions.length > 0 && (
            <div className="rounded-xl border border-ink/10 bg-surface p-5 shadow-sm">
              <h3 className="text-sm font-semibold text-ink">Active Server Portfolio Positions</h3>
              <div className="mt-3 overflow-x-auto">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-ink/10 bg-ink/[0.02] text-ink/60">
                    <tr>
                      <th className="px-3 py-2 font-medium">Symbol</th>
                      <th className="px-3 py-2 font-medium">Quantity</th>
                      <th className="px-3 py-2 font-medium">Mark Price</th>
                      <th className="px-3 py-2 font-medium"><MetricLabel metric="market_value">Market Value</MetricLabel></th>
                      <th className="px-3 py-2 font-medium">Last Updated</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-ink/10 font-mono">
                    {positions.map((p) => (
                      <tr key={p.symbol} className="hover:bg-ink/[0.02]">
                        <td className="px-3 py-2 font-bold text-ink">{p.symbol}</td>
                        <td className="px-3 py-2 text-ink">{p.qty.toFixed(4)}</td>
                        <td className="px-3 py-2 text-ink">${p.price.toFixed(2)}</td>
                        <td className="px-3 py-2 text-ink font-semibold">${(p.qty * p.price).toFixed(2)}</td>
                        <td className="px-3 py-2 text-ink/50 font-sans">{new Date(p.updated_at).toLocaleString()}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Tab 2: Pre-Trade Simulator */}
      {activeTab === 'simulator' && (
        <div className="grid gap-6 lg:grid-cols-2">
          {/* Simulator Form */}
          <div className="rounded-2xl border border-ink/10 bg-surface p-6 shadow-sm">
            <h2 className="text-base font-semibold text-ink">Hypothetical Order Evaluation</h2>
            <p className="mt-1 text-xs text-ink/60">
              Simulate candidate orders against current portfolio state, limits, and kill switches.
            </p>

            <form onSubmit={handleRunSimulation} className="mt-5 space-y-4">
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label htmlFor={simSymbolId} className="text-xs font-medium text-ink/70">Symbol</label>
                  <input
                    id={simSymbolId}
                    type="text"
                    value={simSymbol}
                    onChange={(e) => setSimSymbol(e.target.value.toUpperCase())}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm uppercase text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
                <div>
                  <label htmlFor={simSideId} className="text-xs font-medium text-ink/70">Side</label>
                  <select
                    id={simSideId}
                    value={simSide}
                    onChange={(e) => setSimSide(e.target.value as 'buy' | 'sell')}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 text-sm text-ink focus:border-accent focus:outline-none"
                  >
                    <option value="buy">Buy (Long)</option>
                    <option value="sell">Sell (Short)</option>
                  </select>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label htmlFor={simQtyId} className="text-xs font-medium text-ink/70">Quantity (Shares)</label>
                  <input
                    id={simQtyId}
                    type="number"
                    step="any"
                    value={simQty}
                    onChange={(e) => setSimQty(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
                <div>
                  <label htmlFor={simPriceId} className="text-xs font-medium text-ink/70">Limit / Market Price ($)</label>
                  <input
                    id={simPriceId}
                    type="number"
                    step="any"
                    value={simPrice}
                    onChange={(e) => setSimPrice(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-4 pt-2 border-t border-ink/10">
                <div>
                  <label htmlFor={simCashId} className="text-xs font-medium text-ink/70">Hypothetical Cash ($)</label>
                  <input
                    id={simCashId}
                    type="number"
                    value={simCash}
                    onChange={(e) => setSimCash(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  />
                </div>
                <div>
                  <label htmlFor={simEquityId} className="text-xs font-medium text-ink/70">Portfolio Equity ($)</label>
                  <input
                    id={simEquityId}
                    type="number"
                    value={simEquity}
                    onChange={(e) => setSimEquity(Number(e.target.value))}
                    className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  />
                </div>
              </div>

              <button
                type="submit"
                disabled={simEvaluating}
                className="mt-2 w-full inline-flex items-center justify-center gap-2 rounded-xl bg-accent px-4 py-2.5 text-sm font-semibold text-onaccent shadow transition hover:opacity-90 disabled:opacity-50"
              >
                <Play className="h-4 w-4" />
                {simEvaluating ? 'Evaluating Order...' : 'Run Pre-Trade Risk Check'}
              </button>
            </form>
          </div>

          {/* Simulator Verdict & Breakdown */}
          <div className="rounded-2xl border border-ink/10 bg-surface p-6 shadow-sm">
            <h2 className="text-base font-semibold text-ink">Risk Engine Verdict</h2>
            <p className="mt-1 text-xs text-ink/60">Real-time enforcement decision and projected impacts.</p>

            {simResult ? (
              <div className="mt-5 space-y-4">
                <div
                  className={cn(
                    'rounded-xl border p-4 text-center',
                    simResult.verdict === 'approved'
                      ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
                      : simResult.verdict === 'reduced'
                      ? 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300'
                      : 'border-rose-500/30 bg-rose-500/10 text-rose-700 dark:text-rose-300'
                  )}
                >
                  <div className="text-lg font-bold uppercase tracking-wider">
                    {simResult.verdict}
                  </div>
                  <div className="mt-1 text-xs">
                    {simResult.verdict === 'approved' && 'Order fully passes all limits and active risk policies.'}
                    {simResult.verdict === 'reduced' &&
                      `Order reduced from ${simResult.requested_qty} to ${simResult.approved_qty} shares.`}
                    {simResult.verdict === 'rejected' && 'Order completely blocked due to limit breach or kill switch.'}
                  </div>
                </div>

                {simResult.breaches.length > 0 && (
                  <div className="space-y-2">
                    <span className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                      Detected Violations ({simResult.breaches.length})
                    </span>
                    <div className="space-y-1.5">
                      {simResult.breaches.map((b, idx) => (
                        <div
                          key={idx}
                          className="flex items-start gap-2 rounded-lg border border-rose-500/20 bg-rose-500/5 p-2.5 text-xs text-rose-600 dark:text-rose-400"
                        >
                          <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
                          <div>
                            <span className="font-mono font-bold uppercase">{b.code}</span>: {b.message}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                <div className="space-y-2 pt-2 border-t border-ink/10">
                  <span className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                    Projected Metrics
                  </span>
                  <div className="grid grid-cols-2 gap-3 text-xs">
                    <div className="rounded-lg bg-ink/[0.03] p-2.5">
                      <span className="text-ink/60">Projected Gross Exposure:</span>
                      <div className="font-mono font-bold text-ink">
                        {formatCurrency(simResult.projected_metrics.gross_exposure)}
                      </div>
                    </div>
                    <div className="rounded-lg bg-ink/[0.03] p-2.5">
                      <span className="text-ink/60">Projected Leverage:</span>
                      <div className="font-mono font-bold text-ink">
                        {simResult.projected_metrics.leverage.toFixed(2)}x
                      </div>
                    </div>
                    <div className="rounded-lg bg-ink/[0.03] p-2.5">
                      <span className="text-ink/60">Position Size:</span>
                      <div className="font-mono font-bold text-ink">
                        {formatCurrency(simResult.projected_metrics.position_size)}
                      </div>
                    </div>
                    <div className="rounded-lg bg-ink/[0.03] p-2.5">
                      <span className="text-ink/60">Concentration:</span>
                      <div className="font-mono font-bold text-ink">
                        {(simResult.projected_metrics.concentration_pct * 100).toFixed(1)}%
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="mt-12 text-center text-xs text-ink/40">
                Submit an order candidate to inspect simulated pre-trade checks and sizing verdict.
              </div>
            )}
          </div>
        </div>
      )}

      {/* Tab 3: Limits Configuration */}
      {activeTab === 'limits' && (
        <div className="rounded-2xl border border-ink/10 bg-surface p-6 shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-base font-semibold text-ink">Workspace Risk Limits</h2>
              <p className="mt-1 text-xs text-ink/60">
                Configure gross/net limits, max position size, max concentration, leverage and drawdowns.
              </p>
            </div>
            {limits?.updated_at && (
              <span className="text-xs text-ink/40">
                Last updated: {new Date(limits.updated_at).toLocaleString()}
              </span>
            )}
          </div>

          <form onSubmit={handleSaveLimits} className="mt-6 space-y-5">
            <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
              <div>
                <label htmlFor={grossExposureId} className="text-xs font-medium text-ink/70">
                  Max Gross Exposure ($)
                </label>
                <input
                  id={grossExposureId}
                  type="number"
                  value={limitsForm.max_gross_exposure}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_gross_exposure: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={netExposureId} className="text-xs font-medium text-ink/70">
                  Max Net Exposure ($)
                </label>
                <input
                  id={netExposureId}
                  type="number"
                  value={limitsForm.max_net_exposure}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_net_exposure: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={positionSizeId} className="text-xs font-medium text-ink/70">
                  Max Position Size ($)
                </label>
                <input
                  id={positionSizeId}
                  type="number"
                  value={limitsForm.max_position_size}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_position_size: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={concentrationId} className="text-xs font-medium text-ink/70">
                  Max Concentration (0.01 - 1.00)
                </label>
                <input
                  id={concentrationId}
                  type="number"
                  step="0.01"
                  value={limitsForm.max_concentration_pct}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_concentration_pct: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={leverageId} className="text-xs font-medium text-ink/70">
                  Max Leverage (x)
                </label>
                <input
                  id={leverageId}
                  type="number"
                  step="0.1"
                  value={limitsForm.max_leverage}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_leverage: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={drawdownId} className="text-xs font-medium text-ink/70">
                  Max Drawdown (0.01 - 1.00)
                </label>
                <input
                  id={drawdownId}
                  type="number"
                  step="0.01"
                  value={limitsForm.max_drawdown_pct}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_drawdown_pct: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>

              <div>
                <label htmlFor={dailyLossId} className="text-xs font-medium text-ink/70">
                  Max Daily Loss ($)
                </label>
                <input
                  id={dailyLossId}
                  type="number"
                  value={limitsForm.max_daily_loss}
                  onChange={(e) =>
                    setLimitsForm({ ...limitsForm, max_daily_loss: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-ink/10 bg-surface px-3 py-2 font-mono text-sm text-ink focus:border-accent focus:outline-none"
                  required
                />
              </div>
            </div>

            <div className="flex items-center gap-3 pt-3 border-t border-ink/10">
              <input
                id={reductionToggleId}
                type="checkbox"
                checked={limitsForm.size_reduction_enabled}
                onChange={(e) =>
                  setLimitsForm({ ...limitsForm, size_reduction_enabled: e.target.checked })
                }
                className="h-4 w-4 rounded border-ink/20 text-accent focus:ring-accent"
              />
              <label htmlFor={reductionToggleId} className="text-xs font-medium text-ink">
                Enable Automatic Size Reduction (if an order exceeds position size or leverage, downscale it to max permissible size instead of full rejection)
              </label>
            </div>

            <div className="pt-2">
              <button
                type="submit"
                disabled={loading}
                className="inline-flex items-center gap-2 rounded-xl bg-accent px-5 py-2.5 text-sm font-semibold text-onaccent shadow transition hover:opacity-90 disabled:opacity-50"
              >
                <Sliders className="h-4 w-4" />
                {loading ? 'Saving Limits...' : 'Save Risk Limits'}
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Tab 4: Decision Audit Trail */}
      {activeTab === 'audit' && (
        <div className="rounded-2xl border border-ink/10 bg-surface p-6 shadow-sm">
          <div className="flex items-center justify-between mb-4">
            <div>
              <h2 className="text-base font-semibold text-ink">Risk Decision Audit Trail</h2>
              <p className="mt-1 text-xs text-ink/60">
                Immutable ledger of all pre-trade evaluations, size reductions, rejections and overrides.
              </p>
            </div>
            <button
              type="button"
              onClick={loadData}
              disabled={loading}
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-surface px-3 py-1.5 text-xs font-medium text-ink/70 hover:bg-ink/5"
            >
              <RotateCcw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} />
              Refresh Log
            </button>
          </div>

          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-ink/10 bg-ink/[0.02] text-ink/60">
                <tr>
                  <th className="px-3 py-2.5 font-medium">Timestamp</th>
                  <th className="px-3 py-2.5 font-medium">Symbol</th>
                  <th className="px-3 py-2.5 font-medium">Side</th>
                  <th className="px-3 py-2.5 font-medium">Requested Qty</th>
                  <th className="px-3 py-2.5 font-medium">Approved Qty</th>
                  <th className="px-3 py-2.5 font-medium">Price</th>
                  <th className="px-3 py-2.5 font-medium">Verdict</th>
                  <th className="px-3 py-2.5 font-medium">Reasons</th>
                  <th className="px-3 py-2.5 font-medium">Deterministic Replay</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-ink/10 font-mono">
                {auditLogs.length > 0 ? (
                  auditLogs.map((log) => (
                    <tr key={log.id} className="hover:bg-ink/[0.02]">
                      <td className="px-3 py-2.5 text-ink/60">
                        {new Date(log.created_at).toLocaleTimeString()}
                      </td>
                      <td className="px-3 py-2.5 font-bold text-ink">{log.symbol}</td>
                      <td className="px-3 py-2.5 uppercase text-ink/80">{log.side}</td>
                      <td className="px-3 py-2.5 text-ink">{log.requested_qty}</td>
                      <td className="px-3 py-2.5 text-ink">{log.approved_qty}</td>
                      <td className="px-3 py-2.5 text-ink">${log.price.toFixed(2)}</td>
                      <td className="px-3 py-2.5">
                        <span
                          className={cn(
                            'rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider',
                            log.verdict === 'approved'
                              ? 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400'
                              : log.verdict === 'reduced'
                              ? 'bg-amber-500/20 text-amber-600 dark:text-amber-400'
                              : 'bg-rose-500/20 text-rose-600 dark:text-rose-400'
                          )}
                        >
                          {log.verdict}
                        </span>
                      </td>
                      <td className="px-3 py-2.5 font-sans text-xs text-ink/60 max-w-xs truncate">
                        {log.reasons?.length > 0 ? log.reasons.join('; ') : '—'}
                      </td>
                      <td className="px-3 py-2.5 font-sans">
                        {replayResults[log.id] ? (
                          <div className="flex items-center gap-1.5">
                            <span
                              className={cn(
                                'rounded px-2 py-0.5 text-[10px] font-semibold font-mono uppercase tracking-wide',
                                replayResults[log.id].matches
                                  ? 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400'
                                  : 'bg-rose-500/20 text-rose-600 dark:text-rose-400'
                              )}
                            >
                              {replayResults[log.id].matches ? '✓ Verified Match' : '⚠ Mismatch'}
                            </span>
                          </div>
                        ) : (
                          <button
                            type="button"
                            onClick={() => handleReplayDecision(log.id)}
                            disabled={replayingIds[log.id]}
                            className="inline-flex items-center gap-1 rounded bg-ink/5 px-2 py-1 text-[11px] font-medium text-ink hover:bg-ink/10 disabled:opacity-50 transition"
                          >
                            <RotateCcw className={cn('h-3 w-3', replayingIds[log.id] && 'animate-spin')} />
                            {replayingIds[log.id] ? 'Replaying...' : 'Replay'}
                          </button>
                        )}
                      </td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={9} className="px-3 py-8 text-center text-xs font-sans text-ink/40">
                      No pre-trade decisions logged yet.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
