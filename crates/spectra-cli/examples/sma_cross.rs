//! Demo: SMA(10/30) crossover on synthetic data.
//! Run with: `cargo run --release -p spectra-cli --example sma_cross`

use std::process::ExitCode;

use spectra_core::{Bar, Bps, Money, Price, Qty, Rng, Side, ENGINE_VERSION, SCALE};
use spectra_engine::{run_backtest, BacktestConfig, Context, Strategy};

fn synthetic_bars(n: usize, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = 100 * SCALE;
    (0..n)
        .map(|i| {
            let open = px;
            // Mild upward drift plus noise.
            let close = (open + rng.range(-SCALE * 3 / 2, SCALE * 3 / 2 + SCALE / 20)).max(SCALE);
            let high = open.max(close) + rng.range(0, SCALE / 2);
            let low = (open.min(close) - rng.range(0, SCALE / 2)).max(1);
            let volume = rng.range(100_000, 1_000_000) * SCALE;
            px = close;
            Bar {
                ts_ns: (i as i64 + 1) * 86_400_000_000_000,
                known_at_ns: (i as i64 + 1) * 86_400_000_000_000,
                instrument: 1,
                open: Price::from_raw(open),
                high: Price::from_raw(high),
                low: Price::from_raw(low),
                close: Price::from_raw(close),
                volume: Qty::from_raw(volume),
            }
        })
        .collect()
}

struct SmaCross {
    fast: usize,
    slow: usize,
    size: Qty,
}

impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.slow.max(self.fast) {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let bullish = sma(self.fast) > sma(self.slow);
        let pos = ctx.position(id);
        if bullish && pos.is_zero() {
            ctx.market(id, Side::Buy, self.size);
        } else if !bullish && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn main() -> ExitCode {
    let bars = synthetic_bars(1000, 2026);
    let mut cfg = BacktestConfig {
        initial_cash: Money::from_raw(100_000 * SCALE),
        seed: 2026,
        ..BacktestConfig::default()
    };
    cfg.costs.slippage = Bps::from_f64(2.0).expect("finite");
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0).expect("finite");

    let mut strategy = SmaCross { fast: 10, slow: 30, size: Qty::from_raw(100 * SCALE) };
    let r = match run_backtest(&bars, &mut strategy, &cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let m = &r.metrics;
    println!("Spectra Labs {ENGINE_VERSION} - SMA(10/30) crossover on {} synthetic bars\n", bars.len());
    match r.ledger.equity() {
        Ok(eq) => println!("final equity        {eq}"),
        Err(e) => println!("final equity        unavailable ({e})"),
    }
    println!("total return        {:.2}%", m.total_return * 100.0);
    println!("annualized return   {:.2}%", m.annualized_return * 100.0);
    println!("annualized vol      {:.2}%", m.annualized_volatility * 100.0);
    println!("sharpe              {:.2}", m.sharpe);
    println!("max drawdown        {:.2}%", m.max_drawdown * 100.0);
    println!("fills               {}", r.fill_count());
    println!("fees paid           {}", r.ledger.total_fees());
    let ok = r.ledger.check_invariants();
    println!("ledger reconciles   {}\n", ok.as_ref().map_or_else(Clone::clone, |()| "yes".to_string()));
    println!("run record");
    println!(
        "  engine   {}\n  dataset  {}\n  config   {}\n  result   {}",
        r.run.engine_version, r.run.dataset_sha256, r.run.config_sha256, r.run.result_sha256
    );
    if ok.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
