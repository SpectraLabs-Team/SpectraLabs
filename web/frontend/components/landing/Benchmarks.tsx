'use client';

import Link from 'next/link';
import { useMemo, useState } from 'react';
import { Bar, BarChart, CartesianGrid, LabelList, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { ArrowRight, Play } from 'lucide-react';
import { runBenchmark } from '@/lib/api';
import { BASELINE, HEADLINE, pps } from '@/lib/baseline';
import { SAMPLE_CSV } from '@/lib/portfolio';
import { useAuth } from '@/lib/auth';
import { chartColors, useTheme } from '@/lib/theme';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn, formatCompact, formatDuration } from '@/lib/utils';
import { Segmented, Spinner } from '@/components/ui/primitives';

type Metric = 'throughput' | 'time' | 'memory';
const tab = (short: string, long: string) => (
  <>
    <span className="sm:hidden">{short}</span>
    <span className="hidden sm:inline">{long}</span>
  </>
);
const TABS: { value: Metric; label: React.ReactNode }[] = [
  { value: 'throughput', label: tab('Throughput', 'Throughput (paths/sec)') },
  { value: 'time', label: tab('Time', 'Execution Time (ms)') },
  { value: 'memory', label: tab('Memory', 'Memory Usage (MB)') },
];
const SUB: Record<Metric, string> = {
  throughput: 'Simulated paths per second (higher is better)',
  time: 'Median wall time per 99% VaR run, log scale (lower is better)',
  memory: 'Whole-process peak RSS per run (lower is better)',
};

const fmt = (m: Metric, v: number) => (m === 'throughput' ? formatCompact(v) : m === 'time' ? (v < 1 ? `${(v * 1000).toFixed(0)}µs` : `${v.toFixed(v < 10 ? 2 : 1)}ms`) : `${v.toFixed(1)}MB`);

