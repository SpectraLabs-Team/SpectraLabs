'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

export * from './auth-shared';
import { type AuthMode, type AuthUser, type Role } from './auth-shared';

// Frontend-stage session persistence so a refresh doesn't sign you out.
// Replace with the server-issued session (HttpOnly cookie) once the SMTP/OTP endpoints land.
const KEY = 'spectra-auth';

type AuthCtx = {
  user: AuthUser | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (email: string, role: Role) => void;
  logout: () => void;
  isAuthModalOpen: boolean;
  authMode: AuthMode;
  setAuthMode: (mode: AuthMode) => void;
  openAuthModal: (mode?: AuthMode) => void;
  closeAuthModal: () => void;
};

const Ctx = createContext<AuthCtx | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isAuthModalOpen, setModalOpen] = useState(false);
  const [authMode, setAuthMode] = useState<AuthMode>('login');

  useEffect(() => {
    let active = true;
    // Check server-side HttpOnly session first, fallback to sessionStorage
    fetch('/api/auth/session')
      .then((res) => res.json())
      .then((data) => {
        if (!active) return;
        if (data.ok && data.user) {
          setUser(data.user);
          try {
            sessionStorage.setItem(KEY, JSON.stringify(data.user));
          } catch {}
        } else {
          // Check sessionStorage fallback
          try {
            const raw = sessionStorage.getItem(KEY);
            if (raw) {
              const u = JSON.parse(raw) as AuthUser;
              if (u && typeof u.email === 'string' && (u.role === 'admin' || u.role === 'user')) setUser(u);
            }
          } catch {}
        }
      })
      .catch(() => {
        if (!active) return;
        try {
          const raw = sessionStorage.getItem(KEY);
          if (raw) {
            const u = JSON.parse(raw) as AuthUser;
            if (u && typeof u.email === 'string') setUser(u);
          }
        } catch {}
      })
      .finally(() => {
        if (active) setIsLoading(false);
      });

    return () => {
      active = false;
    };
  }, []);

  const login = useCallback((email: string, role: Role) => {
    const u = { email: email.trim().toLowerCase(), role };
    setUser(u);
    try {
      sessionStorage.setItem(KEY, JSON.stringify(u));
    } catch {}
  }, []);

  const logout = useCallback(() => {
    setUser(null);
    try {
      sessionStorage.removeItem(KEY);
    } catch {}
    fetch('/api/auth/logout', { method: 'POST' }).catch(() => {});
  }, []);

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
