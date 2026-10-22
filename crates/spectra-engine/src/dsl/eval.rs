//! Deterministic runtime evaluator and `Strategy` implementation for the Spectra DSL.

use std::collections::HashMap;

use spectra_core::{Bar, InstrumentId, Price, Qty, Result, Side};

use super::ast::*;
use crate::backtest::{Context, OrderId, Strategy};

/// Value representation during DSL execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Exact fixed-point price represented by its raw i64 integer (scaled by 1e8).
    PriceRaw(i64),
    /// Exact fixed-point quantity represented by its raw i64 integer (scaled by 1e8).
    QtyRaw(i64),
}

impl Val {
    pub fn as_bool(self) -> bool {
        match self {
            Self::Bool(b) => b,
            Self::Int(i) => i != 0,
            Self::Float(f) => f != 0.0 && !f.is_nan(),
            Self::PriceRaw(p) => p != 0,
            Self::QtyRaw(q) => q != 0,
        }
    }

    pub fn to_f64(self) -> f64 {
        match self {
            Self::Int(i) => i as f64,
            Self::Float(f) => f,
            Self::Bool(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            Self::PriceRaw(p) => p as f64 / 1e8,
            Self::QtyRaw(q) => q as f64 / 1e8,
        }
    }

    pub fn as_qty(self) -> Qty {
        match self {
            Self::QtyRaw(raw) => Qty::from_raw(raw),
            Self::Int(i) => Qty::from_f64(i as f64).unwrap_or(Qty::ZERO),
            Self::Float(f) => Qty::from_f64(f).unwrap_or(Qty::ZERO),
            Self::PriceRaw(p) => Qty::from_raw(p),
            Self::Bool(b) => {
                if b {
                    Qty::ONE
                } else {
                    Qty::ZERO
                }
            }
        }
    }

    pub fn as_price(self) -> Price {
        match self {
            Self::PriceRaw(raw) => Price::from_raw(raw),
            Self::Int(i) => Price::from_f64(i as f64).unwrap_or(Price::ZERO),
            Self::Float(f) => Price::from_f64(f).unwrap_or(Price::ZERO),
            Self::QtyRaw(q) => Price::from_raw(q),
            Self::Bool(b) => {
                if b {
                    Price::from_raw(100_000_000)
                } else {
                    Price::ZERO
                }
            }
        }
    }

    pub fn add(self, other: Self) -> Self {
        match (self, other) {
            (Self::PriceRaw(a), Self::PriceRaw(b)) => Self::PriceRaw(a.saturating_add(b)),
            (Self::QtyRaw(a), Self::QtyRaw(b)) => Self::QtyRaw(a.saturating_add(b)),
            (Self::QtyRaw(a), Self::Float(b)) | (Self::Float(b), Self::QtyRaw(a)) => {
                let b_raw = (b * 1e8).round() as i64;
                Self::QtyRaw(a.saturating_add(b_raw))
            }
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_add(b)),
            (a, b) => Self::Float(a.to_f64() + b.to_f64()),
        }
    }

