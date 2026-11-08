'use client';

import React from 'react';
import {
  Minus,
  MousePointer,
  Redo2,
  RotateCcw,
  Slash,
  Sliders,
  Trash2,
  Type,
  Undo2,
} from 'lucide-react';
import {
  DRAWING_COLOR_PALETTE,
  type DrawingToolType,
} from '@/lib/chart-drawings';
import { cn } from '@/lib/utils';

export interface DrawingToolbarProps {
  activeTool: DrawingToolType | null;
  onSelectTool: (tool: DrawingToolType | null) => void;
  currentColor: string;
  onSelectColor: (color: string) => void;
  canUndo: boolean;
  canRedo: boolean;
  hasSelection: boolean;
  drawingCount: number;
  onUndo: () => void;
  onRedo: () => void;
  onDeleteSelected: () => void;
  onClearAll: () => void;
  className?: string;
}

export function DrawingToolbar({
  activeTool,
  onSelectTool,
  currentColor,
  onSelectColor,
  canUndo,
  canRedo,
  hasSelection,
  drawingCount,
  onUndo,
  onRedo,
  onDeleteSelected,
  onClearAll,
  className,
}: DrawingToolbarProps) {
  const tools: Array<{
    id: DrawingToolType;
    label: string;
    icon: React.ReactNode;
    shortcut: string;
  }> = [
    {
      id: 'select',
      label: 'Cursor / Select (V)',
      icon: <MousePointer className="h-4 w-4" />,
      shortcut: 'V',
    },
    {
      id: 'trendline',
      label: 'Trendline (T)',
      icon: <Slash className="h-4 w-4 rotate-45" />,
      shortcut: 'T',
    },
    {
      id: 'horizontal_level',
      label: 'Horizontal Level (H)',
      icon: <Minus className="h-4 w-4" />,
      shortcut: 'H',
    },
    {
      id: 'fibonacci',
      label: 'Fibonacci Retracement (F)',
      icon: <Sliders className="h-4 w-4" />,
      shortcut: 'F',
    },
    {
      id: 'text',
      label: 'Text Annotation (A)',
      icon: <Type className="h-4 w-4" />,
      shortcut: 'A',
    },
  ];

  return (
    <div
      className={cn(
        'flex flex-col items-center gap-1.5 rounded-xl border border-ink/10 bg-panel/90 p-1.5 shadow-sm backdrop-blur-md',
        className
      )}
      role="toolbar"
      aria-label="Drawing tools"
    >
      {/* Tool Selection Buttons */}
      <div className="flex flex-col gap-1">
        {tools.map((t) => {
          const isActive = activeTool === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => onSelectTool(isActive ? null : t.id)}
              className={cn(
                'group relative flex h-8 w-8 items-center justify-center rounded-lg text-ink/70 transition-all hover:bg-ink/[0.08] hover:text-ink',
                isActive && 'bg-accent/15 text-accent ring-1 ring-accent/40 font-semibold'
              )}
              title={t.label}
              aria-label={t.label}
              aria-pressed={isActive}
            >
              {t.icon}
              <span className="sr-only">{t.label}</span>
            </button>
          );
        })}
      </div>

      <div className="my-1 h-px w-6 bg-ink/10" />

      {/* Color Picker Dropdown / Dot */}
      <div className="flex flex-col gap-1">
        {DRAWING_COLOR_PALETTE.slice(0, 4).map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => onSelectColor(c)}
            className={cn(
              'h-4 w-4 rounded-full transition-all border border-black/20 hover:scale-125',
              currentColor === c && 'ring-2 ring-accent ring-offset-1'
            )}
            style={{ backgroundColor: c }}
            title={`Color ${c}`}
            aria-label={`Select color ${c}`}
          />
        ))}
      </div>

      <div className="my-1 h-px w-6 bg-ink/10" />

      {/* Undo / Redo */}
      <div className="flex flex-col gap-1">
        <button
          type="button"
          onClick={onUndo}
          disabled={!canUndo}
          className="flex h-8 w-8 items-center justify-center rounded-lg text-ink/70 transition-all hover:bg-ink/[0.08] hover:text-ink disabled:opacity-30"
          title="Undo (Ctrl+Z)"
          aria-label="Undo drawing"
        >
          <Undo2 className="h-4 w-4" />
        </button>
        <button
          type="button"
          onClick={onRedo}
          disabled={!canRedo}
          className="flex h-8 w-8 items-center justify-center rounded-lg text-ink/70 transition-all hover:bg-ink/[0.08] hover:text-ink disabled:opacity-30"
          title="Redo (Ctrl+Y)"
          aria-label="Redo drawing"
        >
          <Redo2 className="h-4 w-4" />
        </button>
      </div>

      {/* Delete / Clear */}
      <div className="flex flex-col gap-1">
        <button
          type="button"
          onClick={onDeleteSelected}
          disabled={!hasSelection}
          className="flex h-8 w-8 items-center justify-center rounded-lg text-rose-500/80 transition-all hover:bg-rose-500/10 hover:text-rose-600 disabled:opacity-30"
          title="Delete selected (Delete / Backspace)"
          aria-label="Delete selected drawing"
        >
          <Trash2 className="h-4 w-4" />
        </button>
        {drawingCount > 0 && (
          <button
            type="button"
            onClick={onClearAll}
            className="flex h-8 w-8 items-center justify-center rounded-lg text-ink/40 transition-all hover:bg-rose-500/10 hover:text-rose-500"
            title={`Clear all drawings (${drawingCount})`}
            aria-label="Clear all drawings"
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
    </div>
  );
}
