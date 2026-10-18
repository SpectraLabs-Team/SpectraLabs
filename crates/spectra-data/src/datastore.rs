//! Content-addressed dataset store.
//!
//! Layout:   `<root>/<dataset id>/bars.qfb`   binary bars + symbols + lineage
//!           `<root>/<dataset id>/meta.json`  human-readable summary (write-only)
//!
//! Because the id is a hash of the content, storing the same data twice is a
//! no-op and an id can never silently point at different data. Every load
//! re-verifies three things: the file's trailing checksum, the bars
//! fingerprint, and that the recomputed dataset id equals the directory name.
//!
//! `bars.qfb` is little-endian: magic `QFBARS01`, row counts, symbol table,
//! lineage strings, fixed-width bar records, then a SHA-256 trailer over
//! everything before it. The format is byte-identical to the C++ reference, so
//! stores written by either engine can be read by the other.
//!
//! Limits: whole datasets are held in memory; per-category rejection counts
//! from ingestion live only in `meta.json`.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use spectra_core::{bars_sha256, validate_bars, Bar, Error, Price, Qty, Result, Sha256};

use crate::dataset::{assess_quality, compute_dataset_id, CsvOptions, Dataset};
use crate::time::format_timestamp;

const MAGIC: &[u8; 8] = b"QFBARS01";
const RECORD: usize = 8 + 4 + 5 * 8;

fn data_err(msg: impl Into<String>) -> Error {
    Error::Data(msg.into())
}

fn is_hex_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
}

struct Reader<'a> {
    p: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, k: usize) -> Result<&[u8]> {
        if k > self.p.len() - self.pos {
            return Err(data_err("dataset file is truncated or corrupt"));
        }
        let s = &self.p[self.pos..self.pos + k];
        self.pos += k;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(self.u64()? as i64)
    }
    fn str(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let b = self.take(len)?;
        String::from_utf8(b.to_vec()).map_err(|_| data_err("dataset file contains invalid text"))
    }
    fn remaining(&self) -> usize {
        self.p.len() - self.pos
    }
}

/// The JSON escaping used by `meta.json` (as in the C++ store).
fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_array(v: &[String]) -> String {
    let items: Vec<String> = v.iter().map(|s| json_str(s)).collect();
    format!("[{}]", items.join(", "))
}

fn meta_json(d: &Dataset) -> String {
    let i = &d.info;
    let q = &d.quality;
    let mut j = String::from("{\n");
    j += &format!("  \"id\": {},\n", json_str(&i.id));
    j += &format!("  \"bars_sha256\": {},\n", json_str(&i.bars_sha256));
    j += &format!("  \"source_sha256\": {},\n", json_str(&i.source_sha256));
    j += &format!("  \"parent_id\": {},\n", json_str(&i.parent_id));
    j += &format!("  \"note\": {},\n", json_str(&i.note));
    j += &format!("  \"engine_version\": {},\n", json_str(&i.engine_version));
    j += &format!("  \"bar_count\": {},\n", i.bar_count);
    j += &format!("  \"first_ts_ns\": {},\n", i.first_ts);
    j += &format!("  \"last_ts_ns\": {},\n", i.last_ts);
    j += &format!("  \"first_ts\": {},\n", json_str(&format_timestamp(i.first_ts)));
    j += &format!("  \"last_ts\": {},\n", json_str(&format_timestamp(i.last_ts)));
    j += &format!("  \"symbols\": {},\n", json_array(&d.symbols.symbols));
    j += "  \"quality\": {\n";
    j += &format!("    \"rows_total\": {},\n", q.rows_total);
    j += &format!("    \"rows_accepted\": {},\n", q.rows_accepted);
    j += &format!("    \"rows_rejected\": {},\n", q.rows_rejected);
    let cats: Vec<String> =
        q.rejected_by_category.iter().map(|(c, n)| format!("{}: {n}", json_str(c))).collect();
    j += &format!("    \"rejected_by_category\": {{{}}},\n", cats.join(", "));
    j += &format!("    \"input_was_sorted\": {},\n", q.input_was_sorted);
    j += &format!("    \"warnings\": {},\n", json_array(&q.warnings));
    j += &format!("    \"notes\": {},\n", json_array(&q.notes));
    let syms: Vec<String> = q
        .symbols
        .iter()
        .map(|s| {
            format!(
                "{{\"symbol\": {}, \"bars\": {}, \"gaps\": {}, \"outlier_returns\": {}, \"stale_runs\": {}, \
                 \"zero_volume_bars\": {}}}",
                json_str(&s.symbol),
                s.bars,
                s.gaps,
                s.outlier_returns,
                s.stale_runs,
                s.zero_volume_bars
            )
        })
        .collect();
    j += &format!("    \"symbols\": [{}]\n  }}\n}}\n", syms.join(", "));
    j
}

