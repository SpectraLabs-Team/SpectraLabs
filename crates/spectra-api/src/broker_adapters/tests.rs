//! Comprehensive test suite for Multi-Venue Broker Adapters (M24-advanced Tasks 1 to 18).

use std::collections::HashMap;

use spectra_core::{InstrumentType, OptionRight};
use spectra_risk::enforcement::{OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, RiskLimits};

use super::binance::{compute_funding_payment, evaluate_binance_quarterly_settlement, BinanceUsdmAdapter};
use super::ibkr::check_futures_roll_due;
use super::kite::compute_indian_fo_taxes;
use super::options::{
    compute_option_greeks, evaluate_expiry_settlement, venue_options_support, SettlementStyle,
};
use super::symbols::{
    map_binance_symbol_to_instrument, map_cme_symbol_to_instrument, map_eurex_symbol_to_instrument,
    map_nse_symbol_to_instrument, map_venue_symbol_to_instrument, VenueId,
};
use super::{
    evaluate_derivatives_risk, reconcile_positions_and_fills, verify_execution_mode_allowed, BrokerError,
    ExecutionMode, MarginRequirement, VenuePosition,
};

#[test]
fn test_cme_symbol_mapping() {
    // ESZ26: E-mini S&P 500, Dec 2026
    let es = map_cme_symbol_to_instrument("ESZ26").unwrap();
    assert_eq!(es.symbol, "ESZ26");
    assert_eq!(es.kind, InstrumentType::Future);
    assert_eq!(es.currency, "USD");
    assert_eq!(es.multiplier.to_f64(), 50.0);
    assert_eq!(es.tick_size.to_f64(), 0.25);
    assert!(es.expiry_ns.is_some());

    // NQM27: E-mini Nasdaq 100, June 2027
    let nq = map_cme_symbol_to_instrument("NQM27").unwrap();
    assert_eq!(nq.multiplier.to_f64(), 20.0);
    assert_eq!(nq.tick_size.to_f64(), 0.25);

    // Micro E-mini MES
    let mes = map_cme_symbol_to_instrument("MESU26").unwrap();
    assert_eq!(mes.multiplier.to_f64(), 5.0);

    // Crude Oil CL
    let cl = map_cme_symbol_to_instrument("CLF27").unwrap();
    assert_eq!(cl.multiplier.to_f64(), 1000.0);
    assert_eq!(cl.tick_size.to_f64(), 0.01);
}

#[test]
fn test_eurex_symbol_mapping() {
    // Euro-Bund Future
    let bund = map_eurex_symbol_to_instrument("FGBLZ26").unwrap();
    assert_eq!(bund.symbol, "FGBLZ26");
    assert_eq!(bund.currency, "EUR");
    assert_eq!(bund.multiplier.to_f64(), 1000.0);
    assert_eq!(bund.tick_size.to_f64(), 0.01);

    // Euro Stoxx 50
    let fesx = map_eurex_symbol_to_instrument("FESXZ26").unwrap();
    assert_eq!(fesx.currency, "EUR");
    assert_eq!(fesx.multiplier.to_f64(), 10.0);
    assert_eq!(fesx.tick_size.to_f64(), 1.0);
}

#[test]
fn test_nse_symbol_mapping() {
    // Nifty index future
    let nifty = map_nse_symbol_to_instrument("NIFTY26NOVFUT").unwrap();
    assert_eq!(nifty.currency, "INR");
    assert_eq!(nifty.kind, InstrumentType::Future);
    assert_eq!(nifty.lot_size.to_f64(), 50.0);
    assert_eq!(nifty.tick_size.to_f64(), 0.05);

    // Bank Nifty option
    let opt = map_nse_symbol_to_instrument("BANKNIFTY26D1052000CE").unwrap();
    assert_eq!(opt.currency, "INR");
    assert_eq!(opt.kind, InstrumentType::Option);
    assert_eq!(opt.option_right, Some(OptionRight::Call));
    assert_eq!(opt.lot_size.to_f64(), 15.0);
    assert_eq!(opt.strike.unwrap().to_f64(), 52000.0);
}

