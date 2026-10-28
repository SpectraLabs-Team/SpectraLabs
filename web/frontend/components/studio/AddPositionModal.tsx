'use client';

import { useState, useMemo } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Search, Plus, X, TrendingUp, Sparkles, Check } from 'lucide-react';
import { CURATED_WATCHLIST_ITEMS, type CuratedAsset } from '@/lib/marketStatus';
import { formatCurrency, formatPercent } from '@/lib/utils';
import type { Position } from '@/lib/types';

interface AddPositionModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAdd: (position: Position) => void;
  existingPositions: Position[];
  currentNetValue: number;
}

export function AddPositionModal({
  isOpen,
  onClose,
  onAdd,
  existingPositions,
  currentNetValue,
}: AddPositionModalProps) {
  const [selectedAsset, setSelectedAsset] = useState<CuratedAsset | null>(CURATED_WATCHLIST_ITEMS[0]);
  const [customSymbol, setCustomSymbol] = useState('');
  const [quantity, setQuantity] = useState('100');
  const [price, setPrice] = useState(String(CURATED_WATCHLIST_ITEMS[0]?.defaultPrice ?? 100));
  const [volatility, setVolatility] = useState('0.20');
  const [search, setSearch] = useState('');
  const [category, setCategory] = useState<'all' | 'tech' | 'etfs' | 'volatility'>('all');

  const filteredAssets = useMemo(() => {
    return CURATED_WATCHLIST_ITEMS.filter((item) => {
      const matchCat = category === 'all' || item.category === category;
      const q = search.trim().toLowerCase();
      const matchSearch =
        !q || item.symbol.toLowerCase().includes(q) || item.name.toLowerCase().includes(q);
      return matchCat && matchSearch;
    });
  }, [category, search]);

  const selectAsset = (asset: CuratedAsset) => {
    setSelectedAsset(asset);
    setCustomSymbol(asset.symbol);
    setPrice(String(asset.defaultPrice));
  };

  const parsedQty = Math.max(1, parseFloat(quantity) || 100);
  const parsedPrice = Math.max(0.01, parseFloat(price) || 100);
  const positionValue = parsedQty * parsedPrice;
  const activeSymbol = customSymbol.trim().toUpperCase() || selectedAsset?.symbol || 'AAPL';
  const newProjectedValue = currentNetValue + positionValue;
  const projectedWeight = newProjectedValue > 0 ? positionValue / newProjectedValue : 0;
  const isAlreadyInPortfolio = existingPositions.some((p) => p.symbol.toUpperCase() === activeSymbol);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeSymbol) return;

    const vol = parseFloat(volatility);
    const newPos: Position = {
      symbol: activeSymbol,
      quantity: parsedQty,
      price: parsedPrice,
      volatility: Number.isFinite(vol) && vol > 0 ? vol : undefined,
    };
    onAdd(newPos);
    onClose();
  };

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onClose}
          className="fixed inset-0 bg-black/60 backdrop-blur-sm"
        />

        <motion.div
          role="dialog"
          aria-modal="true"
          aria-labelledby="add-position-title"
          initial={{ opacity: 0, scale: 0.95, y: 15 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 15 }}
          className="relative z-10 flex max-h-[90vh] w-full max-w-2xl flex-col rounded-2xl border border-ink/10 bg-panel shadow-2xl overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-ink/[0.08] px-6 py-4">
            <div className="flex items-center gap-2.5">
              <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent/10 text-accent">
                <Sparkles className="h-5 w-5" />
              </div>
              <div>
                <h2 id="add-position-title" className="text-base font-bold text-ink">
                  Add Position to Portfolio
                </h2>
                <p className="text-xs text-ink/50">
                  Select a stock or ETF to preview model performance and concentration before buying.
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg p-1.5 text-ink/40 transition hover:bg-ink/[0.06] hover:text-ink"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="flex flex-1 flex-col overflow-y-auto">
            <div className="grid gap-6 p-6 sm:grid-cols-2">
              {/* Left Column: Stock Selection */}
              <div className="flex flex-col space-y-3">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                    Stock / Asset Selector
                  </label>
                  <span className="text-[11px] text-ink/40">{filteredAssets.length} options</span>
                </div>

                {/* Search */}
                <div className="relative">
                  <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-ink/40" />
                  <input
                    type="text"
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder="Search by symbol or company..."
                    className="w-full rounded-xl border border-ink/10 bg-canvas/80 py-2 pl-9 pr-3 text-xs text-ink placeholder:text-ink/40 focus:border-accent/60 focus:outline-none"
                  />
                </div>

                {/* Categories */}
                <div className="flex gap-1.5">
                  {(['all', 'tech', 'etfs', 'volatility'] as const).map((cat) => (
                    <button
                      key={cat}
                      type="button"
                      onClick={() => setCategory(cat)}
                      className={`rounded-lg px-2.5 py-1 text-[11px] font-medium transition ${
                        category === cat
                          ? 'bg-accent/15 text-accent'
                          : 'bg-ink/[0.04] text-ink/60 hover:text-ink'
                      }`}
                    >
                      {cat === 'all'
                        ? 'All'
                        : cat === 'tech'
                        ? 'Tech'
                        : cat === 'etfs'
                        ? 'ETFs'
                        : 'High Beta'}
                    </button>
                  ))}
                </div>

                {/* Asset List */}
                <div className="max-h-52 space-y-1.5 overflow-y-auto rounded-xl border border-ink/[0.07] bg-canvas/40 p-1.5">
                  {filteredAssets.map((asset) => {
                    const isSelected = activeSymbol === asset.symbol;
                    return (
                      <button
                        key={asset.symbol}
                        type="button"
                        onClick={() => selectAsset(asset)}
                        className={`flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left text-xs transition ${
                          isSelected
                            ? 'bg-accent/15 text-ink'
                            : 'hover:bg-ink/[0.04] text-ink/75 hover:text-ink'
                        }`}
                      >
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5 font-bold">
                            <span>{asset.symbol}</span>
                            {isSelected && <Check className="h-3 w-3 text-accent" />}
                          </div>
                          <span className="block truncate text-[11px] text-ink/45">
                            {asset.name}
                          </span>
                        </div>
                        <div className="text-right">
                          <span className="block font-mono font-medium text-ink">
                            {formatCurrency(asset.defaultPrice)}
                          </span>
                          <span
                            className={`text-[10.5px] font-medium ${
                              asset.changePct >= 0 ? 'text-profit' : 'text-loss'
                            }`}
                          >
                            {asset.changePct >= 0 ? '+' : ''}
                            {asset.changePct.toFixed(2)}%
                          </span>
                        </div>
                      </button>
                    );
                  })}
                  {filteredAssets.length === 0 && (
                    <div className="p-4 text-center text-xs text-ink/45">
                      No stock matches. You can enter a custom symbol on the right.
                    </div>
                  )}
                </div>
              </div>

              {/* Right Column: Parameters & Performance Impact */}
              <div className="space-y-4">
                <div>
                  <label className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                    Symbol
                  </label>
                  <input
                    type="text"
                    value={customSymbol}
                    onChange={(e) => setCustomSymbol(e.target.value.toUpperCase())}
                    placeholder="e.g. AAPL, NVDA, SPY"
                    className="mt-1 w-full rounded-xl border border-ink/10 bg-canvas/80 px-3 py-2 font-mono text-sm font-semibold uppercase text-ink focus:border-accent/60 focus:outline-none"
                    required
                  />
                  {isAlreadyInPortfolio && (
                    <p className="mt-1 text-[11px] text-accent">
                      Note: {activeSymbol} already has an existing position in this portfolio.
                    </p>
                  )}
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                      Quantity
                    </label>
                    <input
                      type="number"
                      step="any"
                      min="1"
                      value={quantity}
                      onChange={(e) => setQuantity(e.target.value)}
                      className="mt-1 w-full rounded-xl border border-ink/10 bg-canvas/80 px-3 py-2 font-mono text-xs text-ink focus:border-accent/60 focus:outline-none"
                      required
                    />
                  </div>
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                      Price ($)
                    </label>
                    <input
                      type="number"
                      step="any"
                      min="0.01"
                      value={price}
                      onChange={(e) => setPrice(e.target.value)}
                      className="mt-1 w-full rounded-xl border border-ink/10 bg-canvas/80 px-3 py-2 font-mono text-xs text-ink focus:border-accent/60 focus:outline-none"
                      required
                    />
                  </div>
                </div>

                <div>
                  <label className="text-xs font-semibold uppercase tracking-wider text-ink/60">
                    Annualized Volatility (optional)
                  </label>
                  <input
                    type="number"
                    step="0.01"
                    min="0.01"
                    max="5.0"
                    value={volatility}
                    onChange={(e) => setVolatility(e.target.value)}
                    placeholder="0.20 (20% default)"
                    className="mt-1 w-full rounded-xl border border-ink/10 bg-canvas/80 px-3 py-2 font-mono text-xs text-ink focus:border-accent/60 focus:outline-none"
                  />
                  <span className="text-[10.5px] text-ink/40">
                    Used by the Risk Lab engine for closed-form VaR and stress testing.
                  </span>
                </div>

                {/* Instant Performance & Exposure Preview Card */}
                <div className="rounded-xl border border-accent/20 bg-accent/[0.04] p-3.5">
                  <div className="flex items-center gap-1.5 text-xs font-semibold text-accent">
                    <TrendingUp className="h-3.5 w-3.5" /> Performance & Exposure Preview
                  </div>
                  <dl className="mt-2.5 space-y-1.5 text-xs">
                    <div className="flex justify-between">
                      <dt className="text-ink/60">Position Market Value:</dt>
                      <dd className="font-mono font-medium text-ink">
                        {formatCurrency(positionValue)}
                      </dd>
                    </div>
                    <div className="flex justify-between">
                      <dt className="text-ink/60">New Portfolio Net Value:</dt>
                      <dd className="font-mono font-semibold text-ink">
                        {formatCurrency(newProjectedValue)}
                      </dd>
                    </div>
                    <div className="flex justify-between">
                      <dt className="text-ink/60">Projected Portfolio Weight:</dt>
                      <dd className="font-mono text-accent">
                        {formatPercent(projectedWeight, 1)}
                      </dd>
                    </div>
                  </dl>
                </div>
              </div>
            </div>

            {/* Footer actions */}
            <div className="flex items-center justify-end gap-3 border-t border-ink/[0.08] bg-canvas/40 px-6 py-4">
              <button
                type="button"
                onClick={onClose}
                className="rounded-xl border border-ink/15 px-4 py-2 text-xs font-semibold text-ink/70 transition hover:bg-ink/[0.04] hover:text-ink"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="inline-flex items-center gap-1.5 rounded-xl bg-accent px-5 py-2 text-xs font-bold text-onaccent shadow-glow transition hover:brightness-110 active:scale-95"
              >
                <Plus className="h-4 w-4" /> Add to Portfolio
              </button>
            </div>
          </form>
        </motion.div>
      </div>
    </AnimatePresence>
  );
}
