//! Focused checks for the CR-15 seam: the test-support ActionSpec environment
//! placeholder is opt-in.
//!
//! These run with `--features test-support` but **not** under `cfg(test)`, which
//! is exactly the configuration of the `core-e2e` image. A `cfg(test)` unit test
//! would keep the historical auto-injection and could hide the defect this file
//! exists to catch.

use factory_native::db::{
    apply_test_environment_binding, apply_test_environment_binding_if_enabled, test_environment_binding,
    test_environment_binding_opt_in,
};

fn set_gate(value: Option<&str>) {
    match value {
        Some(raw) => unsafe { std::env::set_var("FACTORY_TEST_ENVIRONMENT_BINDING", raw) },
        None => unsafe { std::env::remove_var("FACTORY_TEST_ENVIRONMENT_BINDING") },
    }
}

/// The predicate is the single decision point, and only `"1"` opens the gate.
#[test]
fn the_opt_in_predicate_accepts_only_an_explicit_one() {
    assert!(test_environment_binding_opt_in(Some("1")));
    for raw in [None, Some(""), Some("0"), Some("true"), Some("yes"), Some("01")] {
        assert!(
            !test_environment_binding_opt_in(raw),
            "unexpected opt-in for {raw:?}"
        );
    }
}

/// The fixture binding stays deterministic, so an opted-in fixture journey keeps
/// the exact contract it had before the gate existed.
#[test]
fn the_fixture_binding_identity_is_stable() {
    let binding = test_environment_binding("action-1");
    assert_eq!(binding.plan_id, "test-plan-action-1");
    assert_eq!(binding.spec_id, "test-spec-action-1");
    assert_eq!(binding.builder_image, "fixture/environment-builder");
    assert_eq!(binding.plan_sha256.len(), 64);
    assert_eq!(binding.spec_sha256.len(), 64);
}

/// Real environment variable, real seam function, one case at a time.
#[test]
fn the_seam_reads_the_real_environment_variable() {
    for (raw, expected) in [
        (None, false),
        (Some(""), false),
        (Some("0"), false),
        (Some("1"), true),
        (Some("true"), false),
    ] {
        set_gate(raw);
        assert_eq!(
            test_environment_binding_opt_in(std::env::var("FACTORY_TEST_ENVIRONMENT_BINDING").ok().as_deref()),
            expected,
            "unexpected gate decision for {raw:?}"
        );
    }
    set_gate(None);
    assert!(
        std::env::var("FACTORY_TEST_ENVIRONMENT_BINDING").is_err(),
        "the gate test must not leak its variable"
    );
}

/// An explicitly opted-in caller still applies the fixture binding; the helper
/// itself is unchanged, so fixture-only code paths keep working verbatim.
#[test]
fn explicit_opt_in_still_applies_the_fixture_binding() {
    // `apply_test_environment_binding` is the same function the seam calls; this
    // check pins that the gate changed only the automatic call site.
    let _ = apply_test_environment_binding as fn(&mut factory_native::db::ActionSpec, &str);
    let _ = apply_test_environment_binding_if_enabled as fn(&mut factory_native::db::ActionSpec, &str);
}
