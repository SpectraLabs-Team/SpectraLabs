'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { Moon, Sun } from 'lucide-react';
import { useTheme } from '@/lib/theme';
import { cn } from '@/lib/utils';

export function ThemeToggle({ className }: { className?: string }) {
  const { theme, toggle } = useTheme();
  const dark = theme === 'dark';
  return (
    <button
      type="button"
      onClick={(e) => toggle(e)}
      aria-label={dark ? 'Switch to light mode' : 'Switch to dark mode'}
      title={dark ? 'Light mode' : 'Dark mode'}
      className={cn(
        'relative isolate flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border transition-colors',
        'border-slate-300 bg-slate-100 text-slate-700 hover:border-slate-400 hover:text-slate-900',
        'dark:border-white/15 dark:bg-white/5 dark:text-slate-200 dark:hover:border-white/30 dark:hover:text-white',
        className,
      )}
    >
      {/* No exit animation and no opacity fade: the icon is fully opaque in every frame, so the
          view-transition snapshot (old + new) never captures a half-faded glyph. */}
      <AnimatePresence initial={false}>
        <motion.span
          key={theme}
          className="absolute inset-0 flex items-center justify-center"
          initial={{ rotate: -90, scale: 0.6 }}
          animate={{ rotate: 0, scale: 1 }}
          transition={{ type: 'spring', stiffness: 420, damping: 26 }}
        >
          {dark ? <Moon className="h-4 w-4" strokeWidth={2.25} /> : <Sun className="h-4 w-4" strokeWidth={2.25} />}
        </motion.span>
      </AnimatePresence>
    </button>
  );
}
