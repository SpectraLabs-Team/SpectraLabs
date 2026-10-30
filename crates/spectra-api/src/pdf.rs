//! A minimal, dependency-free PDF writer shared by every server-rendered report (M16).
//!
//! Reports draw with raw PDF content-stream operators (text, rectangles, lines) in the base-14
//! Helvetica fonts, so no font files or rendering engine are needed and the bytes are fully
//! determined by the input. The M4 validation report was the first user; the tear sheet and gap
//! reports build on the same pieces.
//!
//! Usage: add objects in order (object numbers start at 1, so the catalog is usually object 1),
//! wrap each page's drawing operators with [`stream_object`], and call [`SimplePdf::finish`].

/// Escapes text for a PDF string literal `( … )`.
///
/// Parentheses and backslashes are escaped; tabs and line breaks become spaces. The base-14
/// fonts cover ASCII only, so any other character is written as `?`.
pub(crate) fn escape_pdf(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// A PDF content stream object (`<< /Length n >> stream … endstream`) for a page's drawing
/// operators.
pub(crate) fn stream_object(operators: &str) -> Vec<u8> {
    let bytes = operators.as_bytes();
    let header = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
    let mut out = Vec::with_capacity(header.len() + bytes.len() + 12);
    out.extend_from_slice(&header);
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A PDF document assembled from numbered objects, with the cross-reference table and trailer
/// written by [`SimplePdf::finish`]. Object 1 must be the document catalog.
#[derive(Debug, Default)]
pub(crate) struct SimplePdf {
    objects: Vec<Vec<u8>>,
}

impl SimplePdf {
    pub(crate) fn new() -> Self {
        Self { objects: Vec::new() }
    }

    /// Adds an object (its dictionary or stream, without `n 0 obj`) and returns its number.
    pub(crate) fn add_object(&mut self, data: Vec<u8>) -> usize {
        self.objects.push(data);
        self.objects.len()
    }

    /// The finished file: header, objects, cross-reference table and trailer.
    pub(crate) fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::new();
        for (i, obj) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            let num = i + 1;
            let header = format!("{num} 0 obj\n");
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = out.len();
        let total_objs = self.objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {total_objs}\n0000000000 65535 f \n").as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_pdf_string_syntax_and_non_ascii() {
        assert_eq!(escape_pdf(r"a (b) \c"), r"a \(b\) \\c");
        assert_eq!(escape_pdf("x\ny\tz"), "x y z");
        assert_eq!(escape_pdf("DSR — 0.81 ✓"), "DSR ? 0.81 ?");
    }

    #[test]
    fn stream_length_counts_the_operators() {
        let s = stream_object("BT ET");
        assert_eq!(s, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec());
    }

    #[test]
    fn cross_reference_offsets_point_at_each_object() {
        let mut pdf = SimplePdf::new();
        assert_eq!(pdf.add_object(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()), 1);
        assert_eq!(pdf.add_object(b"<< /Type /Pages /Kids [] /Count 0 >>".to_vec()), 2);
        let out = pdf.finish();
        // Byte offsets: the header holds non-UTF-8 bytes, so search the raw bytes.
        let xref = out.windows(5).position(|w| w == b"xref\n").unwrap();
        let tail = std::str::from_utf8(&out[xref..]).unwrap();
        // Every xref entry's offset is where "<n> 0 obj" starts.
        let entries: Vec<usize> = tail.lines().skip(3).take(2).map(|l| l[..10].parse().unwrap()).collect();
        assert!(out[entries[0]..].starts_with(b"1 0 obj\n"));
        assert!(out[entries[1]..].starts_with(b"2 0 obj\n"));
        let startxref: usize = tail.lines().rev().nth(1).unwrap().parse().unwrap();
        assert_eq!(startxref, xref);
        assert!(tail.contains("<< /Size 3 /Root 1 0 R >>") && out.ends_with(b"%%EOF\n"));
    }
}
