//! Exact fixed-point arithmetic for money, prices and quantities.
//!
//! Every accounting value is a 64-bit integer scaled by 1e8 ([`SCALE`]).
//! Arithmetic is exact and overflow-checked; the only rounding happens in the
//! explicitly named operations [`notional`] and [`prorate`], which always round
//! half away from zero.
//!
//! The representable range is symmetric (`-i64::MAX ..= i64::MAX`) so negation
//! can never overflow. `i64::MIN` is never produced.
//!
//! Ported from the C++ reference `quantforge/fixed.hpp` (v0.6.0). Behaviour is
//! identical, including rounding and the textual form; the parity tests in
//! `tests/parity_ledger.rs` check this against golden output from the C++ code.
//!
//! Error handling: the `checked_*` methods and free functions return
//! [`ArithmeticError`]. The `+`, `-` and unary `-` operators panic on overflow
//! and exist for tests and obviously-bounded code; ledger code uses the checked
//! forms.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Raw units per 1.0. Eight decimal places cover crypto quantities and
/// sub-cent equity prices, leaving about 9.2e10 whole units of headroom.
pub const SCALE: i64 = 100_000_000;

/// Largest magnitude any raw value may take.
pub const MAX_RAW: i64 = i64::MAX;

/// Why an exact arithmetic operation could not produce a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticError {
    /// The exact result does not fit in the representable range.
    Overflow,
    /// A division (prorate) by a zero quantity.
    DivisionByZero,
    /// A floating-point input was NaN or infinite.
    NotFinite,
}

impl fmt::Display for ArithmeticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overflow => "fixed-point overflow",
            Self::DivisionByZero => "division by zero",
            Self::NotFinite => "value is not finite",
        })
    }
}

impl std::error::Error for ArithmeticError {}

/// Divide, rounding half away from zero. `den` must be non-zero.
pub fn div_round_half_away(mut num: i128, mut den: i128) -> i128 {
    debug_assert!(den != 0);
    if den < 0 {
        num = -num;
        den = -den;
    }
    let mut q = num / den;
    let r = num % den;
    if r.abs() * 2 >= den {
        q += if num < 0 { -1 } else { 1 };
    }
    q
}

/// Narrow to i64, rejecting anything outside the symmetric range.
///
/// # Errors
/// [`ArithmeticError::Overflow`] when `v` does not fit.
pub fn narrow(v: i128) -> Result<i64, ArithmeticError> {
    if v > i128::from(MAX_RAW) || v < -i128::from(MAX_RAW) {
        Err(ArithmeticError::Overflow)
    } else {
        Ok(v as i64)
    }
}

/// Marker trait for the meaning of a fixed-point value.
pub trait Unit: Copy + 'static {
    /// Short name used in `Debug` output.
    const NAME: &'static str;
}

/// A scaled integer tagged by meaning, so prices, quantities and money cannot
/// be mixed by accident.
pub struct Fixed<U: Unit> {
    raw: i64,
    _unit: PhantomData<U>,
}

macro_rules! unit {
    ($tag:ident, $alias:ident, $name:literal, $doc:literal) => {
        #[doc = concat!("Unit tag for [`", stringify!($alias), "`].")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct $tag;
        impl Unit for $tag {
            const NAME: &'static str = $name;
        }
        #[doc = $doc]
        pub type $alias = Fixed<$tag>;
    };
}

unit!(PriceUnit, Price, "Price", "Currency units per unit of instrument.");
unit!(QtyUnit, Qty, "Qty", "Units of an instrument; signed for positions (short < 0).");
unit!(MoneyUnit, Money, "Money", "Currency units.");
unit!(BpsUnit, Bps, "Bps", "Basis points: 1.0 is one basis point (0.01%).");
unit!(RatioUnit, Ratio, "Ratio", "A dimensionless ratio, e.g. 2.0 for 2x leverage.");

