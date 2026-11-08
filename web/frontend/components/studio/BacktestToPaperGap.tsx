'use client';

import React, { useCallback, useEffect, useState } from 'react';
import {
  AlertCircle,
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Clock,
  DollarSign,
  Layers,
  Mail,
  Play,
  RefreshCw,
  TrendingDown,
  TrendingUp,
} from 'lucide-react';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn, formatCurrency, formatNumber, formatPercent } from '@/lib/utils';
import type { paths } from '@/lib/generated/engine-api';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';

type GapReportDetail = paths['/api/v1/paper/gap/reports/{id}']['get']['responses']['200']['content']['application/json'];
type GapReportSummary = paths['/api/v1/paper/gap/reports']['get']['responses']['200']['content']['application/json'][number];

interface Props {
  strategyId?: string;
  runId?: string;
  readOnly?: boolean;
}

interface GapDigest {
  workspace_id: string;
  period_start: string;
  period_end: string;
  total_reports: number;
  total_warnings: number;
  total_breaches: number;
  max_tracking_error_pct: number;
  max_slippage_drift_bps: number;
  max_timing_latency_ms: number;
  warnings: Array<{
    report_id: string;
    strategy_id: string;
    code: string;
    severity: string;
    message: string;
    metric_name: string;
    threshold: number;
    actual_value: number;
    created_at: string;
  }>;
}

