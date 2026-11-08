'use client';

import React, { useState } from 'react';
import {
  AlertTriangle,
  FolderOpen,
  LayoutTemplate,
  Plus,
  Save,
  Trash2,
  X,
} from 'lucide-react';
import type { ChartTemplate } from '@/lib/chart-drawings';
import { cn } from '@/lib/utils';

export interface TemplateModalProps {
  isOpen: boolean;
  onClose: () => void;
  templates: ChartTemplate[];
  currentIndicatorCount: number;
  currentDrawingCount: number;
  currentComparisonCount: number;
  isViewer?: boolean;
  onSaveTemplate: (name: string) => Promise<void>;
  onLoadTemplate: (template: ChartTemplate) => void;
  onDeleteTemplate: (templateId: string) => Promise<void>;
}

export function TemplateModal({
  isOpen,
  onClose,
  templates,
  currentIndicatorCount,
  currentDrawingCount,
  currentComparisonCount,
  isViewer = false,
  onSaveTemplate,
  onLoadTemplate,
  onDeleteTemplate,
}: TemplateModalProps) {
  const [activeTab, setActiveTab] = useState<'load' | 'save'>('load');
  const [templateName, setTemplateName] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!isOpen) return null;

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    if (isViewer) {
      setError('Viewers have read-only access to templates.');
      return;
    }
    const name = templateName.trim();
    if (!name) {
      setError('Please enter a template name.');
      return;
    }
    if (name.length > 100) {
      setError('Template name cannot exceed 100 characters.');
      return;
    }
    if (templates.length >= 50) {
      setError('Workspace limit of 50 templates reached.');
      return;
    }

    try {
      setIsSaving(true);
      setError(null);
      await onSaveTemplate(name);
      setTemplateName('');
      setActiveTab('load');
    } catch (err: any) {
      setError(err?.message || 'Failed to save template');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm animate-in fade-in duration-200"
      onClick={onClose}
    >
      <div
        className="relative w-full max-w-lg rounded-2xl border border-ink/10 bg-panel p-6 shadow-2xl transition-all"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Chart Templates"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink/10 pb-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent/15 text-accent">
              <LayoutTemplate className="h-5 w-5" />
            </div>
            <div>
              <h2 className="text-base font-bold text-ink">Chart Layout Templates</h2>
              <p className="text-xs text-ink/60">
                Save & load chart layouts across indicators, drawings, and comparisons
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

        {/* Tabs */}
        <div className="mt-4 flex border-b border-ink/10">
          <button
            type="button"
            onClick={() => {
              setActiveTab('load');
              setError(null);
            }}
            className={cn(
              'flex items-center gap-2 border-b-2 px-4 py-2 text-xs font-semibold transition-all',
              activeTab === 'load'
                ? 'border-accent text-accent'
                : 'border-transparent text-ink/60 hover:text-ink'
            )}
          >
            <FolderOpen className="h-4 w-4" />
            Saved Templates ({templates.length}/50)
          </button>
          <button
            type="button"
            onClick={() => {
              setActiveTab('save');
              setError(null);
            }}
            className={cn(
              'flex items-center gap-2 border-b-2 px-4 py-2 text-xs font-semibold transition-all',
              activeTab === 'save'
                ? 'border-accent text-accent'
                : 'border-transparent text-ink/60 hover:text-ink'
            )}
          >
            <Save className="h-4 w-4" />
            Save Current Layout
          </button>
        </div>

        {/* Tab 1: Load Templates */}
        {activeTab === 'load' && (
          <div className="mt-4">
            {templates.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-10 text-center">
                <LayoutTemplate className="h-10 w-10 text-ink/20" />
                <p className="mt-2 text-sm font-medium text-ink/60">No saved templates yet</p>
                <p className="text-xs text-ink/40">Save your current setup to access it anytime</p>
                <button
                  type="button"
                  onClick={() => setActiveTab('save')}
                  className="mt-3 flex items-center gap-1.5 rounded-xl bg-accent/10 px-3.5 py-1.5 text-xs font-semibold text-accent hover:bg-accent/20 transition-colors"
                >
                  <Plus className="h-3.5 w-3.5" />
                  Save current setup
                </button>
              </div>
            ) : (
              <div className="flex flex-col gap-2 max-h-72 overflow-y-auto pr-1">
                {templates.map((tmpl) => {
                  const hasRetired =
                    (tmpl.retired_versions && tmpl.retired_versions.length > 0) ||
                    (tmpl.retired_dataset_versions && tmpl.retired_dataset_versions.length > 0);
                  const indCount = Array.isArray(tmpl.indicators) ? tmpl.indicators.length : 0;
                  const drawCount = Array.isArray(tmpl.drawings) ? tmpl.drawings.length : 0;
                  const compCount = Array.isArray(tmpl.comparisons) ? tmpl.comparisons.length : 0;

                  return (
                    <div
                      key={tmpl.id}
                      className="group flex items-center justify-between rounded-xl border border-ink/10 bg-canvas/60 p-3 text-sm hover:border-accent/40 transition-all"
                    >
                      <div className="flex flex-col gap-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="font-semibold text-ink truncate">{tmpl.name}</span>
                          <span className="rounded bg-ink/[0.06] px-1.5 py-0.2 text-[10px] font-mono text-ink/60">
                            v{tmpl.version}
                          </span>
                          {hasRetired && (
                            <span
                              className="flex items-center gap-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-400"
                              title="One or more datasets in this template are retired or missing"
                            >
                              <AlertTriangle className="h-3 w-3" />
                              Retired Dataset
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-3 text-xs text-ink/50">
                          <span>{indCount} indicators</span>
                          <span>•</span>
                          <span>{drawCount} drawings</span>
                          <span>•</span>
                          <span>{compCount} comparisons</span>
                        </div>
                      </div>

                      <div className="flex items-center gap-2">
                        <button
                          type="button"
                          onClick={() => {
                            onLoadTemplate(tmpl);
                            onClose();
                          }}
                          className="rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-white shadow-sm hover:opacity-95 transition-opacity"
                        >
                          Apply
                        </button>
                        {!isViewer && (
                          <button
                            type="button"
                            onClick={() => onDeleteTemplate(tmpl.id)}
                            className="rounded-lg p-1.5 text-ink/40 hover:bg-rose-500/10 hover:text-rose-500 transition-colors"
                            title="Delete template"
                            aria-label={`Delete template ${tmpl.name}`}
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {/* Tab 2: Save Template */}
        {activeTab === 'save' && (
          <form onSubmit={handleSave} className="mt-4 flex flex-col gap-4">
            {isViewer && (
              <div className="rounded-xl border border-rose-500/20 bg-rose-500/10 p-3 text-xs text-rose-500 font-medium">
                Viewers have read-only access. You cannot create or modify templates in this workspace.
              </div>
            )}

            {/* Current layout summary */}
            <div className="rounded-xl border border-ink/10 bg-canvas/40 p-3.5">
              <span className="text-xs font-semibold text-ink/70 uppercase tracking-wider">Layout Contents:</span>
              <div className="mt-2 grid grid-cols-3 gap-2 text-center">
                <div className="rounded-lg bg-panel p-2">
                  <div className="text-lg font-bold text-ink">{currentIndicatorCount}</div>
                  <div className="text-[11px] text-ink/60">Indicators</div>
                </div>
                <div className="rounded-lg bg-panel p-2">
                  <div className="text-lg font-bold text-ink">{currentDrawingCount}</div>
                  <div className="text-[11px] text-ink/60">Drawings</div>
                </div>
                <div className="rounded-lg bg-panel p-2">
                  <div className="text-lg font-bold text-ink">{currentComparisonCount}</div>
                  <div className="text-[11px] text-ink/60">Comparisons</div>
                </div>
              </div>
            </div>

            <div>
              <label htmlFor="template-name-input" className="block text-xs font-medium text-ink/70">
                Template Name
              </label>
              <input
                id="template-name-input"
                type="text"
                value={templateName}
                onChange={(e) => {
                  setTemplateName(e.target.value);
                  setError(null);
                }}
                disabled={isViewer || isSaving}
                placeholder="e.g. Trend Momentum & Levels"
                maxLength={100}
                className="mt-1.5 w-full rounded-xl border border-ink/15 bg-canvas px-3.5 py-2 text-sm text-ink placeholder:text-ink/40 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent disabled:opacity-50"
                autoFocus
              />
              <span className="mt-1 block text-right text-[10px] text-ink/40">
                {templateName.length}/100
              </span>
            </div>

            {error && <p className="text-xs font-medium text-rose-500">{error}</p>}

            <div className="flex justify-end gap-2 border-t border-ink/10 pt-4">
              <button
                type="button"
                onClick={onClose}
                className="rounded-xl border border-ink/15 bg-canvas px-4 py-2 text-xs font-semibold text-ink hover:bg-ink/[0.05] transition-colors"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={isViewer || isSaving || !templateName.trim()}
                className="flex items-center gap-1.5 rounded-xl bg-accent px-4 py-2 text-xs font-semibold text-white shadow-sm hover:opacity-95 disabled:opacity-40 transition-all"
              >
                <Save className="h-3.5 w-3.5" />
                {isSaving ? 'Saving...' : 'Save Template'}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
