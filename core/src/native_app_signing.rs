//! Sign newly staged native aliases and verify cached aliases without a tool.
//!
//! A valid ad-hoc signature is not a payload identity. The derived executable
//! must also contain exactly the selected package's Mach-O bytes, apart from
//! structurally validated signature and LINKEDIT size bookkeeping.

use super::*;
use std::ffi::c_void;

const SIGN_TIMEOUT: Duration = Duration::from_millis(600);
const LC_SEGMENT_64: u32 = 0x19;
const LC_CODE_SIGNATURE: u32 = 0x1d;

fn invalid() -> String {
    "Native loader has an unsupported or malformed signed Mach-O layout".into()
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(8).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ))
}

fn be32(bytes: &[u8], offset: usize) -> Result<usize, String> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ) as usize)
}

fn checked_range(offset: usize, size: usize, end: usize) -> Result<std::ops::Range<usize>, String> {
    let limit = offset.checked_add(size).ok_or_else(invalid)?;
    if limit > end {
        return Err(invalid());
    }
    Ok(offset..limit)
}

// Code-signature data can reserve more bytes than the SuperBlob uses. Every
// indexed blob must fit, indexes must be unique, and unused tail must be zero.
fn validate_signature_blob(blob: &[u8]) -> Result<(), String> {
    if be32(blob, 0)? != 0xfade0cc0 {
        return Err(invalid());
    }
    let length = be32(blob, 4)?;
    let count = be32(blob, 8)?;
    let index_end = count
        .checked_mul(8)
        .and_then(|n| n.checked_add(12))
        .ok_or_else(invalid)?;
    if count == 0
        || count > 64
        || index_end > length
        || length > blob.len()
        || blob[length..].iter().any(|byte| *byte != 0)
    {
        return Err(invalid());
    }
    let mut slots = std::collections::BTreeSet::new();
    let mut ranges = Vec::new();
    let mut code_directory = false;
    for index in 0..count {
        let slot = be32(blob, 12 + index * 8)?;
        let offset = be32(blob, 16 + index * 8)?;
        if !slots.insert(slot) || offset < index_end {
            return Err(invalid());
        }
        checked_range(offset, 8, length)?;
        let size = be32(blob, offset + 4)?;
        if size < 8 {
            return Err(invalid());
        }
        let range = checked_range(offset, size, length)?;
        if ranges
            .iter()
            .any(|old: &std::ops::Range<usize>| range.start < old.end && old.start < range.end)
        {
            return Err(invalid());
        }
        if slot == 0 {
            if be32(blob, offset)? != 0xfade0c02 || size < 44 {
                return Err(invalid());
            }
            code_directory = true;
        }
        ranges.push(range);
    }
    if !code_directory {
        return Err(invalid());
    }
    Ok(())
}

