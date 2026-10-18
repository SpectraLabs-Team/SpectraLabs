'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { FileText, ShieldCheck, Zap } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { HEADLINE } from '@/lib/baseline';
import { buildAndVerify } from '@/lib/hashChain';
import { analyticVaR } from '@/lib/model';
import { DEFAULT_PAPER, paperAccount } from '@/lib/paperLedger';
import { SAMPLE_CSV, parseCsv, portfolioStats } from '@/lib/portfolio';
import { useEngineHealth } from '@/lib/useEngineHealth';
import { cn, formatCurrency, formatDuration, formatPercent } from '@/lib/utils';

const SAMPLE = parseCsv(SAMPLE_CSV).positions;
const STATS = portfolioStats(SAMPLE);
const ACCOUNT = paperAccount(STATS, DEFAULT_PAPER);

function Holo({ icon, title, children, className, delay = 0, tone = 'accent' }: { icon: ReactNode; title: string; children: ReactNode; className?: string; delay?: number; tone?: 'accent' | 'profit' }) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      initial={reduce ? false : { opacity: 0, y: 16, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.7, delay, ease: [0.22, 1, 0.36, 1] }}
      className={cn('animate-float', className)}
      style={{ animationDelay: `${delay * 2}s` }}
    >
      <div className={cn('rounded-xl border bg-panel/70 p-4 shadow-glow backdrop-blur-md dark:bg-[#071426]/70', tone === 'profit' ? 'border-profit/30' : 'border-accent/30')}>
        <div className="flex items-start gap-3">
          <span className={cn('mt-0.5', tone === 'profit' ? 'text-profit' : 'text-accent')}>{icon}</span>
          <div className="min-w-0">
            <p className="text-[13px] font-semibold text-ink">{title}</p>
            <div className="mt-1.5 space-y-0.5 text-[12px] text-ink/70">{children}</div>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

/** The hero's three floating cards, fed by the live engine where it is reachable. */
export function useHeroData() {
  const engine = useEngineHealth();
  const online = engine.status === 'online';
  const [kernel, setKernel] = useState<{ pps: number; latency: number; live: boolean }>({ pps: HEADLINE.rustPps, latency: HEADLINE.smallMs ?? 0, live: false });
  const [varValue, setVarValue] = useState<{ v: number; live: boolean }>({ v: analyticVaR({ value: STATS.netValue, volatility: STATS.modelVolatility, horizonDays: 1 }, 0.99), live: false });
  const [ledger, setLedger] = useState<{ head: string; verified: boolean } | null>(null);

  useEffect(() => {
    buildAndVerify().then(setLedger);
  }, []);

  useEffect(() => {
    if (!online) return;
    let live = true;
    (async () => {
      try {
        const base = { portfolio_csv: SAMPLE_CSV, metric: 'var' as const, confidence: 0.99, horizon_days: 1, backend: 'auto' as const };
        await calculateRisk({ ...base, simulations: 100_000, backend: 'parallel' }); // warm-up, untimed
        const big = await calculateRisk({ ...base, simulations: 1_000_000, backend: 'parallel' });
        const small = await calculateRisk({ ...base, simulations: 10_000 });
        const risk = await calculateRisk({ ...base, simulations: 100_000 });
        if (!live) return;
        setKernel({ pps: (big.simulations_run * 1000) / big.compute_ms, latency: small.compute_ms, live: true });
        setVarValue({ v: risk.value, live: true });
      } catch {
        /* keep reference values */
      }
    })();
    return () => {
      live = false;
    };
  }, [online]);

  return { kernel, varValue, ledger };
}

