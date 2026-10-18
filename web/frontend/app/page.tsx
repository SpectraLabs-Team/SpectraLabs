import { Architecture } from '@/components/landing/Architecture';
import { BenchmarkShowcase } from '@/components/landing/BenchmarkShowcase';
import { Footer } from '@/components/landing/Footer';
import { Hero } from '@/components/landing/Hero';
import { Navbar } from '@/components/landing/Navbar';
import { Pillars } from '@/components/landing/Pillars';

export default function LandingPage() {
  return (
    <>
      <Navbar />
      <main>
        <Hero />
        <Pillars />
        <BenchmarkShowcase />
        <Architecture />
      </main>
      <Footer />
    </>
  );
}