/// Writes to `<path>.tmp`, flushes it to disk, then renames over `path`.
fn write_file_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    {
        let mut f =
            fs::File::create(&tmp).map_err(|_| data_err(format!("cannot write {}", tmp.display())))?;
        f.write_all(data)
            .and_then(|()| f.sync_all())
            .map_err(|_| data_err(format!("write failed for {}", tmp.display())))?;
    }
    fs::rename(&tmp, path).map_err(|e| data_err(format!("cannot finalize {}: {e}", path.display())))
}

/// The binary `bars.qfb` image of a dataset (exposed for parity tests).
pub fn encode_dataset(d: &Dataset) -> Vec<u8> {
    let mut w = Writer::default();
    w.0.extend_from_slice(MAGIC);
    w.u64(d.bars.len() as u64);
    w.u32(d.symbols.symbols.len() as u32);
    for s in &d.symbols.symbols {
        w.str(s);
    }
    w.str(&d.info.bars_sha256);
    w.str(&d.info.source_sha256);
    w.str(&d.info.parent_id);
    w.str(&d.info.note);
    w.str(&d.info.engine_version);
    w.u64(d.quality.rows_total as u64);
    w.u64(d.quality.rows_rejected as u64);
    for b in &d.bars {
        w.i64(b.ts_ns);
        w.u32(b.instrument);
        w.i64(b.open.raw());
        w.i64(b.high.raw());
        w.i64(b.low.raw());
        w.i64(b.close.raw());
        w.i64(b.volume.raw());
    }
    let mut trailer = Sha256::new();
    trailer.update(&w.0);
    let digest = trailer.digest();
    w.0.extend_from_slice(&digest);
    w.0
}

/// Decodes and fully verifies a `bars.qfb` image that should have id `id`.
///
/// # Errors
/// [`Error::Data`] on any corruption or mismatch.
pub fn decode_dataset(raw: &[u8], id: &str) -> Result<Dataset> {
    if raw.len() < MAGIC.len() + 32 {
        return Err(data_err("dataset file is truncated or corrupt"));
    }
    // 1. Trailing checksum over everything before it.
    let body = raw.len() - 32;
    let mut h = Sha256::new();
    h.update(&raw[..body]);
    if h.digest()[..] != raw[body..] {
        return Err(data_err(format!(
            "checksum mismatch: dataset file {id} is corrupt or has been modified"
        )));
    }

    let mut r = Reader { p: &raw[..body], pos: 0 };
    if r.take(8)? != MAGIC {
        return Err(data_err("not a QuantForge dataset file"));
    }
    let mut d = Dataset::default();
    let n_bars = r.u64()?;
    let n_symbols = r.u32()?;
    for _ in 0..n_symbols {
        d.symbols.symbols.push(r.str()?);
    }
    d.info.bars_sha256 = r.str()?;
    d.info.source_sha256 = r.str()?;
    d.info.parent_id = r.str()?;
    d.info.note = r.str()?;
    d.info.engine_version = r.str()?;
    let rows_total = r.u64()?;
    let rows_rejected = r.u64()?;

    if n_bars > (r.remaining() / RECORD) as u64 || n_bars as usize * RECORD != r.remaining() {
        return Err(data_err("dataset file has an inconsistent record count"));
    }
    d.bars.reserve(n_bars as usize);
    for _ in 0..n_bars {
        d.bars.push(Bar {
            ts_ns: r.i64()?,
            instrument: r.u32()?,
            open: Price::from_raw(r.i64()?),
            high: Price::from_raw(r.i64()?),
            low: Price::from_raw(r.i64()?),
            close: Price::from_raw(r.i64()?),
            volume: Qty::from_raw(r.i64()?),
        });
    }

    // 2. Content must match its recorded fingerprint, 3. and its address.
    if bars_sha256(&d.bars) != d.info.bars_sha256 {
        return Err(data_err("bars fingerprint mismatch"));
    }
    d.info.id = compute_dataset_id(&d.bars, &d.symbols);
    if d.info.id != id {
        return Err(data_err("dataset id mismatch: content does not match its address"));
    }
    if let Some(err) = validate_bars(&d.bars) {
        return Err(data_err(format!("stored bars invalid: {err}")));
    }
    if d.bars.is_empty() {
        return Err(data_err("dataset has no bars"));
    }
    d.info.bar_count = d.bars.len();
    d.info.first_ts = d.bars[0].ts_ns;
    d.info.last_ts = d.bars[d.bars.len() - 1].ts_ns;
    d.quality = assess_quality(&d.bars, &d.symbols, &CsvOptions::default());
    d.quality.rows_total = rows_total as usize;
    d.quality.rows_rejected = rows_rejected as usize;
    d.quality.notes.push("Per-category rejection counts from ingestion are in meta.json.".into());
    Ok(d)
}

