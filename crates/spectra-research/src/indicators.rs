//! Technical indicator engine for charting and backtests (M35).
//!
//! All calculations are performed in Rust and produce identical values across
//! charts, backtests, screener rules (M36), and formula expressions (M38).
//! Missing / warm-up values are represented as `None`.

use spectra_core::Bar;

/// A simple moving average (SMA) over `period` bars.
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = Vec::with_capacity(values.len());
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i + 1 >= period {
            if i >= period {
                sum -= values[i - period];
            }
            out.push(Some(sum / period as f64));
        } else {
            out.push(None);
        }
    }
    out
}

/// An exponential moving average (EMA) with smoothing factor `alpha = 2 / (period + 1)`.
/// The first value is seeded from the SMA of the initial `period` bars.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.len() < period {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let alpha = 2.0 / (period as f64 + 1.0);

    let seed_sum: f64 = values[..period].iter().sum();
    let mut prev_ema = seed_sum / period as f64;
    out[period - 1] = Some(prev_ema);

    for i in period..values.len() {
        let current_ema = values[i] * alpha + prev_ema * (1.0 - alpha);
        out[i] = Some(current_ema);
        prev_ema = current_ema;
    }
    out
}

/// Weighted moving average (WMA) with linear weights `1, 2, ..., period`.
pub fn wma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let weight_sum = (period * (period + 1)) as f64 / 2.0;
    let mut out = Vec::with_capacity(values.len());

    for i in 0..values.len() {
        if i + 1 < period {
            out.push(None);
            continue;
        }
        let mut weighted_sum = 0.0;
        for w in 1..=period {
            let idx = i + w - period;
            weighted_sum += values[idx] * (w as f64);
        }
        out.push(Some(weighted_sum / weight_sum));
    }
    out
}

/// Double exponential moving average (DEMA): `2 * EMA(P) - EMA(EMA(P))`.
pub fn dema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let mut ema1_values = Vec::new();
    let mut ema1_valid_start = None;

    for (i, v) in ema1.iter().enumerate() {
        if let Some(val) = v {
            if ema1_valid_start.is_none() {
                ema1_valid_start = Some(i);
            }
            ema1_values.push(*val);
        }
    }

    if ema1_values.len() < period {
        return vec![None; values.len()];
    }

    let ema2_sub = ema(&ema1_values, period);
    let mut out = vec![None; values.len()];
    let offset = ema1_valid_start.unwrap_or(0);

    for j in 0..ema2_sub.len() {
        let original_idx = offset + j;
        if let (Some(e1), Some(e2)) = (ema1[original_idx], ema2_sub[j]) {
            out[original_idx] = Some(2.0 * e1 - e2);
        }
    }
    out
}

/// Triple exponential moving average (TEMA): `3*EMA1 - 3*EMA2 + EMA3`.
pub fn tema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let mut ema1_vals = Vec::new();
    let mut start1 = None;
    for (i, v) in ema1.iter().enumerate() {
        if let Some(val) = v {
            if start1.is_none() {
                start1 = Some(i);
            }
            ema1_vals.push(*val);
        }
    }
    if ema1_vals.len() < period {
        return vec![None; values.len()];
    }

    let ema2_vals = ema(&ema1_vals, period);
    let mut ema2_clean = Vec::new();
    let mut start2 = None;
    for (i, v) in ema2_vals.iter().enumerate() {
        if let Some(val) = v {
            if start2.is_none() {
                start2 = Some(i);
            }
            ema2_clean.push(*val);
        }
    }
    if ema2_clean.len() < period {
        return vec![None; values.len()];
    }

    let ema3_vals = ema(&ema2_clean, period);
    let mut out = vec![None; values.len()];
    let s1 = start1.unwrap_or(0);
    let s2 = start2.unwrap_or(0);

    for k in 0..ema3_vals.len() {
        if let Some(e3) = ema3_vals[k] {
            let j = s2 + k;
            let orig_idx = s1 + j;
            if let (Some(e1), Some(e2)) = (ema1[orig_idx], ema2_vals[j]) {
                out[orig_idx] = Some(3.0 * e1 - 3.0 * e2 + e3);
            }
        }
    }
    out
}

