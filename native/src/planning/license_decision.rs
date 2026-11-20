//! Core-verified exact licence decision consumer.
//!
//! A model may propose a decision, but this module validates the exact identity,
//! source evidence, delegation, commercial/no-payment predicate, selected
//! branch and obligations against the shared evidence and the current policy.
//! The resulting `VerifiedLicenseDecision` is the only value the durable
//! decision writer accepts.

use serde::{Deserialize, Serialize};

use super::license_policy::{
    LicenseContext, LicenseDecision, default_catalog, default_profile, evaluate,
};

pub const VERSIONED_SOURCE_EVIDENCE_JSON: &str = include_str!(
    "../../../deploy/environment-builder/environment_builder/versioned_source_evidence.json"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LicenseEvidenceReview {
    pub decision: String,
    pub pin: String,
    pub name: String,
    pub version: String,
    pub output_drv_path: String,
    pub output_store_path: String,
    pub output_nar_hash: String,
    pub source_drv_path: String,
    pub source_store_path: String,
    pub source_output_hash: String,
    pub license_expression: String,
    pub selected_branch: String,
    pub documentation_license: Option<String>,
    pub use_context: String,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub obligations_reviewed: bool,
    pub obligations: Vec<String>,
    pub prior_deny_reconciled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyBinding {
    pub catalog_sha256: String,
    pub profile_sha256: String,
    pub policy_sha256: String,
    pub spec_sha256: String,
    pub inventory_sha256: String,
    pub candidate_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicenseDecision {
    pub identity: serde_json::Value,
    pub license_expression: String,
    pub selected_branch: String,
    pub documentation_license: Option<String>,
    pub evidence_sha256: String,
    pub provenance: serde_json::Value,
    pub use_context: String,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub obligations: Vec<String>,
    pub decision: String,
    pub revision: u64,
    pub binding: PolicyBinding,
}

fn exact<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn catalogue_obligations(identifier: &str) -> Option<Vec<String>> {
    default_catalog()
        .entries
        .get(identifier)
        .map(|entry| entry.obligations.to_vec())
}

/// Validate one model review against the exact shared evidence, the Owner
/// delegation and the current policy binding. The result is still not an
/// authority by itself; it is the exact payload Core may persist.
pub fn validate_license_evidence_review(
    review: &LicenseEvidenceReview,
    delegation: &serde_json::Value,
    binding: &PolicyBinding,
) -> Result<VerifiedLicenseDecision, String> {
    if !matches!(
        review.decision.as_str(),
        "ACCEPT" | "NEEDS_EVIDENCE" | "DENY"
    ) {
        return Err("licence review decision is invalid".into());
    }
    let asset: serde_json::Value =
        serde_json::from_str(VERSIONED_SOURCE_EVIDENCE_JSON).map_err(|error| error.to_string())?;
    let identities = asset
        .get("identities")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "shared evidence has no identities".to_string())?;
    let identity = identities
        .iter()
        .find(|identity| {
            exact(identity, "pin") == Some(review.pin.as_str())
                && exact(identity, "name") == Some(review.name.as_str())
                && exact(identity, "version") == Some(review.version.as_str())
        })
        .ok_or_else(|| "review identity is not in the shared evidence".to_string())?;
    let output = identity
        .get("output")
        .ok_or_else(|| "shared evidence has no output".to_string())?;
    let source = identity
        .get("source")
        .ok_or_else(|| "shared evidence has no source".to_string())?;
    for (label, actual, expected) in [
        (
            "output drv",
            review.output_drv_path.as_str(),
            exact(output, "drvPath"),
        ),
        (
            "output path",
            review.output_store_path.as_str(),
            exact(output, "storePath"),
        ),
        (
            "output NAR",
            review.output_nar_hash.as_str(),
            exact(output, "narHash"),
        ),
        (
            "source drv",
            review.source_drv_path.as_str(),
            exact(source, "drvPath"),
        ),
        (
            "source path",
            review.source_store_path.as_str(),
            exact(source, "storePath"),
        ),
        (
            "source output hash",
            review.source_output_hash.as_str(),
            exact(source, "outputHash"),
        ),
    ] {
        if expected != Some(actual) {
            return Err(format!("{label} does not match the exact shared evidence"));
        }
    }
    let evidence_sha256 = asset
        .get("evidence")
        .and_then(|evidence| evidence.get("licenseEvidenceCanonicalSha256"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "shared evidence has no canonical licence hash".to_string())?;
    if !is_sha256(evidence_sha256) {
        return Err("shared licence evidence hash is malformed".into());
    }
    if !is_sha256(&binding.catalog_sha256)
        || !is_sha256(&binding.profile_sha256)
        || !is_sha256(&binding.policy_sha256)
        || !is_sha256(&binding.spec_sha256)
        || !is_sha256(&binding.inventory_sha256)
        || !is_sha256(&binding.candidate_sha256)
    {
        return Err("policy/spec/inventory/candidate binding is incomplete".into());
    }
    let delegation_domain =
        exact(delegation, "domain").ok_or_else(|| "delegation has no domain".to_string())?;
    if delegation_domain != "licenses-identity-evidence" {
        return Err("delegation is outside the licence/identity/evidence domain".into());
    }
    if exact(delegation, "pin") != Some(review.pin.as_str()) {
        return Err("delegation pin differs from the exact identity".into());
    }
    if exact(delegation, "useContext") != Some(review.use_context.as_str()) {
        return Err("delegation use context differs from the review".into());
    }
    if exact(delegation, "economicCriterion") != Some("no-mandatory-fee-commercial-use") {
        return Err("delegation economic criterion is missing".into());
    }
    let delegation_revision = delegation
        .get("revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if delegation_revision == 0 {
        return Err("delegation revision is missing".into());
    }
    if review.license_expression.trim().is_empty()
        || review.selected_branch.trim().is_empty()
        || review.use_context.trim().is_empty()
    {
        return Err("licence expression, selected branch or use context is empty".into());
    }
    if review.decision == "ACCEPT"
        && (review.payment_required || !review.commercial_use || !review.obligations_reviewed)
    {
        return Err("commercial use without a payment obligation is not established".into());
    }
    let software_decision = evaluate(
        &review.license_expression,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    );
    if software_decision == LicenseDecision::Denied {
        let reconciled = delegation
            .get("reconcilePriorDeny")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !review.prior_deny_reconciled || !reconciled {
            return Err("a denied licence requires an explicit reconciled Owner delegation".into());
        }
    }
    if software_decision == LicenseDecision::Unknown {
        let classified = catalogue_obligations(&review.license_expression).is_some();
        let allowed_unknown = delegation
            .get("allowClassifiedUnknown")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !classified || !allowed_unknown {
            return Err(
                "an unclassified/unknown licence requires an explicit exact delegation".into(),
            );
        }
    }
    let expected_obligations = catalogue_obligations(&review.selected_branch)
        .ok_or_else(|| "selected branch is not a classified licence".to_string())?;
    if review.obligations != expected_obligations {
        return Err("obligations do not exactly match the selected branch/context".into());
    }
    if let Some(documentation) = &review.documentation_license {
        let documented = identity
            .get("documentationLicense")
            .and_then(serde_json::Value::as_str);
        if documented != Some(documentation.as_str()) {
            return Err("documentation licence differs from the exact evidence".into());
        }
    } else if identity.get("documentationLicense").is_some() {
        return Err("documentation licence is required by the exact evidence".into());
    }
    Ok(VerifiedLicenseDecision {
        identity: identity.clone(),
        license_expression: review.license_expression.clone(),
        selected_branch: review.selected_branch.clone(),
        documentation_license: review.documentation_license.clone(),
        evidence_sha256: evidence_sha256.to_owned(),
        provenance: serde_json::json!({
            "pin": review.pin,
            "outputDrvPath": review.output_drv_path,
            "outputStorePath": review.output_store_path,
            "outputNarHash": review.output_nar_hash,
            "sourceDrvPath": review.source_drv_path,
            "sourceStorePath": review.source_store_path,
            "sourceOutputHash": review.source_output_hash,
            "licenseEvidenceSha256": evidence_sha256,
        }),
        use_context: review.use_context.clone(),
        commercial_use: review.commercial_use,
        payment_required: review.payment_required,
        obligations: review.obligations.clone(),
        decision: review.decision.clone(),
        revision: delegation_revision,
        binding: binding.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> PolicyBinding {
        PolicyBinding {
            catalog_sha256: "a".repeat(64),
            profile_sha256: "b".repeat(64),
            policy_sha256: "c".repeat(64),
            spec_sha256: "d".repeat(64),
            inventory_sha256: "e".repeat(64),
            candidate_sha256: "f".repeat(64),
        }
    }

    fn delegation() -> serde_json::Value {
        serde_json::json!({
            "domain": "licenses-identity-evidence",
            "pin": "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d",
            "useContext": "commercial-development",
            "economicCriterion": "no-mandatory-fee-commercial-use",
            "revision": 1,
            "allowClassifiedUnknown": true,
            "reconcilePriorDeny": false
        })
    }

    fn review() -> LicenseEvidenceReview {
        LicenseEvidenceReview {
            decision: "ACCEPT".into(),
            pin: "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d".into(),
            name: "ed".into(),
            version: "1.21.1".into(),
            output_drv_path: "/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv".into(),
            output_store_path: "/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1".into(),
            output_nar_hash: "sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=".into(),
            source_drv_path: "/nix/store/j151nb5vh176kdrrh1rkilxkdkmvm47w-ed-1.21.1.tar.lz.drv"
                .into(),
            source_store_path: "/nix/store/wa88qds9615j2r5imfryzbyg86c97iga-ed-1.21.1.tar.lz"
                .into(),
            source_output_hash: "sha256-1tDHGSsCsFGckCqTcZBT6GWt5aeEo7Mn2T2IhFeyPEs=".into(),
            license_expression: "GPL-2.0-or-later".into(),
            selected_branch: "GPL-2.0-or-later".into(),
            documentation_license: Some("GFDL-1.3-or-later".into()),
            use_context: "commercial-development".into(),
            commercial_use: true,
            payment_required: false,
            obligations_reviewed: true,
            obligations: vec!["notice".into(), "source".into()],
            prior_deny_reconciled: false,
        }
    }

    #[test]
    fn exact_evidence_with_classified_gfdl_and_delegation_is_accepted() {
        let decision = validate_license_evidence_review(&review(), &delegation(), &binding())
            .expect("exact evidence");
        assert_eq!(decision.selected_branch, "GPL-2.0-or-later");
        assert_eq!(decision.obligations, ["notice", "source"]);
    }

    #[test]
    fn wrong_identity_payment_or_obligations_fail_closed() {
        let mut wrong = review();
        wrong.output_store_path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-ed-1.21.1".into();
        assert!(validate_license_evidence_review(&wrong, &delegation(), &binding()).is_err());
        let mut paid = review();
        paid.payment_required = true;
        assert!(validate_license_evidence_review(&paid, &delegation(), &binding()).is_err());
        let mut obligations = review();
        obligations.obligations = vec!["notice".into()];
        assert!(validate_license_evidence_review(&obligations, &delegation(), &binding()).is_err());
        let mut missing = review();
        missing.documentation_license = None;
        assert!(validate_license_evidence_review(&missing, &delegation(), &binding()).is_err());
    }

    #[test]
    fn agpl_requires_an_explicit_reconciled_delegation() {
        let mut legacy = review();
        legacy.license_expression = "AGPL-3.0-only".into();
        legacy.selected_branch = "AGPL-3.0-only".into();
        legacy.obligations = catalogue_obligations("AGPL-3.0-only").unwrap();
        assert!(validate_license_evidence_review(&legacy, &delegation(), &binding()).is_err());
        let mut reconciled = delegation();
        reconciled["reconcilePriorDeny"] = serde_json::json!(true);
        legacy.prior_deny_reconciled = true;
        assert!(validate_license_evidence_review(&legacy, &reconciled, &binding()).is_ok());
    }
}
