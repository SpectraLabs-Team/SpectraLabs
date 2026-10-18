// The engine's GBM terminal-loss kernel, in plain JavaScript, so a visitor's browser can be
// timed doing the same work as the Rust backends: draw z ~ N(0,1), compute
// loss = V − V·exp(−σ²T/2 + σ√T·z) per path, then sort a copy and take the VaR quantile.

export interface JsRunResult {
  paths: number;
  ms: number;
  var: number;
}

function xorshift32(seed: number) {
  let x = seed >>> 0 || 0x9e3779b9;
  return () => {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    return ((x >>> 0) + 0.5) / 4294967296;
  };
}

export function runJsKernel(paths: number, value: number, volatility: number, horizonDays: number, confidence = 0.99, seed = 42): JsRunResult {
  const t0 = performance.now();
  const t = horizonDays / 252;
  const s = volatility * Math.sqrt(t);
  const drift = -0.5 * s * s;
  const rand = xorshift32(seed);
  const losses = new Float64Array(paths);
  for (let i = 0; i < paths; i += 2) {
    // Box–Muller: two normals per pair of uniforms.
    const r = Math.sqrt(-2 * Math.log(rand()));
    const th = 2 * Math.PI * rand();
    losses[i] = value - value * Math.exp(drift + s * r * Math.cos(th));
    if (i + 1 < paths) losses[i + 1] = value - value * Math.exp(drift + s * r * Math.sin(th));
  }
  const sorted = losses.slice().sort();
  const rank = confidence * (paths - 1);
  const lo = Math.floor(rank);
  const hi = Math.min(Math.ceil(rank), paths - 1);
  const v = sorted[lo] + (rank - lo) * (sorted[hi] - sorted[lo]);
  return { paths, ms: performance.now() - t0, var: v };
}

/** Median of `repetitions` timed runs after `warmups` untimed ones; yields to the UI between runs. */
export async function benchmarkJs(paths: number, value: number, volatility: number, repetitions = 3, warmups = 1) {
  const yieldToUi = () => new Promise((r) => setTimeout(r, 0));
  for (let i = 0; i < warmups; i++) {
    runJsKernel(paths, value, volatility, 1);
    await yieldToUi();
  }
  const samples: number[] = [];
  for (let i = 0; i < repetitions; i++) {
    samples.push(runJsKernel(paths, value, volatility, 1, 0.99, 42 + i).ms);
    await yieldToUi();
  }
  samples.sort((a, b) => a - b);
  const p50 = samples[Math.ceil(samples.length / 2) - 1];
  return { p50, samples, pathsPerSecond: (paths * 1000) / p50 };
}
