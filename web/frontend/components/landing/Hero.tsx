'use client';

import Image from 'next/image';
import { useRef } from 'react';
import Link from 'next/link';
import { motion, useReducedMotion } from 'framer-motion';
import { ArrowRight } from 'lucide-react';
import { useLaunchStudio } from './Header';
import { DustOverlay } from './HeroScene';
import { TickerChips } from './HoloCards';

const BADGES = [
  { label: 'v0.7.0 Pure Rust', dot: 'bg-cyan-400 shadow-[0_0_8px_#22D3EE]' },
  { label: 'Sub-millisecond Execution', dot: 'bg-violet-400 shadow-[0_0_8px_#A78BFA]' },
  { label: 'Std-Only Core Engines', dot: 'bg-emerald-400 shadow-[0_0_8px_#34D399]' },
];

export function Hero() {
  const reduce = useReducedMotion();
  const sectionRef = useRef<HTMLElement>(null);
  const tickerRef = useRef<HTMLDivElement>(null);
  const launch = useLaunchStudio();

  const rise = (delay: number) =>
    reduce
      ? {}
      : {
          initial: { opacity: 0, y: 18 },
          animate: { opacity: 1, y: 0 },
          transition: { duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] as const },
        };

  return (
    <section ref={sectionRef} className="dark relative isolate overflow-hidden bg-[#040711] text-slate-200">
      {/* Backdrop plate with smooth ambient drift */}
      <div
        aria-hidden
        className="pointer-events-none absolute -bottom-6 -left-8 -right-6 -top-4 -z-20 transform-gpu animate-hero-drift will-change-transform [backface-visibility:hidden] [transform-origin:76%_18%]"
      >
        <Image
          src="/images/hero-backdrop.webp"
          alt=""
          fill
          priority
          unoptimized
          sizes="100vw"
          className="object-cover object-right-top lg:object-[82%_top] xl:object-[80%_top] 2xl:object-[78%_top]"
        />
      </div>

      {/* Layered Vignette & Ambient Cosmic Glows */}
      {/* Soft left scrim that preserves cosmic dust and starlight */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-[linear-gradient(90deg,rgba(4,7,17,0.58)_0%,rgba(4,7,17,0.32)_28%,rgba(4,7,17,0.08)_50%,transparent_70%)]"
      />
      {/* Radial edge vignette */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_120%_90%_at_60%_40%,transparent_55%,rgba(4,7,17,0.85)_100%)]"
      />
      {/* Left cyan glow behind headline */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_at_18%_42%,rgba(0,240,255,0.14),transparent_60%)]"
      />
      {/* Top and bottom subtle edge fades */}
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-20 bg-gradient-to-b from-[#040711]/70 to-transparent" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-24 bg-gradient-to-t from-canvas to-transparent" />

      {/* Twinkling star dust overlay (especially rich on the upper right) */}
      <div aria-hidden className="pointer-events-none absolute -inset-6 -z-10">
        <DustOverlay className="h-full w-full" />
      </div>

      <div className="relative mx-auto flex min-h-[640px] max-w-[1320px] flex-col px-5 pb-6 pt-24 sm:px-8 lg:h-[100svh] lg:max-h-[860px] lg:pt-[104px] [@media(max-height:760px)]:min-h-[600px] lg:[@media(max-height:760px)]:pt-[80px]">
        <div className="flex flex-1 items-start">
          {/* Headline Column */}
          <div className="relative z-10 max-w-[580px]">
            <motion.p {...rise(0)} className="font-mono text-[11.5px] font-semibold uppercase tracking-[0.24em] text-cyan-400">
              The next generation quant platform
            </motion.p>
            <motion.h1
              {...rise(0.06)}
              className="mt-4 text-[2.5rem] font-extrabold leading-[1.07] tracking-[-0.03em] text-white sm:text-[3rem] xl:text-[3.45rem] lg:[@media(max-height:760px)]:mt-3 lg:[@media(max-height:760px)]:text-[2.65rem]"
            >
              Institutional Quantitative <br className="hidden lg:inline" />
              Research &amp;{' '}
              <span className="bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#22D3EE] bg-clip-text text-transparent drop-shadow-[0_0_30px_rgba(34,211,238,0.4)]">
                Real-Time <br className="hidden lg:inline" />
                Portfolio Risk
              </span>
            </motion.h1>
            <motion.p {...rise(0.12)} className="mt-5 max-w-[500px] text-[16px] leading-[1.65] text-slate-300 lg:[@media(max-height:760px)]:mt-3">
              High-performance pure-Rust quantitative engine. Sub-millisecond Monte Carlo risk, zero look-ahead backtesting, and cryptographic auditability.
            </motion.p>
            <motion.div {...rise(0.16)} className="mt-6 flex flex-wrap gap-2.5 lg:[@media(max-height:760px)]:mt-4">
              {BADGES.map((b) => (
                <span
                  key={b.label}
                  className="inline-flex items-center gap-2 rounded-full border border-cyan-500/25 bg-[#06122a]/80 px-3.5 py-1.5 text-[12px] font-medium text-slate-200 shadow-[0_0_12px_rgba(34,211,238,0.15)] backdrop-blur-md"
                >
                  <span className={`h-2 w-2 rounded-full ${b.dot}`} />
                  {b.label}
                </span>
              ))}
            </motion.div>
            <motion.div {...rise(0.22)} className="mt-8 flex flex-wrap items-center gap-3.5 lg:[@media(max-height:760px)]:mt-5">
              <button
                type="button"
                onClick={launch}
                className="group inline-flex items-center gap-2 rounded-xl bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#0284C7] px-6 py-3.5 text-sm font-bold text-[#03131A] shadow-[0_0_30px_rgba(0,242,254,0.5)] transition-all hover:shadow-[0_0_45px_rgba(0,242,254,0.75)] hover:brightness-110 active:scale-[0.98]"
              >
                Launch Quant Studio{' '}
                <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
              </button>
              <Link
                href="/architecture"
                className="inline-flex cursor-pointer items-center rounded-xl border border-white/25 bg-[#040711]/60 px-6 py-3.5 text-sm font-semibold text-white backdrop-blur-md transition-all hover:border-white/45 hover:bg-white/10 active:scale-[0.98]"
              >
                View Architecture
              </Link>
            </motion.div>
          </div>
        </div>

        {/* Bottom Tickers riding the wave troughs */}
        <div ref={tickerRef} className="relative mt-8 lg:mt-2">
          <div className="flex flex-wrap items-end justify-center gap-3 sm:gap-5 lg:justify-between lg:px-[6%]">
            <TickerChips />
          </div>
          <p className="mt-2 text-center font-mono text-[10px] tracking-wide text-slate-500 lg:absolute lg:-bottom-5 lg:right-[6%] lg:mt-0">
            Illustrative quotes · no live market feed
          </p>
        </div>
      </div>
    </section>
  );
}
