//! M37 Company Research Reference Tests.

use spectra_research::company_research::*;

#[test]
fn test_m37_accounting_tie_out_and_ratios() {
    let stmt = FinancialStatementItems {
        period: "2024-Q1".into(),
        fiscal_year: 2024,
        revenue: 50_000_000.0,
        cost_of_goods_sold: 25_000_000.0,
        gross_profit: 25_000_000.0,
        operating_expenses: 10_000_000.0,
        operating_income: 15_000_000.0,
        interest_expense: 1_000_000.0,
        net_income: 11_000_000.0,
        diluted_shares_outstanding: 5_000_000.0,
        cash_and_equivalents: 15_000_000.0,
        current_assets: 30_000_000.0,
        inventory: 5_000_000.0,
        total_assets: 80_000_000.0,
        current_liabilities: 20_000_000.0,
        total_debt: 20_000_000.0,
        total_liabilities: 30_000_000.0,
        stockholders_equity: 50_000_000.0, // 30M + 50M = 80M
        operating_cash_flow: 14_000_000.0,
        capital_expenditures: 4_000_000.0,
        free_cash_flow: 10_000_000.0,
    };

    let tie_out = stmt.verify_tie_out(0.01);
    assert!(tie_out.is_balanced, "Balance sheet must tie out");

    let share_price = 44.0; // Mcap = 220M
    let ratios = compute_ratios(&stmt, share_price);

    // EPS = 11M / 5M = 2.2 -> PE = 44 / 2.2 = 20.0
    assert_eq!(ratios.pe_ratio, Some(20.0));
    // Gross Margin = 25M / 50M = 50%
    assert_eq!(ratios.gross_margin, Some(0.50));
    // Operating Margin = 15M / 50M = 30%
    assert_eq!(ratios.operating_margin, Some(0.30));
    // Net Margin = 11M / 50M = 22%
    assert_eq!(ratios.net_margin, Some(0.22));
    // Quick Ratio = (30M - 5M) / 20M = 1.25
    assert_eq!(ratios.quick_ratio, Some(1.25));
    // Current Ratio = 30M / 20M = 1.50
    assert_eq!(ratios.current_ratio, Some(1.50));
    // Interest coverage = 15M / 1M = 15.0
    assert_eq!(ratios.interest_coverage, Some(15.0));
}

#[test]
fn test_m37_valuation_percentile_hand_calculation() {
    let historical_pes = vec![12.0, 15.0, 18.0, 22.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0];
    // Hand calculation: 25.0 is the 5th value out of 10 -> exactly 50th percentile (5 / 10 = 50%)
    let p50 = multiple_percentile(&historical_pes, 25.0).unwrap();
    assert_eq!(p50, 50.0);

    // 15.0 is the 2nd value out of 10 -> 20th percentile
    let p20 = multiple_percentile(&historical_pes, 15.0).unwrap();
    assert_eq!(p20, 20.0);
}

#[test]
fn test_m37_health_scorecard_rules_signoff() {
    let stmt = FinancialStatementItems {
        period: "2024-Q1".into(),
        fiscal_year: 2024,
        revenue: 10_000_000.0,
        cost_of_goods_sold: 5_000_000.0,
        gross_profit: 5_000_000.0,
        operating_expenses: 2_000_000.0,
        operating_income: 3_000_000.0,
        interest_expense: 100_000.0,
        net_income: 2_500_000.0,
        diluted_shares_outstanding: 1_000_000.0,
        cash_and_equivalents: 4_000_000.0,
        current_assets: 6_000_000.0,
        inventory: 1_000_000.0,
        total_assets: 20_000_000.0,
        current_liabilities: 3_000_000.0,
        total_debt: 2_000_000.0,
        total_liabilities: 5_000_000.0,
        stockholders_equity: 15_000_000.0,
        operating_cash_flow: 3_200_000.0,
        capital_expenditures: 500_000.0,
        free_cash_flow: 2_700_000.0,
    };

    let scorecard = evaluate_health_scorecard(&stmt, None);
    assert!(scorecard.score >= 4);
    // Ensure all checks contain explanatory descriptions without buy/sell advice
    for check in &scorecard.checks {
        assert!(!check.name.to_lowercase().contains("buy"));
        assert!(!check.name.to_lowercase().contains("sell"));
        assert!(!check.description.to_lowercase().contains("buy"));
        assert!(!check.description.to_lowercase().contains("sell"));
    }
}
