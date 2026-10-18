"use client";

import { useEffect, useState } from 'react';
import { AlertTriangle, Loader2, ShieldAlert, Upload } from 'lucide-react';
import { getSamplePortfolio, runStressTest } from '@/lib/api';
import type { StressResult } from '@/lib/types';

const scenarios = [
  { value: 'market-crash', label: 'Market shock', impact: 'Apply a 30% price shock to each position.' },
  { value: 'mild-correction', label: 'Mild correction', impact: 'Apply a 10% price shock to each position.' },
];

export default function StressTest() {
  const [portfolio, setPortfolio] = useState('');
  const [scenario, setScenario] = useState(scenarios[0].value);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<StressResult | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { getSamplePortfolio().then(setPortfolio).catch((e: Error) => setError(e.message)); }, []);

  const selected = scenarios.find((item) => item.value === scenario) ?? scenarios[0];
  const execute = async () => {
    setLoading(true); setError(''); setResult(null);
    try { setResult(await runStressTest(portfolio, scenario)); }
    catch (e) { setError(e instanceof Error ? e.message : 'Stress test failed.'); }
    finally { setLoading(false); }
  };

  return <div className="space-y-7">
    <header><p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-warning">Scenario analysis</p><h1 className="text-3xl font-semibold tracking-tight">Stress testing</h1><p className="mt-2 max-w-2xl text-sm leading-6 text-foreground-muted">Apply a defined price shock to the portfolio, then recalculate VaR with the Monte Carlo engine.</p></header>
    <div className="grid gap-5 xl:grid-cols-[0.8fr_1.2fr]">
      <section className="rounded-xl border border-border bg-surface p-5 sm:p-6"><div className="mb-5 flex items-start gap-3"><span className="rounded-lg bg-accent-warning/10 p-2 text-accent-warning"><ShieldAlert size={18} /></span><div><h2 className="font-semibold">Scenario setup</h2><p className="mt-1 text-xs text-foreground-muted">These are deterministic position price shocks.</p></div></div>
        <label htmlFor="stress-portfolio" className="mb-2 block text-sm font-medium">Portfolio CSV</label><textarea id="stress-portfolio" value={portfolio} onChange={(e) => setPortfolio(e.target.value)} placeholder="symbol,quantity,price" className="h-36 w-full resize-y rounded-lg border border-border bg-background p-3 font-mono text-xs outline-none focus:border-accent-primary" />
        <button type="button" onClick={() => getSamplePortfolio().then(setPortfolio).catch((e: Error) => setError(e.message))} className="mt-2 inline-flex items-center gap-1.5 text-xs text-accent-primary hover:underline"><Upload size={13} />Load sample portfolio</button>
        <label htmlFor="stress-scenario" className="mb-2 mt-5 block text-sm font-medium">Price shock</label><select id="stress-scenario" value={scenario} onChange={(e) => setScenario(e.target.value)} className="w-full rounded-lg border border-border bg-background p-3 text-sm outline-none focus:border-accent-primary">{scenarios.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select>
        <p className="mt-2 text-xs leading-5 text-foreground-muted">{selected.impact} Shock sizes available in this engine are intentionally explicit.</p>
        {error && <p role="alert" className="mt-4 rounded-lg border border-accent-danger/30 bg-accent-danger/10 p-3 text-sm text-accent-danger">{error}</p>}
        <button type="button" disabled={loading || !portfolio.trim()} onClick={execute} className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-accent-warning px-4 py-3 text-sm font-semibold text-slate-950 transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50">{loading ? <><Loader2 size={16} className="animate-spin" />Calculating scenario…</> : <><AlertTriangle size={16} />Run stress test</>}</button>
      </section>
      <section className="rounded-xl border border-border bg-surface p-5 sm:p-6"><div className="flex items-center gap-2"><AlertTriangle size={17} className="text-accent-warning" /><h2 className="font-semibold">Scenario result</h2></div>
        {!result ? <div className="mt-6 flex min-h-56 flex-col items-center justify-center rounded-lg border border-dashed border-border p-6 text-center"><span className="text-sm text-foreground-muted">Run a scenario to view stressed VaR and run metadata.</span><span className="mt-2 text-xs text-foreground-subtle">Result uses the selected position price shock.</span></div> : <div className="mt-5 space-y-4">
          <div className="rounded-xl border border-accent-warning/15 bg-gradient-to-br from-accent-warning/10 to-transparent p-5"><p className="text-xs uppercase tracking-wider text-foreground-subtle">{selected.label} · {result.metric}</p><p className="mt-2 text-3xl font-semibold tabular-nums">{result.value.toLocaleString(undefined, { maximumFractionDigits: 2 })}</p><p className="mt-2 text-xs text-foreground-muted">{selected.impact}</p></div>
          <div className="grid gap-3 sm:grid-cols-2">{[
            ['Confidence', `${(result.confidence * 100).toFixed(1)}%`],
            ['Simulations', result.simulations_run.toLocaleString()],
            ['Backend used', result.backend],
            ['Compute time', `${result.compute_ms.toFixed(2)} ms`],
            ['Seed', String(result.seed)],
            ['Engine version', result.library_version],
          ].map(([label, value]) => <div key={label} className="rounded-lg bg-background px-3 py-3"><p className="text-[10px] uppercase tracking-wider text-foreground-subtle">{label}</p><p className="mt-1 text-sm font-medium tabular-nums">{value}</p></div>)}</div>
        </div>}
      </section>
    </div>
  </div>;
}
