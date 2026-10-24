//! Native governance checks for ADR supersession and capability/roadmap consistency.
//!
//! This deliberately ports only those governance checks. It does not yet port
//! the former checker's evidence-manifest, reference-path, certification, or
//! authorization validation.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Milestone {
    id: String,
    title: String,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Capability {
    id: String,
    name: String,
    decision: String,
    implementation: String,
    dependencies: Vec<String>,
    target_milestone: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Registry {
    schema_version: Option<u32>,
    milestones: Vec<Milestone>,
    capabilities: Vec<Capability>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    None,
    Milestones,
    Capabilities,
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
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = Some(PathBuf::from(args.next().ok_or("--root requires a path")?)),
            "--base-ref" => base_ref = Some(args.next().ok_or("--base-ref requires a Git ref")?),
            "-h" | "--help" => {
                println!("usage: factory-architecture-check --root PATH [--base-ref REF]");
                println!("Validates ADR supersession and capability registry/roadmap consistency.");
                println!(
                    "Does not validate capability evidence, reference paths, or authorization."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let root = root.ok_or("--root is required")?;
    check_repository(&root, base_ref.as_deref())
}

fn check_repository(root: &Path, base_ref: Option<&str>) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve root {}: {error}", root.display()))?;
    let registry_path = root.join("docs/product/capabilities.yaml");
    let roadmap_path = root.join("docs/roadmap/ROADMAP.md");
    let registry_text = fs::read_to_string(&registry_path)
        .map_err(|error| format!("read {}: {error}", registry_path.display()))?;
    let roadmap = fs::read_to_string(&roadmap_path)
        .map_err(|error| format!("read {}: {error}", roadmap_path.display()))?;
    let decisions_path = root.join("docs/product/capability-decisions.md");
    let decisions = fs::read_to_string(&decisions_path)
        .map_err(|error| format!("read {}: {error}", decisions_path.display()))?;
    let registry = parse_registry(&registry_text)?;
    validate_registry(&registry)?;
    validate_capability_preservation(&registry, &decisions)?;
    let expected = render_roadmap(&registry);
    if roadmap != expected {
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
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "capability decision record is missing selected capabilities: {}",
            missing.join(", ")
        ))
    }
}

