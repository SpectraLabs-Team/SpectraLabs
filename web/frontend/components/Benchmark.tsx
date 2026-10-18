"use client";

import { useEffect, useMemo, useState } from 'react';
import { Activity, Gauge, Loader2, Play, RotateCcw, Timer } from 'lucide-react';
import { CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { getSamplePortfolio, runBenchmark } from '@/lib/api';
import type { BenchmarkResult } from '@/lib/types';

const colors: Record<string, string> = { CPU: '#60a5fa', Parallel: '#34d399', CUDA: '#c084fc' };

export default function Benchmark() {
  const [portfolio, setPortfolio] = useState('');
  const [simulations, setSimulations] = useState(100000);
  const [repetitions, setRepetitions] = useState(5);
  const [warmups, setWarmups] = useState(1);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<BenchmarkResult | null>(null);
  const [error, setError] = useState('');

  useEffect(() => { getSamplePortfolio().then(setPortfolio).catch((e: Error) => setError(e.message)); }, []);
  const comparable = result?.benchmarks.filter((row) => row.status === 'available' && row.backend === row.requested_backend) ?? [];
  const maxPaths = Math.floor(10_000_000 / (3 * Math.max(1, repetitions + warmups)));
  const chartData = useMemo(() => Array.from({ length: repetitions }, (_, index) => {
    const row: Record<string, number> = { trial: index + 1 };
    comparable.forEach((backend) => { if (backend.samples_ms[index] !== undefined) row[backend.requested_backend] = backend.samples_ms[index]; });
    return row;
  }), [comparable, repetitions]);

  const execute = async () => {
    setError(''); setResult(null); setLoading(true);
    if (simulations * (repetitions + warmups) * 3 > 10_000_000) { setError('Reduce the simulation paths or trial count to stay within the 10 million total-path budget.'); setLoading(false); return; }
    try { setResult(await runBenchmark(portfolio, simulations, repetitions, warmups)); }
    catch (e) { setError(e instanceof Error ? e.message : 'Benchmark could not be completed.'); }
    finally { setLoading(false); }
  };

  return <div className="mx-auto max-w-7xl space-y-7 p-5 sm:p-8">
    <header className="flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
      <div><p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-primary">Performance</p><h1 className="text-3xl font-semibold tracking-tight">Latency benchmark</h1><p className="mt-2 max-w-2xl text-sm leading-6 text-foreground-muted">Compare repeated VaR runs across the backends this build can actually execute. Warmups are excluded; fallback runs are called out and left out of direct comparisons.</p></div>
      <div className="flex items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2 text-xs text-foreground-muted"><Activity size={15} className="text-accent-success" /> Engine reported latency</div>
    </header>

    <section className="grid gap-5 xl:grid-cols-[0.72fr_1.28fr]">
      <div className="rounded-xl border border-border bg-surface p-5 sm:p-6">
        <div className="mb-5 flex items-start gap-3"><div className="rounded-lg bg-accent-primary-dim p-2 text-accent-primary"><Gauge size={18} /></div><div><h2 className="font-semibold">Benchmark setup</h2><p className="mt-1 text-xs text-foreground-muted">Same portfolio, path count and seed per backend.</p></div></div>
        <label className="mb-2 block text-sm font-medium" htmlFor="bench-portfolio">Portfolio CSV</label>
        <textarea id="bench-portfolio" value={portfolio} onChange={(e) => setPortfolio(e.target.value)} className="h-32 w-full resize-y rounded-lg border border-border bg-background p-3 font-mono text-xs outline-none focus:border-accent-primary" placeholder="symbol,quantity,price" />
        <button type="button" onClick={() => getSamplePortfolio().then(setPortfolio).catch((e: Error) => setError(e.message))} className="mt-2 text-xs text-accent-primary hover:underline">Reload sample portfolio</button>
        <div className="mt-5 grid grid-cols-2 gap-3">
          <label className="text-xs text-foreground-muted">Simulation paths<input type="number" min={1000} max={maxPaths} step={10000} value={simulations} onChange={(e) => setSimulations(Number(e.target.value))} className="mt-1.5 w-full rounded-lg border border-border bg-background px-3 py-2.5 text-sm text-foreground outline-none focus:border-accent-primary" /></label>
          <label className="text-xs text-foreground-muted">Measured trials<input type="number" min={1} max={30} value={repetitions} onChange={(e) => setRepetitions(Number(e.target.value))} className="mt-1.5 w-full rounded-lg border border-border bg-background px-3 py-2.5 text-sm text-foreground outline-none focus:border-accent-primary" /></label>
          <label className="text-xs text-foreground-muted">Warmups<input type="number" min={0} max={5} value={warmups} onChange={(e) => setWarmups(Number(e.target.value))} className="mt-1.5 w-full rounded-lg border border-border bg-background px-3 py-2.5 text-sm text-foreground outline-none focus:border-accent-primary" /></label>
          <div className="flex flex-col justify-end rounded-lg bg-background px-3 py-2.5"><span className="text-xs text-foreground-muted">Maximum work</span><span className="mt-1 text-xs font-medium text-foreground">10M total paths</span></div>
        </div>
        <p className="mt-3 text-[11px] leading-5 text-foreground-subtle">Up to {maxPaths.toLocaleString()} paths per run at these trial settings (10 million total-path budget). Reported latency is engine wall time and excludes process startup.</p>
        {error && <p role="alert" className="mt-4 rounded-lg border border-accent-danger/30 bg-accent-danger/10 p-3 text-sm text-accent-danger">{error}</p>}
        <button type="button" onClick={execute} disabled={loading || !portfolio.trim() || simulations < 1000 || simulations > maxPaths || repetitions < 1 || repetitions > 30 || warmups < 0 || warmups > 5} className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-accent-primary px-4 py-3 text-sm font-medium text-white transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50">{loading ? <><Loader2 size={17} className="animate-spin" /> Running trials…</> : <><Play size={16} /> Run latency benchmark</>}</button>
      </div>

      <div className="min-w-0 space-y-5">
        {!result && <div className="flex min-h-[360px] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-surface/70 p-8 text-center"><span className="rounded-2xl bg-accent-primary-dim p-4 text-accent-primary"><Timer size={26} /></span><h2 className="mt-4 text-lg font-semibold">Measure, then compare</h2><p className="mt-2 max-w-sm text-sm leading-6 text-foreground-muted">Repeated runs give you p50, p95 and p99 latency, plus the individual trial timings to spot noisy results.</p></div>}
        {result && <>
          <div className="grid gap-3 sm:grid-cols-3">{[
            ['Backends available', String(comparable.length)],
            ['Trials per backend', result.requested_repetitions.toLocaleString()],
            ['Warmups excluded', result.warmups.toLocaleString()],
          ].map(([label, value]) => <div key={label} className="rounded-xl border border-border bg-surface p-4"><p className="text-xs text-foreground-muted">{label}</p><p className="mt-2 text-xl font-semibold tabular-nums">{value}</p></div>)}</div>
          <section className="rounded-xl border border-border bg-surface p-5"><div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><h2 className="font-semibold">Trial latency</h2><p className="mt-1 text-xs text-foreground-muted">Engine wall time · milliseconds</p></div><div className="flex flex-wrap gap-3">{comparable.map((row) => <span key={row.requested_backend} className="flex items-center gap-1.5 text-xs text-foreground-muted"><i className="h-2 w-2 rounded-full" style={{ backgroundColor: colors[row.requested_backend] }} />{row.requested_backend}</span>)}</div></div>
            {comparable.length ? <div className="h-64 w-full"><ResponsiveContainer width="100%" height="100%"><LineChart data={chartData} margin={{ top: 8, right: 10, bottom: 0, left: -12 }}><CartesianGrid strokeDasharray="3 3" stroke="#273449" /><XAxis dataKey="trial" stroke="#94a3b8" tickLine={false} axisLine={false} /><YAxis stroke="#94a3b8" tickLine={false} axisLine={false} unit=" ms" /><Tooltip contentStyle={{ backgroundColor: '#101a2a', border: '1px solid #2a3950', borderRadius: 10 }} labelFormatter={(trial) => `Trial ${trial}`} formatter={(value) => [`${Number(value).toFixed(2)} ms`, 'Latency']} /><Line type="monotone" dataKey="CPU" stroke={colors.CPU} strokeWidth={2} dot={{ r: 3 }} connectNulls={false} /><Line type="monotone" dataKey="Parallel" stroke={colors.Parallel} strokeWidth={2} dot={{ r: 3 }} connectNulls={false} /><Line type="monotone" dataKey="CUDA" stroke={colors.CUDA} strokeWidth={2} dot={{ r: 3 }} connectNulls={false} /></LineChart></ResponsiveContainer></div> : <p className="rounded-lg bg-background p-4 text-sm text-foreground-muted">No direct backend measurements were available. Check the backend result details below.</p>}
          </section>
          <section className="overflow-hidden rounded-xl border border-border bg-surface"><div className="border-b border-border px-5 py-4"><h2 className="font-semibold">Backend latency summary</h2><p className="mt-1 text-xs text-foreground-muted">Requested backend is compared with the backend that actually ran.</p></div><div className="overflow-x-auto"><table className="w-full min-w-[700px] text-sm"><thead><tr className="text-left text-[10px] uppercase tracking-wider text-foreground-subtle"><th className="px-4 py-3 font-medium">Request → actual</th><th className="px-3 py-3 text-right font-medium">p50</th><th className="px-3 py-3 text-right font-medium">p95</th><th className="px-3 py-3 text-right font-medium">p99</th><th className="px-3 py-3 text-right font-medium">Paths / sec</th><th className="px-4 py-3 text-right font-medium">Status</th></tr></thead><tbody>{result.benchmarks.map((row) => <tr key={row.requested_backend} className="border-t border-border-subtle"><td className="px-4 py-3 font-medium">{row.requested_backend} <span className="text-foreground-subtle">→ {row.backend ?? '—'}</span></td><td className="px-3 py-3 text-right tabular-nums">{row.latency_ms?.p50.toFixed(2) ?? '—'}</td><td className="px-3 py-3 text-right tabular-nums">{row.latency_ms?.p95.toFixed(2) ?? '—'}</td><td className="px-3 py-3 text-right tabular-nums">{row.latency_ms?.p99.toFixed(2) ?? '—'}</td><td className="px-3 py-3 text-right tabular-nums">{row.paths_per_second ? Math.round(row.paths_per_second).toLocaleString() : '—'}</td><td className="px-4 py-3 text-right"><span className={`rounded-full px-2 py-1 text-[10px] uppercase tracking-wider ${row.status === 'available' ? 'bg-accent-success/10 text-accent-success' : row.status === 'fallback' ? 'bg-accent-warning/10 text-accent-warning' : 'bg-accent-danger/10 text-accent-danger'}`}>{row.status}</span></td></tr>)}</tbody></table></div></section>
          <button type="button" onClick={execute} disabled={loading} className="inline-flex items-center gap-2 text-xs text-foreground-muted transition hover:text-foreground disabled:opacity-50"><RotateCcw size={13} /> Run again with identical settings</button>
        </>}
      </div>
    </section>
  </div>;
}
