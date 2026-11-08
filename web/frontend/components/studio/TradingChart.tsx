'use client';

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  BarChart2,
  Calendar,
  Eye,
  EyeOff,
  HelpCircle,
  Maximize2,
  Minus,
  Plus,
  RefreshCw,
  RotateCcw,
  Settings,
  Sliders,
  TrendingDown,
  TrendingUp,
  X,
  Zap,
  LayoutTemplate,
} from 'lucide-react';
import { cn, formatCurrency, formatPercent } from '@/lib/utils';
import type { CandleBar } from '@/lib/alpaca';
import { useTheme } from '@/lib/theme';
import {
  INDICATOR_REGISTRY,
  computeIndicatorSeries,
  type IndicatorName,
} from '@/lib/indicators';
import {
  type ChartDrawing,
  type ChartDrawingPoint,
  type ChartDrawingStyle,
  type ChartTemplate,
  type DrawingToolType,
  FIBONACCI_LEVELS,
} from '@/lib/chart-drawings';
import { IndicatorPickerModal } from './indicators/IndicatorPickerModal';
import { IndicatorSettingsModal } from './indicators/IndicatorSettingsModal';
import { DrawingToolbar } from './charts/DrawingToolbar';
import { ComparisonModal } from './charts/ComparisonModal';
import { TemplateModal } from './charts/TemplateModal';

export type { ChartDrawing, DrawingToolType, ChartTemplate } from '@/lib/chart-drawings';

export interface ComparisonSeries {
  symbol: string;
  values: (number | null)[];
  color?: string;
  basePrice?: number;
}

export interface OverlaySeries {
  id?: string;
  indicator: IndicatorName | string;
  name?: string;
  color?: string;
  values?: (number | null)[];
  series?: Record<string, (number | null)[]>;
  visible?: boolean;
  params?: Record<string, number>;
}

export interface SubPaneSeries {
  id?: string;
  indicator: IndicatorName | string;
  name?: string;
  color?: string;
  values?: (number | null)[];
  series?: Record<string, (number | null)[]>;
  visible?: boolean;
  params?: Record<string, number>;
}

export interface TradingChartProps {
  symbol: string;
  bars: CandleBar[];
  currentPrice?: number;
  timeframe: string;
  onTimeframeChange: (tf: string) => void;
  dataStatus?: string;
  loading?: boolean;
  onRefresh?: () => void;
  className?: string;
  priceFlash?: 'up' | 'down' | null;
  overlays?: OverlaySeries[];
  subpanes?: SubPaneSeries[];
  onAddIndicator?: (indicator: IndicatorName) => void;
  onRemoveIndicator?: (idOrName: string) => void;
  onToggleVisibility?: (idOrName: string) => void;
  onUpdateParams?: (idOrName: string, params: Record<string, number>, color: string) => void;
  drawings?: ChartDrawing[];
  onDrawingsChange?: (drawings: ChartDrawing[]) => void;
  onSaveDrawing?: (drawing: Omit<ChartDrawing, 'id'>) => Promise<ChartDrawing | void>;
  onDeleteDrawing?: (id: string) => Promise<void>;
  comparisons?: ComparisonSeries[];
  onAddComparison?: (symbol: string, color?: string) => void;
  onRemoveComparison?: (symbol: string) => void;
  templates?: ChartTemplate[];
  onSaveTemplate?: (name: string) => Promise<void>;
  onLoadTemplate?: (template: ChartTemplate) => void;
  onDeleteTemplate?: (id: string) => Promise<void>;
  isViewer?: boolean;
}

