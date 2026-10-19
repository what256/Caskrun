//! Bounded, read-only evidence for choosing a game's graphics backend.
//!
//! Adapted ideas from MaPC/MacadeKit's GraphicsProbe.swift and PEImage.swift,
//! revision 37bb046423dbb85c5cb98fee742868dccb96002d (Apache-2.0).
//! Copyright and license attribution: see THIRD_PARTY_NOTICES.md.
//! Caskrun's implementation uses descriptor-relative, no-follow filesystem
//! reads, explicit budgets, and conservative import evidence. It never starts
//! the inspected executable and never includes local paths in its result.

use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;
use std::path::{Component, Path};

const MAX_DEPTH: usize = 4;
const MAX_ENTRIES: usize = 4096;
const MAX_EXECUTABLES: u32 = 32;
const MAX_LIBRARIES: u32 = 64;
const MAX_DEPENDENCY_DEPTH: usize = 8;
const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const MAX_PE_OFFSET: u64 = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 96;
const MAX_IMPORTS: usize = 512;
const MAX_DLL_NAME: usize = 256;

/// Positive API evidence comes from parsed PE imports, including delay loads.
/// A missing API is unknown: dynamically loaded graphics cannot be ruled out.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct Probe {
    /// Direct3D 10, 10.1 or 11 imports, all served by DXMT.
    pub uses_d3d11: bool,
    pub uses_d3d12: bool,
    pub uses_vulkan: bool,
    pub uses_opengl: bool,
    pub uses_d3d9: bool,
    pub has_64_bit_executable: bool,
    pub has_x86_executable: bool,
    pub has_arm64_executable: bool,
    pub has_unknown_architecture: bool,
    pub scanned_executables: u32,
    pub scanned_libraries: u32,
    pub malformed_executables: u32,
    pub skipped_entries: u32,
    /// A work limit prevented a complete scan. Such evidence cannot select DXMT.
    pub truncated: bool,
    /// An eligible image or directory could not be read completely.
    pub incomplete: bool,
}

impl Probe {
    pub(crate) fn supports_dxmt(&self) -> bool {
        self.uses_d3d11
            && self.has_64_bit_executable
            && !self.has_x86_executable
            && !self.has_arm64_executable
            && !self.has_unknown_architecture
            && !self.uses_d3d12
            && !self.uses_vulkan
            && !self.uses_opengl
            && !self.uses_d3d9
            && self.malformed_executables == 0
            && !self.truncated
            && !self.incomplete
    }