export function BacktestToPaperGap({ strategyId, runId, readOnly = false }: Props) {
  const [reports, setReports] = useState<GapReportSummary[]>([]);
  const [selectedReport, setSelectedReport] = useState<GapReportDetail | null>(null);
  const [digest, setDigest] = useState<GapDigest | null>(null);
  const [dispatchingDigest, setDispatchingDigest] = useState(false);
  const [digestDispatchResult, setDigestDispatchResult] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const effectiveStrategyId = strategyId || runId;

  const loadDigest = useCallback(async () => {
    try {
      const res = await fetch('/api/engine/api/v1/paper/gap/digest');
      if (res.ok) {
        const d = (await res.json()) as GapDigest;
        setDigest(d);
      }
    } catch {}
  }, []);

  const handleDispatchDigest = async () => {
    setDispatchingDigest(true);
    setDigestDispatchResult(null);
    try {
      const res = await fetch('/api/engine/api/v1/paper/gap/digest/dispatch', { method: 'POST' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      setDigestDispatchResult(`Delivered via M27 email: ${data.warning_count} warnings, ${data.breach_count} breaches`);
      loadDigest();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to dispatch gap digest');
    } finally {
      setDispatchingDigest(false);
    }
  };

  const loadDetail = useCallback(async (id: string) => {
    try {
      const res = await fetch(`/api/engine/api/v1/paper/gap/reports/${encodeURIComponent(id)}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as GapReportDetail;
      setSelectedReport(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load report detail');
    }
  }, []);

  const runAnalysis = useCallback(async () => {
    setAnalyzing(true);
    setError(null);
    try {
      const targetStrat = effectiveStrategyId || '00000000-0000-0000-0000-000000000000';
      const res = await fetch('/api/engine/api/v1/paper/gap/analyze', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          strategy_id: targetStrat,
          backtest_run_id: runId || (effectiveStrategyId ? effectiveStrategyId : undefined),
          max_slippage_drift_bps: 10.0,
          max_timing_latency_ms: 1000.0,
          max_tracking_error_pct: 3.0,
          min_fill_ratio: 0.95,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        throw new Error(body?.message || `HTTP ${res.status}`);
      }
      const data = (await res.json()) as GapReportDetail;
      setSelectedReport(data);
      // Reload reports list
      const url = effectiveStrategyId
        ? `/api/engine/api/v1/paper/gap/reports?strategy_id=${encodeURIComponent(effectiveStrategyId)}`
        : '/api/engine/api/v1/paper/gap/reports';
      const repRes = await fetch(url);
      if (repRes.ok) {
        const reps = (await repRes.json()) as GapReportSummary[];
        setReports(reps);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Analysis failed');
    } finally {
      setAnalyzing(false);
    }
  }, [effectiveStrategyId, runId]);

  const loadReports = useCallback(async () => {
    setLoading(true);
    setError(null);
    loadDigest();
    try {
      const url = effectiveStrategyId
        ? `/api/engine/api/v1/paper/gap/reports?strategy_id=${encodeURIComponent(effectiveStrategyId)}`
        : '/api/engine/api/v1/paper/gap/reports';
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as GapReportSummary[];
      setReports(data);
      if (data.length > 0) {
        setSelectedReport((prev) => {
          if (!prev) loadDetail(data[0].id);
          return prev;
        });
      } else if (readOnly) {
        // Auto-run analysis if readOnly and no reports exist yet
        runAnalysis();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load gap reports');
    } finally {
      setLoading(false);
    }
  }, [effectiveStrategyId, loadDetail, loadDigest, readOnly, runAnalysis]);

  useEffect(() => {
    loadReports();
  }, [loadReports]);

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="text-lg font-semibold text-ink">Backtest-to-Paper Gap Analysis</h2>
            {readOnly && (
              <Badge tone="neutral" className="text-[10px]">
                Read-Only
              </Badge>
            )}
          </div>
          <p className="text-xs text-ink/60">
            Compare trade fills, slippage, timing latency and P&L between paper execution and backtests.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={loadReports}
            disabled={loading || analyzing}
            className="btn-secondary flex items-center gap-1.5 px-3 py-1.5 text-xs"
          >
            <RefreshCw className={cn('h-3.5 w-3.5', (loading || analyzing) && 'animate-spin')} /> Refresh
          </button>
          {!readOnly && (
            <button
              type="button"
              onClick={runAnalysis}
              disabled={analyzing}
              className="btn-primary flex items-center gap-1.5 px-3 py-1.5 text-xs"
            >
              <Play className="h-3.5 w-3.5 fill-current" /> {analyzing ? 'Analyzing...' : 'Run Gap Comparison'}
            </button>
          )}
        </div>
      </div>

      {/* Daily Gap Warning Digest Card (M10) */}
      <GlassCard className="p-4 border-ink/10">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <AlertTriangle className={cn('h-4 w-4', (digest?.total_breaches ?? 0) > 0 ? 'text-loss animate-pulse' : (digest?.total_warnings ?? 0) > 0 ? 'text-amber-500' : 'text-emerald-500')} />
            <div>
              <h3 className="text-xs font-semibold text-ink">Daily Gap Warning Digest (Workspace 24h)</h3>
              <p className="text-[11px] text-ink/60">
                {digest
                  ? `${digest.total_reports} reports analyzed · ${digest.total_warnings} warnings · ${digest.total_breaches} breaches`
                  : 'Monitoring workspace backtest-to-paper execution drift'}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={handleDispatchDigest}
            disabled={dispatchingDigest}
            className="inline-flex items-center gap-1.5 rounded-lg border border-ink/15 bg-surface/80 px-2.5 py-1 text-xs font-medium text-ink shadow-xs hover:bg-surface disabled:opacity-50"
          >
            <Mail className="h-3.5 w-3.5" />
            <span>{digestDispatchResult ? digestDispatchResult : dispatchingDigest ? 'Delivering...' : 'Deliver Digest via M27 Email'}</span>
          </button>
        </div>

        {digest && (digest.total_warnings > 0 || digest.total_breaches > 0) && (
          <div className="mt-3 grid gap-2 sm:grid-cols-3">
            <div className="rounded-lg bg-surface/60 p-2.5 border border-ink/5 text-xs">
              <span className="text-ink/60">Max Slippage Drift</span>
              <p className="font-mono font-semibold text-ink">{digest.max_slippage_drift_bps.toFixed(1)} bps</p>
            </div>
            <div className="rounded-lg bg-surface/60 p-2.5 border border-ink/5 text-xs">
              <span className="text-ink/60">Max Timing Latency</span>
              <p className="font-mono font-semibold text-ink">{digest.max_timing_latency_ms.toFixed(1)} ms</p>
            </div>
            <div className="rounded-lg bg-surface/60 p-2.5 border border-ink/5 text-xs">
              <span className="text-ink/60">Max Tracking Error</span>
              <p className="font-mono font-semibold text-ink">{digest.max_tracking_error_pct.toFixed(2)}%</p>
            </div>
          </div>
        )}
      </GlassCard>

      {error && <ErrorNote>{error}</ErrorNote>}

      {selectedReport && (
        <div className="space-y-6">
          {/* Top Status & Key Metrics */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <GlassCard className="p-4">
              <div className="flex items-center justify-between text-xs text-ink/60">
                <span>Alert Status</span>
                {selectedReport.alert_status === 'normal' && (
                  <Badge tone="profit" className="text-[10px]">
                    <CheckCircle2 className="mr-1 h-3 w-3 inline" /> Normal
                  </Badge>
                )}
                {selectedReport.alert_status === 'warning' && (
                  <Badge tone="caution" className="text-[10px]">
                    <AlertTriangle className="mr-1 h-3 w-3 inline" /> Warning
                  </Badge>
                )}
                {selectedReport.alert_status === 'breach' && (
                  <Badge tone="loss" className="text-[10px]">
                    <AlertCircle className="mr-1 h-3 w-3 inline" /> Breach
                  </Badge>
                )}
              </div>
              <div className="mt-2 text-xl font-bold font-mono text-ink">
                {selectedReport.alert_status.toUpperCase()}
              </div>
              <p className="mt-1 text-[11px] text-ink/50"><MetricLabel metric="tracking_error">Tracking error</MetricLabel> {formatPercent(selectedReport.tracking_error_pct / 100)}</p>
            </GlassCard>

            <GlassCard className="p-4">
              <div className="flex items-center justify-between text-xs text-ink/60">
                <span className="inline-flex items-center gap-1">Total P&L Gap <InfoTip metric="pnl_gap" /></span>
                <DollarSign className="h-4 w-4 text-ink/40" />
              </div>
              <div className={cn('mt-2 text-xl font-bold font-mono', selectedReport.total_pnl_gap >= 0 ? 'text-profit' : 'text-loss')}>
                {formatCurrency(selectedReport.total_pnl_gap)}
              </div>
              <p className="mt-1 text-[11px] text-ink/50">Paper vs backtest P&L discrepancy</p>
            </GlassCard>

            <GlassCard className="p-4">
              <div className="flex items-center justify-between text-xs text-ink/60">
                <span className="inline-flex items-center gap-1">Mean Slippage Drift <InfoTip metric="slippage_drift" /></span>
                <TrendingDown className="h-4 w-4 text-ink/40" />
              </div>
              <div className="mt-2 text-xl font-bold font-mono text-ink">
                {selectedReport.mean_slippage_drift_bps.toFixed(2)} bps
              </div>
              <p className="mt-1 text-[11px] text-ink/50">Realized vs modeled spread</p>
            </GlassCard>

            <GlassCard className="p-4">
              <div className="flex items-center justify-between text-xs text-ink/60">
                <span className="inline-flex items-center gap-1">Mean Timing Latency <InfoTip metric="timing_latency" /></span>
                <Clock className="h-4 w-4 text-ink/40" />
              </div>
              <div className="mt-2 text-xl font-bold font-mono text-ink">
                {selectedReport.mean_timing_latency_ms.toFixed(1)} ms
              </div>
              <p className="mt-1 text-[11px] text-ink/50">Fill execution delay</p>
            </GlassCard>
          </div>

          {/* Judged Thresholds (Task 3) */}
          {(selectedReport.report as any)?.thresholds && (
            <div className="flex flex-wrap items-center gap-2 text-[11px] text-ink/60 bg-ink/[0.02] border border-ink/[0.06] rounded-xl px-3.5 py-2">
              <span className="font-semibold text-ink/80">Thresholds Judged Against:</span>
              <span>Slippage Drift ≤ {(selectedReport.report as any).thresholds.max_slippage_drift_bps} bps</span>
              <span className="text-ink/30">•</span>
              <span>Latency ≤ {(selectedReport.report as any).thresholds.max_timing_latency_ms} ms</span>
              <span className="text-ink/30">•</span>
              <span>Tracking Error ≤ {(selectedReport.report as any).thresholds.max_tracking_error_pct}%</span>
              <span className="text-ink/30">•</span>
              <span>Fill Ratio ≥ {(((selectedReport.report as any).thresholds.min_fill_ratio ?? 0.95) * 100).toFixed(0)}%</span>
            </div>
          )}

          {/* Gap Attribution Waterfall Breakdown */}
          <GlassCard className="p-5">
            <h3 className="text-sm font-semibold text-ink">Gap Attribution by Cause</h3>
            <p className="text-xs text-ink/60">Decomposition of dollar difference between paper execution and backtest simulation.</p>

            <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-4">
              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                <span className="inline-flex items-center gap-1 text-[11px] font-medium text-ink/60">Transaction Costs <InfoTip metric="fees" label="Transaction Costs" /></span>
                <div className="mt-1 text-base font-semibold font-mono text-ink">
                  {formatCurrency(selectedReport.cost_gap)}
                </div>
                <span className="text-[10px] text-ink/40">Fees & borrow delta</span>
              </div>

              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                <span className="inline-flex items-center gap-1 text-[11px] font-medium text-ink/60">Slippage & Spread <InfoTip metric="slippage" label="Slippage & Spread" /></span>
                <div className="mt-1 text-base font-semibold font-mono text-ink">
                  {formatCurrency(selectedReport.slippage_gap)}
                </div>
                <span className="text-[10px] text-ink/40">Realized market impact</span>
              </div>

              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                <span className="inline-flex items-center gap-1 text-[11px] font-medium text-ink/60">Execution Latency <InfoTip metric="timing_latency" label="Execution Latency" /></span>
                <div className="mt-1 text-base font-semibold font-mono text-ink">
                  {formatCurrency(selectedReport.timing_delay_gap)}
                </div>
                <span className="text-[10px] text-ink/40">Price drift before fill</span>
              </div>

              <div className="rounded-lg border border-ink/[0.08] bg-ink/[0.02] p-3">
                <span className="inline-flex items-center gap-1 text-[11px] font-medium text-ink/60">Missed / Unfilled <InfoTip metric="missed_fills" label="Missed / Unfilled" /></span>
                <div className="mt-1 text-base font-semibold font-mono text-ink">
                  {formatCurrency(selectedReport.missed_fills_gap)}
                </div>
                <span className="text-[10px] text-ink/40">Rejections & partials</span>
              </div>
            </div>
          </GlassCard>

          {/* Trade-by-trade alignment table */}
          {Array.isArray((selectedReport.report as any)?.matched_trades) && (
            <GlassCard className="overflow-hidden p-0">
              <div className="border-b border-ink/[0.06] px-5 py-3.5">
                <h3 className="text-sm font-semibold text-ink">Matched Trade Comparisons</h3>
                <p className="text-xs text-ink/60">Trade-by-trade fill price, latency, and P&L divergence.</p>
              </div>
              <div className="overflow-x-auto">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-ink/[0.06] bg-ink/[0.02] text-ink/60">
                    <tr>
                      <th className="px-4 py-2.5 font-medium">Symbol</th>
                      <th className="px-4 py-2.5 font-medium">Side</th>
                      <th className="px-4 py-2.5 font-medium text-right">Qty</th>
                      <th className="px-4 py-2.5 font-medium text-right">Paper Price</th>
                      <th className="px-4 py-2.5 font-medium text-right">Backtest Price</th>
                      <th className="px-4 py-2.5 font-medium text-right"><span className="inline-flex items-center gap-1">Price Delta <InfoTip metric="slippage_drift" label="Price Delta" /></span></th>
                      <th className="px-4 py-2.5 font-medium text-right"><span className="inline-flex items-center gap-1">Latency <InfoTip metric="timing_latency" label="Latency" /></span></th>
                      <th className="px-4 py-2.5 font-medium text-right"><span className="inline-flex items-center gap-1">P&L Gap <InfoTip metric="pnl_gap" label="P&L Gap" /></span></th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-ink/[0.04]">
                    {((selectedReport.report as any).matched_trades as any[]).map((t, idx) => (
                      <tr key={idx} className="hover:bg-ink/[0.02]">
                        <td className="px-4 py-2 font-mono font-medium text-ink">{t.symbol}</td>
                        <td className="px-4 py-2">
                          <span className={cn('font-semibold uppercase text-[10px]', t.side === 'buy' ? 'text-profit' : 'text-loss')}>
                            {t.side}
                          </span>
                        </td>
                        <td className="px-4 py-2 text-right font-mono text-ink/80">{formatNumber(t.qty)}</td>
                        <td className="px-4 py-2 text-right font-mono text-ink">{formatCurrency(t.paper_price)}</td>
                        <td className="px-4 py-2 text-right font-mono text-ink/70">{formatCurrency(t.backtest_price)}</td>
                        <td className={cn('px-4 py-2 text-right font-mono font-medium', t.price_delta > 0 ? 'text-loss' : 'text-profit')}>
                          {t.price_delta > 0 ? `+${t.price_delta.toFixed(4)}` : t.price_delta.toFixed(4)}
                        </td>
                        <td className="px-4 py-2 text-right font-mono text-ink/70">{t.timing_latency_ms.toFixed(1)} ms</td>
                        <td className={cn('px-4 py-2 text-right font-mono font-semibold', t.pnl_gap >= 0 ? 'text-profit' : 'text-loss')}>
                          {formatCurrency(t.pnl_gap)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </GlassCard>
          )}
        </div>
      )}
    </div>
  );
}
