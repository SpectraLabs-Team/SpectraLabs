'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { Boxes, CheckCircle2, Terminal } from 'lucide-react';
import { GlassCard } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type Crate = { name: string; role: string; tone?: 'accent' | 'ice' };

const layers: { label: string; crates: Crate[] }[] = [
  {
    label: 'Interfaces',
    crates: [
      { name: 'web/frontend', role: 'Next.js studio', tone: 'ice' },
      { name: 'spectra-api', role: 'std-only HTTP/1.1 server', tone: 'accent' },
      { name: 'spectra-cli', role: 'ingest · registry · risk binaries' },
    ],
  },
  {
    label: 'Engines',
    crates: [
      { name: 'spectra-risk', role: 'Monte Carlo · stress · benchmark', tone: 'accent' },
      { name: 'spectra-engine', role: 'event-driven backtester' },
      { name: 'spectra-research', role: 'sweeps · DSR · PBO · Survival Lab' },
      { name: 'spectra-registry', role: 'hash-chained log · holdout vault' },
    ],
  },
  {
    label: 'Foundation',
    crates: [
      { name: 'spectra-data', role: 'CSV ingest · content-addressed store' },
      { name: 'spectra-core', role: 'fixed-point money · ledger · JSON · SHA-256 · RNG' },
    ],
  },
];

const parity = [
  ['Ledger', '12 scripts · 4,800 operations', 'byte-identical journals'],
  ['Data', '31 CSV cases', 'byte-identical bars + metadata'],
  ['Backtester', '39 runs', 'identical result fingerprints'],
  ['Research', '565-line transcript', 'every trial, PBO, DSR, walk-forward'],
  ['Registry', '140-event log', 'each engine verifies the other'],
  ['Risk', 'Monte Carlo losses', 'bit-identical path by path (CPU)'],
];

export function Architecture() {
  const reduce = useReducedMotion();
  return (
    <section id="architecture" className="relative py-24 sm:py-28">
      <div className="mx-auto max-w-7xl px-5 sm:px-8">
        <div className="max-w-2xl">
          <p className="eyebrow">Architecture</p>
          <h2 className="mt-3 text-3xl font-semibold tracking-tight text-white sm:text-4xl">Eight crates. Standard library only.</h2>
          <p className="mt-4 text-slate-400">
            No crates.io dependencies and <code className="font-mono text-[13px] text-slate-300">unsafe_code = &quot;forbid&quot;</code> across the workspace. The HTTP server, JSON, SHA-256 and RNG are written in-house, so the whole supply chain fits in one review.
          </p>
        </div>

        <div className="mt-12 grid gap-5 lg:grid-cols-[1.25fr_0.75fr]">
          <GlassCard className="p-5 sm:p-7">
            <div className="flex items-center gap-2 text-sm font-semibold text-white">
              <Boxes className="h-4 w-4 text-accent" /> Workspace map
            </div>
            <div className="mt-6 space-y-3">
              {layers.map((layer, li) => (
                <motion.div
                  key={layer.label}
                  initial={reduce ? false : { opacity: 0, x: -12 }}
                  whileInView={{ opacity: 1, x: 0 }}
                  viewport={{ once: true }}
                  transition={{ delay: li * 0.1, duration: 0.45 }}
                  className="grid gap-3 sm:grid-cols-[96px_1fr] sm:items-center"
                >
                  <span className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-slate-500">{layer.label}</span>
                  <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
                    {layer.crates.map((c) => (
                      <div
                        key={c.name}
                        className={cn(
                          'rounded-lg border px-3 py-2.5',
                          c.tone === 'accent' ? 'border-accent/30 bg-accent/[0.06]' : c.tone === 'ice' ? 'border-accent-ice/25 bg-accent-ice/[0.05]' : 'border-white/[0.07] bg-obsidian-950/60',
                        )}
                      >
                        <p className={cn('font-mono text-[12px] font-medium', c.tone ? 'text-accent' : 'text-slate-200')}>{c.name}</p>
                        <p className="mt-0.5 text-[11px] leading-4 text-slate-500">{c.role}</p>
                      </div>
                    ))}
                  </div>
                </motion.div>
              ))}
            </div>
            <div className="mt-6 rounded-lg border border-white/[0.07] bg-obsidian-950/80 p-4 font-mono text-[12px] leading-6">
              <div className="mb-2 flex items-center gap-2 text-[10.5px] uppercase tracking-wider text-slate-500">
                <Terminal className="h-3.5 w-3.5" /> request path
              </div>
              <p className="text-slate-400">
                <span className="text-accent">studio</span> → POST /api/risk → <span className="text-accent">spectra-api</span> → RiskEngine::calculate_named → <span className="text-accent">Parallel</span> kernel → RiskReport (seed, backend, timings, version)
              </p>
            </div>
          </GlassCard>

          <GlassCard className="p-5 sm:p-7">
            <p className="text-sm font-semibold text-white">Verified against the C++ reference</p>
            <p className="mt-1.5 text-xs leading-5 text-slate-500">Each original C++ engine was frozen. Golden outputs must be reproduced exactly, and every parity test is mutation-checked.</p>
            <ul className="mt-5 divide-y divide-white/[0.05]">
              {parity.map(([area, scope, result]) => (
                <li key={area} className="flex gap-3 py-2.5">
                  <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-profit" />
                  <div className="min-w-0">
                    <p className="text-[13px] text-slate-200">
                      {area} <span className="font-mono text-[11px] text-slate-500">· {scope}</span>
                    </p>
                    <p className="text-[11.5px] text-slate-500">{result}</p>
                  </div>
                </li>
              ))}
            </ul>
          </GlassCard>
        </div>
      </div>
    </section>
  );
}
