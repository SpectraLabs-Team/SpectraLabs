import type { Metadata } from 'next';
import { Architecture } from '@/components/landing/Architecture';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';

export const metadata: Metadata = {
  title: 'Architecture',
  description: 'The eight-crate, std-only Rust workspace behind Spectra Labs and its byte-for-byte parity with the C++ reference engines.',
};

export default function ArchitecturePage() {
  return (
    <>
      <Header />
      <main className="mx-auto max-w-[1320px] px-5 py-24 sm:px-8">
        <Architecture />
      </main>
      <Footer />
    </>
  );
}
