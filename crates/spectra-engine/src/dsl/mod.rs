//! Spectra Labs restricted strategy language (DSL) and interpreter.
//!
//! A deterministic, bounded rule language for user-authored quantitative strategies.
//! The language enforces static safety:
//! - No unbounded loops (while, for, loop are forbidden)
//! - No recursion or arbitrary code execution
//! - No lookahead into future bars (indexing into future bars is statically rejected)
//! - Structured line-level diagnostics for editor integration
//! - Bit-for-bit parity with built-in reference strategies

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::wildcard_imports,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::should_implement_trait,
    clippy::unused_self,
    clippy::if_not_else,
    clippy::match_same_arms
)]

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod validator;

pub use ast::{ParamDecl, ParamType, Program, Span};
pub use eval::{DslStrategy, Val};
pub use lexer::{LexError, Lexer, Token, TokenKind};
pub use parser::{ParseError, Parser};
pub use validator::{ValidationError, Validator};

/// Parse and statically validate a strategy DSL script.
///
/// # Errors
/// Returns [`ValidationError`] with line and column numbers if syntax is invalid,
/// if safety rules are violated (e.g. lookahead, loops), or if undeclared symbols are referenced.
pub fn parse_and_validate(source: &str) -> Result<Program, ValidationError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| ValidationError {
        line: e.span.line,
        col: e.span.col,
        message: e.message,
        code: "syntax_error",
    })?;

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().map_err(|e| ValidationError {
        line: e.span.line,
        col: e.span.col,
        message: e.message,
        code: "parse_error",
    })?;

    let validator = Validator::new();
    validator.validate(&program)?;

    Ok(program)
}
