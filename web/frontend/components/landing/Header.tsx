'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useState } from 'react';
import { ArrowRight, LogOut, Menu, X } from 'lucide-react';
import { Logo } from '@/components/ui/primitives';
import { ThemeToggle } from '@/components/ui/ThemeToggle';
import { useAuth } from '@/lib/auth';
import { NAV } from '@/lib/site';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';

const navLink =
  'rounded-lg px-3 py-1.5 text-[13.5px] font-medium transition text-slate-600 hover:text-slate-900 hover:bg-slate-100/80 dark:text-slate-300 dark:hover:text-white dark:hover:bg-white/5';

function StatusPills({ className }: { className?: string }) {
  const s = useEngineHealth();
  const online = s.status === 'online';

  return (
    <div
      className={cn(
        'flex items-center gap-3.5 whitespace-nowrap rounded-full border px-4 py-1.5 text-[12px] backdrop-blur-md',
        'border-slate-300/80 bg-slate-100/90 text-slate-700 shadow-sm',
        'dark:border-cyan-500/20 dark:bg-[#060b18]/80 dark:text-slate-200 dark:shadow-[0_2px_12px_rgba(0,0,0,0.5)]',
        className,
      )}
      aria-live="polite"
    >
      <span className="flex items-center gap-2">
        <span
          className={cn(
            'h-2 w-2 rounded-full',
            online ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.9)]' : 'bg-rose-500 shadow-[0_0_8px_rgba(244,63,94,0.9)]',
          )}
        />
        {online ? 'Rust Engine Online' : 'Connecting Engine…'}
      </span>
      <span className="h-3 w-px bg-slate-300 dark:bg-white/15" />
      <span className="flex items-center gap-2">
        <span className="h-2 w-2 rounded-full bg-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.9)]" />
        Paper Trading Active
      </span>
    </div>
  );
}

/** "Launch Quant Studio": straight to /studio when signed in, otherwise opens the 2FA modal. */
export function useLaunchStudio() {
  const router = useRouter();
  const { isAuthenticated, openAuthModal } = useAuth();
  return () => (isAuthenticated ? router.push('/studio') : openAuthModal());
}

function UserChip({ className }: { className?: string }) {
  const { user, logout } = useAuth();
  if (!user) return null;
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <span className="flex max-w-[220px] items-center gap-2 rounded-full border border-slate-300/80 bg-slate-100/90 py-1 pl-3 pr-1 text-[12px] text-slate-700 dark:border-white/10 dark:bg-white/5 dark:text-slate-200">
        <span className="truncate" title={user.email}>{user.email}</span>
        <span
          className={cn(
            'shrink-0 rounded-full px-2 py-0.5 text-[10.5px] font-semibold',
            user.role === 'admin' ? 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300' : 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
          )}
        >
          {user.role === 'admin' ? 'Admin' : 'User'}
        </span>
      </span>
      <button
        type="button"
        onClick={logout}
        className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[12.5px] font-medium text-slate-600 transition hover:bg-slate-100/80 hover:text-slate-900 dark:text-slate-300 dark:hover:bg-white/5 dark:hover:text-white"
      >
        <LogOut className="h-3.5 w-3.5" /> Sign Out
      </button>
    </div>
  );
}

export function Header({ overHero = false }: { overHero?: boolean }) {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);
  const { isAuthenticated, isLoading, user, openAuthModal, logout } = useAuth();
  const launch = useLaunchStudio();

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const solid = scrolled || open || !overHero;

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
          {NAV.map((l) =>
            l.external ? (
              <a key={l.label} href={l.href} target="_blank" rel="noreferrer" className={navLink}>
                {l.label}
              </a>
            ) : (
              <Link key={l.label} href={l.href} className={navLink}>
                {l.label}
              </Link>
            ),
          )}
        </div>

        <div className="ml-auto flex items-center gap-3 lg:ml-0">
          <StatusPills className={cn('hidden', isAuthenticated ? '2xl:flex' : 'xl:flex')} />
          <ThemeToggle />
          {!isLoading &&
            (isAuthenticated ? (
              <UserChip className="hidden lg:flex" />
            ) : (
              <button
                type="button"
                onClick={openAuthModal}
                className="hidden rounded-xl border border-slate-300 px-3.5 py-2 text-[13px] font-semibold text-slate-700 transition hover:border-slate-400 hover:text-slate-900 dark:border-white/15 dark:text-slate-200 dark:hover:border-white/30 dark:hover:text-white sm:inline-flex"
              >
                Sign In
              </button>
            ))}
          <button
            type="button"
            onClick={launch}
            className="hidden items-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 px-4 py-2 text-[13px] font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.4)] transition hover:shadow-[0_0_28px_rgba(6,182,212,0.6)] hover:brightness-110 active:scale-[0.98] sm:inline-flex"
          >
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </button>

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
          <div className="grid gap-2">
            {NAV.map((l) => (
              <Link
                key={l.label}
                href={l.href}
                onClick={() => setOpen(false)}
                {...(l.external ? { target: '_blank', rel: 'noreferrer' } : {})}
                className="rounded-lg px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-100/80 dark:text-slate-200 dark:hover:bg-white/5"
              >
                {l.label}
              </Link>
            ))}
          </div>
          <StatusPills className="mt-4 flex w-fit" />
          {isAuthenticated && user ? (
            <div className="mt-4 flex items-center justify-between gap-3 text-sm text-slate-700 dark:text-slate-200">
              <span className="truncate">
                {user.email} · <span className="font-semibold">{user.role === 'admin' ? 'Admin' : 'User'}</span>
              </span>
              <button type="button" onClick={logout} className="inline-flex shrink-0 items-center gap-1.5 font-medium text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-white">
                <LogOut className="h-4 w-4" /> Sign Out
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                openAuthModal();
              }}
              className="mt-4 w-full rounded-xl border border-slate-300 py-3 text-sm font-semibold text-slate-700 dark:border-white/15 dark:text-slate-200"
            >
              Sign In
            </button>
          )}
          <button
            type="button"
            onClick={() => {
              setOpen(false);
              launch();
            }}
            className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.4)]"
          >
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </button>
        </div>
      )}
    </header>
  );
}
