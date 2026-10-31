//! Binance USDⓈ-M Futures Testnet & Production Adapter (M24-advanced Task 14, 15, 16).
//!
//! Provides HMAC-SHA256 authenticated REST order routing, perpetual funding payment
//! tracking in the ledger, and dated quarterly contract expiration handling.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderValue};
use ring::hmac;
use serde::{Deserialize, Serialize};

use super::symbols::map_binance_symbol_to_instrument;
use super::{
    BrokerError, CancelOrderResponse, VenueAccountSummary, VenueOrderRequest, VenueOrderResponse,
    VenuePosition,
};

pub const BINANCE_TESTNET_BASE_URL: &str = "https://testnet.binancefuture.com";
pub const BINANCE_PROD_BASE_URL: &str = "https://fapi.binance.com";

/// Binance USDⓈ-M Futures adapter client.
#[derive(Debug, Clone)]
pub struct BinanceUsdmAdapter {
    pub base_url: String,
    pub api_key: String,
    pub api_secret: String,
    pub client: reqwest::Client,
}

impl BinanceUsdmAdapter {
    #[must_use]
    pub fn new(api_key: String, api_secret: String, is_testnet: bool) -> Self {
        let base_url =
            if is_testnet { BINANCE_TESTNET_BASE_URL.to_string() } else { BINANCE_PROD_BASE_URL.to_string() };

        Self { base_url, api_key, api_secret, client: reqwest::Client::new() }
    }

