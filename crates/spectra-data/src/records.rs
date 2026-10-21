//! Reading CSV records one at a time from any [`Read`] (M1b task 9).
//!
//! [`RecordReader`] is the tokenizer `parse_records` used to be, unchanged in
//! behaviour, made incremental: it keeps one record in memory rather than the
//! whole file, so a large file can be ingested in memory proportional to its
//! bars, not its text. It also hashes every byte it reads (including a
//! leading byte-order mark, as before) and counts them, for the dataset's
//! `source_sha256` and for progress.
//!
//! The rules, as before: a field is quoted when its first non-blank character
//! is `"`; `""` inside quotes is a literal quote; quoted fields keep their
//! spaces and line breaks; unquoted fields are trimmed and `\r` is dropped
//! outside quotes; a line holding one empty field is skipped; a quote left
//! open at the end of the file is an error.

use std::io::Read;

use spectra_core::{Error, Result, Sha256};

use crate::time::trim;

fn data_err(msg: impl Into<String>) -> Error {
    Error::Data(msg.into())
}

/// One record: its 1-based starting line and its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) line: usize,
    pub(crate) fields: Vec<String>,
}

const BUF: usize = 64 * 1024;

/// Records from a byte stream, one at a time.
pub(crate) struct RecordReader<R: Read> {
    src: R,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    eof: bool,
    started: bool,
    delim: u8,
    line: usize,
    hasher: Sha256,
    bytes_read: u64,
}

impl<R: Read> RecordReader<R> {
    pub(crate) fn new(src: R, delim: u8) -> Self {
        Self {
            src,
            buf: vec![0; BUF],
            pos: 0,
            len: 0,
            eof: false,
            started: false,
            delim,
            line: 1,
            hasher: Sha256::new(),
            bytes_read: 0,
        }
    }

    /// Bytes consumed from the stream so far.
    pub(crate) fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    /// Refills the buffer. `Ok(false)` at the end of the stream.
    fn fill(&mut self) -> Result<bool> {
        if self.eof {
            return Ok(false);
        }
        loop {
            match self.src.read(&mut self.buf) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.hasher.update(&self.buf[..n]);
                    self.bytes_read += n as u64;
                    self.pos = 0;
                    self.len = n;
                    return Ok(true);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(data_err(format!("cannot read the file: {e}"))),
            }
        }
    }

    fn peek(&mut self) -> Result<Option<u8>> {
        if self.pos >= self.len && !self.fill()? {
            return Ok(None);
        }
        Ok(Some(self.buf[self.pos]))
    }

    fn next_byte(&mut self) -> Result<Option<u8>> {
        let b = self.peek()?;
        if b.is_some() {
            self.pos += 1;
        }
        Ok(b)
    }

    /// Skips a UTF-8 byte-order mark at the very start of the stream.
    fn skip_bom(&mut self) -> Result<()> {
        self.started = true;
        // The mark may straddle reads: gather up to three bytes first.
        let mut head = Vec::with_capacity(3);
        while head.len() < 3 {
            match self.next_byte()? {
                Some(b) => head.push(b),
                None => break,
            }
        }
        if head != b"\xEF\xBB\xBF" {
            // Not a mark: put the bytes back in front of the buffer.
            let rest = self.buf[self.pos..self.len].to_vec();
            let mut merged = head;
            merged.extend_from_slice(&rest);
            if merged.len() > self.buf.len() {
                self.buf.resize(merged.len(), 0);
            }
            self.buf[..merged.len()].copy_from_slice(&merged);
            self.pos = 0;
            self.len = merged.len();
        }
        Ok(())
    }

    /// The next record that is not a blank line, or `None` at the end.
    ///
    /// # Errors
    /// An unterminated quoted field (at the end of the stream) or a read error.
    pub(crate) fn next_record(&mut self) -> Result<Option<Record>> {
        if !self.started {
            self.skip_bom()?;
        }
        loop {
            if self.peek()?.is_none() {
                return Ok(None);
            }
            let start_line = self.line;
            let mut fields: Vec<String> = Vec::new();
            let mut field: Vec<u8> = Vec::new();
            let mut in_quotes = false;
            let mut quoted = false;
            let end_field = |fields: &mut Vec<String>, field: &mut Vec<u8>, quoted: &mut bool| {
                let text = if *quoted { to_string_lossy(field) } else { to_string_lossy(trim(field)) };
                fields.push(text);
                field.clear();
                *quoted = false;
            };
            let mut at_eof = false;
            loop {
                let Some(c) = self.next_byte()? else {
                    at_eof = true;
                    break;
                };
                if in_quotes {
                    if c == b'"' {
                        if self.peek()? == Some(b'"') {
                            field.push(b'"');
                            self.pos += 1;
                        } else {
                            in_quotes = false;
                        }
                    } else {
                        if c == b'\n' {
                            self.line += 1;
                        }
                        field.push(c);
                    }
                } else if c == b'"' && trim(&field).is_empty() {
                    field.clear();
                    in_quotes = true;
                    quoted = true;
                } else if c == self.delim {
                    end_field(&mut fields, &mut field, &mut quoted);
                } else if c == b'\n' {
                    end_field(&mut fields, &mut field, &mut quoted);
                    self.line += 1;
                    break;
                } else if c != b'\r' {
                    field.push(c);
                }
            }
            if at_eof {
                if in_quotes {
                    return Err(data_err(format!("unterminated quoted field starting on line {start_line}")));
                }
                end_field(&mut fields, &mut field, &mut quoted);
            }
            let blank = fields.len() == 1 && fields[0].is_empty();
            if !blank {
                return Ok(Some(Record { line: start_line, fields }));
            }
            if at_eof {
                return Ok(None);
            }
        }
    }

    /// Reads to the end of the stream. A file-level error found there (an
    /// unterminated quote) is returned: the whole-file parser reported it
    /// before any row error, so streaming callers keep that precedence.
    pub(crate) fn drain(&mut self) -> Option<Error> {
        loop {
            match self.next_record() {
                Ok(Some(_)) => {}
                Ok(None) => return None,
                Err(e) => return Some(e),
            }
        }
    }

    /// Hex SHA-256 of every byte read so far.
    pub(crate) fn source_sha256(&self) -> String {
        self.hasher.clone().hex_digest()
    }
}

