//! A small strict JSON value, parser and serializer (RFC 8259).
//!
//! Used for the registry's event log and report exports. Ported from the C++
//! reference `qf::Json`; serialization is byte-identical, which matters because
//! registry events are hash-chained over their serialized form.
//!
//!  * Objects keep insertion order and [`Json::dump`] is deterministic.
//!  * Integers that fit in `i64` stay exact ([`Json::Int`]); anything with a
//!    fraction or exponent is a double, written with `%.17g` (so 3.0 is
//!    written "3" and re-parses as an Int; the two compare equal).
//!  * Non-finite numbers cannot be represented: [`Json::number`] refuses them.
//!  * Duplicate keys, trailing text, leading zeros and nesting deeper than 64
//!    levels are rejected on parse. `\uXXXX` escapes (including surrogate
//!    pairs) are decoded.

use crate::cfmt::fmt_g17;
use crate::error::{Error, Result};

/// A JSON value.
#[derive(Debug, Clone, Default)]
pub enum Json {
    /// `null`
    #[default]
    Null,
    /// `true` / `false`
    Bool(bool),
    /// An integer that fits in `i64`.
    Int(i64),
    /// Any other (finite) number.
    Double(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object with insertion-ordered keys.
    Object(Vec<(String, Json)>),
}

fn type_err(what: &str) -> Error {
    Error::Json(format!("JSON value is not {what}"))
}

impl Json {
    /// A boolean.
    pub fn boolean(v: bool) -> Self {
        Self::Bool(v)
    }
    /// An exact integer.
    pub fn integer(v: i64) -> Self {
        Self::Int(v)
    }
    /// A double.
    ///
    /// # Errors
    /// If `v` is NaN or infinite.
    pub fn number(v: f64) -> Result<Self> {
        if v.is_finite() {
            Ok(Self::Double(v))
        } else {
            Err(Error::Json("JSON cannot represent a non-finite number".into()))
        }
    }
    /// A string.
    pub fn string(v: impl Into<String>) -> Self {
        Self::String(v.into())
    }
    /// An empty array.
    pub fn array() -> Self {
        Self::Array(Vec::new())
    }
    /// An empty object.
    pub fn object() -> Self {
        Self::Object(Vec::new())
    }

    /// True for `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// # Errors
    /// If this is not a boolean.
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Self::Bool(b) => Ok(*b),
            _ => Err(type_err("a boolean")),
        }
    }
    /// # Errors
    /// If this is not an integer.
    pub fn as_int(&self) -> Result<i64> {
        match self {
            Self::Int(i) => Ok(*i),
            _ => Err(type_err("an integer")),
        }
    }
    /// Accepts integers too.
    ///
    /// # Errors
    /// If this is not a number.
    pub fn as_double(&self) -> Result<f64> {
        match self {
            Self::Int(i) => Ok(*i as f64),
            Self::Double(d) => Ok(*d),
            _ => Err(type_err("a number")),
        }
    }
    /// # Errors
    /// If this is not a string.
    pub fn as_str(&self) -> Result<&str> {
        match self {
            Self::String(s) => Ok(s),
            _ => Err(type_err("a string")),
        }
    }

    /// Append to an array.
    ///
    /// # Errors
    /// If this is not an array.
    pub fn push(&mut self, v: Self) -> Result<&mut Self> {
        match self {
            Self::Array(a) => {
                a.push(v);
                Ok(self)
            }
            _ => Err(type_err("an array")),
        }
    }
    /// Array items.
    ///
    /// # Errors
    /// If this is not an array.
    pub fn items(&self) -> Result<&[Json]> {
        match self {
            Self::Array(a) => Ok(a),
            _ => Err(type_err("an array")),
        }
    }

    /// Set a key, replacing an existing one in place.
    ///
    /// # Errors
    /// If this is not an object.
    pub fn set(&mut self, key: impl Into<String>, v: Self) -> Result<&mut Self> {
        let key = key.into();
        match self {
            Self::Object(o) => {
                if let Some(slot) = o.iter_mut().find(|(k, _)| *k == key) {
                    slot.1 = v;
                } else {
                    o.push((key, v));
                }
                Ok(self)
            }
            _ => Err(type_err("an object")),
        }
    }
    /// Builder form of [`Json::set`] for objects created in place.
    ///
    /// # Panics
    /// If this is not an object (a programming error).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, v: Self) -> Self {
        self.set(key, v).expect("Json::with on a non-object");
        self
    }
    /// The value of `key`, or `None` if absent or this is not an object.
    pub fn find(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// The value of `key`.
    ///
    /// # Errors
    /// If absent.
    pub fn at(&self, key: &str) -> Result<&Json> {
        self.find(key).ok_or_else(|| Error::Json(format!("missing JSON key: {key}")))
    }
    /// Object entries in insertion order.
    ///
    /// # Errors
    /// If this is not an object.
    pub fn entries(&self) -> Result<&[(String, Json)]> {
        match self {
            Self::Object(o) => Ok(o),
            _ => Err(type_err("an object")),
        }
    }

    /// Deterministic compact serialization.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        dump_into(&mut out, self);
        out
    }

    /// Strict parse.
    ///
    /// # Errors
    /// [`Error::Json`] describing the first problem and its byte offset.
    pub fn parse(text: &str) -> Result<Self> {
        Parser { t: text.as_bytes(), pos: 0 }.parse_document()
    }
}

