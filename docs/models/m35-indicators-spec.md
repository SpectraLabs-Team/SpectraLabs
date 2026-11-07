# M35: Indicator Engine & Formula Reference Specification

**Status**: Ready for Product Owner Sign-off (M35 Task 3)  
**Implementation**: `crates/spectra-research/src/indicators.rs`  
**Tests**: `crates/spectra-research/tests/m35_indicator_reference.rs` (25/25 passing)

This specification documents the mathematical formulation, parameters, warm-up rules, and tolerances for the 25 indicators in SpectraLabs Market Research Layer (M35).

---

## 1. Moving Averages & Trend Overlays

### 1.1 Simple Moving Average (SMA)
- **Formula**: $\text{SMA}_t(n) = \frac{1}{n} \sum_{i=0}^{n-1} P_{t-i}$
- **Warm-up**: Produces `None` for $t < n - 1$.
- **Pane**: Price Overlay.

### 1.2 Exponential Moving Average (EMA)
- **Formula**: $\alpha = \frac{2}{n + 1}$, $\text{EMA}_t = \alpha P_t + (1 - \alpha)\text{EMA}_{t-1}$. Seeded with $n$-period SMA.
- **Warm-up**: Produces `None` for $t < n - 1$.
- **Pane**: Price Overlay.

### 1.3 Weighted Moving Average (WMA)
- **Formula**: $\text{WMA}_t(n) = \frac{\sum_{i=1}^n i \cdot P_{t - n + i}}{n(n+1)/2}$
- **Warm-up**: Produces `None` for $t < n - 1$.
- **Pane**: Price Overlay.

### 1.4 Double Exponential Moving Average (DEMA)
- **Formula**: $\text{DEMA} = 2 \cdot \text{EMA}(P) - \text{EMA}(\text{EMA}(P))$
- **Warm-up**: Requires $2n - 2$ bars.
- **Pane**: Price Overlay.

### 1.5 Triple Exponential Moving Average (TEMA)
- **Formula**: $\text{TEMA} = 3 \cdot \text{EMA}_1 - 3 \cdot \text{EMA}_2 + \text{EMA}_3$
- **Warm-up**: Requires $3n - 3$ bars.
- **Pane**: Price Overlay.

### 1.6 Bollinger Bands
- **Formula**: $\text{Middle} = \text{SMA}(P, n)$, $\text{Upper} = \text{Middle} + k \cdot \sigma$, $\text{Lower} = \text{Middle} - k \cdot \sigma$, $\%B = \frac{P - \text{Lower}}{\text{Upper} - \text{Lower}}$, $\text{Bandwidth} = \frac{\text{Upper} - \text{Lower}}{\text{Middle}}$. Default $n=20, k=2.0$.
- **Pane**: Price Overlay + Optional Sub-pane (%B, Bandwidth).

### 1.7 Ichimoku Kinko Hyo
- **Formula**:
  - Tenkan-sen (Conversion Line, 9): $(\max_9(H) + \min_9(L)) / 2$
  - Kijun-sen (Base Line, 26): $(\max_{26}(H) + \min_{26}(L)) / 2$
  - Senkou Span A (Leading Span A, 26 displaced forward): $(\text{Tenkan} + \text{Kijun}) / 2$
  - Senkou Span B (Leading Span B, 52 displaced forward): $(\max_{52}(H) + \min_{52}(L)) / 2$
  - Chikou Span (Lagging Span, 26 displaced backward): $C_t$
- **Pane**: Price Overlay.

### 1.8 Parabolic SAR (Stop and Reverse)
- **Formula**: Wilder's trailing stop with acceleration factor $af$ starting at 0.02, incrementing by 0.02 up to 0.20 on each new extreme point.
- **Pane**: Price Overlay.

### 1.9 Donchian Channels
- **Formula**: Upper = $\max_n(H)$, Lower = $\min_n(L)$, Middle = $(\text{Upper} + \text{Lower}) / 2$. Default $n=20$.
- **Pane**: Price Overlay.

### 1.10 Keltner Channels
- **Formula**: Middle = $\text{EMA}(C, n)$, Upper = Middle + $m \cdot \text{ATR}(n)$, Lower = Middle - $m \cdot \text{ATR}(n)$. Default $n=20, m=2.0$.
- **Pane**: Price Overlay.

### 1.11 Volume-Weighted Average Price (VWAP)
- **Formula**: $\text{VWAP}_t = \frac{\sum_{i=1}^t \text{TypicalPrice}_i \cdot \text{Volume}_i}{\sum_{i=1}^t \text{Volume}_i}$
- **Pane**: Price Overlay.

### 1.12 Typical Price & 1.13 Median Price
- **Formula**: $\text{Typical} = \frac{H + L + C}{3}$, $\text{Median} = \frac{H + L}{2}$.
- **Pane**: Data transformation / price series.

---

## 2. Momentum & Oscillators (Sub-Panes)

### 2.1 Relative Strength Index (RSI)
- **Formula**: Wilder smoothing of upward gains and downward losses over period $n$ (default 14).
  $\text{RS} = \frac{\text{AvgGain}}{\text{AvgLoss}}$, $\text{RSI} = 100 - \frac{100}{1 + \text{RS}}$.
- **Range**: $[0, 100]$. Overbought: 70, Oversold: 30.
- **Pane**: Sub-pane.

### 2.2 Moving Average Convergence Divergence (MACD)
- **Formula**: MACD Line = $\text{EMA}(C, 12) - \text{EMA}(C, 26)$, Signal Line = $\text{EMA}(\text{MACD}, 9)$, Histogram = MACD - Signal.
- **Pane**: Sub-pane.

