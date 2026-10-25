//! Core-signed grants that bind a Supervisor workspace allocation to one
//! authorized Attempt lease. Possession of the Supervisor admin bearer alone is
//! not enough to allocate an arbitrary workspace.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MIN_KEY_BYTES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptGrantClaims {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub repository: String,
    pub base_commit: String,
    pub action_spec_sha256: String,
    pub expires_at_ms: i64,
}

pub fn sign(key: &[u8], claims: &AttemptGrantClaims) -> Result<String, String> {
    if key.len() < MIN_KEY_BYTES {
        return Err("Attempt grant key must be at least 32 bytes".into());
    }
    let payload = serde_json::to_vec(claims).map_err(|_| "Attempt grant claims are invalid")?;
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&key, &payload);
    let envelope = serde_json::to_vec(&serde_json::json!({
        "claims": claims,
        "signature": hex(tag.as_ref())
    }))
    .map_err(|_| "Attempt grant claims are invalid")?;
    Ok(hex(&envelope))
}

pub fn parse(grant: &str) -> Result<AttemptGrantClaims, String> {
    if grant.is_empty() || grant.len() > 2048 {
        return Err("Attempt grant is malformed".into());
    }
    let bytes = decode_hex(grant)?;
    #[derive(serde::Deserialize)]
    struct Envelope {
        claims: AttemptGrantClaims,
        signature: String,
    }
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| "Attempt grant is malformed")?;
    if envelope.signature.len() != 64
        || !envelope
            .signature
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Attempt grant signature is malformed".into());
    }
    Ok(envelope.claims)
}

pub fn signature(grant: &str) -> Result<String, String> {
    let bytes = decode_hex(grant)?;
    #[derive(serde::Deserialize)]
    struct Envelope {
        signature: String,
    }
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| "Attempt grant is malformed")?;
    Ok(envelope.signature)
}

pub fn verify(
    key: &[u8],
    grant: &str,
    expected: &AttemptGrantClaims,
    now_ms: i64,
) -> Result<(), String> {
    if key.len() < MIN_KEY_BYTES {
        return Err("Attempt grant key must be at least 32 bytes".into());
    }
    if expected.expires_at_ms <= now_ms {
        return Err("Attempt grant has expired".into());
    }
    let payload = serde_json::to_vec(expected).map_err(|_| "Attempt grant claims are invalid")?;
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let expected_tag = ring::hmac::sign(&key, &payload);
    let expected_hex = hex(expected_tag.as_ref());
    let actual = signature(grant)?;
    if !constant_time_eq(actual.as_bytes(), expected_hex.as_bytes()) {
        return Err("Attempt grant signature is invalid".into());
    }
    Ok(())
}

pub fn workspace_capability(
    key: &[u8],
    workspace_id: &str,
    attempt_id: &str,
    expires_at_ms: i64,
) -> Result<String, String> {
    if key.len() < MIN_KEY_BYTES {
        return Err("Workspace capability key must be at least 32 bytes".into());
    }
    let payload =
        format!("factory-workspace-capability-v1\0{workspace_id}\0{attempt_id}\0{expires_at_ms}");
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&key, payload.as_bytes());
    Ok(hex(tag.as_ref()))
}

pub fn verify_workspace_capability(
    key: &[u8],
    capability: &str,
    workspace_id: &str,
    attempt_id: &str,
    expires_at_ms: i64,
    now_ms: i64,
) -> Result<(), String> {
    if expires_at_ms <= now_ms {
        return Err("Workspace capability has expired".into());
    }
    let expected = workspace_capability(key, workspace_id, attempt_id, expires_at_ms)?;
    if !constant_time_eq(capability.as_bytes(), expected.as_bytes()) {
        return Err("Workspace capability is invalid".into());
    }
    Ok(())
}

pub fn digest(claims: &AttemptGrantClaims) -> Result<String, String> {
    let payload = serde_json::to_vec(claims).map_err(|_| "Attempt grant claims are invalid")?;
    Ok(hex(Sha256::digest(&payload).as_ref()))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) || value.len() > 4096 {
        return Err("Attempt grant is malformed".into());
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text =
                std::str::from_utf8(pair).map_err(|_| "Attempt grant is malformed".to_string())?;
            u8::from_str_radix(text, 16).map_err(|_| "Attempt grant is malformed".to_string())
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims() -> AttemptGrantClaims {
        AttemptGrantClaims {
            attempt_id: "attempt-a".into(),
            worker_id: "worker-a".into(),
            generation: 3,
            repository: "/projects/alpha".into(),
            base_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            action_spec_sha256: "a".repeat(64),
            expires_at_ms: 2_000,
        }
    }

    #[test]
    fn grant_is_bound_to_every_claim_and_expiry() {
        let key = b"attempt-grant-key-0123456789abcdef";
        let grant = sign(key, &claims()).unwrap();
        assert!(verify(key, &grant, &claims(), 1_000).is_ok());
        assert!(verify(key, &grant, &claims(), 2_000).is_err());
        let mut changed = claims();
        changed.attempt_id = "attempt-b".into();
        assert!(verify(key, &grant, &changed, 1_000).is_err());
        let mut changed = claims();
        changed.base_commit = "f".repeat(40);
        assert!(verify(key, &grant, &changed, 1_000).is_err());
        assert!(verify(b"short", &grant, &claims(), 1_000).is_err());
        assert!(verify(key, "not-hex", &claims(), 1_000).is_err());
    }
}
