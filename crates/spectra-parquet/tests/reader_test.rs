use std::sync::Arc;

use arrow_array::{Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use spectra_data::{load_csv, CsvOptions};
use spectra_parquet::{load_parquet, preview_parquet, report_parquet};

fn make_parquet_bytes(batch: &RecordBatch) -> Vec<u8> {
    let mut buf = Vec::new();
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props)).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
    buf
}

#[test]
fn test_parquet_csv_identity_parity() {
    let csv_content = b"timestamp,open,high,low,close,volume,symbol\n\
2024-01-02 09:30:00Z,185.50,186.00,185.00,185.75,10000,AAPL\n\
2024-01-02 09:31:00Z,185.75,186.25,185.50,186.00,15000,AAPL\n\
2024-01-02 09:30:00Z,375.00,376.50,374.50,376.00,8000,MSFT\n";

    let opt = CsvOptions::default();
    let csv_ds = load_csv(csv_content, &opt).unwrap();

    // Create equivalent Parquet batch
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("open", DataType::Utf8, false),
        Field::new("high", DataType::Utf8, false),
        Field::new("low", DataType::Utf8, false),
        Field::new("close", DataType::Utf8, false),
        Field::new("volume", DataType::Int64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![
                "2024-01-02 09:30:00Z",
                "2024-01-02 09:31:00Z",
                "2024-01-02 09:30:00Z",
            ])),
            Arc::new(StringArray::from(vec!["185.50", "185.75", "375.00"])),
            Arc::new(StringArray::from(vec!["186.00", "186.25", "376.50"])),
            Arc::new(StringArray::from(vec!["185.00", "185.50", "374.50"])),
            Arc::new(StringArray::from(vec!["185.75", "186.00", "376.00"])),
            Arc::new(Int64Array::from(vec![10000, 15000, 8000])),
            Arc::new(StringArray::from(vec!["AAPL", "AAPL", "MSFT"])),
        ],
    )
    .unwrap();

    let pq_bytes = make_parquet_bytes(&batch);
    let pq_ds = load_parquet(bytes::Bytes::from(pq_bytes), &opt, None).unwrap();

    // Must have identical bars, bar count, bars_sha256 and dataset_id!
    assert_eq!(pq_ds.bars.len(), csv_ds.bars.len());
    assert_eq!(pq_ds.bars, csv_ds.bars);
    assert_eq!(pq_ds.symbols, csv_ds.symbols);
    assert_eq!(pq_ds.info.bars_sha256, csv_ds.info.bars_sha256);
    assert_eq!(pq_ds.info.id, csv_ds.info.id);
}

#[test]
fn test_parquet_rejects_floats() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Int64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec!["2024-01-02 09:30:00Z"])),
            Arc::new(arrow_array::Float64Array::from(vec![185.5])),
            Arc::new(arrow_array::Float64Array::from(vec![186.0])),
            Arc::new(arrow_array::Float64Array::from(vec![185.0])),
            Arc::new(arrow_array::Float64Array::from(vec![185.75])),
            Arc::new(Int64Array::from(vec![10000])),
            Arc::new(StringArray::from(vec!["AAPL"])),
        ],
    )
    .unwrap();

    let pq_bytes = make_parquet_bytes(&batch);
    let opt = CsvOptions::default();

    // Reject policy must fail with bad_number
    let err = load_parquet(bytes::Bytes::from(pq_bytes.clone()), &opt, None).unwrap_err();
    assert!(err.to_string().contains("[bad_number]"));
    assert!(err.to_string().contains("floating point numbers are not allowed"));

    // Skip policy counts it
    let rep_ds = report_parquet(bytes::Bytes::from(pq_bytes), &opt).unwrap();
    assert_eq!(rep_ds.quality.rows_rejected, 1);
    assert_eq!(*rep_ds.quality.rejected_by_category.get("bad_number").unwrap(), 1);
}

#[test]
fn test_parquet_rejects_more_than_8_decimals() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("open", DataType::Utf8, false),
        Field::new("high", DataType::Utf8, false),
        Field::new("low", DataType::Utf8, false),
        Field::new("close", DataType::Utf8, false),
        Field::new("volume", DataType::Int64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec!["2024-01-02 09:30:00Z"])),
            Arc::new(StringArray::from(vec!["185.123456789"])), // 9 decimal places!
            Arc::new(StringArray::from(vec!["186.00"])),
            Arc::new(StringArray::from(vec!["185.00"])),
            Arc::new(StringArray::from(vec!["185.75"])),
            Arc::new(Int64Array::from(vec![10000])),
            Arc::new(StringArray::from(vec!["AAPL"])),
        ],
    )
    .unwrap();

    let pq_bytes = make_parquet_bytes(&batch);
    let opt = CsvOptions::default();

    let err = load_parquet(bytes::Bytes::from(pq_bytes), &opt, None).unwrap_err();
    assert!(err.to_string().contains("[bad_number]"));
    assert!(err.to_string().contains("max 8 decimal places, no rounding"));
}

