import type { Metadata } from 'next';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';
import { Privacy } from '@/components/landing/Privacy';

export const metadata: Metadata = {
  title: 'Privacy & Data Retention Policy | Spectra Labs',
  description: 'Data retention schedules, privacy policies, and security commitments for the Spectra Labs quantitative platform.',
};

export default function PrivacyPage() {
  return (
    <>
      <Header />
      <main className="mx-auto max-w-[1320px] px-5 py-24 sm:px-8">
        <Privacy />
      </main>
      <Footer />
    </>
  );
}
