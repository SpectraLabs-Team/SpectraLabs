'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { Boxes, CheckCircle2, Terminal } from 'lucide-react';
import { cn } from '@/lib/utils';

type Crate = { name: string; role: string; hi?: boolean };

const layers: { label: string; crates: Crate[] }[] = [
  { label: 'Interfaces', crates: [{ name: 'web/frontend', role: 'Next.js studio', hi: true }, { name: 'spectra-api', role: 'std-only HTTP/1.1 server', hi: true }, { name: 'spectra-cli', role: 'ingest · registry · risk binaries' }] },
  { label: 'Engines', crates: [{ name: 'spectra-risk', role: 'Monte Carlo · stress · benchmark', hi: true }, { name: 'spectra-engine', role: 'event-driven backtester' }, { name: 'spectra-research', role: 'sweeps · DSR · PBO · Survival Lab' }, { name: 'spectra-registry', role: 'hash-chained log · holdout vault' }] },
  { label: 'Foundation', crates: [{ name: 'spectra-data', role: 'CSV ingest · content-addressed store' }, { name: 'spectra-core', role: 'fixed-point money · ledger · JSON · SHA-256 · RNG' }] },
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
    <section id="architecture" className="py-16">
      <div className="mx-auto max-w-[1320px] px-5 sm:px-8">
        <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Architecture</p>
        <h2 className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.15rem]">Eight crates. Standard library only.</h2>
        <p className="mt-3 max-w-2xl text-sm leading-6 text-ink/60">
          No crates.io dependencies and <code className="font-mono text-ink/80">unsafe_code = &quot;forbid&quot;</code> across the workspace. The HTTP server, JSON, SHA-256 and RNG are written in-house, so the whole supply chain fits in one review.
        </p>

        <div className="mt-10 grid gap-5 lg:grid-cols-[1.3fr_0.7fr]">
          <div className="glass p-5 sm:p-7">
            <div className="flex items-center gap-2 text-sm font-semibold text-ink">
              <Boxes className="h-4 w-4 text-accent" /> Workspace map
            </div>
            <div className="mt-6 space-y-3">
              {layers.map((layer, li) => (
                <motion.div key={layer.label} initial={reduce ? false : { opacity: 0, x: -12 }} whileInView={{ opacity: 1, x: 0 }} viewport={{ once: true }} transition={{ delay: li * 0.1, duration: 0.45 }} className="grid gap-3 sm:grid-cols-[96px_1fr] sm:items-center">
                  <span className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-ink/45">{layer.label}</span>
                  <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
                    {layer.crates.map((c) => (
                      <div key={c.name} className={cn('rounded-lg border px-3 py-2.5', c.hi ? 'border-accent/30 bg-accent/[0.06]' : 'border-ink/[0.08] bg-canvas/60')}>
                        <p className={cn('font-mono text-[12px] font-medium', c.hi ? 'text-accent' : 'text-ink/85')}>{c.name}</p>
                        <p className="mt-0.5 text-[11px] leading-4 text-ink/50">{c.role}</p>
                      </div>
                    ))}
                  </div>
                </motion.div>
              ))}
            </div>
            <div className="mt-6 rounded-lg border border-ink/[0.08] bg-canvas/70 p-4 font-mono text-[12px] leading-6">
              <div className="mb-2 flex items-center gap-2 text-[10.5px] uppercase tracking-wider text-ink/45">
                <Terminal className="h-3.5 w-3.5" /> request path
              </div>
              <p className="text-ink/60">
                <span className="text-accent">studio</span> → POST /api/risk → <span className="text-accent">spectra-api</span> → RiskEngine::calculate_named → <span className="text-accent">Parallel</span> kernel → RiskReport (seed, backend, timings, version)
              </p>
            </div>
          </div>

          <div className="glass p-5 sm:p-7">
            <p className="text-sm font-semibold text-ink">Verified against the C++ reference</p>
            <p className="mt-1.5 text-xs leading-5 text-ink/50">Each original C++ engine was frozen. Golden outputs must be reproduced exactly, and every parity test is mutation-checked.</p>
            <ul className="mt-5 divide-y divide-ink/[0.06]">
              {parity.map(([area, scope, result]) => (
                <li key={area} className="flex gap-3 py-2.5">
                  <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-profit" />
                  <div className="min-w-0">
                    <p className="text-[13px] text-ink/90">
                      {area} <span className="font-mono text-[11px] text-ink/45">· {scope}</span>
                    </p>
                    <p className="text-[11.5px] text-ink/50">{result}</p>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
    </section>
  );
}
