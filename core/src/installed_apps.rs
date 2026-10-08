//! Conservative, read-only suggestions after a user-driven Windows install.
//! An EXE's presence is not evidence that the installer completed or the app runs.
//! Shortcut structure follows Microsoft's MS-SHLLINK specification:
//! https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-shllink/747629b3-b5be-452a-8101-b9a2ec49978c

use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::{O_NOFOLLOW_ANY, same_snapshot};

const MAX_SCAN_ENTRIES: usize = 50_000;
const MAX_DEPTH: u8 = 6;
const MAX_FOUND: usize = 512;
const MAX_RETURNED: usize = 64;
const MAX_PE_OFFSET: u64 = 64 * 1024;
const MAX_SHORTCUT_SIZE: u64 = 1024 * 1024;
const MAX_SHORTCUT_DEPTH: u8 = 6;
const MAX_USERS: usize = 128;
// Matches the bounds the core applies when it saves a launcher's arguments.
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_ARGUMENTS_TOTAL_BYTES: usize = 16 * 1024;
// A work bound on the raw string. Quoting can only shrink a command, so this
// cannot reject a command whose parsed form would be within the limits above.
const MAX_ARGUMENT_STRING_BYTES: usize = 64 * 1024;
const SHELL_LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

#[derive(Debug, Serialize)]
pub(crate) struct DiscoveryResult {
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Candidate {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) source: &'static str,
    /// A shortcut's own Windows arguments, already split the way Windows splits
    /// them. Saving a suggestion without these would launch a different command.
    /// Always serialized, including when empty, so a reader never has to infer
    /// the difference between "no arguments" and "an older core".
    #[serde(default)]
    pub(crate) arguments: Vec<String>,
    /// A checked directory relative to the prefix, beginning with drive_c.
    /// None retains the launcher's existing executable-parent behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) working_directory: Option<String>,
}

struct Found {
    candidate: Candidate,
    rank: u8,
}

struct PendingDirectory {
    path: PathBuf,
    depth: u8,
    product: String,
}

struct PendingShortcutDirectory {
    path: PathBuf,
    depth: u8,
    source: &'static str,
}

pub(crate) fn discover(prefix: &Path) -> Result<DiscoveryResult, String> {
    let mut result = DiscoveryResult {
        candidates: Vec::new(),
        truncated: false,
    };
    let drive = prefix.join("drive_c");
    let drive_metadata = match fs::symlink_metadata(&drive) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Cannot inspect the bottle's C: drive: {error}")),
    };
    if !drive_metadata.file_type().is_dir() {
        return Err("Bottle C: drive must be a real directory".into());
    }
    let canonical_drive = drive.canonicalize().map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let mut examined = 0usize;
    let shortcut_targets = scan_shortcuts(
        &drive,
        &canonical_drive,
        &mut found,
        &mut examined,
        &mut result.truncated,
    );
    let represented_products: HashSet<PathBuf> = shortcut_targets
        .iter()
        .filter_map(|target| program_files_product_dir(&canonical_drive, target))
        .collect();
    let mut pending = VecDeque::new();
    for root_name in ["Program Files", "Program Files (x86)"] {
        let root = drive.join(root_name);
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_dir() => pending.push_back(PendingDirectory {
                path: root,
                depth: 0,
                product: String::new(),
            }),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => result.truncated = true,
        }
    }

    'scan: while let Some(directory) = pending.pop_front() {
        let entries = match fs::read_dir(&directory.path) {
            Ok(entries) => entries,
            Err(_) => {
                result.truncated = true;
                continue;
            }
        };
        let mut bounded_entries = Vec::new();
        let mut hit_entry_limit = false;
        for entry in entries {
            examined += 1;
            if examined > MAX_SCAN_ENTRIES {
                result.truncated = true;
                hit_entry_limit = true;
                break;
            }
            match entry {
                Ok(entry) => bounded_entries.push(entry),
                Err(_) => result.truncated = true,
            }
        }
        bounded_entries.sort_by_key(|entry| entry.file_name());
        for entry in bounded_entries {
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    result.truncated = true;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.file_type().is_dir() {
                let dirname = entry.file_name().to_string_lossy().into_owned();
                if directory.depth == 0 && skip_top_level_dir(&dirname) {
                    continue;
                }
                if directory.depth == 0
                    && path
                        .canonicalize()
                        .is_ok_and(|product| represented_products.contains(&product))
                {
                    continue;
                }
                if directory.depth >= MAX_DEPTH {
                    result.truncated = true;
                    continue;
                }
                let product = if directory.depth == 0 {
                    dirname
                } else {
                    directory.product.clone()
                };
                pending.push_back(PendingDirectory {
                    path,
                    depth: directory.depth + 1,
                    product,
                });
                continue;
            }
            // Programs installed directly in Program Files are often support
            // tools. Require a product directory before suggesting a launcher.
            if directory.depth == 0 || !metadata.file_type().is_file() {
                continue;
            }
            let Some(stem) = plausible_exe_name(&path) else {
                continue;
            };
            if !is_windows_executable(&path, &metadata) {
                continue;
            }
            let resolved = match path.canonicalize() {
                Ok(path) if path.starts_with(&canonical_drive) => path,
                _ => {
                    result.truncated = true;
                    continue;
                }
            };
            let parent = path
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rank = if normalized(&stem) == normalized(&directory.product) {
                2
            } else if normalized(&stem) == normalized(&parent) {
                3
            } else if directory.depth == 1 {
                4
            } else {
                5
            };
            found.push(Found {
                candidate: Candidate {
                    name: stem,
                    path: resolved.display().to_string(),
                    source: "program-files",
                    arguments: Vec::new(),
                    working_directory: None,
                },
                rank,
            });
            if found.len() >= MAX_FOUND {
                result.truncated = true;
                break 'scan;
            }
        }
        if hit_entry_limit {
            break 'scan;
        }
    }
    found.retain(|item| {
        item.candidate.source != "program-files"
            || shortcut_targets.contains(&item.candidate.path)
            || !program_files_product_dir(&canonical_drive, &item.candidate.path)
                .is_some_and(|product| represented_products.contains(&product))
    });
    found.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then_with(|| {
                a.candidate
                    .name
                    .to_lowercase()
                    .cmp(&b.candidate.name.to_lowercase())
            })
            .then_with(|| a.candidate.path.cmp(&b.candidate.path))
    });
    let mut seen_commands = HashSet::new();
    for item in found {
        let command = (
            item.candidate.path.clone(),
            item.candidate.arguments.clone(),
            item.candidate.working_directory.clone(),
        );
        if !seen_commands.insert(command) {
            continue;
        }
        if result.candidates.len() >= MAX_RETURNED {
            result.truncated = true;
            break;
        }
        result.candidates.push(item.candidate);
    }
    Ok(result)
}

