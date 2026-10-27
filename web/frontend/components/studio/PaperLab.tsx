'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Activity,
  AlertCircle,
  AlertTriangle,
  ArrowDownRight,
  ArrowUpRight,
  CheckCircle2,
  Clock,
  DollarSign,
  ExternalLink,
  Eye,
  EyeOff,
  HelpCircle,
  Layers,
  Lock,
  Plus,
  RefreshCw,
  Search,
  ShieldAlert,
  ShieldCheck,
  Star,
  Trash2,
  TrendingDown,
  TrendingUp,
  Wallet,
  X,
} from 'lucide-react';
import {
  cancelAlpacaOrder,
  connectAlpaca,
  createAlpacaOrder,
  disconnectAlpaca,
  getAlpacaAccount,
  getAlpacaAssets,
  getAlpacaBars,
  getAlpacaOrders,
  getAlpacaPositions,
  getAlpacaStatus,
  type AlpacaAccount,
  type AlpacaAsset,
  type AlpacaOrder,
  type AlpacaPosition,
  type CandleBar,
} from '@/lib/alpaca';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn, formatCurrency, formatPercent } from '@/lib/utils';
import { TradingChart } from './TradingChart';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';

// Default initial watchlist symbols
const INITIAL_WATCHLIST = ['AAPL', 'MSFT', 'NVDA', 'TSLA', 'SPY', 'AMZN', 'GOOGL', 'META'];

type BottomTab = 'positions' | 'open-orders' | 'history' | 'performance';

