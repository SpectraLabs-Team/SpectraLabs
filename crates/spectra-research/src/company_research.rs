//! Company Research & Financial Ratios Engine (M37).
//!
//! Computes valuation, profitability, leverage, and liquidity ratios from
//! balance sheet, income statement, and cash flow statement line items.
//! Provides statement tie-out validation, historical multiple percentiles,
//! and objective rule-based financial health scorecards without buy/sell advice.

/// Fundamental financial statement items for a reporting period.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FinancialStatementItems {
    pub period: String,
    pub fiscal_year: u16,
    // Income Statement
    pub revenue: f64,
    pub cost_of_goods_sold: f64,
    pub gross_profit: f64,
    pub operating_expenses: f64,
    pub operating_income: f64,
    pub interest_expense: f64,
    pub net_income: f64,
    pub diluted_shares_outstanding: f64,
    // Balance Sheet
    pub cash_and_equivalents: f64,
    pub current_assets: f64,
    pub inventory: f64,
    pub total_assets: f64,
    pub current_liabilities: f64,
    pub total_debt: f64,
    pub total_liabilities: f64,
    pub stockholders_equity: f64,
    // Cash Flow
    pub operating_cash_flow: f64,
    pub capital_expenditures: f64,
    pub free_cash_flow: f64,
}

/// Balance sheet tie-out validation result.
#[derive(Debug, Clone, PartialEq)]
pub struct StatementTieOut {
    pub is_balanced: bool,
    pub assets_minus_liabilities_and_equity: f64,
    pub tolerance: f64,
}

impl FinancialStatementItems {
    /// Validates accounting balance sheet identity: Assets == Liabilities + Equity.
    pub fn verify_tie_out(&self, tolerance: f64) -> StatementTieOut {
        let diff = (self.total_assets - (self.total_liabilities + self.stockholders_equity)).abs();
        StatementTieOut {
            is_balanced: diff <= tolerance,
            assets_minus_liabilities_and_equity: diff,
            tolerance,
        }
    }
}

/// Key financial ratios computed for a company.
#[derive(Debug, Clone, PartialEq)]
pub struct CompanyRatios {
    // Valuation Ratios
    pub pe_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
    pub ps_ratio: Option<f64>,
    pub ev_to_ebitda: Option<f64>,
    pub price_to_fcf: Option<f64>,
    // Profitability Ratios
    pub gross_margin: Option<f64>,
    pub operating_margin: Option<f64>,
    pub net_margin: Option<f64>,
    pub return_on_equity: Option<f64>,
    pub return_on_assets: Option<f64>,
    // Leverage & Solvency Ratios
    pub debt_to_equity: Option<f64>,
    pub debt_to_assets: Option<f64>,
    pub interest_coverage: Option<f64>,
    // Liquidity Ratios
    pub current_ratio: Option<f64>,
    pub quick_ratio: Option<f64>,
    pub cash_ratio: Option<f64>,
}