// This intentionally supports only thin x86_64 executables with a final
// LC_CODE_SIGNATURE. Unknown commands are refused rather than guessing which
// file data could be hidden behind the signature interval.
fn unsigned_payload(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 32
        || bytes.len() as u64 > MAX_LOADER_BYTES
        || u32_at(bytes, 0)? != 0xfeedfacf
        || u32_at(bytes, 4)? != 0x0100_0007
        || u32_at(bytes, 12)? != 2
    {
        return Err(invalid());
    }
    let count = u32_at(bytes, 16)? as usize;
    let command_size = u32_at(bytes, 20)? as usize;
    if count == 0 || count > 128 || command_size < count * 8 || command_size > 64 * 1024 {
        return Err(invalid());
    }
    let commands_end = checked_range(32, command_size, bytes.len())?.end;
    let mut cursor = 32;
    let mut linkedit = None;
    let mut signature = None;
    let mut data_ranges = Vec::new();
    let mut segment_ranges = Vec::new();
    for _ in 0..count {
        let command = u32_at(bytes, cursor)?;
        let size = u32_at(bytes, cursor + 4)? as usize;
        if size < 8 || size % 8 != 0 {
            return Err(invalid());
        }
        let end = checked_range(cursor, size, commands_end)?.end;
        let mut add_data = |offset: u64, length: u64| -> Result<(), String> {
            let offset = usize::try_from(offset).map_err(|_| invalid())?;
            let length = usize::try_from(length).map_err(|_| invalid())?;
            let range = checked_range(offset, length, bytes.len())?;
            if length != 0 {
                data_ranges.push(range);
            }
            Ok(())
        };
        match command {
            LC_SEGMENT_64 => {
                if size < 72 {
                    return Err(invalid());
                }
                let sections = u32_at(bytes, cursor + 64)? as usize;
                if size != 72 + sections.checked_mul(80).ok_or_else(invalid)? {
                    return Err(invalid());
                }
                let fileoff = u64_at(bytes, cursor + 40)?;
                let filesize = u64_at(bytes, cursor + 48)?;
                let vmsize = u64_at(bytes, cursor + 32)?;
                u64_at(bytes, cursor + 24)?
                    .checked_add(vmsize)
                    .ok_or_else(invalid)?;
                if filesize > vmsize {
                    return Err(invalid());
                }
                let range = checked_range(
                    usize::try_from(fileoff).map_err(|_| invalid())?,
                    usize::try_from(filesize).map_err(|_| invalid())?,
                    bytes.len(),
                )?;
                if bytes[cursor + 8..cursor + 24] == *b"__LINKEDIT\0\0\0\0\0\0" {
                    if linkedit.is_some()
                        || sections != 0
                        || fileoff < commands_end as u64
                        || u32_at(bytes, cursor + 56)? != 1
                        || u32_at(bytes, cursor + 60)? != 1
                        || vmsize % 4096 != 0
                        || vmsize > filesize.checked_add(16383).ok_or_else(invalid)? / 16384 * 16384
                    {
                        return Err(invalid());
                    }
                    linkedit = Some((cursor, range, vmsize));
                } else if filesize != 0 {
                    if segment_ranges.iter().any(|old: &std::ops::Range<usize>| {
                        range.start < old.end && old.start < range.end
                    }) {
                        return Err(invalid());
                    }
                    segment_ranges.push(range);
                }
                for section in 0..sections {
                    let base = cursor + 72 + section * 80;
                    let relocation = u32_at(bytes, base + 56)? as u64;
                    let count = u32_at(bytes, base + 60)? as u64;
                    add_data(relocation, count.checked_mul(8).ok_or_else(invalid)?)?;
                }
            }
            LC_CODE_SIGNATURE => {
                if size != 16 || end != commands_end || signature.is_some() {
                    return Err(invalid());
                }
                let offset = u32_at(bytes, cursor + 8)? as usize;
                let length = u32_at(bytes, cursor + 12)? as usize;
                let range = checked_range(offset, length, bytes.len())?;
                if length == 0 || range.end != bytes.len() {
                    return Err(invalid());
                }
                validate_signature_blob(&bytes[range.clone()])?;
                signature = Some((cursor, range));
            }
            0x2 => {
                if size != 24 {
                    return Err(invalid());
                }
                add_data(
                    u32_at(bytes, cursor + 8)? as u64,
                    (u32_at(bytes, cursor + 12)? as u64) * 16,
                )?;
                add_data(
                    u32_at(bytes, cursor + 16)? as u64,
                    u32_at(bytes, cursor + 20)? as u64,
                )?;
            }
            0xb => {
                if size != 80 {
                    return Err(invalid());
                }
                for (field, width) in [(6, 8_u64), (8, 56), (10, 4), (12, 4), (14, 8), (16, 8)] {
                    add_data(
                        u32_at(bytes, cursor + 8 + field * 4)? as u64,
                        (u32_at(bytes, cursor + 12 + field * 4)? as u64) * width,
                    )?;
                }
            }
            0x22 | 0x8000_0022 => {
                if size != 48 {
                    return Err(invalid());
                }
                for index in 0..5 {
                    add_data(
                        u32_at(bytes, cursor + 8 + index * 8)? as u64,
                        u32_at(bytes, cursor + 12 + index * 8)? as u64,
                    )?;
                }
            }
            0x26 | 0x29 | 0x2b | 0x2e | 0x8000_0033 | 0x8000_0034 => {
                if size != 16 {
                    return Err(invalid());
                }
                add_data(
                    u32_at(bytes, cursor + 8)? as u64,
                    u32_at(bytes, cursor + 12)? as u64,
                )?;
            }
            0x16 => {
                if size != 16 {
                    return Err(invalid());
                }
                add_data(
                    u32_at(bytes, cursor + 8)? as u64,
                    (u32_at(bytes, cursor + 12)? as u64) * 4,
                )?;
            }
            // These contain no separate LINKEDIT payload. Their complete
            // bytes (including UUID, entry point, flags and dependencies)
            // stay in the comparison below.
            0x5 | 0xc | 0xe | 0x1b | 0x24 | 0x2a | 0x32 | 0x8000_0018 | 0x8000_001c
            | 0x8000_001f | 0x8000_0023 | 0x8000_0028 => {}
            _ => return Err(invalid()),
        }
        cursor = end;
    }
    if cursor != commands_end {
        return Err(invalid());
    }
    let (segment, link_range, _) = linkedit.ok_or_else(invalid)?;
    let (signature_command, signature_range) = signature.ok_or_else(invalid)?;
    if link_range.end != bytes.len()
        || segment_ranges
            .iter()
            .any(|range| range.end > link_range.start)
    {
        return Err(invalid());
    }
    let mut payload_end = link_range.start;
    for range in data_ranges {
        if range.start < link_range.start || range.end > signature_range.start {
            return Err(invalid());
        }
        payload_end = payload_end.max(range.end);
    }
    if signature_range.start != payload_end.checked_add(15).ok_or_else(invalid)? & !15
        || bytes[payload_end..signature_range.start]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(invalid());
    }
    let mut result = bytes[..payload_end].to_vec();
    result[16..20].copy_from_slice(&((count - 1) as u32).to_le_bytes());
    result[20..24].copy_from_slice(&((command_size - 16) as u32).to_le_bytes());
    result[signature_command..signature_command + 16].fill(0);
    result[segment + 32..segment + 40].fill(0); // validated signature-dependent VM extent
    result[segment + 48..segment + 56]
        .copy_from_slice(&((payload_end - link_range.start) as u64).to_le_bytes());
    Ok(result)
}

