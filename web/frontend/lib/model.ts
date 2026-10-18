// Closed-form version of the engine's risk model (crates/spectra-risk/src/simulation.rs):
// the book is ONE aggregate position worth V with volatility σ (simple average of asset
// vols, 20% default) and zero drift. Over a horizon of T years,
//   X = −σ²T/2 + σ√T·Z,   loss L = V·(1 − e^X),   Z ~ N(0, 1).
// The engine estimates this by Monte Carlo; the formulas below are its exact limits, used for
// instant previews and for drawing the loss density. Engine results are always labelled as such.

export const TRADING_DAYS = 252;
export const DEFAULT_VOLATILITY = 0.2;

/** Standard normal CDF (Abramowitz–Stegun 7.1.26 via erf; |error| < 1.5e-7). */
export function normCdf(x: number): number {
  const t = 1 / (1 + 0.3275911 * (Math.abs(x) / Math.SQRT2));
  const y =
    1 - ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * Math.exp(-(x * x) / 2);
  return x >= 0 ? 0.5 * (1 + y) : 0.5 * (1 - y);
}

export function normPdf(x: number): number {
  return Math.exp(-0.5 * x * x) / Math.sqrt(2 * Math.PI);
}

/** Inverse standard normal CDF (Acklam; relative error < 1.2e-9). */
export function normInv(p: number): number {
  if (p <= 0) return -Infinity;
  if (p >= 1) return Infinity;
  const a = [-39.69683028665376, 220.9460984245205, -275.9285104469687, 138.357751867269, -30.66479806614716, 2.506628277459239];
  const b = [-54.47609879822406, 161.5858368580409, -155.6989798598866, 66.80131188771972, -13.28068155288572];
  const c = [-0.007784894002430293, -0.3223964580411365, -2.400758277161838, -2.549732539343734, 4.374664141464968, 2.938163982698783];
  const d = [0.007784695709041462, 0.3224671290700398, 2.445134137142996, 3.754408661907416];
  const lo = 0.02425;
  if (p < lo) {
    const q = Math.sqrt(-2 * Math.log(p));
    return (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1);
  }
  if (p > 1 - lo) return -normInv(1 - p);
  const q = p - 0.5;
  const r = q * q;
  return ((((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q) / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1);
}

export interface ModelInput {
  /** Portfolio value V (sum of quantity × price). */
  value: number;
  /** Annualized aggregate volatility σ. */
  volatility: number;
  /** Horizon in trading days (T = days / 252). */
  horizonDays: number;
}

const params = ({ volatility, horizonDays }: ModelInput) => {
  const t = horizonDays / TRADING_DAYS;
  const s = volatility * Math.sqrt(t);
  return { s, m: -0.5 * s * s };
};

/** Loss quantile: the loss exceeded with probability 1 − p. */
export function lossQuantile(input: ModelInput, p: number): number {
  const { s, m } = params(input);
  return input.value * (1 - Math.exp(m + s * normInv(1 - p)));
}

export const analyticVaR = (input: ModelInput, confidence: number) => lossQuantile(input, confidence);

/** E[L | L ≥ VaR] = V·(1 − Φ(z₁₋α − s) / (1 − α)). */
export function analyticCVaR(input: ModelInput, confidence: number): number {
  const { s } = params(input);
  const z = normInv(1 - confidence);
  return input.value * (1 - normCdf(z - s) / (1 - confidence));
}

/** Standard deviation of the loss in dollars (what the engine's "Volatility" metric reports). */
export function analyticLossStd(input: ModelInput): number {
  const { s } = params(input);
  return Math.abs(input.value) * Math.sqrt(Math.expm1(s * s));
}

/** Density of L at loss l (l < V). */
export function lossDensity(input: ModelInput, l: number): number {
  const { s, m } = params(input);
  const g = 1 - l / input.value;
  if (g <= 0 || s <= 0) return 0;
  const x = (Math.log(g) - m) / s;
  return normPdf(x) / (s * Math.abs(input.value - l));
}

/** P(L ≥ l). */
export function tailProbability(input: ModelInput, l: number): number {
  const { s, m } = params(input);
  const g = 1 - l / input.value;
  if (g <= 0) return 0;
  return normCdf((Math.log(g) - m) / s);
}

export interface DensityPoint {
  loss: number;
  body: number | null;
  tail: number | null;
}

/** Points for a density chart, split at `cutoff` (normally the VaR) so the tail can be shaded. */
export function densityCurve(input: ModelInput, cutoff: number, points = 140): DensityPoint[] {
  const lo = lossQuantile(input, 0.0005);
  const hi = lossQuantile(input, 0.9995);
  const xs: number[] = [];
  for (let i = 0; i <= points; i++) xs.push(lo + ((hi - lo) * i) / points);
  if (cutoff > lo && cutoff < hi) xs.push(cutoff);
  xs.sort((a, b) => a - b);
  return xs.map((loss) => {
    const f = lossDensity(input, loss);
    return { loss, body: loss <= cutoff ? f : null, tail: loss >= cutoff ? f : null };
  });
}