function hexToRgba(hex: string, alpha: number): string {
  const clean = hex.replace('#', '');
  if (clean.length === 6) {
    const r = parseInt(clean.substring(0, 2), 16);
    const g = parseInt(clean.substring(2, 4), 16);
    const b = parseInt(clean.substring(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  return hex;
}

export const TIMEFRAMES = [
  { label: '1m', value: '1Min', description: '1 Minute' },
  { label: '5m', value: '5Min', description: '5 Minutes' },
  { label: '15m', value: '15Min', description: '15 Minutes' },
  { label: '1h', value: '1Hour', description: '1 Hour' },
  { label: '1D', value: '1Day', description: 'Daily' },
  { label: '1W', value: '1Week', description: 'Weekly' },
  { label: '1M', value: '1Month', description: 'Monthly' },
];

export function TradingChart({
  symbol,
  bars,
  currentPrice,
  timeframe,
  onTimeframeChange,
  dataStatus = 'Alpaca Paper (IEX Feed)',
  loading = false,
  onRefresh,
  className = '',
  priceFlash = null,
  overlays = [],
  subpanes = [],
  onAddIndicator,
  onRemoveIndicator,
  onToggleVisibility,
  onUpdateParams,
  drawings = [],
  onDrawingsChange,
  onSaveDrawing,
  onDeleteDrawing,
  comparisons = [],
  onAddComparison,
  onRemoveComparison,
  templates = [],
  onSaveTemplate,
  onLoadTemplate,
  onDeleteTemplate,
  isViewer = false,
}: TradingChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const plotRef = useRef<HTMLDivElement>(null);
  const { theme } = useTheme();
  const [plotSize, setPlotSize] = useState({ w: 0, h: 0 });

  // M36: Drawing & Comparison State
  const [activeDrawingTool, setActiveDrawingTool] = useState<DrawingToolType | null>(null);
  const [drawingColor, setDrawingColor] = useState<string>('#2962FF');
  const [localDrawings, setLocalDrawings] = useState<ChartDrawing[]>(drawings);
  const [selectedDrawingId, setSelectedDrawingId] = useState<string | null>(null);
  const [inProgressDrawing, setInProgressDrawing] = useState<{
    tool: DrawingToolType;
    points: ChartDrawingPoint[];
    style?: ChartDrawingStyle;
  } | null>(null);
  const [undoStack, setUndoStack] = useState<ChartDrawing[][]>([]);
  const [redoStack, setRedoStack] = useState<ChartDrawing[][]>([]);

  useEffect(() => {
    setLocalDrawings(drawings);
  }, [drawings]);

  // Comparison & Templates Modals State
  const [isComparisonModalOpen, setIsComparisonModalOpen] = useState(false);
  const [isTemplateModalOpen, setIsTemplateModalOpen] = useState(false);
  const [localComparisons, setLocalComparisons] = useState<ComparisonSeries[]>(comparisons);

  useEffect(() => {
    setLocalComparisons(comparisons);
  }, [comparisons]);

  // Undo / Redo helpers
  const pushToUndo = useCallback(
    (newDrawings: ChartDrawing[]) => {
      setUndoStack((prev) => [...prev.slice(-30), localDrawings]);
      setRedoStack([]);
      setLocalDrawings(newDrawings);
      onDrawingsChange?.(newDrawings);
    },
    [localDrawings, onDrawingsChange]
  );

  const handleUndo = useCallback(() => {
    if (undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    setUndoStack((prev) => prev.slice(0, -1));
    setRedoStack((prev) => [...prev, localDrawings]);
    setLocalDrawings(previous);
    onDrawingsChange?.(previous);
  }, [undoStack, localDrawings, onDrawingsChange]);

  const handleRedo = useCallback(() => {
    if (redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    setRedoStack((prev) => prev.slice(0, -1));
    setUndoStack((prev) => [...prev, localDrawings]);
    setLocalDrawings(next);
    onDrawingsChange?.(next);
  }, [redoStack, localDrawings, onDrawingsChange]);

  const handleDeleteSelected = useCallback(() => {
    if (!selectedDrawingId) return;
    const updated = localDrawings.filter((d) => d.id !== selectedDrawingId);
    pushToUndo(updated);
    onDeleteDrawing?.(selectedDrawingId);
    setSelectedDrawingId(null);
  }, [selectedDrawingId, localDrawings, pushToUndo, onDeleteDrawing]);

  const handleClearAll = useCallback(() => {
    if (localDrawings.length === 0) return;
    pushToUndo([]);
    setSelectedDrawingId(null);
  }, [localDrawings, pushToUndo]);

  const handleAddComparison = (sym: string, color?: string) => {
    if (onAddComparison) {
      onAddComparison(sym, color);
    } else {
      const newComp: ComparisonSeries = {
        symbol: sym,
        color: color || '#9C27B0',
        values: bars.map(() => null),
      };
      setLocalComparisons((prev) => [...prev, newComp]);
    }
  };

  const handleRemoveComparison = (sym: string) => {
    if (onRemoveComparison) {
      onRemoveComparison(sym);
    } else {
      setLocalComparisons((prev) => prev.filter((c) => c.symbol !== sym));
    }
  };

  const handleSaveTemplate = async (name: string) => {
    if (onSaveTemplate) {
      await onSaveTemplate(name);
    }
  };

  const handleApplyTemplate = (tmpl: ChartTemplate) => {
    if (onLoadTemplate) {
      onLoadTemplate(tmpl);
    } else {
      if (Array.isArray(tmpl.drawings)) {
        setLocalDrawings(tmpl.drawings);
      }
    }
  };

  const handleDeleteTemplate = async (id: string) => {
    if (onDeleteTemplate) {
      await onDeleteTemplate(id);
    }
  };

  // Internal indicator state
  const [internalOverlays, setInternalOverlays] = useState<OverlaySeries[]>(overlays);
  const [internalSubpanes, setInternalSubpanes] = useState<SubPaneSeries[]>(subpanes);

  useEffect(() => {
    setInternalOverlays(overlays);
  }, [overlays]);

  useEffect(() => {
    setInternalSubpanes(subpanes);
  }, [subpanes]);

  // Modals state
  const [isPickerOpen, setIsPickerOpen] = useState(false);
  const [settingsModalState, setSettingsModalState] = useState<{
    isOpen: boolean;
    indicatorId: IndicatorName | string;
    targetId: string;
    isOverlay: boolean;
    params: Record<string, number>;
    color: string;
  } | null>(null);

  const effectiveOverlays = internalOverlays;
  const effectiveSubpanes = internalSubpanes;
  const effectiveComps = comparisons.length > 0 ? comparisons : localComparisons;

  const handleSelectIndicator = useCallback(
    (indicatorId: IndicatorName) => {
      if (onAddIndicator) {
        onAddIndicator(indicatorId);
        return;
      }
      const meta = INDICATOR_REGISTRY[indicatorId];
      if (!meta) return;
      const instanceId = `${indicatorId}_${Date.now()}`;
      const defaultParams = { ...meta.defaultParams };
      const computed = computeIndicatorSeries(indicatorId, bars, defaultParams);

      if (meta.category === 'overlay') {
        const newOverlay: OverlaySeries = {
          id: instanceId,
          indicator: indicatorId,
          name: `${meta.shortName} (${Object.values(defaultParams).join(', ')})`,
          color: meta.defaultColor,
          visible: true,
          params: defaultParams,
          values: computed.values,
          series: computed.series,
        };
        setInternalOverlays((prev) => [...prev, newOverlay]);
      } else {
        const newSubpane: SubPaneSeries = {
          id: instanceId,
          indicator: indicatorId,
          name: `${meta.shortName} (${Object.values(defaultParams).join(', ')})`,
          color: meta.defaultColor,
          visible: true,
          params: defaultParams,
          values: computed.values,
          series: computed.series,
        };
        setInternalSubpanes((prev) => [...prev, newSubpane]);
      }
    },
    [bars, onAddIndicator]
  );

  const handleRemoveIndicator = useCallback(
    (idOrName: string) => {
      if (onRemoveIndicator) {
        onRemoveIndicator(idOrName);
        return;
      }
      setInternalOverlays((prev) => prev.filter((o) => (o.id || o.indicator) !== idOrName));
      setInternalSubpanes((prev) => prev.filter((s) => (s.id || s.indicator) !== idOrName));
    },
    [onRemoveIndicator]
  );

  const handleToggleVisibility = useCallback(
    (idOrName: string) => {
      if (onToggleVisibility) {
        onToggleVisibility(idOrName);
        return;
      }
      setInternalOverlays((prev) =>
        prev.map((o) =>
          (o.id || o.indicator) === idOrName ? { ...o, visible: o.visible === false } : o
        )
      );
      setInternalSubpanes((prev) =>
        prev.map((s) =>
          (s.id || s.indicator) === idOrName ? { ...s, visible: s.visible === false } : s
        )
      );
    },
    [onToggleVisibility]
  );

  const handleUpdateParams = useCallback(
    (targetId: string, newParams: Record<string, number>, newColor: string) => {
      if (onUpdateParams) {
        onUpdateParams(targetId, newParams, newColor);
        return;
      }
      setInternalOverlays((prev) =>
        prev.map((o) => {
          if ((o.id || o.indicator) === targetId) {
            const computed = computeIndicatorSeries(o.indicator, bars, newParams);
            const meta = INDICATOR_REGISTRY[o.indicator as IndicatorName];
            return {
              ...o,
              color: newColor,
              params: newParams,
              name: `${meta?.shortName || o.indicator} (${Object.values(newParams).join(', ')})`,
              values: computed.values,
              series: computed.series,
            };
          }
          return o;
        })
      );
      setInternalSubpanes((prev) =>
        prev.map((s) => {
          if ((s.id || s.indicator) === targetId) {
            const computed = computeIndicatorSeries(s.indicator, bars, newParams);
            const meta = INDICATOR_REGISTRY[s.indicator as IndicatorName];
            return {
              ...s,
              color: newColor,
              params: newParams,
              name: `${meta?.shortName || s.indicator} (${Object.values(newParams).join(', ')})`,
              values: computed.values,
              series: computed.series,
            };
          }
          return s;
        })
      );
    },
    [bars, onUpdateParams]
  );

  useEffect(() => {
    const el = plotRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const r = entry.contentRect;
      setPlotSize({ w: Math.floor(r.width), h: Math.floor(r.height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // View state: index window [startIdx, endIdx]
  const [viewWindow, setViewWindow] = useState<{ start: number; end: number }>({ start: 0, end: 0 });
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);
  const [crosshair, setCrosshair] = useState<{ x: number; y: number; price: number } | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; startIdx: number; endIdx: number } | null>(null);

  // Initialize or update view window when bars change
  useEffect(() => {
    if (bars.length > 0) {
      const visibleCount = Math.min(bars.length, 60);
      setViewWindow({
        start: Math.max(0, bars.length - visibleCount),
        end: bars.length - 1,
      });
    }
  }, [bars.length, symbol, timeframe]);

  // Derived price stats
  const activeBar = hoveredIdx !== null && bars[hoveredIdx] ? bars[hoveredIdx] : bars[bars.length - 1];
  const firstBar = bars[0];
  const latestPrice = currentPrice ?? (activeBar ? activeBar.c : 0);
  const priceChange = firstBar && activeBar ? activeBar.c - firstBar.o : 0;
  const pctChange = firstBar && firstBar.o > 0 ? (priceChange / firstBar.o) * 100 : 0;
  const isUp = priceChange >= 0;

  // Zoom handler
  const handleZoom = useCallback((direction: 'in' | 'out', factor = 0.15) => {
    if (bars.length === 0) return;
    setViewWindow((prev) => {
      const span = prev.end - prev.start + 1;
      const delta = Math.max(2, Math.round(span * factor));
      if (direction === 'in') {
        if (span <= 10) return prev;
        const newStart = Math.min(prev.end - 8, prev.start + Math.floor(delta / 2));
        const newEnd = Math.max(newStart + 8, prev.end - Math.ceil(delta / 2));
        return { start: newStart, end: newEnd };
      } else {
        const newStart = Math.max(0, prev.start - Math.floor(delta / 2));
        const newEnd = Math.min(bars.length - 1, prev.end + Math.ceil(delta / 2));
        return { start: newStart, end: newEnd };
      }
    });
  }, [bars.length]);

  const handleResetZoom = useCallback(() => {
    if (bars.length === 0) return;
    const visibleCount = Math.min(bars.length, 60);
    setViewWindow({
      start: Math.max(0, bars.length - visibleCount),
      end: bars.length - 1,
    });
  }, [bars.length]);

  // Canvas drawing effect
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = plotRef.current;
    if (!canvas || !container) return;

    const css = getComputedStyle(document.documentElement);
    const isDark = document.documentElement.classList.contains('dark') || theme === 'dark';
    const fallbackInk = isDark ? '241, 245, 249' : '15, 23, 42';
    const fallbackPanel = isDark ? '11, 15, 23' : '255, 255, 255';

    const getRawRgb = (n: string, fallback: string) => {
      try {
        const raw = css.getPropertyValue(`--${n}`).replace(/\/\*.*?\*\//g, '').replace(/;/g, '').trim();
        const parts = raw.split(/\s+/).filter(Boolean);
        if (parts.length === 3 && parts.every((p) => !isNaN(Number(p)))) {
          return parts.join(', ');
        }
      } catch {
        // fallback
      }
      return fallback;
    };

    const inkRgb = getRawRgb('ink', fallbackInk);
    const panelRgb = getRawRgb('panel', fallbackPanel);

    const inkA = (a: number) => `rgba(${inkRgb}, ${a})`;
    const panelBg = `rgb(${panelRgb})`;
    const inkSolid = `rgb(${inkRgb})`;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = container.getBoundingClientRect();
    const width = Math.floor(rect.width) || plotSize.w || 600;
    const height = Math.floor(rect.height) || plotSize.h || 360;

    if (width <= 0 || height <= 0) return;

    canvas.width = width * dpr;
    canvas.height = height * dpr;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;

    ctx.save();
    ctx.scale(dpr, dpr);

    // Layout dimensions
    const overlays = effectiveOverlays;
    const subpanes = effectiveSubpanes;
    const priceScaleWidth = 65;
    const timeScaleHeight = 24;
    const activeSubpanes = (subpanes || []).filter((sp) => sp.visible !== false);
    const subCount = activeSubpanes.length;
    const totalUsableHeight = height - timeScaleHeight;
    const subPaneHeight =
      subCount > 0 ? Math.min(110, Math.max(65, Math.floor((totalUsableHeight * 0.45) / subCount))) : 0;
    const totalSubHeight = subCount * subPaneHeight;
    const mainChartHeight = totalUsableHeight - totalSubHeight;
    const volumeHeight = Math.floor(mainChartHeight * 0.18);
    const chartHeight = totalUsableHeight;
    const candleChartHeight = mainChartHeight - volumeHeight - 10;
    const chartWidth = width - priceScaleWidth;

    // Clear background
    ctx.fillStyle = panelBg;
    ctx.fillRect(0, 0, width, height);

    if (bars.length === 0) {
      ctx.fillStyle = inkA(0.4);
      ctx.font = '13px sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText(loading ? 'Loading market chart...' : `No market bars for ${symbol}`, width / 2, height / 2);
      ctx.restore();
      return;
    }

    const { start, end } = viewWindow;
    const visibleBars = bars.slice(start, end + 1);
    const count = visibleBars.length;
    if (count === 0) {
      ctx.restore();
      return;
    }

    // Min and Max prices in visible range
    let minPrice = Infinity;
    let maxPrice = -Infinity;
    let maxVolume = 0;

    for (const b of visibleBars) {
      if (b.l < minPrice) minPrice = b.l;
      if (b.h > maxPrice) maxPrice = b.h;
      if (b.v > maxVolume) maxVolume = b.v;
    }

    // Expand price range to include visible overlay values
    if (overlays && overlays.length > 0) {
      for (const o of overlays) {
        if (o.visible === false) continue;
        if (o.values) {
          for (let i = 0; i < count; i++) {
            const v = o.values[start + i];
            if (v !== null && v !== undefined && !isNaN(v)) {
              if (v < minPrice) minPrice = v;
              if (v > maxPrice) maxPrice = v;
            }
          }
        }
        if (o.series) {
          for (const line of Object.values(o.series)) {
            for (let i = 0; i < count; i++) {
              const v = line[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                if (v < minPrice) minPrice = v;
                if (v > maxPrice) maxPrice = v;
              }
            }
          }
        }
      }
    }

    // Add padding to price range
    const priceRange = maxPrice - minPrice || 1;
    const paddedMin = Math.max(0.01, minPrice - priceRange * 0.05);
    const paddedMax = maxPrice + priceRange * 0.05;
    const fullPriceRange = paddedMax - paddedMin;

    const priceToY = (p: number) => {
      return candleChartHeight - ((p - paddedMin) / fullPriceRange) * candleChartHeight + 10;
    };

    const yToPrice = (y: number) => {
      const norm = (candleChartHeight + 10 - y) / candleChartHeight;
      return paddedMin + norm * fullPriceRange;
    };

    const minSlots = 30;
    const effectiveSlots = Math.max(count, minSlots);
    const candleSlotWidth = chartWidth / effectiveSlots;
    const candleWidth = Math.min(24, Math.max(2, candleSlotWidth));
    const bodyWidth = Math.max(1, Math.min(18, candleWidth * 0.74));
    const xOffset = count < minSlots ? Math.max(0, chartWidth - count * candleSlotWidth - 10) : 0;

    // Grid lines (horizontal price lines)
    const priceSteps = 6;
    ctx.strokeStyle = inkA(0.05);
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 4]);

    for (let i = 0; i <= priceSteps; i++) {
      const p = paddedMin + (fullPriceRange * i) / priceSteps;
      const y = Math.round(priceToY(p));
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(chartWidth, y);
      ctx.stroke();

      // Price scale text on right
      ctx.fillStyle = inkA(0.45);
      ctx.font = '10px monospace';
      ctx.textAlign = 'left';
      ctx.fillText(p.toFixed(2), chartWidth + 6, y + 3);
    }

    // Grid lines (vertical time lines)
    const timeStep = Math.max(1, Math.floor(count / 6));
    for (let i = 0; i < count; i += timeStep) {
      const x = Math.round(xOffset + i * candleSlotWidth + candleSlotWidth / 2);
      ctx.beginPath();
      ctx.moveTo(x, 10);
      ctx.lineTo(x, chartHeight);
      ctx.stroke();

      // Time label on bottom axis
      const bar = visibleBars[i];
      if (bar) {
        const d = new Date(bar.t);
        let label = '';
        if (timeframe.includes('Min') || timeframe.includes('Hour')) {
          label = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        } else {
          label = d.toLocaleDateString([], { month: 'short', day: 'numeric' });
        }
        ctx.fillStyle = inkA(0.45);
        ctx.font = '10px monospace';
        ctx.textAlign = 'center';
        ctx.fillText(label, x, height - 6);
      }
    }
    ctx.setLineDash([]);

    // Draw Volume Bars
    const volBaseY = mainChartHeight;
    const volHeightMax = volumeHeight - 10;

    for (let i = 0; i < count; i++) {
      const b = visibleBars[i];
      const x = xOffset + i * candleSlotWidth + (candleSlotWidth - bodyWidth) / 2;
      const isBarUp = b.c >= b.o;
      const vHeight = maxVolume > 0 ? (b.v / maxVolume) * volHeightMax : 0;
      const vY = volBaseY - vHeight;

      ctx.fillStyle = isBarUp ? 'rgba(16, 185, 129, 0.25)' : 'rgba(239, 68, 68, 0.25)';
      ctx.fillRect(x, vY, bodyWidth, vHeight);
    }

    // Draw Overlay Channel Fills / Cloud (behind candlesticks)
    if (overlays && overlays.length > 0) {
      for (const o of overlays) {
        if (o.visible === false) continue;
        const color = o.color || '#3B82F6';
        const ind = String(o.indicator).toUpperCase();

        if (ind === 'BB' || ind === 'KELTNER' || ind === 'DONCHIAN') {
          const upper = o.series?.upper;
          const lower = o.series?.lower;
          if (upper && lower) {
            ctx.fillStyle = hexToRgba(color, 0.08);
            ctx.beginPath();
            let hasPoint = false;
            for (let i = 0; i < count; i++) {
              const u = upper[start + i];
              if (u !== null && u !== undefined && !isNaN(u)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = priceToY(u);
                if (!hasPoint) {
                  ctx.moveTo(x, y);
                  hasPoint = true;
                } else {
                  ctx.lineTo(x, y);
                }
              }
            }
            if (hasPoint) {
              for (let i = count - 1; i >= 0; i--) {
                const l = lower[start + i];
                if (l !== null && l !== undefined && !isNaN(l)) {
                  const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                  const y = priceToY(l);
                  ctx.lineTo(x, y);
                }
              }
              ctx.closePath();
              ctx.fill();
            }
          }
        } else if (ind === 'ICHIMOKU') {
          const spanA = o.series?.senkou_span_a;
          const spanB = o.series?.senkou_span_b;
          if (spanA && spanB) {
            for (let i = 0; i < count - 1; i++) {
              const a0 = spanA[start + i];
              const b0 = spanB[start + i];
              const a1 = spanA[start + i + 1];
              const b1 = spanB[start + i + 1];
              if (
                a0 !== null &&
                b0 !== null &&
                a1 !== null &&
                b1 !== null &&
                a0 !== undefined &&
                b0 !== undefined &&
                a1 !== undefined &&
                b1 !== undefined
              ) {
                const x0 = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const x1 = xOffset + (i + 1) * candleSlotWidth + candleSlotWidth / 2;
                const yA0 = priceToY(a0);
                const yB0 = priceToY(b0);
                const yA1 = priceToY(a1);
                const yB1 = priceToY(b1);

                ctx.fillStyle = a0 >= b0 ? 'rgba(16, 185, 129, 0.12)' : 'rgba(239, 68, 68, 0.12)';
                ctx.beginPath();
                ctx.moveTo(x0, yA0);
                ctx.lineTo(x1, yA1);
                ctx.lineTo(x1, yB1);
                ctx.lineTo(x0, yB0);
                ctx.closePath();
                ctx.fill();
              }
            }
          }
        }
      }
    }

    // Draw Candlesticks (Wick + Body)
    for (let i = 0; i < count; i++) {
      const b = visibleBars[i];
      const isBarUp = b.c >= b.o;
      const candleColor = isBarUp ? '#10B981' : '#EF4444';
      const wickColor = isBarUp ? '#34D399' : '#F87171';

      const xCenter = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
      const xBody = xOffset + i * candleSlotWidth + (candleSlotWidth - bodyWidth) / 2;

      const yHigh = priceToY(b.h);
      const yLow = priceToY(b.l);
      const yOpen = priceToY(b.o);
      const yClose = priceToY(b.c);

      const yTop = Math.min(yOpen, yClose);
      const bHeight = Math.max(1.5, Math.abs(yClose - yOpen));

      // Draw wick
      ctx.strokeStyle = wickColor;
      ctx.lineWidth = 1.2;
      ctx.beginPath();
      ctx.moveTo(xCenter, yHigh);
      ctx.lineTo(xCenter, yTop);
      ctx.moveTo(xCenter, yTop + bHeight);
      ctx.lineTo(xCenter, yLow);
      ctx.stroke();

      // Draw body
      ctx.fillStyle = candleColor;
      ctx.fillRect(xBody, yTop, bodyWidth, bHeight);
    }

    // Draw Overlay Lines and Indicators (on top of candlesticks)
    if (overlays && overlays.length > 0) {
      for (const o of overlays) {
        if (o.visible === false) continue;
        const color = o.color || '#3B82F6';
        const ind = String(o.indicator).toUpperCase();

        if (ind === 'PSAR') {
          // Parabolic SAR dots
          const pts = o.values || o.series?.values;
          if (pts) {
            ctx.fillStyle = color;
            for (let i = 0; i < count; i++) {
              const v = pts[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = priceToY(v);
                ctx.beginPath();
                ctx.arc(x, y, 2.5, 0, Math.PI * 2);
                ctx.fill();
              }
            }
          }
        } else if (ind === 'BB' || ind === 'KELTNER' || ind === 'DONCHIAN') {
          // Multi-line bands
          const lines = [
            { key: 'middle', style: 'solid', width: 1.2, stroke: color },
            { key: 'upper', style: 'dashed', width: 1.0, stroke: color },
            { key: 'lower', style: 'dashed', width: 1.0, stroke: color },
          ];
          for (const line of lines) {
            const seriesData = o.series?.[line.key];
            if (!seriesData) continue;
            ctx.strokeStyle = line.stroke;
            ctx.lineWidth = line.width;
            if (line.style === 'dashed') {
              ctx.setLineDash([3, 3]);
            } else {
              ctx.setLineDash([]);
            }
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = seriesData[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = priceToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
            ctx.setLineDash([]);
          }
        } else if (ind === 'ICHIMOKU') {
          // Ichimoku 5 lines
          const ichiLines = [
            { key: 'tenkan_sen', stroke: '#3B82F6', width: 1.2, dash: [] },
            { key: 'kijun_sen', stroke: '#EF4444', width: 1.4, dash: [] },
            { key: 'senkou_span_a', stroke: '#10B981', width: 1.0, dash: [4, 2] },
            { key: 'senkou_span_b', stroke: '#F59E0B', width: 1.0, dash: [4, 2] },
            { key: 'chikou_span', stroke: '#8B5CF6', width: 1.0, dash: [2, 2] },
          ];
          for (const line of ichiLines) {
            const seriesData = o.series?.[line.key];
            if (!seriesData) continue;
            ctx.strokeStyle = line.stroke;
            ctx.lineWidth = line.width;
            ctx.setLineDash(line.dash);
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = seriesData[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = priceToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
            ctx.setLineDash([]);
          }
        } else {
          // Single-line overlay: SMA, EMA, WMA, DEMA, TEMA, VWAP, TYPICAL, MEDIAN
          const pts = o.values || o.series?.values;
          if (pts) {
            ctx.strokeStyle = color;
            ctx.lineWidth = 1.5;
            ctx.setLineDash([]);
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = pts[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = priceToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
          }
        }
      }
    }

    // Latest price dotted line
    const lastBar = visibleBars[visibleBars.length - 1];
    if (lastBar) {
      const latestY = priceToY(lastBar.c);
      const lineUp = lastBar.c >= lastBar.o;
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = lineUp ? '#10B981' : '#EF4444';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(0, latestY);
      ctx.lineTo(chartWidth, latestY);
      ctx.stroke();
      ctx.setLineDash([]);

      // Latest price pill on right scale
      ctx.fillStyle = lineUp ? '#10B981' : '#EF4444';
      ctx.fillRect(chartWidth + 1, latestY - 9, priceScaleWidth - 2, 18);
      ctx.fillStyle = '#FFFFFF';
      ctx.font = 'bold 10px monospace';
      ctx.textAlign = 'center';
      ctx.fillText(lastBar.c.toFixed(2), chartWidth + priceScaleWidth / 2, latestY + 3.5);
    }

    // Draw Crosshair
    if (crosshair && crosshair.x >= 0 && crosshair.x <= chartWidth && crosshair.y >= 0 && crosshair.y <= chartHeight) {
      ctx.setLineDash([2, 2]);
      ctx.strokeStyle = inkA(0.4);
      ctx.lineWidth = 1;

      // Vertical line
      ctx.beginPath();
      ctx.moveTo(crosshair.x, 0);
      ctx.lineTo(crosshair.x, chartHeight);
      ctx.stroke();

      // Horizontal line
      ctx.beginPath();
      ctx.moveTo(0, crosshair.y);
      ctx.lineTo(chartWidth, crosshair.y);
      ctx.stroke();
      ctx.setLineDash([]);

      // Crosshair price tag (only rendered within candlestick price chart area)
      if (crosshair.y <= candleChartHeight + 10) {
        const crossPrice = yToPrice(crosshair.y);
        ctx.fillStyle = inkSolid;
        ctx.fillRect(chartWidth + 1, crosshair.y - 9, priceScaleWidth - 2, 18);
        ctx.fillStyle = panelBg;
        ctx.font = '10px monospace';
        ctx.textAlign = 'center';
        ctx.fillText(crossPrice.toFixed(2), chartWidth + priceScaleWidth / 2, crosshair.y + 3.5);
      }

      // Crosshair time tag
      if (hoveredIdx !== null && visibleBars[hoveredIdx - start]) {
        const hoveredBar = visibleBars[hoveredIdx - start];
        const d = new Date(hoveredBar.t);
        const tagTime = d.toLocaleString([], {
          month: 'numeric',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        });
        ctx.fillStyle = inkSolid;
        ctx.fillRect(crosshair.x - 45, height - 20, 90, 18);
        ctx.fillStyle = panelBg;
        ctx.font = '10px monospace';
        ctx.textAlign = 'center';
        ctx.fillText(tagTime, crosshair.x, height - 7);
      }
    }

    // Draw Sub-Panes (RSI, MACD, Stochastic, ATR, ADX, OBV, CCI, Williams %R, MFI, CMF, etc.)
    if (activeSubpanes && activeSubpanes.length > 0) {
      activeSubpanes.forEach((sp, sIdx) => {
        const spTop = mainChartHeight + sIdx * subPaneHeight;
        const spBottom = spTop + subPaneHeight;
        const spInnerTop = spTop + 16;
        const spInnerBottom = spBottom - 6;
        const spInnerH = spInnerBottom - spInnerTop;

        // Sub-pane top border separator
        ctx.strokeStyle = inkA(0.12);
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(0, spTop);
        ctx.lineTo(width, spTop);
        ctx.stroke();

        // Determine min and max for this sub-pane in visible range
        let minVal = Infinity;
        let maxVal = -Infinity;
        const ind = String(sp.indicator).toUpperCase();

        if (ind === 'RSI' || ind === 'STOCH' || ind === 'MFI') {
          minVal = 0;
          maxVal = 100;
        } else if (ind === 'WILLR') {
          minVal = -100;
          maxVal = 0;
        } else if (ind === 'CMF') {
          minVal = -0.5;
          maxVal = 0.5;
        } else if (ind === 'MACD') {
          let absMax = 0.5;
          for (let i = 0; i < count; i++) {
            const idx = start + i;
            for (const key of ['macd', 'signal', 'histogram']) {
              const v = sp.series?.[key]?.[idx];
              if (v !== null && v !== undefined && !isNaN(v)) {
                if (Math.abs(v) > absMax) absMax = Math.abs(v);
              }
            }
          }
          minVal = -absMax * 1.15;
          maxVal = absMax * 1.15;
        } else if (ind === 'ATR') {
          // ATR is strictly positive volatility
          minVal = 0;
          if (sp.values) {
            for (let i = 0; i < count; i++) {
              const v = sp.values[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                if (v > maxVal) maxVal = v;
              }
            }
          }
          maxVal = (maxVal > 0 ? maxVal : 1) * 1.15;
        } else if (ind === 'OBV') {
          // On-Balance Volume auto-scaling
          if (sp.values) {
            for (let i = 0; i < count; i++) {
              const v = sp.values[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                if (v < minVal) minVal = v;
                if (v > maxVal) maxVal = v;
              }
            }
          }
          const span = maxVal - minVal || 1000;
          minVal -= span * 0.05;
          maxVal += span * 0.05;
        } else {
          if (sp.values) {
            for (let i = 0; i < count; i++) {
              const v = sp.values[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                if (v < minVal) minVal = v;
                if (v > maxVal) maxVal = v;
              }
            }
          }
          if (sp.series) {
            for (const line of Object.values(sp.series)) {
              for (let i = 0; i < count; i++) {
                const v = line[start + i];
                if (v !== null && v !== undefined && !isNaN(v)) {
                  if (v < minVal) minVal = v;
                  if (v > maxVal) maxVal = v;
                }
              }
            }
          }
          if (!isFinite(minVal) || !isFinite(maxVal)) {
            minVal = 0;
            maxVal = 100;
          }
          const valRange = maxVal - minVal || 1;
          minVal = minVal - valRange * 0.08;
          maxVal = maxVal + valRange * 0.08;
        }

        const spRange = maxVal - minVal || 1;
        const spValueToY = (v: number) => {
          return spInnerBottom - ((v - minVal) / spRange) * spInnerH;
        };
        const spYToValue = (y: number) => {
          const norm = (spInnerBottom - y) / spInnerH;
          return minVal + norm * spRange;
        };

        // Reference lines
        const drawRefLine = (refVal: number, labelText?: string) => {
          if (refVal < minVal || refVal > maxVal) return;
          const refY = Math.round(spValueToY(refVal));
          ctx.setLineDash([3, 3]);
          ctx.strokeStyle = inkA(0.12);
          ctx.beginPath();
          ctx.moveTo(0, refY);
          ctx.lineTo(chartWidth, refY);
          ctx.stroke();
          ctx.setLineDash([]);

          ctx.fillStyle = inkA(0.4);
          ctx.font = '9px monospace';
          ctx.textAlign = 'left';
          ctx.fillText(labelText || refVal.toFixed(0), chartWidth + 6, refY + 3);
        };

        if (ind === 'RSI') {
          drawRefLine(70);
          drawRefLine(30);
        } else if (ind === 'STOCH' || ind === 'MFI') {
          drawRefLine(80);
          drawRefLine(20);
        } else if (ind === 'WILLR') {
          drawRefLine(-20);
          drawRefLine(-80);
        } else if (ind === 'MACD' || ind === 'CMF' || ind === 'CCI') {
          drawRefLine(0);
        } else if (ind === 'ADX') {
          drawRefLine(25);
        }

        // Sub-pane top-left label
        const activeIdx = hoveredIdx !== null ? hoveredIdx : (bars.length > 0 ? bars.length - 1 : null);
        ctx.fillStyle = inkA(0.85);
        ctx.font = 'bold 10px monospace';
        ctx.textAlign = 'left';

        const label = sp.name || sp.indicator;
        let readout = label;
        if (activeIdx !== null) {
          if (sp.values) {
            const v = sp.values[activeIdx];
            readout += `: ${v !== null && v !== undefined ? v.toFixed(2) : '–'}`;
          } else if (sp.series) {
            const parts = Object.entries(sp.series).map(([k, line]) => {
              const v = line[activeIdx];
              return `${k}: ${v !== null && v !== undefined ? v.toFixed(2) : '–'}`;
            });
            readout += ` (${parts.join('  ')})`;
          }
        }
        ctx.fillText(readout, 8, spTop + 11);

        // Draw data elements
        if (ind === 'MACD') {
          // Histogram bars
          const hist = sp.series?.histogram;
          const zeroY = spValueToY(0);
          if (hist) {
            for (let i = 0; i < count; i++) {
              const v = hist[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + (candleSlotWidth - bodyWidth) / 2;
                const y = spValueToY(v);
                ctx.fillStyle = v >= 0 ? 'rgba(16, 185, 129, 0.7)' : 'rgba(239, 68, 68, 0.7)';
                const bH = Math.max(1, Math.abs(y - zeroY));
                const top = Math.min(y, zeroY);
                ctx.fillRect(x, top, bodyWidth, bH);
              }
            }
          }
          // MACD and Signal lines
          const macdLines = [
            { key: 'macd', stroke: '#3B82F6', width: 1.4 },
            { key: 'signal', stroke: '#F59E0B', width: 1.4 },
          ];
          for (const line of macdLines) {
            const sData = sp.series?.[line.key];
            if (!sData) continue;
            ctx.strokeStyle = line.stroke;
            ctx.lineWidth = line.width;
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = sData[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = spValueToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
          }
        } else if (ind === 'STOCH') {
          const stochLines = [
            { key: 'k', stroke: '#10B981', width: 1.4 },
            { key: 'd', stroke: '#F59E0B', width: 1.4 },
          ];
          for (const line of stochLines) {
            const sData = sp.series?.[line.key];
            if (!sData) continue;
            ctx.strokeStyle = line.stroke;
            ctx.lineWidth = line.width;
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = sData[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = spValueToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
          }
        } else if (ind === 'ADX') {
          const adxLines = [
            { key: 'adx', stroke: '#F59E0B', width: 1.5 },
            { key: 'plus_di', stroke: '#10B981', width: 1.2 },
            { key: 'minus_di', stroke: '#EF4444', width: 1.2 },
          ];
          for (const line of adxLines) {
            const sData = sp.series?.[line.key];
            if (!sData) continue;
            ctx.strokeStyle = line.stroke;
            ctx.lineWidth = line.width;
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = sData[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = spValueToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
          }
        } else {
          // Single-line sub-pane (RSI, ATR, OBV, CCI, WILLR, CMF, MOM, ROC, MFI, STDDEV)
          const pts = sp.values || sp.series?.values;
          if (pts) {
            ctx.strokeStyle = sp.color || '#8B5CF6';
            ctx.lineWidth = 1.4;
            ctx.beginPath();
            let drawing = false;
            for (let i = 0; i < count; i++) {
              const v = pts[start + i];
              if (v !== null && v !== undefined && !isNaN(v)) {
                const x = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
                const y = spValueToY(v);
                if (!drawing) {
                  ctx.moveTo(x, y);
                  drawing = true;
                } else {
                  ctx.lineTo(x, y);
                }
              } else {
                drawing = false;
              }
            }
            ctx.stroke();
          }
        }

        // Sub-pane crosshair readout on right scale
        if (crosshair && crosshair.y >= spTop && crosshair.y <= spBottom) {
          const spYVal = spYToValue(crosshair.y);
          ctx.fillStyle = inkSolid;
          ctx.fillRect(chartWidth + 1, crosshair.y - 9, priceScaleWidth - 2, 18);
          ctx.fillStyle = panelBg;
          ctx.font = '10px monospace';
          ctx.textAlign = 'center';
          ctx.fillText(spYVal.toFixed(2), chartWidth + priceScaleWidth / 2, crosshair.y + 3.5);
        }
      });
    }

    // -------------------------------------------------------------------------
    // M36: Render Symbol Comparisons (rebased to 100 on first bar of full dataset)
    // -------------------------------------------------------------------------
    const effectiveComps = comparisons.length > 0 ? comparisons : localComparisons;
    if (effectiveComps && effectiveComps.length > 0) {
      for (const comp of effectiveComps) {
        if (!comp.values || comp.values.length === 0) continue;
        const baseP = comp.basePrice || visibleBars[0]?.c || 100;
        const compColor = comp.color || '#9C27B0';
        ctx.strokeStyle = compColor;
        ctx.lineWidth = 2;
        ctx.beginPath();
        let hasPoint = false;
        for (let i = 0; i < count; i++) {
          const val = comp.values[start + i];
          if (val === null || val === undefined || isNaN(val)) {
            hasPoint = false;
            continue;
          }
          const impliedPrice = baseP * (val / 100);
          const cx = xOffset + i * candleSlotWidth + candleSlotWidth / 2;
          const cy = 10 + candleChartHeight - ((impliedPrice - minPrice) / priceRange) * candleChartHeight;
          if (!hasPoint) {
            ctx.moveTo(cx, cy);
            hasPoint = true;
          } else {
            ctx.lineTo(cx, cy);
          }
        }
        ctx.stroke();
      }
    }

    // -------------------------------------------------------------------------
    // M36: Render Chart Drawings (Trendline, Horizontal Level, Fibonacci, Text)
    // -------------------------------------------------------------------------
    const getYForPrice = (p: number) => {
      const norm = (p - minPrice) / (priceRange || 1);
      return 10 + candleChartHeight - norm * candleChartHeight;
    };

    const getXForTime = (timeOrTs: number) => {
      if (bars.length === 0) return 0;
      const t = timeOrTs > 1e12 ? timeOrTs : timeOrTs * 1000;
      let bestIdx = 0;
      let bestDiff = Infinity;
      for (let i = 0; i < bars.length; i++) {
        const barTime = typeof bars[i].t === 'string' ? new Date(bars[i].t).getTime() : Number(bars[i].t);
        const diff = Math.abs(barTime - t);
        if (diff < bestDiff) {
          bestDiff = diff;
          bestIdx = i;
        }
      }
      const localIdx = bestIdx - start;
      return xOffset + localIdx * candleSlotWidth + candleSlotWidth / 2;
    };

    const allDrawingsToDraw = [...localDrawings];
    if (inProgressDrawing && crosshair) {
      const livePrice = minPrice + ((10 + candleChartHeight - crosshair.y) / candleChartHeight) * priceRange;
      const liveLocalIdx = Math.floor((crosshair.x - xOffset) / candleSlotWidth);
      const liveActualIdx = Math.min(bars.length - 1, Math.max(0, start + Math.max(0, liveLocalIdx)));
      const liveTime = typeof bars[liveActualIdx]?.t === 'string'
        ? new Date(bars[liveActualIdx].t).getTime()
        : Number(bars[liveActualIdx]?.t || Date.now());

      allDrawingsToDraw.push({
        id: '__in_progress__',
        tool: inProgressDrawing.tool,
        points: [inProgressDrawing.points[0], { time: liveTime, price: livePrice }],
        style: inProgressDrawing.style,
      });
    }

    for (const d of allDrawingsToDraw) {
      const isSelected = d.id === selectedDrawingId;
      const drawColor = d.style?.color || '#2962FF';
      const drawWidth = d.style?.lineWidth || 2;

      if (d.tool === 'trendline' && d.points.length >= 2) {
        const x1 = getXForTime(d.points[0].time || d.points[0].ts_ns || 0);
        const y1 = getYForPrice(d.points[0].price || 0);
        const x2 = getXForTime(d.points[1].time || d.points[1].ts_ns || 0);
        const y2 = getYForPrice(d.points[1].price || 0);

        ctx.strokeStyle = drawColor;
        ctx.lineWidth = drawWidth;
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.stroke();

        if (isSelected) {
          ctx.fillStyle = '#FFFFFF';
          ctx.strokeStyle = drawColor;
          ctx.lineWidth = 2;
          ctx.beginPath(); ctx.arc(x1, y1, 4.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
          ctx.beginPath(); ctx.arc(x2, y2, 4.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
        }
      } else if (d.tool === 'horizontal_level' && d.points.length >= 1) {
        const priceVal = d.points[0].price || 0;
        const y = getYForPrice(priceVal);

        ctx.strokeStyle = drawColor;
        ctx.lineWidth = isSelected ? drawWidth + 1 : drawWidth;
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(chartWidth, y);
        ctx.stroke();
        ctx.setLineDash([]);

        // Right scale badge
        ctx.fillStyle = drawColor;
        ctx.fillRect(chartWidth + 1, y - 8, priceScaleWidth - 2, 16);
        ctx.fillStyle = '#FFFFFF';
        ctx.font = '10px monospace';
        ctx.textAlign = 'center';
        ctx.fillText(formatCurrency(priceVal), chartWidth + priceScaleWidth / 2, y + 3.5);
      } else if (d.tool === 'fibonacci' && d.points.length >= 2) {
        const p1 = d.points[0].price || 0;
        const p2 = d.points[1].price || 0;
        const pDiff = p2 - p1;

        for (const lvl of FIBONACCI_LEVELS) {
          const pLvl = p1 + lvl.ratio * pDiff;
          const yLvl = getYForPrice(pLvl);

          ctx.strokeStyle = lvl.color;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(0, yLvl);
          ctx.lineTo(chartWidth, yLvl);
          ctx.stroke();

          ctx.fillStyle = lvl.color;
          ctx.font = '10px monospace';
          ctx.textAlign = 'left';
          ctx.fillText(`${lvl.label} ($${pLvl.toFixed(2)})`, 10, yLvl - 3);
        }
      } else if (d.tool === 'text' && d.points.length >= 1) {
        const p = d.points[0];
        const tx = getXForTime(p.time || p.ts_ns || 0);
        const ty = getYForPrice(p.price || 0);
        const textStr = d.style?.text || 'Note';

        ctx.font = '12px sans-serif';
        const textMetrics = ctx.measureText(textStr);
        const padding = 6;
        const boxW = textMetrics.width + padding * 2;
        const boxH = 22;

        ctx.fillStyle = panelBg;
        ctx.strokeStyle = drawColor;
        ctx.lineWidth = isSelected ? 2 : 1;
        ctx.fillRect(tx, ty - boxH / 2, boxW, boxH);
        ctx.strokeRect(tx, ty - boxH / 2, boxW, boxH);

        ctx.fillStyle = inkSolid;
        ctx.textAlign = 'left';
        ctx.fillText(textStr, tx + padding, ty + 4);
      }
    }

    // Scale border separators
    ctx.strokeStyle = inkA(0.1);
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(chartWidth, 0);
    ctx.lineTo(chartWidth, height);
    ctx.moveTo(0, chartHeight);
    ctx.lineTo(width, chartHeight);
    ctx.stroke();

    ctx.restore();
  }, [
    bars,
    viewWindow,
    crosshair,
    hoveredIdx,
    loading,
    symbol,
    timeframe,
    plotSize,
    theme,
    effectiveOverlays,
    effectiveSubpanes,
    localDrawings,
    inProgressDrawing,
    selectedDrawingId,
    comparisons,
    localComparisons,
  ]);

  // Drawing tool click handler
  const handleCanvasClickForDrawing = useCallback(
    (x: number, y: number) => {
      if (bars.length === 0 || !activeDrawingTool) return false;

      const { start, end } = viewWindow;
      const visibleBars = bars.slice(start, end + 1);
      let minP = Infinity;
      let maxP = -Infinity;
      for (const b of visibleBars) {
        if (b.l < minP) minP = b.l;
        if (b.h > maxP) maxP = b.h;
      }
      const pRange = maxP - minP || 1;
      const priceScaleWidth = 65;
      const timeScaleHeight = 24;
      const container = plotRef.current;
      const rect = container?.getBoundingClientRect();
      const height = Math.floor(rect?.height || plotSize.h || 360);
      const subCount = (effectiveSubpanes || []).filter((s) => s.visible !== false).length;
      const totalUsableHeight = height - timeScaleHeight;
      const subPaneHeight =
        subCount > 0 ? Math.min(110, Math.max(65, Math.floor((totalUsableHeight * 0.45) / subCount))) : 0;
      const mainChartHeight = totalUsableHeight - subCount * subPaneHeight;
      const volumeHeight = Math.floor(mainChartHeight * 0.18);
      const candleChartHeight = mainChartHeight - volumeHeight - 10;
      const chartWidth = (rect?.width || plotSize.w || 600) - priceScaleWidth;

      if (x > chartWidth || y > mainChartHeight) return false;

      const count = end - start + 1;
      const minSlots = 30;
      const effectiveSlots = Math.max(count, minSlots);
      const candleSlotWidth = chartWidth / effectiveSlots;
      const xOffset = count < minSlots ? Math.max(0, chartWidth - count * candleSlotWidth - 10) : 0;
      const localIdx = Math.floor((x - xOffset) / candleSlotWidth);
      const actualIdx = Math.min(bars.length - 1, Math.max(0, start + Math.max(0, localIdx)));
      const clickedTime =
        typeof bars[actualIdx]?.t === 'string'
          ? new Date(bars[actualIdx].t).getTime()
          : Number(bars[actualIdx]?.t || Date.now());
      const clickedPrice = minP + ((10 + candleChartHeight - y) / candleChartHeight) * pRange;

      if (activeDrawingTool === 'trendline') {
        if (!inProgressDrawing) {
          setInProgressDrawing({
            tool: 'trendline',
            points: [{ time: clickedTime, price: clickedPrice }],
            style: { color: drawingColor, lineWidth: 2 },
          });
        } else {
          const pt1 = inProgressDrawing.points[0];
          const newD: ChartDrawing = {
            id: `draw_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
            tool: 'trendline',
            points: [pt1, { time: clickedTime, price: clickedPrice }],
            style: { color: drawingColor, lineWidth: 2 },
          };
          pushToUndo([...localDrawings, newD]);
          onSaveDrawing?.(newD);
          setInProgressDrawing(null);
        }
        return true;
      } else if (activeDrawingTool === 'horizontal_level') {
        const newD: ChartDrawing = {
          id: `draw_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          tool: 'horizontal_level',
          points: [{ time: clickedTime, price: clickedPrice }],
          style: { color: drawingColor, lineWidth: 1.5 },
        };
        pushToUndo([...localDrawings, newD]);
        onSaveDrawing?.(newD);
        return true;
      } else if (activeDrawingTool === 'fibonacci') {
        if (!inProgressDrawing) {
          setInProgressDrawing({
            tool: 'fibonacci',
            points: [{ time: clickedTime, price: clickedPrice }],
            style: { color: drawingColor, lineWidth: 1.5 },
          });
        } else {
          const pt1 = inProgressDrawing.points[0];
          const newD: ChartDrawing = {
            id: `draw_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
            tool: 'fibonacci',
            points: [pt1, { time: clickedTime, price: clickedPrice }],
            style: { color: drawingColor, lineWidth: 1.5 },
          };
          pushToUndo([...localDrawings, newD]);
          onSaveDrawing?.(newD);
          setInProgressDrawing(null);
        }
        return true;
      } else if (activeDrawingTool === 'text') {
        const textVal = 'Key Level';
        const newD: ChartDrawing = {
          id: `draw_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          tool: 'text',
          points: [{ time: clickedTime, price: clickedPrice }],
          style: { color: drawingColor, text: textVal, fontSize: 12 },
        };
        pushToUndo([...localDrawings, newD]);
        onSaveDrawing?.(newD);
        return true;
      } else if (activeDrawingTool === 'select') {
        let hitId: string | null = null;
        for (let i = localDrawings.length - 1; i >= 0; i--) {
          const d = localDrawings[i];
          if (d.tool === 'horizontal_level' && d.points.length >= 1) {
            const dy = 10 + candleChartHeight - (((d.points[0].price || 0) - minP) / pRange) * candleChartHeight;
            if (Math.abs(y - dy) < 12) {
              hitId = d.id;
              break;
            }
          } else if (d.tool === 'trendline' || d.tool === 'fibonacci' || d.tool === 'text') {
            hitId = d.id;
            break;
          }
        }
        setSelectedDrawingId(hitId);
        return true;
      }
      return false;
    },
    [
      bars,
      activeDrawingTool,
      viewWindow,
      effectiveSubpanes,
      plotSize,
      drawingColor,
      inProgressDrawing,
      pushToUndo,
      localDrawings,
      onSaveDrawing,
    ]
  );

  // Keyboard shortcut listener
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      if (e.key === 'Escape') {
        setInProgressDrawing(null);
        setSelectedDrawingId(null);
        setActiveDrawingTool(null);
      } else if (e.key === 'Delete' || e.key === 'Backspace') {
        if (selectedDrawingId) {
          handleDeleteSelected();
        }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        if (e.shiftKey) {
          handleRedo();
        } else {
          handleUndo();
        }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'y') {
        handleRedo();
      } else if (e.key.toLowerCase() === 'v') {
        setActiveDrawingTool('select');
      } else if (e.key.toLowerCase() === 't') {
        setActiveDrawingTool('trendline');
      } else if (e.key.toLowerCase() === 'h') {
        setActiveDrawingTool('horizontal_level');
      } else if (e.key.toLowerCase() === 'f') {
        setActiveDrawingTool('fibonacci');
      } else if (e.key.toLowerCase() === 'a') {
        setActiveDrawingTool('text');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [selectedDrawingId, handleDeleteSelected, handleUndo, handleRedo]);

  // Mouse interaction handlers
  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas || bars.length === 0) return;

    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const priceScaleWidth = 65;
    const chartWidth = rect.width - priceScaleWidth;

    if (x <= chartWidth && x >= 0) {
      const { start, end } = viewWindow;
      const count = end - start + 1;
      const minSlots = 30;
      const effectiveSlots = Math.max(count, minSlots);
      const candleSlotWidth = chartWidth / effectiveSlots;
      const candleWidth = Math.min(24, Math.max(2, candleSlotWidth));
      const xOffset = count < minSlots ? Math.max(0, chartWidth - count * candleSlotWidth - 10) : 0;

      const localIdx = Math.floor((x - xOffset) / candleSlotWidth);
      const actualIdx = Math.min(bars.length - 1, Math.max(0, start + Math.max(0, localIdx)));

      setHoveredIdx(actualIdx);
      setCrosshair({ x, y, price: 0 });

      // Handle dragging (pan)
      if (isDragging && dragStartRef.current) {
        const dx = e.clientX - dragStartRef.current.x;
        const indexShift = Math.round(dx / candleSlotWidth);
        const newStart = Math.max(0, Math.min(bars.length - count, dragStartRef.current.startIdx - indexShift));
        const newEnd = Math.min(bars.length - 1, newStart + count - 1);
        setViewWindow({ start: newStart, end: newEnd });
      }
    } else {
      setCrosshair(null);
    }
  };

  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (bars.length === 0) return;
    const canvas = canvasRef.current;
    if (canvas && activeDrawingTool) {
      const rect = canvas.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;
      if (handleCanvasClickForDrawing(x, y)) {
        return;
      }
    }
    setIsDragging(true);
    dragStartRef.current = {
      x: e.clientX,
      startIdx: viewWindow.start,
      endIdx: viewWindow.end,
    };
  };

  const handleTouchStart = (e: React.TouchEvent<HTMLCanvasElement>) => {
    if (e.touches.length === 1) {
      const touch = e.touches[0];
      const canvas = canvasRef.current;
      if (!canvas || bars.length === 0) return;
      const rect = canvas.getBoundingClientRect();
      const x = touch.clientX - rect.left;
      const y = touch.clientY - rect.top;
      if (activeDrawingTool) {
        if (handleCanvasClickForDrawing(x, y)) {
          return;
        }
      }
      setIsDragging(true);
      dragStartRef.current = {
        x: touch.clientX,
        startIdx: viewWindow.start,
        endIdx: viewWindow.end,
      };
    }
  };

  const handleTouchMove = (e: React.TouchEvent<HTMLCanvasElement>) => {
    if (e.touches.length === 1) {
      const touch = e.touches[0];
      const canvas = canvasRef.current;
      if (!canvas || bars.length === 0) return;
      const rect = canvas.getBoundingClientRect();
      const x = touch.clientX - rect.left;
      const y = touch.clientY - rect.top;

      const priceScaleWidth = 65;
      const chartWidth = rect.width - priceScaleWidth;

      if (x <= chartWidth && x >= 0) {
        const { start, end } = viewWindow;
        const count = end - start + 1;
        const minSlots = 30;
        const effectiveSlots = Math.max(count, minSlots);
        const candleSlotWidth = chartWidth / effectiveSlots;
        const xOffset = count < minSlots ? Math.max(0, chartWidth - count * candleSlotWidth - 10) : 0;
        const localIdx = Math.floor((x - xOffset) / candleSlotWidth);
        const actualIdx = Math.min(bars.length - 1, Math.max(0, start + Math.max(0, localIdx)));

        setHoveredIdx(actualIdx);
        setCrosshair({ x, y, price: 0 });

        if (isDragging && dragStartRef.current) {
          const dx = touch.clientX - dragStartRef.current.x;
          const indexShift = Math.round(dx / candleSlotWidth);
          const newStart = Math.max(0, Math.min(bars.length - count, dragStartRef.current.startIdx - indexShift));
          const newEnd = Math.min(bars.length - 1, newStart + count - 1);
          setViewWindow({ start: newStart, end: newEnd });
        }
      }
    }
  };

  const handleTouchEnd = () => {
    setIsDragging(false);
    dragStartRef.current = null;
  };

  const handleMouseUp = () => {
    setIsDragging(false);
    dragStartRef.current = null;
  };

  const handleMouseLeave = () => {
    setIsDragging(false);
    dragStartRef.current = null;
    setCrosshair(null);
    setHoveredIdx(null);
  };

  // Plain scroll scrolls the page; Ctrl/Cmd + scroll (or trackpad pinch) zooms the chart.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      handleZoom(e.deltaY < 0 ? 'in' : 'out', 0.12);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });
    return () => canvas.removeEventListener('wheel', onWheel);
  }, [handleZoom]);

  return (
    <div
      ref={containerRef}
      className={`relative flex flex-col rounded-2xl border border-ink/10 bg-panel p-4 text-ink shadow-panel ${className}`}
    >
      {/* Top Bar: Symbol info, stats, timeframe bar, actions */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink/10 pb-3">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="font-mono text-lg font-bold tracking-tight text-ink">{symbol}</span>
            <span className="rounded-md bg-ink/[0.07] px-2 py-0.5 text-[10px] font-medium text-ink/70">
              USD
            </span>
          </div>

          <div className="flex items-baseline gap-2">
            <span
              className={cn(
                'font-mono text-xl font-bold transition-all duration-300 rounded px-1',
                priceFlash === 'up' && 'bg-emerald-500/20 text-emerald-400 ring-1 ring-emerald-500/30 scale-105',
                priceFlash === 'down' && 'bg-rose-500/20 text-rose-400 ring-1 ring-rose-500/30 scale-105'
              )}
            >
              {formatCurrency(latestPrice)}
            </span>
            <span
              className={`flex items-center gap-0.5 font-mono text-xs font-semibold ${
                isUp ? 'text-profit' : 'text-loss'
              }`}
            >
              {isUp ? <TrendingUp className="h-3.5 w-3.5" /> : <TrendingDown className="h-3.5 w-3.5" />}
              {priceChange >= 0 ? '+' : ''}
              {priceChange.toFixed(2)} ({pctChange >= 0 ? '+' : ''}
              {pctChange.toFixed(2)}%)
            </span>
          </div>
        </div>

        {/* Timeframe Buttons */}
        <div className="flex items-center gap-1 rounded-xl bg-ink/[0.04] p-1">
          {TIMEFRAMES.map((tf) => (
            <button
              key={tf.value}
              type="button"
              onClick={() => onTimeframeChange(tf.value)}
              className={`rounded-lg px-2.5 py-1 text-xs font-medium transition ${
                timeframe === tf.value
                  ? 'bg-cyan-500/20 text-accent shadow-sm'
                  : 'text-ink/55 hover:bg-ink/[0.04] hover:text-ink'
              }`}
              title={tf.description}
            >
              {tf.label}
            </button>
          ))}
        </div>

        {/* Indicators Picker Button */}
        <button
          type="button"
          onClick={() => setIsPickerOpen(true)}
          className="flex items-center gap-1.5 rounded-xl border border-accent/40 bg-accent/10 px-2.5 py-1 text-xs font-semibold text-accent hover:bg-accent/20 transition shadow-sm"
          title="Add or configure technical indicators"
        >
          <Sliders className="h-3.5 w-3.5" />
          <span>Indicators</span>
          {(effectiveOverlays.length > 0 || effectiveSubpanes.length > 0) && (
            <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent text-[10px] font-bold text-white">
              {effectiveOverlays.length + effectiveSubpanes.length}
            </span>
          )}
        </button>

        {/* Compare Symbols Button */}
        <button
          type="button"
          onClick={() => setIsComparisonModalOpen(true)}
          className="flex items-center gap-1.5 rounded-xl border border-ink/15 bg-ink/[0.04] px-2.5 py-1 text-xs font-semibold text-ink/80 hover:bg-ink/[0.08] hover:text-ink transition shadow-sm"
          title="Compare performance against other symbols"
        >
          <TrendingUp className="h-3.5 w-3.5 text-purple-500" />
          <span>Compare</span>
          {effectiveComps.length > 0 && (
            <span className="flex h-4 w-4 items-center justify-center rounded-full bg-purple-500 text-[10px] font-bold text-white">
              {effectiveComps.length}
            </span>
          )}
        </button>

        {/* Layout Templates Button */}
        <button
          type="button"
          onClick={() => setIsTemplateModalOpen(true)}
          className="flex items-center gap-1.5 rounded-xl border border-ink/15 bg-ink/[0.04] px-2.5 py-1 text-xs font-semibold text-ink/80 hover:bg-ink/[0.08] hover:text-ink transition shadow-sm"
          title="Saved chart layout templates"
        >
          <LayoutTemplate className="h-3.5 w-3.5 text-cyan-500" />
          <span>Templates</span>
          {templates.length > 0 && (
            <span className="flex h-4 w-4 items-center justify-center rounded-full bg-cyan-500/20 text-[10px] font-bold text-cyan-500">
              {templates.length}
            </span>
          )}
        </button>

        {/* Actions & Status */}
        <div className="flex items-center gap-2">
          <span
            className="flex items-center gap-1.5 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-1 text-[11px] font-semibold text-profit shadow-sm shadow-emerald-500/10"
            title="Real-Time Active Feed · Refreshes every 3s"
          >
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
            </span>
            <span>LIVE · {dataStatus}</span>
          </span>

          <button
            type="button"
            onClick={() => handleZoom('in')}
            className="rounded-lg border border-ink/10 bg-ink/[0.04] p-1.5 text-ink/70 hover:bg-ink/[0.07]"
            title="Zoom In"
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={() => handleZoom('out')}
            className="rounded-lg border border-ink/10 bg-ink/[0.04] p-1.5 text-ink/70 hover:bg-ink/[0.07]"
            title="Zoom Out"
          >
            <Minus className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={handleResetZoom}
            className="rounded-lg border border-ink/10 bg-ink/[0.04] p-1.5 text-ink/70 hover:bg-ink/[0.07]"
            title="Reset View"
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </button>
          {onRefresh && (
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              className="rounded-lg border border-ink/10 bg-ink/[0.04] p-1.5 text-ink/70 hover:bg-ink/[0.07]"
              title="Refresh Data"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
            </button>
          )}
        </div>
      </div>

      {/* Candlestick Legend (OHLC values for hovered/latest bar) */}
      <div className="flex flex-wrap items-center gap-4 py-2 font-mono text-[11px] text-ink/55">
        <div>
          O: <span className="text-ink font-medium">{activeBar ? activeBar.o.toFixed(2) : '-'}</span>
        </div>
        <div>
          H: <span className="text-ink font-medium">{activeBar ? activeBar.h.toFixed(2) : '-'}</span>
        </div>
        <div>
          L: <span className="text-ink font-medium">{activeBar ? activeBar.l.toFixed(2) : '-'}</span>
        </div>
        <div>
          C: <span className="text-ink font-medium">{activeBar ? activeBar.c.toFixed(2) : '-'}</span>
        </div>
        <div>
          Vol: <span className="text-ink font-medium">{activeBar ? activeBar.v.toLocaleString() : '-'}</span>
        </div>
        {activeBar && (
          <div className="ml-auto text-ink/45">
            {new Date(activeBar.t).toLocaleString(undefined, {
              dateStyle: 'medium',
              timeStyle: 'short',
            })}
          </div>
        )}
      </div>

      {/* Active Overlays Readout in Legend */}
      {(effectiveOverlays.length > 0 || effectiveSubpanes.length > 0) && (
        <div className="flex flex-wrap items-center gap-2.5 border-t border-ink/[0.06] py-1.5 font-mono text-[11px]">
          {/* Overlays */}
          {effectiveOverlays.map((ov, oIdx) => {
            const isVisible = ov.visible !== false;
            const idx = hoveredIdx !== null ? hoveredIdx : (bars.length > 0 ? bars.length - 1 : null);
            const color = ov.color || '#3B82F6';
            const label = ov.name || ov.indicator;
            const targetId = ov.id || `${ov.indicator}-${oIdx}`;

            return (
              <div
                key={targetId}
                className={`group flex items-center gap-1.5 rounded-lg border px-2 py-0.5 transition ${
                  isVisible
                    ? 'border-ink/10 bg-ink/[0.02] text-ink'
                    : 'border-dashed border-ink/10 bg-transparent text-ink/40 opacity-60'
                }`}
              >
                <span className="inline-block h-2 w-2 rounded-full shrink-0" style={{ backgroundColor: color }} />
                <span className="text-ink/70 font-medium">{label}:</span>

                {isVisible && idx !== null && (
                  <>
                    {ov.values && (
                      <span className="font-semibold text-ink">
                        {ov.values[idx] !== null && ov.values[idx] !== undefined ? ov.values[idx]!.toFixed(2) : '–'}
                      </span>
                    )}

                    {ov.series && (
                      <span className="flex items-center gap-1.5">
                        {Object.entries(ov.series).map(([k, s]) => {
                          const val = s[idx];
                          const cleanKey = k.replace('_sen', '').replace('senkou_', '');
                          return (
                            <span key={k} className="text-ink/75">
                              <span className="text-[9px] uppercase text-ink/45 mr-0.5">{cleanKey}:</span>
                              <span className="font-medium text-ink">
                                {val !== null && val !== undefined ? val.toFixed(2) : '–'}
                              </span>
                            </span>
                          );
                        })}
                      </span>
                    )}
                  </>
                )}

                {/* Quick Action Icons */}
                <div className="flex items-center gap-0.5 ml-1 opacity-70 group-hover:opacity-100 transition">
                  <button
                    type="button"
                    onClick={() => handleToggleVisibility(ov.id || ov.indicator)}
                    className="rounded p-0.5 hover:bg-ink/10 text-ink/60 hover:text-ink transition"
                    title={isVisible ? 'Hide overlay' : 'Show overlay'}
                  >
                    {isVisible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      const meta = INDICATOR_REGISTRY[ov.indicator as IndicatorName];
                      setSettingsModalState({
                        isOpen: true,
                        indicatorId: ov.indicator,
                        targetId: ov.id || ov.indicator,
                        isOverlay: true,
                        params: ov.params || meta?.defaultParams || {},
                        color: ov.color || meta?.defaultColor || '#3B82F6',
                      });
                    }}
                    className="rounded p-0.5 hover:bg-ink/10 text-ink/60 hover:text-ink transition"
                    title="Configure parameters & color"
                  >
                    <Settings className="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemoveIndicator(ov.id || ov.indicator)}
                    className="rounded p-0.5 hover:bg-loss/10 text-ink/40 hover:text-loss transition"
                    title="Remove overlay"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              </div>
            );
          })}

          {/* Subpanes */}
          {effectiveSubpanes.map((sp, sIdx) => {
            const isVisible = sp.visible !== false;
            const idx = hoveredIdx !== null ? hoveredIdx : (bars.length > 0 ? bars.length - 1 : null);
            const color = sp.color || '#8B5CF6';
            const label = sp.name || sp.indicator;
            const targetId = sp.id || `${sp.indicator}-${sIdx}`;

            return (
              <div
                key={targetId}
                className={`group flex items-center gap-1.5 rounded-lg border px-2 py-0.5 transition ${
                  isVisible
                    ? 'border-ink/10 bg-ink/[0.02] text-ink'
                    : 'border-dashed border-ink/10 bg-transparent text-ink/40 opacity-60'
                }`}
              >
                <span className="inline-block h-2 w-2 rounded-full shrink-0" style={{ backgroundColor: color }} />
                <span className="text-ink/70 font-medium">{label}:</span>

                {isVisible && idx !== null && (
                  <>
                    {sp.values && (
                      <span className="font-semibold text-ink">
                        {sp.values[idx] !== null && sp.values[idx] !== undefined ? sp.values[idx]!.toFixed(2) : '–'}
                      </span>
                    )}

                    {sp.series && (
                      <span className="flex items-center gap-1.5">
                        {Object.entries(sp.series).map(([k, s]) => {
                          const val = s[idx];
                          return (
                            <span key={k} className="text-ink/75">
                              <span className="text-[9px] uppercase text-ink/45 mr-0.5">{k}:</span>
                              <span className="font-medium text-ink">
                                {val !== null && val !== undefined ? val.toFixed(2) : '–'}
                              </span>
                            </span>
                          );
                        })}
                      </span>
                    )}
                  </>
                )}

                {/* Quick Action Icons */}
                <div className="flex items-center gap-0.5 ml-1 opacity-70 group-hover:opacity-100 transition">
                  <button
                    type="button"
                    onClick={() => handleToggleVisibility(sp.id || sp.indicator)}
                    className="rounded p-0.5 hover:bg-ink/10 text-ink/60 hover:text-ink transition"
                    title={isVisible ? 'Hide sub-pane' : 'Show sub-pane'}
                  >
                    {isVisible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      const meta = INDICATOR_REGISTRY[sp.indicator as IndicatorName];
                      setSettingsModalState({
                        isOpen: true,
                        indicatorId: sp.indicator,
                        targetId: sp.id || sp.indicator,
                        isOverlay: false,
                        params: sp.params || meta?.defaultParams || {},
                        color: sp.color || meta?.defaultColor || '#8B5CF6',
                      });
                    }}
                    className="rounded p-0.5 hover:bg-ink/10 text-ink/60 hover:text-ink transition"
                    title="Configure parameters & color"
                  >
                    <Settings className="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemoveIndicator(sp.id || sp.indicator)}
                    className="rounded p-0.5 hover:bg-loss/10 text-ink/40 hover:text-loss transition"
                    title="Remove sub-pane"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              </div>
            );
          })}

          {/* Active Comparisons in Legend */}
          {effectiveComps.map((c) => (
            <div
              key={c.symbol}
              className="group flex items-center gap-1.5 rounded-lg border border-purple-500/30 bg-purple-500/10 px-2 py-0.5 text-xs text-ink"
            >
              <span className="inline-block h-2 w-2 rounded-full shrink-0" style={{ backgroundColor: c.color || '#9C27B0' }} />
              <span className="font-semibold text-ink">{c.symbol}</span>
              <button
                type="button"
                onClick={() => handleRemoveComparison(c.symbol)}
                className="text-ink/40 hover:text-rose-500 transition-colors ml-0.5"
                title={`Remove ${c.symbol} comparison`}
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ))}
        </div>
      )}

      <p className="pb-1 text-right text-[10px] text-ink/40">Drag to pan · Ctrl/⌘ + scroll to zoom · V/T/H/F/A for tools</p>
      {/* Chart Canvas Area with Drawing Toolbar */}
      {(() => {
        const subCount = (effectiveSubpanes || []).filter((s) => s.visible !== false).length;
        const dynamicH =
          subCount > 0
            ? `clamp(${320 + subCount * 90}px, ${48 + subCount * 12}vh, ${480 + subCount * 120}px)`
            : 'clamp(320px, 48vh, 480px)';
        const dynamicMinH = subCount > 0 ? `${340 + subCount * 90}px` : '340px';
        return (
          <div className="relative flex flex-1 w-full gap-2">
            {/* Left Drawing Toolbar */}
            <DrawingToolbar
              activeTool={activeDrawingTool}
              onSelectTool={setActiveDrawingTool}
              currentColor={drawingColor}
              onSelectColor={setDrawingColor}
              canUndo={undoStack.length > 0}
              canRedo={redoStack.length > 0}
              hasSelection={selectedDrawingId !== null}
              drawingCount={localDrawings.length}
              onUndo={handleUndo}
              onRedo={handleRedo}
              onDeleteSelected={handleDeleteSelected}
              onClearAll={handleClearAll}
              className="shrink-0"
            />

            <div
              ref={plotRef}
              style={{ minHeight: dynamicMinH, height: dynamicH }}
              className={cn(
                'relative flex-1 select-none',
                activeDrawingTool ? 'cursor-crosshair' : 'cursor-default'
              )}
            >
              <canvas
                ref={canvasRef}
                onMouseMove={handleMouseMove}
                onMouseDown={handleMouseDown}
                onMouseUp={handleMouseUp}
                onMouseLeave={handleMouseLeave}
                onTouchStart={handleTouchStart}
                onTouchMove={handleTouchMove}
                onTouchEnd={handleTouchEnd}
                className="absolute inset-0 block h-full w-full rounded-lg"
              />
            </div>
          </div>
        );
      })()}

      {/* Comparison Modal */}
      <ComparisonModal
        isOpen={isComparisonModalOpen}
        onClose={() => setIsComparisonModalOpen(false)}
        baseSymbol={symbol}
        activeComparisons={effectiveComps.map((c) => ({
          symbol: c.symbol,
          color: c.color,
          visible: true,
        }))}
        onAddComparison={handleAddComparison}
        onRemoveComparison={handleRemoveComparison}
      />

      {/* Template Modal */}
      <TemplateModal
        isOpen={isTemplateModalOpen}
        onClose={() => setIsTemplateModalOpen(false)}
        templates={templates}
        currentIndicatorCount={effectiveOverlays.length + effectiveSubpanes.length}
        currentDrawingCount={localDrawings.length}
        currentComparisonCount={effectiveComps.length}
        isViewer={isViewer}
        onSaveTemplate={handleSaveTemplate}
        onLoadTemplate={handleApplyTemplate}
        onDeleteTemplate={handleDeleteTemplate}
      />

      {/* Indicator Picker Modal */}
      <IndicatorPickerModal
        isOpen={isPickerOpen}
        onClose={() => setIsPickerOpen(false)}
        onSelectIndicator={handleSelectIndicator}
        activeIndicatorIds={[
          ...effectiveOverlays.map((o) => String(o.indicator)),
          ...effectiveSubpanes.map((s) => String(s.indicator)),
        ]}
      />

      {/* Indicator Settings Modal */}
      {settingsModalState && (
        <IndicatorSettingsModal
          isOpen={settingsModalState.isOpen}
          indicatorId={settingsModalState.indicatorId}
          currentParams={settingsModalState.params}
          currentColor={settingsModalState.color}
          onClose={() => setSettingsModalState(null)}
          onSave={(newParams, newColor) => {
            handleUpdateParams(settingsModalState.targetId, newParams, newColor);
            setSettingsModalState(null);
          }}
          onRemove={() => {
            handleRemoveIndicator(settingsModalState.targetId);
            setSettingsModalState(null);
          }}
        />
      )}
    </div>
  );
}
