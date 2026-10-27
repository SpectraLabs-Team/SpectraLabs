'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { applyOnboarding, normalizeOnboarding, onboardingStorageKey, TOUR_VERSION, type OnboardingAction, type OnboardingState } from '@/lib/onboarding';

type Source = 'server' | 'local';

function readLocal(email: string): OnboardingState | null {
  try {
    const raw = localStorage.getItem(onboardingStorageKey(email));
    return raw ? normalizeOnboarding(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}

function writeLocal(email: string, s: OnboardingState) {
  try {
    localStorage.setItem(onboardingStorageKey(email), JSON.stringify(s));
  } catch {
    /* storage unavailable: the server copy is the record */
  }
}

/**
 * The signed-in user's tour state. The API (user_onboarding table) is authoritative; a per-user
 * localStorage copy is used only when the API cannot be reached, so an outage does not make the
 * tour reappear on every visit. `ready` is false until the state is known.
 */
export function useOnboardingState(email: string | null | undefined) {
  const [state, setState] = useState<OnboardingState | null>(null);
  const [source, setSource] = useState<Source | null>(null);
  const stateRef = useRef<OnboardingState | null>(null);
  stateRef.current = state;

  useEffect(() => {
    if (!email) return;
    let live = true;
    setState(null);
    setSource(null);
    (async () => {
      try {
        const res = await fetch('/api/auth/onboarding', { credentials: 'same-origin', cache: 'no-store' });
        if (!res.ok) throw new Error(String(res.status));
        const body = (await res.json()) as { onboarding?: unknown };
        if (!live) return;
        let s = normalizeOnboarding(body.onboarding);
        // The server has no record but this browser saved a finish/dismissal while the API was
        // unreachable: keep the user's choice and send it to the server now.
        const local = readLocal(email);
        const serverEmpty = !s.onboardingCompleted && !s.onboardingSkipped && s.onboardingVersion === 0 && !s.lastCompletedStep;
        if (serverEmpty && local && (local.onboardingCompleted || local.onboardingSkipped)) {
          s = local;
          fetch('/api/auth/onboarding', {
            method: 'PUT',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ action: local.onboardingCompleted ? 'complete' : 'skip', step: local.lastCompletedStep, version: Math.max(1, local.onboardingVersion) }),
          }).catch(() => {});
        }
        writeLocal(email, s);
        setState(s);
        setSource('server');
      } catch {
        if (!live) return;
        setState(readLocal(email) ?? normalizeOnboarding(null));
        setSource('local');
      }
    })();
    return () => {
      live = false;
    };
  }, [email]);

  const record = useCallback(
    (action: OnboardingAction, step: string | null = null) => {
      if (!email) return;
      const next = applyOnboarding(stateRef.current ?? normalizeOnboarding(null), action, step, TOUR_VERSION);
      stateRef.current = next;
      setState(next);
      writeLocal(email, next);
      // Fire and forget; keepalive lets the last update survive navigating away.
      fetch('/api/auth/onboarding', {
        method: 'PUT',
        credentials: 'same-origin',
        keepalive: true,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action, step, version: TOUR_VERSION }),
      }).catch(() => {
        /* kept locally; the next successful load replaces it */
      });
    },
    [email],
  );

  return { state, source, ready: state !== null, record };
}
