'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { Boxes, CheckCircle2, ShieldAlert, Terminal } from 'lucide-react';
import { cn } from '@/lib/utils';

type Crate = { name: string; role: string; hi?: boolean };

const layers: { label: string; crates: Crate[] }[] = [
  {
    label: 'Interfaces & Services',
    crates: [
      { name: 'web/frontend', role: 'Next.js 15 studio · React 19 · Tailwind', hi: true },
      { name: 'spectra-api', role: 'Axum 0.8 · Tokio async REST & SSE', hi: true },
      { name: 'spectra-worker', role: 'Distributed job runner · heartbeats', hi: true },
      { name: 'spectra-cli', role: 'Ingest · audit verification · batch risk' },
    ],
  },
  {
    label: 'Quant Engines (std-only)',
    crates: [
      { name: 'spectra-risk', role: 'Copula VaR/CVaR · GARCH · EVT · HMM', hi: true },
      { name: 'spectra-engine', role: 'Event-driven backtester · impact & fills', hi: true },
      { name: 'spectra-research', role: 'Sweeps · PBO · DSR · Survival Lab', hi: true },
      { name: 'spectra-backtest', role: 'Point-in-time universe · corporate actions' },
    ],
  },
  {
    label: 'Foundation & Storage',
    crates: [
      { name: 'spectra-db', role: 'PostgreSQL · SQLx · RBAC · audit log', hi: true },
      { name: 'spectra-parquet', role: 'Columnar storage · zero-copy Arrow' },
      { name: 'spectra-data', role: 'CSV & PIT ingest · content-addressed store' },
      { name: 'spectra-registry', role: 'Hash-chained event log · holdout vault' },
      { name: 'spectra-core', role: 'Fixed-point ledger · JSON · SHA-256 · RNG', hi: true },
    ],
  },
];

const parity = [
  ['Ledger', '12 scripts · 4,800 operations', 'Byte-identical balance journals & position tracking against C++ reference'],
  ['Market Data', '31 CSV cases · PIT bars', 'Byte-identical bars, calendar rules & binary format metadata'],
  ['Backtester', '39 golden runs', 'Identical trade execution, slippage & portfolio equity curves'],
  ['Research & Sweeps', '565-line canonical transcript', 'Bit-exact trials, PBO, DSR, CSCV & walk-forward results'],
  ['Registry Vault', '140-event hash chain', 'Cross-verification between C++ and Rust engines with zero divergence'],
  ['Risk Engine', 'Monte Carlo & copulas', 'Bit-identical paths, copula losses & VaR/CVaR percentiles on Linux (glibc)'],
];

export function Architecture() {
  const reduce = useReducedMotion();
  return (
    <section aria-labelledby="architecture-title">
      <div>
        <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Architecture</p>
        <h1 id="architecture-title" className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.4rem]">Modular Architecture. Standard-Library Core Engines.</h1>
        <p className="mt-3 max-w-2xl text-sm leading-6 text-ink/60">
          Strict architectural boundary between quantitative execution and platform orchestration. The compute-critical engines (<code className="font-mono text-ink/80">spectra-core</code>, <code className="font-mono text-ink/80">spectra-engine</code>, <code className="font-mono text-ink/80">spectra-risk</code>, <code className="font-mono text-ink/80">spectra-research</code>) are built strictly with the Rust standard library (<code className="font-mono text-ink/80">unsafe_code = &quot;forbid&quot;</code>) and zero external runtime crates, guaranteeing bit-exact mathematical parity with frozen C++ reference golden files. Production services leverage asynchronous <code className="font-mono text-ink/80">Axum 0.8</code>, <code className="font-mono text-ink/80">Tokio</code>, and <code className="font-mono text-ink/80">PostgreSQL</code> for scalable job dispatch, streaming data uploads, and enterprise security.
        </p>

        <div className="mt-10 grid gap-5 lg:grid-cols-[1.3fr_0.7fr]">
          <div className="glass p-5 sm:p-7">
            <div className="flex items-center gap-2 text-sm font-semibold text-ink">
              <Boxes className="h-4 w-4 text-accent" /> Workspace map
            </div>
            <div className="mt-6 space-y-4">
              {layers.map((layer, li) => (
                <motion.div key={layer.label} initial={reduce ? false : { opacity: 0, x: -12 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: li * 0.1, duration: 0.45 }} className="grid gap-3 sm:grid-cols-[110px_1fr] sm:items-center">
                  <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink/45">{layer.label}</span>
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
                <Terminal className="h-3.5 w-3.5" /> asynchronous job &amp; execution pipeline
              </div>
              <p className="text-ink/60">
                <span className="text-accent">studio (Next.js)</span> → POST /api/engine/backtests → <span className="text-accent">spectra-api (Axum 0.8)</span> → PostgreSQL job queue (<span className="text-accent">spectra-db</span>) → <span className="text-accent">spectra-worker</span> claims job → <span className="text-accent">BacktestEngine</span> (std-only) → streaming progress heartbeats → artifact store &amp; hash-chained registry
              </p>
            </div>
          </div>

          <div className="glass p-5 sm:p-7">
            <p className="text-sm font-semibold text-ink">Verified Against Frozen C++ Golden Files</p>
            <p className="mt-1.5 text-xs leading-5 text-ink/50">
              Reference C++ quant engines were frozen, and golden output generators (<code className="font-mono text-ink/70">parity/cpp/*.cpp</code>) produced canonical golden baselines on Linux (<code className="font-mono text-ink/70">glibc/fdlibm</code>). Rust test suites verify byte-for-byte and bit-for-bit identical outputs for every ledger entry, data parse, simulation path, and research metric. Mutation tests ensure that any arithmetic or rounding drift triggers immediate test failures.
            </p>
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
            <div className="mt-4 flex items-start gap-2 rounded-lg border border-ink/[0.08] bg-canvas/60 p-2.5 text-[11px] leading-4 text-ink/50">
              <ShieldAlert className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
              <span>Bit-exact floating-point parity relies on glibc mathematical primitives (<code className="font-mono text-ink/70">exp</code>, <code className="font-mono text-ink/70">log</code>, <code className="font-mono text-ink/70">pow</code>) matching the C++ compiler target.</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
