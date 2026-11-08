//! Formula Language & Idea Scanner Engine (M38).
//!
//! Provides a safe, sandboxed expression grammar and parser for writing custom indicators
//! and trading conditions. Supports technical primitives (Ref, Mov, Rsi, Cross, Hhv, Llv),
//! look-ahead prevention (disallows future bar referencing), bounded step execution,
//! and universe-wide idea scanning.

use crate::indicators::{ema, rsi, sma, wma};
use spectra_core::Bar;

/// Series price/volume fields available in formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesField {
    Open,
    High,
    Low,
    Close,
    Volume,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
    Neq,
    And,
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// Abstract Syntax Tree (AST) node for formula expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Field(SeriesField),
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, expr: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
}

/// Errors occurring during formula lexing, parsing, or evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormulaError {
    UnexpectedToken(String),
    UnexpectedEof,
    InvalidSyntax(String),
    UnknownFunction(String),
    InvalidArgumentCount { name: String, expected: usize, got: usize },
    InvalidArguments(String),
    ExecutionLimitExceeded,
    LookaheadForbidden,
}

impl std::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedToken(s) => write!(f, "unexpected token: {}", s),
            Self::UnexpectedEof => write!(f, "unexpected end of formula"),
            Self::InvalidSyntax(s) => write!(f, "syntax error: {}", s),
            Self::UnknownFunction(s) => write!(f, "unknown function: {}", s),
            Self::InvalidArgumentCount { name, expected, got } => {
                write!(f, "function '{}' expects {} arguments, got {}", name, expected, got)
            }
            Self::InvalidArguments(s) => write!(f, "invalid argument: {}", s),
            Self::ExecutionLimitExceeded => {
                write!(f, "formula execution step limit exceeded (sandbox guard)")
            }
            Self::LookaheadForbidden => write!(
                f,
                "future referencing (positive lag) is strictly forbidden to prevent look-ahead bias"
            ),
        }
    }
}

impl std::error::Error for FormulaError {}

// --- LEXER ---

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    StringLit(String),
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    Lt,
    Gte,
    Lte,
    EqEq,
    Neq,
    And,
    Or,
    Not,
    Comma,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<Token>, FormulaError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let mut num_str = String::new();
            let mut has_dot = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !has_dot)) {
                if chars[i] == '.' {
                    has_dot = true;
                }
                num_str.push(chars[i]);
                i += 1;
            }
            let val: f64 = num_str
                .parse()
                .map_err(|_| FormulaError::InvalidSyntax(format!("bad number: {}", num_str)))?;
            tokens.push(Token::Number(val));
            continue;
        }

        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != quote {
                s.push(chars[i]);
                i += 1;
            }
            if i >= chars.len() {
                return Err(FormulaError::InvalidSyntax("unterminated string literal".into()));
            }
            i += 1; // skip closing quote
            tokens.push(Token::StringLit(s));
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let mut ident = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                ident.push(chars[i]);
                i += 1;
            }
            let upper = ident.to_uppercase();
            match upper.as_str() {
                "AND" => tokens.push(Token::And),
                "OR" => tokens.push(Token::Or),
                "NOT" => tokens.push(Token::Not),
                _ => tokens.push(Token::Ident(ident)),
            }
            continue;
        }

        match c {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            ',' => tokens.push(Token::Comma),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Gte);
                    i += 1;
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Lte);
                    i += 1;
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::EqEq);
                    i += 1;
                } else {
                    tokens.push(Token::EqEq); // Allow '=' as equality in formulas
                }
            }
            '!' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Neq);
                    i += 1;
                } else {
                    tokens.push(Token::Not);
                }
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    i += 1;
                }
                tokens.push(Token::And);
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    i += 1;
                }
                tokens.push(Token::Or);
            }
            _ => return Err(FormulaError::UnexpectedToken(c.to_string())),
        }
        i += 1;
    }

    Ok(tokens)
}

