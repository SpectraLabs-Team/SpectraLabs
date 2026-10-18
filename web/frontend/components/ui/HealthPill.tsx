'use client';

import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';
import { StatusDot } from './primitives';

/** Live engine status from GET /health. */
export function HealthPill({ className }: { className?: string }) {
  const s = useEngineHealth();
  const label =
    s.status === 'online' ? (
      <>
        <span className="text-ink/90">{s.health.engine}</span>
        <span className="num text-ink/50">v{s.health.engine_version}</span>
        <span className="num hidden text-ink/50 sm:inline">· {s.latencyMs.toFixed(0)}ms</span>
      </>
    ) : s.status === 'offline' ? (
      <span className="text-ink/60">Engine offline</span>
    ) : (
      <span className="text-ink/50">Connecting…</span>
    );
  return (
    <span
      className={cn('inline-flex items-center gap-2 rounded-full border border-ink/10 bg-ink/[0.03] px-3 py-1 text-xs', className)}
      title={s.status === 'offline' ? 'Start the API: cargo run --release -p spectra-api' : undefined}
      aria-live="polite"
    >
      <StatusDot tone={s.status} />
      {label}
    </span>
  );
}
