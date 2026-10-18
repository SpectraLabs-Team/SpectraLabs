'use client';

import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Cpu, Flame, Play } from 'lucide-react';
import { calculateRisk, runStressTest } from '@/lib/api';
import { portfolioStats, shockPositions, toCsv } from '@/lib/portfolio';
import { SCENARIOS, STRESS_CONFIDENCE, type Scenario } from '@/lib/scenarios';
import type { BackendRequest } from '@/lib/types';
import { cn, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { usePortfolio } from './PortfolioContext';

interface Outcome {
  valueAfter: number;
  immediateLoss: number;
  postVaR: number;
  backend: string;
  computeMs: number;
}

/** Scenario cards: immediate P&L of a uniform shock, then the engine's 99% VaR of the shocked book. */
export function StressPanel({ simulations, horizonDays, backend }: { simulations: number; horizonDays: number; backend: BackendRequest }) {
  const { positions, csv, stats, engine, revision } = usePortfolio();
  const online = engine.status === 'online';
  const [results, setResults] = useState<Record<string, Outcome>>({});
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [baseVaR, setBaseVaR] = useState<number | null>(null);
  const [error, setError] = useState('');

  // Results depend on the book and run settings.
  useEffect(() => {
    setResults({});
    setBaseVaR(null);
  }, [revision, simulations, horizonDays, backend]);

  const ensureBase = async () => {
    if (baseVaR !== null) return;
    try {
      const r = await calculateRisk({ portfolio_csv: csv, metric: 'var', confidence: STRESS_CONFIDENCE, simulations, horizon_days: horizonDays, backend });
      setBaseVaR(r.value);
    } catch {
      /* surfaced by scenario runs */
    }
  };

  const runOne = async (s: Scenario) => {
    setRunning((r) => new Set(r).add(s.id));
    setError('');
    ensureBase();
    try {
      if (s.source.kind === 'engine') {
        const r = await runStressTest({ portfolio_csv: csv, scenario: s.source.scenario, simulations, horizon_days: horizonDays, backend });
        setResults((x) => ({ ...x, [s.id]: { valueAfter: r.value_after, immediateLoss: r.scenario_loss, postVaR: r.value, backend: r.backend, computeMs: r.compute_ms } }));
      } else {
        const shocked = shockPositions(positions, s.priceFactor, s.volMultiplier);
        const after = portfolioStats(shocked).netValue;
        const r = await calculateRisk({ portfolio_csv: toCsv(shocked), metric: 'var', confidence: STRESS_CONFIDENCE, simulations, horizon_days: horizonDays, backend });
        setResults((x) => ({ ...x, [s.id]: { valueAfter: after, immediateLoss: stats.netValue - after, postVaR: r.value, backend: r.backend, computeMs: r.compute_ms } }));
      }
    } catch (e) {
      setError(`${s.name}: ${e instanceof Error ? e.message : 'failed'}`);
    } finally {
      setRunning((r) => {
        const n = new Set(r);
        n.delete(s.id);
        return n;
      });
    }
  };

  const worst = Math.max(1, ...Object.values(results).map((r) => r.immediateLoss + r.postVaR));

  return (
    <GlassCard as="section" className="p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <Flame className="h-4 w-4 text-loss" /> Stress testing
          </h2>
          <p className="mt-0.5 text-xs text-ink/50">
            Immediate P&amp;L of a uniform price shock, then the engine&apos;s {formatPercent(STRESS_CONFIDENCE, 0)} VaR of the shocked book ({horizonDays}d, {simulations.toLocaleString()} paths).
            {baseVaR !== null && (
              <>
                {' '}Base VaR <span className="num text-ink/80">{formatCurrency(baseVaR)}</span>.
              </>
            )}
          </p>
        </div>
        <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={() => Promise.all(SCENARIOS.map(runOne))} disabled={!online || running.size > 0}>
          {running.size ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />} Run all scenarios
        </button>
      </div>
      {error && (
        <div className="mt-3">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}

      <div className="mt-4 grid gap-3 sm:grid-cols-2 2xl:grid-cols-3">
        {SCENARIOS.map((s) => {
          const r = results[s.id];
          const busy = running.has(s.id);
          return (
            <div key={s.id} className={cn('rounded-xl border p-4 transition-colors', r ? 'border-loss/25 bg-loss/[0.04]' : 'border-ink/[0.08] bg-ink/[0.02]')}>
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-semibold text-ink">{s.name}</span>
                    <span className="num rounded-md bg-loss/10 px-1.5 py-0.5 text-[11px] font-semibold text-loss">{s.shock}</span>
                  </div>
                  <p className="mt-1 text-[11.5px] leading-4 text-ink/50">{s.description}</p>
                </div>
                <button type="button" onClick={() => runOne(s)} disabled={!online || busy} className="btn-secondary shrink-0 px-2 py-1 text-xs" aria-label={`Run ${s.name}`}>
                  {busy ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
                </button>
              </div>
              {r ? (
                <motion.div initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }}>
                  <div className="mt-3 grid grid-cols-3 gap-2">
                    <div>
                      <p className="text-[10px] uppercase tracking-wider text-ink/45">P&amp;L</p>
                      <p className="num mt-0.5 text-sm font-semibold text-loss">−{formatCurrency(r.immediateLoss)}</p>
                    </div>
                    <div>
                      <p className="text-[10px] uppercase tracking-wider text-ink/45">Value after</p>
                      <p className="num mt-0.5 text-sm text-ink/90">{formatCurrency(r.valueAfter)}</p>
                    </div>
                    <div>
                      <p className="text-[10px] uppercase tracking-wider text-ink/45">Post VaR</p>
                      <p className="num mt-0.5 text-sm text-rose-600 dark:text-rose-300">{formatCurrency(r.postVaR)}</p>
                    </div>
                  </div>
                  <div className="mt-3 flex h-1.5 overflow-hidden rounded-full bg-ink/5">
                    <div className="bg-loss" style={{ width: `${(r.immediateLoss / worst) * 100}%` }} />
                    <div className="bg-rose-400/60" style={{ width: `${(r.postVaR / worst) * 100}%` }} />
                  </div>
                  <p className="mt-2 flex items-center gap-1.5 font-mono text-[10.5px] text-ink/45">
                    <Cpu className="h-3 w-3" /> {r.backend} · {formatDuration(r.computeMs)}
                    {baseVaR !== null && (
                      <span className="ml-auto">
                        VaR {r.postVaR >= baseVaR ? '+' : ''}
                        {formatPercent(r.postVaR / baseVaR - 1, 1)}
                      </span>
                    )}
                  </p>
                </motion.div>
              ) : (
                <div className="mt-3 flex items-center gap-2">
                  {s.source.kind === 'engine' ? <Badge tone="accent">/api/stress</Badge> : <Badge>shocked CSV → /api/risk</Badge>}
                </div>
              )}
            </div>
          );
        })}
      </div>
      <p className="mt-4 text-[11px] leading-5 text-ink/40">Custom scenarios are hypothetical uniform shocks sized after historical episodes. They are not historical replays.</p>
    </GlassCard>
  );
}
