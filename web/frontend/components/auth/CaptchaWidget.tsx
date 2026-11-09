'use client';

import React, { useEffect, useRef, useState } from 'react';

/**
 * Reusable Cloudflare Turnstile CAPTCHA Widget (M30 Tasks 3 & 4).
 *
 * - Responsive at phone width (max-width: 300px, fluid width).
 * - Loaded only on authentication surfaces (sign-up, sign-in, reset).
 * - Accessible: includes screen-reader announcements and keyboard focusability.
 * - Bound to action ('signup' | 'login' | 'reset').
 */

export interface CaptchaWidgetProps {
  action: 'signup' | 'login' | 'reset';
  onVerify: (token: string) => void;
  onError?: (error?: string) => void;
  onExpire?: () => void;
  className?: string;
}

declare global {
  interface Window {
    turnstile?: {
      render: (
        container: HTMLElement | string,
        params: {
          sitekey: string;
          action?: string;
          callback?: (token: string) => void;
          'error-callback'?: (err?: unknown) => void;
          'expired-callback'?: () => void;
          theme?: 'light' | 'dark' | 'auto';
          size?: 'normal' | 'compact' | 'flexible';
        },
      ) => string;
      reset: (widgetId: string) => void;
      remove: (widgetId: string) => void;
    };
  }
}

export function CaptchaWidget({ action, onVerify, onError, onExpire, className = '' }: CaptchaWidgetProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const widgetIdRef = useRef<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [fallbackInteractive, setFallbackInteractive] = useState(false);

  const siteKey = process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY || '1x00000000000000000000AA'; // Cloudflare test always-pass sitekey

  useEffect(() => {
    // Dynamically inject Turnstile script only when widget is mounted
    const scriptId = 'cloudflare-turnstile-api';
    let script = document.getElementById(scriptId) as HTMLScriptElement | null;

    if (!script) {
      script = document.createElement('script');
      script.id = scriptId;
      script.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
      script.async = true;
      script.defer = true;
      script.onload = () => setLoaded(true);
      script.onerror = () => {
        setFallbackInteractive(true);
        if (onError) onError('Could not load security check.');
      };
      document.head.appendChild(script);
    } else if (window.turnstile) {
      setLoaded(true);
    } else {
      script.addEventListener('load', () => setLoaded(true));
    }
  }, [onError]);

  useEffect(() => {
    if (!loaded || !containerRef.current || !window.turnstile) return;

    try {
      if (widgetIdRef.current) {
        window.turnstile.remove(widgetIdRef.current);
        widgetIdRef.current = null;
      }

      widgetIdRef.current = window.turnstile.render(containerRef.current, {
        sitekey: siteKey,
        action,
        theme: 'dark',
        size: 'flexible',
        callback: (token: string) => {
          onVerify(token);
        },
        'error-callback': (err) => {
          if (onError) onError(typeof err === 'string' ? err : 'Security check challenge failed.');
        },
        'expired-callback': () => {
          if (onExpire) onExpire();
        },
      });
    } catch {
      setFallbackInteractive(true);
    }

    return () => {
      if (widgetIdRef.current && window.turnstile) {
        try {
          window.turnstile.remove(widgetIdRef.current);
        } catch {}
        widgetIdRef.current = null;
      }
    };
  }, [loaded, action, siteKey, onVerify, onError, onExpire]);

  return (
    <div
      role="region"
      aria-label="Security verification challenge"
      className={`relative my-2 w-full max-w-[300px] mx-auto min-h-[65px] flex flex-col items-center justify-center rounded-lg border border-white/10 bg-black/40 p-1 ${className}`}
    >
      <div ref={containerRef} className="w-full flex justify-center" />

      {/* Screen reader non-visual alternative information */}
      <span className="sr-only">
        Security verification challenge required to protect against automated bots. Press Tab to focus the interactive challenge or audio alternative.
      </span>

      {fallbackInteractive && (
        <div className="text-center p-2 text-xs text-amber-300">
          Security challenge offline. Proceed with rate-limited submission.
        </div>
      )}
    </div>
  );
}
