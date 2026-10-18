'use client';

import { useEffect, useState } from 'react';
import { getHealth } from './api';
import type { HealthResponse } from './types';

export type EngineState = { status: 'checking' } | { status: 'online'; health: HealthResponse; latencyMs: number } | { status: 'offline' };

/** Polls GET /health every `intervalMs` and reports round-trip latency. */
export function useEngineHealth(intervalMs = 15_000): EngineState {
  const [state, setState] = useState<EngineState>({ status: 'checking' });
  useEffect(() => {
    let active = true;
    const check = async () => {
      const t0 = performance.now();
      try {
        const health = await getHealth();
        if (active) setState(health.status === 'healthy' ? { status: 'online', health, latencyMs: performance.now() - t0 } : { status: 'offline' });
      } catch {
        if (active) setState({ status: 'offline' });
      }
    };
    check();
    const id = window.setInterval(check, intervalMs);
    return () => {
      active = false;
      window.clearInterval(id);
    };
  }, [intervalMs]);
  return state;
}
