'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

export * from './auth-shared';
import { type AuthMode, type AuthUser } from './auth-shared';

/**
 * The HttpOnly `spectra_session` cookie is the source of truth. sessionStorage only caches the
 * user so a page shows the right header instantly; it is never trusted over the server.
 */
const CACHE_KEY = 'spectra-auth';
/** localStorage key whose `storage` events tell other tabs to re-check the session. */
const SYNC_KEY = 'spectra-auth-sync';

type AuthCtx = {
  user: AuthUser | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (user: AuthUser) => void;
  /** Ends the session. With `redirectTo`, does a full page load there once the cookie is cleared. */
  logout: (redirectTo?: string) => Promise<void>;
  isAuthModalOpen: boolean;
  authMode: AuthMode;
  setAuthMode: (mode: AuthMode) => void;
  openAuthModal: (mode?: AuthMode) => void;
  closeAuthModal: () => void;
};

const Ctx = createContext<AuthCtx | null>(null);

function isAuthUser(u: unknown): u is AuthUser {
  const x = u as AuthUser;
  return !!x && typeof x.email === 'string' && (x.role === 'admin' || x.role === 'user');
}

function normalize(u: AuthUser): AuthUser {
  return { email: u.email, role: u.role, name: typeof u.name === 'string' ? u.name : '' };
}

function writeCache(u: AuthUser | null) {
  try {
    if (u) sessionStorage.setItem(CACHE_KEY, JSON.stringify(u));
    else sessionStorage.removeItem(CACHE_KEY);
  } catch {}
}

function readCache(): AuthUser | null {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY);
    const u = raw ? JSON.parse(raw) : null;
    return isAuthUser(u) ? normalize(u) : null;
  } catch {
    return null;
  }
}

function broadcast(type: 'login' | 'logout') {
  try {
    // A unique value guarantees a `storage` event even for repeated identical actions.
    localStorage.setItem(SYNC_KEY, JSON.stringify({ type, at: Date.now(), nonce: Math.random() }));
  } catch {}
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isAuthModalOpen, setModalOpen] = useState(false);
  const [authMode, setAuthMode] = useState<AuthMode>('login');
  const reqId = useRef(0);

  const apply = useCallback((u: AuthUser | null) => {
    setUser(u);
    writeCache(u);
  }, []);

  /** Ask the server who we are. Only a network failure falls back to the cached user. */
  const refresh = useCallback(async () => {
    const id = ++reqId.current;
    try {
      const res = await fetch('/api/auth/session', { cache: 'no-store', credentials: 'same-origin' });
      const data = await res.json();
      if (id !== reqId.current) return;
      apply(data?.ok && isAuthUser(data.user) ? normalize(data.user) : null);
    } catch {
      if (id !== reqId.current) return;
      setUser(readCache());
    } finally {
      if (id === reqId.current) setIsLoading(false);
    }
  }, [apply]);

  useEffect(() => {
    const cached = readCache();
    if (cached) setUser(cached); // instant header; confirmed or cleared by refresh()
    refresh();

    // Multi-tab sync: another tab logged in or out → re-check the shared cookie.
    const onStorage = (e: StorageEvent) => {
      if (e.key !== SYNC_KEY || !e.newValue) return;
      try {
        const msg = JSON.parse(e.newValue) as { type: 'login' | 'logout' };
        if (msg.type === 'logout') {
          reqId.current++; // drop any in-flight refresh
          apply(null);
          setIsLoading(false);
        } else {
          refresh();
        }
      } catch {}
    };
    // Returning to a tab is another good moment to catch an expired cookie.
    const onVisible = () => document.visibilityState === 'visible' && refresh();

    window.addEventListener('storage', onStorage);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.removeEventListener('storage', onStorage);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [refresh, apply]);

  /** Call with the user the server returned after it set the session cookie. */
  const login = useCallback(
    (u: AuthUser) => {
      reqId.current++;
      apply(normalize({ ...u, email: u.email.trim().toLowerCase() }));
      setIsLoading(false);
      broadcast('login');
    },
    [apply],
  );

  const logout = useCallback(
    async (redirectTo?: string) => {
      reqId.current++;
      // When leaving a protected page, keep it rendered until the browser navigates away,
      // so the sign-in gate never flashes; the full reload also drops all in-memory state.
      if (!redirectTo) apply(null);
      else writeCache(null);
      try {
        await fetch('/api/auth/logout', { method: 'POST', credentials: 'same-origin' });
      } catch {}
      broadcast('logout');
      if (redirectTo) window.location.replace(redirectTo);
    },
    [apply],
  );

  const openAuthModal = useCallback((mode: AuthMode = 'login') => {
    setAuthMode(mode);
    setModalOpen(true);
  }, []);
  const closeAuthModal = useCallback(() => setModalOpen(false), []);

  const value = useMemo(
    () => ({ user, isAuthenticated: !!user, isLoading, login, logout, isAuthModalOpen, authMode, setAuthMode, openAuthModal, closeAuthModal }),
    [user, isLoading, login, logout, isAuthModalOpen, authMode, openAuthModal, closeAuthModal],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useAuth() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('useAuth must be used inside <AuthProvider>');
  return ctx;
}
