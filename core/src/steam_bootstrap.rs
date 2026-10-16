//! Fetch the current, unmodified Windows x64 Steam bootstrap from Valve.
//!
//! This only prepares a verified temporary file. The caller owns bottle idle
//! checks, replacement and rollback. Steam still performs its normal updates,
//! sign-in and ownership checks; no client version or update setting is pinned.

use super::*;
use flate2::{Decompress, FlushDecompress, Status};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::fd::FromRawFd;
use std::os::unix::fs::DirBuilderExt;

const HOST: &str = "https://client-update.steamstatic.com/";
const MANIFEST_URL: &str = "https://client-update.steamstatic.com/steam_client_win64";
const MAX_MANIFEST: usize = 128 * 1024;
const MAX_PACKAGE: usize = 16 * 1024 * 1024;
const MAX_EXECUTABLE: usize = 32 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(130);
const TEMPORARY_FILES: &[&str] = &[
    "manifest.vdf",
    "manifest.status",
    "package.zip",
    "package.status",
    "steam.exe",
];

pub(super) struct DownloadedBootstrap {
    temporary: Temporary,
    sha256: String,
}

impl DownloadedBootstrap {
    pub(super) fn path(&self) -> PathBuf {
        self.temporary.path.join("steam.exe")
    }

    pub(super) fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Return the checked open handle for the caller's copy. A changed cache
    /// entry is never reopened and trusted merely because it has the same name.
    pub(super) fn open_verified(&self) -> Result<File, String> {
        self.temporary.recheck()?;
        let path = self.path();
        let before = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !before.file_type().is_file() || before.len() > MAX_EXECUTABLE as u64 {
            return Err("Prepared Steam bootstrap is not a bounded regular file".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| e.to_string())?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("Prepared Steam bootstrap changed while opening it".into());
        }
        let bytes = read_bounded(&mut file, MAX_EXECUTABLE)?;
        inspect_x64_pe(&bytes)?;
        if digest(&bytes) != self.sha256
            || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
            || !same_snapshot(
                &before,
                &fs::symlink_metadata(&path).map_err(|e| e.to_string())?,
            )
        {
            return Err("Prepared Steam bootstrap changed after verification".into());
        }
        self.temporary.recheck()?;
        file.rewind().map_err(|e| e.to_string())?;
        Ok(file)
    }
}

struct Temporary {
    path: PathBuf,
    guard: File,
    snapshot: fs::Metadata,
}

