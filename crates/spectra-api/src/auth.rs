//! Trust between the Next.js proxy and the engine.
//!
//! The engine never sees a browser. The proxy checks the session, then signs
//! each request it forwards with HMAC-SHA256 under the shared `ENGINE_SECRET`:
//!
//! ```text
//! X-Spectra-User:       the signed-in user (id or email; opaque to the engine)
//! X-Spectra-Workspace:  the active workspace's UUID, or empty
//! X-Request-Id:         the request id
//! X-Spectra-Timestamp:  Unix seconds
//! X-Spectra-Signature:  hex HMAC-SHA256(ENGINE_SECRET, canonical string)
//! ```
//!
//! The canonical string ([`canonical`]) covers the method, path and query,
//! timestamp, the three identity headers and the SHA-256 of the body, so a
//! signature cannot be moved to a different request. Signatures older or
//! newer than [`MAX_SKEW_SECS`] are refused.
//!
//! HMAC is built on `spectra-core`'s SHA-256 (RFC 2104), so this adds no crate.

use spectra_core::sha256::to_hex;
use spectra_core::Sha256;
use uuid::Uuid;

/// Largest accepted difference between the signature's timestamp and now.
pub const MAX_SKEW_SECS: i64 = 60;

/// Header names (lower case, as HTTP/2 and the `http` crate use them).
pub mod headers {
    /// The signed-in user.
    pub const USER: &str = "x-spectra-user";
    /// The active workspace (UUID or empty).
    pub const WORKSPACE: &str = "x-spectra-workspace";
    /// Unix seconds when the proxy signed the request.
    pub const TIMESTAMP: &str = "x-spectra-timestamp";
    /// Hex HMAC-SHA256.
    pub const SIGNATURE: &str = "x-spectra-signature";
    /// The request id.
    pub const REQUEST_ID: &str = "x-request-id";
}

/// Who a verified request acts for. Handlers read it from the request's
/// extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The signed-in user.
    pub user: String,
    /// The active workspace, if the proxy sent one.
    pub workspace: Option<Uuid>,
}

const BLOCK: usize = 64;

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        k[..32].copy_from_slice(&h.digest());
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(&k.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(&k.map(|b| b ^ 0x5c));
    outer.update(&inner.digest());
    outer.digest()
}

/// Hex SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.hex_digest()
}

/// The string the proxy signs. Must match `signEngineRequest` in
/// `web/frontend/lib/server/engineSign.ts` byte for byte.
pub fn canonical(
    method: &str,
    path_and_query: &str,
    timestamp: &str,
    user: &str,
    workspace: &str,
    request_id: &str,
    body: &[u8],
) -> String {
    format!(
        "spectra-v1\n{}\n{path_and_query}\n{timestamp}\n{user}\n{workspace}\n{request_id}\n{}",
        method.to_ascii_uppercase(),
        sha256_hex(body)
    )
}

/// Hex signature for a canonical string.
pub fn sign(secret: &[u8], canonical: &str) -> String {
    to_hex(&hmac_sha256(secret, canonical.as_bytes()))
}

/// Compares two byte strings in time that depends only on their lengths.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The signed parts of a request, as received.
#[derive(Debug, Clone, Copy)]
pub struct SignedRequest<'a> {
    /// HTTP method.
    pub method: &'a str,
    /// Path and query exactly as received.
    pub path_and_query: &'a str,
    /// `X-Spectra-User`.
    pub user: Option<&'a str>,
    /// `X-Spectra-Workspace`.
    pub workspace: Option<&'a str>,
    /// `X-Request-Id`.
    pub request_id: Option<&'a str>,
    /// `X-Spectra-Timestamp`.
    pub timestamp: Option<&'a str>,
    /// `X-Spectra-Signature`.
    pub signature: Option<&'a str>,
    /// Request body.
    pub body: &'a [u8],
}