fn program_files_product_dir(canonical_drive: &Path, target: &str) -> Option<PathBuf> {
    let relative = Path::new(target).strip_prefix(canonical_drive).ok()?;
    let mut parts = relative.iter();
    let root = parts.next()?.to_str()?;
    if !root.eq_ignore_ascii_case("Program Files")
        && !root.eq_ignore_ascii_case("Program Files (x86)")
    {
        return None;
    }
    let product = parts.next()?;
    // An EXE directly in Program Files does not identify a product folder.
    parts.next()?;
    Some(canonical_drive.join(root).join(product))
}

fn scan_shortcuts(
    drive: &Path,
    canonical_drive: &Path,
    found: &mut Vec<Found>,
    examined: &mut usize,
    truncated: &mut bool,
) -> HashSet<String> {
    // Even an unsupported complete shortcut suppresses a bare EXE fallback.
    // Otherwise its requested arguments or working directory would be lost.
    let mut shortcut_targets = HashSet::new();
    let mut pending = VecDeque::new();
    if let Some(root) = real_directory_chain(
        drive,
        &[
            "ProgramData",
            "Microsoft",
            "Windows",
            "Start Menu",
            "Programs",
        ],
        truncated,
    ) {
        pending.push_back(PendingShortcutDirectory {
            path: root,
            depth: 0,
            source: "start-menu",
        });
    }
    if let Some(users) = real_directory_chain(drive, &["users"], truncated) {
        let mut user_dirs = Vec::new();
        if let Ok(entries) = fs::read_dir(&users) {
            for entry in entries {
                *examined += 1;
                if *examined > MAX_SCAN_ENTRIES {
                    *truncated = true;
                    break;
                }
                match entry {
                    Ok(entry) => user_dirs.push(entry),
                    Err(_) => *truncated = true,
                }
            }
        } else {
            *truncated = true;
        }
        user_dirs.sort_by_key(|entry| entry.file_name());
        if user_dirs.len() > MAX_USERS {
            user_dirs.truncate(MAX_USERS);
            *truncated = true;
        }
        for entry in user_dirs {
            let user_path = entry.path();
            match fs::symlink_metadata(&user_path) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(metadata) if metadata.file_type().is_symlink() => continue,
                _ => continue,
            }
            let Some(user) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            for (tail, source) in [
                (
                    &[
                        "AppData",
                        "Roaming",
                        "Microsoft",
                        "Windows",
                        "Start Menu",
                        "Programs",
                    ][..],
                    "start-menu",
                ),
                (&["Desktop"][..], "desktop"),
            ] {
                let mut components = vec!["users", user.as_str()];
                components.extend_from_slice(tail);
                if let Some(root) = real_directory_chain(drive, &components, truncated) {
                    pending.push_back(PendingShortcutDirectory {
                        path: root,
                        depth: 0,
                        source,
                    });
                }
            }
        }
    }

    let mut shortcut_count = 0;
    'scan: while let Some(directory) = pending.pop_front() {
        if *examined >= MAX_SCAN_ENTRIES || shortcut_count >= MAX_FOUND {
            *truncated = true;
            break;
        }
        let entries = match fs::read_dir(&directory.path) {
            Ok(entries) => entries,
            Err(_) => {
                *truncated = true;
                continue;
            }
        };
        let mut bounded_entries = Vec::new();
        for entry in entries {
            *examined += 1;
            if *examined > MAX_SCAN_ENTRIES {
                *truncated = true;
                break 'scan;
            }
            match entry {
                Ok(entry) => bounded_entries.push(entry),
                Err(_) => *truncated = true,
            }
        }
        bounded_entries.sort_by_key(|entry| entry.file_name());
        for entry in bounded_entries {
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    *truncated = true;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.file_type().is_dir() {
                if directory.depth >= MAX_SHORTCUT_DEPTH {
                    *truncated = true;
                } else {
                    pending.push_back(PendingShortcutDirectory {
                        path,
                        depth: directory.depth + 1,
                        source: directory.source,
                    });
                }
                continue;
            }
            if !metadata.file_type().is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
            {
                continue;
            }
            let Some(name) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(plausible_name)
            else {
                continue;
            };
            let Some(shortcut) = read_shortcut(&path, &metadata) else {
                continue;
            };
            let Some(target) = resolve_shortcut_target(
                drive,
                canonical_drive,
                path.parent().unwrap_or(drive),
                &shortcut,
            ) else {
                continue;
            };
            shortcut_targets.insert(target.display().to_string());
            // A command this parser cannot reproduce completely is skipped.
            let Some(arguments) = parse_windows_arguments(&shortcut.arguments) else {
                continue;
            };
            let working_directory = match shortcut.working_directory.as_deref() {
                None | Some("") => None,
                Some(working) => {
                    let Some(value) = checked_shortcut_working_directory(drive, working) else {
                        continue;
                    };
                    Some(value)
                }
            };
            found.push(Found {
                candidate: Candidate {
                    name,
                    path: target.display().to_string(),
                    source: directory.source,
                    arguments,
                    working_directory,
                },
                rank: if directory.source == "start-menu" {
                    0
                } else {
                    1
                },
            });
            shortcut_count += 1;
            if shortcut_count >= MAX_FOUND {
                *truncated = true;
                break 'scan;
            }
        }
    }
    shortcut_targets
}

