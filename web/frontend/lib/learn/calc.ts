/**
 * Metric calculations for the Learning Center's interactive examples.
 *
 * These are line-for-line ports of the engine's definitions so the examples match what a backtest
 * reports:
 *   - equityMetrics:   spectra_engine::metrics::compute_metrics_from_equity
 *   - drawdownStats:   spectra_engine::metrics::compute_drawdown_stats
 *   - sortino:         spectra_engine::performance::sortino
 *   - tradeStats:      spectra_research::trades::compute_trade_stats
 *   - effectiveBps:    spectra_engine::costs::effective_slippage_bps (without market impact)
 *   - tailStats:       spectra_engine::metrics::compute_tail_stats (historical VaR / ES)
 * scripts/test-learn.mjs checks them against the engine's own unit-test values and against the
 * summary of a real engine run (scripts/fixtures/learn-engine-run.json).
 */

export interface EquityMetrics {
  valid: boolean;
  periods: number;
  totalReturn: number;
  annualizedReturn: number;
  annualizedVolatility: number;
  sharpe: number;
  maxDrawdown: number;
  sortino: number | null;
}

export function returnsFromEquity(eq: number[]): number[] {
  const out: number[] = [];
  for (let i = 1; i < eq.length; i++) if (eq[i - 1] > 0) out.push(eq[i] / eq[i - 1] - 1);
  return out;
}

const mean = (x: number[]) => (x.length ? x.reduce((a, b) => a + b, 0) / x.length : 0);

export function sortino(r: number[], ppy: number, rf: number): number | null {
  if (ppy <= 0 || r.length === 0) return null;
  const target = rf / ppy;
  const dd = Math.sqrt(r.reduce((s, x) => s + Math.min(x - target, 0) ** 2, 0) / r.length);
  return dd > 0 ? ((mean(r) - target) / dd) * Math.sqrt(ppy) : null;
}

export function equityMetrics(eq: number[], ppy: number, rf: number): EquityMetrics {
  const m: EquityMetrics = { valid: false, periods: 0, totalReturn: 0, annualizedReturn: 0, annualizedVolatility: 0, sharpe: 0, maxDrawdown: 0, sortino: null };
  if (eq.length < 2 || ppy <= 0) return m;
  m.valid = true;
  const r = returnsFromEquity(eq);
  let peak = eq[0];
  let maxDd = 0;
  for (const e of eq.slice(1)) {
    if (e > peak) peak = e;
    if (peak > 0) maxDd = Math.max(maxDd, (peak - e) / peak);
  }
  m.maxDrawdown = maxDd;
  m.periods = eq.length - 1;
  const first = eq[0];
  const last = eq[eq.length - 1];
  if (first > 0) {
    m.totalReturn = last / first - 1;
    const base = 1 + m.totalReturn;
    m.annualizedReturn = base > 0 ? base ** (ppy / m.periods) - 1 : -1;
  }
  if (r.length >= 2) {
    const mu = mean(r);
    const sd = Math.sqrt(r.reduce((s, x) => s + (x - mu) ** 2, 0) / (r.length - 1));
    m.annualizedVolatility = sd * Math.sqrt(ppy);
    if (sd > 0) m.sharpe = ((mu - rf / ppy) / sd) * Math.sqrt(ppy);
  }
  m.sortino = sortino(r, ppy, rf);
  return m;
}

export interface DrawdownStats {
  valid: boolean;
  maxDrawdown: number;
  currentDrawdown: number;
  longestUnderwater: number;
  ulcerIndex: number;
}

export function drawdownStats(eq: number[]): DrawdownStats {
  const d: DrawdownStats = { valid: false, maxDrawdown: 0, currentDrawdown: 0, longestUnderwater: 0, ulcerIndex: 0 };
  if (eq.length < 2) return d;
  d.valid = true;
  let peak = eq[0];
  let run = 0;
  let sq = 0;
  for (const e of eq) {
    if (e > peak) peak = e;
    const dd = peak > 0 ? (peak - e) / peak : 0;
    d.maxDrawdown = Math.max(d.maxDrawdown, dd);
    sq += dd * dd;
    if (e < peak) {
      run += 1;
      d.longestUnderwater = Math.max(d.longestUnderwater, run);
    } else run = 0;
    d.currentDrawdown = dd;
  }
  d.ulcerIndex = Math.sqrt(sq / eq.length);
  return d;
}