/// Checks a request's signature and freshness.
///
/// # Errors
/// A short reason, safe to return to the caller.
pub fn verify(secret: &[u8], req: &SignedRequest<'_>, now_secs: i64) -> Result<Identity, &'static str> {
    let (Some(user), Some(ts), Some(sig)) = (req.user, req.timestamp, req.signature) else {
        return Err("Missing engine signature headers.");
    };
    let workspace = req.workspace.unwrap_or("");
    let request_id = req.request_id.unwrap_or("");
    let ts_val: i64 = ts.parse().map_err(|_| "Invalid signature timestamp.")?;
    if (now_secs - ts_val).abs() > MAX_SKEW_SECS {
        return Err("Signature expired.");
    }
    let expected =
        sign(secret, &canonical(req.method, req.path_and_query, ts, user, workspace, request_id, req.body));
    if !constant_time_eq(expected.as_bytes(), sig.trim().to_ascii_lowercase().as_bytes()) {
        return Err("Invalid engine signature.");
    }
    if user.is_empty() || user.len() > 320 {
        return Err("Invalid user header.");
    }
    let workspace = if workspace.is_empty() {
        None
    } else {
        Some(Uuid::parse_str(workspace).map_err(|_| "Invalid workspace header.")?)
    };
    Ok(Identity { user: user.to_string(), workspace })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2.
        assert_eq!(
            to_hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than the block size is hashed first.
        assert_eq!(
            to_hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const WS: &str = "0192f3a4-0000-7000-8000-000000000001";

    fn signed(ts: i64, body: &'static [u8]) -> (String, String) {
        let ts = ts.to_string();
        let sig =
            sign(SECRET, &canonical("POST", "/api/v1/risk", &ts, "alice@example.com", WS, "req-1", body));
        (ts, sig)
    }

    fn req<'a>(ts: &'a str, sig: &'a str, body: &'a [u8]) -> SignedRequest<'a> {
        SignedRequest {
            method: "POST",
            path_and_query: "/api/v1/risk",
            user: Some("alice@example.com"),
            workspace: Some(WS),
            request_id: Some("req-1"),
            timestamp: Some(ts),
            signature: Some(sig),
            body,
        }
    }

    #[test]
    fn a_fresh_signature_verifies() {
        let (ts, sig) = signed(1_000, b"{}");
        let id = verify(SECRET, &req(&ts, &sig, b"{}"), 1_030).unwrap();
        assert_eq!(id.user, "alice@example.com");
        assert_eq!(id.workspace.unwrap().to_string(), WS);
    }

    #[test]
    fn tampering_or_staleness_is_refused() {
        let (ts, sig) = signed(1_000, b"{}");
        assert_eq!(verify(SECRET, &req(&ts, &sig, b"{}"), 1_061), Err("Signature expired."));
        assert_eq!(verify(SECRET, &req(&ts, &sig, b"{}"), 939), Err("Signature expired."));
        assert_eq!(verify(SECRET, &req(&ts, &sig, b"{\"x\":1}"), 1_000), Err("Invalid engine signature."));
        assert_eq!(
            verify(b"another-secret-another-secret-xx", &req(&ts, &sig, b"{}"), 1_000),
            Err("Invalid engine signature.")
        );
        let other_user = SignedRequest { user: Some("mallory@example.com"), ..req(&ts, &sig, b"{}") };
        assert_eq!(verify(SECRET, &other_user, 1_000), Err("Invalid engine signature."));
        let other_path = SignedRequest { path_and_query: "/api/v1/benchmark", ..req(&ts, &sig, b"{}") };
        assert_eq!(verify(SECRET, &other_path, 1_000), Err("Invalid engine signature."));
        let unsigned = SignedRequest { signature: None, ..req(&ts, &sig, b"{}") };
        assert_eq!(verify(SECRET, &unsigned, 1_000), Err("Missing engine signature headers."));
    }

    #[test]
    fn the_canonical_string_is_stable() {
        // Pinned so a change on either side (Rust or engineSign.ts) is caught.
        assert_eq!(
            canonical("post", "/api/v1/risk?x=1", "1700000000", "u", "", "r", b""),
            "spectra-v1\nPOST\n/api/v1/risk?x=1\n1700000000\nu\n\nr\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
