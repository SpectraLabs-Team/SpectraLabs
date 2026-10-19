//! Verifies the signed identity header the Next.js proxy (`/api/engine`) adds to
//! every signed-in request (M19 tasks 3–4, M20 task 5).
//!
//! ```text
//! X-Spectra-Identity: <payload>.<signature>
//!   payload   = base64url(JSON {v: 1, uid, sid, wid, wrole, prole, m, p, iat, exp})
//!   signature = base64url(HMAC-SHA256(ENGINE_SHARED_SECRET, payload))
//! ```
//!
//! `wid` is the caller's active workspace and `wrole` their role in it
//! (`viewer` < `editor` < `owner`); `m`/`p` bind the header to one method and
//! path, and `exp` (60 s after `iat`) limits replay. The Next.js side is
//! `web/frontend/lib/server/engineIdentity.ts`.
//!
//! Standard library only, like the rest of this crate: SHA-256 (FIPS 180-4),
//! HMAC (RFC 2104) and base64url are implemented here and checked against the
//! published test vectors.

use spectra_core::Json;

/// Seconds of clock difference tolerated between the proxy and the engine.
pub const CLOCK_SKEW_S: i64 = 5;
/// Header name (lower-case, as [`crate::Request::header`] expects any case).
pub const HEADER: &str = "x-spectra-identity";
/// Shortest accepted `ENGINE_SHARED_SECRET`.
pub const MIN_SECRET_LEN: usize = 32;

/// A role in a workspace. Ordered: `Viewer < Editor < Owner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Read only.
    Viewer,
    /// Create and run.
    Editor,
    /// Members, settings, delete.
    Owner,
}

impl Role {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

/// A verified caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// User id.
    pub user_id: String,
    /// Session id.
    pub session_id: String,
    /// Active workspace: every read and write must be scoped to it.
    pub workspace_id: String,
    /// The caller's role in `workspace_id`.
    pub workspace_role: Role,
    /// `true` for platform admins.
    pub platform_admin: bool,
}

/// The shared secret, with a `Debug` that never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// `None` if the value is shorter than [`MIN_SECRET_LEN`].
    pub fn new(value: &str) -> Option<Self> {
        (value.len() >= MIN_SECRET_LEN).then(|| Self(value.as_bytes().to_vec()))
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// Checks `header` for a request with this method and path (`/api/risk`) at
/// `now` (unix seconds). The error is a short reason, safe to return.
///
/// # Errors
/// Bad format or signature, expired, or issued for another method/path.
pub fn verify(
    header: &str,
    secret: &Secret,
    method: &str,
    path: &str,
    now: i64,
) -> Result<Identity, &'static str> {
    let (payload, sig) = header.split_once('.').ok_or("malformed identity header")?;
    let sig = base64url_decode(sig).ok_or("malformed identity header")?;
    if !constant_time_eq(&sig, &hmac_sha256(&secret.0, payload.as_bytes())) {
        return Err("invalid identity signature");
    }
    let raw = base64url_decode(payload).ok_or("malformed identity header")?;
    let text = std::str::from_utf8(&raw).map_err(|_| "malformed identity header")?;
    let j = Json::parse(text).map_err(|_| "malformed identity header")?;
    let s = |k: &str| j.find(k).and_then(|v| v.as_str().ok()).filter(|v| !v.is_empty());
    let n = |k: &str| j.find(k).and_then(|v| v.as_int().ok());

    if n("v") != Some(1) {
        return Err("unsupported identity version");
    }
    let exp = n("exp").ok_or("malformed identity header")?;
    if now > exp + CLOCK_SKEW_S {
        return Err("identity header expired");
    }
    if n("iat").is_some_and(|iat| iat > now + CLOCK_SKEW_S) {
        return Err("identity header issued in the future");
    }
    if s("m") != Some(method) || s("p") != Some(path.trim_start_matches('/')) {
        return Err("identity header was issued for another request");
    }
    Ok(Identity {
        user_id: s("uid").ok_or("malformed identity header")?.to_string(),
        session_id: s("sid").ok_or("malformed identity header")?.to_string(),
        workspace_id: s("wid").ok_or("malformed identity header")?.to_string(),
        workspace_role: s("wrole").and_then(Role::parse).ok_or("malformed identity header")?,
        platform_admin: s("prole") == Some("admin"),
    })
}

