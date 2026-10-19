import type { Metadata } from 'next';
import { AccountPage } from '@/components/account/AccountPage';
import { RequireAuth } from '@/components/auth/Gates';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';

export const metadata: Metadata = { title: 'Account', robots: { index: false } };

export default function Account() {
  return (
    <RequireAuth>
      <Header />
      <main className="mx-auto max-w-[920px] px-5 py-24 sm:px-8">
        <AccountPage />
      </main>
      <Footer />
    </RequireAuth>
  );
}
