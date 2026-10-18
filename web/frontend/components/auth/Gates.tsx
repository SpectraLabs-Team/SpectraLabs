'use client';

import Link from 'next/link';
import type { ReactNode } from 'react';
import { motion } from 'framer-motion';
import { ArrowLeft, Lock, Mail, ShieldAlert } from 'lucide-react';
import { displayName, useAuth } from '@/lib/auth';

function Screen({ children }: { children: ReactNode }) {
  return (
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden bg-canvas px-5">
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_50%_35%,rgba(34,211,238,0.12),transparent_60%)]" />
      <motion.div
        initial={{ opacity: 0, y: 14 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
        className="relative w-full max-w-md rounded-2xl border border-slate-200 bg-white/90 p-8 text-center shadow-2xl backdrop-blur-xl dark:border-white/10 dark:bg-panel/80"
      >
        {children}
        <Link href="/" className="mt-6 inline-flex items-center gap-1.5 text-[13px] text-ink/55 transition hover:text-ink">
          <ArrowLeft className="h-3.5 w-3.5" /> Back to home
        </Link>
      </motion.div>
    </div>
  );
}

function Loading() {
  return <div className="min-h-screen bg-canvas" aria-busy="true" />;
}

function SignInScreen({ title }: { title: string }) {
  const { openAuthModal } = useAuth();
  return (
    <Screen>
      <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl border border-cyan-500/30 bg-cyan-500/10 text-cyan-600 shadow-[0_0_30px_rgba(34,211,238,0.25)] dark:text-cyan-300">
        <Lock className="h-6 w-6" />
      </div>
      <h1 className="mt-5 text-xl font-bold tracking-tight text-ink">{title}</h1>
      <p className="mt-2 text-sm leading-6 text-ink/60">Access is protected by a one-time code sent to your email.</p>
      <button
        type="button"
        onClick={() => openAuthModal('login')}
        className="mt-6 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white shadow-[0_0_24px_rgba(6,182,212,0.4)] transition hover:brightness-110 active:scale-[0.99]"
      >
        <Mail className="h-4 w-4" /> Log In with Email
      </button>
    </Screen>
  );
}

/** Client-side gate for the frontend stage. The API must still enforce auth on every request. */
export function RequireAuth({ children }: { children: ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth();
  if (isLoading) return <Loading />;
  if (!isAuthenticated) return <SignInScreen title="Sign in with 2FA to access Quant Studio" />;
  return <>{children}</>;
}

export function RequireAdmin({ children }: { children: ReactNode }) {
  const { user, isLoading } = useAuth();
  if (isLoading) return <Loading />;
  if (!user || user.role !== 'admin') {
    return (
      <Screen>
        <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl border border-rose-500/30 bg-rose-500/10 text-rose-600 dark:text-rose-300">
          <ShieldAlert className="h-6 w-6" />
        </div>
        <h1 className="mt-5 text-xl font-bold tracking-tight text-ink">Unauthorized - Admin Privileges Required</h1>
        <p className="mt-2 text-sm leading-6 text-ink/60">
          {user ? (
            <>
              You&apos;re signed in as <span className="font-medium text-ink/85">{displayName(user)}</span> ({user.email}) with User access.
            </>
          ) : (
            'Sign in with an administrator account to continue.'
          )}
        </p>
        {user ? <SwitchToAdminButton /> : <SignInButton />}
      </Screen>
    );
  }
  return <>{children}</>;
}

function SignInButton() {
  const { openAuthModal } = useAuth();
  return (
    <button
      type="button"
      onClick={() => openAuthModal('login')}
      className="mt-6 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white transition hover:brightness-110"
    >
      <Mail className="h-4 w-4" /> Log In with Email
    </button>
  );
}

/** A signed-in regular user: end their session, then open Log In for the admin account. */
function SwitchToAdminButton() {
  const { logout, openAuthModal } = useAuth();
  return (
    <button
      type="button"
      onClick={async () => {
        await logout();
        openAuthModal('login');
      }}
      className="mt-6 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white transition hover:brightness-110"
    >
      <Mail className="h-4 w-4" /> Sign In with Admin Account
    </button>
  );
}
