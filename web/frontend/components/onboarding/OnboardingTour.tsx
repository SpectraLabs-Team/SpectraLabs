'use client';

import 'driver.js/dist/driver.css';
import { driver, type Driver, type DriveStep } from 'driver.js';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { BookOpen, Compass, LayoutDashboard, PartyPopper, X } from 'lucide-react';
import type { StudioTab } from '@/components/studio/StudioShell';
import { displayName, useAuth } from '@/lib/auth';
import { shouldAutoStart } from '@/lib/onboarding';
import { MAIN_TOUR, PAGE_TOURS, type TourStep } from './tourSteps';
import { useOnboardingState } from './useOnboardingState';

type TourContext = {
  /** Start the main tour from the beginning (Help menu, Learning Center). */
  startTour: () => void;
  /** Start the short tour of a page, if it has one. */
  startPageTour: (tab: StudioTab) => void;
  hasPageTour: (tab: StudioTab) => boolean;
  active: boolean;
};

const Ctx = createContext<TourContext>({ startTour: () => {}, startPageTour: () => {}, hasPageTour: () => false, active: false });
export const useTour = () => useContext(Ctx);

/** Visible on screen: rendered, not display:none (or inside a hidden parent), not zero-sized. */
function isVisible(el: Element): boolean {
  if (!(el instanceof HTMLElement) || el.getClientRects().length === 0) return false;
  const r = el.getBoundingClientRect();
  if (r.width < 2 || r.height < 2) return false;
  return getComputedStyle(el).visibility !== 'hidden';
}

export function findTarget(targets: string[]): HTMLElement | undefined {
  for (const t of targets) {
    for (const el of document.querySelectorAll(`[data-tour="${t}"]`)) if (isVisible(el)) return el as HTMLElement;
  }
  return undefined;
}

/** Resolve once one of the targets is visible, or after `timeoutMs` (the step then shows centred). */
function waitForTarget(targets: string[], timeoutMs = 2500): Promise<HTMLElement | undefined> {
  return new Promise((resolve) => {
    const start = performance.now();
    const tick = () => {
      const el = findTarget(targets);
      if (el || performance.now() - start > timeoutMs) resolve(el);
      else requestAnimationFrame(tick);
    };
    tick();
  });
}

function Dialog({ labelledBy, onEscape, children }: { labelledBy: string; onEscape: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    el?.querySelector<HTMLElement>('[data-autofocus]')?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onEscape();
      } else if (e.key === 'Tab' && el) {
        const items = [...el.querySelectorAll<HTMLElement>('button, a[href]')];
        if (!items.length) return;
        const i = items.indexOf(document.activeElement as HTMLElement);
        if (e.shiftKey && i <= 0) {
          e.preventDefault();
          items[items.length - 1].focus();
        } else if (!e.shiftKey && i === items.length - 1) {
          e.preventDefault();
          items[0].focus();
        }
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onEscape]);
  return (
    <div className="fixed inset-0 z-[70] flex items-end justify-center bg-black/55 p-4 backdrop-blur-[2px] sm:items-center" role="presentation">
      <div ref={ref} role="dialog" aria-modal="true" aria-labelledby={labelledBy} className="relative w-full max-w-md rounded-2xl border border-ink/10 bg-panel p-6 shadow-2xl">
        {children}
      </div>
    </div>
  );
}

/**
 * The guided website tour (driver.js) and its welcome and completion dialogs.
 *
 * Starts by itself once per user and tour version (see lib/onboarding.ts), after the Studio has
 * rendered; never again after the user finishes or dismisses it. It only highlights and explains:
 * the highlighted element cannot be clicked while a step is shown, and no step submits anything.
 */