/// Relative Strength Index (RSI) with Wilder smoothing over `period` (typically 14).
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.len() <= period {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];

    let mut gain_sum = 0.0;
    let mut loss_sum = 0.0;

    for i in 1..=period {
        let diff = values[i] - values[i - 1];
        if diff >= 0.0 {
            gain_sum += diff;
        } else {
            loss_sum += -diff;
        }
    }

    let mut avg_gain = gain_sum / period as f64;
    let mut avg_loss = loss_sum / period as f64;

    let rs = if avg_loss == 0.0 { f64::INFINITY } else { avg_gain / avg_loss };
    out[period] = Some(if avg_loss == 0.0 { 100.0 } else { 100.0 - (100.0 / (1.0 + rs)) });

    let p = period as f64;
    for i in (period + 1)..values.len() {
        let diff = values[i] - values[i - 1];
        let (gain, loss) = if diff >= 0.0 { (diff, 0.0) } else { (0.0, -diff) };
        avg_gain = ((p - 1.0) * avg_gain + gain) / p;
        avg_loss = ((p - 1.0) * avg_loss + loss) / p;

        if avg_loss == 0.0 {
            out[i] = Some(100.0);
        } else {
            let rs = avg_gain / avg_loss;
            out[i] = Some(100.0 - (100.0 / (1.0 + rs)));
        }
    }
    out
}

/// MACD result holding the MACD line, signal line, and histogram.
#[derive(Debug, Clone, PartialEq)]
pub struct MacdResult {
    pub macd: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub histogram: Vec<Option<f64>>,
}

/// Moving Average Convergence Divergence (MACD).
/// Defaults: fast_period = 12, slow_period = 26, signal_period = 9.
pub fn macd(values: &[f64], fast_period: usize, slow_period: usize, signal_period: usize) -> MacdResult {
    let fast_ema = ema(values, fast_period);
    let slow_ema = ema(values, slow_period);

    let n = values.len();
    let mut macd_line = vec![None; n];
    let mut valid_macd = Vec::new();
    let mut macd_valid_start = None;

    for i in 0..n {
        if let (Some(f), Some(s)) = (fast_ema[i], slow_ema[i]) {
            let diff = f - s;
            macd_line[i] = Some(diff);
            if macd_valid_start.is_none() {
                macd_valid_start = Some(i);
            }
            valid_macd.push(diff);
        }
    }

    let mut signal_line = vec![None; n];
    let mut hist = vec![None; n];

    if valid_macd.len() >= signal_period {
        let sig_sub = ema(&valid_macd, signal_period);
        let start_idx = macd_valid_start.unwrap_or(0);
        for j in 0..sig_sub.len() {
            let orig_idx = start_idx + j;
            signal_line[orig_idx] = sig_sub[j];
            if let (Some(m), Some(s)) = (macd_line[orig_idx], sig_sub[j]) {
                hist[orig_idx] = Some(m - s);
            }
        }
    }

    MacdResult { macd: macd_line, signal: signal_line, histogram: hist }
}

/// Bollinger Bands output.
#[derive(Debug, Clone, PartialEq)]
pub struct BollingerBands {
    pub middle: Vec<Option<f64>>,
    pub upper: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
    pub percent_b: Vec<Option<f64>>,
    pub bandwidth: Vec<Option<f64>>,
}

/// Bollinger Bands calculation (typically period = 20, multiplier = 2.0).
pub fn bollinger_bands(values: &[f64], period: usize, multiplier: f64) -> BollingerBands {
    let middle = sma(values, period);
    let n = values.len();
    let mut upper = vec![None; n];
    let mut lower = vec![None; n];
    let mut percent_b = vec![None; n];
    let mut bandwidth = vec![None; n];

    for i in 0..n {
        if let Some(m) = middle[i] {
            let slice = &values[(i + 1 - period)..=i];
            let variance: f64 = slice.iter().map(|v| (v - m).powi(2)).sum::<f64>() / period as f64;
            let std_dev = variance.sqrt();
            let up = m + multiplier * std_dev;
            let low = m - multiplier * std_dev;
            upper[i] = Some(up);
            lower[i] = Some(low);

            let spread = up - low;
            if spread != 0.0 {
                percent_b[i] = Some((values[i] - low) / spread);
            }
            if m != 0.0 {
                bandwidth[i] = Some(spread / m);
            }
        }
    }

    BollingerBands { middle, upper, lower, percent_b, bandwidth }
}

