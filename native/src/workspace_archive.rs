//! Bounded, symlink-free snapshots of a disposable Attempt worktree.
//!
//! The sandbox receives a Podman named volume populated from this archive. No
//! host workspace pathname is ever passed to the container runtime, so replacing
//! the host path cannot redirect the sandbox mount. Host-side extraction is
//! restricted to regular files and directories under the caller's staging root.

use std::{
    fs,
    io::Cursor,
    path::{Component, Path},
    time::UNIX_EPOCH,
};

use sha2::{Digest, Sha256};
use tar::{Archive, Builder, EntryType, Header};

const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("workspace archive I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace archive is invalid: {0}")]
    Invalid(String),
    #[error("workspace archive exceeds its bound: {0}")]
    TooLarge(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub entries: usize,
    pub content_bytes: u64,
}

/// Snapshot every regular file and directory under `root`, excluding Git
/// metadata. Symbolic links, hard links, devices, FIFOs and sockets fail closed.
pub fn snapshot(root: &Path) -> Result<Snapshot, ArchiveError> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(ArchiveError::Invalid(
            "workspace root is not a directory".into(),
        ));
    }
    let mut builder = Builder::new(Vec::new());
    builder.follow_symlinks(false);
    let mut state = SnapshotState {
        entries: 0,
        content_bytes: 0,
    };
    append_directory(&mut builder, &root, &root, &mut state)?;
    let bytes = builder.into_inner().map_err(ArchiveError::Io)?;
    let sha256 = hex(&Sha256::digest(&bytes));
    Ok(Snapshot {
        bytes,
        sha256,
        entries: state.entries,
        content_bytes: state.content_bytes,
    })
}

struct SnapshotState {
    entries: usize,
    content_bytes: u64,
}

fn append_directory(
    builder: &mut Builder<Vec<u8>>,
    root: &Path,
    directory: &Path,
    state: &mut SnapshotState,
) -> Result<(), ArchiveError> {
    let mut children: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child.file_name();
        if name == ".git" {
            continue;
        }
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ArchiveError::Invalid("workspace path escaped its root".into()))?;
        validate_relative(relative)?;
        let file_type = metadata.file_type();
        state.entries = state.entries.saturating_add(1);
        if state.entries > MAX_ENTRIES {
            return Err(ArchiveError::TooLarge("too many workspace entries".into()));
        }
        if file_type.is_symlink() {
            return Err(ArchiveError::Invalid(format!(
                "workspace contains a symbolic link: {}",
                relative.display()
            )));
        }
        if file_type.is_dir() {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Directory);
            header.set_mode(0o755);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_size(0);
            header.set_cksum();
            builder.append_data(&mut header, relative, std::io::empty())?;
            append_directory(builder, root, &path, state)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(ArchiveError::Invalid(format!(
                "workspace contains a non-regular file: {}",
                relative.display()
            )));
        }
        let size = metadata.len();
        if size > MAX_ENTRY_BYTES {
            return Err(ArchiveError::TooLarge(format!(
                "workspace file is too large: {}",
                relative.display()
            )));
        }
        state.content_bytes = state.content_bytes.saturating_add(size);
        if state.content_bytes > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::TooLarge(
                "workspace content is too large".into(),
            ));
        }
        let mode = file_mode(&metadata) & 0o755;
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(if mode == 0 { 0o644 } else { mode });
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_secs()),
        );
        header.set_size(size);
        header.set_cksum();
        let mut file = fs::File::open(&path)?;
        builder.append_data(&mut header, relative, &mut file)?;
    }
    Ok(())
}

/// Extract a sandbox-produced archive into a fresh staging directory. The
/// caller then replaces the Git worktree contents except `.git`.
pub fn extract(bytes: &[u8], destination: &Path) -> Result<usize, ArchiveError> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge("archive is too large".into()));
    }
    if destination.exists() {
        return Err(ArchiveError::Invalid(
            "staging destination already exists".into(),
        ));
    }
    fs::create_dir_all(destination)?;
    set_mode(destination, 0o700)?;
    let mut archive = Archive::new(Cursor::new(bytes));
    let mut count = 0usize;
    let mut content = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        count = count.saturating_add(1);
        if count > MAX_ENTRIES {
            return Err(ArchiveError::TooLarge("too many archive entries".into()));
        }
        let path = entry.path()?.into_owned();
        validate_relative(&path)?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(ArchiveError::Invalid(format!(
                "archive contains a link: {}",
                path.display()
            )));
        }
        if entry_type.is_dir() {
            let target = destination.join(&path);
            fs::create_dir_all(&target)?;
            set_mode(&target, 0o755)?;
            continue;
        }
        if !entry_type.is_file() {
            return Err(ArchiveError::Invalid(format!(
                "archive contains a special entry: {}",
                path.display()
            )));
        }
        let size = entry.header().size()?;
        if size > MAX_ENTRY_BYTES {
            return Err(ArchiveError::TooLarge(format!(
                "archive file is too large: {}",
                path.display()
            )));
        }
        content = content.saturating_add(size);
        if content > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::TooLarge(
                "archive content is too large".into(),
            ));
        }
        let target = destination.join(&path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::copy(&mut entry, &mut output)?;
        output.sync_all()?;
        let mode = entry.header().mode().unwrap_or(0o644) & 0o755;
        set_mode(&target, if mode == 0 { 0o644 } else { mode })?;
    }
    Ok(count)
}

fn validate_relative(path: &Path) -> Result<(), ArchiveError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ArchiveError::Invalid(
            "archive path is empty or absolute".into(),
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(value) if value != ".git" => {}
            _ => {
                return Err(ArchiveError::Invalid(format!(
                    "archive path is unsafe: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode()
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0o644
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), ArchiveError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), ArchiveError> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("factory-archive-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn snapshots_excludes_git_and_round_trips_regular_files() {
        let source = Temp::new();
        fs::create_dir(source.0.join("src")).unwrap();
        fs::write(source.0.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(source.0.join(".git"), "gitdir: elsewhere\n").unwrap();
        fs::write(source.0.join("executable"), "#!/bin/sh\n").unwrap();
        set_mode(&source.0.join("executable"), 0o755).unwrap();
        let snapshot = snapshot(&source.0).unwrap();
        assert!(snapshot.entries >= 3);
        assert_eq!(snapshot.sha256.len(), 64);

        let output = Temp::new();
        let destination = output.0.join("stage");
        extract(&snapshot.bytes, &destination).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert!(!destination.join(".git").exists());
    }

    #[test]
    fn rejects_symlinks_and_unsafe_archives() {
        let source = Temp::new();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", source.0.join("link")).unwrap();
        assert!(matches!(snapshot(&source.0), Err(ArchiveError::Invalid(_))));

        assert!(validate_relative(Path::new("../escape")).is_err());
        assert!(validate_relative(Path::new("/absolute")).is_err());
        assert!(validate_relative(Path::new(".git/config")).is_err());

        let output = Temp::new();
        let destination = output.0.join("stage");
        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("/etc/passwd").unwrap();
        header.set_cksum();
        builder
            .append_data(&mut header, "link", std::io::empty())
            .unwrap();
        let bytes = builder.into_inner().unwrap();
        assert!(extract(&bytes, &destination).is_err());
    }
}
