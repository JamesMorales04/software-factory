//! Broker-observed helper facts, authenticated for Core (ADR-0045).
//!
//! The runtime broker is the only component that owns the nested engine and can
//! observe a realised closure. It produces this envelope from its own query, not
//! from builder self-report, and authenticates it with a key derived from the
//! existing scoped environment bearer that only Core and the broker hold. The
//! builder never sees the key and never signs anything.
//!
//! Derivation context: `factory.environment.helper-facts.v1`. The context is part
//! of the key schedule, so the shared bearer is never used raw for two purposes,
//! and the same context is re-checked on verification.

use ring::hmac;
use serde::{Deserialize, Serialize};

use super::helper_exceptions::{HelperObservation, TrustedStoreIdentity};

/// Key-derivation and envelope context. Changing it invalidates every receipt.
pub const HELPER_FACTS_CONTEXT: &str = "factory.environment.helper-facts.v1";
pub const HELPER_FACTS_SCHEMA_VERSION: u32 = 1;
pub const MAX_HELPER_FACTS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HelperFactsEnvelope {
    pub schema_version: u32,
    pub context: String,
    /// Core-issued operation this observation belongs to.
    pub operation_id: String,
    /// Durable replay key: consumed once by Core.
    pub nonce: String,
    pub pin: String,
    pub plan_sha256: String,
    pub runtime_id: String,
    /// The stdenv and wrapper the broker actually observed, anchored by `.drv`.
    pub stdenv: TrustedStoreIdentity,
    pub shell_wrapper: TrustedStoreIdentity,
    pub observations: Vec<HelperObservation>,
}

/// What Core expects the receipt to be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelperFactsBinding<'a> {
    pub operation_id: &'a str,
    pub pin: &'a str,
    pub plan_sha256: &'a str,
    pub runtime_id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperFactsError {
    InvalidEnvelope,
    ContextMismatch,
    BindingMismatch,
    SignatureMismatch,
    TooManyObservations,
}

impl std::fmt::Display for HelperFactsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEnvelope => "helper facts envelope is not a bounded record",
            Self::ContextMismatch => "helper facts context does not match",
            Self::BindingMismatch => "helper facts are bound to another operation",
            Self::SignatureMismatch => "helper facts signature does not verify",
            Self::TooManyObservations => "helper facts exceed the observation bound",
        })
    }
}

impl std::error::Error for HelperFactsError {}

/// Derive the context-separated MAC key from the shared environment bearer.
///
/// The bearer itself is never used as a raw MAC key: the context is the HMAC
/// message and the bearer is the HMAC key, so the derived key is specific to this
/// purpose.
pub fn derive_key(bearer: &[u8]) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, bearer);
    let tag = hmac::sign(&key, HELPER_FACTS_CONTEXT.as_bytes());
    let mut derived = [0u8; 32];
    derived.copy_from_slice(&tag.as_ref()[..32]);
    derived
}

fn canonical_bytes(envelope: &HelperFactsEnvelope) -> Result<Vec<u8>, HelperFactsError> {
    serde_json::to_vec(envelope).map_err(|_| HelperFactsError::InvalidEnvelope)
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate(envelope: &HelperFactsEnvelope) -> Result<(), HelperFactsError> {
    if envelope.schema_version != HELPER_FACTS_SCHEMA_VERSION
        || envelope.operation_id.trim().is_empty()
        || envelope.operation_id.len() > 200
        || envelope.nonce.trim().is_empty()
        || envelope.nonce.len() > 200
        || envelope.pin.len() > 200
        || !is_hex64(&envelope.plan_sha256)
        || envelope.runtime_id.trim().is_empty()
        || envelope.runtime_id.len() > 200
        || envelope.observations.len() > MAX_HELPER_FACTS
    {
        return Err(HelperFactsError::InvalidEnvelope);
    }
    Ok(())
}

/// Sign an envelope with the derived key (broker side).
pub fn sign(envelope: &HelperFactsEnvelope, bearer: &[u8]) -> Result<String, HelperFactsError> {
    validate(envelope)?;
    let bytes = canonical_bytes(envelope)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &derive_key(bearer));
    let tag = hmac::sign(&key, &bytes);
    let mut output = String::with_capacity(64);
    for byte in tag.as_ref() {
        output.push_str(&format!("{byte:02x}"));
    }
    Ok(output)
}

