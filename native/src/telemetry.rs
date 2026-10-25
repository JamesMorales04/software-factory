//! Bounded credential redaction for process logs.
//!
//! The Factory registers its own role credentials at startup and masks common
//! credential-shaped key/value and bearer forms. Redaction is best effort and
//! is not a substitute for never logging prompts, source bodies or secrets.

use std::{
    collections::BTreeSet,
    io::{self, Write},
    sync::{LazyLock, RwLock},
};

use regex::Regex;

static SECRETS: LazyLock<RwLock<BTreeSet<String>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));

static BEARER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(bearer\s+)[^\s,;\x22]+").expect("bearer redaction pattern")
});

static KEYED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(token|secret|password|api[_-]?key|authorization|set-cookie)(\s*[:=]\s*)[^\s,;\x22]+",
    )
    .expect("keyed redaction pattern")
});

pub fn register_secret(value: &str) {
    if value.len() < 8 {
        return;
    }
    if let Ok(mut secrets) = SECRETS.write() {
        secrets.insert(value.to_owned());
    }
}

pub fn redact(value: &str) -> String {
    let mut output = value.to_owned();
    if let Ok(secrets) = SECRETS.read() {
        for secret in secrets.iter() {
            output = output.replace(secret, "[REDACTED]");
        }
    }
    output = BEARER.replace_all(&output, "$1[REDACTED]").into_owned();
    KEYED
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!("{}{}[REDACTED]", &captures[1], &captures[2])
        })
        .into_owned()
}

pub struct RedactingWriter;

impl Write for RedactingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buffer);
        let redacted = redact(&text);
        io::stderr().write_all(redacted.as_bytes())?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

#[derive(Clone, Copy, Default)]
pub struct RedactingMakeWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_registered_secrets_and_common_forms() {
        register_secret("operator-secret-1234567890");
        let redacted = redact(
            "operator-secret-1234567890 Authorization: Bearer abc.def token=abc, password: hunter2 api_key=xyz",
        );
        assert!(!redacted.contains("operator-secret-1234567890"));
        assert!(!redacted.contains("abc.def"));
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("xyz"));
        assert!(redacted.contains("[REDACTED]"));
        assert!(redacted.contains("Authorization:"));
    }

    #[test]
    fn preserves_ordinary_text_and_short_values() {
        register_secret("short");
        assert_eq!(
            redact("action created for project alpha"),
            "action created for project alpha"
        );
        assert_eq!(redact("token"), "token");
    }
}
