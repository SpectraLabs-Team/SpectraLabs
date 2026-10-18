'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { ArrowRight, Menu, X } from 'lucide-react';
import { Logo, StatusDot } from '@/components/ui/primitives';
import { ThemeToggle } from '@/components/ui/ThemeToggle';
import { NAV } from '@/lib/site';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn } from '@/lib/utils';


function StatusPills({ className }: { className?: string }) {
  const s = useEngineHealth();
  return (
    <div className={cn('items-center gap-3 whitespace-nowrap rounded-full border border-ink/10 bg-ink/[0.03] px-3 py-1.5 text-[11.5px]', className)} aria-live="polite">
      <span className="flex items-center gap-1.5 text-ink/80" title={s.status === 'offline' ? 'Start the API: cargo run --release -p spectra-api' : undefined}>
        <StatusDot tone={s.status} />
        {s.status === 'online' ? 'Rust Engine Online' : s.status === 'offline' ? 'Engine Offline' : 'Connecting…'}
      </span>
      <span className="h-3 w-px bg-ink/15" />
      <span className="flex items-center gap-1.5 text-ink/80" title="Paper trading runs on a simulated ledger in the browser">
        <span className="h-2 w-2 rounded-full bg-profit shadow-[0_0_8px_rgb(var(--profit))]" />
        Paper Trading (sim)
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
    <header className={cn('fixed inset-x-0 top-0 z-50 transition-all duration-300', scrolled || open ? 'border-b border-ink/[0.07] bg-canvas/80 backdrop-blur-xl' : 'dark border-b border-transparent')}>
      <nav className="mx-auto flex h-16 max-w-[1320px] items-center gap-4 px-5 sm:px-8" aria-label="Main">
        <Link href="/" aria-label="Spectra Labs home" className="shrink-0">
          <Logo />
        </Link>
        <div className="mx-auto hidden items-center lg:flex">
          {NAV.map((l) =>
            l.external ? (
              <a key={l.label} href={l.href} target="_blank" rel="noreferrer" className="rounded-md px-2.5 py-2 text-[13.5px] text-ink/65 transition hover:text-ink">
                {l.label}
              </a>
            ) : (
              <Link key={l.label} href={l.href} className="rounded-md px-2.5 py-2 text-[13.5px] text-ink/65 transition hover:text-ink">
                {l.label}
              </Link>
            ),
          )}
        </div>
        <div className="ml-auto flex items-center gap-2.5 lg:ml-0">
          <StatusPills className="hidden xl:flex" />
          <ThemeToggle />
          <Link href="/studio" className="btn-primary hidden whitespace-nowrap py-2 sm:inline-flex">
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </Link>
          <button type="button" onClick={() => setOpen((o) => !o)} className="rounded-lg p-2 text-ink/70 lg:hidden" aria-label="Menu" aria-expanded={open}>
            {open ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
          </button>
        </div>
      </nav>
      {open && (
        <div className="border-t border-ink/[0.06] px-5 pb-5 pt-3 lg:hidden">
          <div className="grid gap-1">
            {NAV.map((l) => (
              <a key={l.label} href={l.href} onClick={() => setOpen(false)} {...(l.external ? { target: '_blank', rel: 'noreferrer' } : {})} className="rounded-md px-2 py-2 text-sm text-ink/75">
                {l.label}
              </a>
            ))}
          </div>
          <StatusPills className="mt-3 inline-flex" />
          <Link href="/studio" className="btn-primary mt-4 w-full">
            Launch Quant Studio <ArrowRight className="h-4 w-4" />
          </Link>
        </div>
      )}
    </header>
  );
}
