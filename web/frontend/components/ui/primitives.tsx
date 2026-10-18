'use client';

import { motion, AnimatePresence } from 'framer-motion';
import { Loader2 } from 'lucide-react';
import { type ReactNode, useId, useRef } from 'react';
import { cn } from '@/lib/utils';

export function GlassCard({ className, children, as: Tag = 'div', ...rest }: { className?: string; children: ReactNode; as?: 'div' | 'section' | 'article' } & React.HTMLAttributes<HTMLElement>) {
  return (
    <Tag className={cn('glass shadow-panel', className)} {...rest}>
      {children}
    </Tag>
  );
}

/** A card whose border glows where the cursor is. */
export function GlowCard({ className, children }: { className?: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <div
      ref={ref}
      onMouseMove={(e) => {
        const el = ref.current;
        if (!el) return;
        const r = el.getBoundingClientRect();
        el.style.setProperty('--mx', `${e.clientX - r.left}px`);
        el.style.setProperty('--my', `${e.clientY - r.top}px`);
      }}
      className={cn('glow-card glass shadow-panel', className)}
    >
      {children}
    </div>
  );
}

export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  size = 'md',
  label,
  className,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  size?: 'sm' | 'md';
  label: string;
  className?: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className={cn('inline-flex rounded-lg border border-ink/10 bg-canvas/70 p-0.5', className)}>
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={String(o.value)}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(o.value)}
            className={cn(
              'relative flex-1 whitespace-nowrap rounded-md font-medium transition-colors',
              size === 'sm' ? 'px-2.5 py-1 text-[11px]' : 'px-3 py-1.5 text-xs',
              active ? 'text-onaccent' : 'text-ink/60 hover:text-ink/90',
            )}
          >
            {active && <motion.span layoutId={`seg-${label}`} className="absolute inset-0 rounded-md bg-accent shadow-glow" transition={{ type: 'spring', stiffness: 500, damping: 38 }} />}
            <span className="relative">{o.label}</span>
          </button>
        );
      })}
    </div>
  );
}

export function RangeSlider({
  value,
  min,
  max,
  step,
  onChange,
  label,
  display,
  hint,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
  label: string;
  display: ReactNode;
  hint?: ReactNode;
}) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <label className="block">
      <span className="mb-2 flex items-baseline justify-between gap-3">
        <span className="text-xs font-medium text-ink/60">{label}</span>
        <span className="num text-sm font-semibold text-ink/90">{display}</span>
      </span>
      <input
        type="range"
        className="range w-full"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{ '--fill': `${fill}%` } as React.CSSProperties}
      />
      {hint && <span className="mt-1.5 flex justify-between text-[10px] text-ink/50 num">{hint}</span>}
    </label>
  );
}

export function StatusDot({ tone }: { tone: 'online' | 'offline' | 'checking' }) {
  return (
    <span className="relative flex h-2 w-2">
      {tone === 'online' && <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-profit opacity-60" />}
      <span className={cn('relative inline-flex h-2 w-2 rounded-full', tone === 'online' ? 'bg-profit' : tone === 'offline' ? 'bg-loss' : 'bg-slate-500 animate-pulse-dot')} />
    </span>
  );
}

export function Badge({ children, tone = 'neutral', className }: { children: ReactNode; tone?: 'neutral' | 'accent' | 'profit' | 'loss' | 'caution'; className?: string }) {
  const tones = {
    neutral: 'border-ink/10 bg-ink/[0.04] text-ink/75',
    accent: 'border-accent/30 bg-accent/10 text-accent',
    profit: 'border-profit/30 bg-profit/10 text-profit',
    loss: 'border-loss/30 bg-loss/10 text-loss',
    caution: 'border-caution/30 bg-caution/10 text-caution',
  } as const;
  return <span className={cn('inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase tracking-wider', tones[tone], className)}>{children}</span>;
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn('h-4 w-4 animate-spin', className)} aria-hidden />;
}

/** A number that cross-fades when it changes. */
export function AnimatedValue({ value, className }: { value: string; className?: string }) {
  return (
    <span className={cn('relative inline-block', className)}>
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.span key={value} initial={{ opacity: 0, y: 6, filter: 'blur(4px)' }} animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }} exit={{ opacity: 0, y: -6, filter: 'blur(4px)' }} transition={{ duration: 0.22 }} className="inline-block">
          {value}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

export function Logo({ className }: { className?: string }) {
  const gid = `sl-g-${useId().replace(/:/g, '')}`;
  return (
    <span className={cn('flex items-center gap-2.5', className)}>
      <svg viewBox="0 0 32 32" className="h-7 w-7" aria-hidden>
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#00F2FE" />
            <stop offset="1" stopColor="#4F8BFF" />
          </linearGradient>
        </defs>
        <rect x="1" y="1" width="30" height="30" rx="8" className="fill-panel" stroke={`url(#${gid})`} strokeOpacity="0.6" />
        <path d="M7 21 L12 14 L16 18 L21 9 L25 13" fill="none" stroke={`url(#${gid})`} strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" />
        <circle cx="21" cy="9" r="2" fill="#00F2FE" />
      </svg>
      <span className="whitespace-nowrap text-[15px] font-semibold tracking-tight text-ink">
        Spectra <span className="text-ink/60">Labs</span>
      </span>
    </span>
  );
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div role="alert" className="rounded-lg border border-loss/30 bg-loss/[0.08] px-3 py-2.5 text-xs leading-5 text-rose-700 dark:text-rose-200">
      {children}
    </div>
  );
}
