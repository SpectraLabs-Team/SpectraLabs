'use client';

import Link from 'next/link';
import { motion, useReducedMotion } from 'framer-motion';
import { ArrowRight, Layers } from 'lucide-react';
import { HeroTeaser } from './HeroTeaser';

export function Hero() {
  const reduce = useReducedMotion();
  const rise = (delay: number) => (reduce ? {} : { initial: { opacity: 0, y: 18 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] } });

  return (
    <section className="relative overflow-hidden pb-20 pt-32 sm:pt-36">
      <div className="bg-dot-grid mask-radial pointer-events-none absolute inset-0" />
      <div className="pointer-events-none absolute left-1/2 top-0 h-[520px] w-[900px] -translate-x-1/2 rounded-full bg-[radial-gradient(closest-side,rgba(34,211,238,0.16),transparent)]" />

      <div className="relative mx-auto grid max-w-7xl items-center gap-12 px-5 sm:px-8 lg:grid-cols-[1.08fr_0.92fr] lg:gap-14">
        <div>
          <motion.div {...rise(0)}>
            <span className="inline-flex items-center gap-2 rounded-full border border-accent/25 bg-accent/[0.07] px-3 py-1 font-mono text-[11px] text-accent">
              <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-[0_0_8px_#22D3EE]" />
              v0.7.0 Pure Rust · Zero Dependencies · Sub-millisecond Execution
            </span>
          </motion.div>
          <motion.h1 {...rise(0.06)} className="text-balance mt-6 text-4xl font-semibold leading-[1.05] tracking-[-0.035em] text-white sm:text-5xl lg:text-[3.6rem]">
            Evidence-First Quantitative Research &amp; <span className="text-gradient">Real-Time Portfolio Risk</span>
          </motion.h1>
          <motion.p {...rise(0.12)} className="mt-6 max-w-xl text-base leading-7 text-slate-400 sm:text-[17px]">
            A Monte Carlo risk engine written in pure Rust with deterministic parallelism, an event-driven backtester that is free of look-ahead by construction, and a SHA-256 hash-chained registry that makes every experiment auditable.
          </motion.p>
          <motion.div {...rise(0.18)} className="mt-9 flex flex-wrap gap-3">
            <Link href="/studio" className="btn-primary px-5 py-3">
              Launch Risk Studio <ArrowRight className="h-4 w-4" />
            </Link>
            <a href="#architecture" className="btn-secondary px-5 py-3">
              <Layers className="h-4 w-4" /> View Architecture
            </a>
          </motion.div>
          <motion.dl {...rise(0.24)} className="mt-12 grid max-w-lg grid-cols-3 gap-6 border-t border-white/[0.06] pt-6">
            {[
              ['0', 'third-party crates'],
              ['8', 'workspace crates'],
              ['bit-exact', 'parity with C++ reference'],
            ].map(([v, l]) => (
              <div key={l}>
                <dt className="num text-xl font-semibold text-white">{v}</dt>
                <dd className="mt-1 text-xs leading-5 text-slate-500">{l}</dd>
              </div>
            ))}
          </motion.dl>
        </div>
        <motion.div {...(reduce ? {} : { initial: { opacity: 0, y: 24, scale: 0.98 }, animate: { opacity: 1, y: 0, scale: 1 }, transition: { duration: 0.7, delay: 0.15, ease: [0.22, 1, 0.36, 1] } })}>
          <HeroTeaser />
        </motion.div>
      </div>
    </section>
  );
}