export function OnboardingProvider({ tab, tabs, onTab, children }: { tab: StudioTab; tabs: readonly StudioTab[]; onTab: (t: StudioTab) => void; children: ReactNode }) {
  const { user } = useAuth();
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const { state, ready, record } = useOnboardingState(user?.email);
  const [welcome, setWelcome] = useState<null | 'new' | 'resume'>(null);
  const [finished, setFinished] = useState(false);
  const [active, setActive] = useState(false);
  const [decided, setDecided] = useState(false);
  const driverRef = useRef<Driver | null>(null);
  const tabRef = useRef(tab);
  tabRef.current = tab;
  const recordRef = useRef(record);
  recordRef.current = record;

  const stop = useCallback(() => {
    const d = driverRef.current;
    driverRef.current = null;
    if (d?.isActive()) d.destroy();
    setActive(false);
  }, []);

  const run = useCallback(
    async (kind: 'main' | 'page', all: TourStep[]) => {
      stop();
      setWelcome(null);
      setFinished(false);
      // Only features this Studio actually has.
      const tabExists = (t?: StudioTab) => !t || tabs.includes(t);
      let steps = all.filter((s) => tabExists(s.requiresTab) && tabExists(s.tab));
      if (kind === 'page') steps = steps.filter((s) => findTarget(s.targets));
      if (!steps.length) return;

      let index = 0;
      let busy = false;

      /** The user ends the tour early (Skip, ×, Escape): remember it so it does not reopen. */
      const dismiss = () => {
        if (kind === 'main') recordRef.current('skip', steps[index]?.id ?? null);
        driverRef.current = null;
        setActive(false);
        d.destroy();
      };

      const go = async (i: number) => {
        if (busy || i < 0 || i >= steps.length) return;
        busy = true;
        // Show that something is happening while another tab loads (driver re-renders the card on move).
        document.querySelector('.driver-popover')?.classList.add('spectra-tour-busy');
        const s = steps[i];
        const switching = !!s.tab && tabRef.current !== s.tab;
        if (switching) onTab(s.tab!);
        await waitForTarget(s.targets, s.tab ? 2500 : 600);
        // Let the tab's enter animation settle so the highlight lands where the element ends up.
        if (switching) await new Promise((r) => window.setTimeout(r, 240));
        busy = false;
        if (driverRef.current !== d) return; // stopped meanwhile
        index = i;
        if (d.isActive()) d.moveTo(i);
        else d.drive(i);
      };

      const driveSteps: DriveStep[] = steps.map((s, i) => ({
        element: (() => findTarget(s.targets)) as () => Element,
        popover: {
          title: s.title,
          description: s.body,
          side: s.side,
          align: 'start',
          doneBtnText: kind === 'main' ? 'Finish' : 'Done',
          showButtons: i === 0 ? ['next', 'close'] : ['next', 'previous', 'close'],
        },
        data: { id: s.id },
      }));

      const d: Driver = driver({
        steps: driveSteps,
        animate: !window.matchMedia('(prefers-reduced-motion: reduce)').matches,
        smoothScroll: true,
        allowClose: true,
        allowKeyboardControl: true,
        overlayClickBehavior: () => {},
        disableActiveInteraction: true,
        showProgress: true,
        progressText: 'Step {{current}} of {{total}}',
        nextBtnText: 'Next',
        prevBtnText: 'Back',
        closeBtnLabel: 'Close the tour',
        stagePadding: 6,
        stageRadius: 10,
        overlayColor: '#020617',
        overlayOpacity: 0.62,
        popoverClass: 'spectra-tour',
        onPopoverRender: (popover) => {
          const skip = document.createElement('button');
          skip.type = 'button';
          skip.className = 'spectra-tour-skip';
          skip.textContent = kind === 'main' ? 'Skip tour' : 'Close';
          skip.addEventListener('click', dismiss);
          popover.footer.insertBefore(skip, popover.footer.firstChild);
        },
        onHighlighted: (_el, step) => {
          if (kind === 'main' && step.data?.id) recordRef.current('progress', String(step.data.id));
        },
        onNextClick: () => {
          if (index >= steps.length - 1) {
            driverRef.current = null;
            d.destroy();
            if (kind === 'main') {
              recordRef.current('complete', 'finish');
              setFinished(true);
            }
            return;
          }
          void go(index + 1);
        },
        onPrevClick: () => void go(index - 1),
        // Close button and Escape. (Calling d.destroy() directly does not run this hook.)
        onDestroyStarted: dismiss,
        onDestroyed: () => {
          if (driverRef.current === d) driverRef.current = null;
          setActive(false);
        },
      });
      driverRef.current = d;
      setActive(true);
      await go(0);
    },
    [onTab, stop, tabs],
  );

  const startTour = useCallback(() => void run('main', MAIN_TOUR), [run]);
  const startPageTour = useCallback((t: StudioTab) => {
    const steps = PAGE_TOURS[t];
    if (steps) void run('page', steps);
  }, [run]);
  const hasPageTour = useCallback((t: StudioTab) => !!PAGE_TOURS[t], []);

  // "Take the tour" from the Learning Center (/studio?tour=1): drop the flag from the URL, then
  // start from the beginning. A ref (not state) guards it so re-renders cannot cancel the start.
  const tourParamHandled = useRef(false);
  const wantsTour = params.get('tour') === '1';
  useEffect(() => {
    if (!ready || !wantsTour || tourParamHandled.current) return;
    tourParamHandled.current = true;
    setDecided(true);
    const next = new URLSearchParams(params.toString());
    next.delete('tour');
    const qs = next.toString();
    router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    window.setTimeout(startTour, 300);
  }, [ready, wantsTour, params, pathname, router, startTour]);

  // First visit: decide once the user's tour state is known and the Studio has rendered.
  useEffect(() => {
    if (!ready || decided || wantsTour || tourParamHandled.current) return;
    if (!shouldAutoStart(state)) {
      setDecided(true);
      return;
    }
    // Wait for the main interface (sidebar and content) before greeting.
    let cancelled = false;
    void waitForTarget(['studio-main'], 4000).then(() => {
      if (cancelled) return;
      window.setTimeout(() => {
        if (cancelled) return;
        setWelcome(state?.lastCompletedStep && state.lastCompletedStep !== 'welcome' ? 'resume' : 'new');
        setDecided(true);
      }, 400);
    });
    return () => {
      cancelled = true;
    };
  }, [ready, decided, wantsTour, state]);

  // Leaving the Studio mid-tour: tidy up the overlay (the progress already saved stays).
  useEffect(() => () => stop(), [stop]);

  const skipWelcome = useCallback(() => {
    setWelcome(null);
    record('skip', 'welcome');
  }, [record]);
  const closeFinish = useCallback(() => setFinished(false), []);

  const ctx = useMemo(() => ({ startTour, startPageTour, hasPageTour, active }), [startTour, startPageTour, hasPageTour, active]);
  const name = user ? displayName(user).split(' ')[0] : '';

  return (
    <Ctx.Provider value={ctx}>
      {children}
      {welcome && (
        <Dialog labelledBy="tour-welcome-title" onEscape={skipWelcome}>
          <button type="button" onClick={skipWelcome} className="absolute right-3 top-3 rounded-md p-1.5 text-ink/45 transition hover:bg-ink/5 hover:text-ink" aria-label="Skip for now">
            <X className="h-4 w-4" />
          </button>
          <span className="flex h-11 w-11 items-center justify-center rounded-xl border border-accent/30 bg-accent/10 text-accent">
            <Compass className="h-5 w-5" aria-hidden />
          </span>
          <h2 id="tour-welcome-title" className="mt-4 text-lg font-semibold text-ink">
            {welcome === 'resume' ? 'Pick up the tour?' : `Welcome to Spectra${name ? `, ${name}` : ''}!`}
          </h2>
          <p className="mt-2 text-[14px] leading-6 text-ink/65">
            {welcome === 'resume'
              ? 'You started the tour last time without finishing it. It takes about a minute.'
              : 'Explore trading strategies, run simulations, analyze performance, learn trading concepts, and practice with paper trading. Let’s take a quick tour to help you get started.'}
          </p>
          <div className="mt-6 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <button type="button" onClick={skipWelcome} className="btn-secondary">
              Skip for Now
            </button>
            <button type="button" data-autofocus onClick={startTour} className="btn-primary">
              {welcome === 'resume' ? 'Start the Tour' : 'Start Tour'}
            </button>
          </div>
          <p className="mt-4 text-[11.5px] text-ink/45">You can take the tour again any time from the Help menu.</p>
        </Dialog>
      )}
      {finished && (
        <Dialog labelledBy="tour-done-title" onEscape={closeFinish}>
          <button type="button" onClick={closeFinish} className="absolute right-3 top-3 rounded-md p-1.5 text-ink/45 transition hover:bg-ink/5 hover:text-ink" aria-label="Close">
            <X className="h-4 w-4" />
          </button>
          <span className="flex h-11 w-11 items-center justify-center rounded-xl border border-profit/30 bg-profit/10 text-profit">
            <PartyPopper className="h-5 w-5" aria-hidden />
          </span>
          <h2 id="tour-done-title" className="mt-4 text-lg font-semibold text-ink">
            You&apos;re all set!
          </h2>
          <p className="mt-2 text-[14px] leading-6 text-ink/65">You can now explore the platform, run simulations, practice trading, and learn at your own pace.</p>
          <div className="mt-6 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <button
              type="button"
              onClick={() => {
                setFinished(false);
                router.push('/learn');
              }}
              className="btn-secondary"
            >
              <BookOpen className="h-4 w-4" /> Explore Learning Center
            </button>
            <button
              type="button"
              data-autofocus
              onClick={() => {
                setFinished(false);
                onTab('overview');
              }}
              className="btn-primary"
            >
              <LayoutDashboard className="h-4 w-4" /> Go to Dashboard
            </button>
          </div>
        </Dialog>
      )}
    </Ctx.Provider>
  );
}