impl Temporary {
    fn create(home: &Path) -> Result<Self, String> {
        storage_root(home, true)?;
        let downloads = real_subdirectory(home, "downloads")?;
        let path = downloads.join(format!("steam-bootstrap-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|e| format!("Could not prepare Steam bootstrap download: {e}"))?;
        let (guard, snapshot) = open_stable_directory(&path)?;
        Ok(Self {
            path,
            guard,
            snapshot,
        })
    }

    fn recheck(&self) -> Result<(), String> {
        // Reopen with the shared all-components no-follow checks. An ancestor
        // replaced by a symlink must not redirect even an apparently same-inode
        // lookup at the final component.
        let (_current_guard, current) = open_stable_directory(&self.path)?;
        let held = self.guard.metadata().map_err(|e| e.to_string())?;
        if !current.file_type().is_dir()
            || !same_identity(&self.snapshot, &current)
            || !same_identity(&self.snapshot, &held)
        {
            return Err("Steam bootstrap download folder changed".into());
        }
        Ok(())
    }

    fn create_file(&self, name: &str) -> Result<File, String> {
        self.recheck()?;
        if !TEMPORARY_FILES.contains(&name) {
            return Err("Invalid Steam bootstrap temporary filename".into());
        }
        let name = CString::new(name).map_err(|_| "Invalid Steam bootstrap temporary filename")?;
        // The held private directory, rather than a re-resolved path, owns the
        // new file even if its name or an ancestor changes after recheck.
        let fd = unsafe {
            libc::openat(
                self.guard.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(format!(
                "Could not prepare Steam bootstrap file: {}",
                std::io::Error::last_os_error()
            ));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Steam bootstrap temporary must be a regular file".into());
        }
        self.recheck()?;
        Ok(file)
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // Unlink only our known entries through the held directory descriptor;
        // a swapped path cannot redirect cleanup to unrelated files. Never
        // recursively remove anything, or remove a replaced directory name.
        for name in TEMPORARY_FILES {
            if let Ok(name) = CString::new(*name) {
                let _ = unsafe { libc::unlinkat(self.guard.as_raw_fd(), name.as_ptr(), 0) };
            }
        }
        if self.recheck().is_ok() {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

pub(super) fn download_verified_bootstrap(home: &Path) -> Result<DownloadedBootstrap, String> {
    download_with_curl(home, Path::new("/usr/bin/curl"))
}

#[cfg(test)]
pub(super) fn from_test_payload(home: &Path, bytes: &[u8]) -> Result<DownloadedBootstrap, String> {
    inspect_x64_pe(bytes)?;
    let temporary = Temporary::create(home)?;
    let mut file = temporary.create_file("steam.exe")?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(DownloadedBootstrap {
        temporary,
        sha256: digest(bytes),
    })
}

fn download_with_curl(home: &Path, curl: &Path) -> Result<DownloadedBootstrap, String> {
    cancellation::check()?;
    let temporary = Temporary::create(home)?;
    let manifest = download(
        &temporary,
        curl,
        MANIFEST_URL,
        "manifest.vdf",
        "manifest.status",
        MAX_MANIFEST,
    )?;
    let package = manifest_package(&manifest)?;
    cancellation::check()?;
    let archive = download(
        &temporary,
        curl,
        &format!("{HOST}{}", package.file),
        "package.zip",
        "package.status",
        MAX_PACKAGE,
    )?;
    let executable = verified_payload(&archive, &package)?;
    cancellation::check()?;
    let mut output = temporary.create_file("steam.exe")?;
    output.write_all(&executable).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())?;
    let result = DownloadedBootstrap {
        temporary,
        sha256: digest(&executable),
    };
    let _ = result.open_verified()?;
    Ok(result)
}

fn download(
    temporary: &Temporary,
    curl: &Path,
    url: &str,
    name: &str,
    status_name: &str,
    maximum: usize,
) -> Result<Vec<u8>, String> {
    cancellation::check()?;
    let mut body = temporary.create_file(name)?;
    let mut http_status = temporary.create_file(status_name)?;
    let body_identity = body.metadata().map_err(|e| e.to_string())?;
    // -q is first to disable curlrc. Hosts are fixed; redirects, cookies and
    // credentials are not accepted. Silent mode leaves only our exact
    // three-digit HTTP status on stderr, not an unbounded diagnostic log.
    let mut child = Command::new(curl)
        .args([
            "-q",
            "--fail",
            "--silent",
            "--proto",
            "=https",
            "--max-redirs",
            "0",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--max-filesize",
            &maximum.to_string(),
            "--write-out",
            "%{stderr}%{http_code}",
            url,
        ])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::from(body.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(
            http_status.try_clone().map_err(|e| e.to_string())?,
        ))
        .spawn()
        .map_err(|e| format!("Could not start Valve bootstrap download: {e}"))?;
    let status = wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT)?;
    cancellation::check()?;
    let code = read_bounded(&mut http_status, 3)?;
    if !status.is_some_and(|s| s.success()) || code != b"200" {
        return Err(
            "Could not download Steam's current bootstrap directly from Valve over HTTPS".into(),
        );
    }
    temporary.recheck()?;
    let transferred = body.metadata().map_err(|e| e.to_string())?;
    let named = fs::symlink_metadata(temporary.path.join(name)).map_err(|e| e.to_string())?;
    // The shared same_identity helper deliberately accepts directories only.
    // File size/timestamps change while curl writes, so compare device/inode
    // to the initially opened file and a full snapshot only after it exits.
    if !transferred.file_type().is_file()
        || !named.file_type().is_file()
        || body_identity.dev() != transferred.dev()
        || body_identity.ino() != transferred.ino()
        || !same_snapshot(&transferred, &named)
    {
        return Err("Steam bootstrap download changed during transfer".into());
    }
    let bytes = read_bounded(&mut body, maximum)?;
    if !same_snapshot(&transferred, &body.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &transferred,
            &fs::symlink_metadata(temporary.path.join(name)).map_err(|e| e.to_string())?,
        )
    {
        return Err("Steam bootstrap download changed while reading it".into());
    }
    temporary.recheck()?;
    Ok(bytes)
}

fn read_bounded(file: &mut File, maximum: usize) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(|e| e.to_string())?.len() > maximum as u64 {
        return Err("Steam bootstrap input exceeds its size limit".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > maximum {
        return Err("Steam bootstrap input exceeds its size limit".into());
    }
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct Package {
    file: String,
    size: usize,
    sha256: String,
}

#[derive(Debug)]
enum Value {
    Text(String),
    Object(BTreeMap<String, Value>),
}

struct Vdf<'a> {
    bytes: &'a [u8],
    cursor: usize,
    entries: usize,
}

impl Vdf<'_> {
    fn whitespace(&mut self) {
        loop {
            while self
                .bytes
                .get(self.cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.cursor += 1;
            }
            if self.bytes.get(self.cursor..self.cursor + 2) == Some(b"//") {
                while self.bytes.get(self.cursor).is_some_and(|b| *b != b'\n') {
                    self.cursor += 1;
                }
            } else {
                break;
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.whitespace();
        if self.bytes.get(self.cursor) != Some(&b'"') {
            return Err("Valve bootstrap manifest requires quoted keys and values".into());
        }
        self.cursor += 1;
        let mut result = Vec::new();
        loop {
            let byte = *self
                .bytes
                .get(self.cursor)
                .ok_or("Valve bootstrap manifest has an unfinished string")?;
            self.cursor += 1;
            match byte {
                b'"' => {
                    return String::from_utf8(result)
                        .map_err(|_| "Valve bootstrap manifest is not UTF-8".into());
                }
                b'\\' => {
                    let escaped = *self
                        .bytes
                        .get(self.cursor)
                        .ok_or("Valve bootstrap manifest has an unfinished escape")?;
                    self.cursor += 1;
                    if escaped != b'"' && escaped != b'\\' {
                        return Err("Valve bootstrap manifest has an unsupported escape".into());
                    }
                    result.push(escaped);
                }
                0..=31 | 127 => {
                    return Err("Valve bootstrap manifest contains a control character".into());
                }
                _ => result.push(byte),
            }
            if result.len() > 8192 {
                return Err("Valve bootstrap manifest token is too large".into());
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<BTreeMap<String, Value>, String> {
        if depth > 8 {
            return Err("Valve bootstrap manifest is nested too deeply".into());
        }
        let mut values = BTreeMap::new();
        loop {
            self.whitespace();
            match self.bytes.get(self.cursor) {
                None if depth == 0 => return Ok(values),
                Some(b'}') if depth > 0 => {
                    self.cursor += 1;
                    return Ok(values);
                }
                None | Some(b'}') => {
                    return Err("Valve bootstrap manifest has mismatched braces".into());
                }
                _ => {}
            }
            let key = self.string()?.to_ascii_lowercase();
            self.entries += 1;
            if self.entries > 4096 || values.contains_key(&key) {
                return Err("Valve bootstrap manifest has duplicate or excessive entries".into());
            }
            self.whitespace();
            let value = if self.bytes.get(self.cursor) == Some(&b'{') {
                self.cursor += 1;
                Value::Object(self.object(depth + 1)?)
            } else {
                Value::Text(self.string()?)
            };
            values.insert(key, value);
        }
    }
}

fn manifest_package(bytes: &[u8]) -> Result<Package, String> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST {
        return Err("Valve bootstrap manifest is empty or too large".into());
    }
    let root = Vdf {
        bytes,
        cursor: 0,
        entries: 0,
    }
    .object(0)?;
    let selected = child_object(
        child_object(child_object(&root, "win64")?, "steam_win64")?,
        "steamrow",
    )?;
    let text = |key: &str| match selected.get(key) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err("Valve bootstrap package metadata is incomplete".to_owned()),
    };
    let file = text("file")?;
    let suffix = file
        .strip_prefix("steam_win64_steamrow.zip.")
        .ok_or("Valve bootstrap package filename is not the reviewed Windows x64 layout")?;
    if suffix.len() != 40
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("Valve bootstrap package filename has an invalid digest suffix".into());
    }
    let size_text = text("size")?;
    if size_text.is_empty() || !size_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Valve bootstrap package size is invalid".into());
    }
    let size: usize = size_text
        .parse()
        .map_err(|_| "Valve bootstrap package size is invalid")?;
    if size == 0 || size > MAX_PACKAGE {
        return Err("Valve bootstrap package exceeds its size limit".into());
    }
    let sha256 = text("sha2")?.to_ascii_lowercase();
    if sha256.len() != 64
        || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || text("isbootstrapperpackage")? != "1"
    {
        return Err(
            "Valve bootstrap package is missing its SHA-256 or bootstrap designation".into(),
        );
    }
    Ok(Package {
        file: file.into(),
        size,
        sha256,
    })
}

fn child_object<'a>(
    values: &'a BTreeMap<String, Value>,
    key: &str,
) -> Result<&'a BTreeMap<String, Value>, String> {
    match values.get(key) {
        Some(Value::Object(value)) => Ok(value),
        _ => Err("Valve manifest is missing its Windows x64 bootstrap package".into()),
    }
}

fn verified_payload(archive: &[u8], package: &Package) -> Result<Vec<u8>, String> {
    if archive.len() != package.size
        || archive.len() > MAX_PACKAGE
        || digest(archive) != package.sha256
    {
        return Err("Valve bootstrap ZIP does not match its manifest size and SHA-256".into());
    }
    let payload = unzip_single_executable(archive)?;
    inspect_x64_pe(&payload)?;
    Ok(payload)
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("Steam bootstrap ZIP/header is truncated")?
            .try_into()
            .unwrap(),
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("Steam bootstrap ZIP/header is truncated")?
            .try_into()
            .unwrap(),
    ))
}

fn extra_fields(bytes: &[u8]) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        let kind = u16_at(bytes, offset)?;
        let size = u16_at(bytes, offset + 2)? as usize;
        if kind == 1 || offset + 4 + size > bytes.len() {
            return Err("Steam bootstrap ZIP has ZIP64 or malformed extra fields".into());
        }
        offset += 4 + size;
    }
    Ok(())
}