// --- PARSER ---

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next_token(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn parse_expression(&mut self) -> Result<Expr, FormulaError> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_logical_and()?;
        while let Some(Token::Or) = self.peek() {
            self.next_token();
            let right = self.parse_logical_and()?;
            left = Expr::Binary { op: BinaryOp::Or, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_equality()?;
        while let Some(Token::And) = self.peek() {
            self.next_token();
            let right = self.parse_equality()?;
            left = Expr::Binary { op: BinaryOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_relational()?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::EqEq => BinaryOp::Eq,
                Token::Neq => BinaryOp::Neq,
                _ => break,
            };
            self.next_token();
            let right = self.parse_relational()?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_additive()?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Gt => BinaryOp::Gt,
                Token::Lt => BinaryOp::Lt,
                Token::Gte => BinaryOp::Gte,
                Token::Lte => BinaryOp::Lte,
                _ => break,
            };
            self.next_token();
            let right = self.parse_additive()?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_multiplicative()?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.next_token();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_unary()?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.next_token();
            let right = self.parse_unary()?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        if let Some(tok) = self.peek() {
            match tok {
                Token::Minus => {
                    self.next_token();
                    let expr = self.parse_unary()?;
                    return Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr) });
                }
                Token::Not => {
                    self.next_token();
                    let expr = self.parse_unary()?;
                    return Ok(Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr) });
                }
                _ => {}
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        let tok = self.next_token().ok_or(FormulaError::UnexpectedEof)?;
        match tok {
            Token::Number(n) => Ok(Expr::Number(n)),
            Token::Ident(name) => {
                let upper = name.to_uppercase();
                match upper.as_str() {
                    "OPEN" => Ok(Expr::Field(SeriesField::Open)),
                    "HIGH" => Ok(Expr::Field(SeriesField::High)),
                    "LOW" => Ok(Expr::Field(SeriesField::Low)),
                    "CLOSE" => Ok(Expr::Field(SeriesField::Close)),
                    "VOL" | "VOLUME" => Ok(Expr::Field(SeriesField::Volume)),
                    _ => {
                        // Function call
                        if let Some(Token::LParen) = self.peek() {
                            self.next_token();
                            let mut args = Vec::new();
                            if let Some(Token::RParen) = self.peek() {
                                self.next_token();
                            } else {
                                loop {
                                    args.push(self.parse_expression()?);
                                    match self.peek() {
                                        Some(Token::Comma) => {
                                            self.next_token();
                                        }
                                        Some(Token::RParen) => {
                                            self.next_token();
                                            break;
                                        }
                                        _ => {
                                            return Err(FormulaError::InvalidSyntax(
                                                "expected comma or ')' in argument list".into(),
                                            ))
                                        }
                                    }
                                }
                            }
                            Ok(Expr::Call { name, args })
                        } else {
                            Err(FormulaError::InvalidSyntax(format!("unknown variable: {}", name)))
                        }
                    }
                }
            }
            Token::LParen => {
                let inner = self.parse_expression()?;
                if let Some(Token::RParen) = self.next_token() {
                    Ok(inner)
                } else {
                    Err(FormulaError::InvalidSyntax("missing closing ')'".into()))
                }
            }
            _ => Err(FormulaError::UnexpectedToken(format!("{:?}", tok))),
        }
    }
}

/// Parses a formula expression into an AST.
pub fn parse_formula(src: &str) -> Result<Expr, FormulaError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expression()?;
    if parser.pos < parser.tokens.len() {
        return Err(FormulaError::InvalidSyntax("trailing tokens after expression".into()));
    }
    Ok(expr)
}

// --- EVALUATOR & SANDBOX ---

#[derive(Debug)]
pub struct Evaluator<'a> {
    bars: &'a [Bar],
    steps_count: usize,
    max_steps: usize,
}

impl<'a> Evaluator<'a> {
    pub fn new(bars: &'a [Bar], max_steps: usize) -> Self {
        Self { bars, steps_count: 0, max_steps }
    }

    fn check_limit(&mut self) -> Result<(), FormulaError> {
        self.steps_count += 1;
        if self.steps_count > self.max_steps {
            Err(FormulaError::ExecutionLimitExceeded)
        } else {
            Ok(())
        }
    }

