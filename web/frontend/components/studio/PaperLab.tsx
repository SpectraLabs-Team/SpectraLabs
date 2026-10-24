'use client';

import React, { useCallback, useEffect, useState } from 'react';
import {
  Activity,
  AlertCircle,
  AlertTriangle,
  ArrowDownRight,
  ArrowUpRight,
  CheckCircle2,
  Clock,
  DollarSign,
  Layers,
  Percent,
  Play,
  RefreshCw,
  RotateCcw,
  Send,
  ShieldAlert,
  ShieldCheck,
  TrendingDown,
  TrendingUp,
  Wallet,
} from 'lucide-react';
import {
  createPaperOrder,
  getPaperAccount,
  listPaperOrders,
  reconcilePaperPositions,
  resetPaperAccount,
  syncPaperOrders,
  type PaperAccountView,
  type PaperOrderView,
  type ReconcilePositionsResponse,
} from '@/lib/api';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn, formatCurrency, formatNumber, formatPercent } from '@/lib/utils';
import { BacktestToPaperGap } from './BacktestToPaperGap';

// Default demo strategy ID when running in studio
const DEFAULT_STRATEGY_ID = '00000000-0000-0000-0000-000000000001';

type TabView = 'positions' | 'orders' | 'gap';

export function PaperLab() {
  const [strategyId, setStrategyId] = useState(DEFAULT_STRATEGY_ID);
  const [account, setAccount] = useState<PaperAccountView | null>(null);
  const [orders, setOrders] = useState<PaperOrderView[]>([]);
  const [reconciliation, setReconciliation] = useState<ReconcilePositionsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<TabView>('positions');

  // Order Ticket State
  const [orderSymbol, setOrderSymbol] = useState('AAPL');
  const [orderSide, setOrderSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<'market' | 'limit' | 'stop'>('market');
  const [orderQty, setOrderQty] = useState('10');
  const [orderPrice, setOrderPrice] = useState('180.00');
  const [submittingOrder, setSubmittingOrder] = useState(false);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [acc, ords, recon] = await Promise.all([
        getPaperAccount(strategyId).catch(() => null),
        listPaperOrders(strategyId).catch(() => []),
        reconcilePaperPositions().catch(() => null),
      ]);
      if (acc) setAccount(acc);
      if (ords) setOrders(ords);
      if (recon) setReconciliation(recon);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load paper trading data');
    } finally {
      setLoading(false);
    }
  }, [strategyId]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleSyncBroker = async () => {
    setActionLoading(true);
    setStatusMsg(null);
    try {
      const res = await syncPaperOrders(strategyId);
      setStatusMsg(`Synced ${res.orders_synced} orders from broker (${res.source}).`);
      await loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Broker sync failed');
    } finally {
      setActionLoading(false);
    }
  };

  const handleReconcile = async () => {
    setActionLoading(true);
    setStatusMsg(null);
    try {
      const res = await reconcilePaperPositions();
      setReconciliation(res);
      setStatusMsg(
        res.reconciled
          ? 'Positions fully reconciled with broker.'
          : `Discrepancy detected: ${res.mismatches_count} mismatch(es).`
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Position reconciliation failed');
    } finally {
      setActionLoading(false);
    }
  };

  const handleResetAccount = async () => {
    if (!confirm('Are you sure you want to reset this paper trading account to $100,000 cash?')) return;
    setActionLoading(true);
    try {
      const res = await resetPaperAccount(strategyId, 100_000);
      setAccount(res);
      setStatusMsg('Paper account reset to $100,000.00 cash.');
      await loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Account reset failed');
    } finally {
      setActionLoading(false);
    }
  };

  const handleSendOrder = async (e: React.FormEvent) => {
    e.preventDefault();
    const qty = parseFloat(orderQty);
    if (!orderSymbol.trim() || isNaN(qty) || qty <= 0) {
      setError('Please provide a valid symbol and quantity');
      return;
    }
    const px = parseFloat(orderPrice);
    setSubmittingOrder(true);
    setError(null);
    setStatusMsg(null);
    try {
      const res = await createPaperOrder({
        strategy_id: strategyId,
        symbol: orderSymbol.toUpperCase().trim(),
        side: orderSide,
        order_type: orderType,
        qty,
        limit_price: orderType === 'limit' && !isNaN(px) ? px : undefined,
        stop_price: orderType === 'stop' && !isNaN(px) ? px : undefined,
        fill_now: orderType === 'market',
        fill_price: orderType === 'market' && !isNaN(px) ? px : undefined,
      });

      if (res.status === 'rejected') {
        setError(`Order rejected by pre-trade risk checker: ${res.reject_reason || 'Risk limit breach'}`);
      } else {
        setStatusMsg(`Order ${res.id.slice(0, 8)} accepted (${res.status}). Filled: ${res.filled_qty}/${res.qty}`);
      }
      await loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to submit order');
    } finally {
      setSubmittingOrder(false);
    }
  };

  const totalPositionsValue = account?.positions.reduce((acc, p) => acc + p.market_value, 0) ?? 0;
  const unrealizedPnl = account?.positions.reduce((acc, p) => acc + p.unrealized_pnl, 0) ?? 0;

  return (
    <div className="space-y-6">
      {/* Header and Controls */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-xl font-bold tracking-tight text-ink">Paper Trading & Live Execution (M9 & M10)</h1>
            <Badge tone="profit" className="text-[10px] uppercase tracking-wider">
              Simulation & Broker Sync
            </Badge>
          </div>
          <p className="mt-1 text-xs text-ink/60">
            Real-time paper trading ledger, M8 pre-trade risk enforcement, Alpaca order sync, and backtest-to-paper gap analytics.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className="btn-secondary flex items-center gap-1.5 px-3 py-1.5 text-xs"
            onClick={handleSyncBroker}
            disabled={actionLoading}
            title="Sync open orders and executions with Alpaca paper broker"
          >
            <RefreshCw className={cn('h-3.5 w-3.5', actionLoading && 'animate-spin')} />
            Sync Broker
          </button>
          <button
            type="button"
            className="btn-secondary flex items-center gap-1.5 px-3 py-1.5 text-xs"
            onClick={handleReconcile}
            disabled={actionLoading}
            title="Reconcile local database positions against broker portfolio"
          >
            <ShieldCheck className="h-3.5 w-3.5 text-accent" />
            Reconcile
          </button>
          <button
            type="button"
            className="btn-secondary flex items-center gap-1.5 px-3 py-1.5 text-xs text-loss hover:border-loss/30"
            onClick={handleResetAccount}
            disabled={actionLoading}
            title="Reset paper balance and clear open positions"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Reset Ledger
          </button>
        </div>
      </div>

      {error && <ErrorNote>{error}</ErrorNote>}
      {statusMsg && (
        <div className="flex items-center gap-2 rounded-lg border border-accent/20 bg-accent/5 px-3.5 py-2 text-xs text-accent">
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{statusMsg}</span>
        </div>
      )}

      {/* Reconciliation Status Alert */}
      {reconciliation && !reconciliation.reconciled && (
        <div className="flex items-start gap-3 rounded-xl border border-loss/30 bg-loss/10 p-3 text-xs text-loss">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <span className="font-semibold">Broker Position Mismatch Detected:</span> Local ledger positions do not match broker records ({reconciliation.mismatches_count} discrepancies).
            <ul className="mt-1 list-disc pl-4 text-[11px]">
              {reconciliation.mismatches.map((m) => (
                <li key={m.symbol}>
                  {m.symbol}: Broker holds {m.broker_qty}, local ledger has {m.local_qty} (diff: {m.discrepancy})
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* KPI Cards */}
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="font-mono text-[10px] uppercase tracking-wider">Total Equity</span>
            <Wallet className="h-4 w-4 text-profit" />
          </div>
          <p className="mt-2 text-xl font-bold tracking-tight text-ink">
            {account ? formatCurrency(account.equity) : '$100,000.00'}
          </p>
          <p className="mt-1 text-[11px] text-ink/50">
            Cash: {account ? formatCurrency(account.cash) : '$100,000.00'} · Positions: {formatCurrency(totalPositionsValue)}
          </p>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="font-mono text-[10px] uppercase tracking-wider">Unrealized P&L</span>
            {unrealizedPnl >= 0 ? <TrendingUp className="h-4 w-4 text-profit" /> : <TrendingDown className="h-4 w-4 text-loss" />}
          </div>
          <p className={cn('mt-2 text-xl font-bold tracking-tight', unrealizedPnl >= 0 ? 'text-profit' : 'text-loss')}>
            {unrealizedPnl >= 0 ? '+' : ''}
            {formatCurrency(unrealizedPnl)}
          </p>
          <p className="mt-1 text-[11px] text-ink/50">
            Peak Equity: {account ? formatCurrency(account.peak_equity) : '$100,000.00'}
          </p>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="font-mono text-[10px] uppercase tracking-wider">M7 Copula VaR (99% 1d)</span>
            <ShieldAlert className="h-4 w-4 text-accent" />
          </div>
          <p className="mt-2 text-xl font-bold tracking-tight text-ink">
            {account ? formatCurrency(account.var_dollar) : '$0.00'}
          </p>
          <p className="mt-1 text-[11px] text-ink/50">
            {account ? formatPercent(account.var_pct, 2) : '0.00%'} of equity (Student-t Copula)
          </p>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="font-mono text-[10px] uppercase tracking-wider">Copula CVaR (Expected Shortfall)</span>
            <Activity className="h-4 w-4 text-ink/40" />
          </div>
          <p className="mt-2 text-xl font-bold tracking-tight text-ink">
            {account ? formatCurrency(account.cvar_dollar) : '$0.00'}
          </p>
          <p className="mt-1 text-[11px] text-ink/50">Tail risk beyond 99th percentile</p>
        </GlassCard>
      </div>

      {/* Main Content Area */}
      <div className="grid gap-6 lg:grid-cols-[1fr_340px]">
        {/* Left Tabs View */}
        <div className="space-y-4">
          <div className="flex items-center gap-1 border-b border-ink/10 pb-2">
            <button
              type="button"
              onClick={() => setActiveTab('positions')}
              className={cn(
                'rounded-lg px-3 py-1.5 text-xs font-medium transition',
                activeTab === 'positions'
                  ? 'bg-accent/10 text-accent font-semibold'
                  : 'text-ink/60 hover:bg-ink/5 hover:text-ink'
              )}
            >
              Active Positions ({account?.positions.length ?? 0})
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('orders')}
              className={cn(
                'rounded-lg px-3 py-1.5 text-xs font-medium transition',
                activeTab === 'orders'
                  ? 'bg-accent/10 text-accent font-semibold'
                  : 'text-ink/60 hover:bg-ink/5 hover:text-ink'
              )}
            >
              Order History ({orders.length})
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('gap')}
              className={cn(
                'rounded-lg px-3 py-1.5 text-xs font-medium transition',
                activeTab === 'gap'
                  ? 'bg-accent/10 text-accent font-semibold'
                  : 'text-ink/60 hover:bg-ink/5 hover:text-ink'
              )}
            >
              Backtest-to-Paper Gap (M10)
            </button>
          </div>

          {activeTab === 'positions' && (
            <GlassCard className="overflow-hidden p-0">
              <div className="border-b border-ink/10 px-4 py-3">
                <h3 className="text-sm font-semibold text-ink">Portfolio Positions</h3>
              </div>
              {account?.positions && account.positions.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-xs">
                    <thead className="border-b border-ink/10 bg-ink/[0.02] text-ink/50">
                      <tr>
                        <th className="px-4 py-2.5 font-medium">Symbol</th>
                        <th className="px-4 py-2.5 font-medium text-right">Quantity</th>
                        <th className="px-4 py-2.5 font-medium text-right">Mark Price</th>
                        <th className="px-4 py-2.5 font-medium text-right">Market Value</th>
                        <th className="px-4 py-2.5 font-medium text-right">Weight</th>
                        <th className="px-4 py-2.5 font-medium text-right">Unrealized P&L</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-ink/5 font-mono">
                      {account.positions.map((p) => (
                        <tr key={p.symbol} className="hover:bg-ink/[0.02]">
                          <td className="px-4 py-2.5 font-semibold text-ink">{p.symbol}</td>
                          <td className="px-4 py-2.5 text-right">{formatNumber(p.qty, 2)}</td>
                          <td className="px-4 py-2.5 text-right">{formatCurrency(p.price)}</td>
                          <td className="px-4 py-2.5 text-right">{formatCurrency(p.market_value)}</td>
                          <td className="px-4 py-2.5 text-right">{formatPercent(p.weight, 1)}</td>
                          <td
                            className={cn(
                              'px-4 py-2.5 text-right font-semibold',
                              p.unrealized_pnl >= 0 ? 'text-profit' : 'text-loss'
                            )}
                          >
                            {p.unrealized_pnl >= 0 ? '+' : ''}
                            {formatCurrency(p.unrealized_pnl)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <div className="p-8 text-center text-xs text-ink/40">
                  No active paper positions. Submit an order using the ticket on the right.
                </div>
              )}
            </GlassCard>
          )}

          {activeTab === 'orders' && (
            <GlassCard className="overflow-hidden p-0">
              <div className="border-b border-ink/10 px-4 py-3">
                <h3 className="text-sm font-semibold text-ink">Order Execution Log</h3>
              </div>
              {orders.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-xs">
                    <thead className="border-b border-ink/10 bg-ink/[0.02] text-ink/50">
                      <tr>
                        <th className="px-4 py-2.5 font-medium">Time</th>
                        <th className="px-4 py-2.5 font-medium">Symbol</th>
                        <th className="px-4 py-2.5 font-medium">Side</th>
                        <th className="px-4 py-2.5 font-medium">Type</th>
                        <th className="px-4 py-2.5 font-medium text-right">Qty</th>
                        <th className="px-4 py-2.5 font-medium text-right">Filled</th>
                        <th className="px-4 py-2.5 font-medium">Status</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-ink/5 font-mono text-[11px]">
                      {orders.map((o) => (
                        <tr key={o.id} className="hover:bg-ink/[0.02]">
                          <td className="px-4 py-2 text-ink/60">{new Date(o.created_at).toLocaleTimeString()}</td>
                          <td className="px-4 py-2 font-semibold text-ink">{o.symbol}</td>
                          <td className="px-4 py-2">
                            <span
                              className={cn(
                                'rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase',
                                o.side === 'buy' ? 'bg-profit/10 text-profit' : 'bg-loss/10 text-loss'
                              )}
                            >
                              {o.side}
                            </span>
                          </td>
                          <td className="px-4 py-2 uppercase text-ink/70">{o.order_type}</td>
                          <td className="px-4 py-2 text-right">{formatNumber(o.qty, 2)}</td>
                          <td className="px-4 py-2 text-right">
                            {formatNumber(o.filled_qty, 2)} {o.filled_avg_price ? `@ ${formatCurrency(o.filled_avg_price)}` : ''}
                          </td>
                          <td className="px-4 py-2">
                            <Badge
                              tone={
                                o.status === 'filled'
                                  ? 'profit'
                                  : o.status === 'rejected'
                                  ? 'loss'
                                  : 'neutral'
                              }
                            >
                              {o.status}
                            </Badge>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <div className="p-8 text-center text-xs text-ink/40">No orders recorded in this session.</div>
              )}
            </GlassCard>
          )}

          {activeTab === 'gap' && (
            <BacktestToPaperGap strategyId={strategyId} />
          )}
        </div>

        {/* Right Panel: Order Entry Ticket with Pre-Trade Risk Checker */}
        <div>
          <GlassCard as="section" className="p-4">
            <div className="flex items-center gap-2 border-b border-ink/10 pb-3">
              <Send className="h-4 w-4 text-accent" />
              <h3 className="text-sm font-semibold text-ink">Order Entry Ticket</h3>
            </div>
            <p className="mt-1 text-[11px] text-ink/50">
              Orders pass through M8 pre-trade checks before entering the ledger.
            </p>

            <form onSubmit={handleSendOrder} className="mt-4 space-y-3.5 text-xs">
              <div>
                <label className="block text-[11px] font-medium text-ink/60">Symbol</label>
                <input
                  type="text"
                  value={orderSymbol}
                  onChange={(e) => setOrderSymbol(e.target.value.toUpperCase())}
                  className="mt-1 w-full rounded-md border border-ink/10 bg-transparent px-3 py-1.5 font-mono uppercase text-ink focus:border-accent focus:outline-none"
                  placeholder="e.g. AAPL"
                  required
                />
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-[11px] font-medium text-ink/60">Side</label>
                  <select
                    value={orderSide}
                    onChange={(e) => setOrderSide(e.target.value as 'buy' | 'sell')}
                    className="mt-1 w-full rounded-md border border-ink/10 bg-canvas px-2.5 py-1.5 text-ink focus:border-accent focus:outline-none"
                  >
                    <option value="buy">BUY / LONG</option>
                    <option value="sell">SELL / SHORT</option>
                  </select>
                </div>
                <div>
                  <label className="block text-[11px] font-medium text-ink/60">Order Type</label>
                  <select
                    value={orderType}
                    onChange={(e) => setOrderType(e.target.value as 'market' | 'limit' | 'stop')}
                    className="mt-1 w-full rounded-md border border-ink/10 bg-canvas px-2.5 py-1.5 text-ink focus:border-accent focus:outline-none"
                  >
                    <option value="market">Market</option>
                    <option value="limit">Limit</option>
                    <option value="stop">Stop</option>
                  </select>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-[11px] font-medium text-ink/60">Quantity</label>
                  <input
                    type="number"
                    step="1"
                    min="1"
                    value={orderQty}
                    onChange={(e) => setOrderQty(e.target.value)}
                    className="mt-1 w-full rounded-md border border-ink/10 bg-transparent px-3 py-1.5 font-mono text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
                <div>
                  <label className="block text-[11px] font-medium text-ink/60">Price ($)</label>
                  <input
                    type="number"
                    step="0.01"
                    min="0.01"
                    value={orderPrice}
                    onChange={(e) => setOrderPrice(e.target.value)}
                    className="mt-1 w-full rounded-md border border-ink/10 bg-transparent px-3 py-1.5 font-mono text-ink focus:border-accent focus:outline-none"
                    required
                  />
                </div>
              </div>

              <div className="rounded-lg border border-ink/10 bg-ink/[0.02] p-2.5 text-[11px] text-ink/60">
                <div className="flex justify-between">
                  <span>Estimated Value:</span>
                  <span className="font-mono font-semibold text-ink">
                    {formatCurrency((parseFloat(orderQty) || 0) * (parseFloat(orderPrice) || 0))}
                  </span>
                </div>
                <div className="mt-1 flex justify-between">
                  <span>Pre-Trade Check:</span>
                  <span className="text-profit">Active</span>
                </div>
              </div>

              <button
                type="submit"
                disabled={submittingOrder}
                className={cn(
                  'w-full rounded-lg py-2 font-medium text-white transition shadow-sm',
                  orderSide === 'buy'
                    ? 'bg-emerald-600 hover:bg-emerald-700'
                    : 'bg-rose-600 hover:bg-rose-700'
                )}
              >
                {submittingOrder ? 'Submitting...' : `${orderSide.toUpperCase()} ${orderSymbol}`}
              </button>
            </form>
          </GlassCard>
        </div>
      </div>
    </div>
  );
}