#[cfg(test)]
fn synthetic_loader(bytes: &[u8]) -> bool {
    bytes.len() == 64
        && bytes[..8] == [0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]
        && bytes[8..].iter().all(|byte| *byte == 0)
}

pub(super) fn loader_code_matches(source: &[u8], derived: &[u8]) -> Result<bool, String> {
    #[cfg(test)]
    if synthetic_loader(source) {
        return Ok(source == derived);
    }
    Ok(unsigned_payload(source)? == unsigned_payload(derived)?)
}

#[cfg(target_os = "macos")]
mod security {
    use super::*;
    type CFRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFRef,
            bytes: *const u8,
            length: isize,
            directory: u8,
        ) -> CFRef;
        fn CFStringCreateWithBytes(
            allocator: CFRef,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> CFRef;
        fn CFRelease(value: CFRef);
    }
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecStaticCodeCreateWithPath(path: CFRef, flags: u32, code: *mut CFRef) -> i32;
        fn SecRequirementCreateWithString(text: CFRef, flags: u32, requirement: *mut CFRef) -> i32;
        fn SecStaticCodeCheckValidity(code: CFRef, flags: u32, requirement: CFRef) -> i32;
    }
    struct Owned(CFRef);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CFRelease(self.0);
                }
            }
        }
    }
    pub(super) fn verify(app: &Path, identifier: &str) -> Result<(), String> {
        if identifier.len() > 256
            || identifier.is_empty()
            || !identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err("Invalid generated native bundle identifier".into());
        }
        let raw = app.as_os_str().as_bytes();
        let url = Owned(unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                raw.as_ptr(),
                raw.len() as isize,
                1,
            )
        });
        if url.0.is_null() {
            return Err("Could not create native bundle signature URL".into());
        }
        let mut code = Owned(std::ptr::null());
        let status = unsafe { SecStaticCodeCreateWithPath(url.0, 0, &mut code.0) };
        if status != 0 {
            return Err(format!(
                "Could not inspect native bundle signature ({status})"
            ));
        }
        let text = format!("identifier \"{identifier}\"");
        let string = Owned(unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as isize,
                0x0800_0100,
                0,
            )
        });
        if string.0.is_null() {
            return Err("Could not create native bundle signature requirement".into());
        }
        let mut requirement = Owned(std::ptr::null());
        let status = unsafe { SecRequirementCreateWithString(string.0, 0, &mut requirement.0) };
        if status != 0 {
            return Err(format!("Could not bind native bundle signature ({status})"));
        }
        // SDK SecStaticCode.h: all architectures, nested code, strict seal and
        // restricted symlinks. Never opt in to kSecCSAllowNetworkAccess.
        let flags = (1 << 0) | (1 << 3) | (1 << 4) | (1 << 7);
        let status = unsafe { SecStaticCodeCheckValidity(code.0, flags, requirement.0) };
        if status != 0 {
            return Err(format!(
                "Generated native bundle signature is invalid ({status})"
            ));
        }
        Ok(())
    }
}