/// Decode exactly one ordinary steam.exe; never extract archive paths to disk.
fn unzip_single_executable(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 22 || bytes.len() > MAX_PACKAGE {
        return Err("Steam bootstrap ZIP is empty or too large".into());
    }
    let end = (bytes.len().saturating_sub(22 + 65535)..=bytes.len() - 22)
        .rev()
        .find(|&p| {
            bytes.get(p..p + 4) == Some(b"PK\x05\x06")
                && u16_at(bytes, p + 20).is_ok_and(|n| p + 22 + n as usize == bytes.len())
        })
        .ok_or("Steam bootstrap ZIP has no complete end record")?;
    if u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || u16_at(bytes, end + 8)? != 1
        || u16_at(bytes, end + 10)? != 1
    {
        return Err("Steam bootstrap ZIP must contain exactly one entry on one disk".into());
    }
    let central_size = u32_at(bytes, end + 12)? as usize;
    let central = u32_at(bytes, end + 16)? as usize;
    if central.checked_add(central_size) != Some(end)
        || bytes.get(central..central.saturating_add(4)) != Some(b"PK\x01\x02")
    {
        return Err("Steam bootstrap ZIP central directory is invalid".into());
    }
    let flags = u16_at(bytes, central + 8)?;
    let method = u16_at(bytes, central + 10)?;
    let crc = u32_at(bytes, central + 16)?;
    let compressed = u32_at(bytes, central + 20)? as usize;
    let size = u32_at(bytes, central + 24)? as usize;
    let name_len = u16_at(bytes, central + 28)? as usize;
    let extra_len = u16_at(bytes, central + 30)? as usize;
    let comment_len = u16_at(bytes, central + 32)? as usize;
    let attrs = u32_at(bytes, central + 38)?;
    let unix_kind = (attrs >> 16) & 0o170000;
    let local = u32_at(bytes, central + 42)? as usize;
    // Valve's current authenticated package has a 20-byte opaque prefix before
    // the ordinary ZIP local header. Accept that exact layout as well as a
    // plain ZIP; do not search arbitrary self-extracting prefixes for a header.
    // verified_payload checks SHA-256 over the entire package, prefix included.
    if central_size != 46 + name_len + extra_len + comment_len
        || bytes.get(central + 46..central + 46 + name_len) != Some(b"steam.exe")
        || u16_at(bytes, central + 34)? != 0
        || !matches!(local, 0 | 20)
        || u16_at(bytes, central + 6)? > 20
        || flags & !0x0806 != 0
        || !matches!(method, 0 | 8)
        || attrs & 0x10 != 0
        || !matches!(unix_kind, 0 | 0o100000)
        || compressed == 0
        || compressed > MAX_PACKAGE
        || !(1024..=MAX_EXECUTABLE).contains(&size)
    {
        return Err("Steam bootstrap ZIP has an unsafe entry, flags, method or size".into());
    }
    extra_fields(
        bytes
            .get(central + 46 + name_len..central + 46 + name_len + extra_len)
            .ok_or("Steam ZIP extra fields are truncated")?,
    )?;
    if bytes.get(local..local + 4) != Some(b"PK\x03\x04")
        || u16_at(bytes, local + 4)? != u16_at(bytes, central + 6)?
        || u16_at(bytes, local + 6)? != flags
        || u16_at(bytes, local + 8)? != method
        || u32_at(bytes, local + 14)? != crc
        || u32_at(bytes, local + 18)? as usize != compressed
        || u32_at(bytes, local + 22)? as usize != size
    {
        return Err("Steam bootstrap ZIP local and central headers disagree".into());
    }
    let local_name = u16_at(bytes, local + 26)? as usize;
    let local_extra = u16_at(bytes, local + 28)? as usize;
    let start = local + 30 + local_name + local_extra;
    if bytes.get(local + 30..local + 30 + local_name) != Some(b"steam.exe")
        || start.checked_add(compressed) != Some(central)
    {
        return Err("Steam bootstrap ZIP local path or data bounds are invalid".into());
    }
    extra_fields(
        bytes
            .get(local + 30 + local_name..start)
            .ok_or("Steam ZIP local extra fields are truncated")?,
    )?;
    let compressed_bytes = bytes
        .get(start..central)
        .ok_or("Steam ZIP payload is truncated")?;
    let output = if method == 0 {
        if compressed != size {
            return Err("Stored Steam ZIP size is inconsistent".into());
        }
        compressed_bytes.to_vec()
    } else {
        let mut output = Vec::with_capacity(size + 1);
        let mut decoder = Decompress::new(false);
        let status = decoder
            .decompress_vec(compressed_bytes, &mut output, FlushDecompress::Finish)
            .map_err(|_| "Steam bootstrap ZIP deflate stream is invalid")?;
        if status != Status::StreamEnd
            || decoder.total_in() != compressed as u64
            || decoder.total_out() != size as u64
        {
            return Err(
                "Steam bootstrap ZIP payload has truncated, trailing or excessive data".into(),
            );
        }
        output
    };
    if output.len() != size || crc32(&output) != crc {
        return Err("Steam bootstrap ZIP payload size or CRC does not match".into());
    }
    Ok(output)
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xedb88320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    !bytes.iter().fold(!0u32, |crc, byte| {
        TABLE[((crc ^ *byte as u32) & 255) as usize] ^ (crc >> 8)
    })
}