/// Average True Range (ATR) with Wilder smoothing over `period` (typically 14).
pub fn atr(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    if period == 0 || bars.is_empty() {
        return vec![None; bars.len()];
    }
    let n = bars.len();
    let mut tr = Vec::with_capacity(n);

    for i in 0..n {
        let h = bars[i].high.to_f64();
        let l = bars[i].low.to_f64();
        if i == 0 {
            tr.push(h - l);
        } else {
            let prev_c = bars[i - 1].close.to_f64();
            let val = (h - l).max((h - prev_c).abs()).max((l - prev_c).abs());
            tr.push(val);
        }
    }

    if n < period {
        return vec![None; n];
    }

    let mut out = vec![None; n];
    let mut smoothed: f64 = tr[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(smoothed);

    let p = period as f64;
    for i in period..n {
        smoothed = ((p - 1.0) * smoothed + tr[i]) / p;
        out[i] = Some(smoothed);
    }
    out
}

/// Stochastic Oscillator result.
#[derive(Debug, Clone, PartialEq)]
pub struct StochasticResult {
    pub percent_k: Vec<Option<f64>>,
    pub percent_d: Vec<Option<f64>>,
    pub slow_d: Vec<Option<f64>>,
}

/// Stochastic Oscillator (%K, fast %D, slow %D). Default k_period = 14, d_period = 3, slow_period = 3.
pub fn stochastic(bars: &[Bar], k_period: usize, d_period: usize, slow_period: usize) -> StochasticResult {
    let n = bars.len();
    let mut percent_k = vec![None; n];

    for i in 0..n {
        if i + 1 < k_period {
            continue;
        }
        let slice = &bars[(i + 1 - k_period)..=i];
        let highest_high = slice.iter().map(|b| b.high.to_f64()).fold(f64::NEG_INFINITY, f64::max);
        let lowest_low = slice.iter().map(|b| b.low.to_f64()).fold(f64::INFINITY, f64::min);
        let current_c = bars[i].close.to_f64();

        let den = highest_high - lowest_low;
        if den > 0.0 {
            percent_k[i] = Some(100.0 * (current_c - lowest_low) / den);
        } else {
            percent_k[i] = Some(50.0);
        }
    }

    // Helper to calculate SMA of Option<f64>
    fn sma_opt(series: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
        let mut out = vec![None; series.len()];
        for i in 0..series.len() {
            if i + 1 < period {
                continue;
            }
            let slice = &series[(i + 1 - period)..=i];
            if slice.iter().all(|x| x.is_some()) {
                let s: f64 = slice.iter().map(|x| x.unwrap()).sum();
                out[i] = Some(s / period as f64);
            }
        }
        out
    }

    let percent_d = sma_opt(&percent_k, d_period);
    let slow_d = sma_opt(&percent_d, slow_period);

    StochasticResult { percent_k, percent_d, slow_d }
}

/// Average Directional Index (ADX) result.
#[derive(Debug, Clone, PartialEq)]
pub struct AdxResult {
    pub adx: Vec<Option<f64>>,
    pub plus_di: Vec<Option<f64>>,
    pub minus_di: Vec<Option<f64>>,
}

/// Average Directional Index (ADX) with +DI and -DI (period typically 14).
pub fn adx(bars: &[Bar], period: usize) -> AdxResult {
    let n = bars.len();
    if period == 0 || n < period * 2 {
        return AdxResult { adx: vec![None; n], plus_di: vec![None; n], minus_di: vec![None; n] };
    }

    let mut tr = Vec::with_capacity(n);
    let mut plus_dm = Vec::with_capacity(n);
    let mut minus_dm = Vec::with_capacity(n);

    tr.push(bars[0].high.to_f64() - bars[0].low.to_f64());
    plus_dm.push(0.0);
    minus_dm.push(0.0);

    for i in 1..n {
        let h = bars[i].high.to_f64();
        let l = bars[i].low.to_f64();
        let prev_h = bars[i - 1].high.to_f64();
        let prev_l = bars[i - 1].low.to_f64();
        let prev_c = bars[i - 1].close.to_f64();

        let current_tr = (h - l).max((h - prev_c).abs()).max((l - prev_c).abs());
        tr.push(current_tr);

        let up_move = h - prev_h;
        let down_move = prev_l - l;

        if up_move > down_move && up_move > 0.0 {
            plus_dm.push(up_move);
        } else {
            plus_dm.push(0.0);
        }

        if down_move > up_move && down_move > 0.0 {
            minus_dm.push(down_move);
        } else {
            minus_dm.push(0.0);
        }
    }

    let p = period as f64;
    let mut smooth_tr = tr[..period].iter().sum::<f64>();
    let mut smooth_pdm = plus_dm[..period].iter().sum::<f64>();
    let mut smooth_mdm = minus_dm[..period].iter().sum::<f64>();

    let mut plus_di = vec![None; n];
    let mut minus_di = vec![None; n];
    let mut dx = vec![None; n];

    let calc_di = |pdm: f64, mdm: f64, atr_val: f64| -> (f64, f64, f64) {
        if atr_val == 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let p_di = 100.0 * pdm / atr_val;
        let m_di = 100.0 * mdm / atr_val;
        let sum = p_di + m_di;
        let diff = (p_di - m_di).abs();
        let dx_val = if sum == 0.0 { 0.0 } else { 100.0 * diff / sum };
        (p_di, m_di, dx_val)
    };

    let (p0, m0, d0) = calc_di(smooth_pdm, smooth_mdm, smooth_tr);
    plus_di[period - 1] = Some(p0);
    minus_di[period - 1] = Some(m0);
    dx[period - 1] = Some(d0);

    for i in period..n {
        smooth_tr = smooth_tr - (smooth_tr / p) + tr[i];
        smooth_pdm = smooth_pdm - (smooth_pdm / p) + plus_dm[i];
        smooth_mdm = smooth_mdm - (smooth_mdm / p) + minus_dm[i];

        let (p_di, m_di, dx_val) = calc_di(smooth_pdm, smooth_mdm, smooth_tr);
        plus_di[i] = Some(p_di);
        minus_di[i] = Some(m_di);
        dx[i] = Some(dx_val);
    }

    let mut adx_out = vec![None; n];
    let adx_start = 2 * period - 1;
    if n > adx_start {
        let first_adx: f64 = dx[(period - 1)..adx_start].iter().filter_map(|&x| x).sum::<f64>() / p;
        adx_out[adx_start - 1] = Some(first_adx);

        let mut prev_adx = first_adx;
        for i in adx_start..n {
            if let Some(cur_dx) = dx[i] {
                prev_adx = ((prev_adx * (p - 1.0)) + cur_dx) / p;
                adx_out[i] = Some(prev_adx);
            }
        }
    }

    AdxResult { adx: adx_out, plus_di, minus_di }
}

/// On-Balance Volume (OBV).
pub fn obv(bars: &[Bar]) -> Vec<Option<f64>> {
    let n = bars.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }

    let mut current_obv = bars[0].volume.to_f64();
    out.push(Some(current_obv));

    for i in 1..n {
        let c = bars[i].close.to_f64();
        let prev_c = bars[i - 1].close.to_f64();
        let vol = bars[i].volume.to_f64();

        if c > prev_c {
            current_obv += vol;
        } else if c < prev_c {
            current_obv -= vol;
        }
        out.push(Some(current_obv));
    }
    out
}

