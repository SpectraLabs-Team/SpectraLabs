'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useInView } from 'framer-motion';
import { Bar, BarChart, Cell, LabelList, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { Globe, Play, RotateCcw, Server } from 'lucide-react';
import { benchmarkFits, runBenchmark } from '@/lib/api';
import { benchmarkJs } from '@/lib/jsKernel';
import { SAMPLE_CSV, parseCsv, portfolioStats } from '@/lib/portfolio';
import { useEngineHealth } from '@/lib/useEngineHealth';
import type { BenchmarkResult } from '@/lib/types';
import { formatCompact, formatDuration } from '@/lib/utils';
import { Badge, GlassCard, Segmented, Spinner } from '@/components/ui/primitives';

const STATS = portfolioStats(parseCsv(SAMPLE_CSV).positions);
const SIZES = [
  { value: 100_000, label: '100k' },
  { value: 250_000, label: '250k' },
  { value: 500_000, label: '500k' },
];
const REPS = 3;
const WARMUPS = 1;

type Row = { name: string; detail: string; pps: number; p50: number; color: string };

export function BenchmarkShowcase() {
  const ref = useRef<HTMLDivElement>(null);
  const inView = useInView(ref, { once: true, margin: '-120px' });
  const engine = useEngineHealth();
  const [paths, setPaths] = useState(100_000);
  const [js, setJs] = useState<{ pps: number; p50: number } | null>(null);
  const [rust, setRust] = useState<BenchmarkResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState('');

  const online = engine.status === 'online';

  const run = useCallback(async () => {
    setRunning(true);
    setError('');
    try {
      const jsRes = await benchmarkJs(paths, STATS.netValue, STATS.modelVolatility, REPS, WARMUPS);
      setJs({ pps: jsRes.pathsPerSecond, p50: jsRes.p50 });
      if (online && benchmarkFits(paths, REPS, WARMUPS)) {
        setRust(await runBenchmark({ portfolio_csv: SAMPLE_CSV, simulations: paths, repetitions: REPS, warmups: WARMUPS }));
      } else {
        setRust(null);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Benchmark failed');
    } finally {
      setRunning(false);
    }
  }, [paths, online]);

  // Run once when the section scrolls into view (after the health check settles).
  const started = useRef(false);
  useEffect(() => {
    if (inView && engine.status !== 'checking' && !started.current) {
      started.current = true;
      run();
    }
  }, [inView, engine.status, run]);

  const rows: Row[] = [];
  if (js) rows.push({ name: 'Browser JS', detail: 'this tab, 1 thread', pps: js.pps, p50: js.p50, color: '#64748B' });
  for (const b of rust?.benchmarks ?? []) {
    if (b.status !== 'available') continue;
    rows.push({
      name: `Rust ${b.backend}`,
      detail: b.backend === 'CPU' ? 'reference kernel, 1 thread' : 'deterministic chunks, all cores',
      pps: b.paths_per_second,
      p50: b.latency_ms.p50,
      color: b.backend === 'CPU' ? '#A78BFA' : '#00F2FE',
    });
  }
  const best = rows.reduce((m, r) => Math.max(m, r.pps), 0);
  const speedup = js && best > js.pps ? best / js.pps : null;

  return (
    <section id="benchmarks" className="relative py-24 sm:py-28">
      <div className="bg-dot-grid mask-fade-b pointer-events-none absolute inset-0 opacity-60" />
      <div ref={ref} className="relative mx-auto grid max-w-7xl gap-10 px-5 sm:px-8 lg:grid-cols-[0.8fr_1.2fr] lg:items-center">
        <div>
          <p className="eyebrow">Benchmarks</p>
          <h2 className="mt-3 text-3xl font-semibold tracking-tight text-white sm:text-4xl">Measured live. Not quoted.</h2>
          <p className="mt-4 leading-7 text-slate-400">
            The same GBM terminal-loss kernel runs in your browser and on the engine's backends: draw a normal, price the path, sort, take the 99% quantile. Every bar on the right is timed while you watch.
          </p>
          <ul className="mt-6 space-y-3 text-sm text-slate-400">
            <li className="flex gap-3">
              <Globe className="mt-0.5 h-4 w-4 shrink-0 text-slate-500" />
              <span>Browser: the median of {REPS} runs after {WARMUPS} warm-up, using a JavaScript port of the kernel.</span>
            </li>
            <li className="flex gap-3">
              <Server className="mt-0.5 h-4 w-4 shrink-0 text-accent" />
              <span>
                Engine: <code className="font-mono text-[12px] text-slate-300">POST /api/benchmark</code>, the engine&apos;s median wall time (HTTP excluded).
              </span>
            </li>
          </ul>
          {speedup && (
            <p className="mt-8 text-sm text-slate-400">
              <span className="num text-5xl font-semibold tracking-tight text-gradient">{speedup.toFixed(1)}×</span>
              <span className="ml-2">faster than your browser on this run</span>
            </p>
          )}
        </div>

        <GlassCard className="p-5 sm:p-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h3 className="text-sm font-semibold text-white">Throughput · paths per second</h3>
              <p className="mt-0.5 font-mono text-[11px] text-slate-500">sample book · 1-day VaR 99% · higher is better</p>
            </div>
            <div className="flex items-center gap-2">
              <Segmented label="bench-size" value={paths} onChange={setPaths} options={SIZES} size="sm" />
              <button type="button" onClick={run} disabled={running} className="btn-secondary px-3 py-1.5 text-xs">
                {running ? <Spinner className="h-3.5 w-3.5" /> : js ? <RotateCcw className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
                {running ? 'Running' : js ? 'Re-run' : 'Run'}
              </button>
            </div>
          </div>

          <div className="mt-6 h-[220px]">
            {rows.length ? (
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={rows} layout="vertical" margin={{ top: 0, right: 64, bottom: 0, left: 0 }} barCategoryGap={18}>
                  <XAxis type="number" hide domain={[0, 'dataMax']} />
                  <YAxis type="category" dataKey="name" width={110} tickLine={false} axisLine={false} tick={{ fill: '#CBD5E1', fontSize: 12 }} />
                  <Tooltip
                    cursor={{ fill: 'rgba(255,255,255,0.03)' }}
                    contentStyle={{ background: '#0B0F17', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 10, fontSize: 12 }}
                    formatter={(v: number, _n, item) => [`${formatCompact(v)} paths/s · p50 ${formatDuration((item.payload as Row).p50)}`, (item.payload as Row).detail]}
                  />
                  <Bar dataKey="pps" radius={[0, 6, 6, 0]} animationDuration={900}>
                    {rows.map((r) => (
                      <Cell key={r.name} fill={r.color} />
                    ))}
                    <LabelList dataKey="pps" position="right" formatter={(v: number) => formatCompact(v)} className="fill-slate-200 font-mono text-[12px]" />
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center gap-2 text-sm text-slate-500">{running ? <><Spinner /> Timing kernels…</> : 'Scroll here or press Run to measure.'}</div>
            )}
          </div>

          <div className="mt-4 flex flex-wrap items-center gap-2 border-t border-white/[0.06] pt-4 text-[11px] text-slate-500">
            {!online && engine.status !== 'checking' && <Badge tone="caution">engine offline · browser only</Badge>}
            {rust?.benchmarks.some((b) => b.status === 'fallback') && <Badge>CUDA not built: fallback runs excluded</Badge>}
            {error && <Badge tone="loss">{error}</Badge>}
            <span className="ml-auto font-mono">{paths.toLocaleString()} paths × {REPS} runs</span>
          </div>
        </GlassCard>
      </div>
    </section>
  );
}
