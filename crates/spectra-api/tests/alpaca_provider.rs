//! Tests for Alpaca market data connector and vault envelope decryption (M1b task 13).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use serde_json::json;
use spectra_api::providers::alpaca::AlpacaConnector;
use spectra_api::providers::vault::{base64_decode, open_aes_256_gcm, parse_master_keyring};
use spectra_api::providers::{BarResolution, FetchBarsRequest, MarketDataConnector, ProviderError};

fn json_response(status: StatusCode, value: &serde_json::Value) -> impl IntoResponse {
    (status, [("content-type", "application/json")], value.to_string())
}

#[test]
fn test_alpaca_connector_redacts_credentials_in_debug() {
    let conn = AlpacaConnector::new("https://example.com", "MY_SECRET_KEY_ID", "SUPER_SECRET_KEY_123", "iex");
    let debug_str = format!("{conn:?}");
    assert!(!debug_str.contains("MY_SECRET_KEY_ID"), "key_id must not leak in Debug");
    assert!(!debug_str.contains("SUPER_SECRET_KEY_123"), "secret_key must not leak in Debug");
    assert!(debug_str.contains("[REDACTED]"));
    assert!(debug_str.contains("https://example.com"));
    assert!(debug_str.contains("iex"));
}

#[test]
fn test_vault_keyring_and_envelope_crypto() {
    // 32 bytes = 256 bits
    let master_raw: [u8; 32] = [42u8; 32];
    let b64_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // Encode master_raw in standard base64 for test
    let mut master_b64 = String::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for &b in &master_raw {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            master_b64.push(b64_chars.chars().nth(((acc >> bits) & 0x3f) as usize).unwrap());
        }
    }
    if bits > 0 {
        acc <<= 6 - bits;
        master_b64.push(b64_chars.chars().nth((acc & 0x3f) as usize).unwrap());
        while !master_b64.len().is_multiple_of(4) {
            master_b64.push('=');
        }
    }

    let parsed = base64_decode(&master_b64).expect("decode base64");
    assert_eq!(parsed, master_raw);

    // Test parse_master_keyring
    let ring_str = format!("1:{master_b64}");
    let ring = parse_master_keyring(&ring_str).expect("parse keyring");
    assert_eq!(ring.get(&1), Some(&master_raw));

    // Test AES-256-GCM seal and open
    let data_key: [u8; 32] = [7u8; 32];
    let aad = b"spectra:workspace-key:00000000-0000-0000-0000-000000000001";
    let plaintext = b"{\"keyId\":\"PK123\",\"secretKey\":\"SK456\"}";

    // Seal using ring
    let unbound = UnboundKey::new(&AES_256_GCM, &data_key).unwrap();
    let safe = LessSafeKey::new(unbound);
    let iv = [1u8; 12];
    let nonce = Nonce::assume_unique_for_key(iv);
    let mut in_out = plaintext.to_vec();
    let tag = safe.seal_in_place_separate_tag(nonce, Aad::from(aad), &mut in_out).unwrap();

    // Construct format byte(1) + iv(12) + tag(16) + ct
    let mut blob = Vec::with_capacity(1 + 12 + 16 + in_out.len());
    blob.push(1); // FORMAT
    blob.extend_from_slice(&iv);
    blob.extend_from_slice(tag.as_ref());
    blob.extend_from_slice(&in_out);

    // Decrypt with open_aes_256_gcm
    let opened = open_aes_256_gcm(&data_key, &blob, aad).expect("open_aes_256_gcm succeeds");
    assert_eq!(opened, plaintext);

    // Tampered tag must fail
    let mut tampered = blob.clone();
    tampered[15] ^= 0x01;
    assert!(open_aes_256_gcm(&data_key, &tampered, aad).is_err());

    // Wrong AAD must fail
    assert!(open_aes_256_gcm(&data_key, &blob, b"wrong:aad").is_err());
}

