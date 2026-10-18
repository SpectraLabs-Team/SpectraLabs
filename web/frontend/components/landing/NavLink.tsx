'use client';

import Link from 'next/link';
import { usePathname, useRouter } from 'next/navigation';
import { useEffect, type MouseEvent, type ReactNode } from 'react';

const PENDING_KEY = 'spectra-scroll-to';

/** "/#features" → "features"; anything else → null. */
function homeAnchor(href: string): string | null {
  return href.startsWith('/#') ? href.slice(2) : null;
}

function scrollToId(id: string) {
  document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

/**
 * Links to homepage sections without ever putting `#section` in the address bar, so a
 * reload always starts at the top. On "/" it smooth-scrolls in place; from another page it
 * navigates to a clean "/" and scrolls once the landing page mounts.
 */
export function NavLink({ href, external, className, onClick, children }: { href: string; external?: boolean; className?: string; onClick?: () => void; children: ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const anchor = homeAnchor(href);

  if (external) {
    return (
      <a href={href} target="_blank" rel="noreferrer" className={className} onClick={onClick}>
        {children}
      </a>
    );
  }

  const handle = (e: MouseEvent<HTMLAnchorElement>) => {
    onClick?.();
    if (!anchor || e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return; // let new-tab clicks through
    e.preventDefault();
    if (pathname === '/') {
      scrollToId(anchor);
    } else {
      try {
        sessionStorage.setItem(PENDING_KEY, anchor);
      } catch {}
      router.push('/');
    }
  };

  return (
    <Link href={href} onClick={handle} className={className} scroll={!anchor}>
      {children}
    </Link>
  );
}

/** Mount on the landing page: finishes a section jump requested from another page. */
export function PendingSectionScroll() {
  useEffect(() => {
    let id: string | null = null;
    try {
      id = sessionStorage.getItem(PENDING_KEY);
      sessionStorage.removeItem(PENDING_KEY);
    } catch {}
    if (!id) return;
    const target = id;
    // Wait a frame so the sections have laid out.
    const raf = requestAnimationFrame(() => setTimeout(() => scrollToId(target), 60));
    return () => cancelAnimationFrame(raf);
  }, []);
  return null;
}