export function Benchmarks() {
  const cc = chartColors(useTheme().theme);
  const engine = useEngineHealth();
  const { isAuthenticated, openAuthModal } = useAuth();
  const [metric, setMetric] = useState<Metric>('throughput');
  const [live, setLive] = useState<Record<number, number> | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState('');

  const data = useMemo(
    () =>
      BASELINE.rows.map((r) => {
        const rustMs = live?.[r.paths] ?? r.rust_ms;
        const v = (ms: number, mb: number) => (metric === 'throughput' ? pps(r.paths, ms) : metric === 'time' ? ms : mb);
        return { label: formatCompact(r.paths, 0).toUpperCase(), rust: v(rustMs, r.rust_mem_mb), numpy: v(r.numpy_ms, r.numpy_mem_mb) };
      }),
    [metric, live],
  );

  const measureLive = async () => {
    setRunning(true);
    setError('');
    try {
      const out: Record<number, number> = {};
      for (const r of BASELINE.rows) {
        const reps = r.paths >= 1_000_000 ? 2 : 3; // stays inside the API's 10M total-path budget
        const b = await runBenchmark({ portfolio_csv: SAMPLE_CSV, simulations: r.paths, repetitions: reps, warmups: 1 });
        out[r.paths] = Math.min(...b.benchmarks.filter((x) => x.status === 'available').map((x) => x.latency_ms.p50));
        setLive({ ...out });
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Benchmark failed');
    } finally {
      setRunning(false);
    }
  };

  // Bars must start at zero to be honest; only execution time (two orders of magnitude) uses a
  // log axis, anchored to the decade below the smallest value.
  const log = metric === 'time';
  const minV = Math.min(...data.flatMap((d) => [d.rust, d.numpy]));
  const logFloor = 10 ** Math.floor(Math.log10(minV));
  const rustLabel = live ? 'Spectra Labs (Rust) · your engine, live' : 'Spectra Labs (Rust)';

  return (
    <section id="benchmarks" className="py-16">
      <div className="mx-auto grid max-w-[1320px] gap-10 px-5 sm:px-8 lg:grid-cols-[0.42fr_1fr] lg:items-center">
        <div className="min-w-0">
          <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Benchmarking</p>
          <h2 className="mt-3 text-3xl font-bold leading-tight tracking-tight text-ink sm:text-[2.15rem]">
            Real Performance.
            <br />
            Real Results.
          </h2>
          <p className="mt-4 text-sm leading-6 text-ink/65">
            Same VaR kernel, same machine: <span className="text-ink">{HEADLINE.speedupVsNumpy.toFixed(1)}× faster</span> than vectorised NumPy at {formatCompact(HEADLINE.largestPaths, 0)} paths, using{' '}
            <span className="text-ink">{HEADLINE.memoryRatio.toFixed(1)}× less memory</span>. A 10k-path VaR runs in {HEADLINE.smallMs ? formatDuration(HEADLINE.smallMs) : '—'}.
          </p>
          <p className="mt-3 text-xs leading-5 text-ink/45">NumPy runs compiled C for this kernel, so the gap is honest rather than dramatic. Parallel speed-up grows with core count.</p>
          <Link href="/studio?tab=benchmark" className="btn-secondary mt-7 border-accent/40 text-accent hover:border-accent">
            View Full Benchmarks <ArrowRight className="h-4 w-4" />
          </Link>
        </div>

        <div className="glass min-w-0 p-5 sm:p-6">
          <div className="flex flex-col justify-between gap-4 xl:flex-row xl:items-start">
            <div>
              <h3 className="text-sm font-semibold text-ink">Monte Carlo Throughput Comparison</h3>
              <p className="mt-0.5 text-xs text-ink/50">{SUB[metric]}</p>
            </div>
            <Segmented label="bench-metric" value={metric} onChange={setMetric} options={TABS} size="sm" className="self-start" />
          </div>
          <div className="mt-4 flex flex-wrap items-center gap-x-5 gap-y-1 text-[12px] text-ink/70">
            <span className="flex items-center gap-1.5">
              <i className="h-2.5 w-2.5 rounded-sm" style={{ background: cc.electric }} />
              {rustLabel}
            </span>
            <span className="flex items-center gap-1.5">
              <i className="h-2.5 w-2.5 rounded-sm" style={{ background: cc.numpy }} />
              Python (NumPy {BASELINE.host.numpy})
            </span>
          </div>
          <div className="mt-4 h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={data} margin={{ top: 24, right: 8, bottom: 4, left: 0 }} barGap={6} barCategoryGap="22%">
                <defs>
                  <linearGradient id="bm-rust" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0" stopColor={cc.electric} stopOpacity={1} />
                    <stop offset="1" stopColor={cc.accent} stopOpacity={0.55} />
                  </linearGradient>
                  <linearGradient id="bm-np" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0" stopColor={cc.numpy} stopOpacity={0.95} />
                    <stop offset="1" stopColor={cc.numpy} stopOpacity={0.45} />
                  </linearGradient>
                </defs>
                <CartesianGrid stroke={cc.grid} vertical={false} />
                <XAxis dataKey="label" tick={{ fill: cc.tick, fontSize: 12 }} tickLine={false} axisLine={{ stroke: cc.axis }} />
                <YAxis
                  scale={log ? 'log' : 'linear'}
                  domain={log ? [logFloor, 'auto'] : [0, 'auto']}
                  allowDataOverflow
                  tick={{ fill: cc.tick, fontSize: 11 }}
                  tickLine={false}
                  axisLine={false}
                  width={56}
                  tickFormatter={(v: number) => fmt(metric, v)}
                />
                <Tooltip cursor={{ fill: 'rgba(127,127,127,0.06)' }} contentStyle={cc.tooltip} formatter={(v, n) => [fmt(metric, Number(v)), n === 'rust' ? rustLabel : 'NumPy']} labelFormatter={(l) => `${l} paths`} />
                <Bar dataKey="rust" fill="url(#bm-rust)" radius={[5, 5, 0, 0]} animationDuration={700}>
                  <LabelList dataKey="rust" position="top" formatter={(v: unknown) => fmt(metric, Number(v))} style={{ fill: cc.accent, fontSize: 11, fontWeight: 600 }} />
                </Bar>
                <Bar dataKey="numpy" fill="url(#bm-np)" radius={[5, 5, 0, 0]} animationDuration={700}>
                  <LabelList dataKey="numpy" position="top" formatter={(v: unknown) => fmt(metric, Number(v))} style={{ fill: cc.label, fontSize: 11 }} />
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
          <div className="mt-3 flex flex-wrap items-center justify-between gap-3 border-t border-ink/[0.06] pt-3">
            <p className="text-[11px] leading-4 text-ink/45">
              Reference run · {BASELINE.host.cpu}, {BASELINE.host.threads} threads · {BASELINE.measured} · <code className="font-mono">web/bench/bench_baseline.py</code>
              {live && <span className="text-caution"> · Rust bars now from your engine; NumPy stays the reference run</span>}
              {error && <span className="text-loss"> · {error}</span>}
            </p>
            {isAuthenticated ? (
              <button type="button" onClick={measureLive} disabled={engine.status !== 'online' || running || metric === 'memory'} className={cn('btn-ghost border border-ink/10', metric === 'memory' && 'hidden')} title={engine.status !== 'online' ? 'Start the engine to measure live' : undefined}>
                {running ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />} Measure Rust live
              </button>
            ) : (
              // The engine is behind sign-in; offer the account instead of a button that would 401.
              <button type="button" onClick={() => openAuthModal('signup')} className={cn('btn-ghost border border-ink/10', metric === 'memory' && 'hidden')}>
                <Play className="h-3.5 w-3.5" /> Sign up to measure live
              </button>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
