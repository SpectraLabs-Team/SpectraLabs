'use client';

import Link from 'next/link';
import { Compass } from 'lucide-react';
import { useAuth } from '@/lib/auth';

/**
 * "Take the tour" outside the Studio (Learning Center). Signed in: opens the Studio, which starts
 * the tour from the beginning (?tour=1). Signed out: opens Log In; new accounts get the tour on
 * their first visit to the Studio anyway.
 */
export function TourLauncher({ className = 'btn-secondary px-3.5 py-2 text-[13px]' }: { className?: string }) {
  const { isAuthenticated, isLoading, openAuthModal } = useAuth();
  if (isLoading) return <span className="inline-block h-9 w-[136px]" aria-hidden />;
  if (!isAuthenticated) {
    return (
      <button type="button" onClick={() => openAuthModal('login')} className={className}>
        <Compass className="h-4 w-4 text-accent" aria-hidden /> Log in to take the tour
      </button>
    );
  }
  return (
    <Link href="/studio?tour=1" className={className}>
      <Compass className="h-4 w-4 text-accent" aria-hidden /> Take the website tour
    </Link>
  );
}