/// Split a shortcut's argument string the way Windows splits a command tail,
/// following the quoting and backslash rules of CommandLineToArgvW. Returns None
/// for a command this cannot represent exactly, so a partial command is never
/// suggested. Reference:
/// https://learn.microsoft.com/en-us/cpp/cpp/main-function-command-line-args
fn parse_windows_arguments(input: &str) -> Option<Vec<String>> {
    if input.len() > MAX_ARGUMENT_STRING_BYTES {
        return None;
    }
    // Tab separates arguments like a space; any other control character cannot
    // survive a round trip through a saved command.
    if input.chars().any(|c| c.is_control() && c != '\t') {
        return None;
    }
    let mut arguments = Vec::new();
    let characters: Vec<char> = input.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        while index < characters.len() && matches!(characters[index], ' ' | '\t') {
            index += 1;
        }
        if index >= characters.len() {
            break;
        }
        let mut current = String::new();
        let mut quoted = false;
        while index < characters.len() {
            match characters[index] {
                '\\' => {
                    let mut slashes = 0;
                    while index < characters.len() && characters[index] == '\\' {
                        slashes += 1;
                        index += 1;
                    }
                    if index < characters.len() && characters[index] == '"' {
                        // Each pair of backslashes is one literal backslash; an odd
                        // one escapes the quote instead of toggling quoting.
                        current.extend(std::iter::repeat_n('\\', slashes / 2));
                        if slashes % 2 == 1 {
                            current.push('"');
                            index += 1;
                        }
                    } else {
                        current.extend(std::iter::repeat_n('\\', slashes));
                    }
                }
                '"' => {
                    index += 1;
                    if quoted && index < characters.len() && characters[index] == '"' {
                        // Inside quotes, a doubled quote is one literal quote.
                        current.push('"');
                        index += 1;
                    } else {
                        quoted = !quoted;
                    }
                }
                ' ' | '\t' if !quoted => break,
                character => {
                    current.push(character);
                    index += 1;
                }
            }
        }
        if quoted {
            // An unterminated quote means the command was not understood.
            return None;
        }
        if current.len() > MAX_ARGUMENT_BYTES {
            return None;
        }
        arguments.push(current);
        if arguments.len() > MAX_ARGUMENTS {
            return None;
        }
    }
    // Never suggest a command the core would refuse to save.
    if arguments.iter().map(String::len).sum::<usize>() > MAX_ARGUMENTS_TOTAL_BYTES {
        return None;
    }
    Some(arguments)
}

fn real_directory_chain(drive: &Path, parts: &[&str], truncated: &mut bool) -> Option<PathBuf> {
    let mut path = drive.to_path_buf();
    for part in parts {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => {
                *truncated = true;
                return None;
            }
        }
    }
    Some(path)
}

struct Shortcut {
    local_target: Option<String>,
    relative_target: Option<String>,
    working_directory: Option<String>,
    arguments: String,
}

