'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

export type Role = 'admin' | 'user';
export type AuthUser = { email: string; role: Role };
export type AuthMode = 'login' | 'signup';

export const ADMIN_EMAIL = 'spectralabs.team@gmail.com';
export const MIN_PASSWORD = 8;
export const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;

export type PasswordRules = {
  length: boolean;
  letter: boolean;
  number: boolean;
  special: boolean;
};

export function checkPasswordRules(password: string): PasswordRules {
  return {
    length: password.length >= MIN_PASSWORD,
    letter: /[a-zA-Z]/.test(password),
    number: /[0-9]/.test(password),
    special: /[^a-zA-Z0-9]/.test(password),
  };
}

export function isPasswordValid(password: string): boolean {
  const r = checkPasswordRules(password);
  return r.length && r.letter && r.number && r.special;
}

/** Role is derived from the email for the UI preview only; the backend must assign the real role. */
export function roleForEmail(email: string): Role {
  return email.trim().toLowerCase() === ADMIN_EMAIL ? 'admin' : 'user';
}

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
    try {
      const raw = sessionStorage.getItem(KEY);
      if (raw) {
        const u = JSON.parse(raw) as AuthUser;
        if (u && typeof u.email === 'string' && (u.role === 'admin' || u.role === 'user')) setUser(u);
      }
    } catch {}
    setIsLoading(false);
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