/// Volume Weighted Average Price (VWAP) running cumulatively across the series.
pub fn vwap(bars: &[Bar]) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(bars.len());
    let mut cum_tp_vol = 0.0;
    let mut cum_vol = 0.0;

    for b in bars {
        let h = b.high.to_f64();
        let l = b.low.to_f64();
        let c = b.close.to_f64();
        let v = b.volume.to_f64();

        let tp = (h + l + c) / 3.0;
        cum_tp_vol += tp * v;
        cum_vol += v;

        if cum_vol > 0.0 {
            out.push(Some(cum_tp_vol / cum_vol));
        } else {
            out.push(None);
        }
    }
    out
}

/// Ichimoku Kinko Hyo structure.
#[derive(Debug, Clone, PartialEq)]
pub struct IchimokuResult {
    pub tenkan_sen: Vec<Option<f64>>,
    pub kijun_sen: Vec<Option<f64>>,
    pub senkou_span_a: Vec<Option<f64>>,
    pub senkou_span_b: Vec<Option<f64>>,
    pub chikou_span: Vec<Option<f64>>,
}

/// Ichimoku Kinko Hyo (Tenkan: 9, Kijun: 26, Span B: 52, Displacement: 26).
pub fn ichimoku(bars: &[Bar], tenkan_p: usize, kijun_p: usize, span_b_p: usize) -> IchimokuResult {
    let n = bars.len();

    fn midpoint(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
        let mut out = vec![None; bars.len()];
        for i in 0..bars.len() {
            if i + 1 < period {
                continue;
            }
            let slice = &bars[(i + 1 - period)..=i];
            let max_h = slice.iter().map(|b| b.high.to_f64()).fold(f64::NEG_INFINITY, f64::max);
            let min_l = slice.iter().map(|b| b.low.to_f64()).fold(f64::INFINITY, f64::min);
            out[i] = Some((max_h + min_l) / 2.0);
        }
        out
    }

    let tenkan = midpoint(bars, tenkan_p);
    let kijun = midpoint(bars, kijun_p);
    let span_b = midpoint(bars, span_b_p);

    let mut span_a = vec![None; n];
    for i in 0..n {
        if let (Some(t), Some(k)) = (tenkan[i], kijun[i]) {
            span_a[i] = Some((t + k) / 2.0);
        }
    }

    let mut chikou = vec![None; n];
    for i in 0..n {
        chikou[i] = Some(bars[i].close.to_f64());
    }

    IchimokuResult {
        tenkan_sen: tenkan,
        kijun_sen: kijun,
        senkou_span_a: span_a,
        senkou_span_b: span_b,
        chikou_span: chikou,
    }
}

