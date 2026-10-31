//! Zerodha Kite Connect v3 Adapter & Indian Market Regulatory Rules (M24-advanced Task 11, 12, 13).
//!
//! Note on Task 11:
//! IBKR India does NOT offer retail API order routing on NSE/BSE due to SEBI algorithmic trading
//! regulations requiring broker algorithmic approval and dedicated audit compliance. Zerodha Kite Connect
//! is the selected industry-standard broker adapter for Indian F&O.

use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use super::{
    BrokerError, CancelOrderResponse, OrderSide, VenueAccountSummary, VenueOrderRequest, VenueOrderResponse,
    VenuePosition,
};

pub const KITE_BASE_URL: &str = "https://api.kite.trade";

/// Indian regulatory taxes and transaction costs breakdown (Task 12).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IndianTaxBreakdown {
    /// Securities Transaction Tax (STT).
    pub stt: f64,
    /// Exchange turnover fees.
    pub exchange_turnover_charge: f64,
    /// SEBI regulatory turnover charges (₹10 / crore).
    pub sebi_turnover_charge: f64,
    /// State stamp duty.
    pub stamp_duty: f64,
    /// Goods and Services Tax (18% on exchange and regulatory charges).
    pub gst: f64,
    /// Total non-brokerage transaction costs in INR.
    pub total_regulatory_costs: f64,
}

/// Computes SEBI / Exchange taxes and statutory fees for an Indian F&O trade (Task 12).
#[must_use]
pub fn compute_indian_fo_taxes(is_option: bool, is_buy: bool, qty: f64, price: f64) -> IndianTaxBreakdown {
    let turnover = qty.abs() * price;

    // 1. STT (Securities Transaction Tax)
    // - Futures: 0.02% on sell turnover
    // - Options: 0.1% on sell premium turnover
    let stt = if is_buy {
        0.0
    } else if is_option {
        turnover * 0.001
    } else {
        turnover * 0.0002
    };

    // 2. Exchange turnover charge
    let exchange_turnover_charge = if is_option { turnover * 0.0005 } else { turnover * 0.000019 };

    // 3. SEBI turnover charge: ₹10 per crore (0.0001%)
    let sebi_turnover_charge = turnover * 0.000001;

    // 4. Stamp duty (applicable on BUY only)
    let stamp_duty = if is_buy {
        if is_option {
            turnover * 0.00003
        } else {
            turnover * 0.00002
        }
    } else {
        0.0
    };

    // 5. GST: 18% on (Exchange turnover charges + SEBI charges)
    let gst = (exchange_turnover_charge + sebi_turnover_charge) * 0.18;

    let total = stt + exchange_turnover_charge + sebi_turnover_charge + stamp_duty + gst;

    IndianTaxBreakdown {
        stt,
        exchange_turnover_charge,
        sebi_turnover_charge,
        stamp_duty,
        gst,
        total_regulatory_costs: total,
    }
}

/// Zerodha Kite Connect v3 REST API adapter (Task 13).
#[derive(Debug, Clone)]
pub struct ZerodhaKiteAdapter {
    pub base_url: String,
    pub api_key: String,
    pub access_token: String,
    pub client: reqwest::Client,
}

