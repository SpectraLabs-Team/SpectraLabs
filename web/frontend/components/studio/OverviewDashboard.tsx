'use client';

import { MetricLabel } from '@/components/ui/MetricInfo';
import type { MetricId } from '@/lib/learn/metrics';
import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { useSearchParams } from 'next/navigation';
import {
  AlertTriangle,
  ArrowDown,
  ArrowRight,
  ArrowUp,
  Beaker,
  Check,
  CheckCircle2,
  Clock,
  Copy,
  Database,
  ExternalLink,
  Eye,
  EyeOff,
  Flame,
  Globe,
  LineChart,
  Lock,
  Play,
  RotateCcw,
  Share2,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  TrendingDown,
  TrendingUp,
  Users,
  X,
  XCircle,
  Zap,
} from 'lucide-react';
import { Badge, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn, formatCompactCurrency } from '@/lib/utils';
import { describeRequest, listBacktests, type BacktestListItem } from '@/lib/backtests';
import {
  getKillSwitches,
  getRiskMetrics,
  listAuditDecisions,
  type KillSwitch,
  type RiskDecision,
  type RiskMetricsSummary,
} from '@/lib/riskCenter';
import {
  DEFAULT_LAYOUT,
  createShareableLayoutUrl,
  decodeLayoutFromSharing,
  encodeLayoutForSharing,
  fetchServerLayout,
  loadSavedLayout,
  resetLayout,
  saveLayout,
  saveServerLayout,
  type WidgetId,
  type WidgetLayoutItem,
} from '@/lib/dashboardLayout';
import { ago, cash, pct, ratio, signedPct, tone } from './backtests/format';
import { StatusBadge } from './backtests/RunList';
import { usePortfolio } from './PortfolioContext';
import { RiskSourceNote } from './RiskSourceNote';
import type { StudioTab } from './StudioShell';

export interface OverviewDashboardProps {
  onTab: (tab: StudioTab) => void;
  onSelectRun?: (runId: string) => void;
}

