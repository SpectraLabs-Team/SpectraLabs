//! Static safety analyzer and type checker for the Spectra restricted strategy DSL.

use std::collections::{HashMap, HashSet};

use super::ast::*;

/// Structured line-level validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
    /// Human-readable explanation.
    pub message: String,
    /// Machine-readable error code.
    pub code: &'static str,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: [{}] {}", self.line, self.col, self.code, self.message)
    }
}

impl std::error::Error for ValidationError {}

const MAX_AST_DEPTH: usize = 128;

#[derive(Debug)]
pub struct Validator {
    params: HashMap<String, ParamDecl>,
    scopes: Vec<HashSet<String>>,
    depth: usize,
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

impl Validator {
    pub fn new() -> Self {
        Self { params: HashMap::new(), scopes: vec![HashSet::new()], depth: 0 }
    }

    /// Validates the program against safety rules and type constraints.
    ///
    /// # Errors
    /// Returns [`ValidationError`] if the program contains lookahead, invalid parameters, or undefined symbols.
    pub fn validate(mut self, program: &Program) -> Result<(), ValidationError> {
        // 1. Validate parameters
        for p in &program.params {
            if self.params.contains_key(&p.name) {
                return Err(ValidationError {
                    line: p.span.line,
                    col: p.span.col,
                    message: format!("duplicate parameter name '{}'", p.name),
                    code: "duplicate_parameter",
                });
            }
            if p.min > p.max {
                return Err(ValidationError {
                    line: p.span.line,
                    col: p.span.col,
                    message: format!(
                        "parameter '{}' has invalid bounds: min ({}) cannot exceed max ({})",
                        p.name, p.min, p.max
                    ),
                    code: "invalid_param_bounds",
                });
            }
            if p.default < p.min || p.default > p.max {
                return Err(ValidationError {
                    line: p.span.line,
                    col: p.span.col,
                    message: format!(
                        "parameter '{}' default value {} is outside bounds [{}..{}]",
                        p.name, p.default, p.min, p.max
                    ),
                    code: "param_default_out_of_bounds",
                });
            }
            self.params.insert(p.name.clone(), p.clone());
        }

        // 2. Validate on_start if present
        if let Some(on_start) = &program.on_start {
            self.enter_scope();
            self.validate_block(on_start)?;
            self.exit_scope();
        }

        // 3. Validate on_bar
        self.enter_scope();
        self.validate_block(&program.on_bar)?;
        self.exit_scope();

        Ok(())
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_var(&mut self, name: &str, span: Span) -> Result<(), ValidationError> {
        if self.is_system_symbol(name) {
            return Err(ValidationError {
                line: span.line,
                col: span.col,
                message: format!("cannot reassign system keyword or variable '{name}'"),
                code: "reserved_symbol",
            });
        }
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains(name) {
                return Err(ValidationError {
                    line: span.line,
                    col: span.col,
                    message: format!("variable '{name}' is already declared in this scope"),
                    code: "duplicate_variable",
                });
            }
            scope.insert(name.to_string());
        }
        Ok(())
    }

    fn is_declared(&self, name: &str) -> bool {
        if self.params.contains_key(name) {
            return true;
        }
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        self.is_system_symbol(name)
    }

    fn is_system_symbol(&self, name: &str) -> bool {
        matches!(
            name,
            "close"
                | "open"
                | "high"
                | "low"
                | "volume"
                | "position"
                | "cash"
                | "equity"
                | "bars_count"
                | "instrument"
                | "now"
                | "seed"
                | "sma"
                | "ema"
                | "rsi"
                | "abs"
                | "min"
                | "max"
                | "round"
                | "market_buy"
                | "market_sell"
                | "limit_buy"
                | "limit_sell"
                | "stop_buy"
                | "stop_sell"
                | "cancel_order"
        )
    }

    fn is_series(&self, name: &str) -> bool {
        matches!(name, "close" | "open" | "high" | "low" | "volume")
    }

    fn validate_block(&mut self, block: &Block) -> Result<(), ValidationError> {
        self.enter_scope();
        for stmt in &block.stmts {
            self.validate_stmt(stmt)?;
        }
        self.exit_scope();
        Ok(())
    }