/// Parabolic SAR (Stop and Reverse) with standard Wilder acceleration.
pub fn parabolic_sar(bars: &[Bar], af_start: f64, af_increment: f64, af_max: f64) -> Vec<Option<f64>> {
    let n = bars.len();
    if n < 2 {
        return vec![None; n];
    }
    let mut sar = vec![None; n];

    // Determine initial trend
    let mut is_long = bars[1].close.to_f64() >= bars[0].close.to_f64();
    let mut af = af_start;
    let mut ep = if is_long { bars[0].high.to_f64() } else { bars[0].low.to_f64() };
    let mut current_sar = if is_long { bars[0].low.to_f64() } else { bars[0].high.to_f64() };

    sar[0] = Some(current_sar);

    for i in 1..n {
        let h = bars[i].high.to_f64();
        let l = bars[i].low.to_f64();
        let prev_h = bars[i - 1].high.to_f64();
        let prev_l = bars[i - 1].low.to_f64();

        let mut next_sar = current_sar + af * (ep - current_sar);

        if is_long {
            next_sar = next_sar.min(prev_l);
            if i >= 2 {
                next_sar = next_sar.min(bars[i - 2].low.to_f64());
            }
            if l < next_sar {
                // Reversal to short
                is_long = false;
                current_sar = ep.max(h);
                ep = l;
                af = af_start;
            } else {
                current_sar = next_sar;
                if h > ep {
                    ep = h;
                    af = (af + af_increment).min(af_max);
                }
            }
        } else {
            next_sar = next_sar.max(prev_h);
            if i >= 2 {
                next_sar = next_sar.max(bars[i - 2].high.to_f64());
            }
            if h > next_sar {
                // Reversal to long
                is_long = true;
                current_sar = ep.min(l);
                ep = h;
                af = af_start;
            } else {
                current_sar = next_sar;
                if l < ep {
                    ep = l;
                    af = (af + af_increment).min(af_max);
                }
            }
        }
        sar[i] = Some(current_sar);
    }
    sar
}

