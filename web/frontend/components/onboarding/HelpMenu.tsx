'use client';

import Link from 'next/link';
import { useEffect, useId, useRef, useState } from 'react';
import { BookOpen, Compass, Gauge, HelpCircle, MapPinned } from 'lucide-react';
import type { StudioTab } from '@/components/studio/StudioShell';
import { cn } from '@/lib/utils';
import { useTour } from './OnboardingTour';

/** Studio header Help menu: restart the tour, a page tour where one exists, and the Learning Center. */
export function HelpMenu({ tab, className }: { tab: StudioTab; className?: string }) {
  const { startTour, startPageTour, hasPageTour } = useTour();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        ref.current?.querySelector<HTMLButtonElement>('button[aria-haspopup]')?.focus();
      }
    };
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey);
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const onMenuKey = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    const items = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[(i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
  };

  const item = 'flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] text-ink/80 transition hover:bg-ink/[0.05] hover:text-ink focus-visible:bg-ink/[0.05]';
  const run = (f: () => void) => () => {
    setOpen(false);
    f();
  };

  return (
    <div ref={ref} className={cn('relative', className)}>
      <button
        type="button"
        data-tour="help-menu"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={menuId}
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[12.5px] font-medium text-ink/65 transition hover:bg-ink/[0.05] hover:text-ink"
      >
        <HelpCircle className="h-4 w-4" aria-hidden /> <span className="hidden md:inline">Help</span>
        <span className="sr-only md:hidden">Help</span>
      </button>
      {open && (
        <div id={menuId} role="menu" aria-label="Help" onKeyDown={onMenuKey} className="absolute right-0 top-full z-40 mt-2 w-64 rounded-xl border border-ink/10 bg-panel p-1.5 shadow-2xl">
          <button type="button" role="menuitem" className={item} onClick={run(startTour)}>
            <Compass className="h-4 w-4 text-accent" aria-hidden /> Take the tour
          </button>
          {hasPageTour(tab) && (
            <button type="button" role="menuitem" className={item} onClick={run(() => startPageTour(tab))}>
              <MapPinned className="h-4 w-4 text-accent" aria-hidden /> Tour this page
            </button>
          )}
          <div className="my-1 h-px bg-ink/[0.07]" role="separator" />
          <Link href="/learn" role="menuitem" className={item} onClick={() => setOpen(false)}>
            <BookOpen className="h-4 w-4 text-ink/50" aria-hidden /> Learning Center
          </Link>
          <Link href="/learn/metrics" role="menuitem" className={item} onClick={() => setOpen(false)}>
            <Gauge className="h-4 w-4 text-ink/50" aria-hidden /> Metric explanations
          </Link>
        </div>
      )}
    </div>
  );
}
