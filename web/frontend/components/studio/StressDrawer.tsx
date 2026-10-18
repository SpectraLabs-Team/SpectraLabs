'use client';

import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Cpu, Flame, Play, X } from 'lucide-react';
import { calculateRisk, runStressTest } from '@/lib/api';
import { portfolioStats, shockPositions, toCsv } from '@/lib/portfolio';
import { SCENARIOS, STRESS_CONFIDENCE, type Scenario } from '@/lib/scenarios';
import type { BackendRequest } from '@/lib/types';
import { cn, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';
import { Badge, ErrorNote, Spinner } from '@/components/ui/primitives';
import { usePortfolio } from './PortfolioContext';

export interface StressOutcome {
  valueBefore: number;
  valueAfter: number;
  immediateLoss: number;
  postVaR: number;
  backend: string;
  computeMs: number;
}

interface Props {
  open: boolean;
  onClose: () => void;
  simulations: number;
  horizonDays: number;
  backend: BackendRequest;
  results: Record<string, StressOutcome>;
  onResult: (id: string, r: StressOutcome) => void;
  baseVaR99: number | null;
}

export function StressDrawer({ open, onClose, simulations, horizonDays, backend, results, onResult, baseVaR99 }: Props) {
  const { positions, csv, stats, engine } = usePortfolio();
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState('');
  const online = engine.status === 'online';

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const runOne = async (s: Scenario) => {
    setRunning((r) => new Set(r).add(s.id));
    setError('');
    try {
      if (s.source.kind === 'engine') {
        const r = await runStressTest({ portfolio_csv: csv, scenario: s.source.scenario, simulations, horizon_days: horizonDays, backend });
        onResult(s.id, { valueBefore: r.value_before, valueAfter: r.value_after, immediateLoss: r.scenario_loss, postVaR: r.value, backend: r.backend, computeMs: r.compute_ms });
      } else {
        const shocked = shockPositions(positions, s.priceFactor, s.volMultiplier);
        const after = portfolioStats(shocked).netValue;
        const r = await calculateRisk({ portfolio_csv: toCsv(shocked), metric: 'var', confidence: STRESS_CONFIDENCE, simulations, horizon_days: horizonDays, backend });
        onResult(s.id, { valueBefore: stats.netValue, valueAfter: after, immediateLoss: stats.netValue - after, postVaR: r.value, backend: r.backend, computeMs: r.compute_ms });
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

  const runAll = () => Promise.all(SCENARIOS.map(runOne));
  const worst = Math.max(1, ...Object.values(results).map((r) => r.immediateLoss + r.postVaR));

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-[2px]" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={onClose} />
          <motion.aside
            role="dialog"
            aria-modal="true"
            aria-label="Stress test scenarios"
            className="fixed inset-y-0 right-0 z-50 flex w-full max-w-[460px] flex-col border-l border-white/10 bg-obsidian-900/95 shadow-2xl backdrop-blur-xl"
            initial={{ x: '100%' }}
            animate={{ x: 0 }}
            exit={{ x: '100%' }}
            transition={{ type: 'spring', stiffness: 380, damping: 40 }}
          >
            <div className="flex items-start justify-between gap-4 border-b border-white/[0.06] p-5">
              <div>
                <p className="eyebrow text-loss">Stress testing</p>
                <h2 className="mt-1 text-lg font-semibold text-white">Scenario shocks</h2>
                <p className="mt-1 text-xs leading-5 text-slate-500">
                  Immediate P&amp;L from a uniform price shock, then the engine&apos;s {formatPercent(STRESS_CONFIDENCE, 0)} VaR of the shocked book over {horizonDays}d at{' '}
                  {simulations.toLocaleString()} paths.
                </p>
              </div>
              <button type="button" onClick={onClose} className="rounded-md p-1.5 text-slate-500 transition hover:bg-white/5 hover:text-white" aria-label="Close">
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 px-5 pt-4">
              <span className="text-xs text-slate-500">
                Base VaR 99%: <span className="num text-slate-200">{baseVaR99 !== null ? formatCurrency(baseVaR99) : '—'}</span>
              </span>
              <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={runAll} disabled={!online || running.size > 0}>
                {running.size ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />} Run all
              </button>
            </div>
            {error && (
              <div className="px-5 pt-3">
                <ErrorNote>{error}</ErrorNote>
              </div>
            )}

            <ul className="flex-1 space-y-2.5 overflow-y-auto p-5">
              {SCENARIOS.map((s) => {
                const r = results[s.id];
                const busy = running.has(s.id);
                return (
                  <li key={s.id} className={cn('rounded-xl border p-4 transition-colors', r ? 'border-loss/20 bg-loss/[0.04]' : 'border-white/[0.07] bg-white/[0.02]')}>
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <Flame className="h-3.5 w-3.5 text-loss" />
                          <span className="text-sm font-medium text-white">{s.name}</span>
                          {s.source.kind === 'engine' ? <Badge tone="accent">/api/stress</Badge> : <Badge>shocked CSV → /api/risk</Badge>}
                        </div>
                        <p className="mt-1 text-xs text-slate-500">{s.description}</p>
                      </div>
                      <button type="button" onClick={() => runOne(s)} disabled={!online || busy} className="btn-secondary shrink-0 px-2.5 py-1 text-xs" aria-label={`Run ${s.name}`}>
                        {busy ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
                      </button>
                    </div>
                    {r && (
                      <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} className="overflow-hidden">
                        <div className="mt-3 grid grid-cols-3 gap-2">
                          <div>
                            <p className="text-[10px] uppercase tracking-wider text-slate-500">Immediate P&amp;L</p>
                            <p className="num mt-0.5 text-sm font-semibold text-loss">−{formatCurrency(r.immediateLoss)}</p>
                          </div>
                          <div>
                            <p className="text-[10px] uppercase tracking-wider text-slate-500">Value after</p>
                            <p className="num mt-0.5 text-sm text-slate-200">{formatCurrency(r.valueAfter)}</p>
                          </div>
                          <div>
                            <p className="text-[10px] uppercase tracking-wider text-slate-500">Post-shock VaR</p>
                            <p className="num mt-0.5 text-sm text-rose-300">{formatCurrency(r.postVaR)}</p>
                          </div>
                        </div>
                        <div className="mt-3 flex h-1.5 overflow-hidden rounded-full bg-white/5">
                          <div className="bg-loss" style={{ width: `${(r.immediateLoss / worst) * 100}%` }} />
                          <div className="bg-rose-300/60" style={{ width: `${(r.postVaR / worst) * 100}%` }} />
                        </div>
                        <p className="mt-2 flex items-center gap-1.5 font-mono text-[10.5px] text-slate-500">
                          <Cpu className="h-3 w-3" /> {r.backend} · {formatDuration(r.computeMs)}
                          {baseVaR99 !== null && <span className="ml-auto">VaR {r.postVaR >= baseVaR99 ? '+' : ''}{formatPercent(r.postVaR / baseVaR99 - 1, 1)} vs base</span>}
                        </p>
                      </motion.div>
                    )}
                  </li>
                );
              })}
            </ul>
            <p className="border-t border-white/[0.06] px-5 py-3 text-[11px] leading-5 text-slate-600">
              Custom scenarios are hypothetical uniform shocks sized after historical episodes. They are not historical replays. Every position moves by the same percentage.
            </p>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  );
}