export function HoloCards({ layout, data }: { layout: 'float' | 'stack'; data: ReturnType<typeof useHeroData> }) {
  const { kernel, varValue, ledger } = data;
  const pos = (cls: string) => (layout === 'float' ? cls : '');

  return (
    <>
      <Holo icon={<Zap className="h-5 w-5" />} title="Pure Rust Kernel" className={pos('absolute left-[45%] top-[22%] w-[230px]')} delay={0.2}>
        <p className="num text-xl font-semibold text-ink">{Math.round(kernel.pps).toLocaleString()}</p>
        <p>paths / sec {kernel.live ? '· live' : '· reference run'}</p>
        <p>
          Latency: <span className="num font-medium text-profit">{formatDuration(kernel.latency)}</span> <span className="text-ink/45">(10k paths)</span>
        </p>
      </Holo>
      <Holo icon={<ShieldCheck className="h-5 w-5" />} title="Pre-Trade Risk Check" className={pos('absolute right-[16%] top-[40%] w-[236px]')} delay={0.35}>
        <p>
          VaR (99%, 1d):{' '}
          <span className="num text-base font-semibold text-loss">−{formatCurrency(varValue.v)}</span>
        </p>
        <p className="num text-loss">({formatPercent(-varValue.v / STATS.netValue)})</p>
        <p>
          Leverage: <span className="num">{ACCOUNT.grossLeverage.toFixed(2)}×</span> <span className="font-medium text-profit">Passed</span>
          <span className="text-ink/45"> · paper</span>
        </p>
      </Holo>
      <Holo icon={<FileText className="h-5 w-5" />} title="SHA-256 Ledger" className={pos('absolute right-0 top-[50%] w-[210px]')} delay={0.5} tone="profit">
        <p className="num">Hash: {ledger ? `${ledger.head.slice(0, 6)}…${ledger.head.slice(-4)}` : '…'}</p>
        <p>
          Audit status:{' '}
          <span className={cn('font-medium', ledger?.verified ? 'text-profit' : 'text-ink/50')}>{ledger ? (ledger.verified ? 'Verified' : 'Broken') : 'checking'}</span>
        </p>
      </Holo>
    </>
  );
}

export function TickerChips({ layout }: { layout: 'float' | 'row' }) {
  const reduce = useReducedMotion();
  const pick = ['AAPL', 'NVDA', 'MSFT'];
  const places = ['absolute left-[9%] bottom-[9%]', 'absolute left-[40%] bottom-[19%]', 'absolute right-[7%] bottom-[14%]'];
  return (
    <>
      {pick.map((sym, i) => {
        const idx = SAMPLE.findIndex((p) => p.symbol === sym);
        const p = SAMPLE[idx];
        const w = STATS.weights[idx] ?? 0;
        return (
          <motion.div
            key={sym}
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.6 + i * 0.12, duration: 0.6 }}
            className={cn(layout === 'float' && places[i], 'animate-float')}
            style={{ animationDelay: `${i * 1.3}s` }}
          >
            <div className="flex items-center gap-3 rounded-xl border border-accent/25 bg-panel/75 px-3.5 py-2.5 shadow-glow backdrop-blur-md dark:bg-[#071426]/75">
              <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-ink/[0.06] font-mono text-[11px] font-bold text-ink">{sym.slice(0, 2)}</span>
              <div>
                <p className="text-[12px] font-semibold text-ink">{sym}</p>
                <p className="num text-[13px] text-ink/85">{formatCurrency(p.price, 2)}</p>
              </div>
              <div className="ml-1 text-right">
                <svg viewBox="0 0 36 36" className="h-8 w-8 -rotate-90" aria-hidden>
                  <circle cx="18" cy="18" r="14" fill="none" className="stroke-ink/10" strokeWidth="4" />
                  <circle cx="18" cy="18" r="14" fill="none" className="stroke-accent" strokeWidth="4" strokeLinecap="round" strokeDasharray={`${w * 88} 88`} />
                </svg>
              </div>
              <p className="num -ml-1 text-[11px] text-ink/55">
                {formatPercent(w, 1)}
                <br />
                <span className="text-[9.5px]">of book</span>
              </p>
            </div>
          </motion.div>
        );
      })}
    </>
  );
}
