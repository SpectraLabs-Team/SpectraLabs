//! Probability of Backtest Overfitting via Combinatorially Symmetric
//! Cross-Validation (Bailey, Borwein, López de Prado, Zhu 2015).
//!
//! Input: the per-period returns of N candidate strategies (e.g. every trial of
//! a sweep) over the SAME T periods. Time is split into S contiguous blocks;
//! for every way of choosing S/2 blocks as in-sample, the best in-sample
//! strategy is ranked out-of-sample:
//!   omega = relative OOS rank of the in-sample winner, in (0, 1)
//!   logit = ln(omega / (1 - omega))
//!   PBO   = fraction of combinations with logit <= 0
//! PBO near 0.5 means in-sample selection carries no information; near 0
//! means it generalises.
//!
//! A single PBO value is NOISY (all combinations reuse one dataset): on pure
//! noise with 20 strategies it averages 0.5 but individual datasets range
//! roughly 0.1-0.9. Treat it as an indicator. Blocks have equal length; any
//! remainder at the end is dropped.

use spectra_core::{Error, Result};

/// CSCV outcome.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PboResult {
    /// Probability of backtest overfitting.
    pub pbo: f64,
    /// Fraction of combinations where the winner's OOS Sharpe < 0.
    pub prob_oos_loss: f64,
    /// Mean logit.
    pub mean_logit: f64,
    /// Number of block combinations evaluated.
    pub combinations: usize,
    /// Number of strategies.
    pub strategies: usize,
    /// Observations used (multiple of the block count).
    pub observations_used: usize,
}

#[derive(Clone, Copy, Default)]
struct BlockSums {
    sum: f64,
    sumsq: f64,
}

fn sharpe_from_sums(sum: f64, sumsq: f64, n: f64) -> f64 {
    if n < 2.0 {
        return 0.0;
    }
    let mean = sum / n;
    let var = (sumsq - n * mean * mean) / (n - 1.0);
    if var > 0.0 {
        mean / var.sqrt()
    } else {
        0.0
    }
}

/// Next lexicographic permutation of a 0/1 selector (`std::next_permutation`).
fn next_permutation(v: &mut [u8]) -> bool {
    if v.len() < 2 {
        return false;
    }
    let mut i = v.len() - 1;
    while i > 0 && v[i - 1] >= v[i] {
        i -= 1;
    }
    if i == 0 {
        v.reverse();
        return false;
    }
    let mut j = v.len() - 1;
    while v[j] <= v[i - 1] {
        j -= 1;
    }
    v.swap(i - 1, j);
    v[i..].reverse();
    true
}