impl ZerodhaKiteAdapter {
    #[must_use]
    pub fn new(api_key: String, access_token: String) -> Self {
        Self { base_url: KITE_BASE_URL.to_string(), api_key, access_token, client: reqwest::Client::new() }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("token {}:{}", self.api_key, self.access_token)) {
            headers.insert("Authorization", val);
        }
        headers.insert("X-Kite-Version", HeaderValue::from_static("3"));
        headers
    }

    /// Submits a regular order via Kite Connect (`POST /orders/regular`).
    pub async fn submit_order(&self, req: &VenueOrderRequest) -> Result<VenueOrderResponse, BrokerError> {
        let url = format!("{}/orders/regular", self.base_url);
        let transaction_type = match req.side {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        };

        let mut form_parts = Vec::new();
        form_parts.push(format!("tradingsymbol={}", req.symbol));
        form_parts.push("exchange=NFO".to_string());
        form_parts.push(format!("transaction_type={transaction_type}"));
        form_parts.push(format!("order_type={}", if req.price.is_some() { "LIMIT" } else { "MARKET" }));
        form_parts.push(format!("quantity={}", req.qty as i64));
        form_parts.push("product=NRML".to_string());
        form_parts.push("validity=DAY".to_string());
        if let Some(price) = req.price {
            form_parts.push(format!("price={price}"));
        }
        let body_str = form_parts.join("&");

        let mut headers = self.headers();
        headers.insert("content-type", HeaderValue::from_static("application/x-www-form-urlencoded"));

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Kite network error: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| BrokerError::Network(format!("Failed to read Kite response: {e}")))?;

        if !status.is_success() {
            return Err(BrokerError::OrderRejected(format!("Kite order rejected (status {status}): {body}")));
        }

        let val: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid Kite JSON: {e}")))?;

        let order_id = val["data"]["order_id"].as_str().unwrap_or("").to_string();

        Ok(VenueOrderResponse {
            venue_order_id: order_id,
            client_order_id: req.client_order_id.clone().unwrap_or_default(),
            status: "SUBMITTED".to_string(),
            filled_qty: 0.0,
            avg_fill_price: req.price.unwrap_or(0.0),
            raw_response: val,
        })
    }

    /// Cancels an order via Kite Connect (`DELETE /orders/regular/{order_id}`).
    pub async fn cancel_order(&self, venue_order_id: &str) -> Result<CancelOrderResponse, BrokerError> {
        let url = format!("{}/orders/regular/{}", self.base_url, venue_order_id);
        let resp = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Kite network error: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(BrokerError::OrderRejected(format!("Failed to cancel Kite order: {body}")));
        }

        Ok(CancelOrderResponse {
            venue_order_id: venue_order_id.to_string(),
            status: "CANCELLED".to_string(),
        })
    }

    /// Fetches positions from Kite (`GET /portfolio/positions`).
    pub async fn get_positions(&self) -> Result<Vec<VenuePosition>, BrokerError> {
        let url = format!("{}/portfolio/positions", self.base_url);
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Kite network error: {e}")))?;

        let val: serde_json::Value =
            resp.json().await.map_err(|e| BrokerError::MalformedResponse(format!("Invalid JSON: {e}")))?;

        let net_arr = val["data"]["net"].as_array().ok_or_else(|| {
            BrokerError::MalformedResponse("Expected data.net array in Kite positions".to_string())
        })?;

        let mut positions = Vec::new();
        for p in net_arr {
            let sym = p["tradingsymbol"].as_str().unwrap_or("").to_string();
            let qty = p["quantity"].as_f64().unwrap_or(0.0);
            if qty.abs() < 1e-8 {
                continue;
            }
            let avg_price = p["average_price"].as_f64().unwrap_or(0.0);
            let last_price = p["last_price"].as_f64().unwrap_or(avg_price);
            let pnl = p["pnl"].as_f64().unwrap_or(0.0);

            positions.push(VenuePosition {
                symbol: sym,
                qty,
                entry_price: avg_price,
                mark_price: last_price,
                unrealized_pnl: pnl,
                initial_margin: 0.0,
                maintenance_margin: 0.0,
            });
        }

        Ok(positions)
    }

    /// Fetches margins from Kite (`GET /user/margins`).
    pub async fn get_account_summary(&self) -> Result<VenueAccountSummary, BrokerError> {
        let url = format!("{}/user/margins", self.base_url);
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Kite network error: {e}")))?;

        let val: serde_json::Value =
            resp.json().await.map_err(|e| BrokerError::MalformedResponse(format!("Invalid JSON: {e}")))?;

        let equity_data = &val["data"]["equity"];
        let net = equity_data["net"].as_f64().unwrap_or(0.0);
        let available_cash = equity_data["available"]["cash"].as_f64().unwrap_or(0.0);
        let utilised_margin = equity_data["utilised"]["debits"].as_f64().unwrap_or(0.0);

        Ok(VenueAccountSummary {
            equity: net,
            available_cash,
            initial_margin: utilised_margin,
            maintenance_margin: utilised_margin * 0.8,
            unrealized_pnl: equity_data["utilised"]["m2m_unrealised"].as_f64().unwrap_or(0.0),
            currency: "INR".to_string(),
        })
    }
}