impl<U: Unit> Fixed<U> {
    /// Zero.
    pub const ZERO: Self = Self::from_raw(0);
    /// One whole unit.
    pub const ONE: Self = Self::from_raw(SCALE);

    /// Wraps a raw scaled integer. `i64::MIN` is outside the symmetric range
    /// and is rejected by every arithmetic operation that could produce it.
    pub const fn from_raw(raw: i64) -> Self {
        Self { raw, _unit: PhantomData }
    }

    /// Whole units, e.g. `Money::from_units(100)` is 100.00000000.
    ///
    /// # Errors
    /// [`ArithmeticError::Overflow`] if out of range.
    pub fn from_units(units: i64) -> Result<Self, ArithmeticError> {
        units
            .checked_mul(SCALE)
            .filter(|&v| v != i64::MIN)
            .map(Self::from_raw)
            .ok_or(ArithmeticError::Overflow)
    }

    /// For I/O boundaries only (UI, synthetic data). Rounds to the nearest raw
    /// unit, ties away from zero, exactly like C++ `std::llround`.
    ///
    /// # Errors
    /// [`ArithmeticError::NotFinite`] or [`ArithmeticError::Overflow`].
    pub fn from_f64(v: f64) -> Result<Self, ArithmeticError> {
        let scaled = v * SCALE as f64;
        if !scaled.is_finite() {
            return Err(ArithmeticError::NotFinite);
        }
        if scaled.abs() >= 9.2e18 {
            return Err(ArithmeticError::Overflow);
        }
        Ok(Self::from_raw(scaled.round() as i64))
    }

    /// The raw scaled integer.
    pub const fn raw(self) -> i64 {
        self.raw
    }

    /// Lossy conversion for analytics and display.
    pub fn to_f64(self) -> f64 {
        self.raw as f64 / SCALE as f64
    }

    /// True if exactly zero.
    pub const fn is_zero(self) -> bool {
        self.raw == 0
    }

    /// -1, 0 or 1.
    pub const fn signum(self) -> i64 {
        self.raw.signum()
    }

    /// Absolute value. Never fails for values inside the symmetric range.
    ///
    /// # Errors
    /// Only for the out-of-range raw value `i64::MIN`.
    pub fn checked_abs(self) -> Result<Self, ArithmeticError> {
        if self.raw < 0 {
            self.checked_neg()
        } else {
            Ok(self)
        }
    }

    /// Negation.
    ///
    /// # Errors
    /// Only for the out-of-range raw value `i64::MIN`.
    pub fn checked_neg(self) -> Result<Self, ArithmeticError> {
        if self.raw == i64::MIN {
            Err(ArithmeticError::Overflow)
        } else {
            Ok(Self::from_raw(-self.raw))
        }
    }

    /// Exact addition.
    ///
    /// # Errors
    /// [`ArithmeticError::Overflow`].
    pub fn checked_add(self, other: Self) -> Result<Self, ArithmeticError> {
        match self.raw.checked_add(other.raw) {
            Some(v) if v != i64::MIN => Ok(Self::from_raw(v)),
            _ => Err(ArithmeticError::Overflow),
        }
    }

    /// Exact subtraction.
    ///
    /// # Errors
    /// [`ArithmeticError::Overflow`].
    pub fn checked_sub(self, other: Self) -> Result<Self, ArithmeticError> {
        match self.raw.checked_sub(other.raw) {
            Some(v) if v != i64::MIN => Ok(Self::from_raw(v)),
            _ => Err(ArithmeticError::Overflow),
        }
    }

    /// Same raw value, different meaning. Use sparingly and only where the
    /// conversion is dimensionally meaningful.
    pub const fn cast<V: Unit>(self) -> Fixed<V> {
        Fixed::from_raw(self.raw)
    }
}

