import type { Metadata } from 'next';
import { Suspense } from 'react';
import { RequireAuth } from '@/components/auth/Gates';
import { StudioApp } from '@/components/studio/StudioApp';

export const metadata: Metadata = { title: 'Studio' };

export default function StudioPage() {
  // useSearchParams (tab state) must sit under a Suspense boundary for static prerendering.
  return (
    <RequireAuth>
      <Suspense fallback={<div className="min-h-screen bg-obsidian-950" />}>
        <StudioApp />
      </Suspense>
    </RequireAuth>
  );
}
