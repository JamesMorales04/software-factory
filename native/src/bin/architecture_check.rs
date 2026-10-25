//! Native governance checks for ADR supersession and capability/roadmap consistency.
//!
//! Checks the capability registry, its roadmap projection, evidence references,
//! certification claims, and append-only ADR history.

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Milestone {
    id: String,
    title: String,
    #[serde(rename = "decision_status")]
    decision: String,
    dependencies: Vec<String>,
    deliverables: Vec<String>,
    capability_ids: Vec<String>,
    exclusions: Vec<String>,
    risks: Vec<String>,
    deterministic_acceptance: Vec<String>,
    real_acceptance: Vec<String>,
    owner_decisions: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Capability {
    id: String,
    name: String,
    category: String,
    purpose: String,
    product_objective: String,
    #[serde(rename = "decision_status")]
    decision: String,
    #[serde(rename = "implementation_status")]
    implementation: String,
    verification: Verification,
    owner: String,
    dependencies: Vec<String>,
    target_milestone: String,
    refs: References,
    acceptance: Acceptance,
    evidence: Evidence,
    limitations: Vec<String>,
    blockers: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Verification {
    deterministic: String,
    live_agent: String,
    uat: String,
    deployment: String,
    isolation: String,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct References {
    adr: Vec<String>,
    contract: Vec<String>,
    source: Vec<String>,
    test: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Acceptance {
    deterministic: String,
    real: String,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Evidence {
    deterministic: Vec<String>,
    live_agent: Vec<String>,
    uat: Vec<String>,
    deployment: Vec<String>,
    isolation: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Registry {
    schema_version: u32,
    milestones: Vec<Milestone>,
    capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Deserialize)]
struct CertificationManifest {
    level: String,
    status: String,
    implementation_sha: String,
    candidate_sha: String,
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("architecture-check: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut root = None;
    let mut base_ref = None;
    let mut registry_path = PathBuf::from("docs/product/capabilities.yaml");
    let mut roadmap_path = PathBuf::from("docs/roadmap/ROADMAP.md");
    let mut write_roadmap = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = Some(PathBuf::from(args.next().ok_or("--root requires a path")?)),
            "--base-ref" => base_ref = Some(args.next().ok_or("--base-ref requires a Git ref")?),
            "--registry" => {
                registry_path = PathBuf::from(args.next().ok_or("--registry requires a path")?)
            }
            "--roadmap" => {
                roadmap_path = PathBuf::from(args.next().ok_or("--roadmap requires a path")?)
            }
            "--write-roadmap" => write_roadmap = true,
            "-h" | "--help" => {
                println!(
                    "usage: factory-architecture-check --root PATH [--registry PATH] [--roadmap PATH] [--write-roadmap] [--base-ref REF]"
                );
                println!(
                    "Validates capability references and evidence, generated roadmap, ADR supersession and accepted ADR history."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let root = root.ok_or("--root is required")?;
    check_repository(
        &root,
        &registry_path,
        &roadmap_path,
        write_roadmap,
        base_ref.as_deref(),
    )
}

fn check_repository(
    root: &Path,
    registry_relative: &Path,
    roadmap_relative: &Path,
    write_roadmap: bool,
    base_ref: Option<&str>,
) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve root {}: {error}", root.display()))?;
    let registry_path = root.join(registry_relative);
    let roadmap_path = root.join(roadmap_relative);
    let registry_text = fs::read_to_string(&registry_path)
        .map_err(|error| format!("read {}: {error}", registry_path.display()))?;
    let roadmap = if write_roadmap {
        String::new()
    } else {
        fs::read_to_string(&roadmap_path)
            .map_err(|error| format!("read {}: {error}", roadmap_path.display()))?
    };
    let decisions_path = root.join("docs/product/capability-decisions.md");
    let decisions = fs::read_to_string(&decisions_path)
        .map_err(|error| format!("read {}: {error}", decisions_path.display()))?;
    let registry = parse_registry(&registry_text)?;
    validate_registry(&root, &registry)?;
    validate_capability_preservation(&registry, &decisions)?;
    let expected = render_roadmap(&registry);
    if write_roadmap {
        if let Some(parent) = roadmap_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        fs::write(&roadmap_path, expected)
            .map_err(|error| format!("write {}: {error}", roadmap_path.display()))?;
    } else if roadmap != expected {
        let line = roadmap
            .lines()
            .zip(expected.lines())
            .position(|(actual, generated)| actual != generated)
            .unwrap_or_else(|| roadmap.lines().count().min(expected.lines().count()));
        let actual = roadmap.lines().nth(line).unwrap_or("<end of file>");
        let generated = expected.lines().nth(line).unwrap_or("<end of file>");
        return Err(format!(
            "roadmap is stale or differs from registry projection at line {} (actual: {actual:?}; expected: {generated:?})",
            line + 1
        ));
    }
    validate_adr_supersession(&root)?;
    if let Some(base_ref) = base_ref {
        validate_stable_capability_ids(&root, base_ref, &registry)?;
        validate_accepted_adr_history(&root, base_ref)?;
    }
    println!("architecture governance: PASS");
    Ok(())
}

fn validate_capability_preservation(registry: &Registry, decisions: &str) -> Result<(), String> {
    let mut missing = Vec::new();
    for capability in &registry.capabilities {
        if !decisions.contains(&format!("| {} |", capability.id)) {
            missing.push(capability.id.clone());
        }
    }
    for required in [
        "Lint, type, build and tests",
        "Other validation adapters",
        "Memory, context and simplification",
        "CAP-020 Librarian",
        "CAP-029 advisory incident diagnosis",
    ] {
        if !decisions.contains(required) {
            missing.push(required.to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "capability decision record is missing selected capabilities or opportunities: {}",
            missing.join(", ")
        ))
    }
}

fn parse_registry(contents: &str) -> Result<Registry, String> {
    serde_saphyr::from_str(contents).map_err(|error| format!("parse registry: {error}"))
}

fn validate_registry(root: &Path, registry: &Registry) -> Result<(), String> {
    let mut errors = Vec::new();
    if registry.schema_version != 1 {
        errors.push("schema_version must be 1".to_string());
    }
    let mut milestone_positions = BTreeMap::new();
    for (index, milestone) in registry.milestones.iter().enumerate() {
        if !valid_id(&milestone.id) {
            errors.push(format!("invalid milestone id {}", milestone.id));
        }
        if milestone_positions
            .insert(milestone.id.as_str(), index)
            .is_some()
        {
            errors.push(format!("duplicate milestone id {}", milestone.id));
        }
        if milestone.title.is_empty() || !valid_decision(&milestone.decision) {
            errors.push(format!(
                "milestone {} requires title and valid decision_status",
                milestone.id
            ));
        }
        for (field, values) in [
            ("deliverables", &milestone.deliverables),
            ("exclusions", &milestone.exclusions),
            ("risks", &milestone.risks),
            (
                "deterministic_acceptance",
                &milestone.deterministic_acceptance,
            ),
            ("real_acceptance", &milestone.real_acceptance),
            ("owner_decisions", &milestone.owner_decisions),
        ] {
            if values.is_empty() {
                errors.push(format!("milestone {} has empty {field}", milestone.id));
            }
        }
    }

    let mut capability_indices = BTreeMap::new();
    for (index, capability) in registry.capabilities.iter().enumerate() {
        if !valid_id(&capability.id) {
            errors.push(format!("invalid capability id {}", capability.id));
        }
        if capability_indices
            .insert(capability.id.as_str(), index)
            .is_some()
        {
            errors.push(format!("duplicate capability id {}", capability.id));
        }
    }
    let mut membership: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for milestone in &registry.milestones {
        for id in &milestone.capability_ids {
            membership.entry(id).or_default().push(&milestone.id);
        }
    }
    for capability in &registry.capabilities {
        if capability.name.is_empty()
            || capability.category.is_empty()
            || capability.purpose.is_empty()
            || capability.product_objective.is_empty()
            || capability.owner.is_empty()
            || !valid_decision(&capability.decision)
            || !matches!(
                capability.implementation.as_str(),
                "implemented" | "in_progress" | "not_implemented"
            )
        {
            errors.push(format!(
                "capability {} has missing or invalid projection fields",
                capability.id
            ));
        }
        validate_capability_claims(root, capability, &mut errors);
        let Some(&target_position) = milestone_positions.get(capability.target_milestone.as_str())
        else {
            errors.push(format!(
                "capability {} references unknown target milestone {}",
                capability.id, capability.target_milestone
            ));
            continue;
        };
        match membership.get(capability.id.as_str()) {
            Some(ids) if ids.len() == 1 && ids[0] == capability.target_milestone => {}
            Some(ids) if ids.is_empty() => errors.push(format!(
                "capability {} must occur in exactly one milestone capability_ids",
                capability.id
            )),
            Some(_) => errors.push(format!(
                "capability {} membership must occur once and match target_milestone",
                capability.id
            )),
            None => errors.push(format!(
                "capability {} must occur in exactly one milestone capability_ids",
                capability.id
            )),
        }
        for dependency in &capability.dependencies {
            let Some(&dependency_index) = capability_indices.get(dependency.as_str()) else {
                errors.push(format!(
                    "capability {} references unknown dependency {}",
                    capability.id, dependency
                ));
                continue;
            };
            let dependency_target = &registry.capabilities[dependency_index].target_milestone;
            if let Some(&dependency_position) = milestone_positions.get(dependency_target.as_str())
                && dependency_position > target_position
            {
                errors.push(format!(
                    "capability {} depends on {} scheduled in a later milestone",
                    capability.id, dependency
                ));
            }
        }
    }
    for milestone in &registry.milestones {
        let Some(&position) = milestone_positions.get(milestone.id.as_str()) else {
            continue;
        };
        for dependency in &milestone.dependencies {
            let Some(&dependency_position) = milestone_positions.get(dependency.as_str()) else {
                errors.push(format!(
                    "milestone {} references unknown dependency {}",
                    milestone.id, dependency
                ));
                continue;
            };
            if dependency_position >= position {
                errors.push(format!(
                    "milestone {} dependency {} is not scheduled earlier",
                    milestone.id, dependency
                ));
            }
        }
        for capability_id in &milestone.capability_ids {
            if !capability_indices.contains_key(capability_id.as_str()) {
                errors.push(format!(
                    "milestone {} references unknown capability {}",
                    milestone.id, capability_id
                ));
            }
        }
    }
    if let Some(cycle) = dependency_cycle(
        registry
            .milestones
            .iter()
            .map(|milestone| milestone.id.as_str()),
        registry
            .milestones
            .iter()
            .map(|milestone| (milestone.id.as_str(), milestone.dependencies.as_slice())),
    ) {
        errors.push(format!(
            "milestone dependency cycle: {}",
            cycle.join(" -> ")
        ));
    }
    if let Some(cycle) = dependency_cycle(
        registry
            .capabilities
            .iter()
            .map(|capability| capability.id.as_str()),
        registry
            .capabilities
            .iter()
            .map(|capability| (capability.id.as_str(), capability.dependencies.as_slice())),
    ) {
        errors.push(format!(
            "capability dependency cycle: {}",
            cycle.join(" -> ")
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        errors.sort();
        Err(format!(
            "registry validation failed:\n - {}",
            errors.join("\n - ")
        ))
    }
}

fn validate_capability_claims(root: &Path, capability: &Capability, errors: &mut Vec<String>) {
    let prefix = format!("capability {} ", capability.id);
    let refs = [
        &capability.refs.adr,
        &capability.refs.contract,
        &capability.refs.source,
        &capability.refs.test,
    ];
    for group in refs {
        for reference in group {
            if let Err(error) = check_reference(root, reference) {
                errors.push(format!("{prefix}{error}"));
            }
        }
    }
    let dimensions = [
        (
            "deterministic",
            &capability.verification.deterministic,
            &capability.evidence.deterministic,
        ),
        (
            "live_agent",
            &capability.verification.live_agent,
            &capability.evidence.live_agent,
        ),
        (
            "uat",
            &capability.verification.uat,
            &capability.evidence.uat,
        ),
        (
            "deployment",
            &capability.verification.deployment,
            &capability.evidence.deployment,
        ),
        (
            "isolation",
            &capability.verification.isolation,
            &capability.evidence.isolation,
        ),
    ];
    for (name, state, records) in dimensions {
        for reference in records {
            if let Err(error) = check_reference(root, reference) {
                errors.push(format!("{prefix}evidence {error}"));
            }
        }
        if state == "not_tested" && !records.is_empty() {
            errors.push(format!(
                "{prefix}{name} has evidence while marked not_tested"
            ));
        }
        if state == "verified" || state == "accepted" {
            if records.is_empty() {
                errors.push(format!("{prefix}{name} {state} without evidence"));
            } else if name == "live_agent" {
                for record in records {
                    if let Err(error) = validate_manifest(root, record) {
                        errors.push(format!("{prefix}{error}"));
                    }
                }
            } else {
                for record in records {
                    if let Err(error) = validate_evidence(root, name, record) {
                        errors.push(format!("{prefix}{error}"));
                    }
                }
            }
        }
    }
    if capability.implementation == "implemented"
        && (capability.refs.source.is_empty() || capability.refs.test.is_empty())
    {
        errors.push(format!(
            "{prefix}implemented requires source and test references"
        ));
    }
    if capability.implementation == "not_implemented"
        && dimensions
            .iter()
            .any(|(_, state, _)| *state == "verified" || *state == "accepted")
    {
        errors.push(format!(
            "{prefix}not_implemented cannot claim verified or accepted evidence"
        ));
    }
    if capability.decision == "rejected" && capability.implementation == "implemented" {
        errors.push(format!(
            "{prefix}rejected capability cannot be marked implemented"
        ));
    }
    if capability.acceptance.deterministic.is_empty() || capability.acceptance.real.is_empty() {
        errors.push(format!(
            "{prefix}requires deterministic and real acceptance criteria"
        ));
    }
    for (name, state, allowed) in [
        (
            "deterministic",
            &capability.verification.deterministic,
            &["verified", "not_tested", "blocked", "failed"][..],
        ),
        (
            "live_agent",
            &capability.verification.live_agent,
            &["verified", "not_tested", "blocked", "failed"][..],
        ),
        (
            "uat",
            &capability.verification.uat,
            &["accepted", "not_tested", "blocked", "failed"][..],
        ),
        (
            "deployment",
            &capability.verification.deployment,
            &["verified", "not_tested", "blocked", "failed"][..],
        ),
        (
            "isolation",
            &capability.verification.isolation,
            &["verified", "not_tested", "blocked", "failed"][..],
        ),
    ] {
        if !allowed.contains(&state.as_str()) {
            errors.push(format!("{prefix}has invalid {name} verification state"));
        }
    }
}

fn check_reference(root: &Path, reference: &str) -> Result<(), String> {
    let path = reference.split('#').next().unwrap_or("");
    if path.is_empty() {
        return Err("has empty reference".into());
    }
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(format!(
            "reference {path:?} is absolute; repository-relative paths are required"
        ));
    }
    if relative
        .components()
        .any(|part| part == std::path::Component::ParentDir)
    {
        return Err(format!(
            "reference {path:?} contains forbidden parent traversal"
        ));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("resolve repository root: {e}"))?;
    let target = root
        .join(relative)
        .canonicalize()
        .map_err(|_| format!("references missing file {path:?}"))?;
    if !target.starts_with(&root) {
        return Err(format!(
            "reference {path:?} resolves outside repository root"
        ));
    }
    if !target.is_file() {
        return Err(format!("references missing file {path:?}"));
    }
    Ok(())
}

fn validate_evidence(root: &Path, dimension: &str, reference: &str) -> Result<(), String> {
    check_reference(root, reference)?;
    let path = reference.split('#').next().unwrap_or("");
    if !path.starts_with("docs/evidence/") || !path.to_ascii_lowercase().ends_with(".json") {
        return Err(format!(
            "{dimension} evidence must reference a JSON file under docs/evidence"
        ));
    }
    let content =
        fs::read(root.join(path)).map_err(|e| format!("{dimension} evidence {path:?}: {e}"))?;
    let record: serde_json::Value = serde_json::from_slice(&content)
        .map_err(|e| format!("{dimension} evidence {path:?} is invalid JSON: {e}"))?;
    let pass = record
        .get("result")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.to_ascii_uppercase().starts_with("PASS"))
        || record
            .get("checks")
            .and_then(|v| v.as_array())
            .is_some_and(|checks| {
                !checks.is_empty()
                    && checks
                        .iter()
                        .all(|v| v.get("pass").and_then(|p| p.as_bool()) == Some(true))
            });
    let required = match dimension {
        "uat" => "PRODUCT_OWNER_UAT_VERIFIED",
        "deployment" => "DEPLOYMENT_VERIFIED",
        "isolation" => "SEC_01_ISOLATION_VERIFIED",
        _ => "",
    };
    let level_ok = required.is_empty()
        || (record.get("evidence_level").and_then(|v| v.as_str()) == Some(required)
            && record
                .get("status")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("verified")));
    if dimension == "deterministic" && !pass {
        return Err(format!(
            "deterministic evidence {path:?} has no recorded passing result"
        ));
    }
    if !level_ok {
        return Err(format!(
            "{dimension} evidence {path:?} must have {required} level and verified status"
        ));
    }
    Ok(())
}

fn validate_manifest(root: &Path, reference: &str) -> Result<(), String> {
    check_reference(root, reference)?;
    if reference.contains('#') {
        return Err(
            "live_agent evidence must reference docs/evidence/*certification-manifest.json without a fragment".into(),
        );
    }
    let path = reference.split('#').next().unwrap_or("");
    if !path.starts_with("docs/evidence/")
        || !path.ends_with("certification-manifest.json")
        || path.matches('/').count() != 2
    {
        return Err(
            "live_agent evidence must reference docs/evidence/*certification-manifest.json".into(),
        );
    }
    let content = fs::read(root.join(path))
        .map_err(|e| format!("live_agent certification manifest {path:?}: {e}"))?;
    let manifest: CertificationManifest = serde_json::from_slice(&content)
        .map_err(|e| format!("live_agent certification manifest {path:?} is invalid JSON: {e}"))?;
    let sha = |s: &str| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if manifest.level != "REAL_PROVIDER_E2E_VERIFIED" || manifest.status != "verified" {
        return Err(format!(
            "live_agent certification manifest {path:?} must have level REAL_PROVIDER_E2E_VERIFIED and status verified"
        ));
    }
    if !sha(&manifest.implementation_sha) || !sha(&manifest.candidate_sha) {
        return Err(format!(
            "live_agent certification manifest {path:?} requires full implementation_sha and candidate_sha values"
        ));
    }
    Ok(())
}

fn dependency_cycle<'a>(
    nodes: impl Iterator<Item = &'a str>,
    edges: impl Iterator<Item = (&'a str, &'a [String])>,
) -> Option<Vec<String>> {
    let mut edge_map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (node, dependencies) in edges {
        edge_map.insert(node, dependencies.iter().map(String::as_str).collect());
    }
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    let mut stack = Vec::new();
    let mut result = None;

    fn visit<'a>(
        node: &'a str,
        edge_map: &BTreeMap<&'a str, Vec<&'a str>>,
        state: &mut BTreeMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
        result: &mut Option<Vec<String>>,
    ) -> bool {
        state.insert(node, 1);
        stack.push(node);
        let mut dependencies = edge_map.get(node).cloned().unwrap_or_default();
        dependencies.sort_unstable();
        for dependency in dependencies {
            match state.get(dependency).copied().unwrap_or(0) {
                1 => {
                    let start = stack
                        .iter()
                        .position(|value| *value == dependency)
                        .unwrap_or(0);
                    let mut cycle = stack[start..]
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>();
                    cycle.push(dependency.to_string());
                    *result = Some(cycle);
                    return true;
                }
                0 if edge_map.contains_key(dependency)
                    && visit(dependency, edge_map, state, stack, result) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        stack.pop();
        state.insert(node, 2);
        false
    }

    let mut sorted_nodes = nodes.collect::<Vec<_>>();
    sorted_nodes.sort_unstable();
    for node in sorted_nodes {
        if state.get(node).copied().unwrap_or(0) == 0
            && visit(node, &edge_map, &mut state, &mut stack, &mut result)
        {
            return result;
        }
    }
    None
}

fn render_roadmap(registry: &Registry) -> String {
    let capabilities: BTreeMap<&str, &Capability> = registry
        .capabilities
        .iter()
        .map(|capability| (capability.id.as_str(), capability))
        .collect();
    let mut output = String::from(
        "# Software Factory Roadmap\n\nThis page is generated from `docs/product/capabilities.yaml`. Edit the registry and run `cargo run --manifest-path native/Cargo.toml --bin factory-architecture-check -- --root . --write-roadmap`.\n",
    );
    for milestone in &registry.milestones {
        output.push_str(&format!(
            "\n## {} — {}\n\nDecision: `{}`\n\n",
            milestone.id, milestone.title, milestone.decision
        ));
        write_list(&mut output, "Dependencies", &milestone.dependencies);
        output.push_str("### Capabilities\n\n");
        if milestone.capability_ids.is_empty() {
            output.push_str("- None specified.\n\n");
        } else {
            for id in &milestone.capability_ids {
                if let Some(capability) = capabilities.get(id.as_str()) {
                    output.push_str(&format!(
                        "- {} — {} ({}, {})\n",
                        capability.id,
                        capability.name,
                        capability.decision,
                        capability.implementation
                    ));
                } else {
                    output.push_str(&format!("- {id} — [missing capability definition]\n"));
                }
            }
            output.push('\n');
        }
        write_list(&mut output, "Deliverables", &milestone.deliverables);
        write_list(&mut output, "Exclusions", &milestone.exclusions);
        write_list(&mut output, "Risks", &milestone.risks);
        write_list(
            &mut output,
            "Deterministic acceptance",
            &milestone.deterministic_acceptance,
        );
        write_list(&mut output, "Real acceptance", &milestone.real_acceptance);
        write_list(
            &mut output,
            "Product Owner decisions",
            &milestone.owner_decisions,
        );
    }
    while output.ends_with('\n') {
        output.pop();
    }
    output.push('\n');
    output
}

fn write_list(output: &mut String, title: &str, values: &[String]) {
    output.push_str(&format!("### {title}\n\n"));
    if values.is_empty() {
        output.push_str("- None specified.\n\n");
        return;
    }
    for value in values {
        output.push_str(&format!("- {value}\n"));
    }
    output.push('\n');
}

fn validate_adr_supersession(root: &Path) -> Result<(), String> {
    let directory = root.join("docs/architecture/decisions");
    let mut adrs = BTreeMap::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| format!("read ADR directory {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("read ADR directory entry: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("ADR-") || !name.ends_with(".md") {
            continue;
        }
        let id = name.split('-').take(2).collect::<Vec<_>>().join("-");
        let text = fs::read_to_string(entry.path())
            .map_err(|error| format!("read ADR {}: {error}", entry.path().display()))?;
        adrs.insert(id, parse_adr(&text));
    }
    let mut errors = Vec::new();
    for (id, adr) in &adrs {
        if adr.accepted && adr.supersedes.is_empty() && adr.status == "superseded" {
            errors.push(format!("{id} is superseded but names no superseding ADR"));
        }
        for target in &adr.supersedes {
            if !adrs.contains_key(target) {
                errors.push(format!("{id} supersedes missing ADR {target}"));
            }
            if !adr.accepted {
                errors.push(format!("{id} supersedes ADRs but is not accepted"));
            }
        }
    }
    if let Some(cycle) = dependency_cycle(
        adrs.keys().map(String::as_str),
        adrs.iter()
            .map(|(id, adr)| (id.as_str(), adr.supersedes.as_slice())),
    ) {
        errors.push(format!("ADR supersession cycle: {}", cycle.join(" -> ")));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        errors.sort();
        Err(format!(
            "ADR supersession validation failed:\n - {}",
            errors.join("\n - ")
        ))
    }
}

#[derive(Default)]
struct Adr {
    status: String,
    accepted: bool,
    supersedes: Vec<String>,
}

fn parse_adr(contents: &str) -> Adr {
    let mut adr = Adr::default();
    for line in contents.lines() {
        let lower = line.to_ascii_lowercase();
        if (lower
            .trim_start_matches(['-', '*', ' '])
            .starts_with("status:")
            || lower.contains("**status:**"))
            && let Some((_, value)) = line.split_once(':')
        {
            let value = value.trim().trim_matches('*').trim();
            let value = value
                .split(|character: char| character.is_whitespace() || character == '(')
                .next()
                .unwrap_or("")
                .trim_matches(['*', '`', '"', '\''])
                .to_ascii_lowercase();
            adr.status = value.clone();
            adr.accepted = value == "accepted";
        }
        if lower.contains("supersedes:") {
            for token in
                line.split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
            {
                let upper = token.to_ascii_uppercase();
                if upper.starts_with("ADR-") && upper.len() == 8 {
                    let digits = &upper[4..];
                    if digits.bytes().all(|byte| byte.is_ascii_digit()) {
                        adr.supersedes.push(upper);
                    }
                }
            }
        }
    }
    adr.supersedes.sort();
    adr.supersedes.dedup();
    adr
}

fn validate_stable_capability_ids(
    root: &Path,
    base_ref: &str,
    current: &Registry,
) -> Result<(), String> {
    let old = git_output(
        root,
        &[
            "show",
            &format!("{base_ref}:docs/product/capabilities.yaml"),
        ],
    )
    .map_err(|error| format!("read base capability registry at {base_ref}: {error}"))?;
    let previous = parse_registry(&String::from_utf8_lossy(&old))?;
    let current_ids: BTreeSet<&str> = current
        .capabilities
        .iter()
        .map(|capability| capability.id.as_str())
        .collect();
    let removed: Vec<&str> = previous
        .capabilities
        .iter()
        .map(|capability| capability.id.as_str())
        .filter(|id| !current_ids.contains(id))
        .collect();
    if removed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "capability IDs are stable; removed from registry: {}",
            removed.join(", ")
        ))
    }
}

fn validate_accepted_adr_history(root: &Path, base_ref: &str) -> Result<(), String> {
    let output = git_output(
        root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            base_ref,
            "HEAD",
            "--",
            "docs/architecture/decisions/ADR-*.md",
        ],
    )
    .map_err(|error| format!("inspect ADR history against {base_ref}: {error}"))?;
    let records = String::from_utf8_lossy(&output);
    for line in records.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 2 {
            return Err(format!("cannot parse ADR history diff record {line:?}"));
        }
        if fields[0].starts_with('A') {
            continue;
        }
        let base_path = fields[1];
        let base_contents = git_output(root, &["show", &format!("{base_ref}:{base_path}")])
            .map_err(|error| format!("read base ADR {base_path}: {error}"))?;
        if parse_adr(&String::from_utf8_lossy(&base_contents)).accepted
            && !has_valid_amendment(root, base_path)?
        {
            return Err(format!(
                "accepted ADR {base_path} was changed in place; add a superseding ADR and preserve accepted history"
            ));
        }
    }
    Ok(())
}

fn has_valid_amendment(root: &Path, adr_path: &str) -> Result<bool, String> {
    let current = fs::read_to_string(root.join(adr_path))
        .map_err(|error| format!("read current ADR {adr_path}: {error}"))?;
    let Some(id) = current.lines().find_map(|line| {
        line.split_once("Amended by:").map(|(_, rest)| {
            rest.trim_start_matches(|character: char| {
                character == '*' || character == ':' || character == ' '
            })
            .split_whitespace()
            .next()
            .unwrap_or("")
        })
    }) else {
        return Ok(false);
    };
    if id.is_empty() {
        return Ok(false);
    }
    let directory = root.join("docs/architecture/decisions");
    let mut matches = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("read ADR directory: {error}"))? {
        let entry = entry.map_err(|error| format!("read ADR entry: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{id}-")) && name.ends_with(".md") {
            matches.push(entry.path());
        }
    }
    if matches.len() != 1 {
        return Ok(false);
    }
    let amendment =
        fs::read_to_string(&matches[0]).map_err(|error| format!("read amendment ADR: {error}"))?;
    Ok(parse_adr(&amendment).accepted)
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("run git {}: {error}", args.join(" ")))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn valid_id(value: &str) -> bool {
    let mut chars = value.chars();
    if !chars
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
    {
        return false;
    }
    let mut previous_hyphen = false;
    for character in chars {
        if character == '-' {
            if previous_hyphen {
                return false;
            }
            previous_hyphen = true;
        } else if character.is_ascii_uppercase() || character.is_ascii_digit() {
            previous_hyphen = false;
        } else {
            return false;
        }
    }
    !previous_hyphen
}

