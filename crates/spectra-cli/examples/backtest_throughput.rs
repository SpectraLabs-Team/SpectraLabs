//! Benchmark: engine throughput in bars per second, with the built-in
//! SMA crossover on synthetic multi-instrument data.
//!
//! ```text
//! cargo run --release -p spectra-cli --example backtest_throughput -- [--bars N] [--instruments K]
//!                                                                     [--repeat R] [--min-bars-per-sec X]
//! ```
//!
//! Prints the best of R runs. With `--min-bars-per-sec`, exits 1 when the best
//! run is slower: CI uses a floor far below a normal result, so it catches an
//! accidental slowdown of an order of magnitude (such as the quadratic order
//! scan the C++ engine had) without failing on a noisy shared runner.

use std::process::ExitCode;
use std::time::Instant;

use spectra_backtest::{builtin, Preset};
use spectra_core::{Bar, Price, Qty, Rng, SCALE};
use spectra_engine::run_backtest;
use spectra_research::ParamSet;

fn bars(n: usize, instruments: u32, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = vec![100 * SCALE; instruments as usize];
    let mut out = Vec::with_capacity(n * instruments as usize);
    for i in 0..n {
        for id in 0..instruments {
            let k = id as usize;
            let open = px[k];
            let close = (open + rng.range(-SCALE, SCALE + SCALE / 50)).max(SCALE);
            px[k] = close;
            out.push(Bar {
                ts_ns: (i as i64 + 1) * 60_000_000_000,
                instrument: id + 1,
                open: Price::from_raw(open),
                high: Price::from_raw(open.max(close) + rng.range(0, SCALE / 4)),
                low: Price::from_raw((open.min(close) - rng.range(0, SCALE / 4)).max(1)),
                close: Price::from_raw(close),
                volume: Qty::from_raw(1_000_000 * SCALE),
            });
        }
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |flag: &str, default: f64| -> f64 {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let (n, k, repeat) =
        (get("--bars", 200_000.0) as usize, get("--instruments", 5.0) as u32, get("--repeat", 3.0) as usize);
    let floor = get("--min-bars-per-sec", 0.0);

    let data = bars(n, k, 42);
    let strategy = builtin("sma_cross").expect("built-in");
    let params = ParamSet::from_pairs([("fast", 10.0), ("slow", 50.0), ("qty", 10.0)]);
    let cfg = Preset::Default.config();

    let mut best = f64::INFINITY;
    let mut fills = 0;
    for _ in 0..repeat.max(1) {
        let mut s = strategy.build(&params).expect("valid params");
        let t = Instant::now();
        match run_backtest(&data, s.as_mut(), &cfg) {
            Ok(r) => fills = r.fill_count(),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(1);
            }
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    let rate = data.len() as f64 / best;
    println!("{} bars ({n} timestamps x {k} instruments), {fills} fills", data.len());
    println!("best of {repeat}: {best:.3} s, {rate:.0} bars/s");
    if floor > 0.0 && rate < floor {
        eprintln!("FAIL: {rate:.0} bars/s is below the floor of {floor:.0}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
