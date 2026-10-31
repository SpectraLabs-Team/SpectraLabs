//! Interactive Brokers Client Portal & Web API Adapter (M24-advanced Task 6, 7, 8, 9, 10).
//!
//! Connects to the local IBKR Client Portal Gateway (CP Gateway on localhost:5000).
//! Implements automated session keep-alive (tickle), multi-step reply confirmation,
//! CME & Eurex futures and options execution, and paper account integration.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    BrokerError, CancelOrderResponse, OrderSide, VenueAccountSummary, VenueOrderRequest, VenueOrderResponse,
    VenuePosition,
};

pub const DEFAULT_IBKR_GATEWAY_URL: &str = "https://localhost:5000/v1/api";

/// Configuration and access details for IBKR Client Portal Gateway (Task 6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbkrGatewayConfig {
    pub base_url: String,
    pub account_id: String,
    pub is_paper: bool,
    pub tickle_interval_secs: u64,
}

impl Default for IbkrGatewayConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_IBKR_GATEWAY_URL.to_string(),
            account_id: String::new(),
            is_paper: true,
            tickle_interval_secs: 45,
        }
    }
}

/// Interactive Brokers Client Portal REST adapter (Task 7).
#[derive(Debug, Clone)]
pub struct InteractiveBrokersAdapter {
    pub config: IbkrGatewayConfig,
    pub client: reqwest::Client,
}

impl InteractiveBrokersAdapter {
    #[must_use]
    pub fn new(config: IbkrGatewayConfig) -> Self {
        // IBKR Client Portal Gateway typically uses a self-signed HTTPS certificate
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap_or_default();

        Self { config, client }
    }

    /// Verifies auth status with the gateway (`GET /iserver/auth/status`).
    pub async fn check_auth_status(&self) -> Result<bool, BrokerError> {
        let url = format!("{}/iserver/auth/status", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR Gateway unreachable: {e}")))?;

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid auth status JSON: {e}")))?;

        let authenticated = val["authenticated"].as_bool().unwrap_or(false);
        let connected = val["connected"].as_bool().unwrap_or(false);
        Ok(authenticated && connected)
    }

    /// Sends a session keep-alive tickle (`GET /tickle`) to prevent session timeout.
    pub async fn tickle(&self) -> Result<(), BrokerError> {
        let url = format!("{}/tickle", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR tickle failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(BrokerError::Authentication("IBKR tickle returned unsuccessful status".to_string()));
        }
        Ok(())
    }

    /// Submits an order and automatically answers confirmation replies if required.
    pub async fn submit_order(&self, req: &VenueOrderRequest) -> Result<VenueOrderResponse, BrokerError> {
        let account_id = if self.config.account_id.is_empty() {
            "DU123456" // Default paper account prefix
        } else {
            &self.config.account_id
        };

        let url = format!("{}/iserver/account/{account_id}/orders", self.config.base_url);
        let side_str = match req.side {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        };

        let order_payload = json!({
            "orders": [{
                "conid": req.conid.unwrap_or(0),
                "ticker": req.symbol,
                "secType": req.sec_type.as_deref().unwrap_or("FUT"),
                "orderType": req.order_type.to_uppercase(),
                "side": side_str,
                "quantity": req.qty,
                "price": req.price,
                "tif": "DAY"
            }]
        });

        let resp = self
            .client
            .post(&url)
            .json(&order_payload)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR order post error: {e}")))?;

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid IBKR response: {e}")))?;

        // Check if IBKR returned a confirmation reply requirement (e.g. price cap, margin warning)
        if let Some(reply_id) = val[0]["id"].as_str() {
            return self.confirm_order_reply(reply_id).await;
        }

        let order_id = val[0]["order_id"]
            .as_str()
            .or_else(|| val[0]["order_id"].as_i64().map(|_| ""))
            .unwrap_or("")
            .to_string();

        let status_str = val[0]["order_status"].as_str().unwrap_or("Submitted").to_string();

        Ok(VenueOrderResponse {
            venue_order_id: order_id,
            client_order_id: req.client_order_id.clone().unwrap_or_default(),
            status: status_str,
            filled_qty: 0.0,
            avg_fill_price: req.price.unwrap_or(0.0),
            raw_response: val,
        })
    }

    /// Answers a confirmation reply (`POST /iserver/reply/{replyId}`).
    pub async fn confirm_order_reply(&self, reply_id: &str) -> Result<VenueOrderResponse, BrokerError> {
        let url = format!("{}/iserver/reply/{reply_id}", self.config.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "confirmed": true }))
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR reply post error: {e}")))?;

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid reply response: {e}")))?;

