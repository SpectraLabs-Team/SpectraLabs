import data from './benchmarkBaseline.json';

// Reference run produced by web/bench/bench_baseline.py: Rust (spectra-risk) and NumPy
// running the same VaR kernel on the same machine. Regenerate on your own hardware with
//   python3 web/bench/bench_baseline.py > web/frontend/lib/benchmarkBaseline.json

export interface BaselineRow {
  paths: number;
  rust_ms: number;
  rust_cpu_ms: number;
  rust_parallel_ms: number;
  numpy_ms: number;
  rust_mem_mb: number;
  numpy_mem_mb: number;
}

export const BASELINE = data as {
  measured: string;
  host: { cpu: string; threads: number; os: string; numpy: string; python: string; rustc: string };
  method: Record<string, unknown>;
  rows: BaselineRow[];
};

export const pps = (paths: number, ms: number) => (paths * 1000) / ms;

const by = (n: number) => BASELINE.rows.find((r) => r.paths === n);
const largest = BASELINE.rows[BASELINE.rows.length - 1];

/** Headline figures derived from the reference run (never hard-coded). */
export const HEADLINE = {
  smallMs: by(10_000)?.rust_ms ?? null,
  rustPps: pps(largest.paths, largest.rust_ms),
  speedupVsNumpy: largest.numpy_ms / largest.rust_ms,
  memoryRatio: largest.numpy_mem_mb / largest.rust_mem_mb,
  largestPaths: largest.paths,
};
