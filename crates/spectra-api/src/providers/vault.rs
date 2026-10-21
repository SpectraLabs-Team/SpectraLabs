//! Vault credential resolution and AES-256-GCM envelope decryption.
//!
//! Replicates the frontend/worker envelope encryption scheme:
//! 1. Master key from `SECRETS_MASTER_KEY` (AES-256) decrypts the workspace data key in `workspace_keys`.
//! 2. Workspace data key decrypts the secret JSON in `secrets`.
//! 3. Plaintext keys are held in memory only for the duration of the request and never logged.

use std::collections::HashMap;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

/// Decodes standard RFC 4648 base64.
#[must_use]
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if bits >= 6 {
        return None;
    }
    Some(out)
}

/// Parses `SECRETS_MASTER_KEY` into a map of version to 32-byte key.
///
/// Supports either `<base64>` (version 1) or `v1:<base64>,v2:<base64>`.
///
/// # Errors
/// Returns an error string if format or key length is invalid.
pub fn parse_master_keyring(raw: &str) -> Result<HashMap<i32, [u8; 32]>, String> {
    let mut by_version = HashMap::new();
    let entries: Vec<&str> = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if entries.is_empty() {
        return Err("SECRETS_MASTER_KEY is empty".to_string());
    }

    for entry in entries {
        let (version, b64) = if let Some((v_str, key_b64)) = entry.split_once(':') {
            let v = v_str.parse::<i32>().map_err(|_| format!("invalid master key version '{v_str}'"))?;
            (v, key_b64.trim())
        } else {
            (1, entry)
        };

        let bytes =
            base64_decode(b64).ok_or_else(|| format!("invalid base64 in master key version {version}"))?;
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| format!("SECRETS_MASTER_KEY version {version} must be exactly 32 bytes"))?;

        by_version.insert(version, key);
    }

    Ok(by_version)
}

/// Decrypts an AES-256-GCM sealed blob formatted as `[format(1)] [iv(12)] [tag(16)] [ciphertext...]`.
///
/// # Errors
/// Returns an error string if decrypting or tag verification fails.
pub fn open_aes_256_gcm(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
    const FORMAT: u8 = 1;
    const HEADER_LEN: usize = 1 + 12 + 16; // 29 bytes

    if blob.len() < HEADER_LEN {
        return Err("ciphertext blob too short".to_string());
    }
    if blob[0] != FORMAT {
        return Err(format!("unknown ciphertext format byte: {}", blob[0]));
    }

    let iv = &blob[1..13];
    let tag = &blob[13..29];
    let ct = &blob[29..];

    let mut in_out = Vec::with_capacity(ct.len() + tag.len());
    in_out.extend_from_slice(ct);
    in_out.extend_from_slice(tag);

    let unbound_key =
        UnboundKey::new(&AES_256_GCM, key).map_err(|_| "failed to create AES-256-GCM key".to_string())?;
    let safe_key = LessSafeKey::new(unbound_key);

    let nonce_bytes: [u8; 12] = iv.try_into().map_err(|_| "invalid IV length".to_string())?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let decrypted = safe_key
        .open_in_place(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| "failed to decrypt ciphertext (authentication tag mismatch)".to_string())?;

    Ok(decrypted.to_vec())
}

#[derive(Deserialize)]
struct AlpacaSecretPayload {
    #[serde(rename = "keyId")]
    key_id: String,
    #[serde(rename = "secretKey")]
    secret_key: String,
}

/// Resolves Alpaca API credentials `(key_id, secret_key)` for a given provider configuration.
///
/// Priority:
/// 1. Direct keys in `provider_cfg.settings` (for tests / manual override).
/// 2. Vault encrypted secret in Postgres (`secrets` + `workspace_keys` via `SECRETS_MASTER_KEY`).
/// 3. Environment variables `ALPACA_API_KEY_ID` and `ALPACA_API_SECRET_KEY`.
///
/// # Errors
/// Returns an error string if credentials cannot be resolved or decrypted.
pub async fn resolve_alpaca_credentials(
    pool: &PgPool,
    workspace_id: Uuid,
    secret_id: Option<Uuid>,
    settings: &JsonValue,
) -> Result<(String, String), String> {
    // 1. Check settings
    if let (Some(k), Some(s)) = (
        settings.get("key_id").and_then(JsonValue::as_str),
        settings.get("secret_key").and_then(JsonValue::as_str),
    ) {
        if !k.is_empty() && !s.is_empty() {
            return Ok((k.to_string(), s.to_string()));
        }
    }

    // 2. Check vault if SECRETS_MASTER_KEY is available
    if let Ok(master_key_env) = std::env::var("SECRETS_MASTER_KEY") {
        if !master_key_env.trim().is_empty() {
            let keyring = parse_master_keyring(&master_key_env)?;

            if let Some(sec_row) =
                spectra_db::vault::get_secret(pool, workspace_id, secret_id, Some("alpaca_paper"))
                    .await
                    .map_err(|e| format!("database error fetching secret: {e}"))?
            {
                if let Some(wk_row) = spectra_db::vault::get_workspace_key(pool, workspace_id)
                    .await
                    .map_err(|e| format!("database error fetching workspace key: {e}"))?
                {
                    let master_key = keyring.get(&wk_row.master_version).ok_or_else(|| {
                        format!(
                            "workspace key master_version {} not in SECRETS_MASTER_KEY",
                            wk_row.master_version
                        )
                    })?;

                    // Decrypt workspace data key
                    let wk_aad = format!("spectra:workspace-key:{workspace_id}");
                    let data_key_bytes =
                        open_aes_256_gcm(master_key, &wk_row.wrapped_key, wk_aad.as_bytes())?;
                    let data_key: [u8; 32] = data_key_bytes
                        .try_into()
                        .map_err(|_| "decrypted data key must be 32 bytes".to_string())?;

                    // Decrypt secret
                    let sec_aad = format!("spectra:secret:{workspace_id}:{}", sec_row.kind);
                    let sec_plaintext = open_aes_256_gcm(&data_key, &sec_row.ciphertext, sec_aad.as_bytes())?;

                    let payload: AlpacaSecretPayload = serde_json::from_slice(&sec_plaintext)
                        .map_err(|e| format!("invalid secret payload JSON: {e}"))?;

                    let _ = spectra_db::vault::mark_secret_used(pool, workspace_id, sec_row.id).await;

                    return Ok((payload.key_id, payload.secret_key));
                }
            }
        }
    }

    // 3. Fallback to environment variables
    if let (Ok(k), Ok(s)) = (std::env::var("ALPACA_API_KEY_ID"), std::env::var("ALPACA_API_SECRET_KEY")) {
        if !k.trim().is_empty() && !s.trim().is_empty() {
            return Ok((k.trim().to_string(), s.trim().to_string()));
        }
    }

    Err("No Alpaca credentials found in settings, vault, or environment (set ALPACA_API_KEY_ID or configure vault secret)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode("YWJj").unwrap(), b"abc");
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn test_parse_master_keyring() {
        let raw =
            "1:YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=,2:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
        let ring = parse_master_keyring(raw).unwrap();
        assert_eq!(ring.len(), 2);
        assert!(ring.contains_key(&1));
        assert!(ring.contains_key(&2));
    }
}
