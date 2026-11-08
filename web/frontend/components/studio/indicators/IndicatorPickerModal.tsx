'use client';

import React, { useEffect, useMemo, useState } from 'react';
import {
  BarChart2,
  Check,
  ExternalLink,
  HelpCircle,
  Layers,
  Plus,
  Search,
  Sliders,
  X,
} from 'lucide-react';
import {
  CORE_INDICATOR_IDS,
  INDICATOR_REGISTRY,
  type IndicatorCategory,
  type IndicatorMeta,
  type IndicatorName,
} from '@/lib/indicators';

export interface IndicatorPickerModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectIndicator: (indicatorId: IndicatorName) => void;
  activeIndicatorIds?: string[];
}

type TabFilter = 'all' | 'overlay' | 'subpane';

export function IndicatorPickerModal({
  isOpen,
  onClose,
  onSelectIndicator,
  activeIndicatorIds = [],
}: IndicatorPickerModalProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedTab, setSelectedTab] = useState<TabFilter>('all');

  // Close on Escape key
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  // Reset search when modal opens
  useEffect(() => {
    if (isOpen) {
      setSearchQuery('');
      setSelectedTab('all');
    }
  }, [isOpen]);

  const allIndicators = useMemo(() => Object.values(INDICATOR_REGISTRY), []);

  const filteredIndicators = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return allIndicators.filter((ind) => {
      if (selectedTab !== 'all' && ind.category !== selectedTab) {
        return false;
      }
      if (!q) return true;
      return (
        ind.id.toLowerCase().includes(q) ||
        ind.name.toLowerCase().includes(q) ||
        ind.shortName.toLowerCase().includes(q) ||
        ind.description.toLowerCase().includes(q)
      );
    });
  }, [allIndicators, searchQuery, selectedTab]);

  const coreIndicators = useMemo(() => {
    return filteredIndicators.filter((ind) => CORE_INDICATOR_IDS.includes(ind.id));
  }, [filteredIndicators]);

  const otherIndicators = useMemo(() => {
    return filteredIndicators.filter((ind) => !CORE_INDICATOR_IDS.includes(ind.id));
  }, [filteredIndicators]);

  if (!isOpen) return null;

  const renderIndicatorRow = (meta: IndicatorMeta) => {
    const isActive = activeIndicatorIds.some(
      (id) => id.toUpperCase() === meta.id.toUpperCase()
    );

    return (
      <div
        key={meta.id}
        className="group flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-xl border border-ink/[0.08] bg-ink/[0.02] p-3.5 transition hover:border-accent/40 hover:bg-ink/[0.04]"
      >
        <div className="flex items-start gap-3 flex-1 min-w-0">
          <span
            className="mt-0.5 inline-block h-3.5 w-3.5 shrink-0 rounded-full border border-black/10 dark:border-white/20 shadow-sm"
            style={{ backgroundColor: meta.defaultColor }}
            title={`Default Color: ${meta.defaultColor}`}
          />
          <div className="flex-1 min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-semibold text-[13.5px] text-ink">{meta.name}</span>
              <span className="rounded bg-ink/[0.06] px-1.5 py-0.5 font-mono text-[10.5px] font-bold text-ink/75">
                {meta.shortName}
              </span>
              <span
                className={`rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider ${
                  meta.category === 'overlay'
                    ? 'border border-cyan-500/30 bg-cyan-500/10 text-cyan-600 dark:text-cyan-400'
                    : 'border border-purple-500/30 bg-purple-500/10 text-purple-600 dark:text-purple-400'
                }`}
              >
                {meta.category === 'overlay' ? 'Price Overlay' : 'Sub-Pane'}
              </span>
            </div>
            <p className="mt-1 text-[12px] text-ink/65 leading-relaxed">
              {meta.description}
            </p>
            <div className="mt-1.5 flex flex-wrap items-center gap-2 text-[11px] text-ink/50">
              <span className="font-mono">
                Params:{' '}
                {meta.params
                  .map((p) => `${p.name}=${meta.defaultParams[p.name] ?? p.default}`)
                  .join(', ')}
              </span>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-2 self-end sm:self-center shrink-0">
          <a
            href={`/learn/indicators/${meta.id.toLowerCase()}`}
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1 rounded-lg border border-ink/10 bg-panel px-2.5 py-1.5 text-[11px] font-medium text-ink/70 transition hover:bg-ink/[0.05] hover:text-ink"
            title="Open Indicator Documentation & Formula"
          >
            <HelpCircle className="h-3.5 w-3.5 text-accent" />
            <span className="hidden xs:inline">Docs</span>
          </a>

          <button
            type="button"
            onClick={() => {
              onSelectIndicator(meta.id);
              onClose();
            }}
            className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-semibold transition ${
              isActive
                ? 'border border-emerald-500/40 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
                : 'bg-accent text-white hover:bg-accent/90 shadow-sm'
            }`}
          >
            {isActive ? (
              <>
                <Check className="h-3.5 w-3.5" />
                <span>Added</span>
              </>
            ) : (
              <>
                <Plus className="h-3.5 w-3.5" />
                <span>Add</span>
              </>
            )}
          </button>
        </div>
      </div>
    );
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4 animate-in fade-in duration-150">
      <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-ink/10 bg-panel shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink/10 px-5 py-4">
          <div className="flex items-center gap-2.5">
            <div className="rounded-lg bg-accent/10 p-2 text-accent">
              <Sliders className="h-5 w-5" />
            </div>
            <div>
              <h2 className="text-base font-bold text-ink">Technical Indicators</h2>
              <p className="text-[12px] text-ink/60">
                Choose from 25 mathematical indicators & price overlays
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-ink/50 hover:bg-ink/[0.06] hover:text-ink transition"
            title="Close (Esc)"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Search & Category Filter Toolbar */}
        <div className="border-b border-ink/10 bg-ink/[0.02] p-4 flex flex-col sm:flex-row gap-3 items-stretch sm:items-center">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-ink/40" />
            <input
              type="text"
              autoFocus
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search by indicator name, symbol, or description..."
              className="w-full rounded-xl border border-ink/15 bg-canvas/80 pl-9 pr-8 py-2 text-[13px] text-ink placeholder:text-ink/40 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-ink/40 hover:text-ink"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>

          {/* Category Tabs */}
          <div className="flex items-center rounded-xl bg-ink/[0.05] p-1 shrink-0 self-center sm:self-auto">
            <button
              type="button"
              onClick={() => setSelectedTab('all')}
              className={`rounded-lg px-3 py-1.5 text-xs font-semibold transition ${
                selectedTab === 'all'
                  ? 'bg-panel text-accent shadow-sm'
                  : 'text-ink/60 hover:text-ink'
              }`}
            >
              All ({allIndicators.length})
            </button>
            <button
              type="button"
              onClick={() => setSelectedTab('overlay')}
              className={`flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-semibold transition ${
                selectedTab === 'overlay'
                  ? 'bg-panel text-accent shadow-sm'
                  : 'text-ink/60 hover:text-ink'
              }`}
            >
              <Layers className="h-3.5 w-3.5" />
              Overlays ({allIndicators.filter((i) => i.category === 'overlay').length})
            </button>
            <button
              type="button"
              onClick={() => setSelectedTab('subpane')}
              className={`flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-semibold transition ${
                selectedTab === 'subpane'
                  ? 'bg-panel text-accent shadow-sm'
                  : 'text-ink/60 hover:text-ink'
              }`}
            >
              <BarChart2 className="h-3.5 w-3.5" />
              Sub-Panes ({allIndicators.filter((i) => i.category === 'subpane').length})
            </button>
          </div>
        </div>

        {/* Scrollable Indicator List */}
        <div className="flex-1 overflow-y-auto p-5 space-y-5">
          {filteredIndicators.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Search className="h-10 w-10 text-ink/20 mb-2" />
              <p className="text-sm font-semibold text-ink">No indicators found</p>
              <p className="text-xs text-ink/50 mt-1 max-w-sm">
                No indicators match &ldquo;{searchQuery}&rdquo;. Try searching for SMA, EMA, RSI, MACD, or Bollinger.
              </p>
            </div>
          ) : searchQuery.trim() ? (
            <div className="space-y-2.5">
              <div className="text-[11px] font-bold uppercase tracking-wider text-ink/50">
                Matching Indicators ({filteredIndicators.length})
              </div>
              {filteredIndicators.map(renderIndicatorRow)}
            </div>
          ) : (
            <>
              {coreIndicators.length > 0 && (
                <div className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="text-[11.5px] font-bold uppercase tracking-wider text-accent">
                      Core Indicators (Recommended)
                    </span>
                    <span className="text-[11px] text-ink/50">
                      {coreIndicators.length} prioritized
                    </span>
                  </div>
                  {coreIndicators.map(renderIndicatorRow)}
                </div>
              )}

              {otherIndicators.length > 0 && (
                <div className="space-y-2.5 pt-2">
                  <div className="flex items-center justify-between border-t border-ink/10 pt-3">
                    <span className="text-[11.5px] font-bold uppercase tracking-wider text-ink/60">
                      Additional Indicators
                    </span>
                    <span className="text-[11px] text-ink/50">
                      {otherIndicators.length} available
                    </span>
                  </div>
                  {otherIndicators.map(renderIndicatorRow)}
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-ink/10 bg-ink/[0.02] px-5 py-3 text-[12px] text-ink/60">
          <span>
            {activeIndicatorIds.length} active on chart
          </span>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-ink/15 bg-panel px-3.5 py-1.5 text-xs font-semibold text-ink hover:bg-ink/[0.05] transition"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
