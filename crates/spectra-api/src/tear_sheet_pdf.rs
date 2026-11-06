//! Renders a tear sheet to PDF from the same sections the frontend shows (M16 Task 6).
//!
//! The base-14 fonts cover ASCII only, so the sheet's typographic characters are mapped first:
//! the minus sign becomes `-`, sigma becomes `sigma`, and the dash placeholder becomes `-`.

use crate::pdf::{escape_pdf, stream_object, SimplePdf};
use crate::tear_sheet::{TearSheet, TearSheetItem, TearSheetSection, TearSheetTable};

const PAGE_TOP: f64 = 740.0;
const PAGE_BOTTOM: f64 = 60.0;
const LEFT: f64 = 40.0;
const VALUE_X: f64 = 420.0;

/// Maps the sheet's non-ASCII characters to ASCII, then escapes for a PDF string.
fn text(s: &str) -> String {
    let ascii = s.replace('−', "-").replace('σ', "sigma").replace('—', "-");
    escape_pdf(&ascii)
}

/// One page of drawing operators under construction.
struct Page {
    ops: String,
}

impl Page {
    /// Starts a page with the brand bar and the sheet title, on every page.
    fn new(title: &str, subtitle: &str) -> Self {
        let mut p = Page { ops: String::new() };
        p.ops.push_str("0.12 0.35 0.85 rg\n40 756 532 5 re f\n");
        p.draw(16.0, "F2", LEFT, 728.0, (0.08, 0.08, 0.12), title);
        p.draw(8.5, "F1", LEFT, 714.0, (0.4, 0.42, 0.48), subtitle);
        p.ops.push_str("0.85 0.86 0.90 RG\n0.75 w\n40 704 m 572 704 l S\n");
        p
    }

    /// Writes one line of text in a font, size, colour and position.
    fn draw(&mut self, size: f64, font: &str, x: f64, y: f64, rgb: (f64, f64, f64), s: &str) {
        self.ops.push_str(&format!(
            "{:.2} {:.2} {:.2} rg\nBT /{font} {size} Tf {x:.1} {y:.1} Td ({}) Tj ET\n",
            rgb.0,
            rgb.1,
            rgb.2,
            text(s)
        ));
    }
}

fn tone_colour(tone: Option<&str>) -> (f64, f64, f64) {
    match tone {
        Some("profit") => (0.10, 0.50, 0.25),
        Some("loss") => (0.75, 0.15, 0.15),
        _ => (0.08, 0.08, 0.12),
    }
}

/// Pages in order, starting a new one whenever the next block would not fit.
struct Writer {
    title: String,
    subtitle: String,
    pages: Vec<Page>,
    y: f64,
}

impl Writer {
    fn new(title: &str, subtitle: &str) -> Self {
        let page = Page::new(title, subtitle);
        Writer { title: title.into(), subtitle: subtitle.into(), pages: vec![page], y: PAGE_TOP - 40.0 }
    }

    /// Makes room for `height` points, starting a fresh page if needed.
    fn reserve(&mut self, height: f64) {
        if self.y - height < PAGE_BOTTOM {
            self.pages.push(Page::new(&self.title, &self.subtitle));
            self.y = PAGE_TOP - 40.0;
        }
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("at least one page")
    }

    fn section(&mut self, section: &TearSheetSection) {
        self.reserve(30.0 + 30.0 * section.items.len() as f64);
        let y = self.y;
        self.page().draw(10.0, "F2", LEFT, y, (0.12, 0.35, 0.85), &section.category.to_uppercase());
        self.y -= 20.0;
        for item in &section.items {
            self.item(item);
        }
        self.y -= 12.0;
    }