    fn imports(&mut self, imports: &[String]) {
        for import in imports {
            match import.as_str() {
                "d3d10.dll" | "d3d10_1.dll" | "d3d10core.dll" | "d3d11.dll" => {
                    self.uses_d3d11 = true;
                }
                "d3d12.dll" | "d3d12core.dll" => self.uses_d3d12 = true,
                "vulkan-1.dll" => self.uses_vulkan = true,
                "opengl32.dll" => self.uses_opengl = true,
                "d3d9.dll" | "d3d8.dll" => self.uses_d3d9 = true,
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Machine {
    X86,
    X64,
    Arm64,
    Other,
}

struct Image {
    machine: Machine,
    imports: Vec<String>,
    is_library: bool,
}

#[derive(Clone, Copy)]
struct Section {
    rva: u64,
    raw_size: u64,
    raw_offset: u64,
}

#[derive(Debug, Eq, PartialEq)]
enum ReadError {
    Malformed,
    Io,
    Budget,
    Canceled,
}

struct Budget {
    entries: usize,
    bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            entries: MAX_ENTRIES,
            bytes: MAX_READ_BYTES,
        }
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ReadError> {
    let raw = bytes.get(at..at + 2).ok_or(ReadError::Malformed)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ReadError> {
    let raw = bytes.get(at..at + 4).ok_or(ReadError::Malformed)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_at(
    file: &File,
    length: u64,
    offset: u64,
    count: usize,
    budget: &mut Budget,
) -> Result<Vec<u8>, ReadError> {
    if crate::cancellation::is_requested() {
        return Err(ReadError::Canceled);
    }
    if offset
        .checked_add(count as u64)
        .is_none_or(|end| end > length)
    {
        return Err(ReadError::Malformed);
    }
    budget.bytes = budget.bytes.checked_sub(count).ok_or(ReadError::Budget)?;
    let mut bytes = vec![0; count];
    file.read_exact_at(&mut bytes, offset)
        .map_err(|_| ReadError::Io)?;
    Ok(bytes)
}

fn rva_offset(
    rva: u64,
    count: usize,
    headers_size: u64,
    sections: &[Section],
) -> Result<(u64, usize), ReadError> {
    let end = rva.checked_add(count as u64).ok_or(ReadError::Malformed)?;
    if rva < headers_size && end <= headers_size {
        return Ok((rva, (headers_size - rva) as usize));
    }
    let mut found = None;
    for section in sections {
        let section_end = section
            .rva
            .checked_add(section.raw_size)
            .ok_or(ReadError::Malformed)?;
        if rva >= section.rva && rva < section_end && end <= section_end {
            // Ambiguous overlapping sections must not manufacture import evidence.
            if found.is_some() {
                return Err(ReadError::Malformed);
            }
            found = Some((
                section
                    .raw_offset
                    .checked_add(rva - section.rva)
                    .ok_or(ReadError::Malformed)?,
                (section_end - rva) as usize,
            ));
        }
    }
    found.ok_or(ReadError::Malformed)
}

fn read_dll_name(
    file: &File,
    length: u64,
    rva: u64,
    headers_size: u64,
    sections: &[Section],
    budget: &mut Budget,
) -> Result<String, ReadError> {
    let (offset, available) = rva_offset(rva, 1, headers_size, sections)?;
    let bytes = read_at(file, length, offset, available.min(MAX_DLL_NAME), budget)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .filter(|end| *end != 0)
        .ok_or(ReadError::Malformed)?;
    let name = &bytes[..end];
    // Imports are module basenames. Never reinterpret an import as a host path.
    if !name
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-' | b'+'))
        || name == b"."
        || name == b".."
    {
        return Err(ReadError::Malformed);
    }
    String::from_utf8(name.to_ascii_lowercase()).map_err(|_| ReadError::Malformed)
}

fn parse_image(file: &File, budget: &mut Budget) -> Result<Image, ReadError> {
    let metadata = file.metadata().map_err(|_| ReadError::Io)?;
    if !metadata.is_file() {
        return Err(ReadError::Malformed);
    }
    let length = metadata.len();
    let dos = read_at(file, length, 0, 64, budget)?;
    if &dos[..2] != b"MZ" {
        return Err(ReadError::Malformed);
    }
    let pe_offset = u32_at(&dos, 0x3c)? as u64;
    if !(64..=MAX_PE_OFFSET).contains(&pe_offset) {
        return Err(ReadError::Malformed);
    }
    let coff = read_at(file, length, pe_offset, 24, budget)?;
    if &coff[..4] != b"PE\0\0" || u16_at(&coff, 22)? & 2 == 0 {
        return Err(ReadError::Malformed);
    }
    let machine = match u16_at(&coff, 4)? {
        0x014c => Machine::X86,
        0x8664 => Machine::X64,
        0xaa64 => Machine::Arm64,
        _ => Machine::Other,
    };
    let section_count = u16_at(&coff, 6)? as usize;
    let optional_size = u16_at(&coff, 20)? as usize;
    if section_count == 0 || section_count > MAX_SECTIONS || !(96..=4096).contains(&optional_size) {
        return Err(ReadError::Malformed);
    }
    let optional = read_at(file, length, pe_offset + 24, optional_size, budget)?;
    let (directory_start, count_offset, image_base) = match u16_at(&optional, 0)? {
        0x10b if machine != Machine::X64 && machine != Machine::Arm64 => {
            (96, 92, u32_at(&optional, 28)? as u64)
        }
        0x20b if machine != Machine::X86 => (
            112,
            108,
            u32_at(&optional, 24)? as u64 | ((u32_at(&optional, 28)? as u64) << 32),
        ),
        _ => return Err(ReadError::Malformed),
    };
    let directory_count = u32_at(&optional, count_offset)? as usize;
    if directory_count > 32
        || directory_start + directory_count * 8 > optional_size
        || directory_start > optional_size
    {
        return Err(ReadError::Malformed);
    }
    let headers_size = u32_at(&optional, 60)? as u64;
    let section_offset = pe_offset + 24 + optional_size as u64;
    let section_table_end = section_offset + (section_count * 40) as u64;
    if headers_size < section_table_end || headers_size > length {
        return Err(ReadError::Malformed);
    }
    let table = read_at(file, length, section_offset, section_count * 40, budget)?;
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let at = index * 40;
        let section = Section {
            rva: u32_at(&table, at + 12)? as u64,
            raw_size: u32_at(&table, at + 16)? as u64,
            raw_offset: u32_at(&table, at + 20)? as u64,
        };
        if section.raw_size != 0
            && (section.raw_offset < headers_size
                || section
                    .raw_offset
                    .checked_add(section.raw_size)
                    .is_none_or(|end| end > length))
        {
            return Err(ReadError::Malformed);
        }
        sections.push(section);
    }
    let mut imports = Vec::new();
    for (directory, descriptor_size, name_offset) in [(1, 20, 12), (13, 32, 4)] {
        if directory_count <= directory {
            continue;
        }
        let at = directory_start + directory * 8;
        let rva = u32_at(&optional, at)? as u64;
        let size = u32_at(&optional, at + 4)? as usize;
        if rva == 0 && size == 0 {
            continue;
        }
        if rva == 0 || size < descriptor_size {
            return Err(ReadError::Malformed);
        }
        // Validate the claimed region without allocating or reading its payload.
        rva_offset(rva, size, headers_size, &sections)?;
        let mut terminated = false;
        // The directory's declared size may also include thunk/name data.
        // Bound the descriptors actually examined, not the whole .idata region.
        for index in 0..(size / descriptor_size).min(MAX_IMPORTS) {
            let entry_rva = rva
                .checked_add((index * descriptor_size) as u64)
                .ok_or(ReadError::Malformed)?;
            let (offset, _) = rva_offset(entry_rva, descriptor_size, headers_size, &sections)?;
            let descriptor = read_at(file, length, offset, descriptor_size, budget)?;
            if descriptor.iter().all(|byte| *byte == 0) {
                terminated = true;
                break;
            }
            let mut name = u32_at(&descriptor, name_offset)? as u64;
            if name == 0 {
                return Err(ReadError::Malformed);
            }
            if directory == 13 {
                match u32_at(&descriptor, 0)? {
                    1 => {}
                    0 => name = name.checked_sub(image_base).ok_or(ReadError::Malformed)?,
                    _ => return Err(ReadError::Malformed),
                }
            }
            let name = read_dll_name(file, length, name, headers_size, &sections, budget)?;
            if !imports.contains(&name) {
                imports.push(name);
            }
        }
        if !terminated {
            return Err(if size / descriptor_size > MAX_IMPORTS {
                ReadError::Budget
            } else {
                ReadError::Malformed
            });
        }
    }
    Ok(Image {
        machine,
        imports,
        is_library: u16_at(&coff, 22)? & 0x2000 != 0,
    })
}

fn owned_openat(parent: RawFd, name: &CStr, directory: bool) -> std::io::Result<OwnedFd> {
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: name is NUL-terminated and the returned descriptor is uniquely owned.
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags) };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: openat returned a new descriptor owned by this function.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

/// Open every path component without following symlinks, including ancestors.
fn open_root(path: &Path) -> Result<(OwnedFd, Option<CString>, File), String> {
    let starting = if path.is_absolute() { c"/" } else { c"." };
    let mut parent = owned_openat(libc::AT_FDCWD, starting, true)
        .map_err(|_| "The graphics scan folder could not be opened".to_string())?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        crate::cancellation::check()?;
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => CString::new(name.as_bytes())
                .map_err(|_| "The graphics scan path is invalid".to_string())?,
            _ => return Err("The graphics scan path must not contain parent traversal".into()),
        };
        if components.peek().is_none() {
            let fd = owned_openat(parent.as_raw_fd(), &name, false).map_err(|_| {
                "The graphics scan target is unavailable or contains a symbolic link".to_string()
            })?;
            return Ok((parent, Some(name), File::from(fd)));
        }
        parent = owned_openat(parent.as_raw_fd(), &name, true).map_err(|_| {
            "The graphics scan path is unavailable or contains a symbolic link".to_string()
        })?;
    }
    let fd = owned_openat(parent.as_raw_fd(), c".", true)
        .map_err(|_| "The graphics scan folder could not be opened".to_string())?;
    Ok((parent, None, File::from(fd)))
}

fn directory_names(fd: RawFd, budget: &mut Budget) -> Result<Vec<CString>, ReadError> {
    let duplicate = owned_openat(fd, c".", true).map_err(|_| ReadError::Io)?;
    use std::os::fd::IntoRawFd;
    let raw = duplicate.into_raw_fd();
    // SAFETY: fdopendir takes ownership of a newly opened directory descriptor.
    let dir = unsafe { libc::fdopendir(raw) };
    if dir.is_null() {
        // SAFETY: fdopendir failed, so this descriptor is still ours to close.
        unsafe { libc::close(raw) };
        return Err(ReadError::Io);
    }
    let mut names = Vec::new();
    let result = loop {
        if crate::cancellation::is_requested() {
            break Err(ReadError::Canceled);
        }
        // Distinguish EOF from a directory read error without using stale errno.
        #[cfg(target_os = "macos")]
        // SAFETY: libc exposes the current thread's errno storage.
        unsafe {
            *libc::__error() = 0;
        }
        #[cfg(target_os = "linux")]
        // SAFETY: libc exposes the current thread's errno storage.
        unsafe {
            *libc::__errno_location() = 0;
        }
        // SAFETY: dir is live and accessed serially; copy its name before readdir again.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break if std::io::Error::last_os_error().raw_os_error() == Some(0) {
                Ok(())
            } else {
                Err(ReadError::Io)
            };
        }
        // SAFETY: d_name is a NUL-terminated filename provided by readdir.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        if budget.entries == 0 {
            break Err(ReadError::Budget);
        }
        budget.entries -= 1;
        names.push(name.to_owned());
    };
    // SAFETY: closedir owns and closes the duplicate, never the caller's descriptor.
    unsafe { libc::closedir(dir) };
    result?;
    names.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
    Ok(names)
}