pub(super) fn inspect_x64_pe(bytes: &[u8]) -> Result<(), String> {
    if !(1024..=MAX_EXECUTABLE).contains(&bytes.len()) || bytes.get(..2) != Some(b"MZ") {
        return Err("Valve bootstrap is not a bounded Windows executable".into());
    }
    let pe = u32_at(bytes, 0x3c)? as usize;
    if pe < 64
        || pe.checked_add(264).is_none_or(|end| end > bytes.len())
        || bytes.get(pe..pe + 4) != Some(b"PE\0\0")
    {
        return Err("Valve bootstrap has an invalid PE header".into());
    }
    let sections = u16_at(bytes, pe + 6)? as usize;
    let optional = u16_at(bytes, pe + 20)? as usize;
    let characteristics = u16_at(bytes, pe + 22)?;
    if u16_at(bytes, pe + 4)? != 0x8664
        || u16_at(bytes, pe + 24)? != 0x20b
        || sections == 0
        || sections > 96
        || optional < 240
        || pe + 24 + optional + sections * 40 > bytes.len()
        || characteristics & 0x2002 != 0x0002
    {
        return Err("Valve bootstrap is not a Windows x64 PE executable".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::DeflateEncoder};

    fn executable() -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[134..136].copy_from_slice(&1u16.to_le_bytes());
        bytes[148..150].copy_from_slice(&240u16.to_le_bytes());
        bytes[150..152].copy_from_slice(&2u16.to_le_bytes());
        bytes[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes
    }

    fn archive(payload: &[u8], deflated: bool) -> Vec<u8> {
        let data = if deflated {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(payload).unwrap();
            encoder.finish().unwrap()
        } else {
            payload.to_vec()
        };
        let name = b"steam.exe";
        let method: u16 = if deflated { 8 } else { 0 };
        let crc = crc32(payload);
        let mut local = vec![0; 30];
        local[..4].copy_from_slice(b"PK\x03\x04");
        local[4..6].copy_from_slice(&20u16.to_le_bytes());
        local[8..10].copy_from_slice(&method.to_le_bytes());
        local[14..18].copy_from_slice(&crc.to_le_bytes());
        local[18..22].copy_from_slice(&(data.len() as u32).to_le_bytes());
        local[22..26].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(name);
        local.extend_from_slice(&data);
        let offset = local.len();
        let mut central = vec![0; 46];
        central[..4].copy_from_slice(b"PK\x01\x02");
        central[6..8].copy_from_slice(&20u16.to_le_bytes());
        central[10..12].copy_from_slice(&method.to_le_bytes());
        central[16..20].copy_from_slice(&crc.to_le_bytes());
        central[20..24].copy_from_slice(&(data.len() as u32).to_le_bytes());
        central[24..28].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        central[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(name);
        let mut end = vec![0; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[8..10].copy_from_slice(&1u16.to_le_bytes());
        end[10..12].copy_from_slice(&1u16.to_le_bytes());
        end[12..16].copy_from_slice(&(central.len() as u32).to_le_bytes());
        end[16..20].copy_from_slice(&(offset as u32).to_le_bytes());
        local.extend_from_slice(&central);
        local.extend_from_slice(&end);
        local
    }

    fn manifest(package: &[u8]) -> String {
        format!(
            r#""win64" {{ "steam_win64" {{ "steamrow" {{ "file" "steam_win64_steamrow.zip.{}" "size" "{}" "sha2" "{}" "IsBootstrapperPackage" "1" }} }} }} "kvsign2" {{ "win64" "fixture" }}"#,
            "a".repeat(40),
            package.len(),
            digest(package)
        )
    }

    #[test]
    fn current_manifest_shape_selects_nested_global_bootstrap_only() {
        let zip = archive(&executable(), true);
        let text = manifest(&zip).replace("\"steamrow\"", "\"file\" \"unselected.zip\" \"steamchina\" { \"file\" \"unselected.zip\" } \"steamrow\"");
        let package = manifest_package(text.as_bytes()).unwrap();
        assert_eq!(package.size, zip.len());
        assert_eq!(verified_payload(&zip, &package).unwrap(), executable());
    }

    #[test]
    fn manifest_refuses_ambiguity_redirect_paths_sizes_and_missing_integrity() {
        let zip = archive(&executable(), true);
        let valid = manifest(&zip);
        for invalid in [
            valid.replace("\"size\"", "\"SIZE\" \"1\" \"size\""),
            valid.replace("steam_win64_steamrow.zip.", "../steam_win64_steamrow.zip."),
            valid.replace(
                "steam_win64_steamrow.zip.",
                "https://other.invalid/steam_win64_steamrow.zip.",
            ),
            valid.replace(&format!("\"{}\"", zip.len()), "\"16777217\""),
            valid.replace(&format!("\"{}\"", digest(&zip)), "\"wrong\""),
            valid.replace(
                "\"IsBootstrapperPackage\" \"1\"",
                "\"IsBootstrapperPackage\" \"0\"",
            ),
            valid.replace("\"steamrow\"", "\"steamchina\""),
            format!("{valid} garbage"),
            valid[..valid.len() - 1].to_owned(),
        ] {
            assert!(manifest_package(invalid.as_bytes()).is_err(), "{invalid}");
        }
        assert!(manifest_package(&vec![b' '; MAX_MANIFEST + 1]).is_err());
        assert!(
            manifest_package(b"\"a\"{\"b\"{\"c\"{\"d\"{\"e\"{\"f\"{\"g\"{\"h\"{\"i\"{}}}}}}}}}")
                .is_err()
        );
    }

    #[test]
    fn zip_accepts_stored_and_deflated_only_after_manifest_verification() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        for deflated in [false, true] {
            let bytes = archive(&executable(), deflated);
            let package = manifest_package(manifest(&bytes).as_bytes()).unwrap();
            assert_eq!(verified_payload(&bytes, &package).unwrap(), executable());
            let mut altered = bytes.clone();
            altered[40] ^= 1;
            assert!(verified_payload(&altered, &package).is_err());
            assert!(verified_payload(&bytes[..bytes.len() - 1], &package).is_err());
        }
    }

    #[test]
    fn valve_twenty_byte_prefix_keeps_header_bounds_and_manifest_integrity() {
        let original = archive(&executable(), true);
        for prefix_size in [0, 1, 19, 20, 21] {
            let mut bytes = vec![0xa5; prefix_size];
            bytes.extend_from_slice(&original);
            let end = bytes.len() - 22;
            let central = u32_at(&original, original.len() - 6).unwrap() as usize + prefix_size;
            bytes[end + 16..end + 20].copy_from_slice(&(central as u32).to_le_bytes());
            bytes[central + 42..central + 46].copy_from_slice(&(prefix_size as u32).to_le_bytes());
            // Match the regular-file mode and DOS creator used by Valve.
            bytes[central + 38..central + 42].copy_from_slice(&0x81ff0000u32.to_le_bytes());
            let package = manifest_package(manifest(&bytes).as_bytes()).unwrap();
            if matches!(prefix_size, 0 | 20) {
                assert_eq!(verified_payload(&bytes, &package).unwrap(), executable());
                if prefix_size == 20 {
                    bytes[0] ^= 1;
                    assert!(verified_payload(&bytes, &package).is_err());
                }
            } else {
                assert!(verified_payload(&bytes, &package).is_err());
            }
        }
    }

    #[test]
    fn zip_refuses_extra_entries_traversal_symlinks_descriptors_and_bad_crc() {
        let valid = archive(&executable(), true);
        let central = u32_at(&valid, valid.len() - 6).unwrap() as usize;
        let mut cases = Vec::new();
        for flag in [1u16, 8, 0x40, 0x1000] {
            let mut b = valid.clone();
            b[6..8].copy_from_slice(&flag.to_le_bytes());
            b[central + 8..central + 10].copy_from_slice(&flag.to_le_bytes());
            cases.push(b);
        }
        let mut b = valid.clone();
        let end = b.len() - 22;
        b[end + 10] = 2;
        cases.push(b);
        let mut b = valid.clone();
        b[30..39].copy_from_slice(b"../a.exeX");
        b[central + 46..central + 55].copy_from_slice(b"../a.exeX");
        cases.push(b);
        let mut b = valid.clone();
        b[central + 38..central + 42].copy_from_slice(&(0o120777u32 << 16).to_le_bytes());
        cases.push(b);
        let mut b = valid.clone();
        b[central + 16] ^= 1;
        b[14] ^= 1;
        cases.push(b);
        let mut b = valid.clone();
        b[central + 24..central + 28].copy_from_slice(&((MAX_EXECUTABLE + 1) as u32).to_le_bytes());
        cases.push(b);
        let mut b = valid.clone();
        b.push(0);
        cases.push(b);
        for bytes in cases {
            assert!(unzip_single_executable(&bytes).is_err());
        }
        for length in 0..valid.len() {
            assert!(unzip_single_executable(&valid[..length]).is_err());
        }
    }

    #[test]
    fn payload_must_be_x64_executable_not_a_dll_or_32_bit_bootstrap() {
        for change in [0, 1, 2, 3] {
            let mut pe = executable();
            match change {
                0 => pe[132..134].copy_from_slice(&0x14cu16.to_le_bytes()),
                1 => pe[152..154].copy_from_slice(&0x10bu16.to_le_bytes()),
                2 => pe[150..152].copy_from_slice(&0x2002u16.to_le_bytes()),
                _ => pe[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes()),
            }
            let zip = archive(&pe, true);
            assert!(
                verified_payload(&zip, &manifest_package(manifest(&zip).as_bytes()).unwrap())
                    .is_err()
            );
        }
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("bootstrap-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn curl(&self, status: &str) -> PathBuf {
            let zip = archive(&executable(), true);
            fs::write(self.0.join("fixture.zip"), &zip).unwrap();
            fs::write(self.0.join("fixture.vdf"), manifest(&zip)).unwrap();
            let quote =
                |path: PathBuf| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
            let script = format!(
                "#!/bin/sh\ntest \"$1\" = -q || exit 91\nfor arg in \"$@\"; do url=$arg; done\ncase \"$url\" in\n  {MANIFEST_URL}) /bin/cat {};;\n  {HOST}steam_win64_steamrow.zip.*) /bin/cat {};;\n  *) exit 92;;\nesac\nprintf '%s' '{status}' >&2\n",
                quote(self.0.join("fixture.vdf")),
                quote(self.0.join("fixture.zip"))
            );
            let path = self.0.join("curl-fixture");
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn downloader_cleans_owned_temporaries_and_detects_replaced_payload() {
        let fixture = Fixture::new();
        let curl = fixture.curl("200");
        let home = fixture.0.join("data");
        let prepared = download_with_curl(&home, &curl).unwrap();
        assert_eq!(prepared.sha256(), digest(&executable()));
        let mut handle = prepared.open_verified().unwrap();
        let mut bytes = Vec::new();
        handle.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, executable());
        let parent = prepared.path().parent().unwrap().to_path_buf();
        fs::write(prepared.path(), b"replaced").unwrap();
        assert!(prepared.open_verified().is_err());
        drop(prepared);
        assert!(!parent.exists());
        assert_eq!(fs::read_dir(home.join("downloads")).unwrap().count(), 0);
        let curl = fixture.curl("302");
        assert!(download_with_curl(&home, &curl).is_err());
        assert_eq!(fs::read_dir(home.join("downloads")).unwrap().count(), 0);
    }

    #[test]
    fn canceled_download_creates_no_staging_files() {
        use std::sync::atomic::Ordering;
        let fixture = Fixture::new();
        let curl = fixture.curl("200");
        let home = fixture.0.join("data");
        let (request, _guard) = cancellation::test_request();
        request.store(true, Ordering::Release);
        assert!(cancellation::is_error(
            &download_with_curl(&home, &curl).err().unwrap()
        ));
        assert!(!home.exists());
    }

    #[test]
    fn cancellation_reaps_its_download_child_and_removes_partial_files() {
        use std::sync::atomic::Ordering;
        let fixture = Fixture::new();
        let home = fixture.0.join("data");
        let marker = fixture.0.join("download.pid");
        let marker_quote = marker.display().to_string().replace('\'', "'\\''");
        let curl = fixture.0.join("curl-blocking-fixture");
        fs::write(
            &curl,
            format!("#!/bin/sh\nprintf '%s' $$ > '{marker_quote}'\nexec /bin/sleep 30\n"),
        )
        .unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
        let (request, _guard) = cancellation::test_request();
        let observer_marker = marker.clone();
        let observer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while fs::read_to_string(&observer_marker)
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .is_none()
            {
                assert!(Instant::now() < deadline, "download fixture did not start");
                std::thread::sleep(Duration::from_millis(10));
            }
            request.store(true, Ordering::Release);
        });
        let error = download_with_curl(&home, &curl).err().unwrap();
        observer.join().unwrap();
        assert!(cancellation::is_error(&error), "{error}");
        let pid = fs::read_to_string(marker).unwrap().parse().unwrap();
        assert!(!process_alive(pid).unwrap());
        assert_eq!(fs::read_dir(home.join("downloads")).unwrap().count(), 0);
    }

    #[test]
    fn integration_fixture_constructor_checks_architecture_and_cleans_up() {
        let fixture = Fixture::new();
        let home = fixture.0.join("data");
        let payload = executable();
        let prepared = from_test_payload(&home, &payload).unwrap();
        let path = prepared.path();
        assert!(prepared.open_verified().is_ok());
        drop(prepared);
        assert!(!path.exists());
        assert!(from_test_payload(&home, b"invalid").is_err());
        assert_eq!(fs::read_dir(home.join("downloads")).unwrap().count(), 0);
    }

    #[test]
    fn temporary_replacement_and_symlink_cannot_redirect_creation_or_cleanup() {
        for linked in [false, true] {
            let fixture = Fixture::new();
            let temporary = Temporary::create(&fixture.0.join("data")).unwrap();
            temporary
                .create_file("steam.exe")
                .unwrap()
                .write_all(b"owned")
                .unwrap();
            let path = temporary.path.clone();
            let moved = fixture.0.join("moved-private-directory");
            fs::rename(&path, &moved).unwrap();
            let unrelated = if linked {
                let target = fixture.0.join("unrelated-directory");
                fs::create_dir(&target).unwrap();
                symlink(&target, &path).unwrap();
                target
            } else {
                fs::create_dir(&path).unwrap();
                path.clone()
            };
            fs::write(unrelated.join("steam.exe"), b"preserve unrelated data").unwrap();
            assert!(temporary.recheck().is_err());
            assert!(temporary.create_file("package.zip").is_err());
            assert!(!unrelated.join("package.zip").exists());
            drop(temporary);
            assert_eq!(
                fs::read(unrelated.join("steam.exe")).unwrap(),
                b"preserve unrelated data"
            );
            assert!(!moved.join("steam.exe").exists());
        }
    }

    #[test]
    fn temporary_ancestor_symlink_is_refused_and_external_files_are_preserved() {
        let fixture = Fixture::new();
        let home = fixture.0.join("data");
        let temporary = Temporary::create(&home).unwrap();
        temporary
            .create_file("manifest.vdf")
            .unwrap()
            .write_all(b"owned")
            .unwrap();
        let basename = temporary.path.file_name().unwrap().to_owned();
        let moved = home.join("moved-downloads");
        fs::rename(home.join("downloads"), &moved).unwrap();
        let unrelated = fixture.0.join("unrelated-downloads");
        fs::create_dir(&unrelated).unwrap();
        let outside = unrelated.join(&basename);
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("manifest.vdf"), b"preserve unrelated data").unwrap();
        symlink(&unrelated, home.join("downloads")).unwrap();
        assert!(temporary.recheck().is_err());
        assert!(temporary.create_file("package.zip").is_err());
        drop(temporary);
        assert_eq!(
            fs::read(outside.join("manifest.vdf")).unwrap(),
            b"preserve unrelated data"
        );
        assert!(!outside.join("package.zip").exists());
        assert!(!moved.join(basename).join("manifest.vdf").exists());
    }

    #[test]
    #[ignore = "downloads the current manifest and bootstrap from Valve; never runs Steam or Wine"]
    fn current_valve_bootstrap_download_verifies_and_cleans_up() {
        let fixture = Fixture::new();
        let prepared = download_verified_bootstrap(&fixture.0.join("data")).unwrap();
        assert!(prepared.open_verified().is_ok());
        let path = prepared.path();
        drop(prepared);
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(fixture.0.join("data/downloads"))
                .unwrap()
                .count(),
            0
        );
    }
}
