//! M26 task 5a: reference-value tests for the options model (Black-Scholes-Merton
//! with a continuous dividend yield, `src/options.rs`).
//!
//! The reference values were computed independently at 40 significant digits
//! (mpmath), with every Greek taken by numerical differentiation of the price
//! rather than from the closed forms used in the model. They agree with the
//! published values: Hull (S=K=100, T=1, r=5%, sigma=20%: call 10.4506, put
//! 5.5735; Example 15.6: call 4.76, put 0.81) and Haug (generalized BSM put
//! 2.4648). Reference document: `docs/models/options.md`.

use spectra_core::OptionRight;
use spectra_engine::{OptionInputs, OptionValue};

/// Absolute tolerance on prices and Greeks. The model agrees with the
/// references to about 2e-14; this leaves room for platform differences only.
const TOL: f64 = 1e-9;

struct Case {
    name: &'static str,
    spot: f64,
    strike: f64,
    years: f64,
    rate: f64,
    dividend_yield: f64,
    volatility: f64,
    call: OptionValue,
    put: OptionValue,
}

const fn ov(price: f64, delta: f64, gamma: f64, vega: f64, theta: f64, rho: f64) -> OptionValue {
    OptionValue { price, delta, gamma, vega, theta, rho }
}

fn cases() -> [Case; 3] {
    [
        Case {
            name: "Hull, at the money",
            spot: 100.0,
            strike: 100.0,
            years: 1.0,
            rate: 0.05,
            dividend_yield: 0.0,
            volatility: 0.2,
            call: ov(
                10.450_583_572_185_566,
                0.636_830_651_175_619_1,
                0.018_762_017_345_846_89,
                37.524_034_691_693_785,
                -6.414_027_546_438_196,
                53.232_481_545_376_34,
            ),
            put: ov(
                5.573_526_022_256_968,
                -0.363_169_348_824_380_96,
                0.018_762_017_345_846_89,
                37.524_034_691_693_785,
                -1.657_880_423_934_625_9,
                -41.890_460_904_695_06,
            ),
        },
        Case {
            name: "Hull, Example 15.6",
            spot: 42.0,
            strike: 40.0,
            years: 0.5,
            rate: 0.10,
            dividend_yield: 0.0,
            volatility: 0.2,
            call: ov(
                4.759_422_392_871_533,
                0.779_131_290_942_669,
                0.049_962_670_405_911_85,
                8.813_415_059_602_852,
                -4.559_092_194_592_627,
                13.982_045_913_360_281,
            ),
            put: ov(
                0.808_599_372_900_093_6,
                -0.220_868_709_057_331_05,
                0.049_962_670_405_911_85,
                8.813_415_059_602_852,
                -0.754_174_496_589_770_5,
                -5.042_542_576_653_999,
            ),
        },
        Case {
            name: "Haug, generalized BSM with dividend yield",
            spot: 100.0,
            strike: 95.0,
            years: 0.5,
            rate: 0.10,
            dividend_yield: 0.05,
            volatility: 0.2,
            call: ov(
                9.628_983_522_021_258,
                0.711_128_312_392_260_3,
                0.022_839_574_296_269_99,
                22.839_574_296_269_99,
                -7.160_658_069_013_174_5,
                30.741_923_858_602_384,
            ),
            put: ov(
                2.464_787_646_755_821,
                -0.264_181_599_636_072_4,
                0.022_839_574_296_269_99,
                22.839_574_296_269_99,
                -3.000_528_096_398_054_5,
                -14.441_473_805_181_53,
            ),
        },
    ]
}

fn inputs(c: &Case, right: OptionRight) -> OptionInputs {
    OptionInputs {
        right,
        spot: c.spot,
        strike: c.strike,
        years: c.years,
        rate: c.rate,
        dividend_yield: c.dividend_yield,
        volatility: c.volatility,
    }
}

fn check(label: &str, got: f64, want: f64) {
    assert!((got - want).abs() < TOL, "{label}: got {got}, reference {want}, diff {}", got - want);
}

#[test]
fn prices_and_greeks_match_the_references() {
    for c in &cases() {
        for (right, want) in [(OptionRight::Call, c.call), (OptionRight::Put, c.put)] {
            let got = inputs(c, right).value().unwrap();
            let l = |f: &str| format!("{} {right:?} {f}", c.name);
            check(&l("price"), got.price, want.price);
            check(&l("delta"), got.delta, want.delta);
            check(&l("gamma"), got.gamma, want.gamma);
            check(&l("vega"), got.vega, want.vega);
            check(&l("theta"), got.theta, want.theta);
            check(&l("rho"), got.rho, want.rho);
        }
    }
}

#[test]
fn prices_round_to_the_published_values() {
    let price = |i: usize, right| inputs(&cases()[i], right).value().unwrap().price;
    // Hull, at the money: 4 decimals.
    assert!((price(0, OptionRight::Call) - 10.4506).abs() < 0.5e-4);
    assert!((price(0, OptionRight::Put) - 5.5735).abs() < 0.5e-4);
    // Hull, Example 15.6: 2 decimals.
    assert!((price(1, OptionRight::Call) - 4.76).abs() < 0.5e-2);
    assert!((price(1, OptionRight::Put) - 0.81).abs() < 0.5e-2);
    // Haug, generalized BSM put: 4 decimals.
    assert!((price(2, OptionRight::Put) - 2.4648).abs() < 0.5e-4);
}

