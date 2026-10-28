'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { BookOpen, Check, Copy, FileCode2, Plus, Sparkles, X } from 'lucide-react';
import { useState } from 'react';
import { COMMON_STRATEGY_TEMPLATES, type CommonStrategyTemplate } from './commonStrategies';

interface CommonStrategiesModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectTemplate: (template: CommonStrategyTemplate, mode: 'load' | 'create') => void;
}

export function CommonStrategiesModal({
  isOpen,
  onClose,
  onSelectTemplate,
}: CommonStrategiesModalProps) {
  const [selected, setSelected] = useState<CommonStrategyTemplate>(COMMON_STRATEGY_TEMPLATES[0]);
  const [copied, setCopied] = useState(false);

  if (!isOpen) return null;

  const handleCopy = () => {
    navigator.clipboard.writeText(selected.code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

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
          aria-labelledby="common-strategies-title"
          initial={{ opacity: 0, scale: 0.95, y: 15 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 15 }}
          className="relative z-10 flex max-h-[90vh] w-full max-w-4xl flex-col rounded-2xl border border-ink/10 bg-panel shadow-2xl overflow-hidden"
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-ink/[0.08] px-6 py-4">
            <div className="flex items-center gap-2.5">
              <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent/10 text-accent">
                <BookOpen className="h-5 w-5" />
              </div>
              <div>
                <h2 id="common-strategies-title" className="text-base font-bold text-ink">
                  Common Quant Strategies
                </h2>
                <p className="text-xs text-ink/50">
                  Battle-tested strategy templates written in Spectra's Restricted Safe DSL.
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

          {/* Body */}
          <div className="grid flex-1 grid-cols-1 overflow-hidden md:grid-cols-5">
            {/* Strategy List (2 cols) */}
            <div className="space-y-2 overflow-y-auto border-r border-ink/[0.08] bg-canvas/30 p-4 md:col-span-2">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-ink/50">
                Templates ({COMMON_STRATEGY_TEMPLATES.length})
              </span>
              <div className="mt-2 space-y-2">
                {COMMON_STRATEGY_TEMPLATES.map((strat) => {
                  const isSelected = selected.id === strat.id;
                  return (
                    <button
                      key={strat.id}
                      type="button"
                      onClick={() => setSelected(strat)}
                      className={`flex w-full flex-col items-start rounded-xl border p-3 text-left transition ${
                        isSelected
                          ? 'border-accent bg-accent/10 shadow-sm'
                          : 'border-ink/10 bg-panel/60 hover:border-ink/20 hover:bg-ink/[0.03]'
                      }`}
                    >
                      <div className="flex w-full items-center justify-between">
                        <span className="rounded-full bg-ink/[0.06] px-2 py-0.5 text-[10px] font-medium text-ink/60">
                          {strat.category}
                        </span>
                        {isSelected && <Sparkles className="h-3.5 w-3.5 text-accent" />}
                      </div>
                      <span className="mt-1.5 text-xs font-semibold text-ink">{strat.name}</span>
                      <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-ink/60">
                        {strat.description}
                      </p>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Code & Details View (3 cols) */}
            <div className="flex flex-col overflow-y-auto p-5 md:col-span-3">
              <div>
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-bold text-ink">{selected.name}</h3>
                  <button
                    type="button"
                    onClick={handleCopy}
                    className="inline-flex items-center gap-1 rounded-lg border border-ink/10 bg-canvas/60 px-2.5 py-1 text-[11px] font-medium text-ink/70 transition hover:bg-ink/[0.05]"
                  >
                    {copied ? <Check className="h-3 w-3 text-profit" /> : <Copy className="h-3 w-3" />}
                    {copied ? 'Copied' : 'Copy Code'}
                  </button>
                </div>
                <p className="mt-1 text-xs text-ink/65 leading-relaxed">{selected.description}</p>
              </div>

              {/* Code Preview */}
              <div className="mt-4 flex-1 overflow-hidden rounded-xl border border-ink/10 bg-canvas/90">
                <div className="border-b border-ink/10 px-3 py-1.5 text-[10.5px] font-mono text-ink/50">
                  {selected.id}.dsl
                </div>
                <pre className="max-h-64 overflow-y-auto p-3 font-mono text-xs text-ink/80 leading-relaxed selection:bg-accent/20">
                  <code>{selected.code}</code>
                </pre>
              </div>

              {/* Actions */}
              <div className="mt-5 flex items-center justify-end gap-2.5">
                <button
                  type="button"
                  onClick={() => {
                    onSelectTemplate(selected, 'load');
                    onClose();
                  }}
                  className="rounded-xl border border-ink/15 px-3.5 py-2 text-xs font-semibold text-ink/80 transition hover:bg-ink/[0.05] hover:text-ink"
                >
                  <FileCode2 className="mr-1.5 inline-block h-3.5 w-3.5" />
                  Load into Current Editor
                </button>
                <button
                  type="button"
                  onClick={() => {
                    onSelectTemplate(selected, 'create');
                    onClose();
                  }}
                  className="inline-flex items-center gap-1.5 rounded-xl bg-accent px-4 py-2 text-xs font-bold text-onaccent shadow-glow transition hover:brightness-110 active:scale-95"
                >
                  <Plus className="h-4 w-4" />
                  Create as New Strategy
                </button>
              </div>
            </div>
          </div>
        </motion.div>
      </div>
    </AnimatePresence>
  );
}