fn parse_registry(contents: &str) -> Result<Registry, String> {
    let mut result = Registry::default();
    let mut section = Section::None;
    let mut current_milestone: Option<usize> = None;
    let mut current_capability: Option<usize> = None;
    let mut list_field: Option<String> = None;

    for (line_number, raw) in contents.lines().enumerate() {
        let line = raw.split_once('#').map_or(raw, |(before, _)| before);
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent <= 1 && trimmed.starts_with("- ") && section != Section::None {
            let (key, value) = trimmed[2..]
                .split_once(':')
                .ok_or_else(|| format!("line {}: expected list item key:value", line_number + 1))?;
            match section {
                Section::Milestones if key == "id" => {
                    result.milestones.push(Milestone {
                        id: parse_scalar(value),
                        ..Milestone::default()
                    });
                    current_milestone = Some(result.milestones.len() - 1);
                    current_capability = None;
                }
                Section::Capabilities if key == "id" => {
                    result.capabilities.push(Capability {
                        id: parse_scalar(value),
                        ..Capability::default()
                    });
                    current_capability = Some(result.capabilities.len() - 1);
                    current_milestone = None;
                }
                _ => {
                    return Err(format!(
                        "line {}: expected {} item to begin with id",
                        line_number + 1,
                        match section {
                            Section::Milestones => "milestone",
                            Section::Capabilities => "capability",
                            Section::None => unreachable!(),
                        }
                    ));
                }
            }
            list_field = None;
            continue;
        }
        if indent == 0 {
            list_field = None;
            if trimmed == "milestones:" {
                section = Section::Milestones;
                current_milestone = None;
                current_capability = None;
                continue;
            }
            if trimmed == "capabilities:" {
                section = Section::Capabilities;
                current_milestone = None;
                current_capability = None;
                continue;
            }
            if let Some((key, value)) = trimmed.split_once(':')
                && key == "schema_version"
            {
                result.schema_version = Some(parse_scalar(value).parse().map_err(|_| {
                    format!(
                        "line {}: schema_version must be an integer",
                        line_number + 1
                    )
                })?);
            }
            continue;
        }

        if !matches!(section, Section::Milestones | Section::Capabilities) {
            continue;
        }

        if let Some(field) = list_field.as_deref()
            && let Some(value) = trimmed.strip_prefix("- ")
        {
            push_list_value(
                &mut result,
                section,
                current_milestone,
                current_capability,
                field,
                parse_scalar(value),
            );
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            let Some(field) = list_field.as_deref() else {
                // Unprojected nested YAML fields are outside this slice's schema model.
                continue;
            };
            append_list_continuation(
                &mut result,
                section,
                current_milestone,
                current_capability,
                field,
                trimmed,
            )?;
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "[]" {
            list_field = is_projected_list(section, key).then(|| key.to_string());
            continue;
        }
        list_field = None;
        let value = parse_scalar(value);
        match section {
            Section::Milestones => {
                let Some(index) = current_milestone else {
                    return Err(format!(
                        "line {}: milestone field before id",
                        line_number + 1
                    ));
                };
                set_milestone_scalar(&mut result.milestones[index], key, value);
            }
            Section::Capabilities => {
                let Some(index) = current_capability else {
                    return Err(format!(
                        "line {}: capability field before id",
                        line_number + 1
                    ));
                };
                set_capability_scalar(&mut result.capabilities[index], key, value);
            }
            Section::None => {}
        }
    }
    Ok(result)
}

fn is_projected_list(section: Section, field: &str) -> bool {
    match section {
        Section::Milestones => matches!(
            field,
            "dependencies"
                | "deliverables"
                | "capability_ids"
                | "exclusions"
                | "risks"
                | "deterministic_acceptance"
                | "real_acceptance"
                | "owner_decisions"
        ),
        Section::Capabilities => field == "dependencies",
        Section::None => false,
    }
}

fn append_list_continuation(
    registry: &mut Registry,
    section: Section,
    milestone: Option<usize>,
    capability: Option<usize>,
    field: &str,
    continuation: &str,
) -> Result<(), String> {
    let last = match section {
        Section::Milestones => {
            let Some(index) = milestone else {
                return Err("list continuation has no milestone".into());
            };
            let target = &mut registry.milestones[index];
            match field {
                "dependencies" => target.dependencies.last_mut(),
                "deliverables" => target.deliverables.last_mut(),
                "capability_ids" => target.capability_ids.last_mut(),
                "exclusions" => target.exclusions.last_mut(),
                "risks" => target.risks.last_mut(),
                "deterministic_acceptance" => target.deterministic_acceptance.last_mut(),
                "real_acceptance" => target.real_acceptance.last_mut(),
                "owner_decisions" => target.owner_decisions.last_mut(),
                _ => None,
            }
        }
        Section::Capabilities => {
            let Some(index) = capability else {
                return Err("list continuation has no capability".into());
            };
            if field == "dependencies" {
                registry.capabilities[index].dependencies.last_mut()
            } else {
                None
            }
        }
        Section::None => None,
    };
    let Some(last) = last else {
        return Err(format!("list field {field} has no preceding item"));
    };
    last.push(' ');
    last.push_str(continuation);
    Ok(())
}

fn parse_scalar(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn set_milestone_scalar(target: &mut Milestone, key: &str, value: String) {
    match key {
        "title" => target.title = value,
        "decision_status" => target.decision = value,
        _ => {}
    }
}

fn set_capability_scalar(target: &mut Capability, key: &str, value: String) {
    match key {
        "name" => target.name = value,
        "decision_status" => target.decision = value,
        "implementation_status" => target.implementation = value,
        "target_milestone" => target.target_milestone = value,
        _ => {}
    }
}

fn push_list_value(
    registry: &mut Registry,
    section: Section,
    milestone: Option<usize>,
    capability: Option<usize>,
    field: &str,
    value: String,
) {
    match section {
        Section::Milestones => {
            let Some(index) = milestone else { return };
            let target = &mut registry.milestones[index];
            match field {
                "dependencies" => target.dependencies.push(value),
                "deliverables" => target.deliverables.push(value),
                "capability_ids" => target.capability_ids.push(value),
                "exclusions" => target.exclusions.push(value),
                "risks" => target.risks.push(value),
                "deterministic_acceptance" => target.deterministic_acceptance.push(value),
                "real_acceptance" => target.real_acceptance.push(value),
                "owner_decisions" => target.owner_decisions.push(value),
                _ => {}
            }
        }
        Section::Capabilities => {
            let Some(index) = capability else { return };
            if field == "dependencies" {
                registry.capabilities[index].dependencies.push(value);
            }
        }
        Section::None => {}
    }
}

fn validate_registry(registry: &Registry) -> Result<(), String> {
    let mut errors = Vec::new();
    if registry.schema_version != Some(1) {
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
        "# Software Factory Roadmap\n\nThis page is generated from `docs/product/capabilities.yaml`. Edit the registry and run `scripts/check-architecture-consistency --write-roadmap`.\n",
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
        line.split_once("Amended by:")
            .map(|(_, rest)| rest.split_whitespace().next().unwrap_or(""))
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
        "schema_version: 1\nmilestones:\n- id: M-1\n  title: First\n  decision_status: approved\n  dependencies: []\n  deliverables:\n  - Deliver\n  capability_ids:\n  - CAP-1\n  exclusions:\n  - None\n  risks:\n  - Low\n  deterministic_acceptance:\n  - Test\n  real_acceptance:\n  - Real\n  owner_decisions:\n  - Owner\ncapabilities:\n- id: CAP-1\n  name: One\n  decision_status: approved\n  implementation_status: implemented\n  dependencies: []\n  target_milestone: M-1\n"
    }

    fn accepted_adr() -> &'static str {
        "# ADR-0001\n\n- **Status:** Accepted\n\n## Decision\nA decision.\n"
    }

    #[test]
    fn valid_registry_renders_exact_roadmap() {
        let repo = Repo::new();
        repo.write("docs/product/capabilities.yaml", tiny_registry());
        let registry = parse_registry(tiny_registry()).unwrap();
        repo.write("docs/roadmap/ROADMAP.md", &render_roadmap(&registry));
        repo.write("docs/product/capability-decisions.md", "| CAP-1 | One |\n");
        repo.write(
            "docs/architecture/decisions/ADR-0001-one.md",
            accepted_adr(),
        );
        assert!(check_repository(&repo.0, None).is_ok());
        let mut stale = render_roadmap(&registry);
        stale.push_str("stale\n");
        repo.write("docs/roadmap/ROADMAP.md", &stale);
        assert!(
            check_repository(&repo.0, None)
                .unwrap_err()
                .contains("roadmap is stale")
        );
    }

    #[test]
    fn registry_rejects_unknown_or_duplicate_membership_and_dependency_cycles() {
        let mut registry = parse_registry(tiny_registry()).unwrap();
        registry.milestones[0].capability_ids.push("CAP-1".into());
        assert!(
            validate_registry(&registry)
                .unwrap_err()
                .contains("membership")
        );
        let mut registry = parse_registry(tiny_registry()).unwrap();
        registry.capabilities[0].dependencies.push("CAP-1".into());
        assert!(
            validate_registry(&registry)
                .unwrap_err()
                .contains("dependency cycle")
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
        repo.write("docs/product/capability-decisions.md", "| CAP-1 | One |\n");
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
