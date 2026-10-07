//! A location-independent identity for Caskrun's own packaged Wine runtime.
//! Custom Wine installations intentionally remain pinned to their path.

use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::{cell::Cell, thread_local};

// Refuse implausibly large or hostile trees instead of spending unbounded time
// hashing a path supplied by a caller.
const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const O_NOFOLLOW_ANY: i32 = 0x2000_0000;

#[cfg(test)]
thread_local! {
    static FULL_CONTENT_HASHES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn full_content_hash_count() -> usize {
    FULL_CONTENT_HASHES.with(Cell::get)
}

/// The content digest permits relocation only to an identical package. The
/// metadata fingerprint makes checks at the already verified location fast;
/// it is never sufficient evidence to authorize a relocation.
pub(crate) struct BundleAttestation {
    pub(crate) identity: String,
    pub(crate) metadata_fingerprint: String,
}

/// Detects one-field corruption of a bottle record before the metadata fast
/// path can run. This is deliberately *not* an authenticator: both inputs are
/// persisted in a writable record, so a same-user process can forge both.
pub(crate) fn bind_record_fingerprint(identity: &str, metadata_fingerprint: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"caskrun-bundled-wine-record-binding-v2\0");
    update_bytes(&mut digest, identity.as_bytes());
    update_bytes(&mut digest, metadata_fingerprint.as_bytes());
    format!("sha256-bound-v2:{:x}", digest.finalize())
}

/// Hashes every file's contents, relative symlinks, path and executable mode.
/// The digest excludes the package's parent path, so an exact copy may move.
pub(crate) fn bundled_runtime_attestation(
    wine: &Path,
) -> Result<Option<BundleAttestation>, String> {
    let Some(root) = bundled_root(wine)? else {
        return Ok(None);
    };
    #[cfg(test)]
    FULL_CONTENT_HASHES.with(|count| count.set(count.get() + 1));
    let mut state = HashState::new(true);
    hash_directory(&root, &root, &mut state)?;
    Ok(Some(BundleAttestation {
        identity: format!("sha256:{:x}", state.content.unwrap().finalize()),
        metadata_fingerprint: format!("sha256:{:x}", state.metadata.finalize()),
    }))
}

/// Used only for an engine at the exact previously verified canonical path.
/// A mismatch causes the caller to rehash file contents before using it.
pub(crate) fn bundled_runtime_metadata_fingerprint(wine: &Path) -> Result<Option<String>, String> {
    let Some(root) = bundled_root(wine)? else {
        return Ok(None);
    };
    let mut state = HashState::new(false);
    hash_directory(&root, &root, &mut state)?;
    Ok(Some(format!("sha256:{:x}", state.metadata.finalize())))
}

#[cfg(test)]
pub(crate) fn bundled_runtime_identity(wine: &Path) -> Result<Option<String>, String> {
    Ok(bundled_runtime_attestation(wine)?.map(|value| value.identity))
}

fn bundled_root(wine: &Path) -> Result<Option<PathBuf>, String> {
    if wine.file_name() != Some(OsStr::new("wine"))
        || wine.parent().and_then(Path::file_name) != Some(OsStr::new("bin"))
    {
        return Ok(None);
    }
    let Some(root) = wine.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    // These paths identify the independently packaged runtime layout. A
    // partial package is an error rather than silently becoming custom Wine.
    if !root.join("SOURCES.txt").exists() || !root.join("bin/_dispatch").exists() {
        return Ok(None);
    }
    for required in [
        "bin/wine",
        "bin/wineboot",
        "bin/wineserver",
        "runtime/bin/wine",
        "runtime/bin/wineboot",
        "runtime/bin/wineserver",
        "lib/libfreetype.6.dylib",
    ] {
        if !root.join(required).exists() {
            return Err(format!("Incomplete bundled Wine runtime: {required}"));
        }
    }
    root.canonicalize()
        .map(Some)
        .map_err(|e| format!("Cannot inspect bundled Wine runtime: {e}"))
}

struct HashState {
    content: Option<Sha256>,
    metadata: Sha256,
    entries: usize,
    bytes: u64,
}

impl HashState {
    fn new(include_contents: bool) -> Self {
        let content = include_contents.then(|| {
            let mut digest = Sha256::new();
            digest.update(b"caskrun-bundled-wine-tree-v1\0");
            digest
        });
        let mut metadata = Sha256::new();
        metadata.update(b"caskrun-bundled-wine-metadata-v1\0");
        Self {
            content,
            metadata,
            entries: 0,
            bytes: 0,
        }
    }
}

fn update_len(digest: &mut Sha256, length: u64) {
    digest.update(length.to_be_bytes());
}