/// CSCV PBO. `returns[i]` is strategy i's series; all equal length.
///
/// # Errors
/// Fewer than 2 strategies, `n_blocks` odd or below 4, unequal lengths, or
/// fewer than 2 observations per block.
pub fn cscv_pbo(returns: &[Vec<f64>], n_blocks: usize) -> Result<PboResult> {
    let n_strat = returns.len();
    if n_strat < 2 {
        return Err(Error::InvalidArgument("cscv_pbo: need at least 2 strategies".into()));
    }
    if n_blocks < 4 || !n_blocks.is_multiple_of(2) {
        return Err(Error::InvalidArgument("cscv_pbo: n_blocks must be even and >= 4".into()));
    }
    let t = returns[0].len();
    if returns.iter().any(|r| r.len() != t) {
        return Err(Error::InvalidArgument("cscv_pbo: return series differ in length".into()));
    }
    if t < n_blocks * 2 {
        return Err(Error::InvalidArgument("cscv_pbo: need at least 2 observations per block".into()));
    }

    let block_len = t / n_blocks;
    let block_n = block_len as f64;
    let sums: Vec<Vec<BlockSums>> = returns
        .iter()
        .map(|r| {
            (0..n_blocks)
                .map(|b| {
                    let mut bs = BlockSums::default();
                    for &v in &r[b * block_len..(b + 1) * block_len] {
                        bs.sum += v;
                        bs.sumsq += v * v;
                    }
                    bs
                })
                .collect()
        })
        .collect();

    let half = n_blocks / 2;
    // selector[b] == 1 means block b is in-sample; start from the smallest arrangement.
    let mut selector: Vec<u8> = (0..n_blocks).map(|b| u8::from(b >= half)).collect();
    selector.sort_unstable();

    let mut res =
        PboResult { strategies: n_strat, observations_used: block_len * n_blocks, ..PboResult::default() };
    let mut is_sharpe = vec![0.0; n_strat];
    let mut oos_sharpe = vec![0.0; n_strat];
    let mut overfit = 0usize;
    let mut oos_loss = 0usize;
    let mut logit_sum = 0.0;
    let n_half = block_n * half as f64;

    loop {
        for s in 0..n_strat {
            let (mut is, mut oos) = (BlockSums::default(), BlockSums::default());
            for (b, &sel) in selector.iter().enumerate() {
                let dst = if sel == 1 { &mut is } else { &mut oos };
                dst.sum += sums[s][b].sum;
                dst.sumsq += sums[s][b].sumsq;
            }
            is_sharpe[s] = sharpe_from_sums(is.sum, is.sumsq, n_half);
            oos_sharpe[s] = sharpe_from_sums(oos.sum, oos.sumsq, n_half);
        }
        // In-sample winner (ties go to the lowest index).
        let mut best = 0;
        for s in 1..n_strat {
            if is_sharpe[s] > is_sharpe[best] {
                best = s;
            }
        }
        // Mid-rank of the winner among all strategies out-of-sample (1 = worst).
        let target = oos_sharpe[best];
        let (mut less, mut equal) = (0.0, 0.0);
        for &v in &oos_sharpe {
            if v < target {
                less += 1.0;
            } else if v == target {
                equal += 1.0;
            }
        }
        let rank = less + (equal + 1.0) / 2.0;
        let omega = rank / (n_strat as f64 + 1.0);
        let logit = (omega / (1.0 - omega)).ln();
        logit_sum += logit;
        if logit <= 0.0 {
            overfit += 1;
        }
        if target < 0.0 {
            oos_loss += 1;
        }
        res.combinations += 1;
        if !next_permutation(&mut selector) {
            break;
        }
    }

    let combos = res.combinations as f64;
    res.pbo = overfit as f64 / combos;
    res.prob_oos_loss = oos_loss as f64 / combos;
    res.mean_logit = logit_sum / combos;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_and_validation() {
        let r: Vec<Vec<f64>> =
            (0..3).map(|s| (0..16).map(|t| f64::from((s * 7 + t * 3) % 5) - 2.0).collect()).collect();
        let p = cscv_pbo(&r, 4).unwrap();
        assert_eq!(p.combinations, 6); // C(4, 2)
        assert_eq!(p.strategies, 3);
        assert!(cscv_pbo(&r[..1], 4).is_err());
        assert!(cscv_pbo(&r, 3).is_err());
        assert!(cscv_pbo(&r, 16).is_err());
    }

    #[test]
    fn a_dominant_strategy_is_not_overfit() {
        let good: Vec<f64> = (0..64).map(|t| 0.02 + if t % 2 == 0 { 0.001 } else { -0.001 }).collect();
        let noise: Vec<Vec<f64>> =
            (1..6).map(|s| (0..64).map(|t| if (t * s) % 3 == 0 { 0.01 } else { -0.01 }).collect()).collect();
        let mut all = vec![good];
        all.extend(noise);
        assert_eq!(cscv_pbo(&all, 8).unwrap().pbo, 0.0);
    }

    #[test]
    fn next_permutation_enumerates_all() {
        let mut v = vec![0, 0, 1, 1];
        let mut n = 1;
        while next_permutation(&mut v) {
            n += 1;
        }
        assert_eq!(n, 6);
    }
}
