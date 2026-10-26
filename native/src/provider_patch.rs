//! Tool-free provider output extraction.
//!
//! The provider returns only a bounded JSON file patch. It cannot execute a
//! tool in this mode; the trusted Worker validates paths/content and asks the
//! Supervisor to apply the patch to the exact Attempt worktree.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    lead::{SpecCheck, is_safe_relative_path},
    workspace::path_covered,
};

pub const MAX_FILES: usize = 64;
pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPatch {
    #[serde(default)]
    pub files: Vec<PatchFile>,
    #[serde(default)]
    pub commands: Vec<ProviderCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCommand {
    pub argv: Vec<String>,
    #[serde(default = "default_command_timeout")]
    pub timeout_secs: u64,
}

fn default_command_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlan {
    pub files: Vec<PatchFile>,
    pub commands: Vec<ProviderCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderAction {
    ToolRequest {
        call_id: String,
        tool: String,
        #[serde(default)]
        path: String,
        #[serde(default)]
        content: String,
        #[serde(default)]
        argv: Vec<String>,
        #[serde(default = "default_command_timeout")]
        timeout_secs: u64,
    },
    Final {
        #[serde(default)]
        files: Vec<PatchFile>,
        #[serde(default)]
        explanation: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub known: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("provider message is not an object")]
    MessageShape,
    #[error("provider message contains a non-text part: {0}")]
    NonTextPart(String),
    #[error("provider message has no text output")]
    MissingText,
    #[error("provider output is not a JSON patch: {0}")]
    InvalidJson(String),
    #[error("provider patch has no files or exceeds the file bound")]
    FileCount,
    #[error("provider file path is unsafe or outside the allowed scope: {0}")]
    Path(String),
    #[error("provider patch has duplicate paths")]
    DuplicatePath,
    #[error("provider file exceeds the content bound")]
    FileSize,
    #[error("provider patch exceeds the total content bound")]
    TotalSize,
    #[error("provider plan contains too many commands")]
    TooManyCommands,
    #[error("provider plan contains an invalid command")]
    InvalidCommand,
}

pub fn extract_patch(
    message: &Value,
    allowed_paths: &[String],
) -> Result<Vec<PatchFile>, PatchError> {
    Ok(extract_plan(message, allowed_paths)?.files)
}

pub fn extract_plan(message: &Value, allowed_paths: &[String]) -> Result<ProviderPlan, PatchError> {
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .ok_or(PatchError::MessageShape)?;
    let mut text = String::new();
    for part in parts {
        let kind = part
            .get("type")
            .and_then(Value::as_str)
            .ok_or(PatchError::MessageShape)?;
        match kind {
            "text" => {
                let value = part
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(PatchError::MessageShape)?;
                text.push_str(value);
            }
            "step-start" | "step-finish" => {}
            other if other.contains("tool") => {
                return Err(PatchError::NonTextPart(other.into()));
            }
            _ => {}
        }
    }
    if text.trim().is_empty() {
        return Err(PatchError::MissingText);
    }
    let text = strip_fence(text.trim());
    let patch: ProviderPatch =
        serde_json::from_str(text).map_err(|error| PatchError::InvalidJson(error.to_string()))?;
    validate_patch(patch, allowed_paths)
}

pub fn command_matches_frozen_check(argv: &[String], checks: &[SpecCheck]) -> bool {
    checks.iter().any(|check| check.command == argv)
}

pub fn extract_action(message: &Value) -> Result<ProviderAction, PatchError> {
    let text = message_text(message)?;
    let text = strip_fence(text.trim());
    serde_json::from_str(text).map_err(|error| PatchError::InvalidJson(error.to_string()))
}

fn message_text(message: &Value) -> Result<String, PatchError> {
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .ok_or(PatchError::MessageShape)?;
    for part in parts {
        let kind = part
            .get("type")
            .and_then(Value::as_str)
            .ok_or(PatchError::MessageShape)?;
        if kind.contains("tool") {
            return Err(PatchError::NonTextPart(kind.into()));
        }
    }
    let text = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    if text.trim().is_empty() {
        return Err(PatchError::MissingText);
    }
    Ok(text)
}

pub fn extract_usage(message: &Value) -> ProviderUsage {
    let info = message.get("info").unwrap_or(&Value::Null);
    let tokens = info.get("tokens").unwrap_or(&Value::Null);
    let input = tokens.get("input").and_then(Value::as_u64).unwrap_or(0);
    let output = tokens.get("output").and_then(Value::as_u64).unwrap_or(0);
    let total = tokens
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input.saturating_add(output));
    let cost = info
        .get("cost")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let known = info.get("tokens").is_some() && info.get("cost").is_some();
    ProviderUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        cost_micros: (cost * 1_000_000.0).round().min(u64::MAX as f64) as u64,
        known,
    }
}