/// Verify a receipt (Core side): signature, context and operation binding.
pub fn verify(
    envelope: &HelperFactsEnvelope,
    bearer: &[u8],
    signature: &str,
    binding: HelperFactsBinding<'_>,
) -> Result<(), HelperFactsError> {
    validate(envelope)?;
    if envelope.context != HELPER_FACTS_CONTEXT {
        return Err(HelperFactsError::ContextMismatch);
    }
    if envelope.operation_id != binding.operation_id
        || envelope.pin != binding.pin
        || envelope.plan_sha256 != binding.plan_sha256
        || envelope.runtime_id != binding.runtime_id
    {
        return Err(HelperFactsError::BindingMismatch);
    }
    if signature.len() != 64 || !signature.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(HelperFactsError::SignatureMismatch);
    }
    let mut tag = [0u8; 32];
    for (index, chunk) in signature.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|_| HelperFactsError::SignatureMismatch)?;
        tag[index] =
            u8::from_str_radix(text, 16).map_err(|_| HelperFactsError::SignatureMismatch)?;
    }
    let bytes = canonical_bytes(envelope)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &derive_key(bearer));
    hmac::verify(&key, &bytes, &tag).map_err(|_| HelperFactsError::SignatureMismatch)
}

/// Durable replay identity for `environment_build_receipts.id`.
pub fn receipt_identity(envelope: &HelperFactsEnvelope) -> String {
    format!("helper-facts:{}", envelope.nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEARER: &[u8] = b"scoped-environment-bearer-0123456789abcdef";
    const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
    const STDENV: &str = "/nix/store/lfwfj17y9fpjb73nsj2m35rmkh587a0x-stdenv-linux";
    const SHELL: &str = "/nix/store/xp720k4l3krsg7fjdjhpv45300ghgr6s-nix-shell-env";
    const HELPER: &str = "/nix/store/cickvswrvann041nqxb0rxilc46svw1n-prune-libtool-files.sh";

    fn envelope() -> HelperFactsEnvelope {
        HelperFactsEnvelope {
            schema_version: HELPER_FACTS_SCHEMA_VERSION,
            context: HELPER_FACTS_CONTEXT.into(),
            operation_id: "op-1".into(),
            nonce: "nonce-1".into(),
            pin: PIN.into(),
            plan_sha256: "a".repeat(64),
            runtime_id: "runtime-1".into(),
            stdenv: TrustedStoreIdentity {
                store_path: STDENV.into(),
                deriver: "/nix/store/nsvrknrqxdf2lxcfbqkr0p05897ww5lx-stdenv-linux.drv".into(),
                nar_hash: "sha256-7uyA5WT1X+9o1QV/Dqs7GsmWtellurWH5ipGyZK8Szk=".into(),
            },
            shell_wrapper: TrustedStoreIdentity {
                store_path: SHELL.into(),
                deriver: "/nix/store/gkp675qsx7wg4rij541kzmi49skfzsnv-nix-shell-env.drv".into(),
                nar_hash: "sha256-lH0a8hOLzQfqRQDAP8xrNxGxwVW7EcjXyDOakbb+MCw=".into(),
            },
            observations: vec![HelperObservation {
                store_path: HELPER.into(),
                nar_hash: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                deriver: String::new(),
                regular_file: true,
                references: vec![STDENV.into(), SHELL.into()],
            }],
        }
    }

    fn binding_for(envelope: &HelperFactsEnvelope) -> HelperFactsBinding<'_> {
        HelperFactsBinding {
            operation_id: "op-1",
            pin: PIN,
            plan_sha256: &envelope.plan_sha256,
            runtime_id: "runtime-1",
        }
    }

    #[test]
    fn a_broker_receipt_verifies_for_its_own_binding() {
        let envelope = envelope();
        let signature = sign(&envelope, BEARER).unwrap();
        verify(&envelope, BEARER, &signature, binding_for(&envelope)).unwrap();
        assert_eq!(receipt_identity(&envelope), "helper-facts:nonce-1");
    }

    #[test]
    fn tampered_facts_do_not_verify() {
        let address = envelope();
        let signature = sign(&address, BEARER).unwrap();
        let mut tampered = address.clone();
        tampered.observations[0].nar_hash = "sha256-OTHER".into();
        assert_eq!(
            verify(&tampered, BEARER, &signature, binding_for(&address)),
            Err(HelperFactsError::SignatureMismatch)
        );
    }

    #[test]
    fn another_operation_pin_plan_or_runtime_does_not_verify() {
        let envelope = envelope();
        let signature = sign(&envelope, BEARER).unwrap();
        let other_plan = "b".repeat(64);
        for broken in [
            HelperFactsBinding {
                operation_id: "op-2",
                pin: PIN,
                plan_sha256: &envelope.plan_sha256,
                runtime_id: "runtime-1",
            },
            HelperFactsBinding {
                operation_id: "op-1",
                pin: "github:NixOS/nixpkgs/other",
                plan_sha256: &envelope.plan_sha256,
                runtime_id: "runtime-1",
            },
            HelperFactsBinding {
                operation_id: "op-1",
                pin: PIN,
                plan_sha256: &other_plan,
                runtime_id: "runtime-1",
            },
            HelperFactsBinding {
                operation_id: "op-1",
                pin: PIN,
                plan_sha256: &envelope.plan_sha256,
                runtime_id: "runtime-2",
            },
        ] {
            assert_eq!(
                verify(&envelope, BEARER, &signature, broken),
                Err(HelperFactsError::BindingMismatch)
            );
        }
    }

    #[test]
    fn another_context_or_bearer_does_not_verify() {
        let envelope = envelope();
        let signature = sign(&envelope, BEARER).unwrap();
        let mut wrong_context = envelope.clone();
        wrong_context.context = "factory.environment.helper-facts.v2".into();
        assert_eq!(
            verify(&wrong_context, BEARER, &signature, binding_for(&envelope)),
            Err(HelperFactsError::ContextMismatch)
        );
        assert_eq!(
            verify(
                &envelope,
                b"another-bearer-0123456789abcdefghij",
                &signature,
                binding_for(&envelope)
            ),
            Err(HelperFactsError::SignatureMismatch)
        );
    }

    #[test]
    fn a_forged_or_malformed_signature_is_rejected() {
        let envelope = envelope();
        let binding = binding_for(&envelope);
        assert_eq!(
            verify(&envelope, BEARER, &"0".repeat(64), binding),
            Err(HelperFactsError::SignatureMismatch)
        );
        assert_eq!(
            verify(&envelope, BEARER, "not-a-signature", binding),
            Err(HelperFactsError::SignatureMismatch)
        );
    }

    #[test]
    fn the_context_derived_key_is_not_the_raw_bearer() {
        assert_ne!(derive_key(BEARER).to_vec(), BEARER.to_vec());
        assert_eq!(derive_key(BEARER), derive_key(BEARER));
        assert_ne!(
            derive_key(BEARER),
            derive_key(b"another-bearer-0123456789abcdefghij")
        );
    }

    #[test]
    fn a_replayed_nonce_is_refused_by_the_durable_identity() {
        let envelope = envelope();
        let identity = receipt_identity(&envelope);
        let mut consumed = std::collections::BTreeSet::new();
        assert!(consumed.insert(identity.clone()));
        // `environment_build_receipts.id` is the primary key: the second insert
        // for the same durable identity fails, so this check mirrors it.
        assert!(!consumed.insert(identity));
    }
}
