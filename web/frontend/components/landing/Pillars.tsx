'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { Activity, FlaskConical, Fingerprint, Timer } from 'lucide-react';
import type { ReactNode } from 'react';
import { GlowCard } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { HashChainDemo } from './HashChainDemo';

function PathsVisual() {
  // Deterministic pseudo-random GBM-ish paths for decoration.
  const paths = Array.from({ length: 14 }, (_, k) => {
    let y = 50;
    let seed = (k + 1) * 9301;
    const pts = Array.from({ length: 40 }, (_, i) => {
      seed = (seed * 9301 + 49297) % 233280;
      y += (seed / 233280 - 0.5) * 11;
      return `${(i / 39) * 300},${y.toFixed(1)}`;
    });
    return { d: `M${pts.join(' L')}`, tail: y > 68 };
  });
  return (
    <svg viewBox="0 0 300 100" className="mt-6 h-36 w-full" preserveAspectRatio="none" aria-hidden>
      <line x1="0" x2="300" y1="68" y2="68" stroke="#F43F5E" strokeDasharray="4 4" strokeOpacity="0.6" />
      <text x="296" y="64" textAnchor="end" className="fill-loss font-mono text-[7px]">VaR</text>
      {paths.map((p, i) => (
        <path key={i} d={p.d} fill="none" stroke={p.tail ? '#F43F5E' : '#22D3EE'} strokeOpacity={p.tail ? 0.9 : 0.4} strokeWidth="1" vectorEffect="non-scaling-stroke" />
      ))}
    </svg>
  );
}

function TimelineVisual() {
  return (
    <div className="mt-6 space-y-2 font-mono text-[11px]">
      {[
        ['T−1', 'submit order', 'text-slate-400'],
        ['T', 'fill @ open · mark @ close', 'text-accent'],
        ['T', 'on_fill → on_bar', 'text-slate-300'],
        ['T+1', 'future bars: never visible', 'text-slate-600 line-through decoration-loss/60'],
      ].map(([t, label, cls], i) => (
        <div key={i} className="flex items-center gap-3">
          <span className="w-8 shrink-0 text-right text-slate-500">{t}</span>
          <span className={cn('h-1.5 w-1.5 shrink-0 rounded-full', i === 1 ? 'bg-accent shadow-[0_0_8px_#22D3EE]' : 'bg-white/20')} />
          <span className={cls}>{label}</span>
        </div>
      ))}
    </div>
  );
}

function ValidationVisual() {
  const bars = [0.92, 0.81, 0.74, 0.66, 0.58, 0.49, 0.42, 0.33, 0.27, 0.2];
  return (
    <div className="mt-6">
      <div className="flex h-20 items-end gap-1.5">
        {bars.map((h, i) => (
          <div key={i} className="flex-1 rounded-t-sm" style={{ height: `${h * 100}%`, background: i === 0 ? 'linear-gradient(to top, rgba(244,63,94,0.2), #F43F5E)' : 'rgba(56,189,248,0.25)' }} />
        ))}
      </div>
      <p className="mt-2 font-mono text-[10.5px] text-slate-500">
        illustrative · the best of N trials always looks good. <span className="text-loss">DSR</span> discounts for the selection.
      </p>
    </div>
  );
}

const pillars: { title: string; icon: typeof Activity; body: ReactNode; tags: string[]; span: string; visual: ReactNode }[] = [
  {
    title: 'Monte Carlo Risk Engine',
    icon: Activity,
    body: 'Millions of GBM paths per second for VaR, Expected Shortfall and distribution metrics. Parallel runs split into fixed seeded chunks, so results depend only on the seed, never on the thread count.',
    tags: ['VaR · CVaR', 'deterministic chunks', 'CPU reference bit-exact'],
    span: 'md:col-span-4',
    visual: <PathsVisual />,
  },
  {
    title: 'Event-Driven Backtester',
    icon: Timer,
    body: 'Free of look-ahead by construction. Orders fill no earlier than the next bar, open orders sit in a price-indexed book, and the ledger enforces a gross leverage limit.',
    tags: ['zero look-ahead', 'limit order book', 'leverage limit'],
    span: 'md:col-span-2',
    visual: <TimelineVisual />,
  },
  {
    title: 'Overfitting & Validation',
    icon: FlaskConical,
    body: 'Every trial in a sweep is ledgered. The Deflated Sharpe Ratio, CSCV probability of backtest overfitting and the Strategy Survival Lab tell you whether an edge is real.',
    tags: ['PSR / DSR', 'CSCV PBO', 'Survival Lab', 'walk-forward'],
    span: 'md:col-span-2',
    visual: <ValidationVisual />,
  },
  {
    title: 'Tamper-Evident Ledger',
    icon: Fingerprint,
    body: (
      <>
        Experiments, runs, artifacts and notes go into one append-only <code className="font-mono text-accent">registry.jsonl</code>. Each event commits to the previous SHA-256 hash, so editing history is detected on every open. Try it:
      </>
    ),
    tags: ['SHA-256 chain', 'holdout vault', 'C++ file-compatible'],
    span: 'md:col-span-4',
    visual: <HashChainDemo />,
  },
];

export function Pillars() {
  const reduce = useReducedMotion();
  return (
    <section id="features" className="relative py-24 sm:py-28">
      <div className="mx-auto max-w-7xl px-5 sm:px-8">
        <div className="max-w-2xl">
          <p className="eyebrow">Platform</p>
          <h2 className="mt-3 text-3xl font-semibold tracking-tight text-white sm:text-4xl">Four engines. One standard of evidence.</h2>
          <p className="mt-4 text-slate-400">Each component was ported from a frozen C++ reference and has to reproduce its golden outputs exactly before it ships.</p>
        </div>
        <div className="mt-12 grid gap-4 md:grid-cols-6">
          {pillars.map((p, i) => (
            <motion.div
              key={p.title}
              className={p.span}
              initial={reduce ? false : { opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: '-80px' }}
              transition={{ duration: 0.5, delay: (i % 2) * 0.08 }}
            >
              <GlowCard className="flex h-full flex-col p-6 sm:p-7">
                <div className="flex items-center gap-3">
                  <span className="flex h-9 w-9 items-center justify-center rounded-lg border border-accent/20 bg-accent/10 text-accent">
                    <p.icon className="h-[18px] w-[18px]" />
                  </span>
                  <h3 className="text-base font-semibold text-white">{p.title}</h3>
                </div>
                <p className="mt-4 text-sm leading-6 text-slate-400">{p.body}</p>
                <div className="mt-4 flex flex-wrap gap-1.5">
                  {p.tags.map((t) => (
                    <span key={t} className="rounded-md border border-white/[0.07] bg-white/[0.02] px-2 py-0.5 font-mono text-[10.5px] text-slate-400">
                      {t}
                    </span>
                  ))}
                </div>
                <div className="mt-auto">{p.visual}</div>
              </GlowCard>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