fn folded_name(name: &CStr) -> Vec<u8> {
    name.to_bytes().to_ascii_lowercase()
}

fn skipped_folder(name: &[u8]) -> bool {
    name.starts_with(b".")
        || [
            "redist",
            "_redist",
            "_commonredist",
            "directx",
            "vcredist",
            "prereq",
            "prereqs",
            "prerequisites",
            "installer",
            "installers",
            // EA's installation metadata and Touchup.exe are not the game.
            "__installer",
            "support",
            "easyanticheat",
            "easyanticheat_eos",
            "battleye",
            "crashreport",
            "crashreporter",
            "thirdparty",
            "third-party",
            "__overlay",
            "uninstall",
        ]
        .iter()
        .any(|part| name == part.as_bytes())
}

fn skipped_executable(name: &[u8]) -> bool {
    name.starts_with(b".")
        || [
            "unins",
            "setup",
            "redist",
            "vcredist",
            "vc_redist",
            "crashreport",
            "crashpad_handler",
            "crashpadhandler",
            "crashhandler",
            "vc_",
            "dxsetup",
            "dotnet",
            "ue4prereq",
            "ueprereq",
            "easyanticheat",
            "battleye",
        ]
        .iter()
        .any(|part| name.starts_with(part.as_bytes()))
        || matches!(
            name,
            b"reporter.exe"
                | b"werfault.exe"
                | b"unitycrashhandler32.exe"
                | b"unitycrashhandler64.exe"
                | b"monocrashhandler.exe"
        )
}