/// Commodity Channel Index (CCI) over `period` (typically 20).
pub fn cci(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let n = bars.len();
    if period == 0 || n < period {
        return vec![None; n];
    }
    let tp: Vec<f64> =
        bars.iter().map(|b| (b.high.to_f64() + b.low.to_f64() + b.close.to_f64()) / 3.0).collect();

    let sma_tp = sma(&tp, period);
    let mut out = vec![None; n];

    for i in 0..n {
        if let Some(mean_tp) = sma_tp[i] {
            let slice = &tp[(i + 1 - period)..=i];
            let mean_dev = slice.iter().map(|&x| (x - mean_tp).abs()).sum::<f64>() / period as f64;
            if mean_dev > 0.0 {
                out[i] = Some((tp[i] - mean_tp) / (0.015 * mean_dev));
            } else {
                out[i] = Some(0.0);
            }
        }
    }
    out
}

/// Williams %R (period typically 14, values between -100 and 0).
pub fn williams_r(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let n = bars.len();
    let mut out = vec![None; n];
    for i in 0..n {
        if i + 1 < period {
            continue;
        }
        let slice = &bars[(i + 1 - period)..=i];
        let highest = slice.iter().map(|b| b.high.to_f64()).fold(f64::NEG_INFINITY, f64::max);
        let lowest = slice.iter().map(|b| b.low.to_f64()).fold(f64::INFINITY, f64::min);
        let c = bars[i].close.to_f64();

        let den = highest - lowest;
        if den > 0.0 {
            out[i] = Some(-100.0 * (highest - c) / den);
        } else {
            out[i] = Some(-50.0);
        }
    }
    out
}

/// Channel output for Donchian and Keltner channels.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelResult {
    pub upper: Vec<Option<f64>>,
    pub middle: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

/// Donchian Channels (period typically 20).
pub fn donchian_channels(bars: &[Bar], period: usize) -> ChannelResult {
    let n = bars.len();
    let mut upper = vec![None; n];
    let mut lower = vec![None; n];
    let mut middle = vec![None; n];

    for i in 0..n {
        if i + 1 < period {
            continue;
        }
        let slice = &bars[(i + 1 - period)..=i];
        let up = slice.iter().map(|b| b.high.to_f64()).fold(f64::NEG_INFINITY, f64::max);
        let low = slice.iter().map(|b| b.low.to_f64()).fold(f64::INFINITY, f64::min);
        upper[i] = Some(up);
        lower[i] = Some(low);
        middle[i] = Some((up + low) / 2.0);
    }

    ChannelResult { upper, middle, lower }
}

/// Keltner Channels (middle = EMA, upper/lower = middle +- multiplier * ATR).
pub fn keltner_channels(bars: &[Bar], period: usize, multiplier: f64) -> ChannelResult {
    let n = bars.len();
    let closes: Vec<f64> = bars.iter().map(|b| b.close.to_f64()).collect();
    let middle_ema = ema(&closes, period);
    let atr_vals = atr(bars, period);

    let mut upper = vec![None; n];
    let mut lower = vec![None; n];

    for i in 0..n {
        if let (Some(m), Some(a)) = (middle_ema[i], atr_vals[i]) {
            upper[i] = Some(m + multiplier * a);
            lower[i] = Some(m - multiplier * a);
        }
    }

    ChannelResult { upper, middle: middle_ema, lower }
}

