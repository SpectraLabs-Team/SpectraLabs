'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { ArrowRight, Menu, X } from 'lucide-react';
import { Logo } from '@/components/ui/primitives';
import { ThemeToggle } from '@/components/ui/ThemeToggle';
import { NAV } from '@/lib/site';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';

function StatusPills({ className }: { className?: string }) {
  const s = useEngineHealth();
  const online = s.status === 'online';

  return (
    <div
      className={cn(
        'flex items-center gap-3.5 whitespace-nowrap rounded-full border border-cyan-500/20 bg-[#060b18]/80 px-4 py-1.5 text-[12px] shadow-[0_2px_12px_rgba(0,0,0,0.5)] backdrop-blur-md',
        className,
      )}
      aria-live="polite"
    >
      <span className="flex items-center gap-2 text-slate-200">
        <span
          className={cn(
            'h-2 w-2 rounded-full',
            online
              ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.9)]'
              : 'bg-rose-500 shadow-[0_0_8px_rgba(244,63,94,0.9)]',
          )}
        />
        {online ? 'Rust Engine Online' : 'Connecting Engine…'}
      </span>
      <span className="h-3 w-px bg-white/15" />
      <span className="flex items-center gap-2 text-slate-200">
        <span className="h-2 w-2 rounded-full bg-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.9)]" />
        Paper Trading Active
      </span>
    </div>
  );
}

export function Header() {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <header
      className={cn(
        'fixed inset-x-0 top-0 z-50 transition-all duration-300',
        scrolled || open
          ? 'border-b border-cyan-500/15 bg-[#040711]/85 backdrop-blur-xl shadow-[0_4px_20px_rgba(0,0,0,0.6)]'
          : 'border-b border-transparent bg-transparent',
      )}
    >
      <nav className="mx-auto flex h-16 max-w-[1360px] items-center gap-6 px-6 sm:px-8" aria-label="Main">
        <Link href="/" aria-label="Spectra Labs home" className="shrink-0">
          <Logo />
        </Link>

        {/* Center Nav Links */}
        <div className="mx-auto hidden items-center gap-1 lg:flex">
          {NAV.map((l) =>
            l.external ? (
              <a
                key={l.label}
                href={l.href}
                target="_blank"
                rel="noreferrer"
                className="rounded-lg px-3 py-1.5 text-[13.5px] font-medium text-slate-300 transition hover:text-white hover:bg-white/5"
              >
                {l.label}
              </a>
            ) : (
              <Link
                key={l.label}
                href={l.href}
                className="rounded-lg px-3 py-1.5 text-[13.5px] font-medium text-slate-300 transition hover:text-white hover:bg-white/5"
              >
                {l.label}
              </Link>
            ),
          )}
        </div>

        {/* Right CTA and status */}
        <div className="ml-auto flex items-center gap-3 lg:ml-0">
          <StatusPills className="hidden xl:flex" />
          <ThemeToggle />
          <Link
            href="/studio"
            className="inline-flex items-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 px-4 py-2 text-[13px] font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.4)] transition hover:shadow-[0_0_28px_rgba(6,182,212,0.6)] hover:brightness-110 active:scale-[0.98]"
          >
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </Link>

          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className="rounded-lg p-2 text-slate-300 hover:text-white lg:hidden"
            aria-label="Menu"
            aria-expanded={open}
          >
            {open ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
          </button>
        </div>
      </nav>

      {/* Mobile Drawer */}
      {open && (
        <div className="border-t border-cyan-500/15 bg-[#040711]/95 px-6 pb-6 pt-4 backdrop-blur-2xl lg:hidden">
          <div className="grid gap-2">
            {NAV.map((l) => (
              <a
                key={l.label}
                href={l.href}
                onClick={() => setOpen(false)}
                {...(l.external ? { target: '_blank', rel: 'noreferrer' } : {})}
                className="rounded-lg px-3 py-2 text-sm font-medium text-slate-200 hover:bg-white/5"
              >
                {l.label}
              </a>
            ))}
          </div>
          <StatusPills className="mt-4 flex w-fit" />
          <Link
            href="/studio"
            className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.4)]"
          >
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </Link>
        </div>
      )}
    </header>
  );
}
