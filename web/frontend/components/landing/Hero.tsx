'use client';

import Image from 'next/image';
import Link from 'next/link';
import { motion, useReducedMotion } from 'framer-motion';
import { ArrowRight } from 'lucide-react';
import { DustOverlay } from './HeroScene';
import { HoloCards, TickerChips, useHeroData } from './HoloCards';

const BADGES = [
  { label: 'v0.7.0 Pure Rust', dot: 'bg-cyan-400 shadow-[0_0_8px_#22D3EE]' },
  { label: 'Sub-millisecond Execution', dot: 'bg-violet-400 shadow-[0_0_8px_#A78BFA]' },
  { label: 'Zero Dependencies', dot: 'bg-emerald-400 shadow-[0_0_8px_#34D399]' },
];

/**
 * The hero is always rendered dark (it sits on a dark artwork plate), in both themes.
 * Height: fills the viewport between ~640 and 860px so the ticker row stays above the fold
 * on 14"/16" laptops.
 */
export function Hero() {
  const reduce = useReducedMotion();
  const data = useHeroData();
  const rise = (delay: number) => (reduce ? {} : { initial: { opacity: 0, y: 18 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] } });

  return (
    <section className="dark relative isolate overflow-hidden bg-[#040711] text-slate-200">
      {/* Backdrop plate */}
      <Image src="/images/hero-backdrop.webp" alt="" fill priority sizes="100vw" className="-z-20 object-cover object-[70%_bottom] opacity-95" />
      {/* Vignette + ambient glows so the plate blends into the page */}
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[linear-gradient(90deg,#040711_0%,rgba(4,7,17,0.85)_22%,rgba(4,7,17,0.25)_48%,transparent_70%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_120%_90%_at_60%_40%,transparent_55%,rgba(4,7,17,0.85)_100%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(40%_35%_at_20%_30%,rgba(34,211,238,0.10),transparent_70%),radial-gradient(35%_30%_at_78%_22%,rgba(139,92,246,0.14),transparent_70%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-28 bg-gradient-to-b from-[#040711] to-transparent" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-24 bg-gradient-to-t from-canvas to-transparent" />
      <DustOverlay className="pointer-events-none absolute inset-0 -z-10 h-full w-full" />

      <div className="mx-auto flex min-h-[640px] max-w-[1320px] flex-col px-5 pb-6 pt-24 sm:px-8 lg:h-[100svh] lg:max-h-[860px] lg:pt-[104px] [@media(max-height:760px)]:min-h-[600px] lg:[@media(max-height:760px)]:pt-[80px]">
        <div className="grid flex-1 items-start gap-10 lg:grid-cols-[minmax(0,520px)_minmax(0,1fr)] lg:gap-12 xl:grid-cols-[minmax(0,620px)_minmax(0,1fr)]">
          {/* Headline */}
          <div className="relative z-10">
            <motion.p {...rise(0)} className="font-mono text-[11.5px] font-medium uppercase tracking-[0.24em] text-slate-300">
              The next generation quant platform
            </motion.p>
            <motion.h1 {...rise(0.06)} className="mt-4 text-[2.4rem] font-bold leading-[1.07] tracking-[-0.03em] text-white sm:text-[2.9rem] xl:text-[3.25rem] lg:[@media(max-height:760px)]:mt-3 lg:[@media(max-height:760px)]:text-[2.5rem]">
              Institutional Quantitative <br className="hidden lg:inline" />
              Research &amp;{' '}
              <span className="bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#22D3EE] bg-clip-text text-transparent">
                Real-Time <br className="hidden lg:inline" />
                Portfolio Risk
              </span>
            </motion.h1>
            <motion.p {...rise(0.12)} className="mt-5 max-w-[520px] text-[15.5px] leading-[1.65] text-slate-300/90 lg:[@media(max-height:760px)]:mt-3">
              High-performance pure-Rust quantitative engine. Sub-millisecond Monte Carlo risk, zero look-ahead backtesting, and cryptographic auditability.
            </motion.p>
            <motion.div {...rise(0.16)} className="mt-5 flex flex-wrap gap-2 lg:[@media(max-height:760px)]:mt-4">
              {BADGES.map((b) => (
                <span key={b.label} className="inline-flex items-center gap-2 rounded-full border border-white/15 bg-[#06122a]/60 px-3 py-1.5 text-[12px] text-slate-200 backdrop-blur">
                  <span className={`h-2 w-2 rounded-full ${b.dot}`} />
                  {b.label}
                </span>
              ))}
            </motion.div>
            <motion.div {...rise(0.22)} className="mt-7 flex flex-wrap gap-3 lg:[@media(max-height:760px)]:mt-5">
              <Link
                href="/studio"
                className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-[#00F2FE] to-[#38BDF8] px-6 py-3 text-sm font-semibold text-[#03131A] shadow-[0_0_0_1px_rgba(0,242,254,0.5),0_0_28px_-2px_rgba(0,242,254,0.7)] transition hover:shadow-[0_0_0_1px_rgba(0,242,254,0.7),0_0_44px_0_rgba(0,242,254,0.85)] hover:brightness-110 active:scale-[0.98]"
              >
                Launch Quant Studio <ArrowRight className="h-4 w-4" />
              </Link>
              <a href="#architecture" className="inline-flex items-center rounded-lg border border-white/25 bg-[#040711]/50 px-6 py-3 text-sm font-semibold text-white backdrop-blur transition hover:border-white/45 hover:bg-white/5">
                View Architecture
              </a>
            </motion.div>
          </div>

          {/* Holographic cards: their own column, stepped diagonally */}
          <div className="hidden pt-6 lg:block xl:pl-6 [@media(max-height:760px)]:pt-2">
            <HoloCards data={data} layout="stepped" />
          </div>
          <div className="lg:hidden">
            <HoloCards data={data} layout="grid" />
          </div>
        </div>

        {/* Tickers riding the wave troughs */}
        <div className="mt-8 flex flex-wrap items-end justify-center gap-3 sm:gap-5 lg:mt-2 lg:justify-between lg:px-[6%]">
          <TickerChips />
        </div>
      </div>
    </section>
  );
}