impl PartialEq for Json {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Int(_) | Self::Double(_), Self::Int(_) | Self::Double(_)) => {
                self.as_double().ok() == other.as_double().ok()
            }
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            _ => false,
        }
    }
}

fn escape_into(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn dump_into(out: &mut String, j: &Json) {
    match j {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Double(d) => out.push_str(&fmt_g17(*d)),
        Json::String(s) => escape_into(out, s),
        Json::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                dump_into(out, v);
            }
            out.push(']');
        }
        Json::Object(o) => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                escape_into(out, k);
                out.push(':');
                dump_into(out, v);
            }
            out.push('}');
        }
    }
}

struct Parser<'a> {
    t: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn fail<T>(&self, msg: &str) -> Result<T> {
        Err(Error::Json(format!("JSON parse error at offset {}: {msg}", self.pos)))
    }
    fn skip_ws(&mut self) {
        while self.pos < self.t.len() && matches!(self.t[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }
    fn peek(&self) -> u8 {
        self.t.get(self.pos).copied().unwrap_or(0)
    }
    fn eof(&self) -> bool {
        self.pos >= self.t.len()
    }
    fn expect(&mut self, c: u8) -> Result<()> {
        if self.eof() || self.peek() != c {
            return self.fail(&format!("expected '{}'", c as char));
        }
        self.pos += 1;
        Ok(())
    }
    fn literal(&mut self, lit: &[u8]) -> bool {
        if self.t[self.pos..].starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn parse_document(mut self) -> Result<Json> {
        self.skip_ws();
        let v = self.parse_value(0)?;
        self.skip_ws();
        if self.pos != self.t.len() {
            return self.fail("unexpected trailing text");
        }
        Ok(v)
    }

    fn parse_value(&mut self, depth: u32) -> Result<Json> {
        if depth > 64 {
            return self.fail("nesting too deep");
        }
        if self.eof() {
            return self.fail("unexpected end of input");
        }
        match self.peek() {
            b'{' => self.parse_object(depth),
            b'[' => self.parse_array(depth),
            b'"' => Ok(Json::String(self.parse_string()?)),
            b't' => {
                if self.literal(b"true") {
                    Ok(Json::Bool(true))
                } else {
                    self.fail("invalid literal")
                }
            }
            b'f' => {
                if self.literal(b"false") {
                    Ok(Json::Bool(false))
                } else {
                    self.fail("invalid literal")
                }
            }
            b'n' => {
                if self.literal(b"null") {
                    Ok(Json::Null)
                } else {
                    self.fail("invalid literal")
                }
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => self.fail("unexpected character"),
        }
    }

    fn parse_object(&mut self, depth: u32) -> Result<Json> {
        self.expect(b'{')?;
        let mut obj: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == b'}' {
            self.pos += 1;
            return Ok(Json::Object(obj));
        }
        loop {
            self.skip_ws();
            if self.peek() != b'"' {
                return self.fail("expected a string key");
            }
            let key = self.parse_string()?;
            if obj.iter().any(|(k, _)| *k == key) {
                return self.fail(&format!("duplicate key: {key}"));
            }
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let v = self.parse_value(depth + 1)?;
            obj.push((key, v));
            self.skip_ws();
            if self.peek() == b',' {
                self.pos += 1;
                continue;
            }
            self.expect(b'}')?;
            return Ok(Json::Object(obj));
        }
    }

    fn parse_array(&mut self, depth: u32) -> Result<Json> {
        self.expect(b'[')?;
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == b']' {
            self.pos += 1;
            return Ok(Json::Array(arr));
        }
        loop {
            self.skip_ws();
            arr.push(self.parse_value(depth + 1)?);
            self.skip_ws();
            if self.peek() == b',' {
                self.pos += 1;
                continue;
            }
            self.expect(b']')?;
            return Ok(Json::Array(arr));
        }
    }

    fn parse_hex4(&mut self) -> Result<u32> {
        if self.pos + 4 > self.t.len() {
            return self.fail("truncated \\u escape");
        }
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.t[self.pos];
            self.pos += 1;
            let d = match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'a'..=b'f' => u32::from(c - b'a' + 10),
                b'A'..=b'F' => u32::from(c - b'A' + 10),
                _ => return self.fail("invalid hex digit in \\u escape"),
            };
            v = (v << 4) | d;
        }
        Ok(v)
    }

    fn parse_string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            if self.eof() {
                return self.fail("unterminated string");
            }
            let c = self.t[self.pos];
            self.pos += 1;
            if c == b'"' {
                return String::from_utf8(out).or_else(|_| self.fail("string is not valid UTF-8"));
            }
            if c < 0x20 {
                return self.fail("unescaped control character in string");
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            if self.eof() {
                return self.fail("unterminated escape");
            }
            let e = self.t[self.pos];
            self.pos += 1;
            match e {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'/' => out.push(b'/'),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'u' => {
                    let mut cp = self.parse_hex4()?;
                    if (0xD800..=0xDBFF).contains(&cp) {
                        if !(self.pos + 1 < self.t.len()
                            && self.t[self.pos] == b'\\'
                            && self.t[self.pos + 1] == b'u')
                        {
                            return self.fail("unpaired surrogate");
                        }
                        self.pos += 2;
                        let lo = self.parse_hex4()?;
                        if !(0xDC00..=0xDFFF).contains(&lo) {
                            return self.fail("invalid low surrogate");
                        }
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                    } else if (0xDC00..=0xDFFF).contains(&cp) {
                        return self.fail("unpaired low surrogate");
                    }
                    let ch = char::from_u32(cp).expect("valid scalar after surrogate checks");
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => return self.fail("invalid escape"),
            }
        }
    }

    fn digits(&mut self) {
        while self.peek().is_ascii_digit() {
            self.pos += 1;
        }
    }

    fn parse_number(&mut self) -> Result<Json> {
        let start = self.pos;
        if self.peek() == b'-' {
            self.pos += 1;
        }
        if self.peek() == b'0' {
            self.pos += 1;
            if self.peek().is_ascii_digit() {
                return self.fail("leading zero in number");
            }
        } else if (b'1'..=b'9').contains(&self.peek()) {
            self.digits();
        } else {
            return self.fail("invalid number");
        }
        let mut is_int = true;
        if self.peek() == b'.' {
            is_int = false;
            self.pos += 1;
            if !self.peek().is_ascii_digit() {
                return self.fail("digit expected after decimal point");
            }
            self.digits();
        }
        if matches!(self.peek(), b'e' | b'E') {
            is_int = false;
            self.pos += 1;
            if matches!(self.peek(), b'+' | b'-') {
                self.pos += 1;
            }
            if !self.peek().is_ascii_digit() {
                return self.fail("digit expected in exponent");
            }
            self.digits();
        }
        let text = std::str::from_utf8(&self.t[start..self.pos]).expect("ASCII number");
        if is_int {
            if let Ok(v) = text.parse::<i64>() {
                return Ok(Json::Int(v));
            }
            // Integer outside i64: fall through to a double.
        }
        match text.parse::<f64>() {
            Ok(d) if d.is_finite() => Ok(Json::Double(d)),
            _ => self.fail("number out of range"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_order() {
        let j = Json::object()
            .with("b", Json::integer(1))
            .with("a", Json::number(3.0).unwrap())
            .with("s", Json::string("x\"y\\z\n\u{1}é"))
            .with("arr", Json::Array(vec![Json::Null, Json::Bool(true), Json::number(0.1).unwrap()]));
        let text = j.dump();
        assert_eq!(text, r#"{"b":1,"a":3,"s":"x\"y\\z\n\u0001é","arr":[null,true,0.10000000000000001]}"#);
        let back = Json::parse(&text).unwrap();
        assert_eq!(back, j); // 3 re-parses as Int and compares equal to 3.0
        assert_eq!(back.dump(), text);
    }

    #[test]
    fn strictness() {
        for bad in [
            "",
            "01",
            "1.",
            "-",
            "{\"a\":1,\"a\":2}",
            "[1,]",
            "{} x",
            "\"\u{1}\"",
            "nul",
            "\"\\ud800\"",
            "1e",
            "\"\\x\"",
        ] {
            assert!(Json::parse(bad).is_err(), "should reject {bad:?}");
        }
        let deep = "[".repeat(66) + &"]".repeat(66);
        assert!(Json::parse(&deep).is_err());
        assert!(Json::number(f64::NAN).is_err());
    }

    #[test]
    fn numbers() {
        assert_eq!(Json::parse("-0").unwrap(), Json::Int(0));
        assert_eq!(Json::parse("9223372036854775807").unwrap(), Json::Int(i64::MAX));
        assert!(matches!(Json::parse("9223372036854775808").unwrap(), Json::Double(_)));
        assert_eq!(Json::parse("1e2").unwrap().as_double().unwrap(), 100.0);
        assert_eq!(Json::parse("\"\\ud83d\\ude00\"").unwrap().as_str().unwrap(), "😀");
    }

    #[test]
    fn accessors_report_type_errors() {
        let j = Json::parse(r#"{"a":true}"#).unwrap();
        assert!(j.at("a").unwrap().as_int().is_err());
        assert!(j.at("missing").is_err());
        assert!(j.find("a").is_some());
    }
}