    fn item(&mut self, item: &TearSheetItem) {
        let y = self.y;
        let page = self.page();
        page.draw(10.0, "F2", LEFT, y, (0.08, 0.08, 0.12), &item.label);
        if let Some(sub) = &item.sub {
            page.draw(8.0, "F1", LEFT, y - 12.0, (0.45, 0.47, 0.52), sub);
        }
        page.draw(12.0, "F2", VALUE_X, y, tone_colour(item.tone), &item.value);
        self.y -= 30.0;
    }

    fn table(&mut self, table: &TearSheetTable) {
        self.reserve(40.0 + 14.0 * table.rows.len() as f64);
        let y = self.y;
        self.page().draw(10.0, "F2", LEFT, y, (0.12, 0.35, 0.85), &table.title.to_uppercase());
        self.y -= 18.0;
        let columns = [LEFT, 180.0, 245.0, 310.0, 375.0, 440.0, 500.0];
        let header_y = self.y;
        for (x, h) in columns.iter().zip(&table.headers) {
            self.page().draw(8.0, "F2", *x, header_y, (0.2, 0.22, 0.28), h);
        }
        self.y -= 14.0;
        for row in &table.rows {
            self.reserve(14.0);
            let row_y = self.y;
            for (x, cell) in columns.iter().zip(row) {
                self.page().draw(8.5, "F1", *x, row_y, (0.08, 0.08, 0.12), cell);
            }
            self.y -= 14.0;
        }
    }
}

/// Renders the sheet as a PDF. `title`, `subtitle` and `generated` appear on every page header.
pub fn render(sheet: &TearSheet, title: &str, subtitle: &str, generated: &str) -> Vec<u8> {
    let mut w = Writer::new(title, &format!("{subtitle}  |  Generated {generated}"));
    for section in &sheet.sections {
        w.section(section);
    }
    if let Some(table) = &sheet.table {
        w.table(table);
    }

    let n = w.pages.len();
    let mut pdf = SimplePdf::new();
    // Objects: 1 catalog, 2 page tree, 3 Helvetica, 4 Helvetica-Bold, then a page and its
    // content stream for each page.
    pdf.add_object(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    pdf.add_object(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
    pdf.add_object(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    pdf.add_object(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_vec());
    for (i, page) in w.pages.iter().enumerate() {
        pdf.add_object(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                6 + 2 * i
            )
            .into_bytes(),
        );
        pdf.add_object(stream_object(&page.ops));
    }
    pdf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tear_sheet::{TearSheetItem, TearSheetSection};

    fn sheet(items: usize, rows: usize) -> TearSheet {
        let section = TearSheetSection {
            category: "Return & Capital Dynamics".into(),
            items: (0..items)
                .map(|i| TearSheetItem {
                    label: format!("Metric {i}"),
                    value: "−14.22%".into(),
                    sub: Some("sub text".into()),
                    tone: Some("loss"),
                })
                .collect(),
        };
        let table = TearSheetTable {
            title: "Strategies".into(),
            headers: vec!["Strategy".into(), "Allocation".into()],
            rows: (0..rows).map(|i| vec![format!("strat{i}"), "50,000".into()]).collect(),
        };
        TearSheet { sections: vec![section], table: Some(table) }
    }

    #[test]
    fn renders_a_valid_single_page_pdf() {
        let out = render(&sheet(4, 2), "Quantitative Strategy Tear-Sheet", "Run X", "2026-11-06 21:00 UTC");
        assert!(out.starts_with(b"%PDF-1.4"));
        assert!(out.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("/Count 1"));
        assert!(text.contains("(-14.22%) Tj"), "minus sign is mapped to ASCII");
        assert!(!text.contains('?'), "no placeholder characters from non-ASCII text");
    }

    #[test]
    fn long_tables_break_onto_more_pages() {
        let out = render(&sheet(4, 80), "Title", "Run", "now");
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("/Count 2") || text.contains("/Count 3"));
    }

    #[test]
    fn maps_typographic_characters_to_ascii() {
        assert_eq!(text("σ 1.00 — −2%"), "sigma 1.00 - -2%");
    }
}