/// Unix seconds now.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// base64url without padding (RFC 4648 §5). Padding is tolerated.
fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
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
    // A lone trailing character (6 bits) can't encode a byte.
    (bits < 6).then_some(out)
}

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, v) in out.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA256(key, data).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + data.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(data);
    let mut outer = Vec::with_capacity(96);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&sha256(&inner));
    sha256(&outer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().fold(String::new(), |mut s, x| {
            use std::fmt::Write;
            let _ = write!(s, "{x:02x}");
            s
        })
    }

    fn b64url(data: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    #[test]
    fn sha256_matches_fips_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_matches_rfc4231_vectors() {
        // Test case 1
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 2
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: key longer than the block size
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn base64url_round_trips() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0xfb, 0xff, 0xfe]] {
            assert_eq!(base64url_decode(&b64url(data)).as_deref(), Some(data));
        }
        assert_eq!(base64url_decode("Zm9vYmFy=="), Some(b"foobar".to_vec()), "padding tolerated");
        assert_eq!(base64url_decode("Zm9v+mFy"), None, "standard alphabet rejected");
        assert_eq!(base64url_decode("Z"), None, "lone trailing character");
    }

    pub(crate) fn sign(secret: &str, payload_json: &str) -> String {
        let p = b64url(payload_json.as_bytes());
        format!("{p}.{}", b64url(&hmac_sha256(secret.as_bytes(), p.as_bytes())))
    }

    const SECRET: &str = "engine-secret-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

    fn payload(role: &str, m: &str, p: &str, iat: i64) -> String {
        format!(
            r#"{{"v":1,"uid":"u1","sid":"s1","wid":"w1","wrole":"{role}","prole":"user","m":"{m}","p":"{p}","iat":{iat},"exp":{}}}"#,
            iat + 60
        )
    }

    #[test]
    fn verifies_a_good_header_and_rejects_everything_else() {
        let secret = Secret::new(SECRET).unwrap();
        let t = 1_800_000_000;
        let good = sign(SECRET, &payload("editor", "POST", "api/risk", t));
        let id = verify(&good, &secret, "POST", "/api/risk", t + 1).unwrap();
        assert_eq!(id.workspace_id, "w1");
        assert_eq!(id.workspace_role, Role::Editor);
        assert!(!id.platform_admin);

        assert_eq!(
            verify(&good, &secret, "POST", "/api/stress", t),
            Err("identity header was issued for another request")
        );
        assert_eq!(
            verify(&good, &secret, "GET", "/api/risk", t),
            Err("identity header was issued for another request")
        );
        assert_eq!(verify(&good, &secret, "POST", "/api/risk", t + 66), Err("identity header expired"));
        assert_eq!(
            verify(&good, &secret, "POST", "/api/risk", t - 10),
            Err("identity header issued in the future")
        );
        let other = Secret::new("another-secret-yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy").unwrap();
        assert_eq!(verify(&good, &other, "POST", "/api/risk", t), Err("invalid identity signature"));

        // Re-signing a raised role without the secret is impossible; swapping payloads breaks the signature.
        let (_, sig) = good.split_once('.').unwrap();
        let forged = format!("{}.{sig}", b64url(payload("owner", "POST", "api/risk", t).as_bytes()));
        assert_eq!(verify(&forged, &secret, "POST", "/api/risk", t), Err("invalid identity signature"));

        for bad in ["", "abc", "a.b", "....", "eyJ2IjoxfQ.!!"] {
            assert!(verify(bad, &secret, "POST", "/api/risk", t).is_err(), "{bad}");
        }
        let unknown_role = sign(SECRET, &payload("god", "POST", "api/risk", t));
        assert_eq!(verify(&unknown_role, &secret, "POST", "/api/risk", t), Err("malformed identity header"));
    }

    #[test]
    fn roles_are_ordered_and_secrets_are_redacted() {
        assert!(Role::Viewer < Role::Editor && Role::Editor < Role::Owner);
        assert!(Secret::new("short").is_none());
        assert_eq!(format!("{:?}", Secret::new(SECRET).unwrap()), "Secret(…)");
    }
}
