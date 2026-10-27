//! Protected provider credential material for Core-owned configuration.
//!
//! Credentials are encrypted with AES-256-GCM under a master key stored in a
//! separate protected file. The database stores only ciphertext, a nonce and a
//! one-way fingerprint. Credentials are never returned to the browser.

use std::path::Path;

use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use sha2::{Digest, Sha256};

const NONCE_BYTES: usize = 12;
const KEY_BYTES: usize = 32;
const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct ProviderSecretKey {
    key: [u8; KEY_BYTES],
}

impl std::fmt::Debug for ProviderSecretKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderSecretKey(<redacted>)")
    }
}

impl ProviderSecretKey {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("master key file must be absolute".into());
        }
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| format!("cannot inspect master key file: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err("master key file must be a regular non-symlink file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o137 != 0 {
                return Err("master key file permissions are broader than owner/group read".into());
            }
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read master key file: {error}"))?;
        let raw = raw.trim();
        if raw.len() != KEY_BYTES * 2 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("master key file must contain 64 hex characters".into());
        }
        let mut key = [0u8; KEY_BYTES];
        for (index, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
            let text = std::str::from_utf8(pair).map_err(|_| "master key is not UTF-8")?;
            key[index] =
                u8::from_str_radix(text, 16).map_err(|_| "master key contains invalid hex")?;
        }
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedCredential, String> {
        if plaintext.is_empty() || plaintext.len() > MAX_CREDENTIAL_BYTES {
            return Err("credential is empty or too large".into());
        }
        let mut nonce = [0u8; NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| "system randomness unavailable")?;
        let unbound = UnboundKey::new(&AES_256_GCM, &self.key)
            .map_err(|_| "credential encryption key is invalid")?;
        let key = LessSafeKey::new(unbound);
        let mut ciphertext = plaintext.to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| "credential encryption failed")?;
        let fingerprint = Sha256::digest(plaintext);
        Ok(EncryptedCredential {
            ciphertext: hex(&ciphertext),
            nonce: hex(&nonce),
            fingerprint: fingerprint
                .iter()
                .take(8)
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        })
    }

    pub fn decrypt(&self, ciphertext: &str, nonce: &str) -> Result<Vec<u8>, String> {
        let mut ciphertext = unhex(ciphertext)?;
        let nonce = unhex(nonce)?;
        let nonce: [u8; NONCE_BYTES] = nonce
            .try_into()
            .map_err(|_| "credential nonce is invalid")?;
        let unbound = UnboundKey::new(&AES_256_GCM, &self.key)
            .map_err(|_| "credential encryption key is invalid")?;
        let key = LessSafeKey::new(unbound);
        let plaintext = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| "credential decryption failed")?;
        Ok(plaintext.to_vec())
    }
}

pub struct EncryptedCredential {
    pub ciphertext: String,
    pub nonce: String,
    pub fingerprint: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("encrypted credential encoding is invalid".into());
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let text = std::str::from_utf8(pair).map_err(|_| "credential is not UTF-8")?;
        output.push(u8::from_str_radix(text, 16).map_err(|_| "credential hex is invalid")?);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn key() -> ProviderSecretKey {
        let path =
            std::env::temp_dir().join(format!("factory-master-key-{}.txt", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all("11".repeat(32).as_bytes()).unwrap();
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let key = ProviderSecretKey::from_file(&path).unwrap();
        let _ = std::fs::remove_file(path);
        key
    }

    #[test]
    fn encrypts_and_decrypts_without_exposing_plaintext() {
        let key = key();
        let encrypted = key.encrypt(b"super-secret-provider-credential").unwrap();
        assert!(!encrypted.ciphertext.contains("super-secret"));
        assert_eq!(encrypted.fingerprint.len(), 16);
        assert_eq!(
            key.decrypt(&encrypted.ciphertext, &encrypted.nonce)
                .unwrap(),
            b"super-secret-provider-credential"
        );
    }

    #[test]
    fn rejects_broad_master_key_permissions() {
        let path =
            std::env::temp_dir().join(format!("factory-master-open-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, "11".repeat(32)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ProviderSecretKey::from_file(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_symlinked_master_key() {
        let target = std::env::temp_dir().join(format!(
            "factory-master-target-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&target, "11".repeat(32)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link =
            std::env::temp_dir().join(format!("factory-master-link-{}.txt", uuid::Uuid::new_v4()));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ProviderSecretKey::from_file(&link).is_err());
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_file(target);
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let key = key();
        let mut encrypted = key.encrypt(b"credential-value").unwrap();
        let tamper = if encrypted.ciphertext.starts_with("00") {
            "01"
        } else {
            "00"
        };
        encrypted.ciphertext.replace_range(0..2, tamper);
        assert!(
            key.decrypt(&encrypted.ciphertext, &encrypted.nonce)
                .is_err()
        );
    }
}
