//! M40 Portfolios & Baskets Reference Tests.

use spectra_research::portfolio_basket::*;
use std::collections::HashMap;

#[test]
fn test_m40_portfolio_valuation_reconciliation() {
    let holdings = vec![
        HoldingPosition {
            symbol: "AAPL".into(),
            quantity: 100.0,
            cost_basis_per_share: 150.0, // cost = 15,000
            current_price: 180.0,        // value = 18,000
            is_paper: false,
        },
        HoldingPosition {
            symbol: "MSFT".into(),
            quantity: 50.0,
            cost_basis_per_share: 300.0, // cost = 15,000
            current_price: 320.0,        // value = 16,000
            is_paper: true,
        },
    ];

    let val = evaluate_portfolio(5_000.0, &holdings);
    assert_eq!(val.cash, 5_000.0);
    assert_eq!(val.equity_value, 34_000.0);
    assert_eq!(val.total_portfolio_value, 39_000.0);
    assert_eq!(val.total_cost_basis, 30_000.0);
    assert_eq!(val.total_unrealized_pnl, 4_000.0);
}

#[test]
fn test_m40_basket_validation_rules() {
    // 1. Valid basket
    let basket = Basket::new(
        "core_basket",
        "Core 70/30",
        vec![
            BasketItem { symbol: "SPY".into(), target_weight: 0.70 },
            BasketItem { symbol: "QQQ".into(), target_weight: 0.30 },
        ],
    );
    assert!(basket.validate().is_ok());

    // 2. Weights do not sum to 1.0 (70% + 20% = 90%)
    let invalid_sum = Basket::new(
        "bad_sum",
        "Bad",
        vec![
            BasketItem { symbol: "SPY".into(), target_weight: 0.70 },
            BasketItem { symbol: "QQQ".into(), target_weight: 0.20 },
        ],
    );
    assert!(matches!(invalid_sum.validate(), Err(BasketError::InvalidWeightSum { .. })));

    // 3. Duplicate symbol
    let duplicate = Basket::new(
        "dup",
        "Duplicate",
        vec![
            BasketItem { symbol: "SPY".into(), target_weight: 0.50 },
            BasketItem { symbol: "SPY".into(), target_weight: 0.50 },
        ],
    );
    assert!(matches!(duplicate.validate(), Err(BasketError::DuplicateSymbol(_))));
}

#[test]
fn test_m40_rebalancing_with_costs_and_drift() {
    let holdings = vec![
        HoldingPosition {
            symbol: "AAPL".into(),
            quantity: 80.0, // value = $8,000 (80%)
            cost_basis_per_share: 100.0,
            current_price: 100.0,
            is_paper: false,
        },
        HoldingPosition {
            symbol: "MSFT".into(),
            quantity: 20.0, // value = $2,000 (20%)
            cost_basis_per_share: 100.0,
            current_price: 100.0,
            is_paper: false,
        },
    ];

    // Target basket is 50/50 ($5,000 AAPL, $5,000 MSFT)
    let basket = Basket::new(
        "target_50_50",
        "50/50",
        vec![
            BasketItem { symbol: "AAPL".into(), target_weight: 0.50 },
            BasketItem { symbol: "MSFT".into(), target_weight: 0.50 },
        ],
    );

    let mut prices = HashMap::new();
    prices.insert("AAPL".into(), 100.0);
    prices.insert("MSFT".into(), 100.0);

    let plan = compute_rebalance_trades(
        0.0,
        &holdings,
        &basket,
        &prices,
        CostConfig { fixed_fee_per_order: 2.0, bps_slippage_and_comm: 0.001 },
        0.02,
    )
    .unwrap();

    assert_eq!(plan.trades.len(), 2);

    let aapl_trade = plan.trades.iter().find(|t| t.symbol == "AAPL").unwrap();
    assert_eq!(aapl_trade.side, OrderSide::Sell);
    assert_eq!(aapl_trade.shares, 30.0); // Sell 30 shares = $3,000 to drop from 8,000 to 5,000
    assert_eq!(aapl_trade.estimated_cost, 2.0 + 3000.0 * 0.001); // 2 + 3 = 5.0

    let msft_trade = plan.trades.iter().find(|t| t.symbol == "MSFT").unwrap();
    assert_eq!(msft_trade.side, OrderSide::Buy);
    assert_eq!(msft_trade.shares, 30.0); // Buy 30 shares = $3,000 to raise from 2,000 to 5,000
}