    /// Evaluates `expr` producing a series of `Vec<Option<f64>>` aligned with `bars`.
    pub fn eval(&mut self, expr: &Expr) -> Result<Vec<Option<f64>>, FormulaError> {
        self.check_limit()?;
        let n = self.bars.len();
        match expr {
            Expr::Number(val) => Ok(vec![Some(*val); n]),
            Expr::Field(field) => {
                let mut out = Vec::with_capacity(n);
                for b in self.bars {
                    let v = match field {
                        SeriesField::Open => b.open.to_f64(),
                        SeriesField::High => b.high.to_f64(),
                        SeriesField::Low => b.low.to_f64(),
                        SeriesField::Close => b.close.to_f64(),
                        SeriesField::Volume => b.volume.to_f64(),
                    };
                    out.push(Some(v));
                }
                Ok(out)
            }
            Expr::Unary { op, expr } => {
                let inner = self.eval(expr)?;
                let mut out = Vec::with_capacity(n);
                for v in inner {
                    out.push(v.map(|x| match op {
                        UnaryOp::Neg => -x,
                        UnaryOp::Not => {
                            if x == 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    }));
                }
                Ok(out)
            }
            Expr::Binary { op, left, right } => {
                let l_vals = self.eval(left)?;
                let r_vals = self.eval(right)?;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    if let (Some(a), Some(b)) = (l_vals[i], r_vals[i]) {
                        let res = match op {
                            BinaryOp::Add => a + b,
                            BinaryOp::Sub => a - b,
                            BinaryOp::Mul => a * b,
                            BinaryOp::Div => {
                                if b != 0.0 {
                                    a / b
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Gt => {
                                if a > b {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Lt => {
                                if a < b {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Gte => {
                                if a >= b {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Lte => {
                                if a <= b {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Eq => {
                                if (a - b).abs() < 1e-9 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Neq => {
                                if (a - b).abs() >= 1e-9 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::And => {
                                if a != 0.0 && b != 0.0 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            BinaryOp::Or => {
                                if a != 0.0 || b != 0.0 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                        };
                        out.push(Some(res));
                    } else {
                        out.push(None);
                    }
                }
                Ok(out)
            }
            Expr::Call { name, args } => self.eval_call(name, args),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Vec<Option<f64>>, FormulaError> {
        let n = self.bars.len();
        let upper = name.to_uppercase();

        match upper.as_str() {
            "REF" => {
                if args.len() != 2 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Ref".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let series = self.eval(&args[0])?;
                let offset_series = self.eval(&args[1])?;
                let offset_f64 = offset_series.last().and_then(|x| *x).unwrap_or(0.0);
                let offset_int = offset_f64.round() as i64;

                // Look-ahead prevention check: positive offsets reference the future!
                if offset_int > 0 {
                    return Err(FormulaError::LookaheadForbidden);
                }

                let lag = (-offset_int) as usize;
                let mut out = vec![None; n];
                if lag < n {
                    out[lag..n].copy_from_slice(&series[..(n - lag)]);
                }
                Ok(out)
            }
            "MOV" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Mov".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let series = self.eval(&args[0])?;
                let period_series = self.eval(&args[1])?;
                let period = period_series.last().and_then(|x| *x).unwrap_or(20.0).max(1.0) as usize;

                let mode = if args.len() == 3 {
                    let m_series = self.eval(&args[2])?;
                    // Mode represented numerically or default Simple
                    m_series.last().and_then(|x| *x).unwrap_or(0.0) as i64
                } else {
                    0 // default Simple
                };

                // Filter out nones for standard calculation
                let values: Vec<f64> = series.iter().map(|v| v.unwrap_or(0.0)).collect();
                let res = match mode {
                    1 => ema(&values, period),
                    2 => wma(&values, period),
                    _ => sma(&values, period),
                };
                Ok(res)
            }
            "RSI" => {
                if args.len() != 2 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Rsi".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let series = self.eval(&args[0])?;
                let period_series = self.eval(&args[1])?;
                let period = period_series.last().and_then(|x| *x).unwrap_or(14.0).max(1.0) as usize;

                let values: Vec<f64> = series.iter().map(|v| v.unwrap_or(0.0)).collect();
                Ok(rsi(&values, period))
            }
            "CROSS" => {
                if args.len() != 2 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Cross".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let mut out = vec![Some(0.0); n];

                for i in 1..n {
                    if let (Some(a_curr), Some(b_curr), Some(a_prev), Some(b_prev)) =
                        (a[i], b[i], a[i - 1], b[i - 1])
                    {
                        if a_prev <= b_prev && a_curr > b_curr {
                            out[i] = Some(1.0);
                        }
                    }
                }
                Ok(out)
            }
            "HHV" => {
                if args.len() != 2 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Hhv".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let series = self.eval(&args[0])?;
                let p_series = self.eval(&args[1])?;
                let period = p_series.last().and_then(|x| *x).unwrap_or(20.0).max(1.0) as usize;
                let mut out = vec![None; n];

                for i in 0..n {
                    if i + 1 >= period {
                        let slice = &series[(i + 1 - period)..=i];
                        let max_v = slice.iter().filter_map(|x| *x).fold(f64::NEG_INFINITY, f64::max);
                        if max_v.is_finite() {
                            out[i] = Some(max_v);
                        }
                    }
                }
                Ok(out)
            }
            "LLV" => {
                if args.len() != 2 {
                    return Err(FormulaError::InvalidArgumentCount {
                        name: "Llv".into(),
                        expected: 2,
                        got: args.len(),
                    });
                }
                let series = self.eval(&args[0])?;
                let p_series = self.eval(&args[1])?;
                let period = p_series.last().and_then(|x| *x).unwrap_or(20.0).max(1.0) as usize;
                let mut out = vec![None; n];

                for i in 0..n {
                    if i + 1 >= period {
                        let slice = &series[(i + 1 - period)..=i];
                        let min_v = slice.iter().filter_map(|x| *x).fold(f64::INFINITY, f64::min);
                        if min_v.is_finite() {
                            out[i] = Some(min_v);
                        }
                    }
                }
                Ok(out)
            }
            _ => Err(FormulaError::UnknownFunction(name.to_string())),
        }
    }
}

/// Evaluates a formula expression string against bar data with sandbox step guard.
pub fn evaluate_formula(src: &str, bars: &[Bar], max_steps: usize) -> Result<Vec<Option<f64>>, FormulaError> {
    let expr = parse_formula(src)?;
    let mut eval = Evaluator::new(bars, max_steps);
    eval.eval(&expr)
}

/// One idea scan trigger signal.
#[derive(Debug, Clone, PartialEq)]
pub struct IdeaSignal {
    pub symbol: String,
    pub ts_ns: i64,
    pub rule_name: String,
    pub trigger_value: f64,
}

/// Idea scanner running formula rules across a universe of symbols.
pub fn run_idea_scanner(
    rule_name: &str,
    formula_src: &str,
    universe: &[(&str, &[Bar])],
    max_steps: usize,
) -> Result<Vec<IdeaSignal>, FormulaError> {
    let expr = parse_formula(formula_src)?;
    let mut signals = Vec::new();

    for (symbol, bars) in universe {
        if bars.is_empty() {
            continue;
        }
        let mut eval = Evaluator::new(bars, max_steps);
        let results = eval.eval(&expr)?;

        if let Some(Some(val)) = results.last() {
            if *val > 0.0 {
                let last_bar = bars.last().unwrap();
                signals.push(IdeaSignal {
                    symbol: (*symbol).to_string(),
                    ts_ns: last_bar.ts_ns,
                    rule_name: rule_name.to_string(),
                    trigger_value: *val,
                });
            }
        }
    }

    Ok(signals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::{Price, Qty};

    fn make_test_bar(ts: i64, c: f64) -> Bar {
        Bar::new(
            ts,
            0,
            Price::from_f64(c).unwrap(),
            Price::from_f64(c + 1.0).unwrap(),
            Price::from_f64(c - 1.0).unwrap(),
            Price::from_f64(c).unwrap(),
            Qty::from_f64(100.0).unwrap(),
        )
    }

    #[test]
    fn test_parse_and_eval_simple_formula() {
        let bars = vec![make_test_bar(1000, 10.0), make_test_bar(2000, 20.0), make_test_bar(3000, 30.0)];

        let res = evaluate_formula("Close * 2 + 5", &bars, 10_000).unwrap();
        assert_eq!(res, vec![Some(25.0), Some(45.0), Some(65.0)]);
    }

    #[test]
    fn test_built_in_parity_sma() {
        let bars: Vec<Bar> = (1..=30).map(|i| make_test_bar(i * 1000, i as f64)).collect();
        let formula_res = evaluate_formula("Mov(Close, 5, 0)", &bars, 10_000).unwrap();

        let closes: Vec<f64> = bars.iter().map(|b| b.close.to_f64()).collect();
        let builtin_res = sma(&closes, 5);

        assert_eq!(formula_res, builtin_res);
    }

    #[test]
    fn test_ref_lookahead_prevention() {
        let bars = vec![make_test_bar(1000, 10.0), make_test_bar(2000, 20.0)];

        // Negative offset (lagging) is valid
        let lag_res = evaluate_formula("Ref(Close, -1)", &bars, 10_000).unwrap();
        assert_eq!(lag_res[0], None);
        assert_eq!(lag_res[1], Some(10.0));

        // Positive offset (future peeking) is strictly forbidden!
        let future_res = evaluate_formula("Ref(Close, 1)", &bars, 10_000);
        assert_eq!(future_res, Err(FormulaError::LookaheadForbidden));
    }

    #[test]
    fn test_sandbox_execution_limit() {
        let bars = vec![make_test_bar(1000, 10.0)];
        // Extremely low limit stops execution
        let res = evaluate_formula("Close + Close + Close", &bars, 1);
        assert_eq!(res, Err(FormulaError::ExecutionLimitExceeded));
    }

    #[test]
    fn test_idea_scanner_universe() {
        let bars_a = vec![make_test_bar(1000, 10.0), make_test_bar(2000, 20.0)]; // Close > 15 -> true
        let bars_b = vec![make_test_bar(1000, 5.0), make_test_bar(2000, 10.0)]; // Close > 15 -> false

        let universe = [("AAA", bars_a.as_slice()), ("BBB", bars_b.as_slice())];

        let signals = run_idea_scanner("Breakout", "Close > 15", &universe, 10_000).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].symbol, "AAA");
        assert_eq!(signals[0].rule_name, "Breakout");
    }
}
