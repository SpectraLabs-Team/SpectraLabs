'use client';

import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { CheckCircle2, Cpu, Gauge, MemoryStick, Play, Server, Zap } from 'lucide-react';
import { BENCHMARK_PATH_BUDGET, getCpuTelemetry, runBenchmark } from '@/lib/api';
import type { BenchmarkResult, CpuTelemetry } from '@/lib/types';
import { cn, formatBytes, formatCompact, formatDuration } from '@/lib/utils';
import { Badge, ErrorNote, GlassCard, Segmented, Spinner } from '@/components/ui/primitives';
import { usePortfolio } from './PortfolioContext';

const SIZES = [10_000, 50_000, 100_000, 250_000, 500_000, 1_000_000];
const WARMUPS = 1;
/** Peak loss buffers per path: kernel output + collected losses + sorted copy for the quantile, 8 bytes each. */
const BYTES_PER_PATH = 24;
const COLORS = { CPU: '#A78BFA', Parallel: '#00F2FE' } as const;

/** As many repetitions as fit the API's 10M total-path budget, at most 5. */
const repsFor = (n: number) => Math.max(1, Math.min(5, Math.floor(BENCHMARK_PATH_BUDGET / (3 * n)) - WARMUPS));

function niceCeil(v: number) {
  const p = 10 ** Math.floor(Math.log10(v));
  return Math.ceil(v / p) * p;
}

function ThroughputGauge({ label, value, max, color, detail }: { label: string; value: number | null; max: number; color: string; detail: string }) {
  const frac = value ? Math.min(1, value / max) : 0;
  const r = 70;
  const len = Math.PI * r;
  return (
    <div className="flex flex-col items-center">
      <svg viewBox="0 0 180 104" className="w-full max-w-[220px]" role="img" aria-label={`${label}: ${value ? formatCompact(value) : 'no data'} paths per second`}>
        <path d="M20 94 A70 70 0 0 1 160 94" fill="none" stroke="rgba(255,255,255,0.07)" strokeWidth="12" strokeLinecap="round" />
        <motion.path
          d="M20 94 A70 70 0 0 1 160 94"
          fill="none"
          stroke={color}
          strokeWidth="12"
          strokeLinecap="round"
          strokeDasharray={len}
          initial={{ strokeDashoffset: len }}
          animate={{ strokeDashoffset: len * (1 - frac) }}
          transition={{ duration: 1.1, ease: [0.22, 1, 0.36, 1] }}
          style={{ filter: `drop-shadow(0 0 6px ${color}88)` }}
        />
        <text x="90" y="78" textAnchor="middle" className="fill-white font-mono text-[22px] font-semibold">
          {value ? formatCompact(value) : '—'}
        </text>
        <text x="90" y="96" textAnchor="middle" className="fill-slate-500 font-mono text-[9px]">
          paths / sec
        </text>
      </svg>
      <p className="mt-1 text-sm font-medium text-slate-200">{label}</p>
      <p className="text-[11px] text-slate-500">{detail}</p>
    </div>
  );
}

