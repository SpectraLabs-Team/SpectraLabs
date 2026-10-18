'use client';

import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';

export type Theme = 'dark' | 'light';
const KEY = 'spectra-theme';

/**
 * Runs before first paint (inlined in <head>) so the stored theme applies without a flash.
 * Dark is the default; only an explicit "light" choice switches it off.
 */
export const themeInitScript = `(function(){try{var t=localStorage.getItem('${KEY}');if(t==='light'){document.documentElement.classList.remove('dark')}else{document.documentElement.classList.add('dark')}}catch(e){}})();`;

const Ctx = createContext<{ theme: Theme; toggle: (e?: React.MouseEvent) => void }>({ theme: 'dark', toggle: () => {} });

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>('dark');
  useEffect(() => {
    setTheme(document.documentElement.classList.contains('dark') ? 'dark' : 'light');
  }, []);

  const toggle = useCallback((e?: React.MouseEvent) => {
    const isDark = document.documentElement.classList.contains('dark');
    const next: Theme = isDark ? 'light' : 'dark';

    const hasViewTransitions =
      typeof document !== 'undefined' &&
      'startViewTransition' in document &&
      !window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    if (!hasViewTransitions) {
      document.documentElement.classList.toggle('dark', next === 'dark');
      setTheme(next);
      try {
        localStorage.setItem(KEY, next);
      } catch {}
      return;
    }

    const x = e?.clientX ?? window.innerWidth - 80;
    const y = e?.clientY ?? 32;
    const endRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y),
    );

    const transition = (document as any).startViewTransition(async () => {
      document.documentElement.classList.toggle('dark', next === 'dark');
      setTheme(next);
      try {
        localStorage.setItem(KEY, next);
      } catch {}
    });

    transition.ready.then(() => {
      document.documentElement.animate(
        {
          clipPath: [
            `circle(0px at ${x}px ${y}px)`,
            `circle(${endRadius}px at ${x}px ${y}px)`,
          ],
        },
        {
          duration: 480,
          easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
          pseudoElement: '::view-transition-new(root)',
        },
      );
    });
  }, []);

  return <Ctx.Provider value={{ theme, toggle }}>{children}</Ctx.Provider>;
}

export const useTheme = () => useContext(Ctx);

/** Colours for SVG chart props (presentation attributes can't read CSS variables). */
export function chartColors(theme: Theme) {
  const dark = theme === 'dark';
  return {
    grid: dark ? 'rgba(255,255,255,0.05)' : 'rgba(15,23,42,0.07)',
    axis: dark ? 'rgba(255,255,255,0.08)' : 'rgba(15,23,42,0.12)',
    tick: dark ? '#64748B' : '#64748B',
    label: dark ? '#CBD5E1' : '#334155',
    cursor: dark ? 'rgba(255,255,255,0.25)' : 'rgba(15,23,42,0.25)',
    tooltip: { background: dark ? '#0B0F17' : '#FFFFFF', border: `1px solid ${dark ? 'rgba(255,255,255,0.1)' : 'rgba(15,23,42,0.1)'}`, borderRadius: 10, fontSize: 12, color: dark ? '#E2E8F0' : '#0F172A' },
    accent: dark ? '#22D3EE' : '#0891B2',
    electric: dark ? '#00F2FE' : '#06B6D4',
    violet: dark ? '#A78BFA' : '#7C3AED',
    loss: dark ? '#F43F5E' : '#E11D48',
    lossSoft: dark ? '#FDA4AF' : '#FB7185',
    numpy: dark ? '#A78BFA' : '#7C3AED',
    muted: dark ? '#64748B' : '#94A3B8',
  };
}
