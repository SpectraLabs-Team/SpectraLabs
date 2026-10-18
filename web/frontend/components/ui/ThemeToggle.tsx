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
      onClick={toggle}
      aria-label={dark ? 'Switch to light mode' : 'Switch to dark mode'}
      title={dark ? 'Light mode' : 'Dark mode'}
      className={cn('relative flex h-9 w-9 items-center justify-center overflow-hidden rounded-lg border border-ink/10 bg-ink/[0.03] text-ink/70 transition hover:border-ink/20 hover:text-ink', className)}
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.span key={theme} initial={{ y: 12, opacity: 0, rotate: -40 }} animate={{ y: 0, opacity: 1, rotate: 0 }} exit={{ y: -12, opacity: 0, rotate: 40 }} transition={{ duration: 0.2 }}>
          {dark ? <Moon className="h-4 w-4" /> : <Sun className="h-4 w-4" />}
        </motion.span>
      </AnimatePresence>
    </button>
  );
}
