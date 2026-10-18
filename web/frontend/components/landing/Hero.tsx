'use client';

import Link from 'next/link';
import { motion, useReducedMotion } from 'framer-motion';
import { ArrowRight } from 'lucide-react';
import { HEADLINE } from '@/lib/baseline';
import { formatDuration } from '@/lib/utils';
import { GalaxyCanvas, WavesCanvas } from './HeroScene';
import { HoloCards, TickerChips, useHeroData } from './HoloCards';
import { analyticVaR } from '@/lib/model';
import { SAMPLE_CSV, parseCsv, portfolioStats } from '@/lib/portfolio';
import { formatPercent } from '@/lib/utils';

const STATS = portfolioStats(parseCsv(SAMPLE_CSV).positions);

const BADGES = [
  { label: 'v0.7.0 Pure Rust', dot: 'bg-accent' },
  { label: 'Sub-millisecond Execution', dot: 'bg-violet-500' },
  { label: 'Zero Dependencies', dot: 'bg-profit' },
];

// Decorative labels on the wave: real figures from the sample book and the reference run.
const LABELS = [
  { text: `σ ${formatPercent(STATS.modelVolatility, 1)}`, cls: 'left-[4%] top-[63%]' },
  { text: `p50 ${HEADLINE.smallMs ? formatDuration(HEADLINE.smallMs) : ''}`, cls: 'left-[31%] top-[56%]' },
  { text: `VaR₉₉ ${formatPercent(analyticVaR({ value: STATS.netValue, volatility: STATS.modelVolatility, horizonDays: 1 }, 0.99) / STATS.netValue)}`, cls: 'right-[24%] top-[60%]' },
  { text: `HHI ${STATS.hhi.toFixed(3)}`, cls: 'right-[3%] top-[70%]' },
];

export function Hero() {
  const reduce = useReducedMotion();
  const data = useHeroData();
  const rise = (delay: number) => (reduce ? {} : { initial: { opacity: 0, y: 18 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] } });

  return (
    <section className="relative overflow-hidden pt-16">
      {/* Space backdrop */}
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_70%_20%,rgba(79,70,229,0.10),transparent_55%)] dark:bg-[radial-gradient(ellipse_at_70%_20%,rgba(79,70,229,0.22),transparent_55%),radial-gradient(ellipse_at_20%_80%,rgba(34,211,238,0.10),transparent_50%)]" />
      <div className="bg-dot-grid pointer-events-none absolute inset-0 opacity-50 mask-fade-b" />
      <GalaxyCanvas className="pointer-events-none absolute right-[-6%] top-0 h-[78%] w-[66%] opacity-90" />
      <WavesCanvas className="pointer-events-none absolute inset-x-0 bottom-0 h-[56%] w-full [mask-image:linear-gradient(to_bottom,transparent_0%,black_38%)]" />
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-canvas to-transparent" />

      <div className="relative mx-auto max-w-[1320px] px-5 sm:px-8">
        <div className="relative min-h-[680px] lg:min-h-[760px]">
          <div className="relative z-10 max-w-[600px] pt-14 sm:pt-20">
            <motion.p {...rise(0)} className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/70">
              The next generation quant platform
            </motion.p>
            <motion.h1 {...rise(0.06)} className="text-balance mt-5 text-[2.6rem] font-bold leading-[1.06] tracking-[-0.03em] text-ink sm:text-5xl lg:text-[3.5rem]">
              Institutional Quantitative Research &amp; <span className="text-gradient">Real-Time Portfolio Risk</span>
            </motion.h1>
            <motion.p {...rise(0.12)} className="mt-6 max-w-[540px] text-[16.5px] leading-7 text-ink/70">
              High-performance pure-Rust quantitative engine. Sub-millisecond Monte Carlo risk, zero look-ahead backtesting, and cryptographic auditability.
            </motion.p>
            <motion.div {...rise(0.16)} className="mt-6 flex flex-wrap gap-2.5">
              {BADGES.map((b) => (
                <span key={b.label} className="inline-flex items-center gap-2 rounded-full border border-ink/10 bg-panel/60 px-3 py-1.5 text-[12px] text-ink/80 backdrop-blur dark:border-white/10">
                  <span className={`h-2 w-2 rounded-full ${b.dot}`} />
                  {b.label}
                </span>
              ))}
            </motion.div>
            <motion.div {...rise(0.22)} className="mt-9 flex flex-wrap gap-3">
              <Link href="/studio" className="btn-primary px-6 py-3">
                Launch Quant Studio <ArrowRight className="h-4 w-4" />
              </Link>
              <a href="#architecture" className="btn-secondary bg-panel/40 px-6 py-3 backdrop-blur">
                View Architecture
              </a>
            </motion.div>
          </div>

          {/* Floating cards (desktop) */}
          <div className="pointer-events-none absolute inset-0 hidden lg:block [&>*]:pointer-events-auto">
            <HoloCards layout="float" data={data} />
            <TickerChips layout="float" />
            {LABELS.map((l) => (
              <span key={l.text} className={`absolute font-mono text-[10.5px] text-accent/70 ${l.cls}`}>
                {l.text}
              </span>
            ))}
          </div>

          {/* Stacked cards (mobile / tablet) */}
          <div className="relative z-10 mt-12 grid gap-3 pb-16 sm:grid-cols-2 lg:hidden">
            <HoloCards layout="stack" data={data} />
          </div>
          <div className="relative z-10 flex flex-wrap gap-3 pb-12 lg:hidden">
            <TickerChips layout="row" />
          </div>
        </div>
      </div>
    </section>
  );
}
