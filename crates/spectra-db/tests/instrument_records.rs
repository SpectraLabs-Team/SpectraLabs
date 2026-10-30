//! M24.1: instrument records round-trip through Postgres, and the table refuses
//! a row that breaks its type's rules. Needs `DATABASE_URL` (see db.rs).

use spectra_core::{Instrument, InstrumentType, Price, Qty, Ratio, SCALE};
use spectra_db::instruments::{create_instrument, instrument_types, list_instruments};
use spectra_db::{connect, migrate, PoolConfig};
use uuid::Uuid;

async fn pool() -> Option<spectra_db::PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

async fn workspace(pool: &spectra_db::PgPool) -> Uuid {
    // A fresh workspace per test keeps symbols from colliding across runs.
    let seeded = spectra_db::seed_dev(pool, &format!("instr-{}@example.com", Uuid::now_v7())).await.unwrap();
    seeded.workspace_id
}

#[tokio::test]
async fn future_round_trips_exactly() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    // Unique per run: the dev workspace is shared between runs.
    let symbol = format!("ESZ6-{}", Uuid::now_v7().simple());
    let mut fut = Instrument::equity(symbol.clone(), "USD", Price::from_raw(SCALE / 4), Qty::from_raw(SCALE));
    fut.kind = InstrumentType::Future;
    fut.multiplier = Ratio::from_units(50).unwrap();
    fut.underlying = Some("ES".into());
    // A nanosecond-precision expiry: whole microseconds survive, the sub-microsecond part is dropped by Postgres.
    fut.expiry_ns = Some(1_798_675_200_000_000_000);
    create_instrument(&pool, ws, &fut).await.unwrap();

    let rows = list_instruments(&pool, ws).await.unwrap();
    let row = rows.iter().find(|r| r.instrument.symbol == symbol).expect("stored");
    assert_eq!(row.instrument, fut);
    let types = instrument_types(&pool, ws).await.unwrap();
    assert_eq!(types.get(&symbol), Some(&InstrumentType::Future));
}

#[tokio::test]
async fn refuses_rows_that_break_their_type() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    let mut fut = Instrument::equity(
        format!("ESH7-{}", Uuid::now_v7().simple()),
        "USD",
        Price::from_raw(1),
        Qty::from_raw(SCALE),
    );
    fut.kind = InstrumentType::Future;
    assert!(create_instrument(&pool, ws, &fut).await.is_err(), "a future without an expiry");
    // An equity cannot carry an underlying (the table checks it, not only the Rust code).
    let mut eq = Instrument::equity(
        format!("EQ-{}", Uuid::now_v7().simple()),
        "USD",
        Price::from_raw(1),
        Qty::from_raw(SCALE),
    );
    eq.underlying = Some("X".into());
    assert!(create_instrument(&pool, ws, &eq).await.is_err(), "an equity with an underlying");
}

#[tokio::test]
async fn bond_terms_round_trip_and_are_required() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    let symbol = format!("UST-{}", Uuid::now_v7().simple());
    let mut b = Instrument::equity(symbol.clone(), "USD", Price::from_raw(SCALE / 128), Qty::from_raw(SCALE));
    b.kind = InstrumentType::Bond;
    b.multiplier = Ratio::from_units(10).unwrap();
    b.maturity_ns = Some(1_893_456_000_000_000_000);
    b.coupon_rate = Some(Ratio::from_raw(SCALE / 20));
    b.coupon_frequency = Some(2);
    b.day_count = Some(spectra_core::DayCount::Thirty360);
    create_instrument(&pool, ws, &b).await.unwrap();
    let rows = list_instruments(&pool, ws).await.unwrap();
    assert_eq!(rows.iter().find(|r| r.instrument.symbol == symbol).unwrap().instrument, b);
    // The table refuses a bond without its coupon terms.
    let mut missing = b.clone();
    missing.symbol = format!("UST2-{}", Uuid::now_v7().simple());
    missing.day_count = None;
    assert!(create_instrument(&pool, ws, &missing).await.is_err());
}
