#!/usr/bin/env python3
"""Rust vs NumPy baseline for the landing-page benchmark chart.

Both sides run the same model on the same machine: the engine's GBM terminal-loss
kernel (one aggregate position, zero drift, 1-day horizon), then sort a copy of the
losses and take the 99% quantile, as spectra-risk does for VaR.

  Rust : `spectra-risk benchmark` (median wall time of the faster of CPU / Parallel)
  NumPy: vectorised standard_normal -> exp -> sort -> interpolated quantile
  Memory: whole-process peak RSS of one VaR run (median of 3), measured with wait4().
          This includes each runtime's fixed overhead (the Python interpreter + NumPy,
          the Rust binary), which is part of what a user actually pays.

Usage (from the repo root, after `cargo build --release -p spectra-cli`):
    python3 web/bench/bench_baseline.py > web/frontend/lib/benchmarkBaseline.json
"""
import json
import os
import platform
import statistics
import subprocess
import sys
import time

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CLI = os.path.join(ROOT, "target", "release", "spectra-risk")
CSV = os.path.join(ROOT, "examples", "sample_portfolio.csv")
SIZES = [10_000, 100_000, 500_000, 1_000_000]
REPS, WARMUPS = 5, 1
VALUE, SIGMA, DAYS = 478_500.0, 0.20, 1


def numpy_var(n, seed=42):
    s = SIGMA * (DAYS / 252) ** 0.5
    z = np.random.default_rng(seed).standard_normal(n)
    losses = VALUE - VALUE * np.exp(-0.5 * s * s + s * z)
    srt = np.sort(losses)
    return float(np.quantile(srt, 0.99))


def time_numpy(n):
    for _ in range(WARMUPS):
        numpy_var(n)
    samples = []
    for i in range(REPS):
        t0 = time.perf_counter()
        numpy_var(n, 42 + i)
        samples.append((time.perf_counter() - t0) * 1000)
    return statistics.median(samples)


def time_rust(n):
    out = subprocess.run(
        [CLI, "benchmark", CSV, "--simulations", str(n), "--repetitions", str(REPS), "--warmups", str(WARMUPS)],
        check=True, capture_output=True, text=True,
    ).stdout
    rows = {b["requested_backend"]: b for b in json.loads(out)["benchmarks"] if b["status"] == "available"}
    return {k: rows[k]["latency_ms"]["p50"] for k in ("CPU", "Parallel")}


def peak_rss_mb(argv):
    """Peak resident set size of a child process, in MB."""
    pid = os.fork()
    if pid == 0:
        devnull = os.open(os.devnull, os.O_WRONLY)
        os.dup2(devnull, 1)
        os.execvp(argv[0], argv)
    _, _, ru = os.wait4(pid, 0)
    kb = ru.ru_maxrss / (1024 if sys.platform == "darwin" else 1)  # macOS reports bytes
    return kb / 1024


def rust_mem(n):
    return peak_rss_mb([CLI, "risk", CSV, "--simulations", str(n), "--horizon-days", "1", "--backend", "auto"])


def numpy_mem(n):
    return peak_rss_mb([sys.executable, "-c", f"import sys; sys.path.insert(0, {os.path.dirname(__file__)!r}); import bench_baseline as b; b.numpy_var({n})"])


def main():
    med3 = lambda f, n: statistics.median(f(n) for _ in range(3))
    rows = []
    for n in SIZES:
        r = time_rust(n)
        best = min(r.values())
        npm = time_numpy(n)
        rows.append({
            "paths": n,
            "rust_ms": round(best, 4),
            "rust_cpu_ms": round(r["CPU"], 4),
            "rust_parallel_ms": round(r["Parallel"], 4),
            "numpy_ms": round(npm, 4),
            "rust_mem_mb": round(med3(rust_mem, n), 1),
            "numpy_mem_mb": round(med3(numpy_mem, n), 1),
        })
        print(f"{n:>9,}  rust {best:8.3f} ms   numpy {npm:8.3f} ms", file=sys.stderr)
    cpu = platform.processor() or platform.machine()
    try:
        with open("/proc/cpuinfo") as f:
            cpu = next(l.split(":", 1)[1].strip() for l in f if l.startswith("model name"))
    except (OSError, StopIteration):
        pass
    rustc = subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip()
    json.dump({
        "measured": time.strftime("%Y-%m-%d"),
        "host": {"cpu": cpu, "threads": os.cpu_count(), "os": platform.system(), "numpy": np.__version__, "python": platform.python_version(), "rustc": rustc},
        "method": {"memory": "whole-process peak RSS, median of 3", "repetitions": REPS, "warmups": WARMUPS, "horizon_days": DAYS, "confidence": 0.99, "portfolio": "examples/sample_portfolio.csv"},
        "rows": rows,
    }, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