#[test]
fn test_binance_symbol_mapping() {
    // Perpetual
    let perp = map_binance_symbol_to_instrument("BTCUSDT").unwrap();
    assert_eq!(perp.currency, "USDT");
    assert_eq!(perp.kind, InstrumentType::Future);
    assert!(perp.expiry_ns.is_none());
    assert_eq!(perp.multiplier.to_f64(), 1.0);
    assert_eq!(perp.tick_size.to_f64(), 0.10);

    // Dated quarterly
    let q = map_binance_symbol_to_instrument("BTCUSDT_261225").unwrap();
    assert!(q.expiry_ns.is_some());
    assert_eq!(q.underlying.as_deref(), Some("BTCUSDT"));
}

#[test]
fn test_unified_venue_symbol_dispatcher() {
    let ibkr_fut = map_venue_symbol_to_instrument(VenueId::InteractiveBrokers, "ESZ26").unwrap();
    assert_eq!(ibkr_fut.multiplier.to_f64(), 50.0);

    let kite_fut = map_venue_symbol_to_instrument(VenueId::ZerodhaKite, "NIFTY26NOVFUT").unwrap();
    assert_eq!(kite_fut.currency, "INR");

    let bin_perp = map_venue_symbol_to_instrument(VenueId::BinanceUsdm, "ETHUSDT").unwrap();
    assert_eq!(bin_perp.currency, "USDT");
}

#[test]
fn test_binance_hmac_signature() {
    let adapter = BinanceUsdmAdapter::new("test_api_key".to_string(), "test_secret_key".to_string(), true);
    let query = "symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=1&price=90000&timestamp=1700000000000";
    let sig = adapter.sign_query(query);
    assert_eq!(sig.len(), 64); // 256 bits = 64 hex characters
}

#[test]
fn test_binance_funding_payments() {
    // Long 10 BTC at 90,000 mark price with positive 0.01% funding rate -> pay 90 USDT
    let fp = compute_funding_payment("BTCUSDT", 10.0, 90_000.0, 0.0001, 1_700_000_000_000_000_000);
    assert_eq!(fp.payment_amount, -90.0);

    // Short 5 BTC with positive funding rate -> receive 45 USDT
    let fp_short = compute_funding_payment("BTCUSDT", -5.0, 90_000.0, 0.0001, 1_700_000_000_000_000_000);
    assert_eq!(fp_short.payment_amount, 45.0);
}

#[test]
fn test_binance_quarterly_settlement() {
    // Bought 2 BTC at 80,000, settles at 85,000 -> PnL +10,000 USDT
    let pnl = evaluate_binance_quarterly_settlement("BTCUSDT_261225", 2.0, 80_000.0, 85_000.0).unwrap();
    assert_eq!(pnl, 10_000.0);

    // Perpetual should fail settlement check
    assert!(evaluate_binance_quarterly_settlement("BTCUSDT", 2.0, 80_000.0, 85_000.0).is_err());
}

#[test]
fn test_indian_regulatory_taxes() {
    // Buy 50 NIFTY Futures at 24,000 (Turnover = 1,200,000 INR)
    let buy_taxes = compute_indian_fo_taxes(false, true, 50.0, 24_000.0);
    assert_eq!(buy_taxes.stt, 0.0); // STT is 0 on futures buy
    assert!(buy_taxes.stamp_duty > 0.0); // Stamp duty is paid on buy
    assert!(buy_taxes.gst > 0.0);

    // Sell 50 NIFTY Futures at 24,000 (STT = 0.02% of turnover = 240 INR)
    let sell_taxes = compute_indian_fo_taxes(false, false, 50.0, 24_000.0);
    assert_eq!(sell_taxes.stt, 240.0);
    assert_eq!(sell_taxes.stamp_duty, 0.0); // No stamp duty on sell
}

#[test]
fn test_options_venue_support() {
    assert!(venue_options_support(VenueId::InteractiveBrokers).supports_options);
    assert!(venue_options_support(VenueId::ZerodhaKite).supports_options);
    assert!(!venue_options_support(VenueId::BinanceUsdm).supports_options);
}

#[test]
fn test_options_greeks_and_settlement() {
    // 1-month ATM call option: spot 100, strike 100, vol 0.20, rate 0.05
    let greeks =
        compute_option_greeks(OptionRight::Call, 100.0, 100.0, 1.0 / 12.0, 0.05, 0.0, 0.20, 10.0, 100.0)
            .unwrap();

    assert!(greeks.delta > 0.45 && greeks.delta < 0.60);
    assert!(greeks.gamma > 0.0);
    assert!(greeks.vega > 0.0);
    assert!(greeks.theta < 0.0);
    assert_eq!(greeks.position_delta, greeks.delta * 10.0 * 100.0);

    // Cash settlement at expiry when spot = 110 (ITM by 10)
    let settlement = evaluate_expiry_settlement(
        "SPY_CALL",
        OptionRight::Call,
        100.0,
        110.0,
        5.0,
        100.0,
        SettlementStyle::CashSettled,
    );
    assert!(settlement.is_in_the_money);
    assert_eq!(settlement.total_cash_settlement, 5.0 * 10.0 * 100.0);
    assert!(!settlement.delivery_required);
}

