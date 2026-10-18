import type { Position } from './types';
import { DEFAULT_VOLATILITY } from './model';

/** examples/sample_portfolio.csv — used when the engine is offline. */
export const SAMPLE_CSV = 'symbol,quantity,price\nAAPL,1000,190.00\nMSFT,500,420.00\nGOOGL,300,175.00\nNVDA,200,130.00\n';

export interface ParseResult {
  positions: Position[];
  errors: string[];
}

/**
 * Parses `symbol,quantity,price[,volatility]` with the same rules as the engine's
 * Portfolio::from_csv_str: first non-empty line is a header, rows need ≥ 3 fields,
 * numbers must be finite, volatility must be ≥ 0 (blank = default).
 */
export function parseCsv(text: string): ParseResult {
  const positions: Position[] = [];
  const errors: string[] = [];
  const lines = text.split('\n').map((l) => l.replace(/\r$/, ''));
  let header = true;
  lines.forEach((line, i) => {
    if (!line) return;
    if (header) {
      header = false;
      return;
    }
    const f = line.split(',');
    if (f.length < 3) return void errors.push(`Line ${i + 1}: expected symbol,quantity,price`);
    const quantity = Number(f[1].trim());
    const price = Number(f[2].trim());
    if (f[1].trim() === '' || f[2].trim() === '' || !Number.isFinite(quantity) || !Number.isFinite(price)) {
      return void errors.push(`Line ${i + 1}: non-numeric quantity or price`);
    }
    let volatility: number | undefined;
    const v = f[3]?.trim();
    if (v) {
      volatility = Number(v);
      if (!Number.isFinite(volatility) || volatility < 0) return void errors.push(`Line ${i + 1}: invalid volatility`);
    }
    if (!f[0].trim()) return void errors.push(`Line ${i + 1}: empty symbol`);
    positions.push({ symbol: f[0].trim(), quantity, price, ...(volatility !== undefined && volatility > 0 ? { volatility } : {}) });
  });
  return { positions, errors };
}

export function toCsv(positions: Position[]): string {
  const withVol = positions.some((p) => p.volatility !== undefined);
  const head = withVol ? 'symbol,quantity,price,volatility' : 'symbol,quantity,price';
  const rows = positions.map((p) =>
    [p.symbol, p.quantity, p.price, ...(withVol ? [p.volatility ?? ''] : [])].join(','),
  );
  return [head, ...rows].join('\n') + '\n';
}

export const positionValue = (p: Position) => p.quantity * p.price;

export interface PortfolioStats {
  netValue: number;
  grossExposure: number;
  weights: number[];
  hhi: number;
  effectiveN: number;
  /** Engine aggregate volatility: simple average of asset vols, default for missing ones. */
  modelVolatility: number;
}

export function portfolioStats(positions: Position[]): PortfolioStats {
  const values = positions.map(positionValue);
  const netValue = values.reduce((a, v) => a + v, 0);
  const grossExposure = values.reduce((a, v) => a + Math.abs(v), 0);
  const weights = grossExposure > 0 ? values.map((v) => Math.abs(v) / grossExposure) : values.map(() => 0);
  const hhi = weights.reduce((a, w) => a + w * w, 0);
  const modelVolatility = positions.length
    ? positions.reduce((a, p) => a + (p.volatility && p.volatility > 0 ? p.volatility : DEFAULT_VOLATILITY), 0) / positions.length
    : DEFAULT_VOLATILITY;
  return { netValue, grossExposure, weights, hhi, effectiveN: hhi > 0 ? 1 / hhi : 0, modelVolatility };
}

/** Every price × factor and every vol × volMultiplier (missing vols start from the 20% default). */
export function shockPositions(positions: Position[], priceFactor: number, volMultiplier = 1): Position[] {
  return positions.map((p) => ({
    ...p,
    price: p.price * priceFactor,
    ...(volMultiplier !== 1 ? { volatility: (p.volatility ?? DEFAULT_VOLATILITY) * volMultiplier } : {}),
  }));
}