    /// Generates an HMAC-SHA256 hex signature for query parameters.
    #[must_use]
    pub fn sign_query(&self, query: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.api_secret.as_bytes());
        let signature = hmac::sign(&key, query.as_bytes());
        let mut hex = String::with_capacity(signature.as_ref().len() * 2);
        for byte in signature.as_ref() {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&self.api_key) {
            headers.insert("X-MBX-APIKEY", val);
        }
        headers
    }

    fn timestamp_ms() -> u128 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
    }

    /// Submits a futures order to Binance USDⓈ-M.
    pub async fn submit_order(&self, req: &VenueOrderRequest) -> Result<VenueOrderResponse, BrokerError> {
        let ts = Self::timestamp_ms();
        let side_str = match req.side {
            super::OrderSide::Buy => "BUY",
            super::OrderSide::Sell => "SELL",
        };
        let type_str = req.order_type.to_uppercase();

        let mut query = format!(
            "symbol={}&side={}&type={}&quantity={}&timestamp={}",
            req.symbol.to_uppercase(),
            side_str,
            type_str,
            req.qty,
            ts
        );

        if let Some(price) = req.price {
            query.push_str(&format!("&price={price}&timeInForce=GTC"));
        }
        if let Some(ref client_id) = req.client_order_id {
            query.push_str(&format!("&newClientOrderId={client_id}"));
        }

        let signature = self.sign_query(&query);
        let url = format!("{}/fapi/v1/order?{}&signature={}", self.base_url, query, signature);

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Binance network error: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| BrokerError::Network(format!("Failed to read Binance response: {e}")))?;

        if !status.is_success() {
            return Err(BrokerError::OrderRejected(format!(
                "Binance order rejected (status {status}): {body}"
            )));
        }

        let val: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid Binance JSON: {e}")))?;

        let order_id = val["orderId"].to_string();
        let client_id = val["clientOrderId"].as_str().unwrap_or("").to_string();
        let status_str = val["status"].as_str().unwrap_or("NEW").to_string();
        let executed_qty = val["executedQty"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let avg_price = val["avgPrice"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

        Ok(VenueOrderResponse {
            venue_order_id: order_id,
            client_order_id: client_id,
            status: status_str,
            filled_qty: executed_qty,
            avg_fill_price: avg_price,
            raw_response: val,
        })
    }

    /// Cancels an open order on Binance USDⓈ-M.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        venue_order_id: &str,
    ) -> Result<CancelOrderResponse, BrokerError> {
        let ts = Self::timestamp_ms();
        let query = format!("symbol={}&orderId={}&timestamp={}", symbol.to_uppercase(), venue_order_id, ts);
        let signature = self.sign_query(&query);
        let url = format!("{}/fapi/v1/order?{}&signature={}", self.base_url, query, signature);

        let resp = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Binance network error: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(BrokerError::OrderRejected(format!("Failed to cancel Binance order: {body}")));
        }

        Ok(CancelOrderResponse { venue_order_id: venue_order_id.to_string(), status: "CANCELED".to_string() })
    }

    /// Fetches positions and risk from Binance USDⓈ-M.
    pub async fn get_positions(&self) -> Result<Vec<VenuePosition>, BrokerError> {
        let ts = Self::timestamp_ms();
        let query = format!("timestamp={ts}");
        let signature = self.sign_query(&query);
        let url = format!("{}/fapi/v2/positionRisk?{}&signature={}", self.base_url, query, signature);

        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Binance network error: {e}")))?;

        let val: serde_json::Value =
            resp.json().await.map_err(|e| BrokerError::MalformedResponse(format!("Invalid JSON: {e}")))?;

        let arr = val.as_array().ok_or_else(|| {
            BrokerError::MalformedResponse("Expected array in Binance positionRisk".to_string())
        })?;

        let mut positions = Vec::new();
        for p in arr {
            let sym = p["symbol"].as_str().unwrap_or("").to_string();
            let amt = p["positionAmt"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            if amt.abs() < 1e-8 {
                continue;
            }
            let entry_price = p["entryPrice"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let mark_price =
                p["markPrice"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(entry_price);
            let unrealized_pnl =
                p["unRealizedProfit"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let initial_margin =
                p["initialMargin"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let maint_margin = p["maintMargin"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

            positions.push(VenuePosition {
                symbol: sym,
                qty: amt,
                entry_price,
                mark_price,
                unrealized_pnl,
                initial_margin,
                maintenance_margin: maint_margin,
            });
        }

        Ok(positions)
    }

    /// Fetches account balance and collateral from Binance USDⓈ-M.
    pub async fn get_account_summary(&self) -> Result<VenueAccountSummary, BrokerError> {
        let ts = Self::timestamp_ms();
        let query = format!("timestamp={ts}");
        let signature = self.sign_query(&query);
        let url = format!("{}/fapi/v2/account?{}&signature={}", self.base_url, query, signature);

        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("Binance network error: {e}")))?;

        let val: serde_json::Value =
            resp.json().await.map_err(|e| BrokerError::MalformedResponse(format!("Invalid JSON: {e}")))?;

        let total_margin_balance =
            val["totalMarginBalance"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let available_balance =
            val["availableBalance"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let total_initial_margin =
            val["totalInitialMargin"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let total_maint_margin =
            val["totalMaintMargin"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let unrealized_profit =
            val["totalUnrealizedProfit"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

        Ok(VenueAccountSummary {
            equity: total_margin_balance,
            available_cash: available_balance,
            initial_margin: total_initial_margin,
            maintenance_margin: total_maint_margin,
            unrealized_pnl: unrealized_profit,
            currency: "USDT".to_string(),
        })
    }
}

/// Computes the 8-hour perpetual funding payment (Task 15).
///
/// Funding payment formula:
/// `Payment = -1 * Position Size * Mark Price * Funding Rate`
///
/// - If Long (qty > 0) and Funding Rate > 0: Pay funding (Cash decreases).
/// - If Short (qty < 0) and Funding Rate > 0: Receive funding (Cash increases).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingPayment {
    pub symbol: String,
    pub position_qty: f64,
    pub mark_price: f64,
    pub funding_rate: f64,
    pub payment_amount: f64,
    pub timestamp_ns: i64,
}

#[must_use]
pub fn compute_funding_payment(
    symbol: &str,
    qty: f64,
    mark_price: f64,
    funding_rate: f64,
    timestamp_ns: i64,
) -> FundingPayment {
    let payment = -qty * mark_price * funding_rate;
    FundingPayment {
        symbol: symbol.to_string(),
        position_qty: qty,
        mark_price,
        funding_rate,
        payment_amount: payment,
        timestamp_ns,
    }
}

/// Evaluates settlement for a dated quarterly Binance contract (Task 16).
pub fn evaluate_binance_quarterly_settlement(
    symbol: &str,
    qty: f64,
    entry_price: f64,
    settlement_index_price: f64,
) -> Result<f64, String> {
    let inst = map_binance_symbol_to_instrument(symbol).map_err(|e| e.to_string())?;
    if inst.expiry_ns.is_none() {
        return Err(format!("{symbol} is a perpetual contract, not a dated quarterly."));
    }
    let pnl = qty * (settlement_index_price - entry_price);
    Ok(pnl)
}