fn count_error(probe: &mut Probe, error: ReadError, executable: bool) {
    match error {
        ReadError::Budget => probe.truncated = true,
        ReadError::Malformed if executable => probe.malformed_executables += 1,
        _ => probe.incomplete = true,
    }
}

fn inspect_executable(
    file: File,
    parent: RawFd,
    probe: &mut Probe,
    budget: &mut Budget,
) -> Result<(), String> {
    crate::cancellation::check()?;
    if probe.scanned_executables >= MAX_EXECUTABLES {
        probe.truncated = true;
        return Ok(());
    }
    probe.scanned_executables += 1;
    let image = match parse_image(&file, budget) {
        Ok(image) if !image.is_library => image,
        Ok(_) => {
            probe.malformed_executables += 1;
            return Ok(());
        }
        Err(ReadError::Canceled) => return Err(crate::cancellation::ERROR.into()),
        Err(error) => {
            count_error(probe, error, true);
            return Ok(());
        }
    };
    match image.machine {
        Machine::X64 => probe.has_64_bit_executable = true,
        Machine::X86 => probe.has_x86_executable = true,
        Machine::Arm64 => {
            probe.has_arm64_executable = true;
            probe.has_64_bit_executable = true;
        }
        Machine::Other => probe.has_unknown_architecture = true,
    }
    probe.imports(&image.imports);
    let mut queue: VecDeque<_> = image.imports.into_iter().map(|name| (name, 0)).collect();
    let mut visited = HashSet::new();
    let mut names = None;
    while let Some((name, depth)) = queue.pop_front() {
        crate::cancellation::check()?;
        if !name.ends_with(".dll") || !visited.insert(name.clone()) {
            continue;
        }
        let name_c = CString::new(name.as_bytes()).expect("validated import basename");
        let fd = match owned_openat(parent, &name_c, false) {
            Ok(fd) => Some(fd),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // PE names are case-insensitive, including on case-sensitive Mac volumes.
                if names.is_none() {
                    match directory_names(parent, budget) {
                        Ok(list) => names = Some(list),
                        Err(ReadError::Canceled) => {
                            return Err(crate::cancellation::ERROR.into());
                        }
                        Err(error) => {
                            count_error(probe, error, false);
                            break;
                        }
                    }
                }
                let matched = names.as_ref().and_then(|names| {
                    names
                        .iter()
                        .find(|candidate| folded_name(candidate) == name.as_bytes())
                });
                match matched {
                    Some(candidate) => match owned_openat(parent, candidate, false) {
                        Ok(fd) => Some(fd),
                        Err(_) => {
                            probe.incomplete = true;
                            None
                        }
                    },
                    None => None,
                }
            }
            // A local symlink, unreadable file, or device must not supply evidence.
            Err(_) => {
                probe.incomplete = true;
                None
            }
        };
        let Some(fd) = fd else {
            // Ordinary Windows system imports need not exist in the game folder.
            continue;
        };
        if depth >= MAX_DEPENDENCY_DEPTH || probe.scanned_libraries >= MAX_LIBRARIES {
            probe.truncated = true;
            break;
        }
        probe.scanned_libraries += 1;
        match parse_image(&File::from(fd), budget) {
            Ok(library) if library.is_library && library.machine == image.machine => {
                probe.imports(&library.imports);
                queue.extend(library.imports.into_iter().map(|name| (name, depth + 1)));
            }
            Ok(_) => probe.incomplete = true,
            Err(ReadError::Canceled) => return Err(crate::cancellation::ERROR.into()),
            Err(error) => count_error(probe, error, false),
        }
    }
    crate::cancellation::check()
}

