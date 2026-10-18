import { Benchmarks } from '@/components/landing/Benchmarks';
import { Capabilities } from '@/components/landing/Capabilities';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';
import { Hero } from '@/components/landing/Hero';
import { Taxonomy } from '@/components/landing/Taxonomy';
import { TrustBar } from '@/components/landing/TrustBar';

export default function LandingPage() {
  return (
    <>
      <Header overHero />
      <main>
        <Hero />
        <Capabilities />
        <Taxonomy />
        <Benchmarks />
        <TrustBar />
      </main>
      <Footer />
    </>
  );
}