fn validate_patch(
    patch: ProviderPatch,
    allowed_paths: &[String],
) -> Result<ProviderPlan, PatchError> {
    if patch.files.is_empty() || patch.files.len() > MAX_FILES {
        return Err(PatchError::FileCount);
    }
    if patch.commands.len() > 8 {
        return Err(PatchError::TooManyCommands);
    }
    for command in &patch.commands {
        if command.argv.is_empty()
            || command.argv.len() > 64
            || command
                .argv
                .iter()
                .any(|value| value.len() > 4096 || value.contains('\0'))
            || command.argv.iter().map(String::len).sum::<usize>() > 16 * 1024
            || command.timeout_secs == 0
            || command.timeout_secs > 300
        {
            return Err(PatchError::InvalidCommand);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for file in &patch.files {
        if file.path.is_empty()
            || !is_safe_relative_path(&file.path)
            || !path_covered(&file.path, allowed_paths)
        {
            return Err(PatchError::Path(file.path.clone()));
        }
        if !seen.insert(file.path.clone()) {
            return Err(PatchError::DuplicatePath);
        }
        if file.content.len() > MAX_FILE_BYTES {
            return Err(PatchError::FileSize);
        }
        total = total.saturating_add(file.content.len());
    }
    if total > MAX_TOTAL_BYTES {
        return Err(PatchError::TotalSize);
    }
    Ok(ProviderPlan {
        files: patch.files,
        commands: patch.commands,
    })
}

fn strip_fence(text: &str) -> &str {
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .unwrap_or(text);
    text.strip_suffix("```").unwrap_or(text).trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(text: &str) -> Value {
        json!({"info":{"id":"msg","role":"assistant","tokens":{"input":10,"output":20,"total":30},"cost":0.0001},"parts":[{"type":"text","text":text}]})
    }

    #[test]
    fn extracts_a_bounded_text_patch_and_usage() {
        let files = extract_patch(
            &message("```json\n{\"files\":[{\"path\":\"src/a.txt\",\"content\":\"a\"}]}\n```"),
            &["src".into()],
        )
        .unwrap();
        assert_eq!(
            files,
            vec![PatchFile {
                path: "src/a.txt".into(),
                content: "a".into()
            }]
        );
        let usage = extract_usage(&message(""));
        assert!(usage.known);
        assert_eq!(usage.total_tokens, 30);
        assert_eq!(usage.cost_micros, 100);
    }

    #[test]
    fn extracts_bounded_commands_and_rejects_malformed_ones() {
        let plan = message(
            "{\"files\":[{\"path\":\"src/a\",\"content\":\"a\"}],\"commands\":[{\"argv\":[\"cargo\",\"test\"],\"timeout_secs\":30}]}",
        );
        let extracted = extract_plan(&plan, &["src".into()]).unwrap();
        assert_eq!(extracted.commands.len(), 1);
        assert_eq!(extracted.commands[0].argv, vec!["cargo", "test"]);
        let bad = message(
            "{\"files\":[{\"path\":\"src/a\",\"content\":\"a\"}],\"commands\":[{\"argv\":[],\"timeout_secs\":30}]}",
        );
        assert_eq!(
            extract_plan(&bad, &["src".into()]),
            Err(PatchError::InvalidCommand)
        );
    }

    #[test]
    fn parses_tool_requests_and_final_actions() {
        let tool = message(
            "{\"type\":\"tool_request\",\"call_id\":\"call-1\",\"tool\":\"run_command\",\"argv\":[\"cargo\",\"test\"],\"timeout_secs\":30}",
        );
        assert_eq!(
            extract_action(&tool).unwrap(),
            ProviderAction::ToolRequest {
                call_id: "call-1".into(),
                tool: "run_command".into(),
                path: String::new(),
                content: String::new(),
                argv: vec!["cargo".into(), "test".into()],
                timeout_secs: 30,
            }
        );
        let final_action =
            message("{\"type\":\"final\",\"files\":[{\"path\":\"src/a\",\"content\":\"a\"}]}");
        assert!(matches!(
            extract_action(&final_action).unwrap(),
            ProviderAction::Final { files, .. } if files.len() == 1
        ));
    }

    #[test]
    fn provider_commands_require_an_exact_frozen_check_match() {
        let checks = vec![SpecCheck {
            id: "functional".into(),
            kind: crate::lead::CheckKind::Functional,
            description: "functional".into(),
            command: vec!["cargo".into(), "test".into()],
            expect_exit: 0,
            timeout_secs: 30,
        }];
        assert!(command_matches_frozen_check(
            &["cargo".into(), "test".into()],
            &checks
        ));
        assert!(!command_matches_frozen_check(
            &["cargo".into(), "test".into(), "--all".into()],
            &checks
        ));
        assert!(!command_matches_frozen_check(&["sh".into()], &checks));
    }

    #[test]
    fn rejects_tool_parts_out_of_scope_paths_and_duplicates() {
        let tool = json!({"info":{"id":"msg"},"parts":[{"type":"tool","tool":"shell"}]});
        assert!(matches!(
            extract_patch(&tool, &["src".into()]),
            Err(PatchError::NonTextPart(_))
        ));
        let out = message("{\"files\":[{\"path\":\"../escape\",\"content\":\"x\"}]}");
        assert!(matches!(
            extract_patch(&out, &["src".into()]),
            Err(PatchError::Path(_))
        ));
        let duplicate = message(
            "{\"files\":[{\"path\":\"src/a\",\"content\":\"a\"},{\"path\":\"src/a\",\"content\":\"b\"}]}",
        );
        assert_eq!(
            extract_patch(&duplicate, &["src".into()]),
            Err(PatchError::DuplicatePath)
        );
    }
}