fn read_shortcut(path: &Path, before: &fs::Metadata) -> Option<Shortcut> {
    if before.len() < 76 || before.len() > MAX_SHORTCUT_SIZE {
        return None;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .ok()?;
    if !same_snapshot(before, &file.metadata().ok()?) {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_SHORTCUT_SIZE + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 != before.len() || !same_snapshot(before, &file.metadata().ok()?) {
        return None;
    }
    parse_shortcut(&bytes)
}

fn parse_shortcut(bytes: &[u8]) -> Option<Shortcut> {
    if read_u32(bytes, 0)? != 76 || bytes.get(4..20)? != SHELL_LINK_CLSID {
        return None;
    }
    let flags = read_u32(bytes, 20)?;
    let mut offset = 76usize;
    if flags & 1 != 0 {
        offset = offset.checked_add(2 + usize::from(read_u16(bytes, offset)?))?;
        bytes.get(..offset)?;
    }
    let mut local_target = None;
    if flags & 2 != 0 {
        let size = usize::try_from(read_u32(bytes, offset)?).ok()?;
        if size < 28 {
            return None;
        }
        let end = offset.checked_add(size)?;
        let link_info = bytes.get(offset..end)?;
        if flags & 0x100 == 0 {
            local_target = parse_link_info(link_info)?;
        }
        offset = end;
    }
    let unicode = flags & 0x80 != 0;
    if flags & 4 != 0 {
        read_string_data(bytes, &mut offset, unicode, 260)?;
    }
    let relative_target = if flags & 8 != 0 {
        Some(read_string_data(bytes, &mut offset, unicode, 260)?)
    } else {
        None
    };
    let working_directory = if flags & 16 != 0 {
        Some(read_string_data(bytes, &mut offset, unicode, 260)?)
    } else {
        None
    };
    let arguments = if flags & 32 != 0 {
        read_string_data(bytes, &mut offset, unicode, 4096)?
    } else {
        String::new()
    };
    if flags & 64 != 0 {
        read_string_data(bytes, &mut offset, unicode, 260)?;
    }
    Some(Shortcut {
        local_target,
        relative_target,
        working_directory,
        arguments,
    })
}

fn parse_link_info(bytes: &[u8]) -> Option<Option<String>> {
    let size = usize::try_from(read_u32(bytes, 0)?).ok()?;
    let header_size = usize::try_from(read_u32(bytes, 4)?).ok()?;
    if size != bytes.len() || (header_size != 28 && header_size < 36) || header_size > size {
        return None;
    }
    let flags = read_u32(bytes, 8)?;
    // Network targets and mapped drives are outside this bottle's C: drive.
    if flags & 2 != 0 {
        return None;
    }
    if flags & 1 == 0 {
        return Some(None);
    }
    let base = if header_size >= 36 {
        let base_offset = usize::try_from(read_u32(bytes, 28)?).ok()?;
        if base_offset != 0 {
            read_nt_utf16(bytes, base_offset, header_size)?
        } else {
            read_nt_ansi(
                bytes,
                usize::try_from(read_u32(bytes, 16)?).ok()?,
                header_size,
            )?
        }
    } else {
        read_nt_ansi(
            bytes,
            usize::try_from(read_u32(bytes, 16)?).ok()?,
            header_size,
        )?
    };
    let suffix = if header_size >= 36 {
        let suffix_offset = usize::try_from(read_u32(bytes, 32)?).ok()?;
        if suffix_offset != 0 {
            read_nt_utf16(bytes, suffix_offset, header_size)?
        } else {
            read_nt_ansi(
                bytes,
                usize::try_from(read_u32(bytes, 24)?).ok()?,
                header_size,
            )?
        }
    } else {
        read_nt_ansi(
            bytes,
            usize::try_from(read_u32(bytes, 24)?).ok()?,
            header_size,
        )?
    };
    let full = if suffix.is_empty() || base.to_lowercase().ends_with(&suffix.to_lowercase()) {
        base
    } else if base.ends_with(['\\', '/']) || suffix.starts_with(['\\', '/']) {
        format!("{base}{suffix}")
    } else {
        format!("{base}\\{suffix}")
    };
    Some(Some(full))
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_nt_ansi(bytes: &[u8], offset: usize, header_size: usize) -> Option<String> {
    let field = bytes.get(offset..)?;
    if offset < header_size {
        return None;
    }
    let end = field.iter().position(|byte| *byte == 0)?;
    if end > 4096 {
        return None;
    }
    Some(std::str::from_utf8(&field[..end]).ok()?.to_owned())
}

fn read_nt_utf16(bytes: &[u8], offset: usize, header_size: usize) -> Option<String> {
    if offset < header_size {
        return None;
    }
    let mut units = Vec::new();
    for index in 0..=4096 {
        let unit = read_u16(bytes, offset.checked_add(index * 2)?)?;
        if unit == 0 {
            return String::from_utf16(&units).ok();
        }
        units.push(unit);
    }
    None
}

fn read_string_data(
    bytes: &[u8],
    offset: &mut usize,
    unicode: bool,
    max_characters: usize,
) -> Option<String> {
    let count = usize::from(read_u16(bytes, *offset)?);
    if count > max_characters {
        return None;
    }
    *offset = offset.checked_add(2)?;
    let byte_count = count.checked_mul(if unicode { 2 } else { 1 })?;
    let end = offset.checked_add(byte_count)?;
    let field = bytes.get(*offset..end)?;
    *offset = end;
    let value = if unicode {
        let units: Vec<u16> = field
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).ok()?
    } else {
        String::from_utf8_lossy(field).into_owned()
    };
    let without_terminator = value.trim_end_matches('\0');
    if without_terminator.contains('\0') {
        return None;
    }
    Some(without_terminator.to_owned())
}

fn resolve_shortcut_target(
    drive: &Path,
    canonical_drive: &Path,
    shortcut_parent: &Path,
    shortcut: &Shortcut,
) -> Option<PathBuf> {
    let mut targets = Vec::new();
    if let Some(relative) = &shortcut.relative_target
        && let Some(target) = map_windows_path(drive, shortcut_parent, relative)
        && let Some(resolved) = checked_executable(drive, canonical_drive, &target)
    {
        targets.push(resolved);
    }
    if let Some(local) = &shortcut.local_target
        && let Some(target) = map_windows_path(drive, shortcut_parent, local)
        && let Some(resolved) = checked_executable(drive, canonical_drive, &target)
    {
        targets.push(resolved);
    }
    if targets.len() == 2 && targets[0] != targets[1] {
        return None;
    }
    let target = targets.into_iter().next()?;
    Some(target)
}

fn checked_shortcut_working_directory(drive: &Path, input: &str) -> Option<String> {
    // Relative paths and environment expansion depend on shell-link execution
    // context. Offer only an exact absolute C: path that maps to a real folder.
    let windows = input.replace('\\', "/");
    if windows.len() < 3
        || !windows.as_bytes()[0].eq_ignore_ascii_case(&b'c')
        || windows.as_bytes().get(1..3) != Some(b":/")
        || windows.contains('%')
    {
        return None;
    }
    let tail = windows[3..].trim_end_matches('/');
    let value = if tail.is_empty() {
        "drive_c".to_owned()
    } else {
        format!("drive_c/{tail}")
    };
    let prefix = drive.parent()?;
    crate::checked_working_directory(prefix, &value).ok()?;
    Some(value)
}

fn map_windows_path(drive: &Path, shortcut_parent: &Path, input: &str) -> Option<PathBuf> {
    if input.is_empty() || input.chars().any(|character| character.is_control()) {
        return None;
    }
    let path = input.replace('/', "\\");
    let (path, mut components) = if path.len() >= 3
        && path.as_bytes()[0].eq_ignore_ascii_case(&b'c')
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'\\'
    {
        (&path[3..], Vec::new())
    } else if path.starts_with('\\') || path.contains(':') {
        return None;
    } else {
        let parent = shortcut_parent.strip_prefix(drive).ok()?;
        (
            path.as_str(),
            parent
                .iter()
                .map(|part| part.to_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?,
        )
    };
    for part in path.split('\\') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            components.pop()?;
            continue;
        }
        if part.chars().any(|character| "<>:\"|?*".contains(character)) {
            return None;
        }
        components.push(part.to_owned());
    }
    if components.is_empty() {
        return None;
    }
    let mut result = drive.to_path_buf();
    for component in components {
        result.push(component);
    }
    Some(result)
}

fn checked_executable(drive: &Path, canonical_drive: &Path, target: &Path) -> Option<PathBuf> {
    let relative = target.strip_prefix(drive).ok()?;
    let mut path = drive.to_path_buf();
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        path.push(part);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
        if parts.peek().is_some() {
            if !metadata.file_type().is_dir() {
                return None;
            }
        } else if !metadata.file_type().is_file()
            || plausible_exe_name(&path).is_none()
            || !is_windows_executable(&path, &metadata)
        {
            return None;
        }
    }
    let resolved = path.canonicalize().ok()?;
    resolved.starts_with(canonical_drive).then_some(resolved)
}

fn skip_top_level_dir(name: &str) -> bool {
    [
        "Common Files",
        "Internet Explorer",
        "Windows Media Player",
        "Windows NT",
    ]
    .iter()
    .any(|system| name.eq_ignore_ascii_case(system))
}

fn plausible_exe_name(path: &Path) -> Option<String> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return None;
    }
    plausible_name(path.file_stem()?.to_str()?)
}