    fn validate_stmt(&mut self, stmt: &Stmt) -> Result<(), ValidationError> {
        match stmt {
            Stmt::Let { name, init, span } => {
                self.validate_expr(init)?;
                self.declare_var(name, *span)?;
            }
            Stmt::Assign { name, value, span } => {
                if !self.is_declared(name) {
                    return Err(ValidationError {
                        line: span.line,
                        col: span.col,
                        message: format!("cannot assign to undeclared variable '{name}'"),
                        code: "undeclared_variable",
                    });
                }
                if self.params.contains_key(name) {
                    return Err(ValidationError {
                        line: span.line,
                        col: span.col,
                        message: format!("strategy parameters like '{name}' are immutable; declare a local variable instead"),
                        code: "immutable_parameter",
                    });
                }
                if self.is_system_symbol(name) {
                    return Err(ValidationError {
                        line: span.line,
                        col: span.col,
                        message: format!("cannot reassign system variable '{name}'"),
                        code: "reserved_symbol",
                    });
                }
                self.validate_expr(value)?;
            }
            Stmt::If { cond, then_branch, else_branch, .. } => {
                self.validate_expr(cond)?;
                self.validate_block(then_branch)?;
                if let Some(eb) = else_branch {
                    match eb {
                        ElseBranch::ElseIf(inner_if) => self.validate_stmt(inner_if)?,
                        ElseBranch::Else(b) => self.validate_block(b)?,
                    }
                }
            }
            Stmt::Return { .. } => {}
            Stmt::Expr(e) => {
                self.validate_expr(e)?;
            }
        }
        Ok(())
    }

    fn validate_expr(&mut self, expr: &Expr) -> Result<(), ValidationError> {
        self.depth += 1;
        if self.depth > MAX_AST_DEPTH {
            return Err(ValidationError {
                line: expr.span().line,
                col: expr.span().col,
                message: "expression exceeds maximum nesting depth".into(),
                code: "max_depth_exceeded",
            });
        }

        let res = match expr {
            Expr::Literal(..) => Ok(()),
            Expr::Ident(name, span) => {
                if !self.is_declared(name) {
                    Err(ValidationError {
                        line: span.line,
                        col: span.col,
                        message: format!("undefined variable or function '{name}'"),
                        code: "undefined_identifier",
                    })
                } else {
                    Ok(())
                }
            }
            Expr::Index { target, index, span } => {
                if !self.is_series(target) {
                    return Err(ValidationError {
                        line: span.line,
                        col: span.col,
                        message: format!("cannot index into '{target}': only price series (close, open, high, low, volume) support historical indexing"),
                        code: "invalid_index_target",
                    });
                }

                // Lookahead protection check:
                // An index represents the number of bars ago (0 is current bar, 1 is 1 bar ago).
                // Negative index is lookahead into future bars!
                if let Expr::Literal(LiteralValue::Int(val), index_span) = &**index {
                    if *val < 0 {
                        return Err(ValidationError {
                            line: index_span.line,
                            col: index_span.col,
                            message: format!(
                                "lookahead forbidden: index must be a non-negative lag (found {val})"
                            ),
                            code: "lookahead_forbidden",
                        });
                    }
                } else if let Expr::Unary { op: UnaryOp::Neg, span: neg_span, .. } = &**index {
                    return Err(ValidationError {
                        line: neg_span.line,
                        col: neg_span.col,
                        message: "lookahead forbidden: negative index accesses future bars".into(),
                        code: "lookahead_forbidden",
                    });
                }

                self.validate_expr(index)
            }
            Expr::Unary { expr, .. } => self.validate_expr(expr),
            Expr::Binary { left, right, .. } => {
                self.validate_expr(left)?;
                self.validate_expr(right)
            }
            Expr::Call { name, args, span } => {
                self.validate_call(name, args, *span)?;
                for a in args {
                    self.validate_expr(a)?;
                }
                Ok(())
            }
            Expr::IfExpr { cond, then_expr, else_expr, .. } => {
                self.validate_expr(cond)?;
                self.validate_expr(then_expr)?;
                self.validate_expr(else_expr)
            }
        };

        self.depth -= 1;
        res
    }

    fn validate_call(&self, name: &str, args: &[Expr], span: Span) -> Result<(), ValidationError> {
        let expected_args = match name {
            "sma" | "ema" | "rsi" | "min" | "max" => 2,
            "abs" | "round" | "market_buy" | "market_sell" | "cancel_order" => 1,
            "limit_buy" | "limit_sell" | "stop_buy" | "stop_sell" => 2,
            _ => {
                return Err(ValidationError {
                    line: span.line,
                    col: span.col,
                    message: format!("unknown function '{name}'"),
                    code: "unknown_function",
                });
            }
        };

        if args.len() != expected_args {
            return Err(ValidationError {
                line: span.line,
                col: span.col,
                message: format!(
                    "function '{name}' expects {expected_args} argument(s), found {}",
                    args.len()
                ),
                code: "argument_count_mismatch",
            });
        }

        // Validate indicator argument types
        if matches!(name, "sma" | "ema" | "rsi") {
            if let Some(Expr::Ident(series_name, s_span)) = args.first() {
                if !self.is_series(series_name) {
                    return Err(ValidationError {
                        line: s_span.line,
                        col: s_span.col,
                        message: format!(
                            "first argument to '{name}' must be a series (close, open, high, low, volume), found '{series_name}'"
                        ),
                        code: "invalid_series_argument",
                    });
                }
            }
        }

        Ok(())
    }
}