/** Nearest-rank index into an ascending sample of size n for the p-quantile (engine: nearest_rank_index). */
export function nearestRankIndex(n: number, p: number): number {
  if (n === 0) return 0;
  const rank = Math.ceil(p * n - 1e-9);
  return Math.max(0, Math.min(n - 1, rank - 1));
}

/** Historical VaR and expected shortfall at `level` as positive loss fractions; needs 20 returns. */
export function tailStats(returns: number[], level = 0.95): { valid: boolean; var: number; es: number; worst: number } {
  if (returns.length < 20 || !(level > 0 && level < 1)) return { valid: false, var: 0, es: 0, worst: 0 };
  const r = [...returns].sort((a, b) => a - b);
  const idx = nearestRankIndex(r.length, 1 - level);
  let sum = 0;
  for (let i = 0; i <= idx; i++) sum += r[i];
  return { valid: true, var: Math.max(-r[idx], 0), es: Math.max(-(sum / (idx + 1)), 0), worst: r[0] };
}

export interface TradeStats {
  n: number;
  wins: number;
  losses: number;
  winRate: number;
  avgWin: number;
  avgLoss: number;
  payoffRatio: number;
  expectancy: number;
  /** Infinity when there are wins and no losses (the results page shows "—"). */
  profitFactor: number;
  totalNet: number;
  best: number;
  worst: number;
}

export function tradeStats(pnl: number[]): TradeStats {
  const st: TradeStats = { n: pnl.length, wins: 0, losses: 0, winRate: 0, avgWin: 0, avgLoss: 0, payoffRatio: 0, expectancy: 0, profitFactor: 0, totalNet: 0, best: 0, worst: 0 };
  if (!pnl.length) return st;
  let winSum = 0;
  let lossSum = 0;
  st.best = -Infinity;
  st.worst = Infinity;
  for (const v of pnl) {
    st.totalNet += v;
    st.best = Math.max(st.best, v);
    st.worst = Math.min(st.worst, v);
    if (v > 0) {
      st.wins += 1;
      winSum += v;
    } else if (v < 0) {
      st.losses += 1;
      lossSum += -v;
    }
  }
  st.winRate = st.wins / st.n;
  st.expectancy = st.totalNet / st.n;
  if (st.wins) st.avgWin = winSum / st.wins;
  if (st.losses) st.avgLoss = lossSum / st.losses;
  if (st.wins && st.losses) st.payoffRatio = st.avgWin / st.avgLoss;
  if (lossSum > 0) st.profitFactor = winSum / lossSum;
  else if (winSum > 0) st.profitFactor = Infinity;
  return st;
}

/** Effective slippage in bps for a market or stop fill: flat + half spread + volume participation. */
export function effectiveBps(slippageBps: number, spreadBps: number, volumeSlippageBps: number, qty: number, barVolume: number): number {
  let bps = slippageBps;
  if (spreadBps > 0) bps += spreadBps / 2;
  if (volumeSlippageBps > 0 && barVolume > 0) bps += volumeSlippageBps * Math.min(1, qty / barVolume);
  return bps;
}

/** Commission on one fill: max(min fee, per unit × qty + notional × bps / 10,000). */
export function commission(perUnit: number, bps: number, minFee: number, qty: number, notional: number): number {
  return Math.max(minFee, perUnit * qty + (notional * bps) / 10_000);
}

/**
 * Parse numbers from free text ("100, 105 98.5\n110", "$100,000 $102,500"). A comma followed by
 * exactly three digits is a thousands separator; any other comma separates values.
 */
export function parseNumbers(text: string): number[] {
  const cleaned = text.replace(/(\d),(?=\d{3}(?!\d))/g, '$1');
  return (cleaned.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number).filter((n) => Number.isFinite(n));
}
