'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useState } from 'react';
import { ArrowRight, LayoutDashboard, LogOut, Menu, X } from 'lucide-react';
import { Logo } from '@/components/ui/primitives';
import { ThemeToggle } from '@/components/ui/ThemeToggle';
import { displayName, useAuth, type AuthUser } from '@/lib/auth';
import { NAV } from '@/lib/site';
import { cn } from '@/lib/utils';
import { NavLink } from './NavLink';

const navLink =
  'rounded-lg px-3 py-1.5 text-[13.5px] font-medium transition text-slate-600 hover:text-slate-900 hover:bg-slate-100/80 dark:text-slate-300 dark:hover:text-white dark:hover:bg-white/5';
const ghostBtn =
  'rounded-xl px-3 py-2 text-sm font-semibold transition text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-white';
const primaryBtn =
  'inline-flex items-center gap-1.5 rounded-xl bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#0284C7] px-4 py-2 text-sm font-bold text-[#03131A] shadow-glow transition hover:brightness-110 active:scale-[0.98]';

/** "Launch Quant Studio": straight to /studio when signed in, otherwise opens Sign Up. */
export function useLaunchStudio() {
  const router = useRouter();
  const { isAuthenticated, openAuthModal } = useAuth();
  return () => (isAuthenticated ? router.push('/studio') : openAuthModal('signup'));
}

/** Signed-in identity: display name + role badge (email in the tooltip). */
export function RolePill({ user, className }: { user: AuthUser; className?: string }) {
  const name = displayName(user);
  return (
    <span
      title={`${name} · ${user.email}`}
      className={cn(
        'flex max-w-[240px] items-center gap-2 rounded-full border border-slate-300/80 bg-slate-100/90 py-1 pl-3 pr-1 text-[12px] font-medium text-slate-700 dark:border-white/10 dark:bg-white/5 dark:text-slate-200',
        className,
      )}
    >
      <span className="truncate">{name}</span>
      <span
        className={cn(
          'shrink-0 rounded-full px-2 py-0.5 text-[10.5px] font-semibold',
          user.role === 'admin' ? 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300' : 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
        )}
      >
        {user.role === 'admin' ? 'Admin' : 'User'}
      </span>
    </span>
  );
}

export function Header({ overHero = false }: { overHero?: boolean }) {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);
  const { user, isLoading, openAuthModal, logout } = useAuth();

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const solid = scrolled || open || !overHero;
  const close = () => setOpen(false);

  return (
    <header
      className={cn(
        'fixed inset-x-0 top-0 z-50 transition-all duration-300',
        // While transparent over the always-dark hero, force dark tokens so text stays legible in light mode.
        !solid && 'dark',
        solid
          ? 'border-b border-slate-200/80 bg-white/85 shadow-sm backdrop-blur-xl dark:border-cyan-500/15 dark:bg-[#040711]/85 dark:shadow-[0_4px_20px_rgba(0,0,0,0.6)]'
          : 'border-b border-transparent bg-transparent',
      )}
    >
      <nav className="mx-auto flex h-16 max-w-[1360px] items-center gap-6 px-6 sm:px-8" aria-label="Main">
        <Link href="/" aria-label="Spectra Labs home" className="shrink-0">
          <Logo />
        </Link>

        <div className="mx-auto hidden items-center gap-1 lg:flex">
          {NAV.map((l) => (
            <NavLink key={l.label} href={l.href} external={l.external} className={navLink}>
              {l.label}
            </NavLink>
          ))}
        </div>

        <div className="ml-auto flex items-center gap-2 lg:ml-0">
          <ThemeToggle />
          {/* Reserve space while the session restores so buttons don't flash. */}
          {isLoading ? (
            <span className="hidden h-9 w-[180px] sm:block" aria-hidden />
          ) : user ? (
            <div className="hidden items-center gap-1 lg:flex">
              <RolePill user={user} className="mr-1" />
              {user.role === 'admin' && (
                <Link href="/admin" className={cn(navLink, 'inline-flex items-center gap-1.5')}>
                  <LayoutDashboard className="h-3.5 w-3.5" /> Admin Dashboard
                </Link>
              )}
              <Link href="/studio" className={primaryBtn}>
                Quant Studio <ArrowRight className="h-4 w-4" />
              </Link>
              <button type="button" onClick={logout} className={cn(ghostBtn, 'inline-flex items-center gap-1.5')}>
                <LogOut className="h-3.5 w-3.5" /> Sign Out
              </button>
            </div>
          ) : (
            <div className="hidden items-center gap-1 sm:flex">
              <button type="button" onClick={() => openAuthModal('login')} className={ghostBtn}>
                Log In
              </button>
              <button type="button" onClick={() => openAuthModal('signup')} className={primaryBtn}>
                Get Started
              </button>
            </div>
          )}

          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className="rounded-lg p-2 text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-white lg:hidden"
            aria-label="Menu"
            aria-expanded={open}
          >
            {open ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
          </button>
        </div>
      </nav>

      {open && (
        <div className="border-t border-slate-200/80 bg-white/95 px-6 pb-6 pt-4 backdrop-blur-2xl dark:border-cyan-500/15 dark:bg-[#040711]/95 lg:hidden">
          <div className="grid gap-1">
            {NAV.map((l) => (
              <NavLink
                key={l.label}
                href={l.href}
                external={l.external}
                onClick={close}
                className="rounded-lg px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-100/80 dark:text-slate-200 dark:hover:bg-white/5"
              >
                {l.label}
              </NavLink>
            ))}
          </div>

          {user ? (
            <div className="mt-4 grid gap-2">
              <RolePill user={user} className="w-fit max-w-full" />
              {user.role === 'admin' && (
                <Link href="/admin" onClick={close} className="rounded-xl border border-slate-300 py-2.5 text-center text-sm font-semibold text-slate-700 dark:border-white/15 dark:text-slate-200">
                  Admin Dashboard
                </Link>
              )}
              <Link href="/studio" onClick={close} className={cn(primaryBtn, 'justify-center py-3')}>
                Quant Studio <ArrowRight className="h-4 w-4" />
              </Link>
              <button type="button" onClick={() => { close(); logout(); }} className={cn(ghostBtn, 'inline-flex items-center justify-center gap-1.5')}>
                <LogOut className="h-4 w-4" /> Sign Out
              </button>
            </div>
          ) : (
            <div className="mt-4 grid gap-2">
              <button type="button" onClick={() => { close(); openAuthModal('signup'); }} className={cn(primaryBtn, 'justify-center py-3')}>
                Get Started
              </button>
              <button type="button" onClick={() => { close(); openAuthModal('login'); }} className="rounded-xl border border-slate-300 py-2.5 text-sm font-semibold text-slate-700 dark:border-white/15 dark:text-slate-200">
                Log In
              </button>
            </div>
          )}
        </div>
      )}
    </header>
  );
}
