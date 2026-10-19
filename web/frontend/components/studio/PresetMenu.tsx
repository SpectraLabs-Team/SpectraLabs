'use client';

import { useEffect, useRef, useState } from 'react';
import { ChevronDown, Layers } from 'lucide-react';
import { PRESETS } from '@/lib/presets';
import { parseCsv, portfolioStats } from '@/lib/portfolio';
import { cn, formatCompactCurrency } from '@/lib/utils';

// Book size and position count per preset, computed once for the menu.
const SUMMARY = Object.fromEntries(
  PRESETS.map((p) => {
    const pos = parseCsv(p.csv).positions;
    return [p.id, { n: pos.length, gross: portfolioStats(pos).grossExposure }];
  }),
);

/** "Presets ▾" button that loads one of the institutional books into the Portfolio Studio. */
export function PresetMenu({ onSelect }: { onSelect: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cn('btn-secondary px-3 py-1.5 text-xs', open && 'border-accent/40 text-accent')}
      >
        <Layers className="h-3.5 w-3.5" /> Presets <ChevronDown className={cn('h-3 w-3 transition-transform', open && 'rotate-180')} />
      </button>
      {open && (
        <div role="menu" className="absolute right-0 z-30 mt-2 w-[min(340px,calc(100vw-2rem))] overflow-hidden rounded-xl border border-ink/10 bg-panel shadow-panel">
          <p className="border-b border-ink/[0.06] px-3.5 py-2 font-mono text-[10px] uppercase tracking-[0.16em] text-ink/45">Institutional books · illustrative prices</p>
          {PRESETS.map((p) => (
            <button
              key={p.id}
              type="button"
              role="menuitem"
              onClick={() => {
                onSelect(p.id);
                setOpen(false);
              }}
              className="block w-full border-b border-ink/[0.05] px-3.5 py-2.5 text-left transition last:border-0 hover:bg-accent/[0.06] focus:bg-accent/[0.06] focus:outline-none"
            >
              <span className="flex items-baseline justify-between gap-3">
                <span className="text-[13px] font-semibold text-ink">{p.name}</span>
                <span className="num shrink-0 text-[11px] text-ink/50">
                  {SUMMARY[p.id].n} pos · {formatCompactCurrency(SUMMARY[p.id].gross)}
                </span>
              </span>
              <span className="mt-0.5 block text-[11.5px] leading-4 text-ink/55">{p.blurb}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