/// Computes financial ratios given financial statement items and market quote.
pub fn compute_ratios(stmt: &FinancialStatementItems, share_price: f64) -> CompanyRatios {
    let mcap = stmt.diluted_shares_outstanding * share_price;
    let eps = if stmt.diluted_shares_outstanding > 0.0 {
        stmt.net_income / stmt.diluted_shares_outstanding
    } else {
        0.0
    };

    let pe_ratio = if eps > 0.0 { Some(share_price / eps) } else { None };

    let book_value_per_share = if stmt.diluted_shares_outstanding > 0.0 {
        stmt.stockholders_equity / stmt.diluted_shares_outstanding
    } else {
        0.0
    };
    let pb_ratio = if book_value_per_share > 0.0 { Some(share_price / book_value_per_share) } else { None };

    let ps_ratio = if stmt.revenue > 0.0 { Some(mcap / stmt.revenue) } else { None };

    // EV = Market Cap + Total Debt - Cash
    let enterprise_value = mcap + stmt.total_debt - stmt.cash_and_equivalents;
    // EBITDA approx = Operating Income + D&A (or operating income baseline)
    let ev_to_ebitda =
        if stmt.operating_income > 0.0 { Some(enterprise_value / stmt.operating_income) } else { None };

    let price_to_fcf = if stmt.free_cash_flow > 0.0 { Some(mcap / stmt.free_cash_flow) } else { None };

    // Profitability
    let gross_margin = if stmt.revenue > 0.0 { Some(stmt.gross_profit / stmt.revenue) } else { None };
    let operating_margin = if stmt.revenue > 0.0 { Some(stmt.operating_income / stmt.revenue) } else { None };
    let net_margin = if stmt.revenue > 0.0 { Some(stmt.net_income / stmt.revenue) } else { None };
    let return_on_equity =
        if stmt.stockholders_equity > 0.0 { Some(stmt.net_income / stmt.stockholders_equity) } else { None };
    let return_on_assets =
        if stmt.total_assets > 0.0 { Some(stmt.net_income / stmt.total_assets) } else { None };

    // Leverage
    let debt_to_equity =
        if stmt.stockholders_equity > 0.0 { Some(stmt.total_debt / stmt.stockholders_equity) } else { None };
    let debt_to_assets =
        if stmt.total_assets > 0.0 { Some(stmt.total_debt / stmt.total_assets) } else { None };
    let interest_coverage =
        if stmt.interest_expense > 0.0 { Some(stmt.operating_income / stmt.interest_expense) } else { None };

    // Liquidity
    let current_ratio = if stmt.current_liabilities > 0.0 {
        Some(stmt.current_assets / stmt.current_liabilities)
    } else {
        None
    };
    let quick_ratio = if stmt.current_liabilities > 0.0 {
        Some((stmt.current_assets - stmt.inventory) / stmt.current_liabilities)
    } else {
        None
    };
    let cash_ratio = if stmt.current_liabilities > 0.0 {
        Some(stmt.cash_and_equivalents / stmt.current_liabilities)
    } else {
        None
    };

    CompanyRatios {
        pe_ratio,
        pb_ratio,
        ps_ratio,
        ev_to_ebitda,
        price_to_fcf,
        gross_margin,
        operating_margin,
        net_margin,
        return_on_equity,
        return_on_assets,
        debt_to_equity,
        debt_to_assets,
        interest_coverage,
        current_ratio,
        quick_ratio,
        cash_ratio,
    }
}

/// Calculate the exact empirical percentile of a current multiple among historical observations.
///
/// Formula: `100.0 * (count of values <= current) / total_count`.
pub fn multiple_percentile(history: &[f64], current: f64) -> Option<f64> {
    if history.is_empty() {
        return None;
    }
    let count = history.iter().filter(|&&v| v <= current).count();
    Some(100.0 * count as f64 / history.len() as f64)
}

/// Financial Health Scorecard test criteria item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthScoreCheck {
    pub name: String,
    pub description: String,
    pub passed: bool,
}

/// Objective Financial Health Scorecard (no buy/sell advice; factual scoring only).
#[derive(Debug, Clone, PartialEq)]
pub struct FinancialHealthScorecard {
    pub score: u32,
    pub max_score: u32,
    pub checks: Vec<HealthScoreCheck>,
}