/// Scan an installed game directory or one direct executable. All opens are
/// no-follow and anchored to held directory descriptors; no data is executed.
/// Direct EXEs inspect their own imports and imported local engine DLLs only.
pub(crate) fn scan(root: &Path) -> Result<Probe, String> {
    crate::cancellation::check()?;
    let (parent, leaf_name, target) = open_root(root)?;
    let metadata = target
        .metadata()
        .map_err(|_| "The graphics scan target could not be read".to_string())?;
    let mut probe = Probe::default();
    let mut budget = Budget::default();
    if metadata.is_file() {
        let name = leaf_name.ok_or("The graphics scan executable has no filename")?;
        let name = folded_name(&name);
        if !name.ends_with(b".exe") {
            return Err("The graphics scan needs a Windows executable or a game folder".into());
        }
        if skipped_executable(&name) {
            probe.skipped_entries += 1;
        } else {
            inspect_executable(target, parent.as_raw_fd(), &mut probe, &mut budget)?;
        }
        crate::cancellation::check()?;
        return Ok(probe);
    }
    if !metadata.is_dir() {
        return Err("The graphics scan needs a regular executable or a game folder".into());
    }
    let mut directories = VecDeque::from([(target, 0)]);
    while let Some((directory, depth)) = directories.pop_front() {
        crate::cancellation::check()?;
        let names = match directory_names(directory.as_raw_fd(), &mut budget) {
            Ok(names) => names,
            Err(ReadError::Canceled) => return Err(crate::cancellation::ERROR.into()),
            Err(error) => {
                count_error(&mut probe, error, false);
                if probe.truncated {
                    break;
                }
                continue;
            }
        };
        for name in names {
            crate::cancellation::check()?;
            let folded = folded_name(&name);
            // Check metadata through the opened descriptor, never path-following stat.
            let fd = match owned_openat(directory.as_raw_fd(), &name, false) {
                Ok(fd) => fd,
                Err(error) => {
                    probe.skipped_entries += 1;
                    // Symlinks are deliberately excluded. Other failures could
                    // conceal a game, so they cannot support automatic selection.
                    if error.raw_os_error() != Some(libc::ELOOP) {
                        probe.incomplete = true;
                    }
                    continue;
                }
            };
            let file = File::from(fd);
            let metadata = match file.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    probe.incomplete = true;
                    continue;
                }
            };
            if metadata.is_dir() {
                if skipped_folder(&folded) {
                    probe.skipped_entries += 1;
                } else if depth >= MAX_DEPTH {
                    probe.truncated = true;
                } else {
                    directories.push_back((file, depth + 1));
                }
            } else if metadata.is_file() && folded.ends_with(b".exe") {
                if skipped_executable(&folded) {
                    probe.skipped_entries += 1;
                } else {
                    inspect_executable(file, directory.as_raw_fd(), &mut probe, &mut budget)?;
                }
            }
            if probe.truncated {
                break;
            }
        }
        if probe.truncated {
            break;
        }
    }
    crate::cancellation::check()?;
    Ok(probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
            let path = parent.join(format!(
                "caskrun-graphics-probe-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, data: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, data).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Minimal, section-backed synthetic PE. No copyrighted executable fixtures.
    fn pe(machine: u16, imports: &[&str], delayed: &[&str], library: bool) -> Vec<u8> {
        let is64 = machine != 0x014c;
        let opt_size = if is64 { 240 } else { 224 };
        let dirs = 0x98 + if is64 { 112 } else { 96 };
        let mut bytes = vec![0; 4096];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 0x84, machine);
        put16(&mut bytes, 0x86, 1);
        put16(&mut bytes, 0x94, opt_size);
        put16(&mut bytes, 0x96, 2 | if library { 0x2000 } else { 0 });
        put16(&mut bytes, 0x98, if is64 { 0x20b } else { 0x10b });
        put32(&mut bytes, 0x98 + if is64 { 24 } else { 28 }, 0x400000);
        put32(&mut bytes, 0x98 + 60, 512);
        put32(&mut bytes, 0x98 + if is64 { 108 } else { 92 }, 16);
        let section = 0x98 + opt_size as usize;
        put32(&mut bytes, section + 8, 3584);
        put32(&mut bytes, section + 12, 0x1000);
        put32(&mut bytes, section + 16, 3584);
        put32(&mut bytes, section + 20, 512);
        let mut names_offset = 2048;
        for (directory, offset, descriptor_size, name_slot, values) in
            [(1, 512, 20, 12, imports), (13, 1024, 32, 4, delayed)]
        {
            if values.is_empty() {
                continue;
            }
            put32(
                &mut bytes,
                dirs + directory * 8,
                (offset - 512 + 0x1000) as u32,
            );
            put32(
                &mut bytes,
                dirs + directory * 8 + 4,
                ((values.len() + 1) * descriptor_size) as u32,
            );
            for (index, name) in values.iter().enumerate() {
                if directory == 13 {
                    put32(&mut bytes, offset + index * descriptor_size, 1);
                }
                put32(
                    &mut bytes,
                    offset + index * descriptor_size + name_slot,
                    (names_offset - 512 + 0x1000) as u32,
                );
                bytes[names_offset..names_offset + name.len()].copy_from_slice(name.as_bytes());
                names_offset += name.len() + 1;
            }
        }
        bytes
    }

    #[test]
    fn x64_d3d11_import_is_positive_evidence() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["D3D11.DLL"], &[], false));
        let probe = scan(&path).unwrap();
        assert!(probe.uses_d3d11);
        assert!(probe.has_64_bit_executable);
        assert!(probe.supports_dxmt());
        assert_eq!(probe.scanned_executables, 1);
    }

    #[test]
    fn delayed_d3d12_overrides_d3d11() {
        let f = Fixture::new();
        let path = f.write(
            "Game.exe",
            &pe(0x8664, &["d3d11.dll"], &["d3d12.dll"], false),
        );
        let probe = scan(&path).unwrap();
        assert!(probe.uses_d3d12);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn large_import_directory_size_does_not_require_reading_its_payload() {
        let f = Fixture::new();
        let mut bytes = pe(0x8664, &["d3d11.dll"], &[], false);
        bytes.resize(512 + 64 * 1024, 0);
        put32(&mut bytes, 0x98 + 240 + 8, 64 * 1024);
        put32(&mut bytes, 0x98 + 240 + 16, 64 * 1024);
        // Linkers can include lookup/name data in this size. A zero descriptor
        // ends traversal even when this advertised region is much larger.
        put32(&mut bytes, 0x98 + 112 + 12, 64 * 1024);
        let path = f.write("Game.exe", &bytes);
        assert!(scan(&path).unwrap().supports_dxmt());
    }

    #[test]
    fn legacy_delay_va_is_supported_without_underflow() {
        let f = Fixture::new();
        let mut bytes = pe(0x014c, &[], &["d3d12.dll"], false);
        put32(&mut bytes, 1024, 0);
        let rva = u32_at(&bytes, 1028).unwrap();
        put32(&mut bytes, 1028, rva + 0x400000);
        let path = f.write("Game.exe", &bytes);
        assert!(scan(&path).unwrap().uses_d3d12);
        put32(&mut bytes, 1028, 1);
        fs::write(path.clone(), bytes).unwrap();
        assert_eq!(scan(&path).unwrap().malformed_executables, 1);
    }

    #[test]
    fn other_apis_and_unknown_are_not_dxmt_evidence() {
        for (imports, expected) in [
            (vec!["vulkan-1.dll"], "vulkan"),
            (vec!["opengl32.dll"], "opengl"),
            (vec!["d3d9.dll"], "d3d9"),
            (vec!["kernel32.dll"], "unknown"),
        ] {
            let f = Fixture::new();
            let path = f.write("Game.exe", &pe(0x8664, &imports, &[], false));
            let probe = scan(&path).unwrap();
            assert!(!probe.supports_dxmt());
            assert_eq!(probe.uses_vulkan, expected == "vulkan");
            assert_eq!(probe.uses_opengl, expected == "opengl");
            assert_eq!(probe.uses_d3d9, expected == "d3d9");
        }
    }

    #[test]
    fn d3d10_and_10_1_share_dxmt_path() {
        for name in ["d3d10.dll", "d3d10_1.dll", "d3d10core.dll"] {
            let f = Fixture::new();
            let path = f.write("Game.exe", &pe(0x8664, &[name], &[], false));
            assert!(scan(&path).unwrap().supports_dxmt());
        }
    }

    #[test]
    fn x86_arm64_and_unknown_machines_are_not_dxmt_targets() {
        for machine in [0x014c, 0xaa64, 0xffff] {
            let f = Fixture::new();
            let path = f.write("Game.exe", &pe(machine, &["d3d11.dll"], &[], false));
            assert!(!scan(&path).unwrap().supports_dxmt());
        }
    }

    #[test]
    fn follows_only_imported_same_directory_engine_libraries() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["UnityPlayer.dll"], &[], false));
        f.write("UnityPlayer.dll", &pe(0x8664, &["engine.dll"], &[], true));
        f.write("engine.dll", &pe(0x8664, &["d3d11.dll"], &[], true));
        f.write("unrelated.dll", &pe(0x8664, &["d3d12.dll"], &[], true));
        let probe = scan(&path).unwrap();
        assert!(probe.supports_dxmt());
        assert_eq!(probe.scanned_libraries, 2);
        assert!(!probe.uses_d3d12);
    }

    #[test]
    fn dependency_cycles_are_bounded() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["a.dll"], &[], false));
        f.write("a.dll", &pe(0x8664, &["b.dll"], &[], true));
        f.write("b.dll", &pe(0x8664, &["a.dll", "d3d11.dll"], &[], true));
        let probe = scan(&path).unwrap();
        assert!(probe.supports_dxmt());
        assert_eq!(probe.scanned_libraries, 2);
    }

    #[test]
    fn skips_installers_crash_reporters_and_redist_directories() {
        let f = Fixture::new();
        f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        let dx12 = pe(0x8664, &["d3d12.dll"], &[], false);
        f.write("Setup.exe", &dx12);
        f.write("CrashReport.exe", &dx12);
        f.write("_CommonRedist/Runtime.exe", &dx12);
        let probe = scan(&f.0).unwrap();
        assert!(probe.supports_dxmt());
        assert_eq!(probe.scanned_executables, 1);
        assert_eq!(probe.skipped_entries, 3);
    }

    #[test]
    fn ea_installer_metadata_helpers_do_not_mask_the_game_or_suppress_a_root() {
        let f = Fixture::new();
        f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        f.write("__Installer/Touchup.exe", &pe(0x014c, &[], &[], false));
        let probe = scan(&f.0).unwrap();
        assert!(probe.supports_dxmt());
        assert!(!probe.has_x86_executable);
        assert_eq!(probe.scanned_executables, 1);
        assert_eq!(probe.skipped_entries, 1);

        // The exclusion applies only while walking helper subdirectories. An
        // explicitly declared game root of that name is still inspected.
        let root_probe = scan(&f.0.join("__Installer")).unwrap();
        assert!(root_probe.has_x86_executable);
        assert_eq!(root_probe.scanned_executables, 1);
        assert_eq!(root_probe.skipped_entries, 0);

        // Never broaden the helper name to a prefix that could hide a game.
        f.write(
            "__InstallerGame/Game.exe",
            &pe(0x8664, &["d3d12.dll"], &[], false),
        );
        let mixed = scan(&f.0).unwrap();
        assert!(mixed.uses_d3d12);
        assert!(!mixed.supports_dxmt());
        assert_eq!(mixed.scanned_executables, 2);
    }

    #[test]
    fn game_names_containing_helper_words_are_not_silently_excluded() {
        let f = Fixture::new();
        let path = f.write(
            "CrashBandicoot.exe",
            &pe(0x8664, &["d3d12.dll"], &[], false),
        );
        let probe = scan(&path).unwrap();
        assert!(probe.uses_d3d12);
        assert_eq!(probe.scanned_executables, 1);
        assert_eq!(probe.skipped_entries, 0);
    }

    #[test]
    fn unity_and_mono_crash_helpers_do_not_mask_x64_game_graphics() {
        let f = Fixture::new();
        f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        // A helper's architecture or imports must not be attributed to the game.
        f.write(
            "UnityCrashHandler32.exe",
            &pe(0x014c, &["d3d12.dll"], &[], false),
        );
        f.write(
            "UnityCrashHandler64.exe",
            &pe(0x8664, &["d3d12.dll"], &[], false),
        );
        f.write(
            "MonoCrashHandler.exe",
            &pe(0x014c, &["d3d12.dll"], &[], false),
        );
        let probe = scan(&f.0).unwrap();
        assert!(probe.supports_dxmt());
        assert!(!probe.has_x86_executable);
        assert!(!probe.uses_d3d12);
        assert_eq!(probe.scanned_executables, 1);
        assert_eq!(probe.skipped_entries, 3);

        // Only the exact known helpers are excluded, never similarly named games.
        f.write(
            "UnityCrashHandlerGame.exe",
            &pe(0x8664, &["d3d12.dll"], &[], false),
        );
        let probe = scan(&f.0).unwrap();
        assert!(probe.uses_d3d12);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn direct_executable_does_not_scan_sibling_executables() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        f.write("OtherGame.exe", &pe(0x8664, &["d3d12.dll"], &[], false));
        assert!(scan(&path).unwrap().supports_dxmt());
        assert!(!scan(&f.0).unwrap().supports_dxmt());
    }

    #[test]
    fn misleading_names_and_payload_strings_do_not_select_an_api() {
        let f = Fixture::new();
        let mut bytes = pe(0x8664, &["kernel32.dll"], &[], false);
        bytes.extend_from_slice(b"D3D11.dll D3D12CreateDevice");
        let path = f.write("DirectX11Game.exe", &bytes);
        f.write("dxil.dll", b"unrelated shipped file");
        let probe = scan(&path).unwrap();
        assert!(!probe.uses_d3d11);
        assert!(!probe.uses_d3d12);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn rejects_symlink_root_ancestors_and_imports() {
        let outside = Fixture::new();
        outside.write("Game.exe", &pe(0x8664, &["d3d12.dll"], &[], false));
        let f = Fixture::new();
        symlink(&outside.0, f.0.join("linked")).unwrap();
        symlink(outside.0.join("Game.exe"), f.0.join("Game.exe")).unwrap();
        assert!(scan(&f.0.join("linked/Game.exe")).is_err());
        assert!(scan(&f.0.join("Game.exe")).is_err());
        let probe = scan(&f.0).unwrap();
        assert_eq!(probe.scanned_executables, 0);
        assert!(!probe.uses_d3d12);
        let path = f.write(
            "RealGame.exe",
            &pe(0x8664, &["engine.dll", "d3d11.dll"], &[], false),
        );
        symlink(outside.0.join("Game.exe"), f.0.join("engine.dll")).unwrap();
        let probe = scan(&path).unwrap();
        assert!(probe.incomplete);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn malformed_import_tables_fail_closed() {
        for mutation in 0..5 {
            let f = Fixture::new();
            let mut bytes = pe(0x8664, &["d3d11.dll"], &[], false);
            match mutation {
                0 => put32(&mut bytes, 0x3c, u32::MAX),
                1 => put16(&mut bytes, 0x86, 0xffff),
                2 => put32(&mut bytes, 512 + 12, 0xfffffff0),
                3 => bytes[532..552].fill(1), // No zero terminator in the directory.
                _ => put32(&mut bytes, 0x98 + 112 + 12, 20), // Size excludes terminator.
            }
            let path = f.write("Game.exe", &bytes);
            let probe = scan(&path).unwrap();
            assert_eq!(probe.malformed_executables, 1, "mutation {mutation}");
            assert!(!probe.supports_dxmt());
        }
    }

    #[test]
    fn virtual_tail_cannot_map_into_other_file_bytes() {
        let f = Fixture::new();
        let mut bytes = pe(0x8664, &["d3d11.dll"], &[], false);
        let section = 0x98 + 240;
        put32(&mut bytes, section + 8, 8192);
        put32(&mut bytes, section + 16, 1024);
        // DLL name is beyond raw data despite being within the virtual section.
        let path = f.write("Game.exe", &bytes);
        assert_eq!(scan(&path).unwrap().malformed_executables, 1);
    }

    #[test]
    fn read_budget_stops_before_large_allocations() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        let file = File::open(path).unwrap();
        let mut budget = Budget {
            entries: 1,
            bytes: 63,
        };
        assert!(matches!(
            parse_image(&file, &mut budget),
            Err(ReadError::Budget)
        ));
    }

    #[test]
    fn cancellation_returns_no_partial_probe_even_before_a_target_is_opened() {
        let f = Fixture::new();
        let (request, _guard) = crate::cancellation::test_request();
        request.store(true, Ordering::Release);
        assert_eq!(
            scan(&f.0.join("not-present.exe")).unwrap_err(),
            crate::cancellation::ERROR
        );
    }

    #[test]
    fn directory_and_pe_reads_observe_cancellation_inside_a_scan() {
        let f = Fixture::new();
        let path = f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        let file = File::open(path).unwrap();
        let directory = File::open(&f.0).unwrap();
        let (request, _guard) = crate::cancellation::test_request();
        request.store(true, Ordering::Release);
        let mut budget = Budget::default();
        let remaining_bytes = budget.bytes;
        let remaining_entries = budget.entries;
        assert_eq!(
            read_at(&file, 4096, 0, 64, &mut budget).unwrap_err(),
            ReadError::Canceled
        );
        assert_eq!(
            directory_names(directory.as_raw_fd(), &mut budget).unwrap_err(),
            ReadError::Canceled
        );
        assert_eq!(budget.bytes, remaining_bytes);
        assert_eq!(budget.entries, remaining_entries);
    }

    #[test]
    fn scan_limits_disable_automatic_selection() {
        let f = Fixture::new();
        for index in 0..MAX_EXECUTABLES + 1 {
            f.write(
                &format!("Game{index:02}.exe"),
                &pe(0x8664, &["d3d11.dll"], &[], false),
            );
        }
        let probe = scan(&f.0).unwrap();
        assert!(probe.truncated);
        assert_eq!(probe.scanned_executables, MAX_EXECUTABLES);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn depth_limit_is_reported_not_silently_accepted() {
        let f = Fixture::new();
        f.write("Game.exe", &pe(0x8664, &["d3d11.dll"], &[], false));
        f.write(
            "a/b/c/d/e/HiddenGame.exe",
            &pe(0x8664, &["d3d12.dll"], &[], false),
        );
        let probe = scan(&f.0).unwrap();
        assert!(probe.truncated);
        assert!(!probe.supports_dxmt());
    }

    #[test]
    fn serialized_evidence_contains_no_filename_or_local_path() {
        let f = Fixture::new();
        let path = f.write(
            "PrivateGameName.exe",
            &pe(0x8664, &["d3d11.dll"], &[], false),
        );
        let output = serde_json::to_string(&scan(&path).unwrap()).unwrap();
        assert!(!output.contains("PrivateGameName"));
        assert!(!output.contains(f.0.to_str().unwrap()));
    }

    #[test]
    fn rejects_parent_traversal_and_non_executable_targets() {
        let f = Fixture::new();
        f.write("game.txt", b"not an executable");
        assert!(scan(&f.0.join("game.txt")).is_err());
        assert!(scan(&f.0.join("../game.exe")).is_err());
    }
}
