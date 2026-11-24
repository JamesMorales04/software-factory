//! Test-support policy probe for the `core-e2e` image only.
//!
//! It exercises the compiled licence evaluator and the real
//! `EnvironmentPlan::validate` with a valid inventory and with stale/forged
//! catalogue/profile hashes on throw-away state. It is not the production
//! `factory-core` binary and is never built into the production Core image.

#[cfg(not(feature = "test-support"))]
fn main() {
    eprintln!("factory-policy-probe is test-support only");
    std::process::exit(2);
}

#[cfg(feature = "test-support")]
fn main() {
    use factory_native::planning::environment::{EnvironmentBlockerCode, policy_probe_plan};
    use factory_native::planning::license_policy::{
        LicenseContext, binding_sha256, default_catalog, default_profile, evaluate,
    };

    let catalog = default_catalog();
    let profile = default_profile();
    let expected_catalog = "725ba589f774e2f1415534e2ee37157ae9fa1ecacfc956a462529d167297cd36";
    let expected_profile = "233c0a4498f6b0b04dac494d596b160766b80f64d3ad0ff020e8e4796ac22aac";
    let expected_binding = "a020b9b854a4bc63cabdc61c236499f1166cd3f4fc1abed9445a0c0a15c20e65";

    let mut failures = Vec::new();
    if catalog.sha256 != expected_catalog {
        failures.push(format!(
            "catalog hash {} != {expected_catalog}",
            catalog.sha256
        ));
    }
    if profile.sha256 != expected_profile {
        failures.push(format!(
            "profile hash {} != {expected_profile}",
            profile.sha256
        ));
    }
    if binding_sha256(catalog, profile) != expected_binding {
        failures.push("binding hash mismatch".to_owned());
    }

    let mut matrix = serde_json::Map::new();
    for context in [
        LicenseContext::Selected,
        LicenseContext::Closure,
        LicenseContext::Build,
        LicenseContext::DevTest,
        LicenseContext::RuntimeDistributed,
    ] {
        let mut row = serde_json::Map::new();
        for identifier in catalog.entries.keys() {
            row.insert(
                identifier.clone(),
                serde_json::json!(evaluate(identifier, context, catalog, profile).as_str()),
            );
        }
        matrix.insert(context.as_str().to_owned(), serde_json::Value::Object(row));
    }

    let mut valid_plan = match policy_probe_plan() {
        Ok(plan) => plan,
        Err(error) => {
            failures.push(format!("probe plan build failed: {error}"));
            print_result(&matrix, &failures);
            std::process::exit(1);
        }
    };
    let valid_plan_ok = match valid_plan.validate(chrono::Utc::now()) {
        Ok(()) => true,
        Err(error) => {
            failures.push(format!("valid plan rejected: {error}"));
            false
        }
    };

    let mut stale_catalog_rejected = false;
    let mut stale_profile_rejected = false;
    for field in ["catalog", "profile"] {
        let mut tampered = valid_plan.clone();
        if field == "catalog" {
            tampered.license_inventory.catalog_sha256 = "0".repeat(64);
        } else {
            tampered.license_inventory.profile_sha256 = "0".repeat(64);
        }
        tampered.license_inventory.refresh_hashes(&tampered.plan_id);
        tampered.license_inventory_sha256 = tampered.license_inventory.inventory_sha256.clone();
        tampered.refresh_hash();
        match tampered.validate(chrono::Utc::now()) {
            Err(error) if error.code == EnvironmentBlockerCode::EnvironmentHashMismatch => {
                if field == "catalog" {
                    stale_catalog_rejected = true;
                } else {
                    stale_profile_rejected = true;
                }
            }
            other => failures.push(format!(
                "{field} stale inventory was not rejected: {other:?}"
            )),
        }
    }
    valid_plan.refresh_hash();

    let output = serde_json::json!({
        "probe": "factory-policy-probe",
        "features": ["test-support"],
        "catalogSha256": catalog.sha256,
        "profileSha256": profile.sha256,
        "bindingSha256": binding_sha256(catalog, profile),
        "matrix": serde_json::Value::Object(matrix),
        "validPlan": valid_plan_ok,
        "staleCatalogRejected": stale_catalog_rejected,
        "staleProfileRejected": stale_profile_rejected,
        "failures": failures,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("probe output serializes")
    );
    if !output["failures"]
        .as_array()
        .is_some_and(|failures| failures.is_empty())
    {
        std::process::exit(1);
    }
}

#[cfg(feature = "test-support")]
fn print_result(matrix: &serde_json::Map<String, serde_json::Value>, failures: &[String]) {
    let output = serde_json::json!({
        "probe": "factory-policy-probe",
        "features": ["test-support"],
        "matrix": serde_json::Value::Object(matrix.clone()),
        "failures": failures,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("probe output serializes")
    );
}