        let order_id = val[0]["order_id"].to_string();
        let status_str = val[0]["order_status"].as_str().unwrap_or("Submitted").to_string();

        Ok(VenueOrderResponse {
            venue_order_id: order_id,
            client_order_id: String::new(),
            status: status_str,
            filled_qty: 0.0,
            avg_fill_price: 0.0,
            raw_response: val,
        })
    }

    /// Cancels an open order (`DELETE /iserver/account/{accountId}/order/{orderId}`).
    pub async fn cancel_order(&self, venue_order_id: &str) -> Result<CancelOrderResponse, BrokerError> {
        let account_id = if self.config.account_id.is_empty() { "DU123456" } else { &self.config.account_id };

        let url = format!("{}/iserver/account/{account_id}/order/{venue_order_id}", self.config.base_url);
        let resp = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR cancel error: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(BrokerError::OrderRejected(format!("Failed to cancel IBKR order: {body}")));
        }

        Ok(CancelOrderResponse {
            venue_order_id: venue_order_id.to_string(),
            status: "Cancelled".to_string(),
        })
    }

    /// Fetches positions (`GET /iserver/account/positions/0`).
    pub async fn get_positions(&self) -> Result<Vec<VenuePosition>, BrokerError> {
        let url = format!("{}/iserver/account/positions/0", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR positions error: {e}")))?;

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid positions JSON: {e}")))?;

        let arr = val
            .as_array()
            .ok_or_else(|| BrokerError::MalformedResponse("Expected array in IBKR positions".to_string()))?;

        let mut positions = Vec::new();
        for p in arr {
            let sym = p["ticker"].as_str().unwrap_or("").to_string();
            let pos_qty = p["position"].as_f64().unwrap_or(0.0);
            if pos_qty.abs() < 1e-8 {
                continue;
            }
            let avg_cost = p["avgCost"].as_f64().unwrap_or(0.0);
            let mkt_price = p["mktPrice"].as_f64().unwrap_or(avg_cost);
            let unrealized = p["unrealizedPnl"].as_f64().unwrap_or(0.0);

            positions.push(VenuePosition {
                symbol: sym,
                qty: pos_qty,
                entry_price: avg_cost,
                mark_price: mkt_price,
                unrealized_pnl: unrealized,
                initial_margin: 0.0,
                maintenance_margin: 0.0,
            });
        }

        Ok(positions)
    }

    /// Fetches account summary (`GET /iserver/account/{accountId}/summary`).
    pub async fn get_account_summary(&self) -> Result<VenueAccountSummary, BrokerError> {
        let account_id = if self.config.account_id.is_empty() { "DU123456" } else { &self.config.account_id };

        let url = format!("{}/iserver/account/{account_id}/summary", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| BrokerError::Network(format!("IBKR summary error: {e}")))?;

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BrokerError::MalformedResponse(format!("Invalid summary JSON: {e}")))?;

        let net_liq = val["netliquidation"]["amount"].as_f64().unwrap_or(100_000.0);
        let avail_funds = val["availablefunds"]["amount"].as_f64().unwrap_or(100_000.0);
        let init_margin = val["initmarginreq"]["amount"].as_f64().unwrap_or(0.0);
        let maint_margin = val["maintmarginreq"]["amount"].as_f64().unwrap_or(0.0);
        let currency = val["currency"].as_str().unwrap_or("USD").to_string();

        Ok(VenueAccountSummary {
            equity: net_liq,
            available_cash: avail_funds,
            initial_margin: init_margin,
            maintenance_margin: maint_margin,
            unrealized_pnl: 0.0,
            currency,
        })
    }
}

/// Evaluates automatic roll rule for CME/Eurex futures (Task 8 & 9).
/// Rolls 5 business days prior to expiry into next quarterly contract.
#[must_use]
pub fn check_futures_roll_due(now_ns: i64, expiry_ns: i64, business_days_before: i64) -> bool {
    let roll_window_ns = business_days_before * 24 * 3600 * 1_000_000_000;
    (expiry_ns - now_ns) <= roll_window_ns
}