/// Chaikin Money Flow (CMF) over `period` (typically 20).
pub fn chaikin_money_flow(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let n = bars.len();
    let mut mfv = Vec::with_capacity(n);
    let mut vols = Vec::with_capacity(n);

    for b in bars {
        let h = b.high.to_f64();
        let l = b.low.to_f64();
        let c = b.close.to_f64();
        let v = b.volume.to_f64();

        let spread = h - l;
        let mult = if spread > 0.0 { ((c - l) - (h - c)) / spread } else { 0.0 };
        mfv.push(mult * v);
        vols.push(v);
    }

    let mut out = vec![None; n];
    for i in 0..n {
        if i + 1 < period {
            continue;
        }
        let mfv_sum: f64 = mfv[(i + 1 - period)..=i].iter().sum();
        let vol_sum: f64 = vols[(i + 1 - period)..=i].iter().sum();
        if vol_sum > 0.0 {
            out[i] = Some(mfv_sum / vol_sum);
        } else {
            out[i] = Some(0.0);
        }
    }
    out
}

/// Momentum: `Close_t - Close_{t - period}` (period typically 10).
pub fn momentum(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        if i >= period {
            out.push(Some(values[i] - values[i - period]));
        } else {
            out.push(None);
        }
    }
    out
}

/// Rate of Change (ROC): `100 * (Close_t - Close_{t - period}) / Close_{t - period}` (period typically 12).
pub fn rate_of_change(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        if i >= period {
            let base = values[i - period];
            if base != 0.0 {
                out.push(Some(100.0 * (values[i] - base) / base));
            } else {
                out.push(None);
            }
        } else {
            out.push(None);
        }
    }
    out
}

/// Money Flow Index (MFI) over `period` (typically 14, values 0 to 100).
pub fn mfi(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let n = bars.len();
    if period == 0 || n <= period {
        return vec![None; n];
    }
    let tp: Vec<f64> =
        bars.iter().map(|b| (b.high.to_f64() + b.low.to_f64() + b.close.to_f64()) / 3.0).collect();

    let mut pos_flow = vec![0.0; n];
    let mut neg_flow = vec![0.0; n];

    for i in 1..n {
        let rmf = tp[i] * bars[i].volume.to_f64();
        if tp[i] > tp[i - 1] {
            pos_flow[i] = rmf;
        } else if tp[i] < tp[i - 1] {
            neg_flow[i] = rmf;
        }
    }

    let mut out = vec![None; n];
    for i in period..n {
        let pf: f64 = pos_flow[(i + 1 - period)..=i].iter().sum();
        let nf: f64 = neg_flow[(i + 1 - period)..=i].iter().sum();

        if nf == 0.0 {
            out[i] = Some(100.0);
        } else {
            let mr = pf / nf;
            out[i] = Some(100.0 - (100.0 / (1.0 + mr)));
        }
    }
    out
}

/// Rolling sample standard deviation over `period`.
pub fn std_dev(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period <= 1 {
        return vec![None; values.len()];
    }
    let sma_vals = sma(values, period);
    let mut out = vec![None; values.len()];

    for i in 0..values.len() {
        if let Some(mean) = sma_vals[i] {
            let slice = &values[(i + 1 - period)..=i];
            let variance: f64 = slice.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / (period - 1) as f64;
            out[i] = Some(variance.sqrt());
        }
    }
    out
}

/// Typical Price: `(High + Low + Close) / 3`.
pub fn typical_price(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| (b.high.to_f64() + b.low.to_f64() + b.close.to_f64()) / 3.0).collect()
}

