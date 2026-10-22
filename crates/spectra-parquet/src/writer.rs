//! Parquet exporter for datasets (M1b task 9 part B).

use std::io::Write;
use std::sync::Arc;

use arrow_array::{ArrayRef, Decimal128Array, RecordBatch, StringArray, TimestampNanosecondArray};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use spectra_core::{Error, Result};
use spectra_data::Dataset;

const CHUNK_SIZE: usize = 8192;

/// Exports a [`Dataset`] to an Apache Parquet writer.
///
/// Converts bars and symbols to standard typed Parquet columns in chunks of 8192 rows.
///
/// # Errors
/// Returns an error if writing to the output stream fails or if schema serialization fails.
pub fn export_parquet<W: Write + Send>(dataset: &Dataset, writer: W) -> Result<()> {
    let has_known_at = dataset.bars.iter().any(|b| b.known_at_ns != b.ts_ns);

    let mut fields = vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())), false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("open", DataType::Decimal128(18, 8), false),
        Field::new("high", DataType::Decimal128(18, 8), false),
        Field::new("low", DataType::Decimal128(18, 8), false),
        Field::new("close", DataType::Decimal128(18, 8), false),
        Field::new("volume", DataType::Decimal128(18, 8), false),
    ];
    if has_known_at {
        fields.push(Field::new(
            "known_at",
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
            false,
        ));
    }

    let schema = Arc::new(Schema::new(fields));
    let props = WriterProperties::builder().build();
    let mut arrow_writer = ArrowWriter::try_new(writer, Arc::clone(&schema), Some(props))
        .map_err(|e| Error::Data(format!("cannot create Parquet writer: {e}")))?;

    let symbol_names = &dataset.symbols.symbols;

    for chunk in dataset.bars.chunks(CHUNK_SIZE) {
        let mut ts_vec = Vec::with_capacity(chunk.len());
        let mut sym_vec = Vec::with_capacity(chunk.len());
        let mut open_vec = Vec::with_capacity(chunk.len());
        let mut high_vec = Vec::with_capacity(chunk.len());
        let mut low_vec = Vec::with_capacity(chunk.len());
        let mut close_vec = Vec::with_capacity(chunk.len());
        let mut vol_vec = Vec::with_capacity(chunk.len());
        let mut kat_vec = if has_known_at { Vec::with_capacity(chunk.len()) } else { Vec::new() };

        for b in chunk {
            ts_vec.push(b.ts_ns);
            let sym = symbol_names.get(b.instrument as usize).map_or("", |s| s.as_str());
            sym_vec.push(sym);
            open_vec.push(i128::from(b.open.raw()));
            high_vec.push(i128::from(b.high.raw()));
            low_vec.push(i128::from(b.low.raw()));
            close_vec.push(i128::from(b.close.raw()));
            vol_vec.push(i128::from(b.volume.raw()));
            if has_known_at {
                kat_vec.push(b.known_at_ns);
            }
        }

        let ts_arr = Arc::new(TimestampNanosecondArray::from(ts_vec).with_timezone("UTC")) as ArrayRef;
        let sym_arr = Arc::new(StringArray::from(sym_vec)) as ArrayRef;
        let open_arr = Arc::new(
            Decimal128Array::from(open_vec)
                .with_precision_and_scale(18, 8)
                .map_err(|e| Error::Data(format!("invalid open decimal: {e}")))?,
        ) as ArrayRef;
        let high_arr = Arc::new(
            Decimal128Array::from(high_vec)
                .with_precision_and_scale(18, 8)
                .map_err(|e| Error::Data(format!("invalid high decimal: {e}")))?,
        ) as ArrayRef;
        let low_arr = Arc::new(
            Decimal128Array::from(low_vec)
                .with_precision_and_scale(18, 8)
                .map_err(|e| Error::Data(format!("invalid low decimal: {e}")))?,
        ) as ArrayRef;
        let close_arr = Arc::new(
            Decimal128Array::from(close_vec)
                .with_precision_and_scale(18, 8)
                .map_err(|e| Error::Data(format!("invalid close decimal: {e}")))?,
        ) as ArrayRef;
        let vol_arr = Arc::new(
            Decimal128Array::from(vol_vec)
                .with_precision_and_scale(18, 8)
                .map_err(|e| Error::Data(format!("invalid volume decimal: {e}")))?,
        ) as ArrayRef;

        let mut columns = vec![ts_arr, sym_arr, open_arr, high_arr, low_arr, close_arr, vol_arr];
        if has_known_at {
            let kat_arr = Arc::new(TimestampNanosecondArray::from(kat_vec).with_timezone("UTC")) as ArrayRef;
            columns.push(kat_arr);
        }

        let batch = RecordBatch::try_new(Arc::clone(&schema), columns)
            .map_err(|e| Error::Data(format!("cannot create RecordBatch: {e}")))?;
        arrow_writer
            .write(&batch)
            .map_err(|e| Error::Data(format!("cannot write Parquet record batch: {e}")))?;
    }

    arrow_writer.close().map_err(|e| Error::Data(format!("cannot finish Parquet writer: {e}")))?;

    Ok(())
}
