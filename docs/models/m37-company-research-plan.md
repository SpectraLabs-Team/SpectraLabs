# M37: Company Research & Ratio Engine Specification

**Status**: Implemented & Verified (M37 Tasks 1-7)  
**Implementation**: `crates/spectra-research/src/company_research.rs`  
**Tests**: `crates/spectra-research/tests/m37_company_research_reference.rs`

## 1. Scope & Architecture
Provides single-company fundamental analysis, financial statement ratio derivation, peer comparisons, valuation percentiles, and objective financial health evaluation without investment advice.

## 2. Ratio Definitions (Task 3)
- **Valuation Multiples**:
  - P/E = $\frac{\text{Share Price}}{\text{EPS}}$
  - P/B = $\frac{\text{Share Price}}{\text{Book Value per Share}}$
  - P/S = $\frac{\text{Market Capitalization}}{\text{Revenue}}$
  - EV/EBITDA = $\frac{\text{Enterprise Value}}{\text{Operating Income}}$ where $\text{EV} = \text{Market Cap} + \text{Total Debt} - \text{Cash}$
  - P/FCF = $\frac{\text{Market Capitalization}}{\text{Free Cash Flow}}$
- **Profitability**:
  - Gross Margin = $\frac{\text{Gross Profit}}{\text{Revenue}}$
  - Operating Margin = $\frac{\text{Operating Income}}{\text{Revenue}}$
  - Net Margin = $\frac{\text{Net Income}}{\text{Revenue}}$
  - Return on Equity (ROE) = $\frac{\text{Net Income}}{\text{Shareholders' Equity}}$
  - Return on Assets (ROA) = $\frac{\text{Net Income}}{\text{Total Assets}}$
- **Leverage & Solvency**:
  - Debt-to-Equity = $\frac{\text{Total Debt}}{\text{Shareholders' Equity}}$
  - Debt-to-Assets = $\frac{\text{Total Debt}}{\text{Total Assets}}$
  - Interest Coverage = $\frac{\text{Operating Income}}{\text{Interest Expense}}$
- **Liquidity**:
  - Current Ratio = $\frac{\text{Current Assets}}{\text{Current Liabilities}}$
  - Quick Ratio = $\frac{\text{Current Assets} - \text{Inventory}}{\text{Current Liabilities}}$
  - Cash Ratio = $\frac{\text{Cash \& Equivalents}}{\text{Current Liabilities}}$

## 3. Data Integrity & Statement Tie-Out (Task 2 & Risk Mitigation)
- Vendor reporting errors are caught by `verify_tie_out(tolerance)`:
  $$\text{Total Assets} = \text{Total Liabilities} + \text{Stockholders' Equity}$$
  If balance sheet fails tie-out beyond tolerance, the record is flagged before displaying to users.

## 4. Valuation History & Empirical Percentile (Task 5)
- Percentile rank formula:
  $$\text{Percentile} = 100 \times \frac{\text{count}(M_i \le M_t)}{N}$$
  Gives user historical context of multiple expensiveness.

## 5. Financial Health Scorecard (Task 7 & Compliance Rules)
- Objective scoring with 6 transparent rules:
  1. Positive Net Income
  2. Positive Operating Cash Flow
  3. Cash Flow exceeds Net Income (high accrual quality)
  4. Prudent leverage (Debt/Equity < 1.5)
  5. Adequate working capital (Current Ratio $\ge 1.0$)
  6. Gross margin expansion YoY
- **Compliance Guard**: No buy/sell/hold recommendation wording or labels. All scoring criteria are explicitly published.
