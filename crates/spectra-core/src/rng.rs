//! Small deterministic PRNG (splitmix64). Identical output on every platform
//! and identical to the C++ reference `qf::Rng`, so seeds recorded in run
//! records reproduce everywhere. Not cryptographic.

/// splitmix64 generator.
#[derive(Debug, Clone)]
pub struct Rng {
    s: u64,
}

impl Rng {
    /// A generator with the given seed.
    pub const fn new(seed: u64) -> Self {
        Self { s: seed }
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.s = self.s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1) with 53 bits of resolution.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / 9_007_199_254_740_992.0
    }

    /// Approximately standard normal (sum of 12 uniforms minus 6).
    pub fn normal(&mut self) -> f64 {
        let mut s = 0.0;
        for _ in 0..12 {
            s += self.uniform();
        }
        s - 6.0
    }

    /// Integer in `[lo, hi]` (requires `lo <= hi`). The tiny modulo bias is
    /// irrelevant for simulation use. Any range up to the full 64-bit span is
    /// well defined.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi as u64).wrapping_sub(lo as u64).wrapping_add(1);
        let offset = if span == 0 { self.next_u64() } else { self.next_u64() % span };
        (lo as u64).wrapping_add(offset) as i64
    }
}

/// One splitmix64 mixing step of `x` (the stateless form used to derive
/// sub-seeds).
pub const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_splitmix64() {
        // First outputs for seed 0 (published splitmix64 test vector).
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn range_is_inclusive_and_handles_full_span() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.range(-3, 3);
            assert!((-3..=3).contains(&v));
        }
        let _ = r.range(i64::MIN, i64::MAX);
    }

    #[test]
    fn uniform_is_in_unit_interval() {
        let mut r = Rng::new(99);
        for _ in 0..10_000 {
            let u = r.uniform();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
