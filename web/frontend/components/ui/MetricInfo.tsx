'use client';

import Link from 'next/link';
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ArrowRight, Info } from 'lucide-react';
import { metricDefinitions, type MetricId } from '@/lib/learn/metrics';
import { cn } from '@/lib/utils';

const OPEN_EVENT = 'spectra-metric-info-open';
const WIDTH = 288;
const GAP = 8;
const MARGIN = 8;

type Pos = { top: number; left: number; placement: 'below' | 'above' };

function place(trigger: DOMRect, height: number): Pos {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const width = Math.min(WIDTH, vw - 2 * MARGIN);
  let left = trigger.left + trigger.width / 2 - width / 2;
  left = Math.max(MARGIN, Math.min(left, vw - width - MARGIN));
  const below = trigger.bottom + GAP;
  const fitsBelow = below + height <= vh - MARGIN;
  const fitsAbove = trigger.top - GAP - height >= MARGIN;
  if (fitsBelow || !fitsAbove) return { top: Math.min(below, Math.max(MARGIN, vh - height - MARGIN)), left, placement: 'below' };
  return { top: trigger.top - GAP - height, left, placement: 'above' };
}

/**
 * ⓘ next to a metric name. Hover (mouse), focus (keyboard) or tap (touch) shows the metric's short
 * explanation from the metric registry (lib/learn/metrics.ts) with a link to its lesson.
 *
 * - Screen readers get the explanation as the button's description, without opening anything.
 * - Keyboard: Tab onto ⓘ opens it, Tab again moves to "Learn more", Escape closes it.
 * - The panel is small, closes on outside click, and only one is open at a time, so it never
 *   covers controls for long.
 */
