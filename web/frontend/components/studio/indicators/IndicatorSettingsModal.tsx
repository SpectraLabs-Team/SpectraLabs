'use client';

import React, { useEffect, useState } from 'react';
import {
  AlertCircle,
  HelpCircle,
  RotateCcw,
  Sliders,
  Trash2,
  X,
} from 'lucide-react';
import {
  INDICATOR_REGISTRY,
  type IndicatorName,
  type ParamField,
} from '@/lib/indicators';

export interface IndicatorSettingsModalProps {
  isOpen: boolean;
  indicatorId: IndicatorName | string;
  currentParams: Record<string, number>;
  currentColor?: string;
  onClose: () => void;
  onSave: (newParams: Record<string, number>, newColor: string) => void;
  onRemove?: () => void;
}

const COLOR_PRESETS = [
  '#3B82F6', // Blue
  '#10B981', // Emerald
  '#8B5CF6', // Purple
  '#F59E0B', // Amber
  '#EF4444', // Red
  '#06B6D4', // Cyan
  '#EC4899', // Pink
  '#F97316', // Orange
  '#6366F1', // Indigo
  '#14B8A6', // Teal
];

export function IndicatorSettingsModal({
  isOpen,
  indicatorId,
  currentParams,
  currentColor,
  onClose,
  onSave,
  onRemove,
}: IndicatorSettingsModalProps) {
  const meta = INDICATOR_REGISTRY[indicatorId as IndicatorName];

  const [params, setParams] = useState<Record<string, number>>({});
  const [color, setColor] = useState<string>('#3B82F6');
  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (isOpen && meta) {
      setParams({ ...(meta.defaultParams || {}), ...(currentParams || {}) });
      setColor(currentColor || meta.defaultColor || '#3B82F6');
      setErrors({});
    }
  }, [isOpen, indicatorId, currentParams, currentColor, meta]);

  if (!isOpen || !meta) return null;

  const validateParam = (field: ParamField, val: number): string | null => {
    if (isNaN(val)) return 'Must be a valid number';
    if (field.type === 'integer' && !Number.isInteger(val)) {
      return 'Must be an integer';
    }
    if (field.min !== undefined && val < field.min) {
      return `Minimum allowed is ${field.min}`;
    }
    if (field.max !== undefined && val > field.max) {
      return `Maximum allowed is ${field.max}`;
    }
    return null;
  };

  const handleParamChange = (field: ParamField, rawVal: string) => {
    const val = field.type === 'integer' ? parseInt(rawVal, 10) : parseFloat(rawVal);
    const newParams = { ...params, [field.name]: val };
    setParams(newParams);

    const err = validateParam(field, val);
    setErrors((prev) => {
      const next = { ...prev };
      if (err) next[field.name] = err;
      else delete next[field.name];
      return next;
    });
  };

  const handleResetDefaults = () => {
    setParams({ ...meta.defaultParams });
    setColor(meta.defaultColor);
    setErrors({});
  };

  const hasErrors = Object.keys(errors).length > 0;

  const handleSave = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (hasErrors) return;

    // Final pass validation
    const validationErrors: Record<string, string> = {};
    for (const field of meta.params) {
      const v = params[field.name] ?? field.default;
      const err = validateParam(field, v);
      if (err) validationErrors[field.name] = err;
    }

    if (Object.keys(validationErrors).length > 0) {
      setErrors(validationErrors);
      return;
    }

    onSave(params, color);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4 animate-in fade-in duration-150">
      <div className="flex max-h-[85vh] w-full max-w-md flex-col rounded-2xl border border-ink/10 bg-panel shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink/10 px-5 py-4">
          <div className="flex items-center gap-2.5">
            <span
              className="inline-block h-4 w-4 rounded-full border border-black/10 dark:border-white/20 shadow-sm"
              style={{ backgroundColor: color }}
            />
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold text-ink">{meta.name}</h3>
                <span className="rounded bg-ink/[0.06] px-1.5 py-0.2 font-mono text-[10px] font-bold text-ink/70">
                  {meta.shortName}
                </span>
              </div>
              <p className="text-[11px] text-ink/55 capitalize">
                {meta.category === 'overlay' ? 'Price Overlay Settings' : 'Sub-Pane Settings'}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-ink/50 hover:bg-ink/[0.06] hover:text-ink transition"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Form Body */}
        <form onSubmit={handleSave} className="flex-1 overflow-y-auto p-5 space-y-5">
          {/* Parameter Fields */}
          <div className="space-y-3.5">
            <div className="flex items-center justify-between">
              <span className="text-[11.5px] font-bold uppercase tracking-wider text-ink/60">
                Parameters
              </span>
              <a
                href={`/learn/indicators/${meta.id.toLowerCase()}`}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-[11px] text-accent hover:underline"
              >
                <HelpCircle className="h-3 w-3" />
                <span>Reference</span>
              </a>
            </div>

            {meta.params.map((field) => {
              const currentVal = params[field.name] ?? field.default;
              const error = errors[field.name];

              return (
                <div key={field.name} className="space-y-1">
                  <div className="flex items-center justify-between text-[12.5px]">
                    <label htmlFor={`param-${field.name}`} className="font-medium text-ink">
                      {field.label}
                    </label>
                    <span className="font-mono text-[11px] text-ink/45">
                      {field.min !== undefined && field.max !== undefined
                        ? `[${field.min} .. ${field.max}]`
                        : field.min !== undefined
                        ? `[≥ ${field.min}]`
                        : ''}
                    </span>
                  </div>

                  <input
                    id={`param-${field.name}`}
                    type="number"
                    step={field.step ?? (field.type === 'integer' ? 1 : 0.01)}
                    min={field.min}
                    max={field.max}
                    value={isNaN(currentVal) ? '' : currentVal}
                    onChange={(e) => handleParamChange(field, e.target.value)}
                    className={`w-full rounded-xl border bg-canvas/80 px-3 py-1.5 font-mono text-[13px] text-ink transition focus:outline-none focus:ring-1 ${
                      error
                        ? 'border-loss focus:border-loss focus:ring-loss'
                        : 'border-ink/15 focus:border-accent focus:ring-accent'
                    }`}
                  />

                  {error && (
                    <div className="flex items-center gap-1 text-[11px] text-loss">
                      <AlertCircle className="h-3 w-3 shrink-0" />
                      <span>{error}</span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          {/* Color Selection */}
          <div className="space-y-2.5 pt-2 border-t border-ink/10">
            <span className="text-[11.5px] font-bold uppercase tracking-wider text-ink/60">
              Style & Color
            </span>
            <div className="flex flex-wrap items-center gap-2.5">
              {COLOR_PRESETS.map((c) => (
                <button
                  key={c}
                  type="button"
                  onClick={() => setColor(c)}
                  className={`h-7 w-7 rounded-full border transition hover:scale-110 shadow-sm ${
                    color.toUpperCase() === c.toUpperCase()
                      ? 'border-accent ring-2 ring-accent/50 scale-105'
                      : 'border-black/10 dark:border-white/10'
                  }`}
                  style={{ backgroundColor: c }}
                  title={c}
                />
              ))}
              <input
                type="color"
                value={color}
                onChange={(e) => setColor(e.target.value)}
                className="h-7 w-7 cursor-pointer rounded-full border border-ink/20 bg-transparent p-0 overflow-hidden"
                title="Custom color"
              />
            </div>
          </div>

          {/* Reset Defaults */}
          <div className="pt-2">
            <button
              type="button"
              onClick={handleResetDefaults}
              className="flex items-center gap-1.5 text-[11.5px] font-semibold text-ink/60 hover:text-ink transition"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              <span>Reset to spec defaults</span>
            </button>
          </div>
        </form>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-ink/10 bg-ink/[0.02] px-5 py-3.5">
          {onRemove ? (
            <button
              type="button"
              onClick={() => {
                onRemove();
                onClose();
              }}
              className="flex items-center gap-1 text-[12px] font-semibold text-loss hover:text-loss/80 transition"
              title="Remove from chart"
            >
              <Trash2 className="h-3.5 w-3.5" />
              <span>Remove</span>
            </button>
          ) : (
            <div />
          )}

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-ink/15 bg-panel px-3 py-1.5 text-xs font-semibold text-ink hover:bg-ink/[0.05] transition"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={hasErrors}
              onClick={() => handleSave()}
              className="rounded-lg bg-accent px-4 py-1.5 text-xs font-semibold text-white shadow-sm hover:bg-accent/90 disabled:opacity-50 transition"
            >
              Apply
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
