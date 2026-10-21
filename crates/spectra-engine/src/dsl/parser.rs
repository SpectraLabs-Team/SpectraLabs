//! Recursive-descent parser for the Spectra restricted strategy DSL.

use super::ast::*;
use super::lexer::{Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug)]
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            self.tokens.last().expect("tokens should not be empty")
        }
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            let actual = self.peek();
            if let TokenKind::ForbiddenKeyword(kw) = &actual.kind {
                return Err(ParseError {
                    message: format!("forbidden language construct '{kw}': loops and arbitrary execution are not permitted"),
                    span: actual.span,
                });
            }
            Err(ParseError {
                message: format!("expected {kind:?}, found {:?}", actual.kind),
                span: actual.span,
            })
        }
    }

    /// Parses the tokens into a [`Program`].
    ///
    /// # Errors
    /// Returns [`ParseError`] if syntax is malformed or required handlers are missing.
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut params = Vec::new();
        let mut on_start = None;
        let mut on_bar = None;

        while !matches!(self.peek_kind(), TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::Param => {
                    params.push(self.parse_param_decl()?);
                }
                TokenKind::OnStart => {
                    self.advance();
                    let block = self.parse_block()?;
                    on_start = Some(block);
                }
                TokenKind::OnBar => {
                    self.advance();
                    let block = self.parse_block()?;
                    on_bar = Some(block);
                }
                TokenKind::ForbiddenKeyword(kw) => {
                    let span = self.peek().span;
                    return Err(ParseError {
                        message: format!(
                            "forbidden language construct '{kw}': loops and arbitrary code are not permitted"
                        ),
                        span,
                    });
                }
                _ => {
                    let tok = self.peek();
                    return Err(ParseError {
                        message: format!("unexpected token at top level: {:?}", tok.kind),
                        span: tok.span,
                    });
                }
            }
        }

        let Some(on_bar) = on_bar else {
            return Err(ParseError {
                message: "strategy must declare an 'on_bar { ... }' handler".into(),
                span: Span::new(1, 1),
            });
        };

        Ok(Program { params, on_start, on_bar })
    }

    fn parse_param_decl(&mut self) -> Result<ParamDecl, ParseError> {
        let start_tok = self.expect(&TokenKind::Param)?;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(name) => {
                let name = name.clone();
                let span = self.peek().span;
                self.advance();
                (name, span)
            }
            _ => {
                return Err(ParseError { message: "expected parameter name".into(), span: self.peek().span });
            }
        };

        self.expect(&TokenKind::Colon)?;

        let kind = match self.peek_kind() {
            TokenKind::IntType => {
                self.advance();
                ParamType::Int
            }
            TokenKind::FloatType => {
                self.advance();
                ParamType::Float
            }
            TokenKind::BoolType => {
                self.advance();
                ParamType::Bool
            }
            _ => {
                return Err(ParseError {
                    message: "expected parameter type (int, float, bool)".into(),
                    span: self.peek().span,
                });
            }
        };

        self.expect(&TokenKind::Eq)?;

        let default_val = match self.peek_kind() {
            TokenKind::Int(i) => {
                let v = *i as f64;
                self.advance();
                v
            }
            TokenKind::Float(f) => {
                let v = *f;
                self.advance();
                v
            }
            TokenKind::True => {
                self.advance();
                1.0
            }
            TokenKind::False => {
                self.advance();
                0.0
            }
            TokenKind::Minus => {
                self.advance();
                match self.peek_kind() {
                    TokenKind::Int(i) => {
                        let v = -(*i as f64);
                        self.advance();
                        v
                    }
                    TokenKind::Float(f) => {
                        let v = -*f;
                        self.advance();
                        v
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected number after '-' in default value".into(),
                            span: self.peek().span,
                        })
                    }
                }
            }
            _ => {
                return Err(ParseError {
                    message: "expected literal default value for parameter".into(),
                    span: self.peek().span,
                });
            }
        };

        let mut min_val = match kind {
            ParamType::Int => 0.0,
            ParamType::Float => 0.0,
            ParamType::Bool => 0.0,
        };
        let mut max_val = match kind {
            ParamType::Int => 1_000_000.0,
            ParamType::Float => 1e12,
            ParamType::Bool => 1.0,
        };
        let mut desc = String::new();

        // Optional attributes: comma separated, or [min..max]
        if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.advance(); // '['
            min_val = self.parse_number_literal()?;
            // '..' or ','
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            max_val = self.parse_number_literal()?;
            self.expect(&TokenKind::RBracket)?;
        }

        while matches!(self.peek_kind(), TokenKind::Comma) {
            self.advance();
            match self.peek_kind() {
                TokenKind::Min => {
                    self.advance();
                    self.expect(&TokenKind::Eq)?;
                    min_val = self.parse_number_literal()?;
                }
                TokenKind::Max => {
                    self.advance();
                    self.expect(&TokenKind::Eq)?;
                    max_val = self.parse_number_literal()?;
                }
                TokenKind::Desc => {
                    self.advance();
                    self.expect(&TokenKind::Eq)?;
                    if let TokenKind::StringLit(s) = self.peek_kind() {
                        desc.clone_from(s);
                        self.advance();
                    } else {
                        return Err(ParseError {
                            message: "expected string description".into(),
                            span: self.peek().span,
                        });
                    }
                }
                _ => break,
            }
        }

        self.expect(&TokenKind::Semi)?;

        Ok(ParamDecl {
            name: name_tok.0,
            kind,
            default: default_val,
            min: min_val,
            max: max_val,
            description: desc,
            span: start_tok.span,
        })
    }

    fn parse_number_literal(&mut self) -> Result<f64, ParseError> {
        let neg = if matches!(self.peek_kind(), TokenKind::Minus) {
            self.advance();
            true
        } else {
            false
        };

        let val = match self.peek_kind() {
            TokenKind::Int(i) => {
                let v = *i as f64;
                self.advance();
                v
            }
            TokenKind::Float(f) => {
                let v = *f;
                self.advance();
                v
            }
            _ => {
                return Err(ParseError { message: "expected number literal".into(), span: self.peek().span });
            }
        };

        Ok(if neg { -val } else { val })
    }

    /// Parses a braced block of statements.
    ///
    /// # Errors
    /// Returns [`ParseError`] if the block is unclosed or statements are invalid.
    pub fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start = self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(Block { stmts, span: start.span })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        if let TokenKind::ForbiddenKeyword(kw) = self.peek_kind() {
            let span = self.peek().span;
            return Err(ParseError {
                message: format!("forbidden language construct '{kw}': loops and recursion are rejected"),
                span,
            });
        }

        match self.peek_kind() {
            TokenKind::Let => {
                let start = self.advance();
                let name = match self.peek_kind() {
                    TokenKind::Ident(id) => {
                        let id = id.clone();
                        self.advance();
                        id
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected variable name after 'let'".into(),
                            span: self.peek().span,
                        });
                    }
                };
                self.expect(&TokenKind::Eq)?;
                let init = self.parse_expr()?;
                self.expect(&TokenKind::Semi)?;
                Ok(Stmt::Let { name, init, span: start.span })
            }
            TokenKind::If => {
                let start = self.advance();
                let cond = self.parse_expr()?;
                let then_branch = self.parse_block()?;
                let else_branch = if matches!(self.peek_kind(), TokenKind::Else) {
                    self.advance();
                    if matches!(self.peek_kind(), TokenKind::If) {
                        let nested_if = self.parse_stmt()?;
                        Some(ElseBranch::ElseIf(Box::new(nested_if)))
                    } else {
                        let block = self.parse_block()?;
                        Some(ElseBranch::Else(block))
                    }
                } else {
                    None
                };
                Ok(Stmt::If { cond, then_branch, else_branch, span: start.span })
            }
            TokenKind::Return => {
                let tok = self.advance();
                self.expect(&TokenKind::Semi)?;
                Ok(Stmt::Return { span: tok.span })
            }
            TokenKind::Ident(name)
                if self.pos + 1 < self.tokens.len()
                    && matches!(self.tokens[self.pos + 1].kind, TokenKind::Eq) =>
            {
                let span = self.peek().span;
                let var_name = name.clone();
                self.advance(); // ident
                self.advance(); // '='
                let value = self.parse_expr()?;
                self.expect(&TokenKind::Semi)?;
                Ok(Stmt::Assign { name: var_name, value, span })
            }
            _ => {
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::Semi)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    /// Parses an expression.
    ///
    /// # Errors
    /// Returns [`ParseError`] if expression syntax is invalid.
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_if_expr()
    }

    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek_kind(), TokenKind::If) {
            let span = self.peek().span;
            self.advance(); // if
            let cond = self.parse_or()?;
            self.expect(&TokenKind::LBrace)?;
            let then_expr = self.parse_expr()?;
            self.expect(&TokenKind::RBrace)?;
            self.expect(&TokenKind::Else)?;
            self.expect(&TokenKind::LBrace)?;
            let else_expr = self.parse_expr()?;
            self.expect(&TokenKind::RBrace)?;
            Ok(Expr::IfExpr {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
                span,
            })
        } else {
            self.parse_or()
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek_kind(), TokenKind::Or) {
            let _op_tok = self.advance();
            let right = self.parse_and()?;
            let span = left.span();
            left = Expr::Binary { op: BinaryOp::Or, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek_kind(), TokenKind::And) {
            let _op_tok = self.advance();
            let right = self.parse_equality()?;
            let span = left.span();
            left = Expr::Binary { op: BinaryOp::And, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while matches!(self.peek_kind(), TokenKind::EqEq | TokenKind::NotEq) {
            let op_tok = self.advance();
            let op = match op_tok.kind {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::NotEq => BinaryOp::NotEq,
                _ => unreachable!(),
            };
            let right = self.parse_comparison()?;
            let span = left.span();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        while matches!(self.peek_kind(), TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq) {
            let op_tok = self.advance();
            let op = match op_tok.kind {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::LtEq => BinaryOp::LtEq,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::GtEq => BinaryOp::GtEq,
                _ => unreachable!(),
            };
            let right = self.parse_term()?;
            let span = left.span();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_factor()?;
        while matches!(self.peek_kind(), TokenKind::Plus | TokenKind::Minus) {
            let op_tok = self.advance();
            let op = match op_tok.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_factor()?;
            let span = left.span();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek_kind(), TokenKind::Star | TokenKind::Slash | TokenKind::Percent) {
            let op_tok = self.advance();
            let op = match op_tok.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => unreachable!(),
            };
            let right = self.parse_factor()?;
            let span = left.span();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek_kind(), TokenKind::Minus | TokenKind::Not) {
            let op_tok = self.advance();
            let op = match op_tok.kind {
                TokenKind::Minus => UnaryOp::Neg,
                TokenKind::Not => UnaryOp::Not,
                _ => unreachable!(),
            };
            let inner = self.parse_unary()?;
            Ok(Expr::Unary { op, expr: Box::new(inner), span: op_tok.span })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Int(i) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Int(*i), tok.span))
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Float(*f), tok.span))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Bool(true), tok.span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Bool(false), tok.span))
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::Literal(LiteralValue::String(s), tok.span))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Ident(_) | TokenKind::Min | TokenKind::Max => {
                let name = match &tok.kind {
                    TokenKind::Min => "min".to_string(),
                    TokenKind::Max => "max".to_string(),
                    TokenKind::Ident(n) => n.clone(),
                    _ => unreachable!(),
                };
                let span = tok.span;
                self.advance();

                // Function call: ident(...)
                if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek_kind(), TokenKind::Comma) {
                            self.advance();
                            if matches!(self.peek_kind(), TokenKind::RParen) {
                                break;
                            }
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Call { name, args, span })
                }
                // Series Indexing: ident[index]
                else if matches!(self.peek_kind(), TokenKind::LBracket) {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    Ok(Expr::Index { target: name, index: Box::new(index), span })
                } else {
                    Ok(Expr::Ident(name, span))
                }
            }
            TokenKind::ForbiddenKeyword(kw) => Err(ParseError {
                message: format!(
                    "forbidden language construct '{kw}': unbounded loops and recursion are rejected"
                ),
                span: tok.span,
            }),
            _ => Err(ParseError {
                message: format!("expected expression, found {:?}", tok.kind),
                span: tok.span,
            }),
        }
    }
}
