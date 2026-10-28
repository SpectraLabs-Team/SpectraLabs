'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Activity,
  AlertCircle,
  AlertTriangle,
  ArrowDownRight,
  ArrowRight,
  ArrowUpRight,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
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
  RotateCcw,
  Search,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Star,
  Trash2,
  TrendingDown,
  TrendingUp,
  Wallet,
  X,
  Zap,
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
import { useToast } from '@/components/ui/StudioToast';
import { cn, formatCurrency, formatPercent } from '@/lib/utils';
import { TradingChart } from './TradingChart';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';
import {
  CURATED_WATCHLIST_ITEMS,
  WATCHLIST_CATEGORY_LABELS,
  getMarketSession,
  getBaselineTickerPrice,
  type CuratedAsset,
} from '@/lib/marketStatus';

// Default initial watchlist symbols
const INITIAL_WATCHLIST = ['AAPL', 'MSFT', 'NVDA', 'TSLA', 'SPY', 'AMZN', 'GOOGL', 'META'];

interface SandboxState {
  cash: number;
  positions: AlpacaPosition[];
  orders: AlpacaOrder[];
}

const DEFAULT_SANDBOX: SandboxState = {
  cash: 100_000,
  positions: [
    {
      asset_id: 'sb-pos-1',
      symbol: 'AAPL',
      exchange: 'NASDAQ',
      asset_class: 'us_equity',
      avg_entry_price: '182.50',
      qty: '50',
      side: 'long',
      market_value: '9250.00',
      cost_basis: '9125.00',
      unrealized_pl: '125.00',
      unrealized_plpc: '0.0137',
      current_price: '185.00',
    },
    {
      asset_id: 'sb-pos-2',
      symbol: 'NVDA',
      exchange: 'NASDAQ',
      asset_class: 'us_equity',
      avg_entry_price: '175.00',
      qty: '20',
      side: 'long',
      market_value: '3600.00',
      cost_basis: '3500.00',
      unrealized_pl: '100.00',
      unrealized_plpc: '0.0285',
      current_price: '180.00',
    },
  ],
  orders: [
    {
      id: 'sb-ord-1',
      client_order_id: 'sl-demo-initial',
      created_at: new Date(Date.now() - 3600_000 * 4).toISOString(),
      symbol: 'AAPL',
      qty: '50',
      filled_qty: '50',
      filled_avg_price: '182.50',
      type: 'market',
      side: 'buy',
      time_in_force: 'day',
      status: 'filled',
    },
    {
      id: 'sb-ord-2',
      client_order_id: 'sl-demo-nvda',
      created_at: new Date(Date.now() - 3600_000 * 2).toISOString(),
      symbol: 'NVDA',
      qty: '20',
      filled_qty: '20',
      filled_avg_price: '175.00',
      type: 'market',
      side: 'buy',
      time_in_force: 'day',
      status: 'filled',
    },
  ],
};

type BottomTab = 'positions' | 'open-orders' | 'history' | 'performance';