/// A directory of content-addressed datasets.
#[derive(Debug, Clone)]
pub struct DataStore {
    root: PathBuf,
}

impl DataStore {
    /// Opens (creating if needed) a store at `root`.
    ///
    /// # Errors
    /// [`Error::Data`] if the directory cannot be created.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|e| data_err(format!("cannot create store directory {}: {e}", root.display())))?;
        Ok(Self { root })
    }

    /// The store directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir_for(&self, id: &str) -> Result<PathBuf> {
        if !is_hex_id(id) {
            return Err(data_err("invalid dataset id"));
        }
        Ok(self.root.join(id))
    }

    /// True if a dataset with this id is stored.
    pub fn contains(&self, id: &str) -> bool {
        is_hex_id(id) && self.root.join(id).join("bars.qfb").exists()
    }

    /// Stored dataset ids, sorted.
    pub fn list(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (e.path().is_dir() && is_hex_id(&name) && e.path().join("bars.qfb").exists()).then_some(name)
            })
            .collect();
        ids.sort();
        ids
    }

    /// Stores the dataset and returns its id. Idempotent.
    ///
    /// # Errors
    /// [`Error::Data`] if `info.id` does not match the content or the files
    /// cannot be written.
    pub fn put(&self, d: &Dataset) -> Result<String> {
        if d.info.id != compute_dataset_id(&d.bars, &d.symbols) || d.info.bars_sha256 != bars_sha256(&d.bars)
        {
            return Err(data_err("dataset id does not match its content; refusing to store"));
        }
        let dir = self.dir_for(&d.info.id)?;
        if dir.join("bars.qfb").exists() {
            return Ok(d.info.id.clone()); // content-addressed: already stored
        }
        fs::create_dir_all(&dir).map_err(|e| data_err(format!("cannot create {}: {e}", dir.display())))?;
        // Metadata first, data last: a directory only counts as stored once bars.qfb exists.
        write_file_atomic(&dir.join("meta.json"), meta_json(d).as_bytes())?;
        write_file_atomic(&dir.join("bars.qfb"), &encode_dataset(d))?;
        Ok(d.info.id.clone())
    }

    /// Loads and verifies a dataset.
    ///
    /// # Errors
    /// [`Error::Data`] if the id is unknown or any integrity check fails.
    pub fn get(&self, id: &str) -> Result<Dataset> {
        let file = self.dir_for(id)?.join("bars.qfb");
        if !file.exists() {
            return Err(data_err(format!("unknown dataset id: {id}")));
        }
        let raw = fs::read(&file).map_err(|_| data_err(format!("cannot open file: {}", file.display())))?;
        decode_dataset(&raw, id)
    }
}
