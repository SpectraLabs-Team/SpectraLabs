# M38: Formula Language & Scanner Specification

**Status**: Implemented & Verified (M38 Tasks 1-9)  
**Implementation**: `crates/spectra-research/src/formula.rs`  
**Tests**: `crates/spectra-research/tests/m38_formula_reference.rs`

## 1. Syntax & Grammar (Task 1)
- **Primary Fields**: `Open`, `High`, `Low`, `Close`, `Volume`
- **Operators**:
  - Arithmetic: `+`, `-`, `*`, `/`
  - Comparison: `>`, `<`, `>=`, `<=`, `==`, `!=`
  - Boolean: `AND` / `&&`, `OR` / `||`, `NOT` / `!`
- **Built-in Functions**:
  - `Ref(series, offset)`: Lags the series by `abs(offset)`. **Guard**: Positive offsets reference future bars and are rejected with `LookaheadForbidden` (Task 9).
  - `Mov(series, period, mode)`: Moving average. `mode = 0` (SMA), `mode = 1` (EMA), `mode = 2` (WMA). Guaranteed bit-for-bit parity with M35 built-ins (Task 3).
  - `Rsi(series, period)`: Relative Strength Index with Wilder smoothing.
  - `Cross(s1, s2)`: Returns 1.0 on bars where `s1` crosses above `s2`, 0.0 otherwise.
  - `Hhv(series, period)`: Highest high value over period.
  - `Llv(series, period)`: Lowest low value over period.

## 2. Sandbox Limits (Task 2 / M32 Guard)
- Evaluator enforces `max_steps` operation limit. Runaway or computationally excessive formulas abort with `FormulaError::ExecutionLimitExceeded`.
- Memory allocated strictly proportional to series length; zero network or filesystem calls allowed in evaluation context.

## 3. Idea Scanner (Task 8)
- Scans user universe using `run_idea_scanner(rule_name, formula, universe, max_steps)`.
- Generates `IdeaSignal` objects with symbol, timestamp, rule name, and trigger value.