/// Median Price: `(High + Low) / 2`.
pub fn median_price(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| (b.high.to_f64() + b.low.to_f64()) / 2.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::{Price, Qty};

    fn make_test_bar(ts: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
        Bar::new(
            ts,
            0,
            Price::from_f64(o).unwrap(),
            Price::from_f64(h).unwrap(),
            Price::from_f64(l).unwrap(),
            Price::from_f64(c).unwrap(),
            Qty::from_f64(v).unwrap(),
        )
    }

    #[test]
    fn test_sma_calculation() {
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let res = sma(&prices, 3);
        assert_eq!(res, vec![None, None, Some(2.0), Some(3.0), Some(4.0)]);
    }

    #[test]
    fn test_ema_calculation() {
        let prices = vec![2.0, 4.0, 6.0, 8.0, 10.0];
        let res = ema(&prices, 3);
        assert_eq!(res[0], None);
        assert_eq!(res[1], None);
        assert_eq!(res[2], Some(4.0)); // SMA seed = (2+4+6)/3 = 4.0
                                       // alpha = 2/(3+1) = 0.5; next = 8.0 * 0.5 + 4.0 * 0.5 = 6.0
        assert_eq!(res[3], Some(6.0));
        // next = 10.0 * 0.5 + 6.0 * 0.5 = 8.0
        assert_eq!(res[4], Some(8.0));
    }

    #[test]
    fn test_wma_calculation() {
        let prices = vec![1.0, 2.0, 3.0, 4.0];
        let res = wma(&prices, 3);
        // weights: 1, 2, 3 -> sum = 6
        // idx 2: (1*1 + 2*2 + 3*3)/6 = (1+4+9)/6 = 14/6 = 2.3333333333333335
        assert_eq!(res[0], None);
        assert_eq!(res[1], None);
        assert!((res[2].unwrap() - 14.0 / 6.0).abs() < 1e-12);
        // idx 3: (2*1 + 3*2 + 4*3)/6 = (2+6+12)/6 = 20/6 = 3.3333333333333335
        assert!((res[3].unwrap() - 20.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn test_rsi_calculation() {
        let prices = vec![
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03, 45.61, 46.28,
            46.28, 46.00,
        ];
        let res = rsi(&prices, 14);
        assert_eq!(res[0], None);
        assert_eq!(res[13], None);
        assert!(res[14].is_some());
        let val = res[14].unwrap();
        // Standard Wilder 14-period RSI check: should be in (60, 80)
        assert!(val > 60.0 && val < 80.0, "RSI was {}", val);
    }

    #[test]
    fn test_macd_calculation() {
        let prices: Vec<f64> = (1..=40).map(|x| x as f64).collect();
        let res = macd(&prices, 12, 26, 9);
        assert_eq!(res.macd[24], None);
        assert!(res.macd[25].is_some());
        assert!(res.signal[33].is_some());
        assert!(res.histogram[33].is_some());
    }

    #[test]
    fn test_bollinger_bands() {
        let prices = vec![10.0, 11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0];
        let bb = bollinger_bands(&prices, 5, 2.0);
        assert!(bb.middle[4].is_some());
        assert!(bb.upper[4].is_some());
        assert!(bb.lower[4].is_some());
        assert!(bb.upper[4].unwrap() > bb.middle[4].unwrap());
        assert!(bb.lower[4].unwrap() < bb.middle[4].unwrap());
    }

    #[test]
    fn test_atr_and_stochastic_and_indicators() {
        let bars = vec![
            make_test_bar(1000, 10.0, 12.0, 9.0, 11.0, 100.0),
            make_test_bar(2000, 11.0, 13.0, 10.0, 12.0, 150.0),
            make_test_bar(3000, 12.0, 14.0, 11.0, 13.0, 200.0),
            make_test_bar(4000, 13.0, 13.5, 11.5, 12.5, 180.0),
            make_test_bar(5000, 12.5, 15.0, 12.0, 14.5, 250.0),
        ];

        let atr_res = atr(&bars, 3);
        assert!(atr_res[2].is_some());

        let stoch_res = stochastic(&bars, 3, 2, 2);
        assert!(stoch_res.percent_k[2].is_some());

        let obv_res = obv(&bars);
        assert_eq!(obv_res.len(), 5);

        let vwap_res = vwap(&bars);
        assert_eq!(vwap_res.len(), 5);
        assert!(vwap_res[0].is_some());

        let psar = parabolic_sar(&bars, 0.02, 0.02, 0.2);
        assert_eq!(psar.len(), 5);

        let cmf = chaikin_money_flow(&bars, 3);
        assert!(cmf[2].is_some());

        let donchian = donchian_channels(&bars, 3);
        assert!(donchian.upper[2].is_some());

        let keltner = keltner_channels(&bars, 3, 1.5);
        assert!(keltner.upper[2].is_some());

        let tp = typical_price(&bars);
        assert_eq!(tp.len(), 5);
        assert!((tp[0] - (12.0 + 9.0 + 11.0) / 3.0).abs() < 1e-12);
    }
}
