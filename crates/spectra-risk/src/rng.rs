//! The random-number pipeline of the C++ CPU reference, reproduced exactly:
//! `std::mt19937_64` seeded with one value, `std::generate_canonical<double,
//! 53>` as libstdc++ implements it, and libstdc++'s Marsaglia-polar
//! `std::normal_distribution<double>` (which caches the second variate of
//! each pair). Same seed, same stream of normals, bit for bit; this is what
//! lets the Rust engine reproduce C++ Monte Carlo results exactly.

use spectra_core::rng::splitmix64;

/// 64-bit Mersenne Twister (`std::mt19937_64`).
#[derive(Clone)]
pub struct Mt19937_64 {
    state: [u64; 312],
    index: usize,
}

impl std::fmt::Debug for Mt19937_64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt19937_64").field("index", &self.index).finish_non_exhaustive()
    }
}

const NN: usize = 312;
const MM: usize = 156;
const MATRIX_A: u64 = 0xB502_6F5A_A966_19E9;
const UPPER: u64 = 0xFFFF_FFFF_8000_0000;
const LOWER: u64 = 0x7FFF_FFFF;

impl Mt19937_64 {
    /// Seeded like `std::mt19937_64(seed)`.
    pub fn new(seed: u64) -> Self {
        let mut state = [0u64; NN];
        state[0] = seed;
        for i in 1..NN {
            let prev = state[i - 1];
            state[i] = 6_364_136_223_846_793_005u64.wrapping_mul(prev ^ (prev >> 62)).wrapping_add(i as u64);
        }
        Self { state, index: NN }
    }

    fn twist(&mut self) {
        for i in 0..NN {
            let x = (self.state[i] & UPPER) | (self.state[(i + 1) % NN] & LOWER);
            let mut xa = x >> 1;
            if x & 1 != 0 {
                xa ^= MATRIX_A;
            }
            self.state[i] = self.state[(i + MM) % NN] ^ xa;
        }
        self.index = 0;
    }

    /// Next raw output.
    pub fn next_u64(&mut self) -> u64 {
        if self.index >= NN {
            self.twist();
        }
        let mut z = self.state[self.index];
        self.index += 1;
        z ^= (z >> 29) & 0x5555_5555_5555_5555;
        z ^= (z << 17) & 0x71D6_7FFF_EDA6_0000;
        z ^= (z << 37) & 0xFFF7_EEE0_0000_0000;
        z ^= z >> 43;
        z
    }

    /// `std::generate_canonical<double, 53>(*this)` as libstdc++ computes it:
    /// one draw, converted to double (round to nearest) and divided by 2^64,
    /// clamped below 1.
    pub fn canonical(&mut self) -> f64 {
        let r = self.next_u64() as f64 / 18_446_744_073_709_551_616.0;
        if r >= 1.0 {
            1.0 - f64::EPSILON / 2.0 // nextafter(1.0, 0.0)
        } else {
            r
        }
    }
}

/// Standard normal variates in exactly the order libstdc++'s
/// `std::normal_distribution<double>(0, 1)` produces them.
#[derive(Debug, Clone)]
pub struct PolarNormal {
    saved: Option<f64>,
}

impl Default for PolarNormal {
    fn default() -> Self {
        Self::new()
    }
}

impl PolarNormal {
    /// A distribution with no cached variate.
    pub const fn new() -> Self {
        Self { saved: None }
    }

    /// Next N(0, 1) variate.
    pub fn sample(&mut self, rng: &mut Mt19937_64) -> f64 {
        if let Some(v) = self.saved.take() {
            return v;
        }
        loop {
            let x = 2.0 * rng.canonical() - 1.0;
            let y = 2.0 * rng.canonical() - 1.0;
            let r2 = x * x + y * y;
            if r2 > 1.0 || r2 == 0.0 {
                continue;
            }
            let mult = (-2.0 * r2.ln() / r2).sqrt();
            self.saved = Some(x * mult);
            return y * mult;
        }
    }
}

/// Derives a distinct, reproducible sub-seed for batch `batch_index` of a run
/// (the C++ `RngManager::seed_for_batch`).
pub const fn seed_for_batch(base_seed: u64, batch_index: u64) -> u64 {
    splitmix64(base_seed ^ splitmix64(batch_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt19937_64_matches_the_standard_check_value() {
        // C++11 [rand.predef]: the 10000th output of a default-seeded
        // (5489) mt19937_64 is 9981545732273789042.
        let mut g = Mt19937_64::new(5489);
        let mut v = 0;
        for _ in 0..10_000 {
            v = g.next_u64();
        }
        assert_eq!(v, 9_981_545_732_273_789_042);
    }
}
