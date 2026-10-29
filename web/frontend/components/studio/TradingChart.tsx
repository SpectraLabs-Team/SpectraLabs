'use client';

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  BarChart2,
  Calendar,
  Maximize2,
  Minus,
  Plus,
  RefreshCw,
  RotateCcw,
  TrendingDown,
  TrendingUp,
  Zap,
} from 'lucide-react';
import { formatCurrency, formatPercent } from '@/lib/utils';
import type { CandleBar } from '@/lib/alpaca';
import { useTheme } from '@/lib/theme';

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
}: TradingChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const plotRef = useRef<HTMLDivElement>(null);
  const { theme } = useTheme();
  const [plotSize, setPlotSize] = useState({ w: 0, h: 0 });
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
    const priceScaleWidth = 65;
    const timeScaleHeight = 24;
    const volumeHeight = Math.floor(height * 0.18);
    const chartHeight = height - timeScaleHeight;
    const candleChartHeight = chartHeight - volumeHeight - 10;
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

    const candleWidth = Math.max(2, chartWidth / count);
    const bodyWidth = Math.max(1, candleWidth * 0.72);

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
      const x = Math.round(i * candleWidth + candleWidth / 2);
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
    const volBaseY = chartHeight;
    const volHeightMax = volumeHeight - 10;

    for (let i = 0; i < count; i++) {
      const b = visibleBars[i];
      const x = i * candleWidth + (candleWidth - bodyWidth) / 2;
      const isBarUp = b.c >= b.o;
      const vHeight = maxVolume > 0 ? (b.v / maxVolume) * volHeightMax : 0;
      const vY = volBaseY - vHeight;

      ctx.fillStyle = isBarUp ? 'rgba(16, 185, 129, 0.25)' : 'rgba(239, 68, 68, 0.25)';
      ctx.fillRect(x, vY, bodyWidth, vHeight);
    }

    // Draw Candlesticks (Wick + Body)
    for (let i = 0; i < count; i++) {
      const b = visibleBars[i];
      const isBarUp = b.c >= b.o;
      const candleColor = isBarUp ? '#10B981' : '#EF4444';
      const wickColor = isBarUp ? '#34D399' : '#F87171';

      const xCenter = i * candleWidth + candleWidth / 2;
      const xBody = i * candleWidth + (candleWidth - bodyWidth) / 2;

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
  }, [bars, viewWindow, crosshair, hoveredIdx, loading, symbol, timeframe, plotSize, theme]);

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
      const candleWidth = chartWidth / count;
      const localIdx = Math.floor(x / candleWidth);
      const actualIdx = Math.min(bars.length - 1, Math.max(0, start + localIdx));

      setHoveredIdx(actualIdx);
      setCrosshair({ x, y, price: 0 });

      // Handle dragging (pan)
      if (isDragging && dragStartRef.current) {
        const dx = e.clientX - dragStartRef.current.x;
        const indexShift = Math.round(dx / candleWidth);
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
    setIsDragging(true);
    dragStartRef.current = {
      x: e.clientX,
      startIdx: viewWindow.start,
      endIdx: viewWindow.end,
    };
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
            <span className="font-mono text-xl font-bold">
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

        {/* Actions & Status */}
        <div className="flex items-center gap-2">
          <span
            className="flex items-center gap-1.5 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-0.5 text-[10.5px] font-medium text-profit"
            title="Real Alpaca market data feed"
          >
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
            {dataStatus}
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

      <p className="pb-1 text-right text-[10px] text-ink/40">Drag to pan · Ctrl/⌘ + scroll to zoom</p>
      {/* Chart Canvas */}
      <div
        ref={plotRef}
        style={{ minHeight: '340px', height: 'clamp(320px, 48vh, 480px)' }}
        className="relative min-h-[340px] h-[clamp(320px,48vh,480px)] w-full flex-1 cursor-crosshair select-none"
      >
        <canvas
          ref={canvasRef}
          onMouseMove={handleMouseMove}
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseLeave}
          className="absolute inset-0 block h-full w-full rounded-lg"
        />
      </div>
    </div>
  );
}
