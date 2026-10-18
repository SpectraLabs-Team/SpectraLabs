'use client';

import Image from 'next/image';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { motion, useMotionValue, useReducedMotion, useSpring, useTransform } from 'framer-motion';
import { ArrowRight } from 'lucide-react';
import { DustOverlay } from './HeroScene';
import { HoloCards, TickerChips, useHeroData } from './HoloCards';
import { layoutHeroCards, spiralEllipses, type Rect } from './heroLayout';

// useLayoutEffect warns during SSR; this hero only measures in the browser.
const useIsoLayoutEffect = typeof window === 'undefined' ? useEffect : useLayoutEffect;

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
  const sectionRef = useRef<HTMLElement>(null);
  const plateRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const tickerRef = useRef<HTMLDivElement>(null);
  const cardRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [positions, setPositions] = useState<(Rect | null)[] | null>(null);

  // Mouse parallax on ONE foreground layer (dust + cards) via a continuous Framer spring: no manual
  // RAF loop, no start/stop threshold. The backdrop only runs its CSS drift, so no element ever has
  // two transform sources. Off for touch devices and reduced motion.
  const mx = useMotionValue(0);
  const my = useMotionValue(0);
  const sx = useSpring(mx, { stiffness: 50, damping: 18, mass: 0.8 });
  const sy = useSpring(my, { stiffness: 50, damping: 18, mass: 0.8 });
  const fx = useTransform(sx, (v) => v * 10);
  const fy = useTransform(sy, (v) => v * 7);
  useEffect(() => {
    const section = sectionRef.current;
    if (!section || reduce || !window.matchMedia('(hover: hover) and (pointer: fine)').matches) return;
    const onMove = (e: PointerEvent) => {
      const r = section.getBoundingClientRect();
      mx.set(((e.clientX - r.left) / r.width) * 2 - 1);
      my.set(((e.clientY - r.top) / r.height) * 2 - 1);
    };
    const onLeave = () => {
      mx.set(0);
      my.set(0);
    };
    section.addEventListener('pointermove', onMove, { passive: true });
    section.addEventListener('pointerleave', onLeave);
    return () => {
      section.removeEventListener('pointermove', onMove);
      section.removeEventListener('pointerleave', onLeave);
    };
  }, [reduce, mx, my]);

  // Card placement from measured geometry (fonts, card heights, plate scale). Re-runs on resize,
  // card size changes and once web fonts finish loading.
  const measure = useCallback(() => {
    const sec = sectionRef.current;
    const plate = plateRef.current;
    const content = contentRef.current;
    const text = textRef.current;
    const ticker = tickerRef.current;
    const cards = cardRefs.current;
    if (!sec || !plate || !content || !text || !ticker || cards.length < 3 || cards.some((c) => !c)) return;
    if (window.innerWidth < 1024) return setPositions(null);
    if (cards.some((c) => !c!.offsetWidth || !c!.offsetHeight)) return;
    const sr = sec.getBoundingClientRect();
    // Widest rendered text line in the headline column.
    let textRight = 0;
    const range = document.createRange();
    text.querySelectorAll('p, h1, a, span').forEach((el) => {
      range.selectNodeContents(el);
      for (const r of Array.from(range.getClientRects())) textRight = Math.max(textRight, r.right - sr.left);
    });
    const cr = content.getBoundingClientRect();
    const padR = parseFloat(getComputedStyle(content).paddingRight) || 0;
    const tr = text.getBoundingClientRect();
    const kr = ticker.getBoundingClientRect();
    setPositions(
      layoutHeroCards({
        sizes: cards.map((c) => ({ w: c!.offsetWidth, h: c!.offsetHeight })),
        // Plate layout box (offset* ignore the drift transform); 14px = 10px parallax travel + 4px air.
        ellipses: spiralEllipses({ x: plate.offsetLeft, y: plate.offsetTop, w: plate.offsetWidth, h: plate.offsetHeight }, 14),
        minX: textRight + 28,
        maxX: cr.right - sr.left - padR,
        minY: tr.top - sr.top,
        maxY: kr.top - sr.top - 16,
      }),
    );
  }, []);

  useIsoLayoutEffect(() => {
    measure();
    const ro = new ResizeObserver(() => measure());
    [sectionRef.current, textRef.current, tickerRef.current, ...cardRefs.current].forEach((el) => el && ro.observe(el));
    document.fonts?.ready.then(() => measure());
    return () => ro.disconnect();
  }, [measure]);

  const rise = (delay: number) => (reduce ? {} : { initial: { opacity: 0, y: 18 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] } });

  return (
    <section ref={sectionRef} className="dark relative isolate overflow-hidden bg-[#040711] text-slate-200">
      {/* Backdrop plate: a single GPU layer running only the CSS ambient drift (20s, ease-in-out,
          alternate), scaling about the galaxy. Oversized past the section edges so the drift never reveals
          an edge; anchored top-right so the whole spiral stays in frame. */}
      <div ref={plateRef} aria-hidden className="pointer-events-none absolute -bottom-6 -left-8 -right-6 -top-4 -z-20 transform-gpu animate-hero-drift will-change-transform [backface-visibility:hidden] [transform-origin:84%_22%]">
        <Image src="/images/hero-backdrop.webp" alt="" fill priority sizes="100vw" className="object-cover object-right-top opacity-95" />
      </div>
      {/* Vignette + ambient glows so the plate blends into the page */}
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[linear-gradient(90deg,rgba(4,7,17,0.72)_0%,rgba(4,7,17,0.45)_25%,rgba(4,7,17,0.12)_45%,transparent_60%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_120%_90%_at_60%_40%,transparent_55%,rgba(4,7,17,0.85)_100%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_at_18%_42%,rgba(0,240,255,0.12),transparent_60%),radial-gradient(35%_30%_at_70%_24%,rgba(139,92,246,0.12),transparent_70%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-20 bg-gradient-to-b from-[#040711]/70 to-transparent" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-24 bg-gradient-to-t from-canvas to-transparent" />
      {/* Foreground parallax layer: dust + holo cards move together against the drifting plate. */}
      <motion.div className="pointer-events-none absolute inset-0 transform-gpu will-change-transform" style={{ x: fx, y: fy }}>
        <div aria-hidden className="absolute -inset-6">
          <DustOverlay className="h-full w-full" />
        </div>
        <div className="hidden lg:block">
          <HoloCards data={data} layout="placed" positions={positions} cardRefs={cardRefs} />
        </div>
      </motion.div>

      <div ref={contentRef} className="relative mx-auto flex min-h-[640px] max-w-[1320px] flex-col px-5 pb-6 pt-24 sm:px-8 lg:h-[100svh] lg:max-h-[860px] lg:pt-[104px] [@media(max-height:760px)]:min-h-[600px] lg:[@media(max-height:760px)]:pt-[80px]">
        <div className="grid flex-1 items-start gap-10 lg:grid-cols-[minmax(0,520px)_minmax(0,1fr)] lg:gap-12 xl:grid-cols-[minmax(0,620px)_minmax(0,1fr)]">
          {/* Headline */}
          <div ref={textRef} className="relative z-10">
            <motion.p {...rise(0)} className="font-mono text-[11.5px] font-medium uppercase tracking-[0.24em] text-slate-300">
              The next generation quant platform
            </motion.p>
            <motion.h1 {...rise(0.06)} className="mt-4 text-[2.4rem] font-bold leading-[1.07] tracking-[-0.03em] text-white sm:text-[2.9rem] xl:text-[3.1rem] lg:[@media(max-height:760px)]:mt-3 lg:[@media(max-height:760px)]:text-[2.5rem]">
              Institutional Quantitative <br className="hidden lg:inline" />
              Research &amp;{' '}
              <span className="bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#22D3EE] bg-clip-text text-transparent drop-shadow-[0_0_25px_rgba(0,242,254,0.4)]">
                Real-Time <br className="hidden lg:inline" />
                Portfolio Risk
              </span>
            </motion.h1>
            <motion.p {...rise(0.12)} className="mt-5 max-w-[520px] text-[15.5px] leading-[1.65] text-slate-300/90 lg:[@media(max-height:760px)]:mt-3">
              High-performance pure-Rust quantitative engine. Sub-millisecond Monte Carlo risk, zero look-ahead backtesting, and cryptographic auditability.
            </motion.p>
            <motion.div {...rise(0.16)} className="mt-5 flex flex-wrap gap-2 lg:[@media(max-height:760px)]:mt-4">
              {BADGES.map((b) => (
                <span key={b.label} className="inline-flex items-center gap-2 rounded-full border border-cyan-500/25 bg-[#06122a]/80 px-3 py-1.5 text-[12px] text-slate-200 shadow-[0_0_12px_rgba(34,211,238,0.15)]">
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

          {/* Holo cards: placed in the foreground layer above on lg+, a simple grid below lg. */}
          <div className="lg:hidden">
            <HoloCards data={data} layout="grid" />
          </div>
        </div>

        {/* Tickers riding the wave troughs */}
        <div ref={tickerRef} className="relative mt-8 lg:mt-2">
          <div className="flex flex-wrap items-end justify-center gap-3 sm:gap-5 lg:justify-between lg:px-[6%]">
            <TickerChips />
          </div>
          {/* Out of flow so it costs the hero no height. */}
          <p className="mt-2 text-center font-mono text-[10px] tracking-wide text-slate-500 lg:absolute lg:-bottom-5 lg:right-[6%] lg:mt-0">Illustrative quotes · no live market feed</p>
        </div>
      </div>
    </section>
  );
}
