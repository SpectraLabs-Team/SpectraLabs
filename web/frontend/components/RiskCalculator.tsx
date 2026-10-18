"use client";

import { useEffect, useState } from "react";
import { Play, Loader2, Upload, ShieldCheck } from "lucide-react";
import { calculateRisk, getSamplePortfolio } from '@/lib/api';
import type { RiskResult } from '@/lib/types';

export default function RiskCalculator() {
  const [portfolio, setPortfolio] = useState("");
  const [metric, setMetric] = useState("var");
  const [confidence, setConfidence] = useState(0.99);
  const [simulations, setSimulations] = useState(1000000);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<RiskResult | null>(null);
  const [error, setError] = useState("");

  useEffect(() => { getSamplePortfolio().then(setPortfolio).catch(() => {}); }, []);

  const runCalculation = async () => {
    setLoading(true);
    setError("");
    setResult(null);

    try {
      setResult(await calculateRisk(portfolio, metric, confidence, simulations));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Risk calculation failed');
    } finally {
      setLoading(false);
    }
  };

  const loadSample = async () => {
    try {
      setPortfolio(await getSamplePortfolio());
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load sample portfolio');
    }
  };

  return (
    <div className="grid gap-5 xl:grid-cols-[1fr_0.75fr]">
      <section className="rounded-xl border border-border bg-surface p-5 sm:p-6">
      <div className="mb-5"><h2 className="text-lg font-semibold">Calculation setup</h2><p className="mt-1 text-sm text-foreground-muted">Configure a simulation using the current engine.</p></div>

      <div className="space-y-4">
        <div>
          <label className="block text-sm font-medium mb-2">Portfolio (CSV)</label>
          <textarea
            value={portfolio}
            onChange={(e) => setPortfolio(e.target.value)}
            className="w-full h-36 resize-y rounded-lg border border-border bg-background p-3 text-sm font-mono outline-none focus:border-accent-primary"
            placeholder="symbol,quantity,price"
          />
          <button
            onClick={loadSample}
            className="mt-2 inline-flex items-center gap-1.5 text-sm text-accent-primary hover:underline"
          >
            <Upload size={14} /> Load sample portfolio
          </button>
        </div>

        <div className="grid gap-3 sm:grid-cols-3">
          <div>
            <label className="block text-sm font-medium mb-2">Metric</label>
            <select
              value={metric}
              onChange={(e) => setMetric(e.target.value)}
              className="w-full rounded-lg border border-border bg-background p-2.5 text-sm outline-none focus:border-accent-primary"
            >
              <optgroup label="Market Risk">
                <option value="var">VaR</option>
                <option value="cvar">CVaR</option>
                <option value="volatility">Volatility</option>
                <option value="downsidedeviation">Downside Deviation</option>
                <option value="semideviation">Semi-Deviation</option>
                <option value="skewness">Skewness</option>
                <option value="kurtosis">Kurtosis</option>
              </optgroup>
              <optgroup label="Portfolio">
                <option value="hhi">Concentration (HHI)</option>
              </optgroup>
              {/* Drawdowns, Sharpe/Sortino-style ratios, Beta, tracking error and VaR attribution are not
                  computable from simulated terminal losses; the engine reports them as unsupported
                  instead of returning placeholder numbers. They are available on backtest returns. */}
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">Confidence</label>
            <select
              value={confidence}
              onChange={(e) => setConfidence(parseFloat(e.target.value))}
              className="w-full rounded-lg border border-border bg-background p-2.5 text-sm outline-none focus:border-accent-primary"
            >
              <option value="0.95">95%</option>
              <option value="0.99">99%</option>
              <option value="0.999">99.9%</option>
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">Simulations</label>
            <input
              type="number"
              value={simulations}
              onChange={(e) => setSimulations(parseInt(e.target.value))}
            className="w-full rounded-lg border border-border bg-background p-2.5 text-sm outline-none focus:border-accent-primary"
              min="1000"
              step="100000"
            />
          </div>
        </div>

        <button
          onClick={runCalculation}
          disabled={loading}
          className="flex items-center gap-2 rounded-lg bg-accent-primary px-5 py-2.5 text-sm font-medium text-white transition hover:brightness-110 disabled:opacity-50"
        >
          {loading ? (
            <Loader2 className="w-5 h-5 animate-spin" />
          ) : (
            <Play className="w-5 h-5" />
          )}
          Calculate
        </button>
      </div>

      {error && (
        <div role="alert" className="mt-4 rounded-lg border border-accent-danger/30 bg-accent-danger/10 p-4 text-sm text-accent-danger">
          {error}
        </div>
      )}

      </section>
      <section className="rounded-xl border border-border bg-surface p-5 sm:p-6">
        <div className="flex items-center gap-2"><ShieldCheck size={17} className="text-accent-success" /><h2 className="font-semibold">Run result</h2></div>
        {!result ? <div className="mt-6 rounded-lg border border-dashed border-border p-6 text-center text-sm text-foreground-muted">Your result and reproducibility details will appear here after a successful run.</div> : (
        <div className="mt-5 space-y-4">
          <div className="rounded-lg bg-background p-4"><p className="text-xs uppercase tracking-wider text-foreground-subtle">{result.metric} · {(result.confidence * 100).toFixed(1)}% confidence</p><p className="mt-2 break-all text-3xl font-semibold tracking-tight">{result.value.toLocaleString(undefined, { maximumFractionDigits: 4 })}</p></div>
          <div className="grid grid-cols-2 gap-3 text-sm">
            <div>
              <span className="text-foreground-muted">Simulations:</span> {result.simulations_run.toLocaleString()}
            </div>
            <div>
              <span className="text-foreground-muted">Backend:</span> {result.backend}
            </div>
            <div>
              <span className="text-foreground-muted">Compute:</span> {result.compute_ms.toFixed(2)} ms
            </div>
            <div>
              <span className="text-foreground-muted">Total time:</span> {result.total_ms.toFixed(2)} ms
            </div>
            <div>
              <span className="text-foreground-muted">Seed:</span> {result.seed}
            </div>
            <div>
              <span className="text-foreground-muted">Version:</span> {result.library_version}
            </div>
          </div>
        </div>)}
      </section>
    </div>
  );
}