#[test]
fn test_parquet_point_in_time_known_at() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("known_at", DataType::Utf8, false),
        Field::new("open", DataType::Utf8, false),
        Field::new("high", DataType::Utf8, false),
        Field::new("low", DataType::Utf8, false),
        Field::new("close", DataType::Utf8, false),
        Field::new("volume", DataType::Int64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]));

    // Bar 1: valid known_at (after timestamp)
    // Bar 2: invalid known_at (before timestamp)
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec!["2024-01-02 09:30:00Z", "2024-01-02 09:31:00Z"])),
            Arc::new(StringArray::from(vec![
                "2024-01-02 09:35:00Z",
                "2024-01-02 09:30:00Z", // before 09:31!
            ])),
            Arc::new(StringArray::from(vec!["185.50", "185.75"])),
            Arc::new(StringArray::from(vec!["186.00", "186.25"])),
            Arc::new(StringArray::from(vec!["185.00", "185.50"])),
            Arc::new(StringArray::from(vec!["185.75", "186.00"])),
            Arc::new(Int64Array::from(vec![10000, 15000])),
            Arc::new(StringArray::from(vec!["AAPL", "AAPL"])),
        ],
    )
    .unwrap();

    let pq_bytes = make_parquet_bytes(&batch);
    let opt = CsvOptions { known_at_column: "known_at".to_string(), ..CsvOptions::default() };

    let err = load_parquet(bytes::Bytes::from(pq_bytes.clone()), &opt, None).unwrap_err();
    assert!(err.to_string().contains("[known_at_before_timestamp]"));

    let rep_ds = report_parquet(bytes::Bytes::from(pq_bytes), &opt).unwrap();
    assert_eq!(rep_ds.quality.rows_accepted, 1);
    assert_eq!(rep_ds.quality.rows_rejected, 1);
    assert_eq!(*rep_ds.quality.rejected_by_category.get("known_at_before_timestamp").unwrap(), 1);
}

#[test]
fn test_parquet_preview() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("open", DataType::Utf8, false),
        Field::new("high", DataType::Utf8, false),
        Field::new("low", DataType::Utf8, false),
        Field::new("close", DataType::Utf8, false),
        Field::new("volume", DataType::Int64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![
                "2024-01-02 09:30:00Z",
                "2024-01-02 09:31:00Z",
                "2024-01-02 09:32:00Z",
            ])),
            Arc::new(StringArray::from(vec!["185.50", "185.75", "186.00"])),
            Arc::new(StringArray::from(vec!["186.00", "186.25", "186.50"])),
            Arc::new(StringArray::from(vec!["185.00", "185.50", "185.75"])),
            Arc::new(StringArray::from(vec!["185.75", "186.00", "186.25"])),
            Arc::new(Int64Array::from(vec![10000, 15000, 12000])),
            Arc::new(StringArray::from(vec!["AAPL", "AAPL", "AAPL"])),
        ],
    )
    .unwrap();

    let pq_bytes = make_parquet_bytes(&batch);
    let opt = CsvOptions::default();

    let preview = preview_parquet(&pq_bytes, &opt, 2).unwrap();
    assert_eq!(preview.dataset.bars.len(), 2);
    assert_eq!(preview.rows_not_read, 1);
    assert!(preview.would_abort.is_none());
}

#[test]
fn test_parquet_export_roundtrip() {
    let csv_content = b"timestamp,open,high,low,close,volume,symbol\n\
2024-01-02 09:30:00Z,185.50,186.00,185.00,185.75,10000,AAPL\n\
2024-01-02 09:31:00Z,185.75,186.25,185.50,186.00,15000,AAPL\n\
2024-01-02 09:30:00Z,375.00,376.50,374.50,376.00,8000,MSFT\n";

    let opt = CsvOptions::default();
    let csv_ds = load_csv(csv_content, &opt).unwrap();

    let mut pq_buf = Vec::new();
    spectra_parquet::export_parquet(&csv_ds, &mut pq_buf).unwrap();

    let pq_ds = load_parquet(bytes::Bytes::from(pq_buf), &opt, None).unwrap();

    assert_eq!(csv_ds.info.id, pq_ds.info.id);
    assert_eq!(csv_ds.bars.len(), pq_ds.bars.len());
    for (b1, b2) in csv_ds.bars.iter().zip(pq_ds.bars.iter()) {
        assert_eq!(b1.ts_ns, b2.ts_ns);
        assert_eq!(b1.instrument, b2.instrument);
        assert_eq!(b1.open, b2.open);
        assert_eq!(b1.high, b2.high);
        assert_eq!(b1.low, b2.low);
        assert_eq!(b1.close, b2.close);
        assert_eq!(b1.volume, b2.volume);
        assert_eq!(b1.known_at_ns, b2.known_at_ns);
    }
    assert_eq!(csv_ds.symbols.symbols, pq_ds.symbols.symbols);
}