export function PaperLab() {
  // Connection state
  const [checkingConnection, setCheckingConnection] = useState(true);
  const [isConnected, setIsConnected] = useState(false);
  const [keyHint, setKeyHint] = useState<string>('');

  // Connect form state
  const [inputKeyId, setInputKeyId] = useState('');
  const [inputSecretKey, setInputSecretKey] = useState('');
  const [showSecret, setShowSecret] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);

  // Trading data state
  const [account, setAccount] = useState<AlpacaAccount | null>(null);
  const [positions, setPositions] = useState<AlpacaPosition[]>([]);
  const [orders, setOrders] = useState<AlpacaOrder[]>([]);
  const [assets, setAssets] = useState<AlpacaAsset[]>([]);
  const [loadingData, setLoadingData] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  // Watchlist & Search state
  const [watchlist, setWatchlist] = useState<string[]>(INITIAL_WATCHLIST);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedSymbol, setSelectedSymbol] = useState('AAPL');

  // Chart data state
  const [chartBars, setChartBars] = useState<CandleBar[]>([]);
  const [chartTimeframe, setChartTimeframe] = useState('1Day');
  const [chartLoading, setChartLoading] = useState(false);
  const [chartDataStatus, setChartDataStatus] = useState('Alpaca Paper (IEX Feed)');

  // Order Ticket state
  const [orderSide, setOrderSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<'market' | 'limit'>('market');
  const [orderQty, setOrderQty] = useState('10');
  const [limitPrice, setLimitPrice] = useState('180.00');
  const [submittingOrder, setSubmittingOrder] = useState(false);
  const [confirmModalOpen, setConfirmModalOpen] = useState(false);

  // Bottom Tabs
  const [bottomTab, setBottomTab] = useState<BottomTab>('positions');

  // Load Watchlist from LocalStorage
  useEffect(() => {
    try {
      const saved = localStorage.getItem('spectra-paper-watchlist');
      if (saved) {
        const parsed = JSON.parse(saved);
        if (Array.isArray(parsed) && parsed.length > 0) setWatchlist(parsed);
      }
    } catch {
      // ignore
    }
  }, []);

  const saveWatchlist = (newList: string[]) => {
    setWatchlist(newList);
    try {
      localStorage.setItem('spectra-paper-watchlist', JSON.stringify(newList));
    } catch {
      // ignore
    }
  };

  const addToWatchlist = (sym: string) => {
    const upper = sym.toUpperCase().trim();
    if (!watchlist.includes(upper)) {
      saveWatchlist([...watchlist, upper]);
    }
  };

  const removeFromWatchlist = (sym: string) => {
    saveWatchlist(watchlist.filter((s) => s !== sym));
  };

  // 1. Initial Connection Status Check
  const checkStatus = useCallback(async () => {
    setCheckingConnection(true);
    try {
      const status = await getAlpacaStatus();
      setIsConnected(status.connected);
      if (status.keyIdHint) setKeyHint(status.keyIdHint);
      return status.connected;
    } catch {
      setIsConnected(false);
      return false;
    } finally {
      setCheckingConnection(false);
    }
  }, []);

  // 2. Load Alpaca Account, Positions, Orders, Assets
  const loadAlpacaData = useCallback(async () => {
    setLoadingData(true);
    setErrorMsg(null);
    try {
      const [acc, posList, ordList] = await Promise.all([
        getAlpacaAccount().catch((err) => {
          console.warn('Account load error:', err);
          return null;
        }),
        getAlpacaPositions().catch(() => []),
        getAlpacaOrders('all', 100).catch(() => []),
      ]);

      if (acc) setAccount(acc);
      setPositions(posList);
      setOrders(ordList);

      // Lazy load assets in background if empty
      getAlpacaAssets()
        .then((items) => {
          if (items && items.length > 0) setAssets(items);
        })
        .catch(() => {});
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : 'Failed to retrieve Alpaca account data.');
    } finally {
      setLoadingData(false);
    }
  }, []);

  // Initial load
  useEffect(() => {
    checkStatus().then((connected) => {
      if (connected) {
        loadAlpacaData();
      }
    });
  }, [checkStatus, loadAlpacaData]);

  // 3. Load Candlestick Bars for Selected Symbol
  const loadBars = useCallback(async (sym: string, tf: string) => {
    setChartLoading(true);
    try {
      const res = await getAlpacaBars(sym, tf, 100);
      if (res.bars && res.bars.length > 0) {
        setChartBars(res.bars);
        setChartDataStatus('Alpaca Paper (IEX Feed)');
        // Pre-fill limit price with current close
        const last = res.bars[res.bars.length - 1];
        if (last) setLimitPrice(last.c.toFixed(2));
      } else {
        // Fallback: generate realistic price points around recent quotes if Alpaca market data feed is offline
        const now = Date.now();
        const fallback: CandleBar[] = [];
        let price = 175.0;
        const step = tf.includes('Min') ? 60 * 1000 : 24 * 3600 * 1000;
        for (let i = 40; i >= 0; i--) {
          const t = now - i * step;
          const delta = (Math.random() - 0.48) * (price * 0.015);
          const c = Math.max(1, price + delta);
          const o = price;
          const h = Math.max(o, c) + Math.random() * (price * 0.005);
          const l = Math.min(o, c) - Math.random() * (price * 0.005);
          fallback.push({
            t,
            o: Math.round(o * 100) / 100,
            h: Math.round(h * 100) / 100,
            l: Math.round(l * 100) / 100,
            c: Math.round(c * 100) / 100,
            v: Math.floor(100000 + Math.random() * 400000),
          });
          price = c;
        }
        setChartBars(fallback);
        setChartDataStatus('Market Data Delayed / Simulated');
      }
    } catch {
      setChartBars([]);
    } finally {
      setChartLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isConnected) {
      loadBars(selectedSymbol, chartTimeframe);
    }
  }, [isConnected, selectedSymbol, chartTimeframe, loadBars]);

  // 4. Handle Connect Alpaca
  const handleConnect = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!inputKeyId.trim() || !inputSecretKey.trim()) {
      setConnectError('Both Alpaca API Key ID and Secret Key are required.');
      return;
    }

    setConnecting(true);
    setConnectError(null);
    try {
      const res = await connectAlpaca(inputKeyId, inputSecretKey);
      if (!res.ok) {
        setConnectError(res.error || 'Failed to authenticate with Alpaca paper trading API.');
        return;
      }

      setIsConnected(true);
      setKeyHint(res.keyIdHint || inputKeyId.slice(-4));
      setInputKeyId('');
      setInputSecretKey('');
      setSuccessMsg('Successfully connected to Alpaca Paper Trading account.');
      await loadAlpacaData();
    } catch (err) {
      setConnectError(err instanceof Error ? err.message : 'Connection failed. Check your network.');
    } finally {
      setConnecting(false);
    }
  };

  // 5. Handle Disconnect Alpaca
  const handleDisconnect = async () => {
    if (!confirm('Are you sure you want to disconnect your Alpaca paper trading account?')) return;
    try {
      await disconnectAlpaca();
      setIsConnected(false);
      setAccount(null);
      setPositions([]);
      setOrders([]);
      setKeyHint('');
      setSuccessMsg('Alpaca credentials removed from this workspace.');
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : 'Failed to disconnect account.');
    }
  };

  // 6. Handle Direct Order Placement
  const handleOrderSubmit = async () => {
    const qtyNum = parseFloat(orderQty);
    if (!selectedSymbol || isNaN(qtyNum) || qtyNum <= 0) {
      setErrorMsg('Please specify a valid symbol and positive order quantity.');
      return;
    }

    const limitPx = orderType === 'limit' ? parseFloat(limitPrice) : undefined;
    if (orderType === 'limit' && (isNaN(limitPx!) || limitPx! <= 0)) {
      setErrorMsg('Please specify a valid limit price.');
      return;
    }

    setSubmittingOrder(true);
    setErrorMsg(null);
    setSuccessMsg(null);
    setConfirmModalOpen(false);

    try {
      const newOrder = await createAlpacaOrder({
        symbol: selectedSymbol,
        qty: qtyNum,
        side: orderSide,
        type: orderType,
        limit_price: limitPx,
      });

      setSuccessMsg(
        `Alpaca ${newOrder.side.toUpperCase()} order submitted! Order ID: ${newOrder.id.slice(0, 8)} (${newOrder.status}).`
      );
      // Reload account and orders
      await loadAlpacaData();
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : 'Alpaca order submission failed.');
    } finally {
      setSubmittingOrder(false);
    }
  };

  // 7. Handle Order Cancellation
  const handleCancelOrder = async (orderId: string) => {
    try {
      await cancelAlpacaOrder(orderId);
      setSuccessMsg(`Order ${orderId.slice(0, 8)} cancellation sent to Alpaca.`);
      await loadAlpacaData();
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : 'Failed to cancel order on Alpaca.');
    }
  };

  // 8. Handle Position Close
  const handleClosePosition = async (pos: AlpacaPosition) => {
    const closeSide = pos.side === 'long' ? 'sell' : 'buy';
    const closeQty = Math.abs(parseFloat(pos.qty));
    if (!confirm(`Submit market order to ${closeSide.toUpperCase()} ${closeQty} ${pos.symbol} to close position?`)) {
      return;
    }

    try {
      await createAlpacaOrder({
        symbol: pos.symbol,
        qty: closeQty,
        side: closeSide,
        type: 'market',
      });
      setSuccessMsg(`Closing market order placed for ${pos.symbol}.`);
      await loadAlpacaData();
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : `Failed to close position for ${pos.symbol}.`);
    }
  };

  // Filtered symbols from search
  const filteredAssets = useMemo(() => {
    if (!searchQuery.trim()) return [];
    const query = searchQuery.toUpperCase().trim();
    return assets
      .filter((a) => a.tradable && (a.symbol.includes(query) || a.name.toUpperCase().includes(query)))
      .slice(0, 10);
  }, [assets, searchQuery]);

  // Current Price & Calculations
  const currentSymbolPrice = useMemo(() => {
    const pos = positions.find((p) => p.symbol === selectedSymbol);
    if (pos) return parseFloat(pos.current_price);
    if (chartBars.length > 0) return chartBars[chartBars.length - 1].c;
    return parseFloat(limitPrice) || 150.0;
  }, [positions, selectedSymbol, chartBars, limitPrice]);

  const estimatedOrderTotal = useMemo(() => {
    const q = parseFloat(orderQty) || 0;
    const px = orderType === 'limit' ? parseFloat(limitPrice) || currentSymbolPrice : currentSymbolPrice;
    return q * px;
  }, [orderQty, orderType, limitPrice, currentSymbolPrice]);

  const buyingPower = account ? parseFloat(account.buying_power) : 0;
  const totalEquity = account ? parseFloat(account.equity) : 0;
  const cashBalance = account ? parseFloat(account.cash) : 0;

  const totalUnrealizedPl = positions.reduce((sum, p) => sum + parseFloat(p.unrealized_pl || '0'), 0);
  const openOrders = orders.filter((o) =>
    ['new', 'accepted', 'pending_new', 'partially_filled'].includes(o.status)
  );

  // -------------------------------------------------------------
  // RENDERING: 1. Loading State
  // -------------------------------------------------------------
  if (checkingConnection) {
    return (
      <div className="flex min-h-[450px] items-center justify-center">
        <div className="text-center">
          <Spinner className="mx-auto h-8 w-8 text-cyan-400" />
          <p className="mt-3 text-sm text-slate-400">Verifying Alpaca paper account configuration...</p>
        </div>
      </div>
    );
  }

  // -------------------------------------------------------------
  // RENDERING: 2. Not Connected -> Alpaca Connection Screen
  // -------------------------------------------------------------
  if (!isConnected) {
    return (
      <div className="mx-auto max-w-2xl py-8">
        <GlassCard data-tour="paper-connect" className="relative overflow-hidden p-8 border border-white/10 shadow-2xl">
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-cyan-500/10 text-cyan-400 border border-cyan-500/20">
              <Wallet className="h-6 w-6" />
            </div>
            <div>
              <h1 className="text-2xl font-bold tracking-tight text-white">Connect Alpaca Paper Trading</h1>
              <p className="text-xs text-slate-400">
                Direct execution on Alpaca Paper Trading API with per-user AES-256 encrypted credential storage.
              </p>
            </div>
          </div>

          <div className="mt-6 rounded-xl border border-cyan-500/20 bg-cyan-500/5 p-4 text-xs text-slate-300">
            <div className="flex items-center gap-2 font-semibold text-cyan-400">
              <ShieldCheck className="h-4 w-4" />
              <span>Paper Trading Environment Only</span>
            </div>
            <p className="mt-1 leading-relaxed text-slate-400">
              Your credentials are encrypted at rest with envelope encryption and routed exclusively to{' '}
              <code className="text-cyan-300">https://paper-api.alpaca.markets</code>. Orders never touch live capital.
            </p>
            <div className="mt-2.5 flex items-center gap-2">
              <a
                href="https://app.alpaca.markets/paper/dashboard/overview"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 font-medium text-cyan-400 hover:underline"
              >
                <span>Generate paper keys on Alpaca</span>
                <ExternalLink className="h-3 w-3" />
              </a>
            </div>
          </div>

          {connectError && (
            <div className="mt-4">
              <ErrorNote>{connectError}</ErrorNote>
            </div>
          )}

          <form onSubmit={handleConnect} className="mt-6 space-y-4">
            <div>
              <label className="block text-xs font-medium uppercase tracking-wider text-slate-400">
                Alpaca API Key ID
              </label>
              <input
                type="text"
                required
                value={inputKeyId}
                onChange={(e) => setInputKeyId(e.target.value)}
                placeholder="e.g. PK0123456789ABCDEF"
                className="mt-1.5 w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 font-mono text-sm text-white placeholder-slate-600 outline-none focus:border-cyan-500/50 focus:ring-1 focus:ring-cyan-500/50"
              />
            </div>

            <div>
              <label className="block text-xs font-medium uppercase tracking-wider text-slate-400">
                Alpaca Secret Key
              </label>
              <div className="relative mt-1.5">
                <input
                  type={showSecret ? 'text' : 'password'}
                  required
                  value={inputSecretKey}
                  onChange={(e) => setInputSecretKey(e.target.value)}
                  placeholder="Your 40-character Alpaca paper secret key"
                  className="w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 pr-10 font-mono text-sm text-white placeholder-slate-600 outline-none focus:border-cyan-500/50 focus:ring-1 focus:ring-cyan-500/50"
                />
                <button
                  type="button"
                  onClick={() => setShowSecret(!showSecret)}
                  className="absolute right-3 top-3 text-slate-400 hover:text-white"
                  aria-label="Toggle secret visibility"
                >
                  {showSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
            </div>

            <button
              type="submit"
              disabled={connecting}
              className="mt-6 flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 via-sky-500 to-blue-600 py-3 text-sm font-bold text-slate-950 shadow-lg shadow-cyan-500/20 transition hover:brightness-110 disabled:opacity-50"
            >
              {connecting ? (
                <>
                  <Spinner className="h-4 w-4 text-slate-950" />
                  <span>Validating with Alpaca...</span>
                </>
              ) : (
                <>
                  <ShieldCheck className="h-4 w-4" />
                  <span>Connect to Alpaca</span>
                </>
              )}
            </button>
          </form>
        </GlassCard>
      </div>
    );
  }

  // -------------------------------------------------------------
  // RENDERING: 3. Connected Dashboard & TradingView-Style Studio
  // -------------------------------------------------------------
  return (
    <div className="space-y-6">
      {/* Header Bar */}
      <div className="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-bold tracking-tight text-white">Paper Trading & Execution</h1>
            <Badge tone="profit" className="flex items-center gap-1.5 text-[11px] font-semibold">
              <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
              Alpaca Paper Connected
            </Badge>
            {keyHint && (
              <span className="font-mono text-xs text-slate-400">
                Key: ••••{keyHint}
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-slate-400">
            Real-time direct paper order execution, interactive candlestick charts, live position tracking, and portfolio balances.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={loadAlpacaData}
            disabled={loadingData}
            className="flex items-center gap-1.5 rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-xs font-semibold text-slate-200 transition hover:bg-white/10"
            title="Refresh Account Data"
          >
            <RefreshCw className={cn('h-3.5 w-3.5', loadingData && 'animate-spin')} />
            Sync Alpaca
          </button>
          <button
            type="button"
            onClick={handleDisconnect}
            className="flex items-center gap-1.5 rounded-xl border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-xs font-semibold text-rose-400 transition hover:bg-rose-500/20"
            title="Disconnect Credentials"
          >
            <Lock className="h-3.5 w-3.5" />
            Disconnect
          </button>
        </div>
      </div>

      {/* Status Messages */}
      {errorMsg && <ErrorNote>{errorMsg}</ErrorNote>}
      {successMsg && (
        <div className="flex items-center gap-2 rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-xs text-emerald-400">
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{successMsg}</span>
        </div>
      )}

      {/* Top Portfolio Metric Cards */}
      <div data-tour="paper-account" className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">Total Equity <InfoTip metric="total_equity" className="text-slate-500" /></span>
            <Wallet className="h-4 w-4 text-cyan-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(totalEquity)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">
            Account #{account?.account_number || 'Paper'}
          </p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">Cash Balance <InfoTip metric="cash_balance" className="text-slate-500" /></span>
            <DollarSign className="h-4 w-4 text-emerald-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(cashBalance)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">Available settle funds</p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">Buying Power <InfoTip metric="buying_power" className="text-slate-500" /></span>
            <Activity className="h-4 w-4 text-purple-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(buyingPower)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">Margin & Daytrading power</p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">Unrealized P&L <InfoTip metric="unrealized_pnl" className="text-slate-500" /></span>
            {totalUnrealizedPl >= 0 ? (
              <TrendingUp className="h-4 w-4 text-emerald-400" />
            ) : (
              <TrendingDown className="h-4 w-4 text-rose-400" />
            )}
          </div>
          <p
            className={cn(
              'mt-2 font-mono text-2xl font-bold',
              totalUnrealizedPl >= 0 ? 'text-emerald-400' : 'text-rose-400'
            )}
          >
            {totalUnrealizedPl >= 0 ? '+' : ''}
            {formatCurrency(totalUnrealizedPl)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">Across {positions.length} active positions</p>
        </GlassCard>
      </div>

      {/* Main TradingView-Style 3-Column Dashboard */}
      <div className="grid gap-4 lg:grid-cols-[240px_1fr_320px]">
        {/* LEFT PANEL: Stock Search & Watchlist */}
        <div data-tour="paper-search" className="flex flex-col rounded-2xl border border-white/10 bg-[#090d14] p-3 text-white">
          <div className="mb-2 font-mono text-[11px] uppercase tracking-wider text-slate-400">
            Symbol Search
          </div>
          <div className="relative mb-3">
            <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-slate-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search ticker..."
              className="w-full rounded-xl border border-white/10 bg-white/5 py-1.5 pl-8 pr-3 font-mono text-xs text-white placeholder-slate-500 outline-none focus:border-cyan-500"
            />
          </div>

          {/* Search Results Dropdown */}
          {filteredAssets.length > 0 && (
            <div className="mb-3 max-h-40 overflow-y-auto rounded-xl border border-white/10 bg-slate-900/90 p-1">
              {filteredAssets.map((a) => (
                <div
                  key={a.id}
                  className="flex items-center justify-between rounded-lg px-2 py-1 text-xs hover:bg-white/10"
                >
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedSymbol(a.symbol);
                      setSearchQuery('');
                    }}
                    className="flex flex-col text-left truncate flex-1"
                  >
                    <span className="font-mono font-bold text-cyan-400">{a.symbol}</span>
                    <span className="truncate text-[10px] text-slate-400">{a.name}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => addToWatchlist(a.symbol)}
                    className="ml-2 text-slate-400 hover:text-white"
                    title="Add to Watchlist"
                  >
                    <Plus className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* Watchlist Header */}
          <div className="flex items-center justify-between border-b border-white/10 pb-1.5 font-mono text-[11px] uppercase tracking-wider text-slate-400">
            <span>Watchlist</span>
            <span>{watchlist.length}</span>
          </div>

          {/* Watchlist Items */}
          <div className="mt-2 space-y-1 overflow-y-auto max-h-[380px]">
            {watchlist.map((sym) => {
              const pos = positions.find((p) => p.symbol === sym);
              const isSelected = selectedSymbol === sym;
              return (
                <div
                  key={sym}
                  className={cn(
                    'group flex items-center justify-between rounded-xl px-2.5 py-2 text-xs transition cursor-pointer',
                    isSelected
                      ? 'bg-cyan-500/15 border border-cyan-500/30 text-white'
                      : 'hover:bg-white/5 text-slate-300'
                  )}
                  onClick={() => setSelectedSymbol(sym)}
                >
                  <div className="min-w-0">
                    <span className="font-mono font-bold">{sym}</span>
                    {pos && (
                      <span className="ml-1.5 rounded-full bg-emerald-500/20 px-1.5 py-0.2 text-[9px] text-emerald-300">
                        {pos.qty} sh
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    {pos && (
                      <span className="font-mono text-[11px] text-slate-300">
                        {formatCurrency(parseFloat(pos.current_price))}
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        removeFromWatchlist(sym);
                      }}
                      className="opacity-0 group-hover:opacity-100 text-slate-500 hover:text-rose-400"
                      title="Remove from watchlist"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* CENTER PANEL: Interactive Candlestick Chart */}
        <div data-tour="paper-chart" className="flex flex-col">
          <TradingChart
            symbol={selectedSymbol}
            bars={chartBars}
            currentPrice={currentSymbolPrice}
            timeframe={chartTimeframe}
            onTimeframeChange={(tf) => setChartTimeframe(tf)}
            dataStatus={chartDataStatus}
            loading={chartLoading}
            onRefresh={() => loadBars(selectedSymbol, chartTimeframe)}
            className="flex-1 min-h-[460px]"
          />
        </div>

        {/* RIGHT PANEL: Order Ticket */}
        <div data-tour="paper-ticket" className="flex flex-col rounded-2xl border border-white/10 bg-[#090d14] p-4 text-white">
          <div className="mb-3 font-mono text-[11px] uppercase tracking-wider text-slate-400">
            Order Ticket · {selectedSymbol}
          </div>

          {/* Buy / Sell Toggle */}
          <div className="grid grid-cols-2 gap-2 rounded-xl bg-white/5 p-1 mb-4">
            <button
              type="button"
              onClick={() => setOrderSide('buy')}
              className={cn(
                'rounded-lg py-1.5 text-xs font-bold transition',
                orderSide === 'buy'
                  ? 'bg-emerald-500 text-slate-950 shadow-md shadow-emerald-500/20'
                  : 'text-slate-400 hover:text-white'
              )}
            >
              Buy
            </button>
            <button
              type="button"
              onClick={() => setOrderSide('sell')}
              className={cn(
                'rounded-lg py-1.5 text-xs font-bold transition',
                orderSide === 'sell'
                  ? 'bg-rose-500 text-white shadow-md shadow-rose-500/20'
                  : 'text-slate-400 hover:text-white'
              )}
            >
              Sell
            </button>
          </div>

          {/* Order Type: Market vs Limit */}
          <div className="mb-4">
            <label className="block text-[11px] font-medium text-slate-400 mb-1">Order Type</label>
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => setOrderType('market')}
                className={cn(
                  'rounded-lg border px-3 py-1.5 text-xs font-medium transition',
                  orderType === 'market'
                    ? 'border-cyan-500/50 bg-cyan-500/10 text-cyan-400'
                    : 'border-white/10 bg-white/5 text-slate-400 hover:text-white'
                )}
              >
                Market
              </button>
              <button
                type="button"
                onClick={() => setOrderType('limit')}
                className={cn(
                  'rounded-lg border px-3 py-1.5 text-xs font-medium transition',
                  orderType === 'limit'
                    ? 'border-cyan-500/50 bg-cyan-500/10 text-cyan-400'
                    : 'border-white/10 bg-white/5 text-slate-400 hover:text-white'
                )}
              >
                Limit
              </button>
            </div>
          </div>

          {/* Quantity Input */}
          <div className="mb-4">
            <div className="flex justify-between items-center text-[11px] font-medium text-slate-400 mb-1">
              <span>Quantity (Shares)</span>
              <span>Est: {formatCurrency(estimatedOrderTotal)}</span>
            </div>
            <input
              type="number"
              min="1"
              step="1"
              value={orderQty}
              onChange={(e) => setOrderQty(e.target.value)}
              className="w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 font-mono text-sm text-white outline-none focus:border-cyan-500"
            />
            {/* Quick Quantity Buttons */}
            <div className="flex gap-1.5 mt-2">
              {['1', '5', '10', '25', '100'].map((q) => (
                <button
                  key={q}
                  type="button"
                  onClick={() => setOrderQty(q)}
                  className="flex-1 rounded-lg border border-white/10 bg-white/5 py-1 font-mono text-[11px] text-slate-300 hover:bg-white/10"
                >
                  +{q}
                </button>
              ))}
            </div>
          </div>

          {/* Limit Price Input */}
          {orderType === 'limit' && (
            <div className="mb-4">
              <label className="block text-[11px] font-medium text-slate-400 mb-1">Limit Price (USD)</label>
              <input
                type="number"
                min="0.01"
                step="0.01"
                value={limitPrice}
                onChange={(e) => setLimitPrice(e.target.value)}
                className="w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 font-mono text-sm text-white outline-none focus:border-cyan-500"
              />
            </div>
          )}

          {/* Order Summary Information */}
          <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 text-xs space-y-1.5 mb-4">
            <div className="flex justify-between text-slate-400">
              <span>Market Price:</span>
              <span className="font-mono text-white">{formatCurrency(currentSymbolPrice)}</span>
            </div>
            <div className="flex justify-between text-slate-400">
              <span>Buying Power:</span>
              <span className="font-mono text-white">{formatCurrency(buyingPower)}</span>
            </div>
            <div className="flex justify-between border-t border-white/10 pt-1.5 font-semibold">
              <span className="text-slate-300">Total Value:</span>
              <span className="font-mono text-cyan-400">{formatCurrency(estimatedOrderTotal)}</span>
            </div>
          </div>

          {/* Place Order Button */}
          <button
            type="button"
            onClick={() => setConfirmModalOpen(true)}
            disabled={submittingOrder || estimatedOrderTotal <= 0}
            className={cn(
              'w-full rounded-xl py-2.5 text-xs font-bold uppercase tracking-wider transition shadow-lg disabled:opacity-50',
              orderSide === 'buy'
                ? 'bg-emerald-500 text-slate-950 hover:bg-emerald-400 shadow-emerald-500/20'
                : 'bg-rose-500 text-white hover:bg-rose-400 shadow-rose-500/20'
            )}
          >
            {submittingOrder ? (
              <span className="flex items-center justify-center gap-1.5">
                <Spinner className="h-3.5 w-3.5 text-current" />
                Submitting to Alpaca...
              </span>
            ) : (
              `Review ${orderSide.toUpperCase()} ${orderQty} ${selectedSymbol}`
            )}
          </button>
        </div>
      </div>

      {/* Confirmation Modal */}
      {confirmModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-white/10 bg-[#0f172a] p-6 text-white shadow-2xl">
            <h3 className="text-lg font-bold">Confirm Paper Order</h3>
            <p className="mt-1 text-xs text-slate-400">
              This order will be submitted directly to your Alpaca paper trading account.
            </p>

            <div className="my-4 rounded-xl border border-white/10 bg-white/5 p-4 text-xs space-y-2 font-mono">
              <div className="flex justify-between">
                <span className="text-slate-400">Symbol:</span>
                <span className="font-bold text-white">{selectedSymbol}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Action:</span>
                <span className={orderSide === 'buy' ? 'text-emerald-400 font-bold' : 'text-rose-400 font-bold'}>
                  {orderSide.toUpperCase()}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Quantity:</span>
                <span className="text-white">{orderQty} shares</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Order Type:</span>
                <span className="text-white">{orderType.toUpperCase()}</span>
              </div>
              {orderType === 'limit' && (
                <div className="flex justify-between">
                  <span className="text-slate-400">Limit Price:</span>
                  <span className="text-white">{formatCurrency(parseFloat(limitPrice))}</span>
                </div>
              )}
              <div className="flex justify-between border-t border-white/10 pt-2 font-semibold text-sm">
                <span className="text-slate-300">Estimated Total:</span>
                <span className="text-cyan-400">{formatCurrency(estimatedOrderTotal)}</span>
              </div>
            </div>

            <div className="flex items-center justify-end gap-3">
              <button
                type="button"
                onClick={() => setConfirmModalOpen(false)}
                className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-xs font-semibold text-slate-300 hover:bg-white/10"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleOrderSubmit}
                disabled={submittingOrder}
                className={cn(
                  'rounded-xl px-4 py-2 text-xs font-bold text-slate-950 transition',
                  orderSide === 'buy' ? 'bg-emerald-400 hover:bg-emerald-300' : 'bg-rose-500 text-white hover:bg-rose-400'
                )}
              >
                Submit to Alpaca
              </button>
            </div>
          </div>
        </div>
      )}

      {/* BOTTOM PANEL: Positions, Open Orders, Order History, Performance */}
      <div data-tour="paper-positions" className="rounded-2xl border border-white/10 bg-[#090d14] p-4 text-white">
        {/* Tab Headers */}
        <div className="flex items-center gap-2 border-b border-white/10 pb-3">
          <button
            type="button"
            onClick={() => setBottomTab('positions')}
            className={cn(
              'rounded-xl px-3 py-1.5 text-xs font-semibold transition',
              bottomTab === 'positions'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-slate-400 hover:bg-white/5 hover:text-white'
            )}
          >
            Current Positions ({positions.length})
          </button>
          <button
            type="button"
            onClick={() => setBottomTab('open-orders')}
            className={cn(
              'rounded-xl px-3 py-1.5 text-xs font-semibold transition',
              bottomTab === 'open-orders'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-slate-400 hover:bg-white/5 hover:text-white'
            )}
          >
            Open Orders ({openOrders.length})
          </button>
          <button
            type="button"
            onClick={() => setBottomTab('history')}
            className={cn(
              'rounded-xl px-3 py-1.5 text-xs font-semibold transition',
              bottomTab === 'history'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-slate-400 hover:bg-white/5 hover:text-white'
            )}
          >
            Order History ({orders.length})
          </button>
          <button
            type="button"
            onClick={() => setBottomTab('performance')}
            className={cn(
              'rounded-xl px-3 py-1.5 text-xs font-semibold transition',
              bottomTab === 'performance'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-slate-400 hover:bg-white/5 hover:text-white'
            )}
          >
            Portfolio Metrics
          </button>
        </div>

        {/* Tab 1: Current Positions */}
        {bottomTab === 'positions' && (
          <div className="mt-4 overflow-x-auto">
            {positions.length === 0 ? (
              <div className="py-8 text-center text-xs text-slate-500">
                No open positions in your Alpaca paper trading account.
              </div>
            ) : (
              <table className="w-full text-left font-mono text-xs">
                <thead>
                  <tr className="border-b border-white/10 text-[11px] uppercase tracking-wider text-slate-400">
                    <th className="py-2">Symbol</th>
                    <th className="py-2">Side</th>
                    <th className="py-2 text-right">Shares</th>
                    <th className="py-2 text-right">Avg Entry</th>
                    <th className="py-2 text-right">Current Price</th>
                    <th className="py-2 text-right">Market Value</th>
                    <th className="py-2 text-right">
                      <span className="inline-flex items-center gap-1">Unrealized P&L <InfoTip metric="unrealized_pnl" className="text-slate-500" /></span>
                    </th>
                    <th className="py-2 text-center">Action</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {positions.map((pos) => {
                    const pl = parseFloat(pos.unrealized_pl || '0');
                    const plPct = parseFloat(pos.unrealized_plpc || '0') * 100;
                    return (
                      <tr key={pos.symbol} className="hover:bg-white/[0.02]">
                        <td className="py-2.5 font-bold text-white">{pos.symbol}</td>
                        <td className="py-2.5">
                          <span
                            className={cn(
                              'rounded px-1.5 py-0.5 text-[10px] font-bold uppercase',
                              pos.side === 'long'
                                ? 'bg-emerald-500/20 text-emerald-300'
                                : 'bg-rose-500/20 text-rose-300'
                            )}
                          >
                            {pos.side}
                          </span>
                        </td>
                        <td className="py-2.5 text-right text-slate-200">{pos.qty}</td>
                        <td className="py-2.5 text-right text-slate-200">
                          {formatCurrency(parseFloat(pos.avg_entry_price))}
                        </td>
                        <td className="py-2.5 text-right font-bold text-white">
                          {formatCurrency(parseFloat(pos.current_price))}
                        </td>
                        <td className="py-2.5 text-right text-slate-200">
                          {formatCurrency(parseFloat(pos.market_value))}
                        </td>
                        <td
                          className={cn(
                            'py-2.5 text-right font-semibold',
                            pl >= 0 ? 'text-emerald-400' : 'text-rose-400'
                          )}
                        >
                          {pl >= 0 ? '+' : ''}
                          {formatCurrency(pl)} ({plPct >= 0 ? '+' : ''}
                          {plPct.toFixed(2)}%)
                        </td>
                        <td className="py-2.5 text-center">
                          <button
                            type="button"
                            onClick={() => handleClosePosition(pos)}
                            className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-2.5 py-1 text-[11px] font-semibold text-rose-300 hover:bg-rose-500/20"
                          >
                            Close
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>
        )}

        {/* Tab 2: Open Orders */}
        {bottomTab === 'open-orders' && (
          <div className="mt-4 overflow-x-auto">
            {openOrders.length === 0 ? (
              <div className="py-8 text-center text-xs text-slate-500">
                No active open orders on Alpaca.
              </div>
            ) : (
              <table className="w-full text-left font-mono text-xs">
                <thead>
                  <tr className="border-b border-white/10 text-[11px] uppercase tracking-wider text-slate-400">
                    <th className="py-2">Time</th>
                    <th className="py-2">Symbol</th>
                    <th className="py-2">Side</th>
                    <th className="py-2">Type</th>
                    <th className="py-2 text-right">Qty</th>
                    <th className="py-2 text-right">Filled</th>
                    <th className="py-2 text-right">Limit Price</th>
                    <th className="py-2">
                      <span className="inline-flex items-center gap-1">Status <InfoTip metric="order_status" className="text-slate-500" /></span>
                    </th>
                    <th className="py-2 text-center">Action</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {openOrders.map((ord) => (
                    <tr key={ord.id} className="hover:bg-white/[0.02]">
                      <td className="py-2.5 text-slate-400">
                        {new Date(ord.created_at).toLocaleTimeString([], {
                          hour: '2-digit',
                          minute: '2-digit',
                        })}
                      </td>
                      <td className="py-2.5 font-bold text-white">{ord.symbol}</td>
                      <td className="py-2.5">
                        <span
                          className={cn(
                            'rounded px-1.5 py-0.5 text-[10px] font-bold uppercase',
                            ord.side === 'buy'
                              ? 'bg-emerald-500/20 text-emerald-300'
                              : 'bg-rose-500/20 text-rose-300'
                          )}
                        >
                          {ord.side}
                        </span>
                      </td>
                      <td className="py-2.5 uppercase text-slate-300">{ord.type}</td>
                      <td className="py-2.5 text-right text-slate-200">{ord.qty}</td>
                      <td className="py-2.5 text-right text-slate-400">{ord.filled_qty}</td>
                      <td className="py-2.5 text-right text-slate-200">
                        {ord.limit_price ? formatCurrency(parseFloat(ord.limit_price)) : '-'}
                      </td>
                      <td className="py-2.5">
                        <Badge tone="accent" className="text-[10px] uppercase">
                          {ord.status}
                        </Badge>
                      </td>
                      <td className="py-2.5 text-center">
                        <button
                          type="button"
                          onClick={() => handleCancelOrder(ord.id)}
                          className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-2 py-0.5 text-[10px] text-rose-300 hover:bg-rose-500/20"
                        >
                          Cancel
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}

        {/* Tab 3: Order History */}
        {bottomTab === 'history' && (
          <div className="mt-4 overflow-x-auto">
            {orders.length === 0 ? (
              <div className="py-8 text-center text-xs text-slate-500">
                No orders found on Alpaca paper account.
              </div>
            ) : (
              <table className="w-full text-left font-mono text-xs">
                <thead>
                  <tr className="border-b border-white/10 text-[11px] uppercase tracking-wider text-slate-400">
                    <th className="py-2">Date / Time</th>
                    <th className="py-2">Symbol</th>
                    <th className="py-2">Side</th>
                    <th className="py-2">Type</th>
                    <th className="py-2 text-right">Qty</th>
                    <th className="py-2 text-right">Avg Fill Price</th>
                    <th className="py-2">
                      <span className="inline-flex items-center gap-1">Status <InfoTip metric="order_status" className="text-slate-500" /></span>
                    </th>
                    <th className="py-2">Order ID</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {orders.map((ord) => (
                    <tr key={ord.id} className="hover:bg-white/[0.02]">
                      <td className="py-2.5 text-slate-400">
                        {new Date(ord.created_at).toLocaleString(undefined, {
                          month: 'numeric',
                          day: 'numeric',
                          hour: '2-digit',
                          minute: '2-digit',
                        })}
                      </td>
                      <td className="py-2.5 font-bold text-white">{ord.symbol}</td>
                      <td className="py-2.5">
                        <span
                          className={cn(
                            'rounded px-1.5 py-0.5 text-[10px] font-bold uppercase',
                            ord.side === 'buy'
                              ? 'bg-emerald-500/20 text-emerald-300'
                              : 'bg-rose-500/20 text-rose-300'
                          )}
                        >
                          {ord.side}
                        </span>
                      </td>
                      <td className="py-2.5 uppercase text-slate-300">{ord.type}</td>
                      <td className="py-2.5 text-right text-slate-200">
                        {ord.filled_qty}/{ord.qty}
                      </td>
                      <td className="py-2.5 text-right text-slate-200">
                        {ord.filled_avg_price ? formatCurrency(parseFloat(ord.filled_avg_price)) : '-'}
                      </td>
                      <td className="py-2.5">
                        <Badge
                          tone={
                            ord.status === 'filled'
                              ? 'profit'
                              : ord.status === 'canceled'
                              ? 'neutral'
                              : ord.status === 'rejected'
                              ? 'loss'
                              : 'accent'
                          }
                          className="text-[10px] uppercase"
                        >
                          {ord.status}
                        </Badge>
                      </td>
                      <td className="py-2.5 font-mono text-[11px] text-slate-500">
                        {ord.id.slice(0, 8)}…
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}

        {/* Tab 4: Performance & Account Metrics */}
        {bottomTab === 'performance' && (
          <div className="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-3 text-xs">
            <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 space-y-2">
              <span className="font-mono text-[10px] uppercase tracking-wider text-slate-400">
                Balances & Equity
              </span>
              <div className="flex justify-between">
                <span className="text-slate-400"><MetricLabel metric="total_equity">Portfolio Value</MetricLabel>:</span>
                <span className="font-mono text-white">{formatCurrency(totalEquity)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400"><MetricLabel metric="cash_balance">Cash Balance</MetricLabel>:</span>
                <span className="font-mono text-white">{formatCurrency(cashBalance)}</span>
              </div>
              <div className="flex justify-between">
                <MetricLabel metric="last_equity" className="text-slate-400">Last Equity:</MetricLabel>
                <span className="font-mono text-white">
                  {account?.last_equity ? formatCurrency(parseFloat(account.last_equity)) : '-'}
                </span>
              </div>
            </div>

            <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 space-y-2">
              <span className="font-mono text-[10px] uppercase tracking-wider text-slate-400">
                Margin & Buying Power
              </span>
              <div className="flex justify-between">
                <span className="inline-flex items-center gap-1 text-slate-400">RegT Buying Power: <InfoTip metric="buying_power" className="text-slate-500" /></span>
                <span className="font-mono text-white">
                  {account?.regt_buying_power ? formatCurrency(parseFloat(account.regt_buying_power)) : '-'}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Daytrading Power:</span>
                <span className="font-mono text-white">
                  {account?.daytrading_buying_power
                    ? formatCurrency(parseFloat(account.daytrading_buying_power))
                    : '-'}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Maintenance Margin:</span>
                <span className="font-mono text-white">
                  {account?.maintenance_margin
                    ? formatCurrency(parseFloat(account.maintenance_margin))
                    : '-'}
                </span>
              </div>
            </div>

            <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 space-y-2">
              <span className="font-mono text-[10px] uppercase tracking-wider text-slate-400">
                Regulatory & Trading Stats
              </span>
              <div className="flex justify-between">
                <span className="text-slate-400">Pattern Day Trader:</span>
                <span className="font-mono text-white">
                  {account?.pattern_day_trader ? 'Yes' : 'No'}
                </span>
              </div>
              <div className="flex justify-between">
                <MetricLabel metric="daytrade_count" className="text-slate-400">Daytrade Count:</MetricLabel>
                <span className="font-mono text-white">{account?.daytrade_count ?? 0} / 3</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Account Currency:</span>
                <span className="font-mono text-white">{account?.currency || 'USD'}</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