#[test]
fn put_call_parity_holds() {
    for c in &cases() {
        let call = inputs(c, OptionRight::Call).value().unwrap();
        let put = inputs(c, OptionRight::Put).value().unwrap();
        let fwd = c.spot * (-c.dividend_yield * c.years).exp() - c.strike * (-c.rate * c.years).exp();
        assert!((call.price - put.price - fwd).abs() < 1e-11, "{}", c.name);
        // Delta differs by e^{-qT}; gamma and vega are the same for both rights.
        assert!((call.delta - put.delta - (-c.dividend_yield * c.years).exp()).abs() < 1e-14, "{}", c.name);
        assert!((call.gamma - put.gamma).abs() < 1e-15 && (call.vega - put.vega).abs() < 1e-12, "{}", c.name);
    }
}

#[test]
fn implied_volatility_recovers_the_reference_volatility() {
    for c in &cases() {
        for (right, want) in [(OptionRight::Call, c.call), (OptionRight::Put, c.put)] {
            let iv = inputs(c, right).implied_volatility(want.price).unwrap();
            assert!((iv - c.volatility).abs() < 1e-9, "{} {right:?}: {iv}", c.name);
        }
    }
}

/// With no volatility an in-the-money option is a forward, so its theta and rho
/// are the forward's. Before M26 task 5a both came back as zero.
#[test]
fn zero_volatility_in_the_money_has_the_forwards_theta_and_rho() {
    let (k, t, r, q): (f64, f64, f64, f64) = (100.0, 1.0, 0.05, 0.02);
    let pv_k = k * (-r * t).exp();
    for (right, spot) in [(OptionRight::Call, 120.0), (OptionRight::Put, 80.0)] {
        let base =
            OptionInputs { right, spot, strike: k, years: t, rate: r, dividend_yield: q, volatility: 0.0 };
        let v = base.value().unwrap();
        let fwd = spot * (-q * t).exp();
        let (theta, rho) = match right {
            OptionRight::Call => (q * fwd - r * pv_k, t * pv_k),
            OptionRight::Put => (r * pv_k - q * fwd, -t * pv_k),
        };
        check(&format!("{right:?} theta"), v.theta, theta);
        check(&format!("{right:?} rho"), v.rho, rho);
        // And the same by finite differences of the price.
        let h = 1e-5;
        let f = |i: OptionInputs| i.value().unwrap().price;
        let dt =
            (f(OptionInputs { years: t - h, ..base }) - f(OptionInputs { years: t + h, ..base })) / (2.0 * h);
        let dr =
            (f(OptionInputs { rate: r + h, ..base }) - f(OptionInputs { rate: r - h, ..base })) / (2.0 * h);
        assert!((v.theta - dt).abs() < 1e-6, "{right:?} theta {} vs {dt}", v.theta);
        assert!((v.rho - dr).abs() < 1e-6, "{right:?} rho {} vs {dr}", v.rho);
        assert_eq!((v.gamma, v.vega), (0.0, 0.0));
    }
    // Out of the money with no volatility: worth nothing, no sensitivities.
    let otm = OptionInputs {
        right: OptionRight::Call,
        spot: 80.0,
        strike: k,
        years: t,
        rate: r,
        dividend_yield: q,
        volatility: 0.0,
    };
    assert_eq!(otm.value().unwrap(), OptionValue::default());
}

#[test]
fn expiry_is_intrinsic_value() {
    let at = |right, spot| OptionInputs {
        right,
        spot,
        strike: 100.0,
        years: 0.0,
        rate: 0.05,
        dividend_yield: 0.02,
        volatility: 0.2,
    };
    assert_eq!(at(OptionRight::Call, 112.5).value().unwrap().price, 12.5);
    assert_eq!(at(OptionRight::Put, 87.5).value().unwrap().price, 12.5);
    assert_eq!(at(OptionRight::Call, 87.5).value().unwrap().price, 0.0);
    assert_eq!(at(OptionRight::Put, 112.5).value().unwrap().price, 0.0);
}

#[test]
fn bad_inputs_are_errors_not_defaults() {
    let ok = inputs(&cases()[0], OptionRight::Call);
    for bad in [
        OptionInputs { volatility: f64::NAN, ..ok },
        OptionInputs { volatility: -0.1, ..ok },
        OptionInputs { volatility: 11.0, ..ok },
        OptionInputs { years: -0.01, ..ok },
        OptionInputs { years: f64::NAN, ..ok },
        OptionInputs { strike: 0.0, ..ok },
        OptionInputs { spot: -1.0, ..ok },
        OptionInputs { rate: f64::INFINITY, ..ok },
    ] {
        assert!(bad.value().is_err(), "{bad:?}");
    }
    // Implied volatility: none at expiry, and none for a price the model cannot reach.
    assert!(OptionInputs { years: 0.0, ..ok }.implied_volatility(10.0).is_err());
    assert!(ok.implied_volatility(150.0).is_err()); // above the spot: no volatility gives it
    assert!(ok.implied_volatility(f64::NAN).is_err());
}
