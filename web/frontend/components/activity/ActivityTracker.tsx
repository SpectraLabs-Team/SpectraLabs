'use client';

import { useEffect, useRef } from 'react';
import { useAuth } from '@/lib/auth';
import { HEARTBEAT_MS, isActive } from '@/lib/activityClient';

/**
 * Sends an activity heartbeat about once a minute while the page is visible and the person has used
 * it in the last few minutes. Counts time only; it records no page names, clicks or content.
 * Renders nothing. Signed-out visitors send nothing.
 */
export function ActivityTracker() {
  const { user } = useAuth();
  const lastInput = useRef(0);

  useEffect(() => {
    if (!user) return;
    lastInput.current = Date.now();
    const onInput = () => {
      lastInput.current = Date.now();
    };
    const events = ['pointerdown', 'keydown', 'scroll', 'touchstart'] as const;
    events.forEach((e) => window.addEventListener(e, onInput, { passive: true }));
    const beat = () => {
      const active = isActive({ visible: document.visibilityState === 'visible', lastInputAt: lastInput.current, now: Date.now() });
      if (!active) return;
      fetch('/api/activity/heartbeat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ active: true }),
        credentials: 'same-origin',
        keepalive: true,
      }).catch(() => undefined);
    };
    const timer = window.setInterval(beat, HEARTBEAT_MS);
    return () => {
      window.clearInterval(timer);
      events.forEach((e) => window.removeEventListener(e, onInput));
    };
  }, [user]);

  return null;
}