// Manual trait impls: deriving would put needless bounds on the unit tag.
#[allow(clippy::expl_impl_clone_on_copy)] // derive(Clone) would require `U: Clone` bounds
impl<U: Unit> Clone for Fixed<U> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<U: Unit> Copy for Fixed<U> {}
impl<U: Unit> Default for Fixed<U> {
    fn default() -> Self {
        Self::ZERO
    }
}
impl<U: Unit> PartialEq for Fixed<U> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<U: Unit> Eq for Fixed<U> {}
impl<U: Unit> PartialOrd for Fixed<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<U: Unit> Ord for Fixed<U> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}
impl<U: Unit> Hash for Fixed<U> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

/// Decimal form with exactly eight fractional digits, e.g. `-12.50000000`.
/// Identical to the C++ `Fixed::to_string`.
impl<U: Unit> fmt::Display for Fixed<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mag = self.raw.unsigned_abs();
        let scale = SCALE as u64;
        let sign = if self.raw < 0 { "-" } else { "" };
        write!(f, "{sign}{}.{:08}", mag / scale, mag % scale)
    }
}

impl<U: Unit> fmt::Debug for Fixed<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({self})", U::NAME)
    }
}

impl<U: Unit> Add for Fixed<U> {
    type Output = Self;
    /// # Panics
    /// On overflow. Use [`Fixed::checked_add`] in accounting code.
    fn add(self, rhs: Self) -> Self {
        self.checked_add(rhs).expect("fixed-point addition overflow")
    }
}
impl<U: Unit> Sub for Fixed<U> {
    type Output = Self;
    /// # Panics
    /// On overflow. Use [`Fixed::checked_sub`] in accounting code.
    fn sub(self, rhs: Self) -> Self {
        self.checked_sub(rhs).expect("fixed-point subtraction overflow")
    }
}
impl<U: Unit> Neg for Fixed<U> {
    type Output = Self;
    /// # Panics
    /// Only for the out-of-range raw value `i64::MIN`.
    fn neg(self) -> Self {
        self.checked_neg().expect("fixed-point negation overflow")
    }
}
impl<U: Unit> AddAssign for Fixed<U> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}
impl<U: Unit> SubAssign for Fixed<U> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

/// `price * qty`, rounded half away from zero to money precision.
///
/// # Errors
/// [`ArithmeticError::Overflow`].
pub fn notional(price: Price, qty: Qty) -> Result<Money, ArithmeticError> {
    let prod = i128::from(price.raw()) * i128::from(qty.raw());
    narrow(div_round_half_away(prod, i128::from(SCALE))).map(Money::from_raw)
}

/// `price * qty * multiplier`, rounded half away from zero to money precision
/// with a single rounding. A multiplier of exactly 1 is [`notional`].
///
/// # Errors
/// [`ArithmeticError::Overflow`].
pub fn notional_with_multiplier(price: Price, qty: Qty, multiplier: Ratio) -> Result<Money, ArithmeticError> {
    if multiplier.raw() == SCALE {
        return notional(price, qty);
    }
    let prod = i128::from(price.raw()) * i128::from(qty.raw()) * i128::from(multiplier.raw());
    narrow(div_round_half_away(prod, i128::from(SCALE) * i128::from(SCALE))).map(Money::from_raw)
}

/// `amount * part / whole`, rounded half away from zero.
///
/// # Errors
/// [`ArithmeticError::DivisionByZero`] if `whole` is zero, or overflow.
pub fn prorate(amount: Money, part: Qty, whole: Qty) -> Result<Money, ArithmeticError> {
    if whole.is_zero() {
        return Err(ArithmeticError::DivisionByZero);
    }
    let num = i128::from(amount.raw()) * i128::from(part.raw());
    narrow(div_round_half_away(num, i128::from(whole.raw()))).map(Money::from_raw)
}