export function BenchmarkLab() {
  const { csv, engine, positions } = usePortfolio();
  const online = engine.status === 'online';
  const [host, setHost] = useState<CpuTelemetry | null>(null);
  const [results, setResults] = useState<Record<number, BenchmarkResult>>({});
  const [current, setCurrent] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [scale, setScale] = useState<'linear' | 'log'>('log');

  useEffect(() => {
    if (online) getCpuTelemetry().then(setHost).catch(() => setHost(null));
  }, [online]);

  const sweep = async () => {
    setError('');
    setResults({});
    for (const n of SIZES) {
      setCurrent(n);
      try {
        const r = await runBenchmark({ portfolio_csv: csv, simulations: n, repetitions: repsFor(n), warmups: WARMUPS });
        setResults((s) => ({ ...s, [n]: r }));
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Benchmark failed');
        break;
      }
    }
    setCurrent(null);
  };

  const done = SIZES.filter((n) => results[n]);
  const series = useMemo(
    () =>
      SIZES.filter((n) => results[n]).map((n) => {
        const b = results[n].benchmarks;
        const cpu = b.find((x) => x.requested_backend === 'CPU');
        const par = b.find((x) => x.requested_backend === 'Parallel');
        return { paths: n, label: formatCompact(n, 0), CPU: cpu?.latency_ms.p50 ?? null, Parallel: par?.latency_ms.p50 ?? null, cpuPps: cpu?.paths_per_second ?? 0, parPps: par?.paths_per_second ?? 0 };
      }),
    [results],
  );
  const largest = done.length ? results[done[done.length - 1]] : null;
  const last = series[series.length - 1];
  const gaugeMax = niceCeil(Math.max(1, ...series.map((s) => Math.max(s.cpuPps, s.parPps))) * 1.15);
  const running = current !== null;

  return (
    <div className="space-y-5">
      <div className="grid gap-5 lg:grid-cols-[1fr_340px]">
        <GlassCard as="section" className="p-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="text-sm font-semibold text-white">Latency vs path count</h2>
              <p className="mt-0.5 text-xs text-slate-500">median engine wall time per VaR run · {positions.length}-position book · lower is better</p>
            </div>
            <div className="flex items-center gap-2">
              <Segmented label="bench-scale" value={scale} onChange={setScale} size="sm" options={[{ value: 'log', label: 'log' }, { value: 'linear', label: 'linear' }]} />
              <button type="button" onClick={sweep} disabled={!online || running} className="btn-primary px-3 py-1.5 text-xs">
                {running ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
                {running ? `Running ${formatCompact(current!, 0)}…` : done.length ? 'Re-run sweep' : 'Run sweep'}
              </button>
            </div>
          </div>

          <div className="mt-4 flex gap-1.5">
            {SIZES.map((n) => (
              <div key={n} className="flex-1">
                <div className={cn('h-1 rounded-full transition-colors', results[n] ? 'bg-accent' : current === n ? 'animate-pulse bg-accent/50' : 'bg-white/[0.07]')} />
                <p className="num mt-1 text-center text-[10px] text-slate-600">{formatCompact(n, 0)}</p>
              </div>
            ))}
          </div>

          <div className="mt-4 h-[300px]">
            {series.length ? (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={series} margin={{ top: 8, right: 12, bottom: 0, left: 0 }}>
                  <CartesianGrid stroke="rgba(255,255,255,0.05)" vertical={false} />
                  <XAxis dataKey="label" tick={{ fill: '#64748B', fontSize: 11 }} tickLine={false} axisLine={{ stroke: 'rgba(255,255,255,0.08)' }} />
                  <YAxis scale={scale} domain={scale === 'log' ? ['auto', 'auto'] : [0, 'auto']} allowDataOverflow tickFormatter={(v: number) => (v < 1 ? `${(v * 1000).toFixed(0)}µs` : `${v.toFixed(v < 10 ? 1 : 0)}ms`)} tick={{ fill: '#64748B', fontSize: 11 }} tickLine={false} axisLine={false} width={52} />
                  <Tooltip contentStyle={{ background: '#0B0F17', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 10, fontSize: 12 }} formatter={(v: number, name) => [formatDuration(v), name]} labelFormatter={(l) => `${l} paths`} />
                  <Legend iconType="circle" wrapperStyle={{ fontSize: 12, color: '#94A3B8' }} />
                  <Line type="monotone" dataKey="CPU" name="CPU · 1 thread" stroke={COLORS.CPU} strokeWidth={2} strokeDasharray="5 4" dot={{ r: 3 }} isAnimationActive={false} />
                  <Line type="monotone" dataKey="Parallel" name="Parallel · all cores" stroke={COLORS.Parallel} strokeWidth={2} dot={{ r: 3 }} isAnimationActive={false} style={{ filter: 'drop-shadow(0 0 4px #00F2FE88)' }} />
                </LineChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-slate-500">
                {running ? (
                  <>
                    <Spinner /> Timing the first size…
                  </>
                ) : online ? (
                  <>Run a sweep from 10k to 1M paths. Each size times CPU, Parallel and the CUDA request (a fallback in this build).</>
                ) : (
                  <>Start the engine to benchmark your hardware.</>
                )}
              </div>
            )}
          </div>
          {error && <ErrorNote>{error}</ErrorNote>}
        </GlassCard>

        <GlassCard as="section" className="p-5">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-white">
            <Server className="h-4 w-4 text-accent" /> Host
          </h2>
          {host ? (
            <dl className="mt-4 space-y-3 text-sm">
              {[
                ['Logical threads', host.cpu_threads],
                ['Physical cores', host.cpu_cores],
                ['Memory', `${host.memory_used_gb.toFixed(1)} / ${host.memory_total_gb.toFixed(1)} GB`],
                ['CPU load', `${host.cpu_utilization.toFixed(1)}%`],
              ].map(([k, v]) => (
                <div key={String(k)} className="flex justify-between gap-3">
                  <dt className="text-slate-500">{k}</dt>
                  <dd className="num text-slate-200">{v}</dd>
                </div>
              ))}
            </dl>
          ) : (
            <p className="mt-4 text-sm text-slate-500">{online ? 'Reading /api/telemetry/cpu…' : 'Host telemetry needs the engine.'}</p>
          )}
          <div className="mt-5 rounded-lg border border-white/[0.07] bg-obsidian-950/60 p-3 text-[11.5px] leading-5 text-slate-500">
            <p>
              <span className="text-slate-300">Parallel</span> splits paths into fixed seeded chunks, so its results are identical on any thread count. On small runs, thread start-up can make it slower than CPU, which is why <span className="font-mono">auto</span> picks CPU there.
            </p>
          </div>
        </GlassCard>
      </div>

      <div className="grid gap-5 lg:grid-cols-[1.1fr_0.9fr]">
        <GlassCard as="section" className="p-5">
          <div className="flex items-center justify-between">
            <h2 className="flex items-center gap-2 text-sm font-semibold text-white">
              <Gauge className="h-4 w-4 text-accent" /> Throughput
            </h2>
            {last && <span className="num text-[11px] text-slate-500">at {last.label} paths</span>}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-4">
            <ThroughputGauge label="CPU" value={last?.cpuPps ?? null} max={gaugeMax} color={COLORS.CPU} detail="single-threaded reference" />
            <ThroughputGauge label="Parallel" value={last?.parPps ?? null} max={gaugeMax} color={COLORS.Parallel} detail={host ? `${host.cpu_threads} threads` : 'all cores'} />
          </div>
          {series.length > 0 && (
            <div className="mt-5 overflow-x-auto">
              <table className="w-full min-w-[420px] text-[12px]">
                <thead>
                  <tr className="text-[10px] uppercase tracking-wider text-slate-600">
                    <th className="py-1.5 text-left font-medium">Paths</th>
                    <th className="py-1.5 text-right font-medium">CPU p50</th>
                    <th className="py-1.5 text-right font-medium">Parallel p50</th>
                    <th className="py-1.5 text-right font-medium">Speed-up</th>
                  </tr>
                </thead>
                <tbody className="num">
                  {series.map((s) => {
                    const up = s.CPU && s.Parallel ? s.CPU / s.Parallel : null;
                    return (
                      <tr key={s.paths} className="border-t border-white/[0.04]">
                        <td className="py-1.5 text-slate-300">{s.paths.toLocaleString()}</td>
                        <td className="py-1.5 text-right text-slate-400">{s.CPU !== null ? formatDuration(s.CPU) : '—'}</td>
                        <td className="py-1.5 text-right text-slate-400">{s.Parallel !== null ? formatDuration(s.Parallel) : '—'}</td>
                        <td className={cn('py-1.5 text-right', up && up >= 1 ? 'text-profit' : 'text-caution')}>{up ? `${up.toFixed(2)}×` : '—'}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </GlassCard>

        <GlassCard as="section" className="p-5">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-white">
            <MemoryStick className="h-4 w-4 text-accent" /> Memory footprint
          </h2>
          <p className="mt-1 text-[11.5px] leading-5 text-slate-500">
            Estimated peak loss buffers: {BYTES_PER_PATH} B/path (kernel output, collected losses and the sorted copy for the quantile, 8 bytes each). Derived from the engine code, not measured.
          </p>
          <ul className="mt-4 space-y-2.5">
            {SIZES.map((n) => {
              const bytes = n * BYTES_PER_PATH;
              const frac = bytes / (SIZES[SIZES.length - 1] * BYTES_PER_PATH);
              return (
                <li key={n} className="grid grid-cols-[52px_1fr_64px] items-center gap-3 text-[12px]">
                  <span className="num text-slate-400">{formatCompact(n, 0)}</span>
                  <div className="h-1.5 overflow-hidden rounded-full bg-white/5">
                    <motion.div className="h-full rounded-full bg-gradient-to-r from-accent-ice to-cyan-electric" initial={{ width: 0 }} animate={{ width: `${Math.max(2, frac * 100)}%` }} transition={{ duration: 0.6 }} />
                  </div>
                  <span className="num text-right text-slate-300">{formatBytes(bytes)}</span>
                </li>
              );
            })}
          </ul>
          {host && (
            <p className="num mt-4 text-[11px] text-slate-500">
              1M paths ≈ {formatBytes(1_000_000 * BYTES_PER_PATH)} = {((1_000_000 * BYTES_PER_PATH) / (host.memory_total_gb * 1024 ** 3) * 100).toFixed(2)}% of host RAM
            </p>
          )}
        </GlassCard>
      </div>

      {largest && (
        <GlassCard as="section" className="overflow-hidden">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] px-5 py-4">
            <div>
              <h2 className="text-sm font-semibold text-white">Backend detail · {largest.simulations.toLocaleString()} paths</h2>
              <p className="mt-0.5 text-xs text-slate-500">
                {largest.requested_repetitions} timed runs after {largest.warmups} warm-up · requested backend → backend that ran
              </p>
            </div>
            <Zap className="h-4 w-4 text-slate-600" />
          </div>
          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] text-sm">
              <thead>
                <tr className="text-left text-[10.5px] uppercase tracking-wider text-slate-500">
                  <th className="px-5 py-2.5 font-medium">Backend</th>
                  {['min', 'p50', 'p95', 'p99', 'max', 'compute p50', 'paths/s'].map((h) => (
                    <th key={h} className="px-3 py-2.5 text-right font-medium">
                      {h}
                    </th>
                  ))}
                  <th className="px-5 py-2.5 text-right font-medium">Status</th>
                </tr>
              </thead>
              <tbody className="num text-[12.5px]">
                {largest.benchmarks.map((b) => (
                  <tr key={b.requested_backend} className={cn('border-t border-white/[0.04]', b.status === 'fallback' && 'text-slate-500')}>
                    <td className="px-5 py-2.5 font-sans text-slate-200">
                      <span className="flex items-center gap-2">
                        <Cpu className="h-3.5 w-3.5 text-slate-500" />
                        {b.requested_backend}
                        {b.backend !== b.requested_backend && <span className="text-slate-500">→ {b.backend}</span>}
                      </span>
                    </td>
                    {[b.latency_ms.min, b.latency_ms.p50, b.latency_ms.p95, b.latency_ms.p99, b.latency_ms.max, b.compute_ms.p50].map((v, i) => (
                      <td key={i} className="px-3 py-2.5 text-right">
                        {formatDuration(v)}
                      </td>
                    ))}
                    <td className="px-3 py-2.5 text-right text-slate-200">{formatCompact(b.paths_per_second)}</td>
                    <td className="px-5 py-2.5 text-right">
                      {b.status === 'available' ? (
                        <Badge tone="profit">
                          <CheckCircle2 className="h-3 w-3" /> available
                        </Badge>
                      ) : (
                        <Badge tone="caution">fallback</Badge>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="border-t border-white/[0.06] px-5 py-3 text-[11px] text-slate-600">CUDA is not part of this build. Requests for it run on Parallel and are reported as a fallback, never as GPU results.</p>
        </GlassCard>
      )}
    </div>
  );
}