### 2.3 Stochastic Oscillator
- **Formula**: $\%K = 100 \cdot \frac{C_t - \min_n(L)}{\max_n(H) - \min_n(L)}$, Fast $\%D = \text{SMA}(\%K, 3)$, Slow $\%D = \text{SMA}(\text{Fast } \%D, 3)$.
- **Range**: $[0, 100]$.
- **Pane**: Sub-pane.

### 2.4 Commodity Channel Index (CCI)
- **Formula**: $\text{CCI} = \frac{\text{TP}_t - \text{SMA}(\text{TP}, n)}{0.015 \cdot \text{MeanDeviation}}$. Default $n=20$.
- **Pane**: Sub-pane.

### 2.5 Williams %R
- **Formula**: $\%R = -100 \cdot \frac{\max_n(H) - C_t}{\max_n(H) - \min_n(L)}$. Default $n=14$.
- **Range**: $[-100, 0]$.
- **Pane**: Sub-pane.

### 2.6 Momentum (MOM)
- **Formula**: $\text{MOM}_t = C_t - C_{t - n}$. Default $n=10$.
- **Pane**: Sub-pane.

### 2.7 Rate of Change (ROC)
- **Formula**: $\text{ROC}_t = 100 \cdot \frac{C_t - C_{t - n}}{C_{t - n}}$. Default $n=12$.
- **Pane**: Sub-pane.

---

## 3. Volatility & Volume Indicators

### 3.1 Average True Range (ATR)
- **Formula**: $\text{TR}_t = \max(H_t - L_t, |H_t - C_{t-1}|, |L_t - C_{t-1}|)$, smoothed via Wilder's moving average over period $n$ (default 14).
- **Pane**: Sub-pane.

### 3.2 Average Directional Index (ADX)
- **Formula**: Smoothed $+DI$ and $-DI$ based on directional movements, with Wilder smoothing for DX over period $n$ (default 14).
- **Pane**: Sub-pane.

### 3.3 On-Balance Volume (OBV)
- **Formula**: Cumulative sum of volume signed by close price direction ($C_t > C_{t-1} \to +V_t$, $C_t < C_{t-1} \to -V_t$).
- **Pane**: Sub-pane.

### 3.4 Chaikin Money Flow (CMF)
- **Formula**: Money Flow Multiplier $\times$ Volume over period $n$ divided by sum of volume. Default $n=20$.
- **Range**: $[-1.0, 1.0]$.
- **Pane**: Sub-pane.

### 3.5 Money Flow Index (MFI)
- **Formula**: Volume-weighted RSI calculated from typical price changes over period $n$ (default 14).
- **Range**: $[0, 100]$.
- **Pane**: Sub-pane.

### 3.6 Rolling Standard Deviation (Volatility)
- **Formula**: $\sigma_t = \sqrt{\frac{1}{n-1} \sum_{i=0}^{n-1} (P_{t-i} - \bar{P})^2}$. Default $n=20$.
- **Pane**: Sub-pane.

---

## Sign-off Matrix

| Indicator | Symbol | Default Parameters | Reference Test Pinned | Status |
|:---|:---|:---|:---:|:---:|
| Simple Moving Average | `SMA` | `period: 20` | Yes | Verified |
| Exponential Moving Average | `EMA` | `period: 20` | Yes | Verified |
| Weighted Moving Average | `WMA` | `period: 20` | Yes | Verified |
| Double Exponential Moving Average | `DEMA` | `period: 20` | Yes | Verified |
| Triple Exponential Moving Average | `TEMA` | `period: 20` | Yes | Verified |
| Relative Strength Index | `RSI` | `period: 14` | Yes | Verified |
| Moving Average Convergence Divergence | `MACD` | `fast: 12, slow: 26, signal: 9` | Yes | Verified |
| Bollinger Bands | `BB` | `period: 20, mult: 2.0` | Yes | Verified |
| Average True Range | `ATR` | `period: 14` | Yes | Verified |
| Stochastic Oscillator | `STOCH` | `k: 14, d: 3, slow_d: 3` | Yes | Verified |
| Average Directional Index | `ADX` | `period: 14` | Yes | Verified |
| On-Balance Volume | `OBV` | `none` | Yes | Verified |
| Volume-Weighted Average Price | `VWAP` | `none (cumulative)` | Yes | Verified |
| Ichimoku Kinko Hyo | `ICHIMOKU`| `tenkan: 9, kijun: 26, span_b: 52` | Yes | Verified |
| Parabolic SAR | `PSAR` | `start: 0.02, step: 0.02, max: 0.20` | Yes | Verified |
| Commodity Channel Index | `CCI` | `period: 20` | Yes | Verified |
| Williams %R | `WILLR` | `period: 14` | Yes | Verified |
| Donchian Channels | `DONCHIAN`| `period: 20` | Yes | Verified |
| Keltner Channels | `KELTNER` | `period: 20, mult: 1.5` | Yes | Verified |
| Chaikin Money Flow | `CMF` | `period: 20` | Yes | Verified |
| Momentum | `MOM` | `period: 10` | Yes | Verified |
| Rate of Change | `ROC` | `period: 12` | Yes | Verified |
| Money Flow Index | `MFI` | `period: 14` | Yes | Verified |
| Standard Deviation | `STDDEV` | `period: 20` | Yes | Verified |
| Typical & Median Price | `TYPICAL` / `MEDIAN` | `none` | Yes | Verified |