fn to_string_lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Every record of `s`.
///
/// # Errors
/// An unterminated quoted field.
pub(crate) fn parse_records(s: &[u8], delim: u8) -> Result<Vec<Record>> {
    let mut r = RecordReader::new(s, delim);
    // Callers strip a byte-order mark themselves when they want it gone.
    r.started = true;
    let mut out = Vec::new();
    while let Some(rec) = r.next_record()? {
        out.push(rec);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tokenizer as it was before streaming, kept verbatim to prove the
    /// streaming one gives the same records.
    fn reference(s: &[u8], delim: u8) -> Result<Vec<Record>> {
        let mut out = Vec::new();
        let mut cur = Record { line: 0, fields: Vec::new() };
        let mut field: Vec<u8> = Vec::new();
        let mut in_quotes = false;
        let mut quoted_field = false;
        let mut record_open = false;
        let mut line = 1usize;
        fn end_field(cur: &mut Record, field: &mut Vec<u8>, quoted: &mut bool) {
            let text = if *quoted { to_string_lossy(field) } else { to_string_lossy(trim(field)) };
            cur.fields.push(text);
            field.clear();
            *quoted = false;
        }
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if !record_open {
                cur.line = line;
                record_open = true;
            }
            if in_quotes {
                if c == b'"' {
                    if i + 1 < s.len() && s[i + 1] == b'"' {
                        field.push(b'"');
                        i += 1;
                    } else {
                        in_quotes = false;
                    }
                } else {
                    if c == b'\n' {
                        line += 1;
                    }
                    field.push(c);
                }
            } else if c == b'"' && trim(&field).is_empty() {
                field.clear();
                in_quotes = true;
                quoted_field = true;
            } else if c == delim {
                end_field(&mut cur, &mut field, &mut quoted_field);
            } else if c == b'\n' {
                end_field(&mut cur, &mut field, &mut quoted_field);
                let blank = cur.fields.len() == 1 && cur.fields[0].is_empty();
                let done = std::mem::replace(&mut cur, Record { line: 0, fields: Vec::new() });
                if !blank {
                    out.push(done);
                }
                record_open = false;
                line += 1;
            } else if c != b'\r' {
                field.push(c);
            }
            i += 1;
        }
        if in_quotes {
            return Err(data_err(format!("unterminated quoted field starting on line {}", cur.line)));
        }
        if record_open {
            end_field(&mut cur, &mut field, &mut quoted_field);
            let blank = cur.fields.len() == 1 && cur.fields[0].is_empty();
            if !blank {
                out.push(cur);
            }
        }
        Ok(out)
    }

    /// A reader that hands out at most `n` bytes per call, to exercise every buffer boundary.
    struct Trickle<'a> {
        data: &'a [u8],
        n: usize,
    }
    impl Read for Trickle<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let k = self.n.min(out.len()).min(self.data.len());
            out[..k].copy_from_slice(&self.data[..k]);
            self.data = &self.data[k..];
            Ok(k)
        }
    }

    fn streamed(s: &[u8], delim: u8, n: usize) -> Result<Vec<Record>> {
        let mut r = RecordReader::new(Trickle { data: s, n }, delim);
        let mut out = Vec::new();
        while let Some(rec) = r.next_record()? {
            out.push(rec);
        }
        Ok(out)
    }

    const CASES: &[&[u8]] = &[
        b"",
        b"\n",
        b"\n\n\n",
        b"a,b\n1,2\n",
        b"a,b\n1,2",
        b"a,b\r\n1,2\r\n\r\n3,4\r\n",
        b"  a , b \n 1 , 2 \n",
        b"a,b\n\"x, y\",\"he said \"\"hi\"\"\"\n",
        b"a,b\n\"multi\nline\",2\n3,4\n",
        b"a,b\n  \"quoted after spaces\" ,2\n",
        b"a,b\nx\"y,2\n",
        b"a,b\n\"\",\"\"\n",
        b"a,b\n\"unterminated,2\n3,4\n",
        b"a,b\n1,2\n\"",
        b"a;b\n1;2\n",
        b"\xEF\xBB\xBFa,b\n1,2\n",
        b"\xEF\xBB",
        b"a,b\n\"x\"\"",
        b"a,b\n\"\r\n\"\r\n",
        b"a\n,\n\n,,\n",
    ];

    #[test]
    fn streaming_matches_the_whole_file_tokenizer_at_every_buffer_size() {
        for case in CASES {
            // Callers of the old parser stripped a byte-order mark first.
            let stripped = case.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(case);
            let delim = if case.contains(&b';') { b';' } else { b',' };
            let want = reference(stripped, delim);
            for n in [1, 2, 3, 7, 64, 1 << 20] {
                let got = streamed(case, delim, n);
                match (&want, &got) {
                    (Ok(a), Ok(b)) => {
                        assert_eq!(a, b, "case {:?} at {n} bytes per read", String::from_utf8_lossy(case));
                    }
                    (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
                    _ => panic!("case {:?}: {want:?} vs {got:?}", String::from_utf8_lossy(case)),
                }
            }
        }
    }

    #[test]
    fn bytes_are_hashed_and_counted_including_the_mark() {
        let data = b"\xEF\xBB\xBFa,b\n1,2\n";
        let mut r = RecordReader::new(Trickle { data, n: 2 }, b',');
        while r.next_record().unwrap().is_some() {}
        assert_eq!(r.bytes_read(), data.len() as u64);
        assert_eq!(r.source_sha256(), spectra_core::sha256_hex(data));
    }

    #[test]
    fn drain_finds_a_late_unterminated_quote() {
        let mut r = RecordReader::new(&b"a,b\n1,2\n3,4\n\"oops\n"[..], b',');
        assert!(r.next_record().unwrap().is_some());
        assert!(r.drain().unwrap().to_string().contains("unterminated quoted field starting on line 4"));
        let mut ok = RecordReader::new(&b"a,b\n1,2\n"[..], b',');
        assert!(ok.drain().is_none());
    }
}
