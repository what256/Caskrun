//! Caskrun's portable folder backup inside a bounded, checksummed gzip/tar
//! envelope. Checksums detect corruption; they do not authenticate a publisher.
//! Extraction only writes an owned staging directory. The existing bottle
//! importer remains the final authority for manifest, profile and link policy.

use crate::*;
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;

const INTEGRITY: &str = "caskrun-archive.json";
const STAGING: &str = "archive-staging";
const MANIFEST_LIMIT: u64 = 16 * 1024 * 1024;
const PATH_LIMIT: usize = 4096;
const MAX_ENTRIES: usize = 100_000;
const MAX_FILE: u64 = 64 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 256 * 1024 * 1024 * 1024;
const MAX_TRAILER: u64 = 1024 * 1024;

#[derive(Clone, Copy)]
struct Limits {
    entries: usize,
    file: u64,
    total: u64,
    metadata: u64,
}
const LIMITS: Limits = Limits {
    entries: MAX_ENTRIES,
    file: MAX_FILE,
    total: MAX_TOTAL,
    metadata: MANIFEST_LIMIT,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Integrity {
    format: String,
    version: u32,
    entries: Vec<Item>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Item {
    path: String,
    mode: u32,
    kind: Kind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Kind {
    Directory,
    File { size: u64, sha256: String },
    Symlink { target: String },
}

struct Stage {
    path: PathBuf,
    guard: File,
}
impl Stage {
    fn create(home: &Path) -> Result<Self, String> {
        ensure_independent_storage(home)?;
        fs::create_dir_all(home).map_err(|e| e.to_string())?;
        let (_home, _) = open_stable_directory(home)?;
        let parent = real_subdirectory(home, STAGING)?;
        let path = parent.join(Uuid::new_v4().to_string());
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|e| e.to_string())?;
        let (guard, _) = open_stable_directory(&path)?;
        Ok(Self { path, guard })
    }
    fn finish<T>(self, result: Result<T, String>) -> Result<T, String> {
        let cleanup = cleanup_stage(&self.path, &self.guard);
        match (result, cleanup) {
            (result, Ok(())) => result,
            (Ok(_), Err(error)) => Err(format!(
                "Backup operation finished, but its owned staging files could not be removed: {error}"
            )),
            (Err(error), Err(cleanup)) => Err(format!(
                "{error}. Owned backup staging cleanup failed: {cleanup}"
            )),
        }
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = cleanup_stage(&self.path, &self.guard);
    }
}

fn cleanup_stage(path: &Path, guard: &File) -> Result<(), String> {
    let now = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if !same_identity(&now, &guard.metadata().map_err(|e| e.to_string())?) {
        return Err("Staging directory identity changed; it was preserved".into());
    }
    guard
        .set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if metadata.file_type().is_dir() {
            let (child, _) = open_stable_directory(&entry.path())?;
            cleanup_stage(&entry.path(), &child)?;
        } else {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    if !same_identity(
        &fs::symlink_metadata(path).map_err(|e| e.to_string())?,
        &guard.metadata().map_err(|e| e.to_string())?,
    ) {
        return Err("Staging directory changed during cleanup".into());
    }
    fs::remove_dir(path).map_err(|e| e.to_string())
}

pub(super) fn export_archive(home: &Path, id: &str, output: &Path) -> Result<BackupResult, String> {
    if output.extension().and_then(OsStr::to_str) != Some("caskrun-backup") {
        return Err("Compressed backup destination must end in .caskrun-backup".into());
    }
    let name = output.file_name().ok_or("Choose a backup filename")?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    ensure_independent_storage(&parent)?;
    let (parent_guard, parent_snapshot) = open_stable_directory(&parent)?;
    let destination = parent.join(name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err("Backup destination already exists".into());
    }
    let source = bottle_dir(home, id)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let storage = storage_root(home, false)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let staging = home
        .join(STAGING)
        .canonicalize()
        .unwrap_or(home.join(STAGING));
    if parent.starts_with(&storage) || parent.starts_with(&source) || parent.starts_with(&staging) {
        return Err("Store compressed backups outside bottle and staging storage".into());
    }
    {
        let _lock = lock_bottle(home, id)?;
        refuse_running_bottle(&source)?;
        prefix_processes::wait_until_idle(&source, Duration::from_secs(3))?;
    }
    let stage = Stage::create(home)?;
    let result = (|| {
        let snapshot = stage.path.join("snapshot.caskrun-bottle");
        let mut result = export_bottle(home, id, &snapshot)?;
        let entries = scan_snapshot(&snapshot, LIMITS)?;
        let integrity = Integrity {
            format: "caskrun-compressed-bottle".into(),
            version: 1,
            entries,
        };
        let temporary = parent.join(format!(".caskrun-backup-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let written = (|| {
            write_archive(&snapshot, &integrity, &mut file)?;
            file.sync_all().map_err(|e| e.to_string())?;
            bottle_control::assert_same_prefix(&parent, &parent_guard, &parent_snapshot)?;
            cancellation::check()?;
            let from = CString::new(temporary.file_name().unwrap().as_bytes())
                .map_err(|e| e.to_string())?;
            let to = CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
            if unsafe {
                renameatx_np(
                    parent_guard.as_raw_fd(),
                    from.as_ptr(),
                    parent_guard.as_raw_fd(),
                    to.as_ptr(),
                    RENAME_EXCL | RENAME_NOFOLLOW_ANY,
                )
            } != 0
            {
                return Err(format!(
                    "Could not publish backup without overwriting another file: {}",
                    std::io::Error::last_os_error()
                ));
            }
            parent_guard.sync_all().map_err(|e| e.to_string())
        })();
        if written.is_err()
            && fs::symlink_metadata(&temporary).is_ok_and(|m| {
                m.dev() == file.metadata().map_or(0, |m| m.dev())
                    && m.ino() == file.metadata().map_or(0, |m| m.ino())
            })
        {
            let _ = fs::remove_file(&temporary);
        }
        written?;
        result.path = destination.display().to_string();
        Ok(result)
    })();
    stage.finish(result)
}

pub(super) fn import_archive(
    home: &Path,
    source: &Path,
    name: Option<&str>,
) -> Result<ImportResult, String> {
    let before = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() || before.len() > MAX_TOTAL {
        return Err("Choose a bounded regular .caskrun-backup file".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(source)
        .map_err(|e| e.to_string())?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Backup changed while opening it".into());
    }
    let stage = Stage::create(home)?;
    let result = (|| {
        let snapshot = stage.path.join("snapshot.caskrun-bottle");
        fs::create_dir(&snapshot).map_err(|e| e.to_string())?;
        read_archive(BufReader::new(&file), &snapshot, LIMITS)?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
            || !same_snapshot(
                &before,
                &fs::symlink_metadata(source).map_err(|e| e.to_string())?,
            )
        {
            return Err("Backup changed during verification".into());
        }
        cancellation::check()?;
        import_bottle(home, &snapshot, name)
    })();
    stage.finish(result)
}

fn safe_path(text: &str) -> Result<PathBuf, String> {
    let path = Path::new(text);
    validate_relative_path(path)?;
    if text.len() > PATH_LIMIT
        || path.components().count() > 256
        || text.chars().any(char::is_control)
        || path
            .components()
            .map(|p| p.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
            != text
    {
        return Err("Archive contains an unsafe or excessive path".into());
    }
    Ok(path.to_owned())
}

fn validate_integrity(integrity: &Integrity, limits: Limits) -> Result<(), String> {
    if integrity.format != "caskrun-compressed-bottle"
        || integrity.version != 1
        || integrity.entries.len() > limits.entries
    {
        return Err("Unsupported or excessive compressed bottle archive".into());
    }
    let mut known = BTreeMap::new();
    let mut previous = "";
    let mut total = 0_u64;
    for item in &integrity.entries {
        cancellation::check()?;
        let path = safe_path(&item.path)?;
        if item.path.as_str() <= previous || item.mode & !0o777 != 0 {
            return Err("Archive has duplicate, unordered or unsafe metadata".into());
        }
        previous = &item.path;
        if item.path == "manifest.json" {
            if !matches!(item.kind, Kind::File { size, .. } if size <= 4 * 1024 * 1024) {
                return Err("Backup manifest must be a regular file".into());
            }
        } else if item.path == "payload" {
            if item.kind != Kind::Directory {
                return Err("Backup payload must be a directory".into());
            }
        } else if !item.path.starts_with("payload/") {
            return Err("Archive contains unexpected top-level data".into());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && known.get(parent) != Some(&true)
        {
            return Err("Archive entry has a missing or non-directory parent".into());
        }
        match &item.kind {
            Kind::File { size, sha256 } => {
                if *size > limits.file
                    || sha256.len() != 64
                    || !sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
                    || item.mode & 0o400 == 0
                {
                    return Err("Archive file has invalid size, permissions or checksum".into());
                }
                total = total.checked_add(*size).ok_or("Archive size overflow")?;
                if total > limits.total {
                    return Err("Archive exceeds the decompressed size limit".into());
                }
            }
            Kind::Directory if item.mode & 0o500 != 0o500 => {
                return Err("Archive directory is not readable and searchable".into());
            }
            Kind::Symlink { target } => {
                if target.len() > PATH_LIMIT || target.chars().any(char::is_control) {
                    return Err("Archive link target is excessive or invalid".into());
                }
                let relative = path
                    .strip_prefix("payload")
                    .map_err(|_| "Archive symlink is outside the payload")?;
                validate_backup_symlink(relative, Path::new(target))?;
            }
            _ => {}
        }
        known.insert(path, matches!(item.kind, Kind::Directory));
    }
    if known.get(Path::new("manifest.json")) != Some(&false)
        || known.get(Path::new("payload/prefix")) != Some(&true)
    {
        return Err("Archive is missing the backup manifest or Windows prefix".into());
    }
    Ok(())
}

fn scan_snapshot(root: &Path, limits: Limits) -> Result<Vec<Item>, String> {
    fn visit(
        root: &Path,
        relative: &Path,
        items: &mut Vec<Item>,
        limits: Limits,
    ) -> Result<(), String> {
        cancellation::check()?;
        let full = root.join(relative);
        let (_guard, before) = open_stable_directory(&full)?;
        for entry in fs::read_dir(&full).map_err(|e| e.to_string())? {
            cancellation::check()?;
            if items.len() >= limits.entries {
                return Err("Backup has too many archive entries".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let path = relative.join(entry.file_name());
            let text = path
                .to_str()
                .ok_or("Backup contains a non-UTF-8 filename")?
                .to_owned();
            safe_path(&text)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            let kind = if metadata.file_type().is_dir() {
                Kind::Directory
            } else if metadata.file_type().is_symlink() {
                Kind::Symlink {
                    target: fs::read_link(entry.path())
                        .map_err(|e| e.to_string())?
                        .to_str()
                        .ok_or("Backup contains a non-UTF-8 link")?
                        .to_owned(),
                }
            } else if metadata.file_type().is_file() {
                if metadata.len() > limits.file {
                    return Err("Backup file exceeds archive limit".into());
                }
                let mut file = open_file(&entry.path(), &metadata)?;
                Kind::File {
                    size: metadata.len(),
                    sha256: hash_copy(&mut file, &mut std::io::sink(), metadata.len())?,
                }
            } else {
                return Err("Backup contains an unsupported archive entry type".into());
            };
            items.push(Item {
                path: text,
                mode: if metadata.file_type().is_symlink() {
                    0o777
                } else {
                    metadata.mode() & 0o777
                },
                kind,
            });
            if metadata.file_type().is_dir() {
                visit(root, &path, items, limits)?;
            }
            assert_stable_path(&entry.path(), &metadata)?;
        }
        assert_stable_path(&full, &before)
    }
    let mut entries = Vec::new();
    visit(root, Path::new(""), &mut entries, limits)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    validate_integrity(
        &Integrity {
            format: "caskrun-compressed-bottle".into(),
            version: 1,
            entries: entries.clone(),
        },
        limits,
    )?;
    Ok(entries)
}

fn open_file(path: &Path, before: &fs::Metadata) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !before.file_type().is_file()
        || !same_snapshot(before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Backup file changed while opening it".into());
    }
    Ok(file)
}

fn hash_copy(input: &mut impl Read, output: &mut impl Write, size: u64) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut remaining = size;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        cancellation::check()?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let count = input
            .read(&mut buffer[..wanted])
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Archive file is truncated".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        digest.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn header(kind: tar::EntryType, size: u64, mode: u32) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

fn write_archive(
    root: &Path,
    integrity: &Integrity,
    output: &mut impl Write,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(integrity).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err("Backup integrity manifest exceeds the size limit".into());
    }
    let gzip = GzEncoder::new(output, Compression::default());
    let mut tar = tar::Builder::new(gzip);
    tar.append_data(
        &mut header(tar::EntryType::Regular, bytes.len() as u64, 0o600),
        INTEGRITY,
        bytes.as_slice(),
    )
    .map_err(|e| e.to_string())?;
    for item in &integrity.entries {
        cancellation::check()?;
        let path = root.join(&item.path);
        let before = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        match &item.kind {
            Kind::Directory => {
                if !before.file_type().is_dir() {
                    return Err("Snapshot directory changed".into());
                }
                tar.append_data(
                    &mut header(tar::EntryType::Directory, 0, item.mode),
                    &item.path,
                    std::io::empty(),
                )
                .map_err(|e| e.to_string())?;
            }
            Kind::Symlink { target } => {
                if !before.file_type().is_symlink()
                    || fs::read_link(&path).map_err(|e| e.to_string())? != Path::new(target)
                {
                    return Err("Snapshot link changed".into());
                }
                // Preserve the link's exact bytes: path-based tar setters
                // normalize targets (including Wine's special root link).
                let mut link = header(tar::EntryType::Symlink, 0, item.mode);
                if link.set_link_name_literal(target.as_bytes()).is_err() {
                    let mut extended = target.as_bytes().to_vec();
                    extended.push(0);
                    tar.append_data(
                        &mut header(tar::EntryType::GNULongLink, extended.len() as u64, 0o600),
                        "caskrun-long-link",
                        extended.as_slice(),
                    )
                    .map_err(|e| e.to_string())?;
                    link.set_link_name_literal(b"caskrun-long-link")
                        .map_err(|e| e.to_string())?;
                }
                tar.append_data(&mut link, &item.path, std::io::empty())
                    .map_err(|e| e.to_string())?;
            }
            Kind::File { size, sha256 } => {
                if before.len() != *size {
                    return Err("Snapshot file size changed".into());
                }
                let mut file = open_file(&path, &before)?;
                // The staging snapshot is private; verify before streaming and
                // again afterwards without retaining whole application files.
                if hash_copy(&mut file, &mut std::io::sink(), *size)? != *sha256 {
                    return Err("Snapshot checksum changed before compression".into());
                }
                file.rewind().map_err(|e| e.to_string())?;
                tar.append_data(
                    &mut header(tar::EntryType::Regular, *size, item.mode),
                    &item.path,
                    CancelRead(&mut file),
                )
                .map_err(|e| e.to_string())?;
                if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
                    return Err("Snapshot file changed during compression".into());
                }
            }
        }
        assert_stable_path(&path, &before)?;
    }
    let gzip = tar.into_inner().map_err(|e| e.to_string())?;
    gzip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

struct CancelRead<R>(R);
impl<R: Read> Read for CancelRead<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        cancellation::check().map_err(std::io::Error::other)?;
        self.0.read(bytes)
    }
}
struct BoundedRead<R> {
    inner: R,
    count: u64,
    maximum: u64,
}
impl<R: Read> Read for BoundedRead<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        cancellation::check().map_err(std::io::Error::other)?;
        if self.count >= self.maximum {
            return Err(std::io::Error::other("Archive exceeds decompression limit"));
        }
        let allowed = bytes.len().min((self.maximum - self.count) as usize);
        let count = self.inner.read(&mut bytes[..allowed])?;
        self.count += count as u64;
        Ok(count)
    }
}

fn long_text(entry: &mut impl Read, size: u64) -> Result<String, String> {
    if size == 0 || size > PATH_LIMIT as u64 + 1 {
        return Err("Archive extension path exceeds the limit".into());
    }
    let mut bytes = Vec::new();
    entry
        .take(size)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size || bytes.pop() != Some(0) || bytes.contains(&0) {
        return Err("Archive extension path is invalid".into());
    }
    String::from_utf8(bytes).map_err(|_| "Archive path is not UTF-8".into())
}

fn read_archive<R: BufRead>(input: R, root: &Path, limits: Limits) -> Result<(), String> {
    // Our encoder writes a fixed gzip header. Refuse optional filename,
    // comment and extra-field headers before the decoder can allocate their
    // untrusted metadata or scan an unbounded name looking for a terminator.
    let mut input = input;
    let mut gzip_header = [0_u8; 10];
    input
        .read_exact(&mut gzip_header)
        .map_err(|e| e.to_string())?;
    if gzip_header[..4] != [0x1f, 0x8b, 8, 0] {
        return Err("Unsupported gzip header in Caskrun backup".into());
    }
    let gzip = GzDecoder::new(std::io::Cursor::new(gzip_header).chain(input));
    let maximum = limits
        .total
        .checked_add(limits.metadata)
        .and_then(|n| n.checked_add(limits.entries as u64 * 10_240 + MAX_TRAILER))
        .ok_or("Archive limit overflow")?;
    let mut archive = tar::Archive::new(BoundedRead {
        inner: gzip,
        count: 0,
        maximum,
    });
    let mut integrity = None;
    let mut next = 0;
    let mut path_extension = None;
    let mut link_extension = None;
    let mut physical = 0_usize;
    {
        let entries = archive.entries().map_err(|e| e.to_string())?.raw(true);
        for entry in entries {
            cancellation::check()?;
            physical += 1;
            if physical > limits.entries.saturating_mul(3) + 1 {
                return Err("Archive contains too many headers".into());
            }
            let mut entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.header().entry_type();
            let size = entry.size();
            if kind.is_gnu_longname() || kind.is_gnu_longlink() {
                if integrity.is_none() {
                    return Err("Archive must start with its integrity manifest".into());
                }
                let value = long_text(&mut entry, size)?;
                let slot = if kind.is_gnu_longname() {
                    &mut path_extension
                } else {
                    &mut link_extension
                };
                if slot.replace(value).is_some() {
                    return Err("Archive has duplicate path extensions".into());
                }
                continue;
            }
            if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() {
                return Err(
                    "Archive contains an unsupported hardlink, sparse, device or metadata entry"
                        .into(),
                );
            }
            let path = match path_extension.take() {
                Some(path) => path,
                None => std::str::from_utf8(&entry.path_bytes())
                    .map_err(|_| "Archive path is not UTF-8")?
                    .to_owned(),
            };
            safe_path(&path)?;
            let mode = entry.header().mode().map_err(|e| e.to_string())?;
            if integrity.is_none() {
                if path != INTEGRITY
                    || !kind.is_file()
                    || size > limits.metadata
                    || mode & !0o777 != 0
                    || link_extension.is_some()
                {
                    return Err("Archive must start with a bounded integrity manifest".into());
                }
                let mut bytes = Vec::new();
                (&mut entry)
                    .take(limits.metadata + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                let parsed: Integrity = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Invalid archive integrity manifest: {e}"))?;
                validate_integrity(&parsed, limits)?;
                integrity = Some(parsed);
                continue;
            }
            let expected = integrity
                .as_ref()
                .unwrap()
                .entries
                .get(next)
                .ok_or("Archive contains entries absent from its integrity manifest")?;
            if path != expected.path || mode != expected.mode {
                return Err(
                    "Archive entry differs from its integrity manifest or is duplicated".into(),
                );
            }
            checked_parents(root, Path::new(&path))?;
            let output = root.join(&path);
            match &expected.kind {
                Kind::Directory if kind.is_dir() && size == 0 && link_extension.is_none() => {
                    fs::DirBuilder::new()
                        .mode(0o700)
                        .create(&output)
                        .map_err(|e| e.to_string())?;
                }
                Kind::File {
                    size: expected_size,
                    sha256,
                } if kind.is_file() && size == *expected_size && link_extension.is_none() => {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .custom_flags(O_NOFOLLOW_ANY)
                        .open(&output)
                        .map_err(|e| e.to_string())?;
                    if hash_copy(&mut entry, &mut file, size)? != *sha256 {
                        return Err(
                            "Archive file checksum does not match its integrity manifest".into(),
                        );
                    }
                    file.sync_all().map_err(|e| e.to_string())?;
                    file.set_permissions(fs::Permissions::from_mode(mode))
                        .map_err(|e| e.to_string())?;
                }
                Kind::Symlink { target } if kind.is_symlink() && size == 0 => {
                    let actual = match link_extension.take() {
                        Some(target) => target,
                        None => String::from_utf8(
                            entry
                                .link_name_bytes()
                                .ok_or("Archive symlink has no target")?
                                .as_ref()
                                .to_vec(),
                        )
                        .map_err(|_| "Archive symlink target is not UTF-8")?,
                    };
                    if actual != *target {
                        return Err(format!(
                            "Archive symlink at {path} differs from its integrity manifest"
                        ));
                    }
                }
                _ => {
                    return Err(
                        "Archive entry type or size differs from its integrity manifest".into(),
                    );
                }
            }
            next += 1;
        }
    }
    if path_extension.is_some() || link_extension.is_some() {
        return Err("Archive ends with an unused path extension".into());
    }
    let integrity = integrity.ok_or("Archive has no integrity manifest")?;
    if next != integrity.entries.len() {
        return Err("Archive is missing entries from its integrity manifest".into());
    }
    // tar stops at its first zero block. Drain only bounded zero padding to
    // validate the second end block, gzip CRC/trailer and compressed EOF.
    let mut reader = archive.into_inner();
    let mut padding = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        padding += count as u64;
        if padding > MAX_TRAILER || buffer[..count].iter().any(|b| *b != 0) {
            return Err("Archive has unexpected data after its tar end marker".into());
        }
    }
    if padding < 512 || !padding.is_multiple_of(512) {
        return Err("Archive is missing complete tar end markers".into());
    }
    let mut compressed = reader.inner.into_inner();
    if !compressed.fill_buf().map_err(|e| e.to_string())?.is_empty() {
        return Err("Archive contains appended gzip members or trailing bytes".into());
    }
    // All files are verified before any link exists. A declared symlink can
    // therefore never become an extraction parent or redirect a file write.
    for item in &integrity.entries {
        cancellation::check()?;
        if let Kind::Symlink { target } = &item.kind {
            checked_parents(root, Path::new(&item.path))?;
            symlink(target, root.join(&item.path)).map_err(|e| e.to_string())?;
        }
    }
    for item in integrity.entries.iter().rev() {
        if item.kind == Kind::Directory {
            let path = root.join(&item.path);
            let (guard, _) = open_stable_directory(&path)?;
            guard
                .set_permissions(fs::Permissions::from_mode(item.mode))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn checked_parents(root: &Path, relative: &Path) -> Result<(), String> {
    let (_root, _) = open_stable_directory(root)?;
    let mut path = root.to_owned();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            let Component::Normal(name) = part else {
                return Err("Unsafe archive parent".into());
            };
            path.push(name);
            let (_guard, _) = open_stable_directory(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "bottle_archive_tests.rs"]
mod tests;
