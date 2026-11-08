'use client';

import React, { useState } from 'react';
import { Plus, Trash2, TrendingUp, X } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface ComparisonItem {
  symbol: string;
  color?: string;
  visible?: boolean;
}

export interface ComparisonModalProps {
  isOpen: boolean;
  onClose: () => void;
  baseSymbol: string;
  activeComparisons: ComparisonItem[];
  onAddComparison: (symbol: string, color?: string) => void;
  onRemoveComparison: (symbol: string) => void;
}

const COMPARISON_COLORS = [
  '#9C27B0', // Purple
  '#00BCD4', // Cyan
  '#FF9800', // Orange
  '#E91E63', // Pink
  '#4CAF50', // Green
  '#3F51B5', // Indigo
];

const SUGGESTED_SYMBOLS = ['SPY', 'QQQ', 'AAPL', 'MSFT', 'NVDA', 'TSLA', 'AMZN', 'GOOG'];

export function ComparisonModal({
  isOpen,
  onClose,
  baseSymbol,
  activeComparisons,
  onAddComparison,
  onRemoveComparison,
}: ComparisonModalProps) {
  const [inputSymbol, setInputSymbol] = useState('');
  const [selectedColor, setSelectedColor] = useState(COMPARISON_COLORS[0]);
  const [error, setError] = useState<string | null>(null);

  if (!isOpen) return null;

  const handleAdd = (symToAdd?: string) => {
    const sym = (symToAdd || inputSymbol).trim().toUpperCase();
    if (!sym) {
      setError('Please enter a symbol');
      return;
    }
    if (sym === baseSymbol.toUpperCase()) {
      setError(`Cannot compare ${sym} against itself`);
      return;
    }
    if (activeComparisons.some((c) => c.symbol.toUpperCase() === sym)) {
      setError(`${sym} is already being compared`);
      return;
    }

    onAddComparison(sym, selectedColor);
    setInputSymbol('');
    setError(null);
    // Cycle to next color for next comparison
    const nextIdx = (COMPARISON_COLORS.indexOf(selectedColor) + 1) % COMPARISON_COLORS.length;
    setSelectedColor(COMPARISON_COLORS[nextIdx]);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm animate-in fade-in duration-200"
      onClick={onClose}
    >
      <div
        className="relative w-full max-w-md rounded-2xl border border-ink/10 bg-panel p-6 shadow-2xl transition-all"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Symbol Comparison"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink/10 pb-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent/15 text-accent">
              <TrendingUp className="h-5 w-5" />
            </div>
            <div>
              <h2 className="text-base font-bold text-ink">Compare Symbols</h2>
              <p className="text-xs text-ink/60">
                Overlay normalized % returns against <span className="font-semibold text-ink">{baseSymbol}</span>
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-ink/50 hover:bg-ink/[0.08] hover:text-ink transition-colors"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Info Banner */}
        <div className="mt-4 rounded-xl border border-accent/20 bg-accent/5 p-3 text-xs text-ink/80">
          <span className="font-semibold text-accent">Normalized Basis:</span> Prices are rebased to 100.0 at the first bar of the full dataset, preserving stable relative performance without scroll jumps.
        </div>

        {/* Input Form */}
        <div className="mt-4 flex flex-col gap-3">
          <div className="flex gap-2">
            <input
              type="text"
              value={inputSymbol}
              onChange={(e) => {
                setInputSymbol(e.target.value.toUpperCase());
                setError(null);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  handleAdd();
                }
              }}
              placeholder="e.g. SPY, MSFT, QQQ..."
              className="flex-1 rounded-xl border border-ink/15 bg-canvas px-3.5 py-2 font-mono text-sm uppercase text-ink placeholder:normal-case placeholder:text-ink/40 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              autoFocus
            />
            <div className="flex items-center gap-1">
              {COMPARISON_COLORS.map((c) => (
                <button
                  key={c}
                  type="button"
                  onClick={() => setSelectedColor(c)}
                  className={cn(
                    'h-6 w-6 rounded-full border border-black/20 transition-all hover:scale-110',
                    selectedColor === c && 'ring-2 ring-accent ring-offset-2'
                  )}
                  style={{ backgroundColor: c }}
                  title={`Select color ${c}`}
                  aria-label={`Color ${c}`}
                />
              ))}
            </div>
            <button
              type="button"
              onClick={() => handleAdd()}
              className="flex items-center gap-1.5 rounded-xl bg-accent px-4 py-2 text-sm font-semibold text-white shadow-sm hover:opacity-95 transition-opacity"
            >
              <Plus className="h-4 w-4" />
              Add
            </button>
          </div>

          {error && <p className="text-xs font-medium text-rose-500">{error}</p>}

          {/* Quick Suggestions */}
          <div>
            <span className="text-[11px] font-medium text-ink/50 uppercase tracking-wider">Quick Suggestions:</span>
            <div className="mt-1.5 flex flex-wrap gap-1.5">
              {SUGGESTED_SYMBOLS.filter((s) => s !== baseSymbol && !activeComparisons.some((c) => c.symbol === s)).map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => handleAdd(s)}
                  className="rounded-lg border border-ink/10 bg-ink/[0.04] px-2.5 py-1 font-mono text-xs text-ink/80 hover:bg-ink/[0.08] hover:text-ink transition-colors"
                >
                  +{s}
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Active Comparisons List */}
        <div className="mt-5 border-t border-ink/10 pt-4">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-ink/60">
            Active Comparisons ({activeComparisons.length})
          </h3>
          {activeComparisons.length === 0 ? (
            <p className="mt-2 text-xs italic text-ink/40">No comparison symbols active</p>
          ) : (
            <div className="mt-2 flex flex-col gap-2 max-h-48 overflow-y-auto pr-1">
              {activeComparisons.map((c) => (
                <div
                  key={c.symbol}
                  className="flex items-center justify-between rounded-xl border border-ink/10 bg-canvas/60 px-3 py-2 text-sm"
                >
                  <div className="flex items-center gap-2.5">
                    <span
                      className="h-3 w-3 rounded-full"
                      style={{ backgroundColor: c.color || '#9C27B0' }}
                    />
                    <span className="font-mono font-bold text-ink">{c.symbol}</span>
                    <span className="text-xs text-ink/60">rebased to 100</span>
                  </div>
                  <button
                    type="button"
                    onClick={() => onRemoveComparison(c.symbol)}
                    className="rounded-lg p-1 text-ink/40 hover:bg-rose-500/10 hover:text-rose-500 transition-colors"
                    title={`Remove ${c.symbol}`}
                    aria-label={`Remove ${c.symbol}`}
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="mt-6 flex justify-end border-t border-ink/10 pt-4">
          <button
            type="button"
            onClick={onClose}
            className="rounded-xl border border-ink/15 bg-canvas px-4 py-2 text-xs font-semibold text-ink hover:bg-ink/[0.05] transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