fn update_bytes(digest: &mut Sha256, bytes: &[u8]) {
    update_len(digest, bytes.len() as u64);
    digest.update(bytes);
}

fn unchanged(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mode() == after.mode()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

fn update_metadata(digest: &mut Sha256, relative: &Path, kind: u8, metadata: &fs::Metadata) {
    digest.update([kind]);
    update_bytes(digest, relative.as_os_str().as_bytes());
    for value in [
        metadata.dev(),
        metadata.ino(),
        metadata.mode() as u64,
        metadata.len(),
        metadata.mtime() as u64,
        metadata.mtime_nsec() as u64,
        metadata.ctime() as u64,
        metadata.ctime_nsec() as u64,
    ] {
        update_len(digest, value);
    }
}

fn hash_directory(root: &Path, dir: &Path, state: &mut HashState) -> Result<(), String> {
    let before = fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if !before.file_type().is_dir() {
        return Err("Bundled Wine runtime directory changed while hashing".into());
    }
    let relative_dir = dir.strip_prefix(root).map_err(|e| e.to_string())?;
    update_metadata(&mut state.metadata, relative_dir, b'D', &before);
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|value| value.path()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for path in entries {
        state.entries += 1;
        if state.entries > MAX_ENTRIES {
            return Err("Bundled Wine runtime has too many files".into());
        }
        let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            if let Some(content) = &mut state.content {
                content.update(b"D");
                update_bytes(content, relative.as_os_str().as_bytes());
            }
            hash_directory(root, &path, state)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&path).map_err(|e| e.to_string())?;
            if target.is_absolute()
                || !path
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(root)
            {
                return Err("Bundled Wine runtime contains an external symlink".into());
            }
            update_metadata(&mut state.metadata, relative, b'L', &metadata);
            update_bytes(&mut state.metadata, target.as_os_str().as_bytes());
            if let Some(content) = &mut state.content {
                content.update(b"L");
                update_bytes(content, relative.as_os_str().as_bytes());
                update_bytes(content, target.as_os_str().as_bytes());
            }
            if !unchanged(
                &metadata,
                &fs::symlink_metadata(&path).map_err(|e| e.to_string())?,
            ) {
                return Err("Bundled Wine runtime changed while hashing".into());
            }
        } else if file_type.is_file() {
            if state.bytes.saturating_add(metadata.len()) > MAX_BYTES {
                return Err("Bundled Wine runtime exceeds the size limit".into());
            }
            state.bytes += metadata.len();
            update_metadata(&mut state.metadata, relative, b'F', &metadata);
            if let Some(content) = &mut state.content {
                content.update(b"F");
                update_bytes(content, relative.as_os_str().as_bytes());
                update_len(content, u64::from(metadata.permissions().mode() & 0o777));
                update_len(content, metadata.len());
                let mut file = OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NOFOLLOW_ANY)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                if !unchanged(&metadata, &file.metadata().map_err(|e| e.to_string())?) {
                    return Err("Bundled Wine runtime changed while hashing".into());
                }
                let mut buffer = [0u8; 128 * 1024];
                loop {
                    let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
                    if count == 0 {
                        break;
                    }
                    content.update(&buffer[..count]);
                }
                if !unchanged(&metadata, &file.metadata().map_err(|e| e.to_string())?) {
                    return Err("Bundled Wine runtime changed while hashing".into());
                }
            } else if !unchanged(
                &metadata,
                &fs::symlink_metadata(&path).map_err(|e| e.to_string())?,
            ) {
                return Err("Bundled Wine runtime changed while hashing".into());
            }
        } else {
            return Err("Bundled Wine runtime contains a special file".into());
        }
    }
    if !unchanged(
        &before,
        &fs::symlink_metadata(dir).map_err(|e| e.to_string())?,
    ) {
        return Err("Bundled Wine runtime changed while hashing".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "requires CASKRUN_BENCH_WINE pointing to a locally built runtime"]
    fn benchmark_real_bundle() {
        let Some(wine) = std::env::var_os("CASKRUN_BENCH_WINE").map(PathBuf::from) else {
            eprintln!("Set CASKRUN_BENCH_WINE to benchmark a local package");
            return;
        };
        let start = Instant::now();
        let attestation = bundled_runtime_attestation(&wine).unwrap().unwrap();
        let full_elapsed = start.elapsed();
        let start = Instant::now();
        let fingerprint = bundled_runtime_metadata_fingerprint(&wine)
            .unwrap()
            .unwrap();
        let quick_elapsed = start.elapsed();
        assert_eq!(fingerprint, attestation.metadata_fingerprint);
        eprintln!(
            "bundled runtime full content hash: {full_elapsed:?}; metadata check: {quick_elapsed:?}"
        );
    }
}
