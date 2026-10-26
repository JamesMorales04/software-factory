use std::process::{Command, Output};

use crate::text::one_line;

pub(crate) fn version() -> Result<String, String> {
    let output = Command::new("podman")
        .arg("--version")
        .output()
        .map_err(|error| format!("podman executable is unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "podman --version exited with {}: {}",
            output.status,
            diagnostic(&output)
        ));
    }
    let version = first_nonempty(&output.stdout, &output.stderr);
    if version.is_empty() {
        return Err("podman --version returned no version text".to_owned());
    }
    Ok(version)
}

pub(crate) fn rootless() -> Result<bool, String> {
    let output = Command::new("podman")
        .args(["info", "--format", "{{.Host.Security.Rootless}}"])
        .output()
        .map_err(|error| format!("podman info could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "podman info exited with {}: {}",
            output.status,
            diagnostic(&output)
        ));
    }
    let value = first_nonempty(&output.stdout, &output.stderr);
    match value.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!(
            "podman info returned an unexpected rootless value: {other:?}"
        )),
    }
}

pub(crate) fn image_exists(image: &str) -> Result<bool, String> {
    let output = Command::new("podman")
        .args(["image", "exists", image])
        .output()
        .map_err(|error| format!("podman image exists could not run: {error}"))?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(1) {
        return Ok(false);
    }
    Err(format!(
        "podman image exists exited with {}: {}",
        output.status,
        diagnostic(&output)
    ))
}

pub(crate) fn container_exists(name: &str) -> Result<bool, String> {
    exists(
        &["container".to_owned(), "exists".to_owned(), name.to_owned()],
        "podman container exists",
    )
}

pub(crate) fn container_state(name: &str) -> Result<Option<String>, String> {
    if !container_exists(name)? {
        return Ok(None);
    }
    let output = Command::new("podman")
        .args(["inspect", "--format", "{{.State.Status}}", name])
        .output()
        .map_err(|error| format!("podman inspect could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "podman inspect exited with {}: {}",
            output.status,
            diagnostic(&output)
        ));
    }
    let state = first_nonempty(&output.stdout, &output.stderr);
    if state.is_empty() {
        return Err("podman inspect returned no container state".to_owned());
    }
    Ok(Some(state))
}

pub(crate) fn container_label(name: &str, key: &str) -> Result<Option<String>, String> {
    if key.is_empty()
        || key.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-')
        })
    {
        return Err("container label key is invalid".to_owned());
    }
    let format = format!("{{{{ index .Config.Labels \"{key}\" }}}}");
    let output = Command::new("podman")
        .args(["inspect", "--format", &format, name])
        .output()
        .map_err(|error| format!("podman inspect could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "podman inspect exited with {}: {}",
            output.status,
            diagnostic(&output)
        ));
    }
    let value = first_nonempty(&output.stdout, &output.stderr);
    if value.is_empty() || value == "<no value>" || value == "<nil>" {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub(crate) fn volume_exists(name: &str) -> Result<bool, String> {
    exists(
        &["volume".to_owned(), "exists".to_owned(), name.to_owned()],
        "podman volume exists",
    )
}

pub(crate) fn run(args: &[String]) -> Result<String, String> {
    let output = Command::new("podman")
        .args(args)
        .output()
        .map_err(|error| format!("podman {} could not run: {error}", operation(args)))?;
    if !output.status.success() {
        return Err(format!(
            "podman {} exited with {}: {}",
            operation(args),
            output.status,
            diagnostic(&output)
        ));
    }
    Ok(first_nonempty(&output.stdout, &output.stderr))
}

pub(crate) fn logs_follow(name: &str) -> Result<i32, String> {
    let status = Command::new("podman")
        .args(["logs", "--follow", name])
        .status()
        .map_err(|error| format!("podman logs could not run: {error}"))?;
    Ok(status.code().unwrap_or(1))
}

pub(crate) fn validate_image_reference(image: &str) -> Result<(), String> {
    if image.is_empty() {
        return Err("FACTORY_IMAGE is empty".to_owned());
    }
    if image.starts_with('-') {
        return Err("FACTORY_IMAGE must not start with '-'".to_owned());
    }
    if image
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("FACTORY_IMAGE must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_container_name(name: &str) -> Result<(), String> {
    validate_resource_name("FACTORY_CONTAINER_NAME", name)
}

pub(crate) fn validate_volume_name(name: &str) -> Result<(), String> {
    validate_resource_name("FACTORY_VOLUME", name)
}

fn validate_resource_name(variable: &str, name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("{variable} is empty"));
    }
    if name.len() > 255 {
        return Err(format!("{variable} must be at most 255 characters"));
    }
    let mut characters = name.chars();
    let first = characters.next().expect("non-empty name");
    if !first.is_ascii_alphanumeric() {
        return Err(format!(
            "{variable} must start with an ASCII letter or digit"
        ));
    }
    if characters.any(|character| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '.' | '-')
    }) {
        return Err(format!(
            "{variable} may contain only ASCII letters, digits, '_', '.' and '-'"
        ));
    }
    Ok(())
}

fn exists(args: &[String], description: &str) -> Result<bool, String> {
    let output = Command::new("podman")
        .args(args)
        .output()
        .map_err(|error| format!("{description} could not run: {error}"))?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(1) {
        return Ok(false);
    }
    Err(format!(
        "{description} exited with {}: {}",
        output.status,
        diagnostic(&output)
    ))
}

fn operation(args: &[String]) -> String {
    args.iter().take(2).cloned().collect::<Vec<_>>().join(" ")
}

fn first_nonempty(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        String::from_utf8_lossy(stderr).trim().to_owned()
    } else {
        stdout
    }
}

fn diagnostic(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        "(no diagnostic)".to_owned()
    } else {
        one_line(stderr, 400)
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_container_name, validate_image_reference, validate_volume_name};

    #[test]
    fn validates_image_references() {
        assert!(validate_image_reference("localhost/software-factory-core:dev").is_ok());
        assert!(validate_image_reference("registry.example/core@sha256:abc").is_ok());
        assert!(validate_image_reference("").is_err());
        assert!(validate_image_reference("-rm").is_err());
        assert!(validate_image_reference("core image").is_err());
    }

    #[test]
    fn validates_container_and_volume_names() {
        assert!(validate_container_name("factory-core").is_ok());
        assert!(validate_container_name("factory_core.dev-1").is_ok());
        assert!(validate_container_name("").is_err());
        assert!(validate_container_name("-core").is_err());
        assert!(validate_container_name("factory core").is_err());
        assert!(validate_container_name("factory/core").is_err());
        assert!(validate_volume_name("factory-core-data").is_ok());
        assert!(validate_volume_name("--force").is_err());
    }
}
