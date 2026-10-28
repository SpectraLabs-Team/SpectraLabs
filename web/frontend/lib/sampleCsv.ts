// Pure, import-free so node tests can load it. See lib/sampleData.ts.
/**
 * Built-in sample dataset, so a fresh workspace can run its first backtest without finding a CSV.
 *
 * The prices are SYNTHETIC: a seeded random walk (geometric Brownian motion) for three made-up
 * symbols. They are not market data and the symbols are not real tickers; the dataset name says so.
 * The same seed always produces the same file, so runs on it are reproducible.
 */
export const SAMPLE_DATASET_NAME = 'Sample data (synthetic, not real prices)';
export const SAMPLE_FILENAME = 'spectra-sample-daily-synthetic.csv';

/** Made-up symbols with a starting price, drift and volatility (annualized) each. */
export const SAMPLE_SYMBOLS = [
  { symbol: 'DEMOA', start: 100, drift: 0.08, vol: 0.18 },
  { symbol: 'DEMOB', start: 50, drift: 0.03, vol: 0.3 },
  { symbol: 'DEMOC', start: 200, drift: 0.05, vol: 0.12 },
] as const;

/** Two years of weekday bars ending on this date (fixed, so the file never changes). */
export const FIRST_DAY = Date.UTC(2023, 0, 2);
const BARS = 504;

/** Header names in the CSV; lib/sampleData.ts maps them in its ingest settings. */
export const SAMPLE_COLUMNS = { timestamp: 'timestamp', open: 'open', high: 'high', low: 'low', close: 'close', volume: 'volume', symbol: 'symbol' } as const;

/** Small seeded PRNG (mulberry32), so the file is identical on every machine. */
function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Standard normal from two uniforms (Box–Muller). */
function normal(next: () => number) {
  const u = Math.max(next(), 1e-12);
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * next());
}

/** Weekday dates (UTC), BARS of them, starting at FIRST_DAY. */
export function sampleDays(): string[] {
  const days: string[] = [];
  for (let t = FIRST_DAY; days.length < BARS; t += 86_400_000) {
    const wd = new Date(t).getUTCDay();
    if (wd !== 0 && wd !== 6) days.push(new Date(t).toISOString().slice(0, 10));
  }
  return days;
}

const r2 = (x: number) => Math.round(x * 100) / 100;

/** The sample CSV: symbol,timestamp,open,high,low,close,volume, daily bars at 21:00 UTC. */
export function buildSampleCsv(seed = 42): string {
  const days = sampleDays();
  const lines = ['symbol,timestamp,open,high,low,close,volume'];
  SAMPLE_SYMBOLS.forEach((s, i) => {
    const next = rng(seed + i * 1009);
    const dt = 1 / 252;
    let close: number = s.start;
    for (const day of days) {
      const open = r2(close * (1 + normal(next) * s.vol * Math.sqrt(dt) * 0.25));
      const c = r2(open * Math.exp((s.drift - 0.5 * s.vol ** 2) * dt + s.vol * Math.sqrt(dt) * normal(next)));
      const wick = s.vol * Math.sqrt(dt) * 0.5;
      const high = r2(Math.max(open, c) * (1 + Math.abs(normal(next)) * wick));
      const low = r2(Math.min(open, c) * (1 - Math.abs(normal(next)) * wick));
      const volume = Math.round(400_000 + next() * 1_200_000);
      lines.push(`${s.symbol},${day}T21:00:00Z,${open.toFixed(2)},${high.toFixed(2)},${low.toFixed(2)},${c.toFixed(2)},${volume}`);
      close = c;
    }
  });
  return `${lines.join('\n')}\n`;
}