    pub fn sub(self, other: Self) -> Self {
        match (self, other) {
            (Self::PriceRaw(a), Self::PriceRaw(b)) => Self::PriceRaw(a.saturating_sub(b)),
            (Self::QtyRaw(a), Self::QtyRaw(b)) => Self::QtyRaw(a.saturating_sub(b)),
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_sub(b)),
            (a, b) => Self::Float(a.to_f64() - b.to_f64()),
        }
    }

    pub fn mul(self, other: Self) -> Self {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_mul(b)),
            (a, b) => Self::Float(a.to_f64() * b.to_f64()),
        }
    }

    pub fn div(self, other: Self) -> Self {
        let b = other.to_f64();
        if b == 0.0 {
            Self::Float(0.0)
        } else {
            Self::Float(self.to_f64() / b)
        }
    }

    pub fn rem(self, other: Self) -> Self {
        let b = other.to_f64();
        if b == 0.0 {
            Self::Float(0.0)
        } else {
            Self::Float(self.to_f64() % b)
        }
    }

    pub fn cmp_eq(self, other: Self) -> bool {
        match (self, other) {
            (Self::PriceRaw(a), Self::PriceRaw(b)) => a == b,
            (Self::QtyRaw(a), Self::QtyRaw(b)) => a == b,
            (Self::QtyRaw(a), Self::Int(b)) => a == b.saturating_mul(100_000_000),
            (Self::Int(a), Self::QtyRaw(b)) => a.saturating_mul(100_000_000) == b,
            (Self::PriceRaw(a), Self::Int(b)) => a == b.saturating_mul(100_000_000),
            (Self::Int(a), Self::PriceRaw(b)) => a.saturating_mul(100_000_000) == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (a, b) => (a.to_f64() - b.to_f64()).abs() < 1e-9,
        }
    }

    pub fn cmp_lt(self, other: Self) -> bool {
        match (self, other) {
            (Self::PriceRaw(a), Self::PriceRaw(b)) => a < b,
            (Self::QtyRaw(a), Self::QtyRaw(b)) => a < b,
            (Self::QtyRaw(a), Self::Int(b)) => a < b.saturating_mul(100_000_000),
            (Self::Int(a), Self::QtyRaw(b)) => a.saturating_mul(100_000_000) < b,
            (Self::PriceRaw(a), Self::Int(b)) => a < b.saturating_mul(100_000_000),
            (Self::Int(a), Self::PriceRaw(b)) => a.saturating_mul(100_000_000) < b,
            (Self::Int(a), Self::Int(b)) => a < b,
            (a, b) => a.to_f64() < b.to_f64(),
        }
    }

    pub fn cmp_gt(self, other: Self) -> bool {
        match (self, other) {
            (Self::PriceRaw(a), Self::PriceRaw(b)) => a > b,
            (Self::QtyRaw(a), Self::QtyRaw(b)) => a > b,
            (Self::QtyRaw(a), Self::Int(b)) => a > b.saturating_mul(100_000_000),
            (Self::Int(a), Self::QtyRaw(b)) => a.saturating_mul(100_000_000) > b,
            (Self::PriceRaw(a), Self::Int(b)) => a > b.saturating_mul(100_000_000),
            (Self::Int(a), Self::PriceRaw(b)) => a.saturating_mul(100_000_000) > b,
            (Self::Int(a), Self::Int(b)) => a > b,
            (a, b) => a.to_f64() > b.to_f64(),
        }
    }
}

/// An interpreted strategy executing AST nodes safely and deterministically.
#[derive(Debug)]
pub struct DslStrategy {
    program: Program,
    params: HashMap<String, Val>,
}

impl DslStrategy {
    pub fn new(program: Program, param_overrides: &HashMap<String, f64>) -> Self {
        let mut params = HashMap::new();
        for p in &program.params {
            let val =
                if let Some(&override_val) = param_overrides.get(&p.name) { override_val } else { p.default };

            let typed_val = match p.kind {
                ParamType::Int => Val::Int(val.round() as i64),
                ParamType::Float => {
                    if p.name == "qty" {
                        Val::QtyRaw((val * 1e8).round() as i64)
                    } else {
                        Val::Float(val)
                    }
                }
                ParamType::Bool => Val::Bool(val != 0.0),
            };
            params.insert(p.name.clone(), typed_val);
        }

        Self { program, params }
    }
}

impl Strategy for DslStrategy {
    fn on_start(&mut self, ctx: &mut Context<'_>) {
        if let Some(on_start) = &self.program.on_start {
            let mut env = ExecEnv::new(&self.params, ctx, 1, None);
            let _ = env.exec_block(on_start);
        }
    }

    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            let mut env = ExecEnv::new(&self.params, ctx, b.instrument, Some(b));
            let _ = env.exec_block(&self.program.on_bar);
        }
    }
}

struct ExecEnv<'a, 'c> {
    params: &'a HashMap<String, Val>,
    ctx: &'a mut Context<'c>,
    instrument: InstrumentId,
    current_bar: Option<&'a Bar>,
    locals: Vec<HashMap<String, Val>>,
    returned: bool,
}

impl<'a, 'c> ExecEnv<'a, 'c> {
    fn new(
        params: &'a HashMap<String, Val>,
        ctx: &'a mut Context<'c>,
        instrument: InstrumentId,
        current_bar: Option<&'a Bar>,
    ) -> Self {
        Self { params, ctx, instrument, current_bar, locals: vec![HashMap::new()], returned: false }
    }

    fn enter_scope(&mut self) {
        self.locals.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.locals.pop();
    }

    fn set_local(&mut self, name: String, val: Val) {
        if let Some(scope) = self.locals.last_mut() {
            scope.insert(name, val);
        }
    }

