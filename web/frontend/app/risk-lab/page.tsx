"use client";

import RiskCalculator from '@/components/RiskCalculator';
import { motion, useReducedMotion } from 'framer-motion';

const implemented = new Set(['Monte Carlo VaR', 'CVaR / Expected Shortfall', 'Volatility', 'Downside deviation', 'Semi-deviation', 'Skewness', 'Kurtosis', 'Maximum drawdown', 'Average drawdown', 'Ulcer index', 'Sharpe ratio', 'Sortino ratio', 'Calmar ratio', 'Information ratio', 'Omega ratio', 'Sterling ratio', 'Burke ratio', 'Marginal VaR', 'Component VaR', 'Incremental VaR', 'Beta', 'Tracking error', 'Diversification ratio', 'HHI concentration']);
const metricGroups = [
  { name: 'Market risk', metrics: ['VaR — historical', 'VaR — parametric', 'Monte Carlo VaR', 'CVaR / Expected Shortfall', 'Marginal VaR', 'Component VaR', 'Incremental VaR', 'Stressed VaR', 'Relative VaR / Tracking Error VaR', 'Volatility (standard deviation)', 'Downside deviation / semi-deviation', 'Maximum drawdown', 'Average drawdown', 'Drawdown duration', 'Ulcer index', 'Skewness', 'Kurtosis', 'Beta', 'Tracking error'] },
  { name: 'Risk adjusted performance', metrics: ['Sharpe ratio', 'Sortino ratio', 'Treynor ratio', 'Calmar ratio', 'Information ratio', 'Omega ratio', 'Sterling ratio', 'Burke ratio'] },
  { name: 'Options and derivatives', metrics: ['Delta', 'Gamma', 'Vega', 'Theta', 'Rho', 'Vanna', 'Vomma', 'Charm', 'Implied volatility'] },
  { name: 'Credit risk', metrics: ['Probability of default (PD)', 'Loss given default (LGD)', 'Exposure at default (EAD)', 'Expected credit loss (ECL)', 'Credit VaR', 'Credit spread', 'Default correlation', 'Credit migration / transition risk'] },
  { name: 'Liquidity risk', metrics: ['Liquidity adjusted VaR (LVaR)', 'Bid ask spread', 'Market impact cost', 'Liquidity coverage ratio (LCR)', 'Net stable funding ratio (NSFR)', 'Amihud illiquidity ratio'] },
  { name: 'Interest rate and duration risk', metrics: ['Macaulay duration', 'Modified duration', 'Convexity', 'DV01 / PV01', 'Key rate duration', 'Basis point value (BPV)'] },
  { name: 'Portfolio and correlation risk', metrics: ['Covariance matrix', 'Correlation matrix', 'Diversification ratio', 'Concentration risk (HHI)', 'Factor exposure (Fama–French)', 'Factor exposure (PCA)'] },
  { name: 'Operational and regulatory risk', metrics: ['Risk weighted assets (RWA)', 'Capital adequacy ratio (CAR)', 'Economic capital', 'Basel III capital buffers', 'Leverage ratio'] },
  { name: 'Systemic risk', metrics: ['CoVaR', 'Marginal expected shortfall (MES)', 'SRISK', 'Network contagion metrics'] },
];

export default function RiskLabPage() {
  const reduce = useReducedMotion();
  return (
    <div className="mx-auto max-w-7xl space-y-7 p-5 sm:p-8">
      <header>
        <p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-primary">Analytics</p>
        <h1 className="text-3xl font-semibold tracking-tight">Risk lab</h1>
        <p className="mt-2 max-w-3xl text-sm leading-6 text-foreground-muted">Run the risk measures currently exposed by the engine. Measures requiring market histories, benchmarks, multiple asset return streams, derivatives, or credit and balance sheet inputs are listed below as data model extensions.</p>
      </header>
      <RiskCalculator />
      <section className="rounded-xl border border-border bg-surface p-5 sm:p-6">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-2"><div><h2 className="font-semibold">Metric coverage</h2><p className="mt-1 text-sm text-foreground-muted">Coverage depends on the available inputs and model, not just the metric name.</p></div><span className="rounded-full border border-accent-warning/30 bg-accent-warning/10 px-3 py-1 text-xs text-accent-warning">Model depth varies</span></div>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {metricGroups.map((group, index) => <motion.article key={group.name} initial={reduce ? false : { opacity: 0, y: 14, rotateX: 3 }} animate={{ opacity: 1, y: 0, rotateX: 0 }} transition={{ delay: index * 0.045, duration: 0.38 }} whileHover={reduce ? undefined : { y: -3, rotateX: 1.5 }} style={{ transformPerspective: 800 }} className="rounded-lg border border-border-subtle bg-background p-4 transition-colors hover:border-accent-primary/30"><h3 className="text-sm font-medium">{group.name}</h3><ul className="mt-3 space-y-2">{group.metrics.map((metric) => <li key={metric} className="flex items-start gap-2 text-xs leading-5"><span className={`mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full ${implemented.has(metric) ? 'bg-accent-success' : 'bg-foreground-subtle'}`} /><span className="text-foreground-muted">{metric}</span><span className="ml-auto shrink-0 text-[10px] text-foreground-subtle">{implemented.has(metric) ? 'kernel' : 'needs model'}</span></li>)}</ul></motion.article>)}
        </div>
        <p className="mt-4 rounded-lg bg-accent-warning/5 p-3 text-xs leading-5 text-foreground-subtle">The current Monte Carlo engine simulates a single aggregate GBM position. Portfolio contribution metrics therefore do not yet represent true multi-asset decomposition; market-sensitive measures need aligned benchmark/history inputs, and credit, options and regulatory measures need dedicated models.</p>
      </section>
    </div>
  );
}
