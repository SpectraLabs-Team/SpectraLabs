'use client';

import { useSyncExternalStore } from 'react';
import { getHealth } from './api';
import type { HealthResponse } from './types';

export type EngineState = { status: 'checking' } | { status: 'online'; health: HealthResponse; latencyMs: number } | { status: 'offline' };

/**
 * One shared GET /health poller for the whole page. Every component that shows engine status
 * reads the same state, so the Studio header, sidebar and admin cards make one request per
 * interval instead of one each. Polling starts with the first subscriber and stops with the last.
 */
const INTERVAL_MS = 15_000;
let state: EngineState = { status: 'checking' };
const listeners = new Set<() => void>();
let timer: number | null = null;
let inFlight = false;

function emit(next: EngineState) {
  state = next;
  listeners.forEach((l) => l());
}

async function check() {
  if (inFlight) return;
  inFlight = true;
  const t0 = performance.now();
  try {
    const health = await getHealth();
    emit(health.status === 'healthy' ? { status: 'online', health, latencyMs: performance.now() - t0 } : { status: 'offline' });
  } catch {
    emit({ status: 'offline' });
  } finally {
    inFlight = false;
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    check();
    timer = window.setInterval(check, INTERVAL_MS);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  };
}

const SERVER_STATE: EngineState = { status: 'checking' };

/** Live engine status, shared across all callers. */
export function useEngineHealth(): EngineState {
  return useSyncExternalStore(subscribe, () => state, () => SERVER_STATE);
}