#[test]
fn test_execution_mode_gating() {
    assert!(verify_execution_mode_allowed(ExecutionMode::Paper, false).is_ok());
    assert!(verify_execution_mode_allowed(ExecutionMode::Live, true).is_ok());

    // Live order on paper mode must fail closed
    let err = verify_execution_mode_allowed(ExecutionMode::Paper, true).unwrap_err();
    assert!(matches!(err, BrokerError::LiveTradingDisabled(_)));
}

#[test]
fn test_derivatives_risk_checks() {
    let limits = RiskLimits { max_leverage: 3.0, max_position_size: 200_000.0, ..RiskLimits::default() };

    let portfolio = PortfolioSnapshot {
        cash: 50_000.0,
        equity: 100_000.0,
        peak_equity: 100_000.0,
        daily_start_equity: 100_000.0,
        positions: HashMap::new(),
    };

    let candidate = OrderCandidate {
        symbol: "ESZ26".to_string(),
        side: OrderSide::Buy,
        qty: 1.0,
        price: 5_000.0 * 50.0, // Notional 250,000
    };

    // Case 1: Initial margin exceeds available cash
    let excessive_margin = MarginRequirement {
        initial_margin: 60_000.0, // Available cash is only 50k
        maintenance_margin: 45_000.0,
        margin_rate: 0.24,
    };
    let res = evaluate_derivatives_risk(&limits, &portfolio, &candidate, &excessive_margin);
    assert!(matches!(res, Err(BrokerError::MarginExceeded(_))));

    // Case 2: Margin ok, but position size exceeds limit (250k > 200k)
    let ok_margin =
        MarginRequirement { initial_margin: 25_000.0, maintenance_margin: 20_000.0, margin_rate: 0.10 };
    let res = evaluate_derivatives_risk(&limits, &portfolio, &candidate, &ok_margin);
    assert!(matches!(res, Err(BrokerError::PositionLimitExceeded(_))));
}

#[test]
fn test_futures_roll_rule() {
    let now = 1_700_000_000_000_000_000i64;
    // Expiry in 4 days (should roll if rule is 5 days)
    let expiry_near = now + (4 * 24 * 3600 * 1_000_000_000);
    assert!(check_futures_roll_due(now, expiry_near, 5));

    // Expiry in 10 days (should NOT roll yet)
    let expiry_far = now + (10 * 24 * 3600 * 1_000_000_000);
    assert!(!check_futures_roll_due(now, expiry_far, 5));
}

#[test]
fn test_paper_position_reconciliation() {
    let mut local = HashMap::new();
    local.insert("ESZ26".to_string(), PositionSnapshot { qty: 2.0, price: 5900.0 });
    local.insert("BTCUSDT".to_string(), PositionSnapshot { qty: 1.5, price: 90000.0 });

    let venue = vec![
        VenuePosition {
            symbol: "ESZ26".to_string(),
            qty: 2.0, // Matched
            entry_price: 5900.0,
            mark_price: 5905.0,
            unrealized_pnl: 500.0,
            initial_margin: 25000.0,
            maintenance_margin: 20000.0,
        },
        VenuePosition {
            symbol: "BTCUSDT".to_string(),
            qty: 1.0, // Mismatched (local 1.5 vs venue 1.0)
            entry_price: 90000.0,
            mark_price: 91000.0,
            unrealized_pnl: 1000.0,
            initial_margin: 9000.0,
            maintenance_margin: 7000.0,
        },
    ];

    let report = reconcile_positions_and_fills(&local, &venue, 150_000.0, 149_500.0);
    assert_eq!(report.matched_positions, 1);
    assert_eq!(report.discrepancies.len(), 1);
    assert_eq!(report.discrepancies[0].symbol, "BTCUSDT");
    assert!((report.discrepancies[0].qty_diff - 0.5).abs() < 1e-6);
}