export function InfoTip({ metric, className, label }: { metric: MetricId; className?: string; label?: string }) {
  const def = metricDefinitions[metric];
  const uid = useId();
  const descId = `${uid}-desc`;
  const panelId = `${uid}-panel`;
  const titleId = `${uid}-title`;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const linkRef = useRef<HTMLAnchorElement>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const openTimer = useRef<number | undefined>(undefined);
  const returningFocus = useRef(false);
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [pos, setPos] = useState<Pos | null>(null);
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  const clearTimers = () => {
    window.clearTimeout(closeTimer.current);
    window.clearTimeout(openTimer.current);
  };

  const show = useCallback((pin = false) => {
    clearTimers();
    setOpen(true);
    if (pin) setPinned(true);
    window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: uid }));
  }, [uid]);

  const hide = useCallback(() => {
    clearTimers();
    setOpen(false);
    setPinned(false);
    setPos(null);
  }, []);

  const hideSoon = () => {
    if (pinned) return;
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(hide, 160);
  };

  // One open at a time.
  useEffect(() => {
    const onOther = (e: Event) => {
      if ((e as CustomEvent<string>).detail !== uid) hide();
    };
    window.addEventListener(OPEN_EVENT, onOther);
    return () => window.removeEventListener(OPEN_EVENT, onOther);
  }, [uid, hide]);

  // Position next to the trigger; follow scrolling and resizing; close when the trigger leaves the screen.
  const reposition = useCallback(() => {
    const t = triggerRef.current;
    const p = panelRef.current;
    if (!t) return;
    const r = t.getBoundingClientRect();
    if (r.bottom < 0 || r.top > window.innerHeight) {
      hide();
      return;
    }
    setPos(place(r, p?.offsetHeight ?? 140));
  }, [hide]);

  useLayoutEffect(() => {
    if (!open) return;
    reposition();
    const raf = requestAnimationFrame(reposition); // again once the panel has its real height
    window.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
  }, [open, reposition]);

  // Outside click and Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const n = e.target as Node;
      if (!triggerRef.current?.contains(n) && !panelRef.current?.contains(n)) hide();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        const inside = panelRef.current?.contains(document.activeElement) || triggerRef.current === document.activeElement;
        hide();
        if (inside) {
          returningFocus.current = true;
          triggerRef.current?.focus();
          returningFocus.current = false;
        }
      }
    };
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open, hide]);

  useEffect(() => () => clearTimers(), []);

  const onTriggerKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Tab' && !e.shiftKey && open && linkRef.current) {
      e.preventDefault();
      linkRef.current.focus();
    }
  };
  const onLinkKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'Tab') return;
    if (e.shiftKey) {
      e.preventDefault();
      triggerRef.current?.focus();
    } else {
      // Back to the trigger, then let the browser continue to whatever follows it.
      hide();
      returningFocus.current = true;
      triggerRef.current?.focus();
      returningFocus.current = false;
    }
  };

  const panel =
    open && mounted
      ? createPortal(
          <div
            ref={panelRef}
            id={panelId}
            role="dialog"
            aria-modal="false"
            aria-labelledby={titleId}
            onPointerEnter={() => window.clearTimeout(closeTimer.current)}
            onPointerLeave={(e) => e.pointerType === 'mouse' && hideSoon()}
            style={{ top: pos?.top ?? -9999, left: pos?.left ?? -9999, width: `min(${WIDTH}px, calc(100vw - ${2 * MARGIN}px))`, visibility: pos ? 'visible' : 'hidden' }}
            className="fixed z-[80] rounded-xl border border-ink/10 bg-panel p-3.5 text-left shadow-2xl"
          >
            <p id={titleId} className="text-[13px] font-semibold text-ink">
              {def.name}
            </p>
            <p className="mt-1 text-[12.5px] font-normal normal-case leading-5 tracking-normal text-ink/70">{def.shortDescription}</p>
            {'formula' in def && def.formula && <p className="num mt-2 rounded-md bg-ink/[0.05] px-2 py-1 text-[11.5px] normal-case tracking-normal text-ink/75">{def.formula}</p>}
            {'interpretation' in def && def.interpretation && <p className="mt-2 text-[11.5px] font-normal normal-case leading-[18px] tracking-normal text-ink/55">{def.interpretation}</p>}
            <Link
              ref={linkRef}
              href={def.learnPath}
              onKeyDown={onLinkKeyDown}
              onClick={hide}
              className="mt-2.5 inline-flex items-center gap-1 text-[12.5px] font-semibold normal-case tracking-normal text-accent hover:underline"
            >
              Learn More <ArrowRight className="h-3.5 w-3.5" aria-hidden />
            </Link>
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        data-metric-info={metric}
        aria-label={`About ${label ?? def.name}`}
        aria-describedby={descId}
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onPointerEnter={(e) => {
          if (e.pointerType !== 'mouse') return;
          window.clearTimeout(closeTimer.current);
          openTimer.current = window.setTimeout(() => show(), 120);
        }}
        onPointerLeave={(e) => e.pointerType === 'mouse' && hideSoon()}
        onFocus={(e) => {
          if (!returningFocus.current && e.currentTarget.matches(':focus-visible')) show();
        }}
        onBlur={(e) => {
          const to = e.relatedTarget as Node | null;
          if (!pinned && !(to && panelRef.current?.contains(to))) hideSoon();
        }}
        onClick={(e) => {
          e.stopPropagation();
          e.preventDefault();
          if (open && pinned) hide();
          else show(true);
        }}
        onKeyDown={onTriggerKeyDown}
        className={cn(
          'relative inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full align-middle text-ink/35 transition hover:text-accent focus-visible:text-accent',
          // A 28px touch target without changing the layout.
          'before:absolute before:-inset-1.5 before:content-[""]',
          open && 'text-accent',
          className,
        )}
      >
        <Info className="h-3.5 w-3.5" aria-hidden />
      </button>
      <span id={descId} className="sr-only">
        {def.shortDescription}
      </span>
      {panel}
    </>
  );
}

/** A metric name followed by its ⓘ. */
export function MetricLabel({ metric, children, className }: { metric: MetricId; children: ReactNode; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1', className)}>
      <span>{children}</span>
      <InfoTip metric={metric} label={typeof children === 'string' ? children : undefined} />
    </span>
  );
}
