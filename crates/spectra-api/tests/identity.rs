//! Compatibility tests for identity tokens issued by the dashboard proxy.

use spectra_api::identity::{hmac_sha256, verify, Role, Secret};

const SECRET: &str = "test-engine-secret-xxxxxxxxxxxxxxxxxxxxxxxxxxxx";

fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &byte)| acc | u32::from(byte) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            output.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    output
}

fn token(role: &str, method: &str, path: &str, now: i64) -> String {
    let json = format!(
        r#"{{"v":1,"uid":"user-1","sid":"session-1","wid":"workspace-1","wrole":"{role}","prole":"user","m":"{method}","p":"{}","iat":{now},"exp":{}}}"#,
        path.trim_start_matches('/'),
        now + 60
    );
    let encoded = base64url(json.as_bytes());
    format!("{encoded}.{}", base64url(&hmac_sha256(SECRET.as_bytes(), encoded.as_bytes())))
}

#[test]
fn dashboard_identity_tokens_are_signed_and_bound_to_the_request() {
    let now = 1_800_000_000;
    let secret = Secret::new(SECRET).unwrap();
    let signed = token("editor", "POST", "/api/risk", now);
    let caller = verify(&signed, &secret, "POST", "/api/risk", now).unwrap();

    assert_eq!(caller.user_id, "user-1");
    assert_eq!(caller.session_id, "session-1");
    assert_eq!(caller.workspace_id, "workspace-1");
    assert_eq!(caller.workspace_role, Role::Editor);
    assert!(!caller.platform_admin);
    assert!(verify(&signed, &secret, "GET", "/api/risk", now).is_err());
    assert!(verify(&signed, &secret, "POST", "/api/stress", now).is_err());
    assert!(verify(&signed, &secret, "POST", "/api/risk", now + 66).is_err());
}