    fn assign(&mut self, name: &str, val: Val) {
        for scope in self.locals.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return;
            }
        }
        self.set_local(name.to_string(), val);
    }

    fn get_var(&self, name: &str) -> Val {
        for scope in self.locals.iter().rev() {
            if let Some(&val) = scope.get(name) {
                return val;
            }
        }
        if let Some(&val) = self.params.get(name) {
            return val;
        }

        // System variables
        match name {
            "close" => {
                if let Some(b) = self.current_bar {
                    Val::PriceRaw(b.close.raw())
                } else {
                    Val::PriceRaw(0)
                }
            }
            "open" => {
                if let Some(b) = self.current_bar {
                    Val::PriceRaw(b.open.raw())
                } else {
                    Val::PriceRaw(0)
                }
            }
            "high" => {
                if let Some(b) = self.current_bar {
                    Val::PriceRaw(b.high.raw())
                } else {
                    Val::PriceRaw(0)
                }
            }
            "low" => {
                if let Some(b) = self.current_bar {
                    Val::PriceRaw(b.low.raw())
                } else {
                    Val::PriceRaw(0)
                }
            }
            "volume" => {
                if let Some(b) = self.current_bar {
                    Val::QtyRaw(b.volume.raw())
                } else {
                    Val::QtyRaw(0)
                }
            }
            "position" => {
                let pos = self.ctx.position(self.instrument);
                Val::QtyRaw(pos.raw())
            }
            "cash" => Val::PriceRaw(self.ctx.cash().raw()),
            "equity" => Val::PriceRaw(self.ctx.equity().raw()),
            "bars_count" => {
                let h = self.ctx.history(self.instrument);
                Val::Int(h.len() as i64)
            }
            "instrument" => Val::Int(i64::from(self.instrument)),
            "now" => Val::Int(self.ctx.now()),
            "seed" => Val::Int(self.ctx.seed() as i64),
            _ => Val::Float(0.0),
        }
    }

    fn exec_block(&mut self, block: &Block) -> Result<()> {
        self.enter_scope();
        for stmt in &block.stmts {
            self.exec_stmt(stmt)?;
            if self.returned {
                break;
            }
        }
        self.exit_scope();
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        match stmt {
            Stmt::Let { name, init, .. } => {
                let val = self.eval_expr(init)?;
                self.set_local(name.clone(), val);
            }
            Stmt::Assign { name, value, .. } => {
                let val = self.eval_expr(value)?;
                self.assign(name, val);
            }
            Stmt::If { cond, then_branch, else_branch, .. } => {
                let c = self.eval_expr(cond)?;
                if c.as_bool() {
                    self.exec_block(then_branch)?;
                } else if let Some(eb) = else_branch {
                    match eb {
                        ElseBranch::ElseIf(inner) => self.exec_stmt(inner)?,
                        ElseBranch::Else(b) => self.exec_block(b)?,
                    }
                }
            }
            Stmt::Return { .. } => {
                self.returned = true;
            }
            Stmt::Expr(e) => {
                self.eval_expr(e)?;
            }
        }
        Ok(())
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Val> {
        match expr {
            Expr::Literal(lit, _) => Ok(match lit {
                LiteralValue::Int(i) => Val::Int(*i),
                LiteralValue::Float(f) => Val::Float(*f),
                LiteralValue::Bool(b) => Val::Bool(*b),
                LiteralValue::String(_) => Val::Float(0.0),
            }),
            Expr::Ident(name, _) => Ok(self.get_var(name)),
            Expr::Index { target, index, .. } => {
                let idx_val = self.eval_expr(index)?;
                let lag = idx_val.to_f64().max(0.0).round() as usize;
                let h = self.ctx.history(self.instrument);
                if lag >= h.len() {
                    return Ok(Val::PriceRaw(0));
                }
                let bar = &h[h.len() - 1 - lag];
                let val = match target.as_str() {
                    "close" => Val::PriceRaw(bar.close.raw()),
                    "open" => Val::PriceRaw(bar.open.raw()),
                    "high" => Val::PriceRaw(bar.high.raw()),
                    "low" => Val::PriceRaw(bar.low.raw()),
                    "volume" => Val::QtyRaw(bar.volume.raw()),
                    _ => Val::PriceRaw(0),
                };
                Ok(val)
            }
            Expr::Unary { op, expr, .. } => {
                let val = self.eval_expr(expr)?;
                Ok(match op {
                    UnaryOp::Neg => match val {
                        Val::Int(i) => Val::Int(-i),
                        Val::PriceRaw(p) => Val::PriceRaw(-p),
                        Val::QtyRaw(q) => Val::QtyRaw(-q),
                        Val::Float(f) => Val::Float(-f),
                        Val::Bool(b) => Val::Int(if b { -1 } else { 0 }),
                    },
                    UnaryOp::Not => Val::Bool(!val.as_bool()),
                })
            }
            Expr::Binary { op, left, right, .. } => {
                let l = self.eval_expr(left)?;
                // Short-circuit logical operators
                match op {
                    BinaryOp::And => {
                        if !l.as_bool() {
                            return Ok(Val::Bool(false));
                        }
                        let r = self.eval_expr(right)?;
                        return Ok(Val::Bool(r.as_bool()));
                    }
                    BinaryOp::Or => {
                        if l.as_bool() {
                            return Ok(Val::Bool(true));
                        }
                        let r = self.eval_expr(right)?;
                        return Ok(Val::Bool(r.as_bool()));
                    }
                    _ => {}
                }

                let r = self.eval_expr(right)?;
                Ok(match op {
                    BinaryOp::Add => l.add(r),
                    BinaryOp::Sub => l.sub(r),
                    BinaryOp::Mul => l.mul(r),
                    BinaryOp::Div => l.div(r),
                    BinaryOp::Rem => l.rem(r),
                    BinaryOp::Eq => Val::Bool(l.cmp_eq(r)),
                    BinaryOp::NotEq => Val::Bool(!l.cmp_eq(r)),
                    BinaryOp::Lt => Val::Bool(l.cmp_lt(r)),
                    BinaryOp::LtEq => Val::Bool(l.cmp_lt(r) || l.cmp_eq(r)),
                    BinaryOp::Gt => Val::Bool(l.cmp_gt(r)),
                    BinaryOp::GtEq => Val::Bool(l.cmp_gt(r) || l.cmp_eq(r)),
                    BinaryOp::And | BinaryOp::Or => unreachable!(),
                })
            }
            Expr::IfExpr { cond, then_expr, else_expr, .. } => {
                let c = self.eval_expr(cond)?;
                if c.as_bool() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }
            Expr::Call { name, args, .. } => self.eval_call(name, args),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Val> {
        match name {
            "abs" => {
                let v = self.eval_expr(&args[0])?;
                Ok(match v {
                    Val::Int(i) => Val::Int(i.abs()),
                    Val::PriceRaw(p) => Val::PriceRaw(p.abs()),
                    Val::QtyRaw(q) => Val::QtyRaw(q.abs()),
                    Val::Float(f) => Val::Float(f.abs()),
                    Val::Bool(b) => Val::Int(i64::from(b)),
                })
            }
            "min" => {
                let a = self.eval_expr(&args[0])?;
                let b = self.eval_expr(&args[1])?;
                if a.cmp_lt(b) {
                    Ok(a)
                } else {
                    Ok(b)
                }
            }
            "max" => {
                let a = self.eval_expr(&args[0])?;
                let b = self.eval_expr(&args[1])?;
                if a.cmp_gt(b) {
                    Ok(a)
                } else {
                    Ok(b)
                }
            }
            "round" => {
                let v = self.eval_expr(&args[0])?;
                Ok(Val::Int(v.to_f64().round() as i64))
            }
            "sma" => {
                let period_val =
                    if args.len() == 1 { self.eval_expr(&args[0])? } else { self.eval_expr(&args[1])? };
                let period = period_val.to_f64().max(1.0).round() as usize;
                let h = self.ctx.history(self.instrument);
                if h.len() < period {
                    return Ok(Val::PriceRaw(0));
                }
                // Compute SMA in exact raw integers with truncation matching sma_raw exactly:
                // history[history.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128
                let window = &h[h.len() - period..];
                let sum: i128 = window.iter().map(|b| i128::from(b.close.raw())).sum();
                let avg = sum / period as i128;
                Ok(Val::PriceRaw(avg as i64))
            }
            "ema" => {
                let period_val =
                    if args.len() == 1 { self.eval_expr(&args[0])? } else { self.eval_expr(&args[1])? };
                let period = period_val.to_f64().max(1.0);
                let h = self.ctx.history(self.instrument);
                if h.is_empty() {
                    return Ok(Val::PriceRaw(0));
                }
                let k = 2.0 / (period + 1.0);
                let mut ema = h[0].close.raw() as f64;
                for b in &h[1..] {
                    ema = (b.close.raw() as f64 * k) + (ema * (1.0 - k));
                }
                Ok(Val::PriceRaw(ema.round() as i64))
            }
            "rsi" => {
                let period_val =
                    if args.len() == 1 { self.eval_expr(&args[0])? } else { self.eval_expr(&args[1])? };
                let period = period_val.to_f64().max(1.0).round() as usize;
                let h = self.ctx.history(self.instrument);
                if h.len() <= period {
                    return Ok(Val::Float(50.0));
                }
                let mut gains = 0.0;
                let mut losses = 0.0;
                let start = h.len() - period;
                for i in start..h.len() {
                    let diff = h[i].close.raw() - h[i - 1].close.raw();
                    if diff > 0 {
                        gains += diff as f64;
                    } else {
                        losses += (-diff) as f64;
                    }
                }
                if losses == 0.0 {
                    return Ok(Val::Float(100.0));
                }
                let rs = (gains / period as f64) / (losses / period as f64);
                let rsi = 100.0 - (100.0 / (1.0 + rs));
                Ok(Val::Float(rsi))
            }
            "target_shares" => {
                let target_val = self.eval_expr(&args[0])?;
                let target_qty = target_val.to_f64();
                let current_pos = self.ctx.position(self.instrument);
                let current_shares = current_pos.raw() as f64 / 100_000_000.0;
                let diff = target_qty - current_shares;
                if diff.abs() > 1e-6 {
                    let side = if diff > 0.0 { spectra_core::Side::Buy } else { spectra_core::Side::Sell };
                    if let Ok(qty) = spectra_core::Qty::from_f64(diff.abs()) {
                        self.ctx.market(self.instrument, side, qty);
                    }
                }
                Ok(Val::Int(0))
            }
            "market_buy" => {
                let qty_val = self.eval_expr(&args[0])?;
                let qty = qty_val.as_qty();
                if qty.raw() > 0 {
                    let id = self.ctx.market(self.instrument, Side::Buy, qty);
                    Ok(Val::Int(id as i64))
                } else {
                    Ok(Val::Int(0))
                }
            }
            "market_sell" => {
                let qty_val = self.eval_expr(&args[0])?;
                let qty = qty_val.as_qty();
                if qty.raw() > 0 {
                    let id = self.ctx.market(self.instrument, Side::Sell, qty);
                    Ok(Val::Int(id as i64))
                } else {
                    Ok(Val::Int(0))
                }
            }
            "limit_buy" => {
                let price_val = self.eval_expr(&args[0])?;
                let qty_val = self.eval_expr(&args[1])?;
                let price = price_val.as_price();
                let qty = qty_val.as_qty();
                let id = self.ctx.limit(self.instrument, Side::Buy, qty, price);
                Ok(Val::Int(id as i64))
            }
            "limit_sell" => {
                let price_val = self.eval_expr(&args[0])?;
                let qty_val = self.eval_expr(&args[1])?;
                let price = price_val.as_price();
                let qty = qty_val.as_qty();
                let id = self.ctx.limit(self.instrument, Side::Sell, qty, price);
                Ok(Val::Int(id as i64))
            }
            "stop_buy" => {
                let price_val = self.eval_expr(&args[0])?;
                let qty_val = self.eval_expr(&args[1])?;
                let price = price_val.as_price();
                let qty = qty_val.as_qty();
                let id = self.ctx.stop(self.instrument, Side::Buy, qty, price);
                Ok(Val::Int(id as i64))
            }
            "stop_sell" => {
                let price_val = self.eval_expr(&args[0])?;
                let qty_val = self.eval_expr(&args[1])?;
                let price = price_val.as_price();
                let qty = qty_val.as_qty();
                let id = self.ctx.stop(self.instrument, Side::Sell, qty, price);
                Ok(Val::Int(id as i64))
            }
            "cancel_order" => {
                let id_val = self.eval_expr(&args[0])?;
                let id = id_val.to_f64().round() as OrderId;
                let cancelled = self.ctx.cancel(id);
                Ok(Val::Bool(cancelled))
            }
            _ => Ok(Val::Float(0.0)),
        }
    }
}
