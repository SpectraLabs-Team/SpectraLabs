//! M24.1 task 5: equity parity. Every built-in strategy, run on equity data
//! through the request path, must give the same config and result
//! fingerprints as before the multi-asset instrument model existed.
//!
//! The pinned values below were recorded on `main` at `c1a3d41`, before any
//! M24 change. If one of them moves, an equity result changed.

use spectra_backtest::{run_request, BacktestRequest};
use spectra_core::{Bar, Json, Price, Qty, Rng, SCALE};
use spectra_engine::RunControl;

/// Three equities with mean-reverting noise, hourly bars, so every built-in strategy trades.
fn bars(n: usize, instruments: u32, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = vec![50 * SCALE; instruments as usize];
    let mut out = Vec::new();
    for i in 0..n {
        for id in 0..instruments {
            let k = id as usize;
            let open = px[k];
            let pull = (50 * SCALE - open) / 20;
            let close = (open + pull + rng.range(-SCALE, SCALE)).max(SCALE);
            px[k] = close;
            out.push(Bar::new(
                (i as i64 + 1) * 3_600_000_000_000,
                id,
                Price::from_raw(open),
                Price::from_raw(open.max(close) + rng.range(0, SCALE / 4)),
                Price::from_raw((open.min(close) - rng.range(0, SCALE / 4)).max(1)),
                Price::from_raw(close),
                Qty::from_raw(1_000_000 * SCALE),
            ));
        }
    }
    out
}

fn run(doc: &str) -> (String, String) {
    let req = BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap();
    let out = run_request(&bars(400, 3, 7), &req, &mut RunControl::default()).unwrap();
    (out.result.run.config_sha256, out.result.run.result_sha256)
}

const CASES: &[(&str, &str)] = &[
    ("buy_and_hold", r#"{"strategy":"buy_and_hold"}"#),
    ("sma_cross", r#"{"strategy":"sma_cross","params":{"fast":5,"slow":20}}"#),
    ("zscore_reversion", r#"{"strategy":"zscore_reversion"}"#),
    ("equal_weight", r#"{"strategy":"equal_weight","params":{"rebalance_every":10}}"#),
    (
        "short_financed",
        r#"{"strategy":"zscore_reversion","config":{"allow_short":true,"borrow_fee_rate":0.02,"cash_interest_rate":0.01}}"#,
    ),
    (
        "multi",
        r#"{"strategies":[{"strategy":"sma_cross","weight":0.5},{"strategy":"equal_weight","weight":0.5}]}"#,
    ),
];

/// `(case, config_sha256, result_sha256)` recorded on `main` at `c1a3d41`.
const PINNED: &[(&str, &str, &str)] = &[
    (
        "buy_and_hold",
        "0d2c8cb38dedd39c870e84656fcb93ae6faf69c2b1fc6d67f1dadfcd94e9ed65",
        "c0057a0928d7d8cdbaab601efe7309cdf3291a75af10e4bdddec80bcf36d31fc",
    ),
    (
        "sma_cross",
        "0d2c8cb38dedd39c870e84656fcb93ae6faf69c2b1fc6d67f1dadfcd94e9ed65",
        "a773da897e8c2e5b43ba5ef35a34b6e974caa25324ad640e74664114a170c755",
    ),
    (
        "zscore_reversion",
        "0d2c8cb38dedd39c870e84656fcb93ae6faf69c2b1fc6d67f1dadfcd94e9ed65",
        "197593564d41a118b0b6153e18c6f0fb880126ebd43d5a04cba440c6d0a9a920",
    ),
    (
        "equal_weight",
        "0d2c8cb38dedd39c870e84656fcb93ae6faf69c2b1fc6d67f1dadfcd94e9ed65",
        "e73841c906146f3181efc3f3851ae62b0f695d7256883bc5b537b8ea6427bae4",
    ),
    (
        "short_financed",
        "52b7294d2a804db1df9a585336a4669a9b654f05ad2b95e59c272f762fdba716",
        "3787b3d23c790843d3a7beb18f2b6f3107375aadc857d6f6be0104f4cc0de011",
    ),
    (
        "multi",
        "6a07a05adafc5da75510a4a4495313541fb517b6ec3409886dea5e811d110f2e",
        "6d435c72ca839d080864a3d5fa1f3922273dda21c6bf589ccaccae7b8a95ffcb",
    ),
];

#[test]
fn equity_fingerprints_are_unchanged() {
    for (name, config, result) in PINNED {
        let doc = CASES.iter().find(|(n, _)| n == name).unwrap().1;
        let (c, r) = run(doc);
        assert_eq!(&c, config, "{name}: config fingerprint changed");
        assert_eq!(&r, result, "{name}: result fingerprint changed");
    }
}
