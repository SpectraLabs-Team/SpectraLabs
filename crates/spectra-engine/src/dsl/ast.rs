//! Abstract Syntax Tree (AST) for the Spectra restricted strategy DSL.

/// Source code location for line-level errors and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
}

impl Span {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// The type of a declared parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Int,
    Float,
    Bool,
}

impl ParamType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
        }
    }
}

/// Parameter declaration in the script header.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub kind: ParamType,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub description: String,
    pub span: Span,
}

/// A block of statements inside `{ ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// One statement in the strategy body.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, init: Expr, span: Span },
    Assign { name: String, value: Expr, span: Span },
    If { cond: Expr, then_branch: Block, else_branch: Option<ElseBranch>, span: Span },
    Return { span: Span },
    Expr(Expr),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. }
            | Self::Assign { span, .. }
            | Self::If { span, .. }
            | Self::Return { span } => *span,
            Self::Expr(e) => e.span(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    ElseIf(Box<Stmt>),
    Else(Block),
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(LiteralValue, Span),
    Ident(String, Span),
    /// Historical series indexing, e.g. `close[0]`, `close[1]`.
    Index {
        target: String,
        index: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
    IfExpr {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Literal(_, s)
            | Self::Ident(_, s)
            | Self::Index { span: s, .. }
            | Self::Unary { span: s, .. }
            | Self::Binary { span: s, .. }
            | Self::Call { span: s, .. }
            | Self::IfExpr { span: s, .. } => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

/// A complete parsed strategy script.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub params: Vec<ParamDecl>,
    pub on_start: Option<Block>,
    pub on_bar: Block,
}
