//! Lexer for the Spectra restricted strategy DSL.

use super::ast::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Param,
    IntType,
    FloatType,
    BoolType,
    Min,
    Max,
    Desc,
    OnStart,
    OnBar,
    Let,
    If,
    Else,
    Return,
    True,
    False,
    And,
    Or,
    Not,

    // Disallowed keywords that trigger specific safety errors
    ForbiddenKeyword(String),

    // Identifiers & literals
    Ident(String),
    Int(i64),
    Float(f64),
    StringLit(String),

    // Symbols & Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Semi,
    Comma,
    Colon,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for LexError {}

#[derive(Debug)]
pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self { source, chars, pos: 0, line: 1, col: 1 }
    }

    fn current_span(&self) -> Span {
        Span::new(self.line, self.col)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).map(|&(_, c)| c)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).map(|&(_, c)| c)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(&(_, c)) = self.chars.get(self.pos) {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    /// Tokenizes the input source text.
    ///
    /// # Errors
    /// Returns [`LexError`] if unclosed strings, unclosed comments, or unexpected characters are encountered.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
                continue;
            }

            // Single line comment
            if c == '/' && self.peek_next() == Some('/') {
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }

            // Multi line comment
            if c == '/' && self.peek_next() == Some('*') {
                let start_span = self.current_span();
                self.advance();
                self.advance();
                let mut closed = false;
                while let Some(ch) = self.peek() {
                    if ch == '*' && self.peek_next() == Some('/') {
                        self.advance();
                        self.advance();
                        closed = true;
                        break;
                    }
                    self.advance();
                }
                if !closed {
                    return Err(LexError { message: "unclosed comment".into(), span: start_span });
                }
                continue;
            }

            let span = self.current_span();

            // Two-character operators
            if c == '=' && self.peek_next() == Some('=') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::EqEq, span });
                continue;
            }
            if c == '!' && self.peek_next() == Some('=') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::NotEq, span });
                continue;
            }
            if c == '<' && self.peek_next() == Some('=') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::LtEq, span });
                continue;
            }
            if c == '>' && self.peek_next() == Some('=') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::GtEq, span });
                continue;
            }
            if c == '&' && self.peek_next() == Some('&') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::And, span });
                continue;
            }
            if c == '|' && self.peek_next() == Some('|') {
                self.advance();
                self.advance();
                tokens.push(Token { kind: TokenKind::Or, span });
                continue;
            }

            // Single-character symbols
            match c {
                '+' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Plus, span });
                }
                '-' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Minus, span });
                }
                '*' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Star, span });
                }
                '/' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Slash, span });
                }
                '%' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Percent, span });
                }
                '=' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Eq, span });
                }
                '<' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Lt, span });
                }
                '>' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Gt, span });
                }
                '!' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Not, span });
                }
                ';' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Semi, span });
                }
                ',' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Comma, span });
                }
                ':' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::Colon, span });
                }
                '(' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::LParen, span });
                }
                ')' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::RParen, span });
                }
                '{' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::LBrace, span });
                }
                '}' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::RBrace, span });
                }
                '[' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::LBracket, span });
                }
                ']' => {
                    self.advance();
                    tokens.push(Token { kind: TokenKind::RBracket, span });
                }
                '"' => {
                    let s = self.lex_string(span)?;
                    tokens.push(s);
                }
                _ if c.is_ascii_digit() => {
                    let num = self.lex_number(span)?;
                    tokens.push(num);
                }
                _ if c.is_ascii_alphabetic() || c == '_' => {
                    let ident = self.lex_ident(span);
                    tokens.push(ident);
                }
                _ => {
                    return Err(LexError { message: format!("unexpected character: '{c}'"), span });
                }
            }
        }

        tokens.push(Token { kind: TokenKind::Eof, span: self.current_span() });
        Ok(tokens)
    }

    fn lex_string(&mut self, span: Span) -> Result<Token, LexError> {
        self.advance(); // quote
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.advance();
                return Ok(Token { kind: TokenKind::StringLit(s), span });
            }
            if c == '\\' {
                self.advance();
                match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(other) => s.push(other),
                    None => {
                        return Err(LexError { message: "unexpected EOF in escape sequence".into(), span })
                    }
                }
            } else if c == '\n' {
                return Err(LexError { message: "unexpected newline in string literal".into(), span });
            } else {
                s.push(c);
                self.advance();
            }
        }
        Err(LexError { message: "unclosed string literal".into(), span })
    }

    fn lex_number(&mut self, span: Span) -> Result<Token, LexError> {
        let start_pos = self.chars[self.pos].0;
        let mut has_dot = false;
        let mut has_exp = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else if c == '.' && !has_dot && !has_exp {
                // Lookahead: only treat as dot if next char is a digit
                if self.peek_next().is_some_and(|n| n.is_ascii_digit()) {
                    has_dot = true;
                    self.advance();
                } else {
                    break;
                }
            } else if (c == 'e' || c == 'E') && !has_exp {
                has_exp = true;
                self.advance();
                if let Some(sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }

        let end_pos = if self.pos < self.chars.len() { self.chars[self.pos].0 } else { self.source.len() };

        let raw = &self.source[start_pos..end_pos];
        if has_dot || has_exp {
            match raw.parse::<f64>() {
                Ok(f) => Ok(Token { kind: TokenKind::Float(f), span }),
                Err(_) => Err(LexError { message: format!("invalid float literal: '{raw}'"), span }),
            }
        } else {
            match raw.parse::<i64>() {
                Ok(i) => Ok(Token { kind: TokenKind::Int(i), span }),
                Err(_) => Err(LexError { message: format!("invalid integer literal: '{raw}'"), span }),
            }
        }
    }

    fn lex_ident(&mut self, span: Span) -> Token {
        let start_pos = self.chars[self.pos].0;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let end_pos = if self.pos < self.chars.len() { self.chars[self.pos].0 } else { self.source.len() };

        let raw = &self.source[start_pos..end_pos];
        let kind = match raw {
            "param" => TokenKind::Param,
            "int" => TokenKind::IntType,
            "float" => TokenKind::FloatType,
            "bool" => TokenKind::BoolType,
            "min" => TokenKind::Min,
            "max" => TokenKind::Max,
            "desc" | "description" => TokenKind::Desc,
            "on_start" => TokenKind::OnStart,
            "on_bar" => TokenKind::OnBar,
            "let" => TokenKind::Let,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,

            // Explicitly forbidden constructs to ensure safety and prevent loops/lookahead/arbitrary code
            "while" | "loop" | "for" | "fn" | "def" | "function" | "async" | "await" | "unsafe"
            | "import" | "include" | "require" | "eval" | "exec" | "spawn" | "fork" => {
                TokenKind::ForbiddenKeyword(raw.to_string())
            }

            _ => TokenKind::Ident(raw.to_string()),
        };

        Token { kind, span }
    }
}
