//! C `printf`-compatible number formatting.
//!
//! Hashes, registry events and human-readable reports in the C++ reference
//! were produced with `printf` conversions (`%.17g`, `%.2f`, `%.3e`, ...).
//! These helpers reproduce glibc's output exactly so the Rust engine writes
//! byte-identical fingerprints, JSON and reports. Rust's own float formatting
//! is exact (correctly rounded, ties to even on exact decimal ties), as is
//! glibc's, so only the layout (exponent form, trailing zeros, NaN/inf
//! spelling) needs adapting.

fn non_finite(v: f64) -> Option<String> {
    if v.is_nan() {
        Some(if v.is_sign_negative() { "-nan" } else { "nan" }.to_string())
    } else if v.is_infinite() {
        Some(if v < 0.0 { "-inf" } else { "inf" }.to_string())
    } else {
        None
    }
}

/// `%.{prec}f`
pub fn fmt_f(v: f64, prec: usize) -> String {
    non_finite(v).unwrap_or_else(|| format!("{v:.prec$}"))
}

/// Splits Rust's `{:e}` output ("1.25e-7") into mantissa and exponent.
fn split_exp(s: &str) -> (&str, i32) {
    let (m, e) = s.split_once('e').expect("exponent form");
    (m, e.parse().expect("exponent"))
}

fn c_exponent(x: i32) -> String {
    format!("e{}{:02}", if x < 0 { '-' } else { '+' }, x.unsigned_abs())
}

/// `%.{prec}e`
pub fn fmt_e(v: f64, prec: usize) -> String {
    if let Some(s) = non_finite(v) {
        return s;
    }
    let s = format!("{v:.prec$e}");
    let (m, x) = split_exp(&s);
    format!("{m}{}", c_exponent(x))
}

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// `%.{prec}g`
pub fn fmt_g(v: f64, prec: usize) -> String {
    if let Some(s) = non_finite(v) {
        return s;
    }
    let p = prec.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let e = format!("{:.*e}", p - 1, v);
    let (mant, x) = split_exp(&e);
    if x < -4 || x >= p as i32 {
        format!("{}{}", strip_zeros(mant), c_exponent(x))
    } else {
        let decimals = (p as i32 - 1 - x) as usize;
        strip_zeros(&format!("{v:.decimals$}")).to_string()
    }
}

/// `%.17g`: the round-trip form used in hashes and JSON.
pub fn fmt_g17(v: f64) -> String {
    fmt_g(v, 17)
}

/// Right-align in a field of `width` characters (`%6.2f` = `pad_left(fmt_f(v, 2), 6)`).
pub fn pad_left(s: &str, width: usize) -> String {
    format!("{s:>width$}")
}

/// Left-align in a field of `width` characters (`%-12s`).
pub fn pad_right(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g17_matches_printf() {
        assert_eq!(fmt_g17(3.0), "3");
        assert_eq!(fmt_g17(252.0), "252");
        assert_eq!(fmt_g17(0.1), "0.10000000000000001");
        assert_eq!(fmt_g17(-2.5), "-2.5");
        assert_eq!(fmt_g17(1e-5), "1.0000000000000001e-05");
        assert_eq!(fmt_g17(0.0001), "0.0001");
        assert_eq!(fmt_g17(1e17), "1e+17");
        assert_eq!(fmt_g17(1e16), "10000000000000000");
        assert_eq!(fmt_g17(123_456_789.0), "123456789");
        assert_eq!(fmt_g17(-0.0), "-0");
        assert_eq!(fmt_g17(1.0 / 3.0), "0.33333333333333331");
        assert_eq!(fmt_g17(f64::MAX), "1.7976931348623157e+308");
    }

    #[test]
    fn g6_matches_printf() {
        assert_eq!(fmt_g(183_869.123, 6), "183869");
        assert_eq!(fmt_g(34.849_123, 6), "34.8491");
        assert_eq!(fmt_g(1_234_567.0, 6), "1.23457e+06");
        assert_eq!(fmt_g(0.000_012_345, 6), "1.2345e-05");
        assert_eq!(fmt_g(999_999.5, 6), "1e+06");
    }

    #[test]
    fn e_and_f_match_printf() {
        assert_eq!(fmt_e(8.135e-5, 3), "8.135e-05");
        assert_eq!(fmt_e(123.0, 2), "1.23e+02");
        assert_eq!(fmt_f(0.125, 2), "0.12"); // exact tie rounds to even, as glibc does
        assert_eq!(fmt_f(2.5, 0), "2");
        assert_eq!(fmt_f(f64::INFINITY, 2), "inf");
        assert_eq!(fmt_f(f64::NAN, 2), "nan");
        assert_eq!(pad_left(&fmt_f(1.5, 2), 6), "  1.50");
        assert_eq!(pad_right("PASS", 12), "PASS        ");
    }
}