#[tokio::test]
async fn test_alpaca_connector_mock_http_flow() {
    let call_count = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&call_count);

    let app = Router::new().route(
        "/stocks/bars",
        get(move |uri: axum::http::Uri, headers: HeaderMap| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::Relaxed);

                let params: HashMap<String, String> = uri
                    .query()
                    .unwrap_or_default()
                    .split('&')
                    .filter_map(|pair| {
                        let (k, v) = pair.split_once('=')?;
                        Some((k.to_string(), v.to_string()))
                    })
                    .collect();

                let key = headers.get("APCA-API-KEY-ID").and_then(|v| v.to_str().ok());
                let secret = headers.get("APCA-API-SECRET-KEY").and_then(|v| v.to_str().ok());
                if key != Some("PK_VALID") || secret != Some("SK_VALID") {
                    return json_response(StatusCode::UNAUTHORIZED, &json!({"message": "unauthorized"}))
                        .into_response();
                }

                if params.get("symbols") == Some(&"RATE_LIMITED".to_string()) {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "45"), ("content-type", "application/json")],
                        json!({"message": "too many requests"}).to_string(),
                    )
                        .into_response();
                }

                // Check if this is the validate_credentials probe
                if params.get("symbols").map(String::as_str) == Some("SPY") {
                    return json_response(
                        StatusCode::OK,
                        &json!({
                            "bars": {
                                "SPY": [{
                                    "t": "2026-01-01T00:00:00Z",
                                    "o": 500.0,
                                    "h": 505.0,
                                    "l": 498.0,
                                    "c": 502.5,
                                    "v": 1_000_000.0
                                }]
                            },
                            "next_page_token": null
                        }),
                    )
                    .into_response();
                }

                // Check pagination
                let page = params.get("page_token").map(String::as_str);
                if page.is_none() {
                    // Page 1
                    json_response(
                        StatusCode::OK,
                        &json!({
                            "bars": {
                                "AAPL": [{
                                    "t": "2026-01-02T14:30:00Z",
                                    "o": 180.0,
                                    "h": 182.0,
                                    "l": 179.5,
                                    "c": 181.0,
                                    "v": 500_000.0
                                }]
                            },
                            "next_page_token": "page_2_token"
                        }),
                    )
                    .into_response()
                } else if page == Some("page_2_token") {
                    // Page 2
                    json_response(
                        StatusCode::OK,
                        &json!({
                            "bars": {
                                "AAPL": [{
                                    "t": "2026-01-03T14:30:00Z",
                                    "o": 181.0,
                                    "h": 184.0,
                                    "l": 180.5,
                                    "c": 183.5,
                                    "v": 600_000.0
                                }]
                            },
                            "next_page_token": null
                        }),
                    )
                    .into_response()
                } else {
                    json_response(StatusCode::BAD_REQUEST, &json!({"message": "unknown page"}))
                        .into_response()
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");

    // 1. Test validate credentials success
    let conn = AlpacaConnector::new(&base_url, "PK_VALID", "SK_VALID", "iex");
    assert_eq!(conn.name(), "alpaca");
    conn.validate_credentials().await.expect("validate_credentials succeeds");

    // 2. Test validate credentials unauthorized
    let bad_conn = AlpacaConnector::new(&base_url, "BAD_KEY", "BAD_SECRET", "iex");
    let err = bad_conn.validate_credentials().await.unwrap_err();
    assert!(matches!(err, ProviderError::AuthenticationFailed(_)));

    // 3. Test rate limiting
    let rate_req = FetchBarsRequest {
        symbols: vec!["RATE_LIMITED".to_string()],
        start_ns: 1_700_000_000_000_000_000,
        end_ns: 1_700_100_000_000_000_000,
        resolution: BarResolution::Day,
    };
    let rl_err = conn.fetch_bars(&rate_req).await.unwrap_err();
    assert_eq!(rl_err, ProviderError::RateLimited { retry_after_secs: Some(45) });

    // 4. Test multi-page fetch_bars
    let req = FetchBarsRequest {
        symbols: vec!["AAPL".to_string()],
        start_ns: 1_700_000_000_000_000_000,
        end_ns: 1_700_100_000_000_000_000,
        resolution: BarResolution::Day,
    };

    let bars = conn.fetch_bars(&req).await.expect("fetch_bars succeeds");
    assert_eq!(bars.len(), 2, "received 2 bars across 2 pages");
    assert_eq!(bars[0].instrument, 0);
    assert_eq!(bars[1].instrument, 0);
    assert!(bars[0].ts_ns < bars[1].ts_ns);
    assert_eq!(bars[0].open.raw(), 180 * 100_000_000);
    assert_eq!(bars[0].close.raw(), 181 * 100_000_000);
    assert_eq!(bars[1].open.raw(), 181 * 100_000_000);
    assert_eq!(bars[1].close.raw(), 18_350_000_000);
}