pub(super) fn verify_bundle(app: &Path, identifier: &str, source: &[u8]) -> Result<(), String> {
    #[cfg(test)]
    if synthetic_loader(source) {
        return Ok(());
    }
    let _ = source;
    #[cfg(target_os = "macos")]
    {
        security::verify(app, identifier)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, identifier, source);
        Err("Native alias signing requires macOS".into())
    }
}

pub(super) fn sign_staging(app: &Path, source: &[u8]) -> Result<(), String> {
    #[cfg(test)]
    if synthetic_loader(source) {
        return Ok(());
    }
    let staging = app
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(".native-app-"))
        .and_then(|name| name.strip_suffix(".app"));
    if !staging.is_some_and(|value| Uuid::parse_str(value).is_ok())
        || !app.is_absolute()
        || app.canonicalize().map_err(|error| error.to_string())? != app
    {
        return Err("Native signing is restricted to a new private staging bundle".into());
    }
    // Check the reviewed source before invoking the trusted signer. The path
    // is a newly created private staging bundle, never a running/cache alias.
    unsigned_payload(source)?;
    if read_stable_file(&app.join("Contents/MacOS/WindowsApp"), MAX_LOADER_BYTES)? != source {
        return Err("Staged native loader differs from the exact copied package source".into());
    }
    #[cfg(test)]
    SIGN_COUNT.with(|count| count.set(count.get() + 1));
    let mut child = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--timestamp=none",
            "--preserve-metadata=flags,entitlements",
        ])
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not sign native bundle: {error}"))?;
    if !wait_child_bounded(&mut child, SIGN_TIMEOUT)?.is_some_and(|status| status.success()) {
        return Err("Native bundle signing failed or exceeded its deadline".into());
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static SIGN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
pub(super) fn test_sign_count() -> usize {
    SIGN_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(super) fn test_loader_bytes(revision: u8) -> Vec<u8> {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
    fs::create_dir_all(&parent).unwrap();
    let temporary = parent.join(format!("native-signing-compiler-{}", Uuid::new_v4()));
    fs::create_dir(&temporary).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temporary.clone());
    let code = temporary.join("fixture.c");
    fs::write(&code, format!("int main(void) {{ return {revision}; }}\n")).unwrap();
    let binary = temporary.join("OriginalFixture");
    let mut compiler = Command::new("/usr/bin/clang")
        .args(["-arch", "x86_64", "-mmacosx-version-min=13.0"])
        .arg(&code)
        .arg("-o")
        .arg(&binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert!(
        wait_child_bounded(&mut compiler, Duration::from_secs(10))
            .unwrap()
            .is_some_and(|status| status.success())
    );
    let status = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--timestamp=none",
            "--identifier",
            "dev.caskrun.loader.fixture",
        ])
        .arg(&binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    fs::read(binary).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        app: PathBuf,
        source: Vec<u8>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("native-signing-{}", Uuid::new_v4()));
            let app = root.join(format!(".native-app-{}.app", Uuid::new_v4()));
            fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            fs::create_dir_all(app.join("Contents/Resources")).unwrap();
            let source = test_loader_bytes(0);
            write_new(&app.join("Contents/MacOS/WindowsApp"), &source, 0o755).unwrap();
            write_new(
                &app.join("Contents/Info.plist"),
                plist(
                    "Fixture App",
                    "11111111-1111-4111-8111-111111111111",
                    &"a".repeat(64),
                    false,
                    false,
                )
                .as_bytes(),
                0o600,
            )
            .unwrap();
            write_new(
                &app.join("Contents/Resources/payload.txt"),
                b"Original fixture resource",
                0o600,
            )
            .unwrap();
            Self { root, app, source }
        }
        fn identifier(&self) -> String {
            bundle_identifier("11111111-1111-4111-8111-111111111111", &"a".repeat(64))
        }
        fn loader(&self) -> PathBuf {
            self.app.join("Contents/MacOS/WindowsApp")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn signed_bundle_retains_the_exact_source_payload_and_matches_apple_stripping() {
        let fixture = Fixture::new();
        sign_staging(&fixture.app, &fixture.source).unwrap();
        verify_bundle(&fixture.app, &fixture.identifier(), &fixture.source).unwrap();
        let derived = fs::read(fixture.loader()).unwrap();
        assert_ne!(derived, fixture.source);
        assert!(loader_code_matches(&fixture.source, &derived).unwrap());
        let source_copy = fixture.root.join("Original");
        let derived_copy = fixture.root.join("Derived");
        for (path, bytes) in [(&source_copy, &fixture.source), (&derived_copy, &derived)] {
            fs::write(path, bytes).unwrap();
            assert!(
                Command::new("/usr/bin/codesign")
                    .arg("--remove-signature")
                    .arg(path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let left = fs::read(source_copy).unwrap();
        let mut right = fs::read(derived_copy).unwrap();
        assert_eq!(left, right, "Apple's full unsigned bytes must match too");
        let count = u32_at(&right, 16).unwrap();
        let mut cursor = 32;
        for _ in 0..count {
            let command = u32_at(&right, cursor).unwrap();
            let size = u32_at(&right, cursor + 4).unwrap() as usize;
            if command == LC_SEGMENT_64
                && right[cursor + 8..cursor + 24] == *b"__LINKEDIT\0\0\0\0\0\0"
            {
                right[cursor + 32..cursor + 40].fill(0);
            }
            cursor += size;
        }
        assert_eq!(unsigned_payload(&derived).unwrap(), right);
        assert!(
            verify_bundle(&fixture.app, "dev.caskrun.wrong.identity", &fixture.source).is_err()
        );
    }

    #[test]
    fn a_valid_seal_cannot_replace_source_code_and_seal_checks_include_info_and_resources() {
        let fixture = Fixture::new();
        sign_staging(&fixture.app, &fixture.source).unwrap();
        let info = fixture.app.join("Contents/Info.plist");
        let original_info = fs::read(&info).unwrap();
        fs::write(&info, b"tampered metadata").unwrap();
        assert!(verify_bundle(&fixture.app, &fixture.identifier(), &fixture.source).is_err());
        fs::write(&info, &original_info).unwrap();
        fs::write(
            fixture.app.join("Contents/Resources/payload.txt"),
            b"changed resource",
        )
        .unwrap();
        assert!(verify_bundle(&fixture.app, &fixture.identifier(), &fixture.source).is_err());
        fs::write(
            fixture.app.join("Contents/Resources/payload.txt"),
            b"Original fixture resource",
        )
        .unwrap();
        let different = test_loader_bytes(7);
        fs::write(fixture.loader(), &different).unwrap();
        assert!(sign_staging(&fixture.app, &fixture.source).is_err());
        // Model an attacker who can make an ad-hoc seal of unrelated code.
        // The independent payload identity check must still reject it.
        assert!(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-", "--timestamp=none"])
                .arg(&fixture.app)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        verify_bundle(&fixture.app, &fixture.identifier(), &fixture.source).unwrap();
        assert!(
            !loader_code_matches(&fixture.source, &fs::read(fixture.loader()).unwrap()).unwrap()
        );
        let final_path = fixture.root.join("Published.app");
        fs::rename(&fixture.app, &final_path).unwrap();
        assert!(
            sign_staging(&final_path, &fixture.source).is_err(),
            "A cache/final alias can never be resigned in place"
        );
    }

    #[test]
    fn malformed_signature_commands_blob_ranges_and_extra_payload_are_refused() {
        let source = test_loader_bytes(0);
        let count = u32_at(&source, 16).unwrap();
        let mut cursor = 32;
        for _ in 0..count {
            if u32_at(&source, cursor).unwrap() == LC_CODE_SIGNATURE {
                break;
            }
            cursor += u32_at(&source, cursor + 4).unwrap() as usize;
        }
        let offset = u32_at(&source, cursor + 8).unwrap() as usize;
        for (at, value) in [
            (cursor + 4, 8_u32),
            (cursor + 8, 0),
            (offset + 4, 1_u32.to_be()),
            (offset + 8, 65_u32.to_be()),
        ] {
            let mut changed = source.clone();
            changed[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(loader_code_matches(&source, &changed).is_err());
        }
        let mut extra = source.clone();
        extra.push(0);
        assert!(loader_code_matches(&source, &extra).is_err());
        let mut architecture = source.clone();
        architecture[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
        assert!(loader_code_matches(&source, &architecture).is_err());
        let mut changed_code = source.clone();
        changed_code[4096] ^= 1;
        assert!(!loader_code_matches(&source, &changed_code).unwrap());
    }
}