fn valid_decision(value: &str) -> bool {
    matches!(value, "approved" | "proposed" | "deferred" | "rejected")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Repo(PathBuf);

    impl Repo {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let root =
                env::temp_dir().join(format!("factory-architecture-{}-{id}", std::process::id()));
            fs::create_dir_all(root.join("docs/product")).unwrap();
            fs::create_dir_all(root.join("docs/roadmap")).unwrap();
            fs::create_dir_all(root.join("docs/architecture/decisions")).unwrap();
            let result = Command::new("git")
                .arg("init")
                .arg("--quiet")
                .arg(&root)
                .status()
                .unwrap();
            assert!(result.success());
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["config", "user.name", "Test"])
                .status()
                .unwrap();
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["config", "user.email", "test@example.invalid"])
                .status()
                .unwrap();
            Self(root)
        }

        fn write(&self, path: &str, value: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }

        fn commit(&self, message: &str) {
            let add = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["add", "-A"])
                .status()
                .unwrap();
            assert!(add.success());
            let commit = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["commit", "-qm", message])
                .status()
                .unwrap();
            assert!(commit.success());
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tiny_registry() -> &'static str {
        "schema_version: 1\nmilestones:\n- id: M-1\n  title: First\n  decision_status: approved\n  dependencies: []\n  deliverables:\n  - Deliver\n  capability_ids:\n  - CAP-1\n  exclusions:\n  - None\n  risks:\n  - Low\n  deterministic_acceptance:\n  - Test\n  real_acceptance:\n  - Real\n  owner_decisions:\n  - Owner\ncapabilities:\n- id: CAP-1\n  name: One\n  category: Runtime\n  purpose: Do work\n  product_objective: Objective\n  decision_status: approved\n  implementation_status: in_progress\n  verification:\n    deterministic: not_tested\n    live_agent: not_tested\n    uat: not_tested\n    deployment: not_tested\n    isolation: not_tested\n  owner: Core\n  dependencies: []\n  target_milestone: M-1\n  refs: {adr: [], contract: [], source: [], test: []}\n  acceptance: {deterministic: Test, real: Run}\n  evidence: {deterministic: [], live_agent: [], uat: [], deployment: [], isolation: []}\n"
    }

    fn accepted_adr() -> &'static str {
        "# ADR-0001\n\n- **Status:** Accepted\n\n## Decision\nA decision.\n"
    }

    #[test]
    fn references_reject_traversal_and_symlink_escape() {
        let repo = Repo::new();
        let outside = env::temp_dir().join(format!(
            "factory-architecture-outside-{}",
            std::process::id()
        ));
        fs::write(&outside, "outside").unwrap();
        repo.write("inside/file.md", "inside");
        assert!(
            check_reference(&repo.0, "../outside")
                .unwrap_err()
                .contains("parent traversal")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, repo.0.join("inside/escape.md")).unwrap();
            assert!(
                check_reference(&repo.0, "inside/escape.md")
                    .unwrap_err()
                    .contains("outside repository root")
            );
        }
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn evidence_and_live_certification_claims_require_typed_passing_records() {
        let repo = Repo::new();
        repo.write("docs/evidence/pass.json", r#"{"checks":[{"pass":true}]}"#);
        repo.write("docs/evidence/fail.json", r#"{"checks":[{"pass":false}]}"#);
        assert!(validate_evidence(&repo.0, "deterministic", "docs/evidence/pass.json").is_ok());
        assert!(
            validate_evidence(&repo.0, "deterministic", "docs/evidence/fail.json")
                .unwrap_err()
                .contains("no recorded passing result")
        );
        assert!(
            validate_evidence(&repo.0, "deterministic", "docs/evidence/../../outside.json")
                .unwrap_err()
                .contains("parent traversal")
        );
        repo.write("docs/evidence/example-certification-manifest.json", r#"{"level":"REAL_PROVIDER_E2E_VERIFIED","status":"verified","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}"#);
        assert!(
            validate_manifest(&repo.0, "docs/evidence/example-certification-manifest.json").is_ok()
        );
        assert!(
            validate_manifest(
                &repo.0,
                "docs/evidence/example-certification-manifest.json#result"
            )
            .unwrap_err()
            .contains("without a fragment")
        );
        repo.write("docs/evidence/example-certification-manifest.json", r#"{"level":"SIMULATED","status":"verified","implementation_sha":"short","candidate_sha":"short"}"#);
        assert!(
            validate_manifest(&repo.0, "docs/evidence/example-certification-manifest.json")
                .unwrap_err()
                .contains("REAL_PROVIDER_E2E_VERIFIED")
        );
        for (name, content, expected) in [
            (
                "rejected status",
                r#"{"level":"REAL_PROVIDER_E2E_VERIFIED","status":"rejected","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}"#,
                "must have level",
            ),
            (
                "short implementation SHA",
                r#"{"level":"REAL_PROVIDER_E2E_VERIFIED","status":"verified","implementation_sha":"short","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}"#,
                "requires full implementation_sha",
            ),
            (
                "short candidate SHA",
                r#"{"level":"REAL_PROVIDER_E2E_VERIFIED","status":"verified","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"short"}"#,
                "requires full implementation_sha",
            ),
            ("malformed JSON", "{", "invalid JSON"),
        ] {
            repo.write("docs/evidence/example-certification-manifest.json", content);
            assert!(
                validate_manifest(&repo.0, "docs/evidence/example-certification-manifest.json")
                    .unwrap_err()
                    .contains(expected),
                "{name}"
            );
        }
    }

    #[test]
    fn registry_rejects_invalid_claim_combinations() {
        let root = Path::new(".");
        let baseline = parse_registry(tiny_registry()).unwrap();
        type ClaimCase = (&'static str, fn(&mut Registry), &'static str);
        let cases: &[ClaimCase] = &[
            (
                "duplicate id",
                |r| r.capabilities.push(r.capabilities[0].clone()),
                "duplicate capability id",
            ),
            (
                "missing reference",
                |r| {
                    r.capabilities[0]
                        .refs
                        .adr
                        .push("docs/architecture/missing.md".into())
                },
                "references missing file",
            ),
            (
                "implemented without references",
                |r| r.capabilities[0].implementation = "implemented".into(),
                "implemented requires source and test references",
            ),
            (
                "verified without evidence",
                |r| r.capabilities[0].verification.deterministic = "verified".into(),
                "deterministic verified without evidence",
            ),
            (
                "invalid decision",
                |r| r.capabilities[0].decision = "implemented".into(),
                "missing or invalid projection fields",
            ),
            (
                "not implemented with verified claim",
                |r| {
                    r.capabilities[0].implementation = "not_implemented".into();
                    r.capabilities[0].verification.deterministic = "verified".into();
                },
                "not_implemented cannot claim verified",
            ),
            (
                "rejected but implemented",
                |r| {
                    r.capabilities[0].decision = "rejected".into();
                    r.capabilities[0].implementation = "implemented".into();
                },
                "rejected capability cannot be marked implemented",
            ),
            (
                "evidence while not tested",
                |r| {
                    r.capabilities[0]
                        .evidence
                        .deterministic
                        .push("docs/evidence/record.json".into())
                },
                "has evidence while marked not_tested",
            ),
            (
                "missing milestone membership",
                |r| r.milestones[0].capability_ids.clear(),
                "must occur in exactly one milestone",
            ),
            (
                "unknown dependency",
                |r| r.capabilities[0].dependencies.push("CAP-999".into()),
                "unknown dependency",
            ),
        ];
        for (name, mutate, expected) in cases {
            let mut registry = baseline.clone();
            mutate(&mut registry);
            assert!(
                validate_registry(root, &registry)
                    .unwrap_err()
                    .contains(expected),
                "{name}"
            );
        }
        let repo = Repo::new();
        repo.write("docs/evidence/live.json", "{}");
        assert!(
            validate_manifest(&repo.0, "docs/evidence/live.json")
                .unwrap_err()
                .contains("must reference docs/evidence/*certification-manifest.json")
        );
    }

    #[test]
    fn registry_parser_rejects_duplicate_keys_unknown_fields_and_malformed_yaml() {
        let duplicate = tiny_registry().replacen(
            "schema_version: 1",
            "schema_version: 1\nschema_version: 2",
            1,
        );
        assert!(parse_registry(&duplicate).is_err());
        let unknown =
            tiny_registry().replacen("  name: One", "  name: One\n  unsupported_field: true", 1);
        assert!(parse_registry(&unknown).is_err());
        assert!(parse_registry("schema_version: [").is_err());
    }

    #[test]
    fn valid_registry_renders_exact_roadmap() {
        let repo = Repo::new();
        repo.write("docs/product/capabilities.yaml", tiny_registry());
        let registry = parse_registry(tiny_registry()).unwrap();
        repo.write("docs/roadmap/ROADMAP.md", &render_roadmap(&registry));
        repo.write("docs/product/capability-decisions.md", "| CAP-1 | One |\nLint, type, build and tests\nOther validation adapters\nMemory, context and simplification\nCAP-020 Librarian\nCAP-029 advisory incident diagnosis\n");
        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            accepted_adr(),
        );
        assert!(
            check_repository(
                &repo.0,
                Path::new("docs/product/capabilities.yaml"),
                Path::new("docs/roadmap/ROADMAP.md"),
                false,
                None
            )
            .is_ok()
        );
        let mut stale = render_roadmap(&registry);
        stale.push_str("stale\n");
        repo.write("docs/roadmap/ROADMAP.md", &stale);
        assert!(
            check_repository(
                &repo.0,
                Path::new("docs/product/capabilities.yaml"),
                Path::new("docs/roadmap/ROADMAP.md"),
                false,
                None
            )
            .unwrap_err()
            .contains("roadmap is stale")
        );
    }

    #[test]
    fn registry_rejects_unknown_or_duplicate_membership_and_dependency_cycles() {
        let mut registry = parse_registry(tiny_registry()).unwrap();
        registry.milestones[0].capability_ids.push("CAP-1".into());
        assert!(
            validate_registry(Path::new("."), &registry)
                .unwrap_err()
                .contains("membership")
        );
        let mut registry = parse_registry(tiny_registry()).unwrap();
        registry.capabilities[0].dependencies.push("CAP-1".into());
        assert!(
            validate_registry(Path::new("."), &registry)
                .unwrap_err()
                .contains("dependency cycle")
        );
        let mut registry = parse_registry(tiny_registry()).unwrap();
        let mut second = registry.milestones[0].clone();
        second.id = "M-2".into();
        second.capability_ids.clear();
        second.dependencies = vec!["M-1".into()];
        registry.milestones[0].dependencies = vec!["M-2".into()];
        registry.milestones.push(second);
        assert!(
            validate_registry(Path::new("."), &registry)
                .unwrap_err()
                .contains("milestone dependency cycle: M-1 -> M-2 -> M-1")
        );
        let mut registry = parse_registry(tiny_registry()).unwrap();
        let mut second = registry.capabilities[0].clone();
        second.id = "CAP-2".into();
        second.dependencies = vec!["CAP-1".into()];
        registry.capabilities[0].dependencies = vec!["CAP-2".into()];
        registry.milestones[0].capability_ids.push("CAP-2".into());
        registry.capabilities.push(second);
        assert!(
            validate_registry(Path::new("."), &registry)
                .unwrap_err()
                .contains("capability dependency cycle: CAP-1 -> CAP-2 -> CAP-1")
        );
    }

    #[test]
    fn supersession_requires_existing_targets_and_acyclic_graph() {
        let repo = Repo::new();
        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            "# ADR-0001\n- **Status:** Accepted\n- **Supersedes:** ADR-0002\n",
        );
        repo.write(
            "docs/architecture/decisions/ADR-0002-two.md",
            "# ADR-0002\n- **Status:** Accepted\n- **Supersedes:** ADR-0001\n",
        );
        assert!(
            validate_adr_supersession(&repo.0)
                .unwrap_err()
                .contains("cycle")
        );
        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            "# ADR-0001\n- **Status:** Accepted\n- **Supersedes:** ADR-0003\n",
        );
        assert!(
            validate_adr_supersession(&repo.0)
                .unwrap_err()
                .contains("missing ADR ADR-0003")
        );
    }

    #[test]
    fn base_ref_protects_accepted_adr_history_and_stable_capability_ids() {
        let repo = Repo::new();
        repo.write("docs/product/capabilities.yaml", tiny_registry());
        repo.write("docs/roadmap/ROADMAP.md", "placeholder\n");
        repo.write("docs/product/capability-decisions.md", "| CAP-1 | One |\nLint, type, build and tests\nOther validation adapters\nMemory, context and simplification\nCAP-020 Librarian\nCAP-029 advisory incident diagnosis\n");
        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            accepted_adr(),
        );
        repo.commit("base");
        let base = git_output(&repo.0, &["rev-parse", "HEAD"]).unwrap();
        let base = String::from_utf8(base).unwrap().trim().to_string();

        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            "# ADR-0001\n- **Status:** Accepted\nChanged in place.\n",
        );
        repo.write(
            "docs/product/capabilities.yaml",
            &tiny_registry().replace("- id: CAP-1\n", "- id: CAP-2\n"),
        );
        repo.commit("invalid change");
        assert!(
            validate_accepted_adr_history(&repo.0, &base)
                .unwrap_err()
                .contains("changed in place")
        );
        let current = parse_registry(
            &fs::read_to_string(repo.0.join("docs/product/capabilities.yaml")).unwrap(),
        )
        .unwrap();
        assert!(
            validate_stable_capability_ids(&repo.0, &base, &current)
                .unwrap_err()
                .contains("CAP-1")
        );
    }
}
