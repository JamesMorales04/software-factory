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
    use super::validate_image_reference;

    #[test]
    fn validates_image_references() {
        assert!(validate_image_reference("localhost/software-factory-core:dev").is_ok());
        assert!(validate_image_reference("registry.example/core@sha256:abc").is_ok());
        assert!(validate_image_reference("").is_err());
        assert!(validate_image_reference("-rm").is_err());
        assert!(validate_image_reference("core image").is_err());
    }
}