/// Evaluates objective rule-based financial health scorecard for a reporting company.
pub fn evaluate_health_scorecard(
    current: &FinancialStatementItems,
    prior: Option<&FinancialStatementItems>,
) -> FinancialHealthScorecard {
    let mut checks = Vec::new();
    let mut score = 0;

    // 1. Positive Net Income
    let pos_ni = current.net_income > 0.0;
    if pos_ni {
        score += 1;
    }
    checks.push(HealthScoreCheck {
        name: "Positive Net Income".into(),
        description: "Company generates positive net profit in the period".into(),
        passed: pos_ni,
    });

    // 2. Positive Operating Cash Flow
    let pos_cfo = current.operating_cash_flow > 0.0;
    if pos_cfo {
        score += 1;
    }
    checks.push(HealthScoreCheck {
        name: "Positive Operating Cash Flow".into(),
        description: "Core operations generate cash inflows".into(),
        passed: pos_cfo,
    });

    // 3. Cash Flow exceeds Net Income (Earnings Quality)
    let cfo_gt_ni = current.operating_cash_flow > current.net_income;
    if cfo_gt_ni {
        score += 1;
    }
    checks.push(HealthScoreCheck {
        name: "Cash Flow Exceeds Net Income".into(),
        description: "Operating cash flow is higher than reported net profit, indicating low accruals".into(),
        passed: cfo_gt_ni,
    });

    // 4. Solvency: Prudent Debt to Equity (< 1.5)
    let prudent_debt =
        current.stockholders_equity > 0.0 && (current.total_debt / current.stockholders_equity) < 1.5;
    if prudent_debt {
        score += 1;
    }
    checks.push(HealthScoreCheck {
        name: "Prudent Leverage (Debt/Equity < 1.5)".into(),
        description: "Total liabilities and debt are covered by equity cushion".into(),
        passed: prudent_debt,
    });

    // 5. Liquidity: Current Ratio >= 1.0
    let liquid =
        current.current_liabilities > 0.0 && (current.current_assets / current.current_liabilities) >= 1.0;
    if liquid {
        score += 1;
    }
    checks.push(HealthScoreCheck {
        name: "Adequate Working Capital (Current Ratio >= 1.0)".into(),
        description: "Short-term assets cover short-term liabilities".into(),
        passed: liquid,
    });

    // 6. YoY Margin Expansion (if prior statement available)
    if let Some(p) = prior {
        let margin_expansion = if current.revenue > 0.0 && p.revenue > 0.0 {
            (current.gross_profit / current.revenue) >= (p.gross_profit / p.revenue)
        } else {
            false
        };
        if margin_expansion {
            score += 1;
        }
        checks.push(HealthScoreCheck {
            name: "Gross Margin Stability/Expansion".into(),
            description: "Gross margin equal to or higher than prior period".into(),
            passed: margin_expansion,
        });
    }

    let max_score = checks.len() as u32;
    FinancialHealthScorecard { score, max_score, checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_statement() -> FinancialStatementItems {
        FinancialStatementItems {
            period: "2024-FY".into(),
            fiscal_year: 2024,
            revenue: 100_000_000.0,
            cost_of_goods_sold: 60_000_000.0,
            gross_profit: 40_000_000.0,
            operating_expenses: 15_000_000.0,
            operating_income: 25_000_000.0,
            interest_expense: 2_000_000.0,
            net_income: 18_000_000.0,
            diluted_shares_outstanding: 10_000_000.0,
            cash_and_equivalents: 20_000_000.0,
            current_assets: 50_000_000.0,
            inventory: 10_000_000.0,
            total_assets: 150_000_000.0,
            current_liabilities: 30_000_000.0,
            total_debt: 40_000_000.0,
            total_liabilities: 60_000_000.0,
            stockholders_equity: 90_000_000.0, // 60M liabilities + 90M equity = 150M total assets
            operating_cash_flow: 22_000_000.0,
            capital_expenditures: 5_000_000.0,
            free_cash_flow: 17_000_000.0,
        }
    }

    #[test]
    fn test_statement_tie_out() {
        let stmt = sample_statement();
        let tie_out = stmt.verify_tie_out(1.0);
        assert!(tie_out.is_balanced);
        assert_eq!(tie_out.assets_minus_liabilities_and_equity, 0.0);
    }

    #[test]
    fn test_ratio_engine_calculations() {
        let stmt = sample_statement();
        let share_price = 36.0; // Market cap = 360M
        let r = compute_ratios(&stmt, share_price);

        // EPS = 18M / 10M = 1.8 -> PE = 36 / 1.8 = 20.0
        assert_eq!(r.pe_ratio, Some(20.0));
        // Gross margin = 40M / 100M = 40%
        assert_eq!(r.gross_margin, Some(0.40));
        // Net margin = 18M / 100M = 18%
        assert_eq!(r.net_margin, Some(0.18));
        // Current ratio = 50M / 30M = 1.6666...
        assert!((r.current_ratio.unwrap() - (50.0 / 30.0)).abs() < 1e-10);
        // Debt to equity = 40M / 90M = 0.4444...
        assert!((r.debt_to_equity.unwrap() - (40.0 / 90.0)).abs() < 1e-10);
    }

    #[test]
    fn test_multiple_percentile() {
        let history = vec![10.0, 15.0, 20.0, 25.0, 30.0];
        // 20.0 is the 3rd out of 5 values (<= 20.0: 10, 15, 20) -> 3/5 = 60.0%
        assert_eq!(multiple_percentile(&history, 20.0), Some(60.0));
        assert_eq!(multiple_percentile(&history, 5.0), Some(0.0));
        assert_eq!(multiple_percentile(&history, 35.0), Some(100.0));
    }

    #[test]
    fn test_health_scorecard_rules() {
        let stmt = sample_statement();
        let card = evaluate_health_scorecard(&stmt, None);
        assert_eq!(card.score, 5); // passes all 5 criteria
        assert_eq!(card.max_score, 5);
        for check in &card.checks {
            assert!(check.passed);
        }
    }
}
