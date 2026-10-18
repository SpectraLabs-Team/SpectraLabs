'use client';

import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';

/** Operational status ("Rust Engine Online" / "Paper Trading Active"). For /studio and /admin, not the public header. */
export function StatusPills({ className }: { className?: string }) {
  const s = useEngineHealth();
  const online = s.status === 'online';

  return (
    <div
      className={cn(
        'flex w-fit items-center gap-3.5 whitespace-nowrap rounded-full border px-4 py-1.5 text-[12px] backdrop-blur-md',
        'border-slate-300/80 bg-slate-100/90 text-slate-700 shadow-sm',
        'dark:border-cyan-500/20 dark:bg-[#060b18]/80 dark:text-slate-200 dark:shadow-[0_2px_12px_rgba(0,0,0,0.5)]',
        className,
      )}
      title={s.status === 'offline' ? 'Start the API: cargo run --release -p spectra-api' : undefined}
      aria-live="polite"
    >
      <span className="flex items-center gap-2">
        <span
          className={cn(
            'h-2 w-2 rounded-full',
            online
              ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.9)]'
              : s.status === 'checking'
                ? 'animate-pulse-dot bg-amber-400'
                : 'bg-rose-500 shadow-[0_0_8px_rgba(244,63,94,0.9)]',
          )}
        />
        {online ? 'Rust Engine Online' : s.status === 'checking' ? 'Connecting Engine…' : 'Engine Offline'}
        {online && <span className="font-mono text-[11px] text-slate-500 dark:text-slate-400">{s.latencyMs.toFixed(0)}ms</span>}
      </span>
      <span className="h-3 w-px bg-slate-300 dark:bg-white/15" />
      <span className="flex items-center gap-2">
        <span className="h-2 w-2 rounded-full bg-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.9)]" />
        Paper Trading Active
      </span>
    </div>
  );
}