function ShareLayoutModal({
  isOpen,
  onClose,
  layout,
}: {
  isOpen: boolean;
  onClose: () => void;
  layout: WidgetLayoutItem[];
}) {
  const [copied, setCopied] = useState(false);
  const [readOnlyMode, setReadOnlyMode] = useState(true);
  if (!isOpen) return null;

  const shareUrl = typeof window !== 'undefined'
    ? createShareableLayoutUrl(window.location.origin + window.location.pathname, layout, { readOnly: readOnlyMode })
    : '';

  const copyLink = () => {
    if (!shareUrl) return;
    navigator.clipboard?.writeText(shareUrl).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="share-layout-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg overflow-hidden rounded-2xl border border-ink/10 bg-panel shadow-2xl transition"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-ink/[0.08] px-6 py-4">
          <div className="flex items-center gap-2">
            <Share2 className="h-4 w-4 text-accent" />
            <h2 id="share-layout-title" className="text-sm font-semibold text-ink">
              Share Dashboard View
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-ink/40 hover:bg-ink/5 hover:text-ink"
            aria-label="Close dialog"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="p-6 space-y-4">
          <p className="text-xs text-ink/65">
            Share this view with workspace members or across teams. The recipient will see your exact widget order and visibility arrangement.
          </p>

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="block text-xs font-medium text-ink/60">
                Shareable Dashboard URL
              </label>
              <label className="flex items-center gap-1.5 cursor-pointer text-[11px] text-ink/70 hover:text-ink">
                <input
                  type="checkbox"
                  checked={readOnlyMode}
                  onChange={(e) => setReadOnlyMode(e.target.checked)}
                  className="rounded border-ink/20 text-accent focus:ring-accent/30 h-3.5 w-3.5"
                />
                <span>Read-Only mode</span>
              </label>
            </div>
            <div className="flex items-center gap-2">
              <input
                readOnly
                value={shareUrl}
                className="input min-w-0 flex-1 font-mono text-[11px] select-all"
                onFocus={(e) => e.target.select()}
              />
              <button
                type="button"
                onClick={copyLink}
                className="btn-primary inline-flex shrink-0 items-center gap-1.5 px-3 py-2 text-xs font-medium"
              >
                {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                {copied ? 'Copied!' : 'Copy Link'}
              </button>
            </div>
            {readOnlyMode && (
              <p className="text-[11px] text-accent/90">
                🔒 Locks layout customization for recipients opening this link.
              </p>
            )}
          </div>

          <div className="rounded-xl border border-ink/[0.08] bg-ink/[0.02] p-3 text-xs space-y-2">
            <div className="flex items-center gap-2 text-ink/80 font-medium">
              <Users className="h-3.5 w-3.5 text-accent" />
              <span>Workspace & Role Access</span>
            </div>
            <ul className="list-disc list-inside space-y-1 text-[11px] text-ink/55 pl-1">
              <li><strong className="text-ink/80">Viewers:</strong> Open in locked read-only telemetry mode.</li>
              <li><strong className="text-ink/80">Editors & Owners:</strong> Can adopt layout into their workspace.</li>
              <li>Works across workspaces without altering the sender&apos;s saved layout.</li>
            </ul>
          </div>
        </div>

        <div className="flex items-center justify-end border-t border-ink/[0.08] bg-ink/[0.02] px-6 py-3.5">
          <button
            type="button"
            onClick={onClose}
            className="btn-secondary px-4 py-1.5 text-xs font-medium"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function CustomizeLayoutModal({
  layout,
  isOpen,
  onClose,
  onToggle,
  onMove,
  onReset,
  canPublishDefault,
  onPublishDefault,
  publishingDefault,
}: {
  layout: WidgetLayoutItem[];
  isOpen: boolean;
  onClose: () => void;
  onToggle: (id: WidgetId) => void;
  onMove: (index: number, direction: 'up' | 'down') => void;
  onReset: () => void;
  canPublishDefault?: boolean;
  onPublishDefault?: () => void;
  publishingDefault?: boolean;
}) {
  if (!isOpen) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="customize-layout-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs"
      onClick={onClose}
    >
      <div
        className="w-full max-w-xl overflow-hidden rounded-2xl border border-ink/10 bg-panel shadow-2xl transition"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-ink/[0.08] px-6 py-4">
          <div className="flex items-center gap-2">
            <SlidersHorizontal className="h-4 w-4 text-accent" />
            <h2 id="customize-layout-title" className="text-sm font-semibold text-ink">
              Customize Overview Layout
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-ink/40 hover:bg-ink/5 hover:text-ink"
            aria-label="Close dialog"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="max-h-[60vh] overflow-y-auto p-6 space-y-3">
          <p className="text-xs text-ink/60">
            Reorder widgets and toggle their visibility. Changes are saved automatically in your browser for this workspace.
          </p>

          <div className="mt-4 divide-y divide-ink/[0.06] rounded-xl border border-ink/[0.08] bg-ink/[0.015]">
            {layout.map((item, idx) => {
              return (
                <div key={item.id} className="flex items-center justify-between p-3.5 transition hover:bg-ink/[0.02]">
                  <div className="flex items-center gap-3 min-w-0 pr-3">
                    <div className="flex flex-col gap-0.5">
                      <button
                        type="button"
                        disabled={idx === 0}
                        onClick={() => onMove(idx, 'up')}
                        className="rounded p-1 text-ink/40 hover:bg-ink/5 hover:text-ink disabled:opacity-20"
                        title="Move Up"
                      >
                        <ArrowUp className="h-3 w-3" />
                      </button>
                      <button
                        type="button"
                        disabled={idx === layout.length - 1}
                        onClick={() => onMove(idx, 'down')}
                        className="rounded p-1 text-ink/40 hover:bg-ink/5 hover:text-ink disabled:opacity-20"
                        title="Move Down"
                      >
                        <ArrowDown className="h-3 w-3" />
                      </button>
                    </div>

                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className={cn('text-xs font-semibold', item.visible ? 'text-ink' : 'text-ink/40 line-through')}>
                          {item.title}
                        </span>
                        <span className={cn(
                          'rounded-full px-1.5 py-0.2 font-mono text-[9px] uppercase tracking-wider',
                          item.visible ? 'bg-accent/15 text-accent' : 'bg-ink/10 text-ink/40'
                        )}>
                          {item.visible ? 'Visible' : 'Hidden'}
                        </span>
                      </div>
                      <p className="mt-0.5 truncate text-[11px] text-ink/45">
                        {item.description}
                      </p>
                    </div>
                  </div>

                  <button
                    type="button"
                    onClick={() => onToggle(item.id)}
                    className={cn(
                      'inline-flex shrink-0 items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-medium transition',
                      item.visible
                        ? 'border-accent/30 bg-accent/10 text-accent hover:bg-accent/20'
                        : 'border-ink/10 bg-ink/5 text-ink/40 hover:bg-ink/10 hover:text-ink'
                    )}
                  >
                    {item.visible ? <Eye className="h-3.5 w-3.5" /> : <EyeOff className="h-3.5 w-3.5" />}
                    <span>{item.visible ? 'Hide' : 'Show'}</span>
                  </button>
                </div>
              );
            })}
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-ink/[0.08] bg-ink/[0.02] px-6 py-3.5">
          <button
            type="button"
            onClick={onReset}
            className="inline-flex items-center gap-1.5 text-xs text-ink/55 hover:text-ink hover:underline"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Reset to default
          </button>
          <div className="flex items-center gap-2">
            {canPublishDefault && onPublishDefault && (
              <button
                type="button"
                onClick={onPublishDefault}
                disabled={publishingDefault}
                className="btn-secondary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-accent hover:border-accent/40"
                title="Save this layout as the workspace-wide default for all members"
              >
                <Globe className="h-3.5 w-3.5" />
                <span>{publishingDefault ? 'Publishing…' : 'Publish as Workspace Default'}</span>
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="btn-primary px-4 py-1.5 text-xs font-medium"
            >
              Done
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function OverviewDashboard({ onTab, onSelectRun }: OverviewDashboardProps) {
  const { account } = usePortfolio();
  const searchParams = useSearchParams();
  const sharedLayoutParam = searchParams.get('layout');
  const readOnlyParam = searchParams.get('readonly') === 'true' || searchParams.get('mode') === 'view';

  const [loading, setLoading] = useState(true);
  const [runs, setRuns] = useState<BacktestListItem[]>([]);
  const [verdicts, setVerdicts] = useState<RiskDecision[]>([]);
  const [killSwitches, setKillSwitches] = useState<KillSwitch[]>([]);
  const [metrics, setMetrics] = useState<RiskMetricsSummary | null>(null);
  const [error, setError] = useState('');
  const [layout, setLayout] = useState<WidgetLayoutItem[]>(() => loadSavedLayout());
  const [customizing, setCustomizing] = useState(false);
  const [shareModalOpen, setShareModalOpen] = useState(false);
  const [isSharedView, setIsSharedView] = useState(false);
  const [isReadOnlyView, setIsReadOnlyView] = useState(false);
  const [adoptMessage, setAdoptMessage] = useState('');
  const [userRole, setUserRole] = useState<'owner' | 'editor' | 'viewer' | null>(null);
  const [workspaceId, setWorkspaceId] = useState<string | null>(null);
  const [isServerDefault, setIsServerDefault] = useState(false);
  const [publishingDefault, setPublishingDefault] = useState(false);

  // Load user session, M19 role, and sync server layout
  useEffect(() => {
    fetch('/api/auth/session', { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((s: { workspace?: { role?: 'owner' | 'editor' | 'viewer'; id?: string } | null }) => {
        if (s?.workspace?.role) setUserRole(s.workspace.role);
        if (s?.workspace?.id) {
          const wsId = s.workspace.id;
          setWorkspaceId(wsId);
          if (!sharedLayoutParam) {
            fetchServerLayout(wsId).then((res) => {
              if (res && res.layout) {
                setLayout(res.layout);
                saveLayout(res.layout, wsId);
                setIsServerDefault(res.isWorkspaceDefault);
              }
            });
          }
        }
      })
      .catch(() => undefined);
  }, [sharedLayoutParam]);

  // Process shared layout parameter from URL
  useEffect(() => {
    if (sharedLayoutParam) {
      const decoded = decodeLayoutFromSharing(sharedLayoutParam);
      if (decoded && decoded.length > 0) {
        setLayout(decoded);
        setIsSharedView(true);
        setIsReadOnlyView(readOnlyParam || userRole === 'viewer');
      }
    } else if (userRole === 'viewer') {
      setIsReadOnlyView(true);
    }
  }, [sharedLayoutParam, readOnlyParam, userRole]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setCustomizing(false);
        setShareModalOpen(false);
      }
    };
    if (customizing || shareModalOpen) {
      window.addEventListener('keydown', handleKeyDown);
      return () => window.removeEventListener('keydown', handleKeyDown);
    }
  }, [customizing, shareModalOpen]);

  const handleToggleWidget = (id: WidgetId) => {
    setLayout((prev) => {
      const updated = prev.map((item) => (item.id === id ? { ...item, visible: !item.visible } : item));
      saveLayout(updated, workspaceId);
      if (workspaceId) void saveServerLayout(workspaceId, updated, false);
      return updated;
    });
  };

  const handleMoveWidget = (index: number, direction: 'up' | 'down') => {
    setLayout((prev) => {
      const targetIndex = direction === 'up' ? index - 1 : index + 1;
      if (targetIndex < 0 || targetIndex >= prev.length) return prev;
      const next = [...prev];
      const [moved] = next.splice(index, 1);
      next.splice(targetIndex, 0, moved);
      saveLayout(next, workspaceId);
      if (workspaceId) void saveServerLayout(workspaceId, next, false);
      return next;
    });
  };

  const handleResetLayout = () => {
    const def = resetLayout(workspaceId);
    setLayout(def);
    if (workspaceId) void saveServerLayout(workspaceId, def, false);
  };

  const handleAdoptSharedLayout = () => {
    saveLayout(layout, workspaceId);
    if (workspaceId) void saveServerLayout(workspaceId, layout, false);
    setAdoptMessage('Shared layout successfully saved as your default workspace layout.');
    setTimeout(() => setAdoptMessage(''), 4000);
  };

  const handlePublishWorkspaceDefault = async () => {
    if (!workspaceId) return;
    setPublishingDefault(true);
    try {
      const ok = await saveServerLayout(workspaceId, layout, true);
      if (ok) {
        setIsServerDefault(true);
        setAdoptMessage('Published this layout as the workspace-wide default for all members.');
        setTimeout(() => setAdoptMessage(''), 4000);
      }
    } finally {
      setPublishingDefault(false);
    }
  };

  const handleExitSharedView = () => {
    try {
      const url = new URL(window.location.href);
      url.searchParams.delete('layout');
      url.searchParams.delete('readonly');
      url.searchParams.delete('mode');
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* ignore */
    }
    setIsSharedView(false);
    setIsReadOnlyView(userRole === 'viewer');
    setLayout(loadSavedLayout(workspaceId));
  };

  const loadData = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const [runsRes, auditRes, metricsRes, ksRes] = await Promise.all([
        listBacktests({ limit: 8 }).catch(() => ({ items: [], next_cursor: null })),
        listAuditDecisions(8).catch(() => []),
        getRiskMetrics().catch(() => null),
        getKillSwitches().catch(() => []),
      ]);
      setRuns(runsRes.items);
      setVerdicts(auditRes);
      setMetrics(metricsRes);
      setKillSwitches(ksRes);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load overview data.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const activeKillSwitches = killSwitches.filter((k) => k.active);
  const isHalted = (metrics?.kill_switch_active ?? false) || activeKillSwitches.length > 0;
  const haltReason = metrics?.kill_switch_reason || activeKillSwitches[0]?.reason || 'Trading halted';

  const approvedCount = verdicts.filter((v) => v.verdict.toLowerCase() === 'approved').length;
  const rejectedCount = verdicts.filter((v) => v.verdict.toLowerCase() === 'rejected').length;
  const reducedCount = verdicts.filter((v) => v.verdict.toLowerCase() === 'reduced').length;

  const canRun = userRole !== 'viewer';

  const limitItems = [
    {
      id: 'gross',
      metric: 'gross_exposure_utilization' as MetricId,
      name: 'Gross Exposure',
      current: metrics?.gross_exposure ?? 0,
      limit: metrics?.gross_exposure_limit ?? 1_000_000,
      utilization: metrics?.gross_exposure_utilization_pct ?? 0,
      formatCurrent: (v: number) => formatCompactCurrency(v),
      formatLimit: (v: number) => formatCompactCurrency(v),
    },
    {
      id: 'net',
      metric: 'net_exposure_utilization' as MetricId,
      name: 'Net Exposure',
      current: metrics?.net_exposure ?? 0,
      limit: metrics?.net_exposure_limit ?? 500_000,
      utilization: metrics?.net_exposure_utilization_pct ?? 0,
      formatCurrent: (v: number) => formatCompactCurrency(v),
      formatLimit: (v: number) => formatCompactCurrency(v),
    },
    {
      id: 'leverage',
      metric: 'leverage_utilization' as MetricId,
      name: 'Leverage',
      current: metrics?.leverage ?? 0,
      limit: metrics?.leverage_limit ?? 3.0,
      utilization: metrics?.leverage_utilization_pct ?? 0,
      formatCurrent: (v: number) => `${v.toFixed(2)}x`,
      formatLimit: (v: number) => `${v.toFixed(2)}x`,
    },
    {
      id: 'drawdown',
      metric: 'drawdown_utilization' as MetricId,
      name: 'Max Drawdown',
      current: metrics?.drawdown_pct ?? 0,
      limit: metrics?.drawdown_limit_pct ?? 0.15,
      utilization: metrics?.drawdown_utilization_pct ?? 0,
      formatCurrent: (v: number) => `${(v * 100).toFixed(1)}%`,
      formatLimit: (v: number) => `${(v * 100).toFixed(1)}%`,
    },
    {
      id: 'daily_loss',
      metric: 'daily_loss_utilization' as MetricId,
      name: 'Daily Loss',
      current: metrics?.daily_loss ?? 0,
      limit: metrics?.daily_loss_limit ?? 50_000,
      utilization: metrics?.daily_loss_utilization_pct ?? 0,
      formatCurrent: (v: number) => formatCompactCurrency(v),
      formatLimit: (v: number) => formatCompactCurrency(v),
    },
  ];

  const breachedLimits = limitItems.filter((l) => l.utilization >= 100);

  // Render individual widget blocks
  const renderSummaryKpis = () => (
    <div key="summary_kpis" className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {/* Risk Posture & Kill Switch Widget */}
      <GlassCard
        className={cn(
          'p-4 transition',
          isHalted
            ? 'border-rose-500/40 bg-rose-500/[0.08] dark:border-rose-500/30 dark:bg-rose-950/20'
            : 'border-emerald-500/20 bg-emerald-500/[0.03] dark:border-emerald-500/20'
        )}
      >
        <div className="flex items-start justify-between">
          <span className="font-mono text-[10px] uppercase tracking-wider text-ink/50">
            Risk Controls
          </span>
          <div
            className={cn(
              'flex h-7 w-7 items-center justify-center rounded-lg shadow-xs',
              isHalted ? 'bg-rose-500 text-white animate-pulse' : 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400'
            )}
          >
            {isHalted ? <ShieldAlert className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}
          </div>
        </div>
        <div className="mt-2">
          <p className={cn('text-lg font-bold', isHalted ? 'text-rose-600 dark:text-rose-400' : 'text-emerald-600 dark:text-emerald-400')}>
            {isHalted ? 'Trading Halted' : 'Normal Operations'}
          </p>
          <p className="mt-0.5 truncate text-[11px] text-ink/60" title={isHalted ? haltReason : '0 open kill switches'}>
            {isHalted ? haltReason : 'All circuit breakers clear'}
          </p>
        </div>
      </GlassCard>

      {/* Recent Backtests KPI */}
      <GlassCard className="p-4">
        <div className="flex items-start justify-between">
          <span className="font-mono text-[10px] uppercase tracking-wider text-ink/50">
            Backtests
          </span>
          <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent/15 text-accent">
            <LineChart className="h-4 w-4" />
          </div>
        </div>
        <div className="mt-2">
          <p className="num text-lg font-bold text-ink">
            {runs.length > 0 ? `${runs.length} recorded` : '0 runs'}
          </p>
          <p className="mt-0.5 text-[11px] text-ink/60">
            {runs.filter((r) => r.status === 'succeeded').length} succeeded · {runs.filter((r) => r.status === 'running').length} running
          </p>
        </div>
      </GlassCard>

      {/* Pre-Trade Checks KPI */}
      <GlassCard className="p-4">
        <div className="flex items-start justify-between">
          <span className="font-mono text-[10px] uppercase tracking-wider text-ink/50">
            Pre-Trade Audit
          </span>
          <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-purple-500/15 text-purple-600 dark:text-purple-400">
            <ShieldCheck className="h-4 w-4" />
          </div>
        </div>
        <div className="mt-2">
          <p className="num text-lg font-bold text-ink">
            {verdicts.length} Decisions
          </p>
          <p className="mt-0.5 text-[11px] text-ink/60">
            <span className="text-profit">{approvedCount} approved</span> ·{' '}
            <span className="text-amber-500">{reducedCount} reduced</span> ·{' '}
            <span className="text-loss">{rejectedCount} blocked</span>
          </p>
        </div>
      </GlassCard>

      {/* Portfolio / Engine Balance */}
      <GlassCard className="p-4">
        <div className="flex items-start justify-between">
          <span className="font-mono text-[10px] uppercase tracking-wider text-ink/50">
            Capital & Day P&L
          </span>
          <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-profit/15 text-profit">
            <TrendingUp className="h-4 w-4" />
          </div>
        </div>
        <div className="mt-2">
          <p className="num text-lg font-bold text-ink">
            {cash(account.equity || 100_000, 0)}
          </p>
          <p className="mt-0.5 text-[11px] text-ink/60">
            Cash: {cash(account.cash || 100_000, 0)} · Lev: {account.grossLeverage.toFixed(2)}x
          </p>
        </div>
      </GlassCard>
    </div>
  );

  const renderRiskGauges = () => (
    <GlassCard key="risk_gauges" as="section" className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between border-b border-ink/[0.06] px-5 py-4">
        <div>
          <div className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-accent" />
            <h2 className="text-sm font-semibold text-ink">Live Risk Limits & Breaches (M8)</h2>
            {breachedLimits.length > 0 ? (
              <Badge tone="loss">
                {breachedLimits.length} BREACHED
              </Badge>
            ) : (
              <Badge tone="profit">
                ENFORCED
              </Badge>
            )}
          </div>
          <p className="text-xs text-ink/50">
            Real-time pre-trade risk thresholds. Any limit exceeding 100% blocks or throttles new orders.
          </p>
          <RiskSourceNote grossExposure={metrics?.gross_exposure} className="mt-2" />
        </div>
        <button
          type="button"
          onClick={() => onTab('risk-center')}
          className="text-xs font-medium text-accent hover:underline"
        >
          Manage Limits in Risk Center <ArrowRight className="inline h-3 w-3" />
        </button>
      </div>

      <div className="grid grid-cols-1 divide-y divide-ink/[0.06] sm:grid-cols-2 sm:divide-x sm:divide-y-0 lg:grid-cols-5">
        {limitItems.map((item) => {
          const isBreached = item.utilization >= 100;
          const isWarning = item.utilization >= 80 && !isBreached;

          return (
            <div
              key={item.id}
              className={cn(
                'p-4 transition',
                isBreached && 'bg-rose-500/[0.08] dark:bg-rose-950/20'
              )}
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-medium text-ink/60"><MetricLabel metric={item.metric}>{item.name}</MetricLabel></span>
                <span
                  className={cn(
                    'font-mono text-xs font-bold',
                    isBreached ? 'text-rose-600 dark:text-rose-400' : isWarning ? 'text-amber-500' : 'text-ink/80'
                  )}
                >
                  {item.utilization.toFixed(1)}%
                </span>
              </div>

              {/* Utilization Progress Bar */}
              <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-ink/10">
                <div
                  className={cn(
                    'h-full rounded-full transition-all duration-500',
                    isBreached ? 'bg-rose-500' : isWarning ? 'bg-amber-500' : 'bg-accent'
                  )}
                  style={{ width: `${Math.min(100, Math.max(2, item.utilization))}%` }}
                />
              </div>

              <div className="mt-2.5 flex items-baseline justify-between text-[11px]">
                <span className="num font-semibold text-ink">
                  {item.formatCurrent(item.current)}
                </span>
                <span className="num text-ink/40">
                  limit: {item.formatLimit(item.limit)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </GlassCard>
  );

  const renderQuickActions = () => (
    <GlassCard key="quick_actions" as="section" className="overflow-hidden p-5">
      <div className="flex items-center justify-between border-b border-ink/[0.06] pb-3.5">
        <div>
          <h2 className="text-sm font-semibold text-ink flex items-center gap-2">
            <Zap className="h-4 w-4 text-accent" /> Quick Actions & Launchpads
          </h2>
          <p className="text-xs text-ink/50">Direct shortcuts into research, risk, and data labs.</p>
        </div>
      </div>
      <div className="mt-4 grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        <button
          type="button"
          onClick={() => onTab('backtests')}
          disabled={!canRun}
          className={cn(
            'flex flex-col items-start gap-1 rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-3 text-left transition',
            canRun ? 'hover:border-accent/40 hover:bg-accent/[0.05]' : 'opacity-60 cursor-not-allowed'
          )}
          title={canRun ? 'Run strategy on datasets' : 'Viewer role has read-only access'}
        >
          <div className="flex items-center justify-between w-full">
            <LineChart className="h-4 w-4 text-accent" />
            {!canRun && <Lock className="h-3 w-3 text-ink/40" />}
          </div>
          <span className="text-xs font-semibold text-ink">New Backtest</span>
          <span className="text-[10.5px] text-ink/45">{canRun ? 'Run strategy on datasets' : 'Read-only mode'}</span>
        </button>
        <button
          type="button"
          onClick={() => onTab('risk-center')}
          className="flex flex-col items-start gap-1 rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-3 text-left transition hover:border-rose-500/40 hover:bg-rose-500/[0.05]"
        >
          <ShieldAlert className="h-4 w-4 text-rose-500" />
          <span className="text-xs font-semibold text-ink">Risk Center</span>
          <span className="text-[10.5px] text-ink/45">Limits & kill switches</span>
        </button>
        <button
          type="button"
          onClick={() => onTab('registry')}
          className="flex flex-col items-start gap-1 rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-3 text-left transition hover:border-accent/40 hover:bg-accent/[0.05]"
        >
          <Beaker className="h-4 w-4 text-accent" />
          <span className="text-xs font-semibold text-ink">Experiment Registry</span>
          <span className="text-[10.5px] text-ink/45">Hypotheses & lineage</span>
        </button>
        <button
          type="button"
          onClick={() => onTab('data')}
          className="flex flex-col items-start gap-1 rounded-xl border border-ink/[0.07] bg-ink/[0.02] p-3 text-left transition hover:border-accent/40 hover:bg-accent/[0.05]"
        >
          <Database className="h-4 w-4 text-accent" />
          <span className="text-xs font-semibold text-ink">Data Lab</span>
          <span className="text-[10.5px] text-ink/45">Market datasets & versions</span>
        </button>
      </div>
    </GlassCard>
  );

  const renderRecentRuns = () => (
    <GlassCard key="recent_runs" as="section" className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-5 py-4">
        <div>
          <h2 className="text-sm font-semibold text-ink">Recent Backtests</h2>
          <p className="text-xs text-ink/50">Most recent runs across this workspace.</p>
        </div>
        <button
          type="button"
          onClick={() => onTab('backtests')}
          className="text-xs font-medium text-accent hover:underline"
        >
          View all <ArrowRight className="inline h-3 w-3" />
        </button>
      </div>

      {loading && runs.length === 0 ? (
        <div className="flex h-48 items-center justify-center">
          <Spinner className="h-5 w-5 text-accent" />
        </div>
      ) : runs.length === 0 ? (
        <div className="flex h-48 flex-col items-center justify-center gap-2 p-6 text-center text-xs text-ink/50">
          <LineChart className="h-8 w-8 text-ink/30" />
          <p>No backtests found in this workspace yet.</p>
          {canRun && (
            <button
              type="button"
              onClick={() => onTab('backtests')}
              className="btn-secondary mt-1 px-3 py-1.5 text-xs"
            >
              Run your first backtest
            </button>
          )}
        </div>
      ) : (
        <div className="divide-y divide-ink/[0.04]">
          {runs.map((r) => {
            const h = r.headline;
            const ret = h?.total_return;
            const sharpe = h?.sharpe;
            const stratName = r.request.strategy || 'Strategy';
            return (
              <div
                key={r.id}
                onClick={() => {
                  if (onSelectRun) onSelectRun(r.id);
                  onTab('backtests');
                }}
                className="group flex cursor-pointer items-center justify-between p-4 transition hover:bg-ink/[0.02]"
              >
                <div className="min-w-0 flex-1 pr-3">
                  <div className="flex items-center gap-2">
                    <StatusBadge status={r.status} cancelling={r.cancel_requested} />
                    <span className="truncate font-sans text-xs font-semibold text-ink group-hover:text-accent">
                      {stratName}
                    </span>
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-ink/45">
                    <span className="font-mono">{r.id.slice(0, 8)}</span>
                    <span>•</span>
                    <span className="flex items-center gap-1">
                      <Clock className="h-3 w-3" /> {ago(r.created_at)}
                    </span>
                    {r.run_id && (
                      <>
                        <span>•</span>
                        <button
                          type="button"
                          title="Open in Experiment Registry"
                          onClick={(e) => {
                            e.stopPropagation();
                            try {
                              const url = new URL(window.location.href);
                              url.searchParams.set('tab', 'registry');
                              url.searchParams.set('run', r.run_id!);
                              window.history.pushState(null, '', url.toString());
                            } catch {
                              /* ignore */
                            }
                            onTab('registry');
                          }}
                          className="inline-flex items-center gap-1 rounded bg-accent/10 px-1.5 py-0.5 font-mono text-[10.5px] text-accent transition hover:bg-accent/20"
                        >
                          <Beaker className="h-2.5 w-2.5" />
                          <span>run {r.run_id.slice(0, 8)}</span>
                          <ExternalLink className="h-2.5 w-2.5 opacity-60" />
                        </button>
                      </>
                    )}
                  </div>
                </div>

                {/* Run Metrics */}
                <div className="flex shrink-0 items-center gap-4 text-right">
                  {ret !== undefined && ret !== null && (
                    <div>
                      <p className={cn('num text-xs font-semibold', tone(ret) === 'profit' ? 'text-profit' : tone(ret) === 'loss' ? 'text-loss' : 'text-ink')}>
                        {signedPct(ret, 1)}
                      </p>
                      <p className="text-[10px] text-ink/45"><span onClick={(e) => e.stopPropagation()}><MetricLabel metric="total_return">Return</MetricLabel></span></p>
                    </div>
                  )}
                  {sharpe !== undefined && sharpe !== null && (
                    <div>
                      <p className="num text-xs font-medium text-ink/80">
                        {ratio(sharpe, 2)}
                      </p>
                      <p className="text-[10px] text-ink/45"><span onClick={(e) => e.stopPropagation()}><MetricLabel metric="sharpe_ratio">Sharpe</MetricLabel></span></p>
                    </div>
                  )}
                  <ArrowRight className="h-3.5 w-3.5 text-ink/30 transition group-hover:translate-x-0.5 group-hover:text-accent" />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </GlassCard>
  );

  const renderKillSwitches = () => (
    <GlassCard key="kill_switches" as="section" className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-5 py-4">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <ShieldAlert className="h-4 w-4 text-rose-500" />
            Kill Switches
          </h2>
          <p className="text-xs text-ink/50">Emergency trading halts and circuit breakers.</p>
        </div>
        <button
          type="button"
          onClick={() => onTab('risk-center')}
          className="text-xs font-medium text-accent hover:underline"
        >
          Controls <ArrowRight className="inline h-3 w-3" />
        </button>
      </div>

      <div className="p-4">
        {isHalted ? (
          <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4">
            <div className="flex items-center gap-2 text-xs font-bold text-rose-600 dark:text-rose-400">
              <XCircle className="h-4 w-4" />
              <span>TRADING HALT ACTIVE</span>
            </div>
            <p className="mt-1 text-xs text-rose-800 dark:text-rose-200">
              {haltReason}
            </p>
            <button
              type="button"
              onClick={() => onTab('risk-center')}
              className="mt-3 inline-flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-medium text-white shadow-xs hover:bg-rose-500"
            >
              Open Risk Center to Manage
            </button>
          </div>
        ) : (
          <div className="flex items-center gap-3 rounded-xl border border-emerald-500/20 bg-emerald-500/5 p-3.5">
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-emerald-500/20 text-emerald-600 dark:text-emerald-400">
              <CheckCircle2 className="h-4 w-4" />
            </div>
            <div>
              <p className="text-xs font-semibold text-emerald-700 dark:text-emerald-300">
                0 Open Kill Switches
              </p>
              <p className="text-[11px] text-ink/50">
                Trading pipeline and pre-trade order routing operating normally.
              </p>
            </div>
          </div>
        )}
      </div>
    </GlassCard>
  );

  const renderAuditDecisions = () => (
    <GlassCard key="audit_decisions" as="section" className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink/[0.06] px-5 py-4">
        <div>
          <h2 className="text-sm font-semibold text-ink">Latest Risk Verdicts</h2>
          <p className="text-xs text-ink/50">Pre-trade check decisions and enforcement audit trail.</p>
        </div>
        <button
          type="button"
          onClick={() => onTab('risk-center')}
          className="text-xs font-medium text-accent hover:underline"
        >
          Risk Center <ArrowRight className="inline h-3 w-3" />
        </button>
      </div>

      {loading && verdicts.length === 0 ? (
        <div className="flex h-40 items-center justify-center">
          <Spinner className="h-5 w-5 text-accent" />
        </div>
      ) : verdicts.length === 0 ? (
        <div className="flex h-40 flex-col items-center justify-center p-6 text-center text-xs text-ink/50">
          <ShieldCheck className="h-7 w-7 text-ink/30" />
          <p className="mt-1">No pre-trade checks recorded yet.</p>
        </div>
      ) : (
        <div className="divide-y divide-ink/[0.04]">
          {verdicts.map((v) => {
            const isApproved = v.verdict.toLowerCase() === 'approved';
            const isRejected = v.verdict.toLowerCase() === 'rejected';
            const isReduced = v.verdict.toLowerCase() === 'reduced';

            return (
              <div key={v.id} className="flex items-center justify-between p-3.5 text-xs">
                <div className="min-w-0 pr-3">
                  <div className="flex items-center gap-2">
                    <span className={cn(
                      'rounded-md px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase',
                      v.side.toLowerCase() === 'buy' ? 'bg-profit/15 text-profit' : 'bg-loss/15 text-loss'
                    )}>
                      {v.side}
                    </span>
                    <span className="font-mono font-semibold text-ink">{v.symbol}</span>
                    <span className="num text-[11px] text-ink/50">
                      {v.requested_qty} @ ${v.price.toFixed(2)}
                    </span>
                  </div>
                  <p className="mt-0.5 text-[10.5px] text-ink/40">
                    {ago(v.created_at)}
                  </p>
                </div>

                <div className="shrink-0 text-right">
                  <span className={cn(
                    'rounded-full px-2 py-0.5 text-[10.5px] font-semibold uppercase tracking-wider',
                    isApproved && 'bg-profit/15 text-profit',
                    isRejected && 'bg-loss/15 text-loss',
                    isReduced && 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
                  )}>
                    {v.verdict}
                  </span>
                  {isReduced && (
                    <p className="num mt-0.5 text-[10px] text-ink/50">
                      qty: {v.approved_qty}
                    </p>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </GlassCard>
  );

  return (
    <div className="space-y-6">
      {/* Overview Top Bar */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-xl font-bold tracking-tight text-ink">Workspace Overview</h1>
            {userRole && (
              <span className={cn(
                'rounded-full px-2 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wider',
                userRole === 'owner' ? 'bg-accent/15 text-accent' : userRole === 'editor' ? 'bg-profit/15 text-profit' : 'bg-ink/10 text-ink/60'
              )}>
                {userRole === 'viewer' ? 'Viewer (Read-Only)' : userRole}
              </span>
            )}
            {isSharedView && (
              <span className="rounded-full bg-accent/20 px-2 py-0.5 font-mono text-[10px] font-bold uppercase tracking-wider text-accent animate-pulse">
                Shared View
              </span>
            )}
            {!isSharedView && isServerDefault && (
              <span className="rounded-full bg-ink/5 px-2 py-0.5 font-mono text-[10px] text-ink/50" title="Viewing the team workspace default layout">
                Workspace Default
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink/60">
            Real runs, pre-trade risk verdicts, and live system state at a glance.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => setShareModalOpen(true)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-surface px-3 py-1.5 text-xs font-medium text-ink/70 transition hover:bg-ink/5"
            aria-label="Share current dashboard layout"
          >
            <Share2 className="h-3.5 w-3.5 text-accent" />
            Share View
          </button>
          {isReadOnlyView || userRole === 'viewer' ? (
            <button
              type="button"
              disabled
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-ink/5 px-3 py-1.5 text-xs font-medium text-ink/40 cursor-not-allowed"
              title={userRole === 'viewer' ? "Viewers have read-only access" : "Read-only view active. Exit shared view or save to customize."}
              aria-label="Layout customization locked in read-only mode"
            >
              <Lock className="h-3.5 w-3.5" />
              Layout Locked
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setCustomizing(true)}
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-surface px-3 py-1.5 text-xs font-medium text-ink/70 transition hover:bg-ink/5"
              aria-label="Customize dashboard layout"
            >
              <SlidersHorizontal className="h-3.5 w-3.5 text-accent" />
              Customize Layout
            </button>
          )}
          <button
            type="button"
            onClick={loadData}
            disabled={loading}
            className="inline-flex items-center gap-1.5 rounded-lg border border-ink/10 bg-surface px-3 py-1.5 text-xs font-medium text-ink/70 transition hover:bg-ink/5"
            aria-label="Refresh overview data"
          >
            <RotateCcw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} />
            Refresh
          </button>
          {canRun ? (
            <button
              type="button"
              onClick={() => onTab('backtests')}
              className="btn-primary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium"
            >
              <Play className="h-3.5 w-3.5" />
              New Backtest
            </button>
          ) : (
            <button
              type="button"
              disabled
              className="inline-flex items-center gap-1.5 rounded-xl border border-ink/10 bg-ink/5 px-3 py-1.5 text-xs font-medium text-ink/40 cursor-not-allowed"
              title="Editor or Owner role needed to queue backtests"
            >
              <Lock className="h-3 w-3" />
              Read-Only
            </button>
          )}
        </div>
      </div>

      {adoptMessage && (
        <div role="status" className="flex items-center gap-2 rounded-xl border border-profit/30 bg-profit/10 p-3 text-xs text-profit">
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{adoptMessage}</span>
        </div>
      )}

      {error && (
        <div className="rounded-xl border border-rose-500/20 bg-rose-500/10 p-3 text-xs text-rose-700 dark:text-rose-300">
          {error}
        </div>
      )}

      {/* Shared View Banner when viewing a layout via URL */}
      {isSharedView && (
        <div className="flex flex-col gap-3 rounded-2xl border border-accent/30 bg-accent/[0.06] p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-start gap-3">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-accent text-white shadow-xs">
              <Share2 className="h-4 w-4" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <p className="text-xs font-bold uppercase tracking-wider text-accent">
                  Shared Dashboard View {isReadOnlyView ? '(Read-Only)' : ''}
                </p>
                <span className="rounded bg-accent/20 px-1.5 py-0.2 font-mono text-[9px] uppercase tracking-wider text-accent">
                  {isReadOnlyView ? 'Protected' : 'Custom'} URL Layout
                </span>
              </div>
              <p className="mt-0.5 text-xs text-ink/65">
                {isReadOnlyView
                  ? 'Viewing a protected layout shared with read-only permissions.'
                  : 'Viewing a custom layout shared by a teammate.'}
                {userRole !== 'viewer' ? ' You can adopt this layout as your personal workspace default.' : ''}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {userRole !== 'viewer' ? (
              <button
                type="button"
                onClick={handleAdoptSharedLayout}
                className="btn-primary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium"
              >
                <Check className="h-3.5 w-3.5" />
                Save as My Layout
              </button>
            ) : (
              <span className="rounded-xl border border-ink/10 bg-ink/5 px-2.5 py-1 text-[11px] font-medium text-ink/40">
                Viewer Mode
              </span>
            )}
            <button
              type="button"
              onClick={handleExitSharedView}
              className="btn-secondary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium"
            >
              <X className="h-3.5 w-3.5" />
              Exit Shared View
            </button>
          </div>
        </div>
      )}

      {/* Breached Limits Alert Banner (always shown if limit breached) */}
      {breachedLimits.length > 0 && (
        <div className="flex flex-col gap-3 rounded-2xl border border-rose-500/40 bg-rose-500/10 p-4 sm:flex-row sm:items-center sm:justify-between dark:border-rose-500/30 dark:bg-rose-950/30">
          <div className="flex items-start gap-3">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-rose-500 text-white animate-pulse shadow-xs">
              <AlertTriangle className="h-5 w-5" />
            </div>
            <div>
              <p className="text-xs font-bold uppercase tracking-wider text-rose-600 dark:text-rose-400">
                Risk Limit Breached ({breachedLimits.length})
              </p>
              <p className="mt-0.5 text-xs text-rose-800 dark:text-rose-200">
                {breachedLimits.map((b) => `${b.name} (${b.utilization.toFixed(1)}% utilized)`).join(', ')} exceeded configured risk thresholds. Orders may be reduced or blocked.
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={() => onTab('risk-center')}
            className="inline-flex shrink-0 items-center gap-1.5 rounded-xl bg-rose-600 px-3.5 py-2 text-xs font-semibold text-white shadow-xs hover:bg-rose-500"
          >
            Review Risk Center <ArrowRight className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {/* Render Dashboard Widgets according to saved layout order & visibility */}
      <div className="space-y-6">
        {layout
          .filter((w) => w.visible)
          .map((w) => {
            switch (w.id) {
              case 'summary_kpis':
                return renderSummaryKpis();
              case 'risk_gauges':
                return renderRiskGauges();
              case 'quick_actions':
                return renderQuickActions();
              case 'recent_runs':
                return renderRecentRuns();
              case 'kill_switches':
                return renderKillSwitches();
              case 'audit_decisions':
                return renderAuditDecisions();
              default:
                return null;
            }
          })}
      </div>

      {layout.every((w) => !w.visible) && (
        <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-ink/15 p-12 text-center">
          <SlidersHorizontal className="h-8 w-8 text-ink/30" />
          <h3 className="mt-3 text-sm font-semibold text-ink">All widgets are hidden</h3>
          <p className="mt-1 text-xs text-ink/50">
            Customize your layout to select which widgets to display on this dashboard.
          </p>
          <div className="mt-4 flex gap-3">
            <button
              type="button"
              onClick={() => setCustomizing(true)}
              className="btn-primary text-xs"
            >
              Customize Layout
            </button>
            <button
              type="button"
              onClick={handleResetLayout}
              className="btn-secondary text-xs"
            >
              Reset to Default
            </button>
          </div>
        </div>
      )}

      {/* Customize Layout Modal */}
      <CustomizeLayoutModal
        layout={layout}
        isOpen={customizing}
        onClose={() => setCustomizing(false)}
        onToggle={handleToggleWidget}
        onMove={handleMoveWidget}
        onReset={handleResetLayout}
        canPublishDefault={userRole === 'owner' || userRole === 'editor'}
        onPublishDefault={handlePublishWorkspaceDefault}
        publishingDefault={publishingDefault}
      />

      {/* Share View Modal */}
      <ShareLayoutModal
        isOpen={shareModalOpen}
        onClose={() => setShareModalOpen(false)}
        layout={layout}
      />
    </div>
  );
}