/// `money * ratio`, rounded half away from zero (used for leverage limits).
///
/// # Errors
/// [`ArithmeticError::Overflow`].
pub fn scale_money(amount: Money, ratio: Ratio) -> Result<Money, ArithmeticError> {
    let prod = i128::from(amount.raw()) * i128::from(ratio.raw());
    narrow(div_round_half_away(prod, i128::from(SCALE))).map(Money::from_raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_cpp_format() {
        assert_eq!(Money::from_raw(0).to_string(), "0.00000000");
        assert_eq!(Money::from_raw(-50_000_000).to_string(), "-0.50000000");
        assert_eq!(Money::from_raw(1_234_567_890_123).to_string(), "12345.67890123");
        assert_eq!(Money::from_raw(MAX_RAW).to_string(), "92233720368.54775807");
        assert_eq!(Money::from_raw(-MAX_RAW).to_string(), "-92233720368.54775807");
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(div_round_half_away(5, 2), 3);
        assert_eq!(div_round_half_away(-5, 2), -3);
        assert_eq!(div_round_half_away(4, 3), 1);
        assert_eq!(div_round_half_away(-4, 3), -1);
        assert_eq!(div_round_half_away(5, -2), -3);
        assert_eq!(div_round_half_away(-5, -2), 3);
        // 0.5 raw unit of notional rounds up in magnitude.
        assert_eq!(notional(Price::from_raw(1), Qty::from_raw(50_000_000)).unwrap().raw(), 1);
        assert_eq!(notional(Price::from_raw(1), Qty::from_raw(49_999_999)).unwrap().raw(), 0);
        assert_eq!(notional(Price::from_raw(-1), Qty::from_raw(50_000_000)).unwrap().raw(), -1);
    }

    #[test]
    fn notional_and_prorate_are_exact() {
        let p = Price::from_f64(100.1).unwrap();
        let q = Qty::from_f64(10.0).unwrap();
        assert_eq!(notional(p, q).unwrap(), Money::from_f64(1001.0).unwrap());
        let cost = Money::from_raw(1_000);
        assert_eq!(prorate(cost, Qty::from_raw(1), Qty::from_raw(3)).unwrap().raw(), 333);
        assert_eq!(prorate(cost, Qty::from_raw(2), Qty::from_raw(3)).unwrap().raw(), 667);
        assert_eq!(prorate(cost, Qty::from_raw(1), Qty::ZERO), Err(ArithmeticError::DivisionByZero));
    }

    #[test]
    fn overflow_is_detected_never_wrapped() {
        let max = Money::from_raw(MAX_RAW);
        assert_eq!(max.checked_add(Money::from_raw(1)), Err(ArithmeticError::Overflow));
        assert_eq!((-max).checked_sub(Money::from_raw(1)), Err(ArithmeticError::Overflow));
        assert_eq!(Money::from_raw(i64::MIN).checked_neg(), Err(ArithmeticError::Overflow));
        assert!(notional(Price::from_raw(MAX_RAW), Qty::from_raw(2 * SCALE)).is_err());
        assert!(Money::from_units(i64::MAX / SCALE + 1).is_err());
        assert!(Money::from_f64(f64::NAN).is_err());
        assert!(Money::from_f64(1e12).is_err());
    }

    #[test]
    fn from_f64_rounds_like_llround() {
        assert_eq!(Money::from_f64(190.0).unwrap().raw(), 19_000_000_000);
        assert_eq!(Money::from_f64(-0.25).unwrap().raw(), -25_000_000);
        assert_eq!(Money::from_f64(1.000_000_004).unwrap().raw(), 100_000_000);
        assert_eq!(Money::from_f64(1.000_000_006).unwrap().raw(), 100_000_001);
    }

    #[test]
    #[should_panic(expected = "overflow")]
    fn operators_panic_instead_of_wrapping() {
        let _ = Money::from_raw(MAX_RAW) + Money::from_raw(1);
    }

    #[test]
    fn debug_names_the_unit() {
        assert_eq!(format!("{:?}", Price::ONE), "Price(1.00000000)");
    }
}