export function PaperLab() {
  const { success, error: toastError, info } = useToast();

  // Mode: 'alpaca' (live credentials) vs 'sandbox' (simulated local execution)
  const [tradingMode, setTradingMode] = useState<'alpaca' | 'sandbox'>('sandbox');
  const [connectModalOpen, setConnectModalOpen] = useState(false);

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

  // Trading data state (Alpaca)
  const [account, setAccount] = useState<AlpacaAccount | null>(null);
  const [positions, setPositions] = useState<AlpacaPosition[]>([]);
  const [orders, setOrders] = useState<AlpacaOrder[]>([]);
  const [assets, setAssets] = useState<AlpacaAsset[]>([]);
  const [loadingData, setLoadingData] = useState(false);

  // Simulated Sandbox state
  const [sandboxCash, setSandboxCash] = useState<number>(DEFAULT_SANDBOX.cash);
  const [sandboxPositions, setSandboxPositions] = useState<AlpacaPosition[]>(DEFAULT_SANDBOX.positions);
  const [sandboxOrders, setSandboxOrders] = useState<AlpacaOrder[]>(DEFAULT_SANDBOX.orders);

  // Watchlist & Search state
  const [watchlist, setWatchlist] = useState<string[]>(INITIAL_WATCHLIST);
  const [watchlistCategory, setWatchlistCategory] = useState<'my' | 'tech' | 'etfs' | 'volatility'>('my');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedSymbol, setSelectedSymbol] = useState('AAPL');
  const [marketSession, setMarketSession] = useState(getMarketSession());

  // Chart data state
  const [chartBars, setChartBars] = useState<CandleBar[]>([]);
  const [chartTimeframe, setChartTimeframe] = useState('1Day');
  const [chartLoading, setChartLoading] = useState(false);
  const [chartDataStatus, setChartDataStatus] = useState('Alpaca Paper (IEX Feed)');

  // Upgraded Order Ticket state
  const [orderSide, setOrderSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<'market' | 'limit' | 'stop' | 'stop_limit'>('market');
  const [timeInForce, setTimeInForce] = useState<'day' | 'gtc'>('day');
  const [sizingMode, setSizingMode] = useState<'shares' | 'notional'>('shares');
  const [orderQty, setOrderQty] = useState('10');
  const [orderNotional, setOrderNotional] = useState('2000');
  const [limitPrice, setLimitPrice] = useState('180.00');
  const [stopPrice, setStopPrice] = useState('175.00');
  const [quickExecute, setQuickExecute] = useState(false);
  const [submittingOrder, setSubmittingOrder] = useState(false);
  const [confirmModalOpen, setConfirmModalOpen] = useState(false);

  // Bottom Tabs
  const [bottomTab, setBottomTab] = useState<BottomTab>('positions');

  // Live Market Session Clock (Eastern Time)
  useEffect(() => {
    const timer = setInterval(() => {
      setMarketSession(getMarketSession());
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  // Load Sandbox state from LocalStorage
  useEffect(() => {
    try {
      const raw = localStorage.getItem('spectra-paper-sandbox-v1');
      if (raw) {
        const d = JSON.parse(raw);
        if (typeof d.cash === 'number') setSandboxCash(d.cash);
        if (Array.isArray(d.positions)) setSandboxPositions(d.positions);
        if (Array.isArray(d.orders)) setSandboxOrders(d.orders);
      }
    } catch {
      // ignore
    }
  }, []);

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
      toastError('Alpaca Error', err instanceof Error ? err.message : 'Failed to retrieve Alpaca account data.');
    } finally {
      setLoadingData(false);
    }
  }, [toastError]);

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
        let price = getBaselineTickerPrice(sym);
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
    loadBars(selectedSymbol, chartTimeframe);
  }, [selectedSymbol, chartTimeframe, loadBars]);

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
      setTradingMode('alpaca');
      setConnectModalOpen(false);
      setKeyHint(res.keyIdHint || inputKeyId.slice(-4));
      setInputKeyId('');
      setInputSecretKey('');
      success('Connected to Alpaca Paper Trading', `Authenticated broker session · Key: ••••${res.keyIdHint || inputKeyId.slice(-4)}`);
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
      setTradingMode('sandbox');
      setAccount(null);
      setPositions([]);
      setOrders([]);
      setKeyHint('');
      info('Alpaca Disconnected', 'Switched to Spectra Simulated Paper Sandbox');
    } catch (err) {
      toastError('Disconnect Failed', err instanceof Error ? err.message : 'Failed to disconnect account.');
    }
  };

  // Reset Sandbox state
  const handleResetSandbox = () => {
    if (!confirm('Reset Sandbox to default $100,000 cash balance and starter positions?')) return;
    setSandboxCash(DEFAULT_SANDBOX.cash);
    setSandboxPositions(DEFAULT_SANDBOX.positions);
    setSandboxOrders(DEFAULT_SANDBOX.orders);
    try {
      localStorage.removeItem('spectra-paper-sandbox-v1');
    } catch {
      // ignore
    }
    info('Sandbox Reset', 'Restored initial $100,000 cash balance and demo book.');
  };

  // 6. Effective States (Sandbox vs Alpaca Live)
  const isSandbox = tradingMode === 'sandbox';
  const effectivePositions = isSandbox ? sandboxPositions : positions;
  const effectiveOrders = isSandbox ? sandboxOrders : orders;
  const effectiveCash = isSandbox ? sandboxCash : (account ? parseFloat(account.cash) : 0);
  const effectiveEquity = isSandbox
    ? sandboxCash + sandboxPositions.reduce((sum, p) => sum + parseFloat(p.market_value || '0'), 0)
    : (account ? parseFloat(account.equity) : 0);
  const effectiveBuyingPower = isSandbox ? sandboxCash * 2 : (account ? parseFloat(account.buying_power) : 0);

  // Filtered symbols from search (combines Alpaca assets and curated stocks)
  const filteredAssets = useMemo(() => {
    if (!searchQuery.trim()) return [];
    const query = searchQuery.toUpperCase().trim();
    const curatedMatches: AlpacaAsset[] = CURATED_WATCHLIST_ITEMS.filter(
      (c) => c.symbol.includes(query) || c.name.toUpperCase().includes(query)
    ).map((c) => ({
      id: `curated-${c.symbol}`,
      class: 'us_equity',
      symbol: c.symbol,
      name: c.name,
      exchange: 'US',
      asset_class: 'us_equity',
      status: 'active',
      tradable: true,
      marginable: true,
      shortable: true,
      fractionable: true,
    }));

    const alpacaMatches = assets.filter(
      (a) => a.tradable && (a.symbol.includes(query) || a.name.toUpperCase().includes(query))
    );

    const merged = [...curatedMatches];
    alpacaMatches.forEach((a) => {
      if (!merged.some((m) => m.symbol === a.symbol)) merged.push(a);
    });

    return merged.slice(0, 10);
  }, [assets, searchQuery]);

  // Current Price & Calculations
  const currentSymbolPrice = useMemo(() => {
    const pos = effectivePositions.find((p) => p.symbol === selectedSymbol);
    if (pos) return parseFloat(pos.current_price);
    if (chartBars.length > 0) return chartBars[chartBars.length - 1].c;
    return parseFloat(limitPrice) || getBaselineTickerPrice(selectedSymbol);
  }, [effectivePositions, selectedSymbol, chartBars, limitPrice]);

  // Sizing Calculations: Shares vs Notional ($)
  const effectiveShares = useMemo(() => {
    if (sizingMode === 'shares') {
      const q = parseFloat(orderQty);
      return !isNaN(q) && q > 0 ? q : 1;
    }
    const n = parseFloat(orderNotional);
    const px = currentSymbolPrice > 0 ? currentSymbolPrice : 1;
    return !isNaN(n) && n > 0 ? Math.max(1, Math.floor(n / px)) : 1;
  }, [sizingMode, orderQty, orderNotional, currentSymbolPrice]);

  const orderPrice = useMemo(() => {
    if (orderType === 'limit' || orderType === 'stop_limit') {
      return parseFloat(limitPrice) || currentSymbolPrice;
    }
    if (orderType === 'stop') {
      return parseFloat(stopPrice) || currentSymbolPrice;
    }
    return currentSymbolPrice;
  }, [orderType, limitPrice, stopPrice, currentSymbolPrice]);

  const slippageBps = orderType === 'market' ? 2.5 : 0.0;
  const slippageCost = Math.round(effectiveShares * orderPrice * (slippageBps / 10000) * 100) / 100;
  const estimatedOrderTotal = Math.round((effectiveShares * orderPrice + (orderSide === 'buy' ? slippageCost : -slippageCost)) * 100) / 100;

  const currentHeldPosition = effectivePositions.find((p) => p.symbol === selectedSymbol);
  const currentHeldQty = currentHeldPosition ? parseFloat(currentHeldPosition.qty) : 0;
  const postTradeQty = orderSide === 'buy' ? currentHeldQty + effectiveShares : currentHeldQty - effectiveShares;
  const postTradeBuyingPower = Math.max(0, effectiveBuyingPower + (orderSide === 'buy' ? -estimatedOrderTotal : estimatedOrderTotal));

  const totalUnrealizedPl = effectivePositions.reduce((sum, p) => sum + parseFloat(p.unrealized_pl || '0'), 0);
  const openOrders = effectiveOrders.filter((o) =>
    ['new', 'accepted', 'pending_new', 'partially_filled'].includes(o.status)
  );

  // 7. Handle Direct Order Placement
  const handleOrderSubmit = async () => {
    const qtyNum = effectiveShares;
    if (!selectedSymbol || isNaN(qtyNum) || qtyNum <= 0) {
      toastError('Invalid Order Size', 'Please specify a valid symbol and positive order quantity.');
      return;
    }

    const limitPx = (orderType === 'limit' || orderType === 'stop_limit') ? parseFloat(limitPrice) : undefined;
    if ((orderType === 'limit' || orderType === 'stop_limit') && (isNaN(limitPx!) || limitPx! <= 0)) {
      toastError('Invalid Limit Price', 'Please specify a valid limit price.');
      return;
    }

    const stopPx = (orderType === 'stop' || orderType === 'stop_limit') ? parseFloat(stopPrice) : undefined;
    if ((orderType === 'stop' || orderType === 'stop_limit') && (isNaN(stopPx!) || stopPx! <= 0)) {
      toastError('Invalid Stop Price', 'Please specify a valid stop trigger price.');
      return;
    }

    if (orderSide === 'buy' && estimatedOrderTotal > effectiveBuyingPower) {
      toastError(
        'Insufficient Buying Power',
        `Order total (${formatCurrency(estimatedOrderTotal)}) exceeds available buying power (${formatCurrency(effectiveBuyingPower)}).`
      );
      return;
    }

    if (orderSide === 'sell' && qtyNum > currentHeldQty) {
      toastError(
        'Short Sale Restricted',
        `Cannot sell ${qtyNum} ${selectedSymbol}; you currently hold ${currentHeldQty} shares.`
      );
      return;
    }

    setSubmittingOrder(true);
    setConfirmModalOpen(false);

    if (tradingMode === 'sandbox') {
      try {
        const slippageMultiplier = orderSide === 'buy' ? 1 + (slippageBps / 10000) : 1 - (slippageBps / 10000);
        const fillPrice = Math.round(orderPrice * slippageMultiplier * 100) / 100;
        const totalFillCost = Math.round(qtyNum * fillPrice * 100) / 100;

        const newOrder: AlpacaOrder = {
          id: `sb-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          client_order_id: `sl-${Date.now()}`,
          created_at: new Date().toISOString(),
          symbol: selectedSymbol,
          qty: String(qtyNum),
          filled_qty: String(qtyNum),
          filled_avg_price: String(fillPrice),
          type: orderType,
          side: orderSide,
          time_in_force: timeInForce,
          limit_price: limitPx ? String(limitPx) : undefined,
          stop_price: stopPx ? String(stopPx) : undefined,
          status: 'filled',
        };

        let nextCash = sandboxCash;
        let nextPositions = [...sandboxPositions];

        if (orderSide === 'buy') {
          nextCash -= totalFillCost;
          if (currentHeldPosition) {
            const newTotalQty = currentHeldQty + qtyNum;
            const newAvgEntry = ((currentHeldQty * parseFloat(currentHeldPosition.avg_entry_price)) + totalFillCost) / newTotalQty;
            nextPositions = nextPositions.map((p) =>
              p.symbol === selectedSymbol
                ? {
                    ...p,
                    qty: String(newTotalQty),
                    avg_entry_price: newAvgEntry.toFixed(2),
                    market_value: (newTotalQty * fillPrice).toFixed(2),
                    current_price: String(fillPrice),
                    unrealized_pl: ((fillPrice - newAvgEntry) * newTotalQty).toFixed(2),
                    unrealized_plpc: (((fillPrice - newAvgEntry) / newAvgEntry)).toFixed(4),
                  }
                : p
            );
          } else {
            nextPositions.push({
              asset_id: `sb-${selectedSymbol}`,
              symbol: selectedSymbol,
              exchange: 'NASDAQ',
              asset_class: 'us_equity',
              avg_entry_price: fillPrice.toFixed(2),
              qty: String(qtyNum),
              side: 'long',
              market_value: totalFillCost.toFixed(2),
              cost_basis: totalFillCost.toFixed(2),
              unrealized_pl: '0.00',
              unrealized_plpc: '0.0000',
              current_price: String(fillPrice),
            });
          }
        } else {
          // Sell
          nextCash += totalFillCost;
          const remainingQty = currentHeldQty - qtyNum;
          if (remainingQty <= 0) {
            nextPositions = nextPositions.filter((p) => p.symbol !== selectedSymbol);
          } else {
            nextPositions = nextPositions.map((p) =>
              p.symbol === selectedSymbol
                ? {
                    ...p,
                    qty: String(remainingQty),
                    market_value: (remainingQty * fillPrice).toFixed(2),
                    current_price: String(fillPrice),
                    unrealized_pl: ((fillPrice - parseFloat(p.avg_entry_price)) * remainingQty).toFixed(2),
                    unrealized_plpc: (((fillPrice - parseFloat(p.avg_entry_price)) / parseFloat(p.avg_entry_price))).toFixed(4),
                  }
                : p
            );
          }
        }

        setSandboxCash(nextCash);
        setSandboxPositions(nextPositions);
        const nextOrders = [newOrder, ...sandboxOrders];
        setSandboxOrders(nextOrders);

        try {
          localStorage.setItem('spectra-paper-sandbox-v1', JSON.stringify({ cash: nextCash, positions: nextPositions, orders: nextOrders }));
        } catch {
          // ignore
        }

        success(
          `Order Filled · ${orderSide.toUpperCase()} ${qtyNum} ${selectedSymbol}`,
          `Executed at ${formatCurrency(fillPrice)} (${orderType.toUpperCase()} · ${timeInForce.toUpperCase()})`
        );
      } catch (err) {
        toastError('Simulation Error', err instanceof Error ? err.message : 'Order simulation failed');
      } finally {
        setSubmittingOrder(false);
      }
      return;
    }

    // Alpaca Paper API mode
    try {
      const newOrder = await createAlpacaOrder({
        symbol: selectedSymbol,
        qty: qtyNum,
        side: orderSide,
        type: orderType,
        time_in_force: timeInForce,
        limit_price: limitPx,
        stop_price: stopPx,
      });

      success(
        `Alpaca Order Submitted · ${newOrder.side.toUpperCase()} ${newOrder.qty} ${newOrder.symbol}`,
        `Order ID: ${newOrder.id.slice(0, 8)} (${newOrder.status.toUpperCase()})`
      );
      await loadAlpacaData();
    } catch (err) {
      toastError('Alpaca Order Failed', err instanceof Error ? err.message : 'Order submission failed.');
    } finally {
      setSubmittingOrder(false);
    }
  };

  // 8. Handle Order Cancellation
  const handleCancelOrder = async (orderId: string) => {
    if (tradingMode === 'sandbox') {
      const nextOrders = sandboxOrders.map((o) => (o.id === orderId ? { ...o, status: 'canceled' as const } : o));
      setSandboxOrders(nextOrders);
      try {
        localStorage.setItem('spectra-paper-sandbox-v1', JSON.stringify({ cash: sandboxCash, positions: sandboxPositions, orders: nextOrders }));
      } catch {
        // ignore
      }
      info('Order Canceled', `Sandbox order ${orderId.slice(0, 8)} marked canceled.`);
      return;
    }

    try {
      await cancelAlpacaOrder(orderId);
      info('Order Canceled', `Order ${orderId.slice(0, 8)} cancellation sent to Alpaca.`);
      await loadAlpacaData();
    } catch (err) {
      toastError('Cancel Failed', err instanceof Error ? err.message : 'Failed to cancel order on Alpaca.');
    }
  };

  // 9. Handle Position Close
  const handleClosePosition = async (pos: AlpacaPosition) => {
    const closeSide = pos.side === 'long' ? 'sell' : 'buy';
    const closeQty = Math.abs(parseFloat(pos.qty));
    if (!confirm(`Submit market order to ${closeSide.toUpperCase()} ${closeQty} ${pos.symbol} to close position?`)) {
      return;
    }

    if (tradingMode === 'sandbox') {
      const fillPrice = parseFloat(pos.current_price) || currentSymbolPrice;
      const proceeds = closeQty * fillPrice;
      const nextCash = sandboxCash + proceeds;
      const nextPositions = sandboxPositions.filter((p) => p.symbol !== pos.symbol);
      const newOrder: AlpacaOrder = {
        id: `sb-${Date.now()}`,
        client_order_id: `sl-close-${pos.symbol}`,
        created_at: new Date().toISOString(),
        symbol: pos.symbol,
        qty: String(closeQty),
        filled_qty: String(closeQty),
        filled_avg_price: String(fillPrice),
        type: 'market',
        side: closeSide,
        time_in_force: 'day',
        status: 'filled',
      };
      const nextOrders = [newOrder, ...sandboxOrders];
      setSandboxCash(nextCash);
      setSandboxPositions(nextPositions);
      setSandboxOrders(nextOrders);
      try {
        localStorage.setItem('spectra-paper-sandbox-v1', JSON.stringify({ cash: nextCash, positions: nextPositions, orders: nextOrders }));
      } catch {
        // ignore
      }
      success('Position Closed', `Liquidated ${closeQty} ${pos.symbol} at ${formatCurrency(fillPrice)}`);
      return;
    }

    try {
      await createAlpacaOrder({
        symbol: pos.symbol,
        qty: closeQty,
        side: closeSide,
        type: 'market',
      });
      success('Closing Order Submitted', `Submitted market order to close ${pos.symbol}`);
      await loadAlpacaData();
    } catch (err) {
      toastError('Close Failed', err instanceof Error ? err.message : `Failed to close position for ${pos.symbol}.`);
    }
  };

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

  return (
    <div className="space-y-6">
      {/* Header Bar */}
      <div className="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="text-2xl font-bold tracking-tight text-white">Paper Trading & Execution</h1>
            {isSandbox ? (
              <Badge tone="caution" className="flex items-center gap-1.5 text-[11px] font-semibold">
                <span className="h-2 w-2 rounded-full bg-amber-400 animate-pulse" />
                Spectra Paper Sandbox ($100k Demo)
              </Badge>
            ) : (
              <Badge tone="profit" className="flex items-center gap-1.5 text-[11px] font-semibold">
                <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
                Alpaca Paper Connected
                {keyHint && <span className="font-mono text-[10px] opacity-75">••••{keyHint}</span>}
              </Badge>
            )}

            {/* Live US Equities Market Session Indicator */}
            <div
              className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-[11px] font-medium text-slate-300"
              title={`${marketSession.description} (${marketSession.formattedTimeEt})`}
            >
              <span
                className={cn(
                  'h-2 w-2 rounded-full',
                  marketSession.badgeTone === 'profit' && 'bg-emerald-400 animate-pulse',
                  marketSession.badgeTone === 'caution' && 'bg-amber-400 animate-pulse',
                  marketSession.badgeTone === 'neutral' && 'bg-slate-400'
                )}
              />
              <span className="font-semibold text-white">{marketSession.label}</span>
              <span className="font-mono text-[10px] text-slate-400">{marketSession.formattedTimeEt}</span>
            </div>
          </div>
          <p className="mt-1 text-xs text-slate-400">
            Real-time direct paper order execution, interactive candlestick charts, live position tracking, and portfolio balances.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {/* Trading Environment Toggle */}
          <div className="inline-flex rounded-xl border border-white/10 bg-white/5 p-1 text-xs font-semibold">
            <button
              type="button"
              onClick={() => setTradingMode('sandbox')}
              className={cn(
                'rounded-lg px-3 py-1.5 transition',
                isSandbox ? 'bg-cyan-500/20 text-cyan-400 shadow-sm' : 'text-slate-400 hover:text-white'
              )}
            >
              Paper Sandbox
            </button>
            <button
              type="button"
              onClick={() => {
                if (isConnected) {
                  setTradingMode('alpaca');
                } else {
                  setConnectModalOpen(true);
                }
              }}
              className={cn(
                'rounded-lg px-3 py-1.5 transition',
                !isSandbox ? 'bg-emerald-500/20 text-emerald-400 shadow-sm' : 'text-slate-400 hover:text-white'
              )}
            >
              {isConnected ? 'Alpaca Broker' : 'Connect Alpaca'}
            </button>
          </div>

          {isSandbox ? (
            <>
              <button
                type="button"
                onClick={handleResetSandbox}
                className="flex items-center gap-1.5 rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-xs font-semibold text-slate-300 transition hover:bg-white/10"
                title="Reset Sandbox Balance"
              >
                <RotateCcw className="h-3.5 w-3.5" />
                Reset $100k
              </button>
              <button
                type="button"
                data-tour="paper-connect"
                onClick={() => setConnectModalOpen(true)}
                className="flex items-center gap-1.5 rounded-xl border border-cyan-500/30 bg-cyan-500/10 px-3 py-2 text-xs font-semibold text-cyan-400 transition hover:bg-cyan-500/20"
              >
                <Wallet className="h-3.5 w-3.5" />
                Connect Alpaca
              </button>
            </>
          ) : (
            <>
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
            </>
          )}
        </div>
      </div>

      {/* Top Portfolio Metric Cards */}
      <div data-tour="paper-account" className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">
              Total Equity <InfoTip metric="total_equity" className="text-slate-500" />
            </span>
            <Wallet className="h-4 w-4 text-cyan-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(effectiveEquity)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">
            {isSandbox ? 'Sandbox Portfolio Value' : `Account #${account?.account_number || 'Paper'}`}
          </p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">
              Cash Balance <InfoTip metric="cash_balance" className="text-slate-500" />
            </span>
            <DollarSign className="h-4 w-4 text-emerald-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(effectiveCash)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">Available settle funds</p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">
              Buying Power <InfoTip metric="buying_power" className="text-slate-500" />
            </span>
            <Activity className="h-4 w-4 text-purple-400" />
          </div>
          <p className="mt-2 font-mono text-2xl font-bold text-white">
            {formatCurrency(effectiveBuyingPower)}
          </p>
          <p className="mt-1 text-[11px] text-slate-400">Margin & Daytrading power</p>
        </GlassCard>

        <GlassCard className="p-4 border border-white/10 bg-[#0d131f]/70">
          <div className="flex items-center justify-between text-slate-400">
            <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-wider">
              Unrealized P&L <InfoTip metric="unrealized_pnl" className="text-slate-500" />
            </span>
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
          <p className="mt-1 text-[11px] text-slate-400">Across {effectivePositions.length} active positions</p>
        </GlassCard>
      </div>

      {/* Main TradingView-Style 3-Column Dashboard */}
      <div className="grid gap-4 lg:grid-cols-[240px_1fr_340px]">
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

          {/* Quick-Pick Watchlist Category Tabs */}
          <div className="mb-2 flex items-center justify-between border-b border-white/10 pb-1.5 font-mono text-[11px] uppercase tracking-wider text-slate-400">
            <div className="flex gap-1">
              {(['my', 'tech', 'etfs', 'volatility'] as const).map((cat) => (
                <button
                  key={cat}
                  type="button"
                  onClick={() => setWatchlistCategory(cat)}
                  className={cn(
                    'rounded-lg px-2 py-0.5 text-[10px] font-semibold transition',
                    watchlistCategory === cat
                      ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                      : 'text-slate-400 hover:text-white hover:bg-white/5'
                  )}
                >
                  {cat === 'my' ? `My (${watchlist.length})` : cat === 'tech' ? 'Tech' : cat === 'etfs' ? 'ETFs' : 'Beta'}
                </button>
              ))}
            </div>
          </div>

          {/* Watchlist Items */}
          <div className="mt-1 space-y-1 overflow-y-auto max-h-[380px]">
            {watchlistCategory === 'my' ? (
              watchlist.length === 0 ? (
                <div className="p-4 text-center text-xs text-slate-500">
                  <Star className="mx-auto h-5 w-5 text-slate-600 mb-1" />
                  Your watchlist is empty. Browse the tabs above or search to star assets.
                </div>
              ) : (
                watchlist.map((sym) => {
                  const pos = effectivePositions.find((p) => p.symbol === sym);
                  const curated = CURATED_WATCHLIST_ITEMS.find((c) => c.symbol === sym);
                  const isSelected = selectedSymbol === sym;
                  const price = pos ? parseFloat(pos.current_price) : curated?.defaultPrice ?? getBaselineTickerPrice(sym);
                  const chg = curated?.changePct ?? 0;

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
                      <div className="min-w-0 flex flex-col">
                        <div className="flex items-center">
                          <span className="font-mono font-bold">{sym}</span>
                          {pos && (
                            <span className="ml-1.5 rounded-full bg-emerald-500/20 px-1.5 py-0.2 text-[9px] text-emerald-300">
                              {pos.qty} sh
                            </span>
                          )}
                        </div>
                        {curated && (
                          <span className="truncate text-[10px] text-slate-400">{curated.name}</span>
                        )}
                      </div>
                      <div className="flex items-center gap-2">
                        <div className="text-right">
                          <p className="font-mono text-[11px] font-semibold text-slate-200">
                            {formatCurrency(price)}
                          </p>
                          {curated && (
                            <p className={cn(
                              'flex items-center justify-end font-mono text-[9.5px]',
                              chg >= 0 ? 'text-emerald-400' : 'text-rose-400'
                            )}>
                              {chg >= 0 ? <ArrowUpRight className="h-2.5 w-2.5 mr-0.5" /> : <ArrowDownRight className="h-2.5 w-2.5 mr-0.5" />}
                              {chg >= 0 ? '+' : ''}{chg.toFixed(2)}%
                            </p>
                          )}
                        </div>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            removeFromWatchlist(sym);
                          }}
                          className="opacity-0 group-hover:opacity-100 text-slate-500 hover:text-rose-400"
                          title="Remove from watchlist"
                        >
                          <X className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  );
                })
              )
            ) : (
              CURATED_WATCHLIST_ITEMS.filter((item) => item.category === watchlistCategory).map((item) => {
                const isSelected = selectedSymbol === item.symbol;
                const isStarred = watchlist.includes(item.symbol);
                const pos = effectivePositions.find((p) => p.symbol === item.symbol);
                const price = pos ? parseFloat(pos.current_price) : item.defaultPrice;

                return (
                  <div
                    key={item.symbol}
                    className={cn(
                      'group flex items-center justify-between rounded-xl px-2.5 py-2 text-xs transition cursor-pointer',
                      isSelected
                        ? 'bg-cyan-500/15 border border-cyan-500/30 text-white'
                        : 'hover:bg-white/5 text-slate-300'
                    )}
                    onClick={() => setSelectedSymbol(item.symbol)}
                  >
                    <div className="min-w-0 flex flex-col">
                      <div className="flex items-center">
                        <span className="font-mono font-bold">{item.symbol}</span>
                        {pos && (
                          <span className="ml-1.5 rounded-full bg-emerald-500/20 px-1.5 py-0.2 text-[9px] text-emerald-300">
                            {pos.qty} sh
                          </span>
                        )}
                      </div>
                      <span className="truncate text-[10px] text-slate-400">{item.name}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <div className="text-right">
                        <p className="font-mono text-[11px] font-semibold text-slate-200">
                          {formatCurrency(price)}
                        </p>
                        <p className={cn(
                          'flex items-center justify-end font-mono text-[9.5px]',
                          item.changePct >= 0 ? 'text-emerald-400' : 'text-rose-400'
                        )}>
                          {item.changePct >= 0 ? <ArrowUpRight className="h-2.5 w-2.5 mr-0.5" /> : <ArrowDownRight className="h-2.5 w-2.5 mr-0.5" />}
                          {item.changePct >= 0 ? '+' : ''}{item.changePct.toFixed(2)}%
                        </p>
                      </div>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (isStarred) {
                            removeFromWatchlist(item.symbol);
                          } else {
                            addToWatchlist(item.symbol);
                          }
                        }}
                        className={cn(
                          'p-0.5 rounded transition',
                          isStarred ? 'text-amber-400' : 'opacity-0 group-hover:opacity-100 text-slate-500 hover:text-amber-400'
                        )}
                        title={isStarred ? 'Remove from My Watchlist' : 'Add to My Watchlist'}
                      >
                        <Star className={cn('h-3.5 w-3.5', isStarred && 'fill-amber-400')} />
                      </button>
                    </div>
                  </div>
                );
              })
            )}
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

        {/* RIGHT PANEL: Institutional Order Ticket */}
        <div data-tour="paper-ticket" className="flex flex-col rounded-2xl border border-white/10 bg-[#090d14] p-4 text-white">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="font-mono text-[11px] uppercase tracking-wider text-slate-400">Order Ticket</span>
              <span className="rounded bg-white/10 px-1.5 py-0.5 font-mono text-[10px] font-bold text-accent">
                {selectedSymbol}
              </span>
            </div>
            <span className="font-mono text-[11px] font-bold text-slate-200">
              {formatCurrency(currentSymbolPrice)}
            </span>
          </div>

          {/* Mode Pill Indicator */}
          <div className="mb-3 flex items-center justify-between rounded-lg border border-white/5 bg-white/[0.02] px-2.5 py-1 text-[11px]">
            <span className="text-slate-400">Environment</span>
            <span className={cn('flex items-center gap-1 font-semibold', isSandbox ? 'text-amber-400' : 'text-emerald-400')}>
              <span className={cn('h-1.5 w-1.5 rounded-full', isSandbox ? 'bg-amber-400' : 'bg-emerald-400')} />
              {isSandbox ? 'Paper Sandbox' : 'Alpaca Broker'}
            </span>
          </div>

          {/* Buy / Sell Toggle */}
          <div className="grid grid-cols-2 gap-2 rounded-xl bg-white/5 p-1 mb-3">
            <button
              type="button"
              onClick={() => setOrderSide('buy')}
              className={cn(
                'rounded-lg py-2 text-xs font-bold uppercase tracking-wider transition',
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
                'rounded-lg py-2 text-xs font-bold uppercase tracking-wider transition',
                orderSide === 'sell'
                  ? 'bg-rose-500 text-white shadow-md shadow-rose-500/20'
                  : 'text-slate-400 hover:text-white'
              )}
            >
              Sell
            </button>
          </div>

          {/* Order Type: Market / Limit / Stop Loss / Stop Limit */}
          <div className="mb-3">
            <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-slate-400">
              <span>Order Type</span>
              <span className="text-[10px] text-slate-500 uppercase">{orderType}</span>
            </div>
            <div className="grid grid-cols-4 gap-1 rounded-xl bg-white/5 p-1 text-[11px]">
              {(['market', 'limit', 'stop', 'stop_limit'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setOrderType(t)}
                  className={cn(
                    'rounded-lg py-1 font-mono uppercase transition text-center',
                    orderType === t
                      ? 'bg-cyan-500/20 font-bold text-cyan-400 shadow-sm'
                      : 'text-slate-400 hover:text-white'
                  )}
                >
                  {t === 'stop_limit' ? 'Stop-L' : t === 'stop' ? 'Stop' : t}
                </button>
              ))}
            </div>
          </div>

          {/* Time in Force: Day vs GTC */}
          <div className="mb-3">
            <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-slate-400">
              <span>Time in Force</span>
              <span className="text-[10px] text-slate-500 uppercase">{timeInForce}</span>
            </div>
            <div className="grid grid-cols-2 gap-1 rounded-xl bg-white/5 p-1 text-[11px]">
              {(['day', 'gtc'] as const).map((tif) => (
                <button
                  key={tif}
                  type="button"
                  onClick={() => setTimeInForce(tif)}
                  className={cn(
                    'rounded-lg py-1 font-mono uppercase transition text-center',
                    timeInForce === tif
                      ? 'bg-white/15 font-bold text-white'
                      : 'text-slate-400 hover:text-white'
                  )}
                >
                  {tif === 'day' ? 'Day Order' : 'GTC (Till Cancel)'}
                </button>
              ))}
            </div>
          </div>

          {/* Sizing Mode: Shares vs Notional ($) */}
          <div className="mb-3">
            <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-slate-400">
              <span>Order Size</span>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setSizingMode('shares')}
                  className={cn('text-[10px] font-mono transition', sizingMode === 'shares' ? 'text-cyan-400 font-bold underline' : 'text-slate-500 hover:text-slate-300')}
                >
                  Shares
                </button>
                <span className="text-slate-600">|</span>
                <button
                  type="button"
                  onClick={() => setSizingMode('notional')}
                  className={cn('text-[10px] font-mono transition', sizingMode === 'notional' ? 'text-cyan-400 font-bold underline' : 'text-slate-500 hover:text-slate-300')}
                >
                  $ USD
                </button>
              </div>
            </div>

            {sizingMode === 'shares' ? (
              <div>
                <input
                  type="number"
                  min="1"
                  step="1"
                  value={orderQty}
                  onChange={(e) => setOrderQty(e.target.value)}
                  placeholder="Number of shares..."
                  className="w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 font-mono text-sm text-white outline-none focus:border-cyan-500"
                />
                {/* Quick Share Buttons */}
                <div className="mt-1.5 flex gap-1">
                  {orderSide === 'buy' ? (
                    ['1', '5', '10', '25', '100'].map((q) => (
                      <button
                        key={q}
                        type="button"
                        onClick={() => setOrderQty(q)}
                        className="flex-1 rounded-lg border border-white/10 bg-white/5 py-0.5 font-mono text-[10px] text-slate-300 hover:bg-white/10"
                      >
                        +{q}
                      </button>
                    ))
                  ) : (
                    [
                      { label: '25%', frac: 0.25 },
                      { label: '50%', frac: 0.5 },
                      { label: '75%', frac: 0.75 },
                      { label: '100%', frac: 1.0 },
                    ].map((btn) => (
                      <button
                        key={btn.label}
                        type="button"
                        onClick={() => setOrderQty(String(Math.max(1, Math.floor(currentHeldQty * btn.frac))))}
                        className="flex-1 rounded-lg border border-white/10 bg-white/5 py-0.5 font-mono text-[10px] text-slate-300 hover:bg-white/10"
                      >
                        {btn.label}
                      </button>
                    ))
                  )}
                </div>
              </div>
            ) : (
              <div>
                <div className="relative">
                  <span className="absolute left-3 top-2 font-mono text-sm text-slate-400">$</span>
                  <input
                    type="number"
                    min="1"
                    step="50"
                    value={orderNotional}
                    onChange={(e) => setOrderNotional(e.target.value)}
                    placeholder="Dollar amount..."
                    className="w-full rounded-xl border border-white/10 bg-white/5 pl-7 pr-3 py-2 font-mono text-sm text-white outline-none focus:border-cyan-500"
                  />
                </div>
                {/* Quick % of Buying Power buttons */}
                <div className="mt-1.5 flex gap-1">
                  {[
                    { label: '10%', frac: 0.1 },
                    { label: '25%', frac: 0.25 },
                    { label: '50%', frac: 0.5 },
                    { label: '100%', frac: 1.0 },
                  ].map((btn) => (
                    <button
                      key={btn.label}
                      type="button"
                      onClick={() => setOrderNotional(String(Math.floor(effectiveBuyingPower * btn.frac)))}
                      className="flex-1 rounded-lg border border-white/10 bg-white/5 py-0.5 font-mono text-[10px] text-slate-300 hover:bg-white/10"
                    >
                      {btn.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* Limit Price Input */}
          {(orderType === 'limit' || orderType === 'stop_limit') && (
            <div className="mb-3">
              <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-slate-400">
                <span>Limit Price (USD)</span>
                <span className="text-[10px] text-slate-500">Execution boundary</span>
              </div>
              <div className="flex gap-1.5">
                <input
                  type="number"
                  min="0.01"
                  step="0.01"
                  value={limitPrice}
                  onChange={(e) => setLimitPrice(e.target.value)}
                  className="w-full rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 font-mono text-sm text-white outline-none focus:border-cyan-500"
                />
                <button
                  type="button"
                  onClick={() => setLimitPrice(currentSymbolPrice.toFixed(2))}
                  className="shrink-0 rounded-xl border border-white/10 bg-white/5 px-2.5 py-1.5 font-mono text-[10px] text-cyan-400 hover:bg-white/10"
                  title="Snap to current price"
                >
                  Snap
                </button>
              </div>
            </div>
          )}

          {/* Stop Trigger Price Input */}
          {(orderType === 'stop' || orderType === 'stop_limit') && (
            <div className="mb-3">
              <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-slate-400">
                <span>Stop Trigger Price (USD)</span>
                <span className="text-[10px] text-rose-400">Loss / Trigger limit</span>
              </div>
              <input
                type="number"
                min="0.01"
                step="0.01"
                value={stopPrice}
                onChange={(e) => setStopPrice(e.target.value)}
                className="w-full rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 font-mono text-sm text-white outline-none focus:border-cyan-500"
              />
            </div>
          )}

          {/* Institutional Order Analytics & Impact Breakdown */}
          <div className="mb-4 space-y-1.5 rounded-xl border border-white/5 bg-white/[0.02] p-3 font-mono text-xs">
            <div className="flex justify-between text-slate-400">
              <span>Shares to Trade:</span>
              <span className="font-bold text-white">{effectiveShares} sh</span>
            </div>
            <div className="flex justify-between text-slate-400">
              <span>Est. Execution Price:</span>
              <span className="text-white">
                {formatCurrency(orderPrice)}
                {orderType === 'market' && <span className="ml-1 text-[10px] text-slate-500">(+2.5 bps slip)</span>}
              </span>
            </div>
            <div className="flex justify-between text-slate-400">
              <span>Est. Commission:</span>
              <span className="text-emerald-400">$0.00 (Zero)</span>
            </div>
            <div className="flex justify-between text-slate-400">
              <span>Current Position:</span>
              <span className="text-slate-300">
                {currentHeldQty} sh → <span className="text-white font-semibold">{postTradeQty} sh</span>
              </span>
            </div>
            <div className="flex justify-between border-t border-white/10 pt-1.5 font-semibold">
              <span className="text-slate-300">Estimated Total:</span>
              <span className="text-cyan-400 font-bold">{formatCurrency(estimatedOrderTotal)}</span>
            </div>
            <div className="flex justify-between text-[11px] text-slate-500">
              <span>Post-Trade Buying Power:</span>
              <span className="text-slate-400">{formatCurrency(postTradeBuyingPower)}</span>
            </div>
          </div>

          {/* Quick-Execute Checkbox */}
          <label className="mb-3 flex cursor-pointer items-center gap-2 text-[11px] text-slate-400">
            <input
              type="checkbox"
              checked={quickExecute}
              onChange={(e) => setQuickExecute(e.target.checked)}
              className="rounded border-white/20 bg-white/5 text-cyan-500 focus:ring-0"
            />
            <span>Instant 1-Click Fill (skip confirmation modal)</span>
          </label>

          {/* Place Order Button */}
          <button
            type="button"
            onClick={() => {
              if (quickExecute) {
                handleOrderSubmit();
              } else {
                setConfirmModalOpen(true);
              }
            }}
            disabled={submittingOrder || estimatedOrderTotal <= 0}
            className={cn(
              'w-full rounded-xl py-3 text-xs font-bold uppercase tracking-wider transition shadow-lg disabled:opacity-50',
              orderSide === 'buy'
                ? 'bg-emerald-500 text-slate-950 hover:bg-emerald-400 shadow-emerald-500/20'
                : 'bg-rose-500 text-white hover:bg-rose-400 shadow-rose-500/20'
            )}
          >
            {submittingOrder ? (
              <span className="flex items-center justify-center gap-1.5">
                <Spinner className="h-3.5 w-3.5 text-current" />
                Executing Order...
              </span>
            ) : (
              `${orderSide.toUpperCase()} ${effectiveShares} ${selectedSymbol} · ${formatCurrency(estimatedOrderTotal)}`
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
              {isSandbox
                ? 'This order will execute immediately in your simulated Spectra Paper Sandbox.'
                : 'This order will be submitted directly to your Alpaca paper trading account.'}
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
                <span className="text-white">{effectiveShares} shares</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Order Type:</span>
                <span className="text-white">{orderType.toUpperCase()} ({timeInForce.toUpperCase()})</span>
              </div>
              {(orderType === 'limit' || orderType === 'stop_limit') && (
                <div className="flex justify-between">
                  <span className="text-slate-400">Limit Price:</span>
                  <span className="text-white">{formatCurrency(parseFloat(limitPrice))}</span>
                </div>
              )}
              {(orderType === 'stop' || orderType === 'stop_limit') && (
                <div className="flex justify-between">
                  <span className="text-slate-400">Stop Trigger:</span>
                  <span className="text-white">{formatCurrency(parseFloat(stopPrice))}</span>
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
                {isSandbox ? 'Execute in Sandbox' : 'Submit to Alpaca'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Connect Alpaca Modal */}
      {connectModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div className="relative w-full max-w-lg rounded-2xl border border-white/10 bg-[#0f172a] p-6 text-white shadow-2xl">
            <button
              type="button"
              onClick={() => setConnectModalOpen(false)}
              className="absolute right-4 top-4 rounded p-1 text-slate-400 hover:text-white"
              aria-label="Close connect modal"
            >
              <X className="h-4 w-4" />
            </button>
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-cyan-500/10 text-cyan-400 border border-cyan-500/20">
                <Wallet className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-lg font-bold">Connect Alpaca Paper Trading</h3>
                <p className="text-xs text-slate-400">Workspace AES-256 encrypted credential storage</p>
              </div>
            </div>

            {connectError && (
              <div className="mt-4">
                <ErrorNote>{connectError}</ErrorNote>
              </div>
            )}

            <form onSubmit={handleConnect} className="mt-5 space-y-4">
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
                  className="mt-1.5 w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2 font-mono text-sm text-white placeholder-slate-600 outline-none focus:border-cyan-500/50"
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
                    className="w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2 pr-10 font-mono text-sm text-white placeholder-slate-600 outline-none focus:border-cyan-500/50"
                  />
                  <button
                    type="button"
                    onClick={() => setShowSecret(!showSecret)}
                    className="absolute right-3 top-2.5 text-slate-400 hover:text-white"
                  >
                    {showSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
              </div>

              <div className="flex items-center justify-end gap-3 pt-2">
                <button
                  type="button"
                  onClick={() => setConnectModalOpen(false)}
                  className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-xs font-semibold text-slate-300 hover:bg-white/10"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={connecting}
                  className="flex items-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 via-sky-500 to-blue-600 px-5 py-2 text-xs font-bold text-slate-950 shadow-md shadow-cyan-500/20 hover:brightness-110 disabled:opacity-50"
                >
                  {connecting ? <Spinner className="h-3.5 w-3.5 text-slate-950" /> : <ShieldCheck className="h-3.5 w-3.5" />}
                  <span>Connect Credentials</span>
                </button>
              </div>
            </form>
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
            Current Positions ({effectivePositions.length})
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
            Order History ({effectiveOrders.length})
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
            {effectivePositions.length === 0 ? (
              <div className="py-8 text-center text-xs text-slate-500">
                No open positions in your {isSandbox ? 'Paper Sandbox' : 'Alpaca paper'} account.
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
                      <span className="inline-flex items-center gap-1">
                        Unrealized P&L <InfoTip metric="unrealized_pnl" className="text-slate-500" />
                      </span>
                    </th>
                    <th className="py-2 text-center">Action</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {effectivePositions.map((pos) => {
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
                No active open orders.
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
                      <span className="inline-flex items-center gap-1">
                        Status <InfoTip metric="order_status" className="text-slate-500" />
                      </span>
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
            {effectiveOrders.length === 0 ? (
              <div className="py-8 text-center text-xs text-slate-500">
                No orders recorded yet.
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
                      <span className="inline-flex items-center gap-1">
                        Status <InfoTip metric="order_status" className="text-slate-500" />
                      </span>
                    </th>
                    <th className="py-2">Order ID</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {effectiveOrders.map((ord) => (
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
                <span className="font-mono text-white">{formatCurrency(effectiveEquity)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400"><MetricLabel metric="cash_balance">Cash Balance</MetricLabel>:</span>
                <span className="font-mono text-white">{formatCurrency(effectiveCash)}</span>
              </div>
              <div className="flex justify-between">
                <MetricLabel metric="last_equity" className="text-slate-400">Last Equity:</MetricLabel>
                <span className="font-mono text-white">
                  {account?.last_equity ? formatCurrency(parseFloat(account.last_equity)) : formatCurrency(effectiveEquity)}
                </span>
              </div>
            </div>

            <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 space-y-2">
              <span className="font-mono text-[10px] uppercase tracking-wider text-slate-400">
                Margin & Buying Power
              </span>
              <div className="flex justify-between">
                <span className="inline-flex items-center gap-1 text-slate-400">Buying Power: <InfoTip metric="buying_power" className="text-slate-500" /></span>
                <span className="font-mono text-white">
                  {formatCurrency(effectiveBuyingPower)}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Daytrading Power:</span>
                <span className="font-mono text-white">
                  {account?.daytrading_buying_power
                    ? formatCurrency(parseFloat(account.daytrading_buying_power))
                    : formatCurrency(effectiveBuyingPower * 2)}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Maintenance Margin:</span>
                <span className="font-mono text-white">
                  {account?.maintenance_margin
                    ? formatCurrency(parseFloat(account.maintenance_margin))
                    : '$0.00'}
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