fn plausible_name(stem: &str) -> Option<String> {
    let stem = stem.trim();
    if stem.is_empty() || stem.chars().count() > 80 || stem.chars().any(char::is_control) {
        return None;
    }
    let display = stem.replace('_', " ");
    let display = display.trim();
    if display.is_empty() {
        return None;
    }
    let lower = stem.to_ascii_lowercase();
    if [
        "unins",
        "uninstall",
        "uninst",
        "setup",
        "install",
        "repair",
        "updater",
        "crashreport",
        "crashhandler",
        "crashpad",
        "steamwebhelper",
        "epicwebhelper",
        "easyanticheat",
        "unitycrashhandler",
        "vcredist",
        "vc_redist",
        "dxsetup",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
    {
        return None;
    }
    Some(display.to_owned())
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_windows_executable(path: &Path, before: &fs::Metadata) -> bool {
    if before.len() < 70 {
        return false;
    }
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|opened| same_snapshot(before, &opened))
    {
        return false;
    }
    let mut dos = [0u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    let offset = u64::from(u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()));
    if !(64..=MAX_PE_OFFSET).contains(&offset) || offset.saturating_add(6) > before.len() {
        return false;
    }
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut header = [0u8; 6];
    if file.read_exact(&mut header).is_err() || &header[..4] != b"PE\0\0" {
        return false;
    }
    // Windows x86 and x86-64 programs are the scope of this Wine build.
    let machine = u16::from_le_bytes([header[4], header[5]]);
    (machine == 0x014c || machine == 0x8664)
        && file
            .metadata()
            .is_ok_and(|after| same_snapshot(before, &after))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use uuid::Uuid;

    fn test_prefix() -> PathBuf {
        let root = std::env::current_dir()
            .unwrap()
            .join("target/test-data")
            .join(format!("discovery-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("drive_c")).unwrap();
        root
    }

    fn fake_pe(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0u8; 0x90];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    pub(crate) fn write_shortcut(path: &Path, target: &str, arguments: Option<&str>) {
        write_shortcut_with_working(path, target, None, arguments);
    }

    pub(crate) fn write_shortcut_with_working(
        path: &Path,
        target: &str,
        working: Option<&str>,
        arguments: Option<&str>,
    ) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut data = vec![0u8; 76];
        data[..4].copy_from_slice(&76u32.to_le_bytes());
        data[4..20].copy_from_slice(&SHELL_LINK_CLSID);
        let flags = 2u32
            | if working.is_some() { 16 | 128 } else { 0 }
            | if arguments.is_some() { 32 | 128 } else { 0 };
        data[20..24].copy_from_slice(&flags.to_le_bytes());

        let mut link_info = vec![0u8; 28];
        link_info[4..8].copy_from_slice(&28u32.to_le_bytes());
        link_info[8..12].copy_from_slice(&1u32.to_le_bytes());
        link_info[12..16].copy_from_slice(&28u32.to_le_bytes());
        // A minimal fixed-volume record, followed by the local path and
        // empty suffix as specified by MS-SHLLINK LinkInfo.
        let mut volume = vec![0u8; 17];
        volume[..4].copy_from_slice(&17u32.to_le_bytes());
        volume[4..8].copy_from_slice(&3u32.to_le_bytes());
        volume[12..16].copy_from_slice(&16u32.to_le_bytes());
        link_info.extend_from_slice(&volume);
        let base_offset = u32::try_from(link_info.len()).unwrap();
        link_info[16..20].copy_from_slice(&base_offset.to_le_bytes());
        link_info.extend_from_slice(target.as_bytes());
        link_info.push(0);
        let suffix_offset = u32::try_from(link_info.len()).unwrap();
        link_info[24..28].copy_from_slice(&suffix_offset.to_le_bytes());
        link_info.push(0);
        let size = u32::try_from(link_info.len()).unwrap();
        link_info[..4].copy_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&link_info);
        if let Some(working) = working {
            let units: Vec<u16> = working.encode_utf16().collect();
            data.extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
            for unit in units {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        }
        if let Some(arguments) = arguments {
            let units: Vec<u16> = arguments.encode_utf16().collect();
            data.extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
            for unit in units {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        }
        fs::write(path, data).unwrap();
    }

    fn write_relative_shortcut(path: &Path, target: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut data = vec![0u8; 76];
        data[..4].copy_from_slice(&76u32.to_le_bytes());
        data[4..20].copy_from_slice(&SHELL_LINK_CLSID);
        data[20..24].copy_from_slice(&(8u32 | 128).to_le_bytes());
        let units: Vec<u16> = target.encode_utf16().collect();
        data.extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
        for unit in units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(path, data).unwrap();
    }

    fn write_unicode_link_info_shortcut(path: &Path, target: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut data = vec![0u8; 76];
        data[..4].copy_from_slice(&76u32.to_le_bytes());
        data[4..20].copy_from_slice(&SHELL_LINK_CLSID);
        data[20..24].copy_from_slice(&2u32.to_le_bytes());
        let mut link_info = vec![0u8; 36];
        link_info[4..8].copy_from_slice(&36u32.to_le_bytes());
        link_info[8..12].copy_from_slice(&1u32.to_le_bytes());
        link_info[12..16].copy_from_slice(&36u32.to_le_bytes());
        let mut volume = vec![0u8; 17];
        volume[..4].copy_from_slice(&17u32.to_le_bytes());
        volume[4..8].copy_from_slice(&3u32.to_le_bytes());
        volume[12..16].copy_from_slice(&16u32.to_le_bytes());
        link_info.extend_from_slice(&volume);
        let ansi_offset = u32::try_from(link_info.len()).unwrap();
        link_info[16..20].copy_from_slice(&ansi_offset.to_le_bytes());
        link_info.extend_from_slice(b"C:\\not-the-target.exe\0");
        let suffix_offset = u32::try_from(link_info.len()).unwrap();
        link_info[24..28].copy_from_slice(&suffix_offset.to_le_bytes());
        link_info.push(0);
        let unicode_offset = u32::try_from(link_info.len()).unwrap();
        link_info[28..32].copy_from_slice(&unicode_offset.to_le_bytes());
        for unit in target.encode_utf16().chain(std::iter::once(0)) {
            link_info.extend_from_slice(&unit.to_le_bytes());
        }
        let unicode_suffix = u32::try_from(link_info.len()).unwrap();
        link_info[32..36].copy_from_slice(&unicode_suffix.to_le_bytes());
        link_info.extend_from_slice(&0u16.to_le_bytes());
        let size = u32::try_from(link_info.len()).unwrap();
        link_info[..4].copy_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&link_info);
        fs::write(path, data).unwrap();
    }

    #[test]
    fn returns_only_plausible_in_bottle_programs() {
        let prefix = test_prefix();
        let app = prefix.join("drive_c/Program Files/Atlas/Atlas.exe");
        fake_pe(&app);
        fake_pe(&prefix.join("drive_c/Program Files/Atlas/Uninstall.exe"));
        fake_pe(&prefix.join("drive_c/Program Files/Internet Explorer/iexplore.exe"));
        fs::write(
            prefix.join("drive_c/Program Files/Atlas/not-a-program.exe"),
            b"plain text",
        )
        .unwrap();
        let outside = prefix
            .parent()
            .unwrap()
            .join(format!("outside-{}.exe", Uuid::new_v4()));
        fake_pe(&outside);
        symlink(
            &outside,
            prefix.join("drive_c/Program Files/Atlas/Outside.exe"),
        )
        .unwrap();

        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].name, "Atlas");
        assert_eq!(result.candidates[0].path, app.display().to_string());
        assert_eq!(result.candidates[0].source, "program-files");
        assert!(!result.truncated); // Deliberately skipped symlinks are not scan limits.
        fs::remove_dir_all(prefix).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn includes_x86_programs_and_prioritizes_product_exe() {
        let prefix = test_prefix();
        let base = prefix.join("drive_c/Program Files (x86)/Orion");
        fake_pe(&base.join("bin/Orion.exe"));
        fake_pe(&base.join("bin/auxiliary.exe"));
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.candidates[0].name, "Orion");
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn candidate_list_is_bounded_and_reports_truncation() {
        let prefix = test_prefix();
        let base = prefix.join("drive_c/Program Files/Many");
        for index in 0..(MAX_RETURNED + 1) {
            fake_pe(&base.join(format!("Game{index}.exe")));
        }
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), MAX_RETURNED);
        assert!(result.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn empty_uninitialized_prefix_is_a_valid_result() {
        let prefix = test_prefix();
        let result = discover(&prefix).unwrap();
        assert!(result.candidates.is_empty());
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn prefers_start_menu_shortcut_and_finds_apps_outside_program_files() {
        let prefix = test_prefix();
        let atlas = prefix.join("drive_c/Program Files/Atlas/Atlas.exe");
        let game = prefix.join("drive_c/Games/Nova/Nova.exe");
        fake_pe(&atlas);
        fake_pe(&game);
        write_shortcut(
            &prefix.join(
                "drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/Atlas/Play Atlas.lnk",
            ),
            "C:\\Program Files\\Atlas\\Atlas.exe",
            None,
        );
        write_shortcut(
            &prefix.join("drive_c/users/Public/Desktop/Atlas Desktop.lnk"),
            "C:\\Program Files\\Atlas\\Atlas.exe",
            None,
        );
        write_shortcut(
            &prefix.join("drive_c/users/Public/Desktop/Nova.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            None,
        );
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.candidates[0].name, "Play Atlas");
        assert_eq!(result.candidates[0].source, "start-menu");
        assert_eq!(result.candidates[0].path, atlas.display().to_string());
        assert_eq!(result.candidates[1].name, "Nova");
        assert_eq!(result.candidates[1].source, "desktop");
        assert_eq!(result.candidates[1].path, game.display().to_string());
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn shortcut_suppresses_helpers_only_in_its_program_files_product() {
        let prefix = test_prefix();
        let atlas = prefix.join("drive_c/Program Files/Atlas/Atlas.exe");
        let boreal = prefix.join("drive_c/Program Files/Boreal/Boreal.exe");
        let zenith = prefix.join("drive_c/Program Files/Zenith/Zenith.exe");
        fake_pe(&atlas);
        fake_pe(&prefix.join("drive_c/Program Files/Atlas/helper.exe"));
        fake_pe(&prefix.join("drive_c/Program Files/Atlas/bin/support.exe"));
        fake_pe(&prefix.join("drive_c/Program Files/Atlas/a/b/c/d/e/f/g/deep-helper.exe"));
        fake_pe(&boreal);
        fake_pe(&zenith);
        let menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        write_shortcut(
            &menu.join("Atlas.lnk"),
            "C:\\Program Files\\Atlas\\Atlas.exe",
            None,
        );
        // A shortcut that carries arguments is suggested as the whole command.
        write_shortcut(
            &menu.join("Zenith.lnk"),
            "C:\\Program Files\\Zenith\\Zenith.exe",
            Some("--mode special"),
        );

        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 3);
        assert_eq!(result.candidates[0].name, "Atlas");
        assert_eq!(result.candidates[0].source, "start-menu");
        assert_eq!(result.candidates[0].path, atlas.display().to_string());
        assert!(result.candidates[0].arguments.is_empty());
        assert!(result.candidates.iter().any(|item| {
            item.path == boreal.display().to_string() && item.source == "program-files"
        }));
        let zenith_command = result
            .candidates
            .iter()
            .find(|item| item.path == zenith.display().to_string())
            .unwrap();
        assert_eq!(zenith_command.source, "start-menu");
        assert_eq!(zenith_command.arguments, ["--mode", "special"]);
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn resolves_unicode_relative_target_within_bottle() {
        let prefix = test_prefix();
        let game = prefix.join("drive_c/Games/Café/Café.exe");
        fake_pe(&game);
        write_relative_shortcut(
            &prefix.join("drive_c/users/Ada/Desktop/Café.lnk"),
            "..\\..\\..\\Games\\Café\\Café.exe",
        );
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].name, "Café");
        assert_eq!(result.candidates[0].path, game.display().to_string());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn prefers_unicode_link_info_target_over_ansi_fallback() {
        let prefix = test_prefix();
        let game = prefix.join("drive_c/Games/Café.exe");
        fake_pe(&game);
        write_unicode_link_info_shortcut(
            &prefix.join("drive_c/users/Public/Desktop/Café.lnk"),
            "C:\\Games\\Café.exe",
        );
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].path, game.display().to_string());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn splits_a_windows_command_tail_the_way_windows_does() {
        let split = |input: &str| parse_windows_arguments(input);
        assert_eq!(split(""), Some(Vec::new()));
        assert_eq!(split("   \t "), Some(Vec::new()));
        assert_eq!(
            split("--mode special"),
            Some(vec!["--mode".into(), "special".into()])
        );
        // Runs of whitespace separate arguments, and tabs count as whitespace.
        assert_eq!(split("  a\t\t b  "), Some(vec!["a".into(), "b".into()]));
        // A quoted run keeps its spaces and loses its quotes.
        assert_eq!(
            split("\"C:\\Program Files\\Game\" --windowed"),
            Some(vec!["C:\\Program Files\\Game".into(), "--windowed".into()])
        );
        // Backslashes are literal unless they precede a quote: two become one
        // and an odd one escapes the quote instead of ending the quoted run.
        assert_eq!(split(r"a\\b"), Some(vec![r"a\\b".into()]));
        assert_eq!(split(r#""a\\" b"#), Some(vec![r"a\".into(), "b".into()]));
        assert_eq!(split(r#"a\"b"#), Some(vec!["a\"b".into()]));
        assert_eq!(split(r#""a\\\"b""#), Some(vec!["a\\\"b".into()]));
        // Inside quotes a doubled quote is one literal quote.
        assert_eq!(split(r#""say ""hi""""#), Some(vec!["say \"hi\"".into()]));
        // A quoted run can be glued to unquoted text.
        assert_eq!(split(r#"-p"a b"c"#), Some(vec!["-pa bc".into()]));

        // Commands this cannot reproduce exactly are refused outright.
        assert_eq!(split("--name \"unfinished"), None);
        assert_eq!(split("--line\nbreak"), None);
        assert_eq!(split("--nul\0byte"), None);
        assert_eq!(split(&"a".repeat(MAX_ARGUMENT_BYTES + 1)), None);
        assert_eq!(split(&"a".repeat(MAX_ARGUMENT_STRING_BYTES + 1)), None);
        assert_eq!(split(&"a ".repeat(MAX_ARGUMENTS + 1)), None);
    }

    #[test]
    fn every_candidate_serializes_its_arguments_even_when_empty() {
        // The app decodes this field. Omitting it when empty made a reader have
        // to tell "no arguments" apart from "an older core", and a decoder that
        // required the key refused every ordinary suggestion.
        let candidate = Candidate {
            name: "Nova".into(),
            path: "/bottle/Nova.exe".into(),
            source: "program-files",
            arguments: Vec::new(),
            working_directory: None,
        };
        let encoded = serde_json::to_value(&candidate).unwrap();
        assert_eq!(encoded.get("arguments"), Some(&serde_json::json!([])));
        let with_arguments = Candidate {
            arguments: vec!["-safe".into()],
            ..candidate
        };
        assert_eq!(
            serde_json::to_value(&with_arguments)
                .unwrap()
                .get("arguments"),
            Some(&serde_json::json!(["-safe"]))
        );
    }

    #[test]
    fn a_suggested_command_never_exceeds_what_the_core_will_save() {
        // Each argument is bounded, and so is their total, so discovery cannot
        // offer a command that saving would then refuse.
        let long = "a".repeat(MAX_ARGUMENT_BYTES);
        let within = (0..3).map(|_| long.clone()).collect::<Vec<_>>().join(" ");
        assert_eq!(parse_windows_arguments(&within).unwrap().len(), 3);
        let beyond = (0..5).map(|_| long.clone()).collect::<Vec<_>>().join(" ");
        assert!(parse_windows_arguments(&beyond).is_none());
    }

    #[test]
    fn two_shortcuts_to_one_program_stay_separate_commands() {
        let prefix = test_prefix();
        let game = prefix.join("drive_c/Games/Nova/Nova.exe");
        fake_pe(&game);
        let menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        write_shortcut(&menu.join("Nova.lnk"), "C:\\Games\\Nova\\Nova.exe", None);
        write_shortcut(
            &menu.join("Nova Safe Mode.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            Some("-safe"),
        );
        // The same command twice is still offered once.
        write_shortcut(
            &menu.join("Nova Again.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            Some("-safe"),
        );
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 2);
        let plain = result
            .candidates
            .iter()
            .find(|item| item.arguments.is_empty())
            .unwrap();
        assert_eq!(plain.path, game.display().to_string());
        let safe = result
            .candidates
            .iter()
            .find(|item| !item.arguments.is_empty())
            .unwrap();
        assert_eq!(safe.path, game.display().to_string());
        assert_eq!(safe.arguments, ["-safe"]);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn keeps_arguments_and_ignores_urls_uninstallers_and_out_of_bottle_targets() {
        let prefix = test_prefix();
        let game = prefix.join("drive_c/Games/Nova/Nova.exe");
        fake_pe(&game);
        let desktop = prefix.join("drive_c/users/Public/Desktop");
        // This one is a complete command and is now suggested with its arguments.
        write_shortcut(
            &desktop.join("With Args.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            Some("--game 123"),
        );
        // An unterminated quote is not a command this can reproduce exactly.
        write_shortcut(
            &desktop.join("Broken Quote.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            Some("--name \"unfinished"),
        );
        write_shortcut_with_working(
            &desktop.join("External Working Dir.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            Some("Z:\\external"),
            None,
        );
        write_shortcut(
            &desktop.join("Uninstall Nova.lnk"),
            "C:\\Games\\Nova\\Nova.exe",
            None,
        );
        write_shortcut(&desktop.join("Other Drive.lnk"), "D:\\Nova.exe", None);
        write_shortcut(
            &desktop.join("Host Path.lnk"),
            "/outside-bottle/Desktop/Nova.exe",
            None,
        );
        write_shortcut(
            &desktop.join("URL.lnk"),
            "https://example.org/Nova.exe",
            None,
        );
        write_relative_shortcut(&desktop.join("Escaped.lnk"), "..\\..\\..\\..\\Nova.exe");
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].name, "With Args");
        assert_eq!(result.candidates[0].path, game.display().to_string());
        assert_eq!(result.candidates[0].arguments, ["--game", "123"]);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn ignores_symlinked_shortcut_roots_and_target_ancestors() {
        let prefix = test_prefix();
        let outside = prefix
            .parent()
            .unwrap()
            .join(format!("external-{}", Uuid::new_v4()));
        fake_pe(&outside.join("External.exe"));
        symlink(&outside, prefix.join("drive_c/Games")).unwrap();
        let desktop = prefix.join("drive_c/users/Public/Desktop");
        write_shortcut(
            &desktop.join("External.lnk"),
            "C:\\Games\\External.exe",
            None,
        );
        let start_menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        fs::create_dir_all(start_menu.parent().unwrap()).unwrap();
        symlink(&outside, &start_menu).unwrap();
        let result = discover(&prefix).unwrap();
        assert!(result.candidates.is_empty());
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn shortcut_directories_are_preserved_and_distinguish_complete_commands() {
        let prefix = test_prefix();
        let exe = prefix.join("drive_c/Program Files/Atlas/Atlas.exe");
        fake_pe(&exe);
        fs::create_dir_all(prefix.join("drive_c/Games/Profile One")).unwrap();
        fs::create_dir_all(prefix.join("drive_c/Games/Profile Two")).unwrap();
        let desktop = prefix.join("drive_c/users/Public/Desktop");
        for (name, working) in [
            ("Atlas One", "C:\\Games\\Profile One"),
            ("Atlas Two", "C:\\Games\\Profile Two"),
            ("Atlas One Duplicate", "C:\\Games\\Profile One"),
            ("Atlas Default", ""),
            ("Atlas C Drive", "C:\\"),
        ] {
            write_shortcut_with_working(
                &desktop.join(format!("{name}.lnk")),
                "C:\\Program Files\\Atlas\\Atlas.exe",
                Some(working),
                Some("--profile \"two words\""),
            );
        }
        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 4);
        let directories: HashSet<_> = result
            .candidates
            .iter()
            .map(|candidate| candidate.working_directory.as_deref())
            .collect();
        assert_eq!(
            directories,
            HashSet::from([
                None,
                Some("drive_c"),
                Some("drive_c/Games/Profile One"),
                Some("drive_c/Games/Profile Two")
            ])
        );
        assert!(result.candidates.iter().all(|candidate| {
            candidate.path == exe.display().to_string()
                && candidate.arguments == ["--profile", "two words"]
        }));
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn unsafe_shortcut_directories_never_fall_back_to_bare_program_files_executables() {
        for working in [
            "C:\\missing",
            "D:\\Games",
            "Z:\\external",
            "\\\\server\\share",
            "C:\\Games\\..\\Atlas",
            "C:\\Games\\.\\Atlas",
            "..\\Games",
            "%PROGRAMFILES%\\Atlas",
            "C:\\Games\\Atlas. ",
            "C:\\Games\\NUL",
        ] {
            let prefix = test_prefix();
            fake_pe(&prefix.join("drive_c/Program Files/Atlas/Atlas.exe"));
            fs::create_dir_all(prefix.join("drive_c/Atlas")).unwrap();
            fs::create_dir_all(prefix.join("drive_c/Games/Atlas")).unwrap();
            write_shortcut_with_working(
                &prefix.join("drive_c/users/Public/Desktop/Atlas.lnk"),
                "C:\\Program Files\\Atlas\\Atlas.exe",
                Some(working),
                None,
            );
            assert!(
                discover(&prefix).unwrap().candidates.is_empty(),
                "{working}"
            );
            fs::remove_dir_all(prefix).unwrap();
        }
        let prefix = test_prefix();
        fake_pe(&prefix.join("drive_c/Program Files/Atlas/Atlas.exe"));
        fs::create_dir_all(prefix.join("drive_c/Real")).unwrap();
        symlink("Real", prefix.join("drive_c/Linked")).unwrap();
        write_shortcut_with_working(
            &prefix.join("drive_c/users/Public/Desktop/Atlas.lnk"),
            "C:\\Program Files\\Atlas\\Atlas.exe",
            Some("C:\\Linked"),
            None,
        );
        assert!(discover(&prefix).unwrap().candidates.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn host_linked_wine_desktop_does_not_report_truncation() {
        let prefix = test_prefix();
        let app = prefix.join("drive_c/Program Files/Atlas/Atlas.exe");
        fake_pe(&app);
        let host_desktop = prefix
            .parent()
            .unwrap()
            .join(format!("host-desktop-{}", Uuid::new_v4()));
        fs::create_dir_all(&host_desktop).unwrap();
        fake_pe(&host_desktop.join("Host.exe"));
        let user = prefix.join("drive_c/users/Ada");
        fs::create_dir_all(&user).unwrap();
        symlink(&host_desktop, user.join("Desktop")).unwrap();

        let result = discover(&prefix).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].path, app.display().to_string());
        assert!(!result.truncated);
        fs::remove_dir_all(prefix).unwrap();
        fs::remove_dir_all(host_desktop).unwrap();
    }

    #[test]
    fn rejects_malformed_and_oversized_shortcuts() {
        let prefix = test_prefix();
        let game = prefix.join("drive_c/Games/Nova.exe");
        fake_pe(&game);
        let desktop = prefix.join("drive_c/users/Public/Desktop");
        write_shortcut(&desktop.join("Broken.lnk"), "C:\\Games\\Nova.exe", None);
        let mut broken = fs::read(desktop.join("Broken.lnk")).unwrap();
        broken[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(desktop.join("Broken.lnk"), broken).unwrap();
        fs::write(
            desktop.join("Huge.lnk"),
            vec![0u8; MAX_SHORTCUT_SIZE as usize + 1],
        )
        .unwrap();
        let result = discover(&prefix).unwrap();
        assert!(result.candidates.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }
}
