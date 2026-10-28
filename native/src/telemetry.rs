//! Bounded credential redaction for process logs.
//!
//! The Factory registers its long-lived role credentials at startup. Transient
//! credentials (for example a submitted or decrypted GitHub token) use scoped
//! registrations that are removed when their last consumer drops, and the
//! owning [`SecretValue`] zeroizes its bytes on drop. Redaction is best effort
//! and is not a substitute for never logging prompts, source bodies or secrets.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    sync::{LazyLock, RwLock},
};

use regex::Regex;
use sha2::{Digest, Sha256};

const MAX_SCOPED_SECRETS: usize = 64;

static SECRETS: LazyLock<RwLock<BTreeSet<String>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));

/// Transient secrets keyed by SHA-256 of their value. The key never contains
/// plaintext; the entry owns a zeroized-on-removal copy for redaction.
static SCOPED_SECRETS: LazyLock<RwLock<BTreeMap<[u8; 32], ScopedEntry>>> =
    LazyLock::new(|| RwLock::new(BTreeMap::new()));

static BEARER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(bearer\s+)[^\s,;\x22]+").expect("bearer redaction pattern")
});

static KEYED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(token|secret|password|api[_-]?key|authorization|set-cookie)(\s*[:=]\s*)[^\s,;\x22]+",
    )
    .expect("keyed redaction pattern")
});

struct ScopedEntry {
    token: Vec<u8>,
    references: usize,
}

impl Drop for ScopedEntry {
    fn drop(&mut self) {
        self.token.fill(0);
    }
}

/// Long-lived redaction registration used for process role credentials.
pub fn register_secret(value: &str) {
    if value.len() < 8 {
        return;
    }
    if let Ok(mut secrets) = SECRETS.write() {
        secrets.insert(value.to_owned());
    }
}

/// Scoped redaction registration. The secret is removed from the process
/// registry after the last guard drops; the registry copy is zeroized then.
/// Registration fails closed when the bounded registry is full.
pub struct ScopedSecret {
    digest: [u8; 32],
}

impl ScopedSecret {
    pub fn register(value: &str) -> Option<Self> {
        if value.len() < 8 {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        let mut secrets = SCOPED_SECRETS.write().ok()?;
        if let Some(entry) = secrets.get_mut(&digest) {
            entry.references = entry.references.saturating_add(1);
            return Some(Self { digest });
        }
        if secrets.len() >= MAX_SCOPED_SECRETS {
            return None;
        }
        secrets.insert(
            digest,
            ScopedEntry {
                token: value.as_bytes().to_vec(),
                references: 1,
            },
        );
        Some(Self { digest })
    }
}

impl Drop for ScopedSecret {
    fn drop(&mut self) {
        let Ok(mut secrets) = SCOPED_SECRETS.write() else {
            return;
        };
        let remove = match secrets.get_mut(&self.digest) {
            Some(entry) => {
                entry.references = entry.references.saturating_sub(1);
                entry.references == 0
            }
            None => false,
        };
        if remove {
            // Removing the entry drops it; ScopedEntry::drop zeroizes the copy.
            let _ = secrets.remove(&self.digest);
        }
    }
}

/// Owns a transient plaintext secret and zeroizes its bytes on drop.
pub struct SecretValue {
    bytes: Vec<u8>,
}

impl SecretValue {
    pub fn from_string(value: String) -> Self {
        Self {
            bytes: value.into_bytes(),
        }
    }

    pub fn from_utf8(bytes: Vec<u8>) -> Result<Self, std::str::Utf8Error> {
        std::str::from_utf8(&bytes)?;
        Ok(Self { bytes })
    }

    /// The bytes are validated at construction, so the fallback is only a
    /// defensive empty value and never a panic.
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or_default()
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue(<redacted>)")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

pub fn redact(value: &str) -> String {
    let mut output = value.to_owned();
    if let Ok(secrets) = SECRETS.read() {
        for secret in secrets.iter() {
            output = output.replace(secret, "[REDACTED]");
        }
    }
    if let Ok(secrets) = SCOPED_SECRETS.read() {
        for entry in secrets.values() {
            if let Ok(secret) = std::str::from_utf8(&entry.token) {
                output = output.replace(secret, "[REDACTED]");
            }
        }
    }
    output = BEARER.replace_all(&output, "$1[REDACTED]").into_owned();
    KEYED
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!("{}{}[REDACTED]", &captures[1], &captures[2])
        })
        .into_owned()
}

#[cfg(test)]
pub(crate) fn scoped_secret_count() -> usize {
    SCOPED_SECRETS
        .read()
        .map(|secrets| secrets.len())
        .unwrap_or_default()
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
        assert!(ScopedSecret::register("short").is_none());
    }

    #[test]
    fn scoped_secrets_refcount_and_are_removed() {
        let baseline = scoped_secret_count();
        let first = ScopedSecret::register("scoped-secret-alpha-1234567890").unwrap();
        let second = ScopedSecret::register("scoped-secret-alpha-1234567890").unwrap();
        assert_eq!(scoped_secret_count(), baseline + 1);
        assert!(!redact("value=scoped-secret-alpha-1234567890").contains("alpha"));
        drop(first);
        assert_eq!(scoped_secret_count(), baseline + 1);
        drop(second);
        assert_eq!(scoped_secret_count(), baseline);
        assert!(redact("value=scoped-secret-alpha-1234567890").contains("alpha"));
    }

    #[test]
    fn secret_value_redacts_debug_and_owns_valid_utf8() {
        let value = SecretValue::from_string("secret-value-1234567890".into());
        assert_eq!(value.expose(), "secret-value-1234567890");
        assert_eq!(format!("{value:?}"), "SecretValue(<redacted>)");
        let bytes = SecretValue::from_utf8(b"secret-bytes-1234567890".to_vec()).unwrap();
        assert_eq!(bytes.expose(), "secret-bytes-1234567890");
        assert!(SecretValue::from_utf8(vec![0xff, 0xfe]).is_err());
    }
}
