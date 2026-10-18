'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { ArrowRight, Github } from 'lucide-react';
import { HealthPill } from '@/components/ui/HealthPill';
import { Logo } from '@/components/ui/primitives';
import { GITHUB_URL } from '@/lib/site';
import { cn } from '@/lib/utils';

const links = [
  { href: '#features', label: 'Features' },
  { href: '#benchmarks', label: 'Benchmarks' },
  { href: '#architecture', label: 'Architecture' },
];

export function Navbar() {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <header className={cn('fixed inset-x-0 top-0 z-50 transition-all duration-300', scrolled ? 'border-b border-white/[0.06] bg-obsidian-950/75 backdrop-blur-xl' : 'border-b border-transparent')}>
      <nav className="mx-auto flex h-16 max-w-7xl items-center gap-6 px-5 sm:px-8" aria-label="Main">
        <Link href="/" aria-label="Spectra Labs home">
          <Logo />
        </Link>
        <HealthPill className="hidden md:inline-flex" />
        <div className="ml-auto hidden items-center gap-1 md:flex">
          {links.map((l) => (
            <a key={l.href} href={l.href} className="rounded-md px-3 py-2 text-sm text-slate-400 transition hover:text-white">
              {l.label}
            </a>
          ))}
          <a href={GITHUB_URL} target="_blank" rel="noreferrer" className="rounded-md p-2 text-slate-400 transition hover:text-white" aria-label="GitHub repository">
            <Github className="h-[18px] w-[18px]" />
          </a>
        </div>
        <Link href="/studio" className="btn-primary ml-auto py-2 md:ml-2">
          Launch Studio <ArrowRight className="h-4 w-4" />
        </Link>
      </nav>
    </header>
  );
}
