//! Copy user-installed Windows applications and data out of a selected Wine prefix.
//! This never executes or modifies the source runtime. It deliberately leaves out
//! Windows/runtime files, native binaries, source metadata and all source symlinks.

use super::*;
use std::io::Write;

const MAX_REGISTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMPORT_ENTRIES: usize = 500_000;
const MAX_IMPORT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Serialize)]
pub(crate) struct CrossOverImportResult {
    pub(crate) bottle: Bottle,
    pub(crate) launchers: Vec<Launcher>,
    pub(crate) notes: Vec<String>,
    pub(crate) copied_files: usize,
    pub(crate) skipped_entries: usize,
    pub(crate) discovery_truncated: bool,
}

#[derive(Default)]
struct CopyState {
    copied_files: usize,
    skipped_entries: usize,
    examined: usize,
    bytes: u64,
    // Keep snapshots until the complete import is ready to publish, so a file
    // changed after its individual copy still invalidates the operation.
    snapshots: Vec<(PathBuf, fs::Metadata)>,
}

pub(crate) fn import(
    home: &Path,
    source: &Path,
    name: Option<&str>,
) -> Result<CrossOverImportResult, String> {
    import_with_guard(home, source, name, refuse_active_source)
}

fn import_with_guard(
    home: &Path,
    source: &Path,
    name: Option<&str>,
    mut idle: impl FnMut(&Path) -> Result<(), String>,
) -> Result<CrossOverImportResult, String> {
    cancellation::check()?;
    if !path_is_real_directory(source) {
        return Err("Choose an existing CrossOver bottle folder containing drive_c and system.reg. Archive files (.cxarchive) are not supported; export or extract the bottle to a folder first.".into());
    }
    ensure_independent_storage(source)?;
    // O_NOFOLLOW_ANY rejects a symlink in any source path component, including
    // one outside the selected folder. Canonicalization alone would hide it.
    let (_source_guard, source_snapshot) = open_stable_directory(source)?;
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    ensure_independent_storage(&source)?;
    if !path_is_real_directory(&source.join("drive_c"))
        || !path_is_real_file(&source.join("system.reg"))
    {
        return Err("The selected folder must contain a real drive_c directory and system.reg file at its root".into());
    }
    let default_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Imported bottle");
    let name = check_name(name.unwrap_or(default_name))?.to_owned();
    let projected_home = projected_path(home)?;
    if projected_home.starts_with(&source) || source.starts_with(&projected_home) {
        return Err("The source bottle and Caskrun storage must be separate folders".into());
    }
    idle(&source)?;
    assert_source_snapshot(&source, &source_snapshot)?;
    let bottle = Bottle {
        id: Uuid::new_v4().to_string(),
        name,
        created_at: now_seconds()?,
    };
    let root = storage_root(home, true)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let (_root_guard, root_snapshot) = open_stable_directory(&root)?;
    let stage = root.join(format!(".crossover-import-{}", Uuid::new_v4()));
    fs::create_dir(&stage)
        .map_err(|e| format!("Could not create the import staging folder: {e}"))?;
    let (_stage_guard, stage_snapshot) = open_stable_directory(&stage)?;
    let result = (|| {
        let prefix = stage.join("prefix");
        fs::create_dir(&prefix).map_err(|e| e.to_string())?;
        let mut state = CopyState::default();
        let mut stripped_sections = 0usize;
        for registry in ["system.reg", "user.reg", "userdef.reg"] {
            let from = source.join(registry);
            let before = match fs::symlink_metadata(&from) {
                Ok(metadata) => metadata,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && registry != "system.reg" =>
                {
                    continue;
                }
                Err(error) => return Err(format!("Cannot inspect {registry}: {error}")),
            };
            let bytes = read_registry(&from, &before)?;
            let (filtered, removed) = filter_registry(&bytes)?;
            stripped_sections += removed;
            write_new_file(&prefix.join(registry), filtered.as_bytes())?;
            state.copied_files += 1;
            state.snapshots.push((from, before));
        }
        let drive = prefix.join("drive_c");
        fs::create_dir(&drive).map_err(|e| e.to_string())?;
        copy_data_tree(&source.join("drive_c"), &drive, Path::new(""), &mut state)?;
        // Source dosdevices is never copied: external drives and host paths
        // must not become part of this bottle. This C: mapping is newly made.
        let devices = prefix.join("dosdevices");
        fs::create_dir(&devices).map_err(|e| e.to_string())?;
        symlink("../drive_c", devices.join("c:")).map_err(|e| e.to_string())?;
        idle(&source)?;
        assert_source_snapshot(&source, &source_snapshot)?;
        for (path, snapshot) in &state.snapshots {
            assert_source_snapshot(path, snapshot)?;
        }
        cancellation::check()?;
        // Use only the imported copy for discovery; none of these launcher
        // paths can retain a reference to the original prefix or its drives.
        let mut discovery = installed_apps::discover(&prefix)?;
        // Steam owns the manifest-to-launch relationship. Executable guesses
        // inside its libraries can bypass DRM, launchers or required arguments.
        discovery.candidates.retain(|candidate| {
            !Path::new(&candidate.path)
                .iter()
                .any(|part| part.to_string_lossy().eq_ignore_ascii_case("steamapps"))
        });
        for location in ["Program Files (x86)/Steam", "Program Files/Steam"] {
            for filename in ["Steam.exe", "steam.exe"] {
                let steam = prefix.join("drive_c").join(location).join(filename);
                if valid_steam_client(&steam)
                    && !discovery.candidates.iter().any(|candidate| {
                        candidate
                            .path
                            .eq_ignore_ascii_case(&steam.to_string_lossy())
                    })
                {
                    discovery.candidates.push(installed_apps::Candidate {
                        name: "Steam".into(),
                        path: steam.display().to_string(),
                        source: "program-files",
                    });
                }
            }
        }
        let final_dir = root.join(&bottle.id);
        let mut launchers = Vec::new();
        for candidate in discovery.candidates {
            let candidate_path = Path::new(&candidate.path);
            let relative = candidate_path
                .strip_prefix(&prefix)
                .map_err(|_| "App discovery returned a path outside the imported bottle")?;
            validate_relative_path(relative)?;
            launchers.push(Launcher {
                id: Uuid::new_v4().to_string(),
                name: {
                    let display: String = candidate
                        .name
                        .chars()
                        .filter(|character| !character.is_control())
                        .take(80)
                        .collect();
                    if display.trim().is_empty() {
                        "Imported app".into()
                    } else {
                        display.trim().into()
                    }
                },
                path: final_dir
                    .join("prefix")
                    .join(relative)
                    .display()
                    .to_string(),
                created_at: bottle.created_at,
                hidden_from_home: false,
                saved_arguments: Vec::new(),
            });
        }
        write_new_file(
            &stage.join("launchers.json"),
            &serde_json::to_vec_pretty(&launchers).map_err(|e| e.to_string())?,
        )?;
        write_new_file(
            &stage.join("bottle.json"),
            &serde_json::to_vec_pretty(&bottle).map_err(|e| e.to_string())?,
        )?;
        // No prefix-record.json is written. First launch must initialize this
        // prefix with Caskrun's independently selected runtime.
        let current_root = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
        if !same_identity(&root_snapshot, &current_root) {
            return Err("Caskrun bottle storage changed during import".into());
        }
        let (_verified_stage_guard, current_stage) = open_stable_directory(&stage)?;
        if !same_identity(&stage_snapshot, &current_stage) {
            return Err("The import staging folder changed during import".into());
        }
        let stage_name = stage
            .file_name()
            .unwrap()
            .to_str()
            .ok_or("Invalid staging folder name")?;
        // renameatx_np publishes atomically and refuses an existing ID. Both
        // sides use the opened storage directory rather than a mutable path.
        let from = CString::new(stage_name).map_err(|e| e.to_string())?;
        let to = CString::new(bottle.id.as_str()).map_err(|e| e.to_string())?;
        if unsafe {
            renameatx_np(
                _root_guard.as_raw_fd(),
                from.as_ptr(),
                _root_guard.as_raw_fd(),
                to.as_ptr(),
                RENAME_EXCL | RENAME_NOFOLLOW_ANY,
            )
        } != 0
        {
            return Err(format!(
                "Could not publish the imported bottle: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut notes = vec![
            "Copied installed Windows apps and data into a new Caskrun bottle. The source folder was only read.".into(),
            "CrossOver metadata, the Windows directory, Wine builtin programs, native engine binaries, all source symlinks and external drive mappings were excluded.".into(),
            "Caskrun must initialize its independent Wine runtime on first launch. Apps that need system DLLs, services, linked host folders or CrossOver-specific settings may require dependency repair or reinstallation; launch compatibility has not been validated.".into(),
        ];
        if state.skipped_entries > 0 {
            notes.push(format!("Skipped {} files, folders or links under drive_c; excluded folders include their contents.", state.skipped_entries));
        }
        if stripped_sections > 0 {
            notes.push(format!("Removed {stripped_sections} registry sections containing Wine/CrossOver configuration or host/external paths."));
        }
        if discovery.truncated {
            notes.push("App discovery reached its scan limit. Additional apps can be added manually from the imported bottle.".into());
        }
        if launchers.is_empty() {
            notes.push("No executable launchers were found. Add an app manually from the imported bottle if needed.".into());
        }
        Ok(CrossOverImportResult {
            bottle: bottle.clone(),
            launchers,
            notes,
            copied_files: state.copied_files,
            skipped_entries: state.skipped_entries,
            discovery_truncated: discovery.truncated,
        })
    })();
    if result.is_err() {
        // Only our random staging path is removed, never the source.
        // A replaced staging entry or storage root might belong to someone
        // else. Leave it alone rather than risk deleting source data.
        if open_stable_directory(&root)
            .is_ok_and(|(_, current)| same_identity(&root_snapshot, &current))
            && open_stable_directory(&stage)
                .is_ok_and(|(_, current)| same_identity(&stage_snapshot, &current))
        {
            let _ = fs::remove_dir_all(&stage);
        }
    }
    result
}

fn assert_source_snapshot(path: &Path, snapshot: &fs::Metadata) -> Result<(), String> {
    let current =
        fs::symlink_metadata(path).map_err(|e| format!("Source changed during import: {e}"))?;
    if !same_snapshot(snapshot, &current) {
        return Err(format!(
            "Source changed during import: {}. Close all Windows apps and try again.",
            path.display()
        ));
    }
    Ok(())
}

fn read_registry(path: &Path, before: &fs::Metadata) -> Result<Vec<u8>, String> {
    if !before.file_type().is_file() || before.len() > MAX_REGISTRY_BYTES {
        return Err("Bottle registry must be a regular file smaller than 64 MiB".into());
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| format!("Could not safely open the bottle registry: {e}"))?;
    if !same_snapshot(before, &input.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle registry changed while opening it".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut input)
        .take(MAX_REGISTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_REGISTRY_BYTES
        || !same_snapshot(before, &input.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Bottle registry changed while reading it".into());
    }
    assert_source_snapshot(path, before)?;
    Ok(bytes)
}

fn filter_registry(bytes: &[u8]) -> Result<(String, usize), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "The bottle registry must use Wine's UTF-8 text format")?;
    if !text.starts_with("WINE REGISTRY Version 2") {
        return Err("Unsupported Wine registry format".into());
    }
    if text.lines().any(|line| line.trim() == "#arch=win32") {
        return Err("This is a 32-bit-only prefix. Caskrun currently initializes 64-bit bottles; import requires a win64 bottle.".into());
    }
    let mut output = String::new();
    let mut section = String::new();
    let mut removed = 0usize;
    let flush = |section: &mut String, output: &mut String, removed: &mut usize| {
        let lower = section.to_ascii_lowercase();
        let unsafe_section = lower.contains("crossover")
            || lower.contains("codeweavers")
            || lower.contains("[software\\\\wine")
            || lower.contains("[software\\\\wow6432node\\\\wine")
            || lower.contains("/users/")
            || lower.contains("/applications/")
            || lower.contains("/volumes/")
            || lower.contains("z:\\\\")
            || lower.contains("\\\\??\\\\unix");
        if unsafe_section {
            *removed += 1;
        } else {
            output.push_str(section);
        }
        section.clear();
    };
    for line in text.split_inclusive('\n') {
        if line.starts_with('[') {
            flush(&mut section, &mut output, &mut removed);
        }
        section.push_str(line);
    }
    flush(&mut section, &mut output, &mut removed);
    Ok((output, removed))
}

fn excluded_path(relative: &Path) -> bool {
    let parts: Vec<_> = relative
        .iter()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .collect();
    if parts.first().is_some_and(|part| part == "windows") {
        return true;
    }
    if parts.len() >= 2
        && ["program files", "program files (x86)"].contains(&parts[0].as_str())
        && [
            "internet explorer",
            "windows media player",
            "windows nt",
            "windows accessories",
            "wine",
            "wine mono",
            "wine gecko",
        ]
        .contains(&parts[1].as_str())
    {
        return true;
    }
    parts.iter().any(|part| {
        part.contains("crossover")
            || part.contains("codeweavers")
            || [
                ".ds_store",
                "cxbottle.conf",
                "cxbottle",
                ".caskrun.lock",
                "wine",
                "wine64",
                "wineboot",
                "wineserver",
                "wine-preloader",
                "wine64-preloader",
                "libwine.dylib",
                "libwine.so",
                "cxinstallers",
                "cxassoc",
                "cxmenu",
                "wine-mono",
                "wine-gecko",
            ]
            .contains(&part.as_str())
    })
}

fn is_engine_binary(header: &[u8]) -> bool {
    if header.starts_with(b"\x7fELF") {
        return true;
    }
    if header.len() >= 4
        && [
            [0xfe, 0xed, 0xfa, 0xce],
            [0xce, 0xfa, 0xed, 0xfe],
            [0xfe, 0xed, 0xfa, 0xcf],
            [0xcf, 0xfa, 0xed, 0xfe],
            [0xca, 0xfe, 0xba, 0xbe],
            [0xbe, 0xba, 0xfe, 0xca],
            [0xca, 0xfe, 0xba, 0xbf],
            [0xbf, 0xba, 0xfe, 0xca],
        ]
        .iter()
        .any(|magic| header[..4] == magic[..])
    {
        return true;
    }
    if header.starts_with(b"MZ") {
        return header
            .windows(b"Wine builtin DLL".len())
            .any(|part| part == b"Wine builtin DLL")
            || header
                .windows(b"Wine placeholder DLL".len())
                .any(|part| part == b"Wine placeholder DLL");
    }
    false
}

fn copy_data_tree(
    source: &Path,
    destination: &Path,
    relative: &Path,
    state: &mut CopyState,
) -> Result<(), String> {
    cancellation::check()?;
    if relative.components().count() > MAX_DEPTH {
        return Err("The bottle exceeds the import directory depth limit".into());
    }
    let (_guard, snapshot) = open_stable_directory(source)?;
    state
        .snapshots
        .push((source.to_path_buf(), snapshot.clone()));
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        cancellation::check()?;
        state.examined += 1;
        if state.examined > MAX_IMPORT_ENTRIES {
            return Err("The bottle exceeds the 500,000 entry import limit".into());
        }
        assert_source_snapshot(source, &snapshot)?;
        let entry = entry.map_err(|e| e.to_string())?;
        let child = relative.join(entry.file_name());
        validate_relative_path(&child)?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let before = fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
        state.snapshots.push((from.clone(), before.clone()));
        if excluded_path(&child) || before.file_type().is_symlink() {
            state.skipped_entries += 1;
            continue;
        }
        if before.file_type().is_dir() {
            fs::create_dir(&to).map_err(|e| e.to_string())?;
            copy_data_tree(&from, &to, &child, state)?;
        } else if before.file_type().is_file() {
            state.bytes = state
                .bytes
                .checked_add(before.len())
                .ok_or("Import size overflow")?;
            if state.bytes > MAX_IMPORT_BYTES {
                return Err("The bottle exceeds the 1 TiB import size limit".into());
            }
            let mut input = OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(&from)
                .map_err(|e| format!("Could not safely read {}: {e}", child.display()))?;
            if !same_snapshot(&before, &input.metadata().map_err(|e| e.to_string())?) {
                return Err(format!("Source changed while opening {}", child.display()));
            }
            let mut header = [0u8; 4096];
            let count = input.read(&mut header).map_err(|e| e.to_string())?;
            if is_engine_binary(&header[..count]) {
                state.skipped_entries += 1;
                assert_source_snapshot(&from, &before)?;
                continue;
            }
            input.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(&to)
                .map_err(|e| e.to_string())?;
            let copied = std::io::copy(
                &mut Read::by_ref(&mut input).take(before.len().saturating_add(1)),
                &mut output,
            )
            .map_err(|e| format!("Could not copy {}: {e}", child.display()))?;
            if copied != before.len()
                || !same_snapshot(&before, &input.metadata().map_err(|e| e.to_string())?)
            {
                return Err(format!("Source changed while copying {}", child.display()));
            }
            assert_source_snapshot(&from, &before)?;
            state.copied_files += 1;
        } else {
            // Sockets, devices and FIFOs are not app data and must never be read.
            state.skipped_entries += 1;
        }
    }
    assert_source_snapshot(source, &snapshot)
}

// Require a real Windows x86/x64 PE before registering the conventional Steam
// client when shortcuts suppress its ordinary Program Files discovery.
fn valid_steam_client(path: &Path) -> bool {
    let Ok(before) = fs::symlink_metadata(path) else {
        return false;
    };
    if !before.file_type().is_file() || before.len() < 70 {
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
        .is_ok_and(|opened| same_snapshot(&before, &opened))
    {
        return false;
    }
    let mut dos = [0u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    let offset = u64::from(u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()));
    if !(64..=65536).contains(&offset)
        || offset + 6 > before.len()
        || file.seek(SeekFrom::Start(offset)).is_err()
    {
        return false;
    }
    let mut header = [0u8; 6];
    file.read_exact(&mut header).is_ok()
        && &header[..4] == b"PE\0\0"
        && [0x014c, 0x8664].contains(&u16::from_le_bytes([header[4], header[5]]))
        && file
            .metadata()
            .is_ok_and(|after| same_snapshot(&before, &after))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

fn is_windows_process(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    [
        "wine",
        "wine64",
        "wine-preloader",
        "wine64-preloader",
        "wineserver",
        "wineserver64",
    ]
    .contains(&base)
        || base.ends_with(".exe")
        || lower.contains("/crossover.app/contents/sharedsupport/crossover/")
}

fn refuse_active_source(source: &Path) -> Result<(), String> {
    // A global process check is intentionally conservative. Wine can retain
    // registry state in wineserver without open files in the prefix itself.
    let mut processes = Command::new("/bin/ps");
    processes.args(["-axo", "comm="]);
    let output = read_only_probe(&mut processes, Duration::from_secs(5))?;
    if !output.status.success() {
        return Err("Cannot inspect Windows processes. Close Wine/CrossOver apps and retry with process inspection available.".into());
    }
    if String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(is_windows_process)
    {
        return Err("Close all Wine and CrossOver Windows apps before importing a bottle. Import requires an idle source.".into());
    }
    let mut open_files = Command::new("/usr/sbin/lsof");
    open_files
        .args([
            "-nP",
            "-t",
            "-a",
            "-p",
            &format!("^{}", std::process::id()),
            "+d",
        ])
        .arg(source);
    let output = read_only_probe(&mut open_files, Duration::from_secs(12))?;
    if !output.stderr.is_empty() {
        return Err(
            "Cannot verify that the source bottle is idle: open-file inspection reported an error"
                .into(),
        );
    }
    match output.status.code() {
        Some(1) if output.stdout.is_empty() => Ok(()),
        Some(0) => Err("The source bottle has open files. Close its Windows apps and any other program using it, then retry.".into()),
        _ => Err("Cannot verify that the source bottle is idle".into()),
    }
}

// Drain both streams while waiting, with bounded memory and a deadline. These
// probes only inspect system state; neither can run or change a Wine runtime.
fn read_only_probe(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot inspect source process state: {e}"))?;
    fn collect(mut pipe: impl Read) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        pipe.by_ref()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Process inspection exceeded its output limit".into());
        }
        Ok(bytes)
    }
    let stdout = child.stdout.take().ok_or("Process output unavailable")?;
    let stderr = child.stderr.take().ok_or("Process errors unavailable")?;
    let stdout_reader = std::thread::spawn(move || collect(stdout));
    let stderr_reader = std::thread::spawn(move || collect(stderr));
    let status = match wait_child_bounded(&mut child, timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => Err("Source process inspection timed out".into()),
        Err(error) => Err(error),
    };
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Process output reader failed")??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Process error reader failed")??;
    Ok(std::process::Output {
        status: status?,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        home: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = env::current_dir()
                .unwrap()
                .join("target/test-data")
                .join(format!("crossover-import-{}", Uuid::new_v4()));
            let source = root.join("Original bottle");
            fs::create_dir_all(source.join("drive_c")).unwrap();
            fs::write(source.join("system.reg"), "WINE REGISTRY Version 2\n#arch=win64\n\n[Software\\\\Atlas]\n\"Installed\"=dword:00000001\n").unwrap();
            Self {
                home: root.join("Caskrun"),
                root,
                source,
            }
        }
        fn import(&self) -> Result<CrossOverImportResult, String> {
            import_with_guard(&self.home, &self.source, None, |_| Ok(()))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn pe(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0u8; 256];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn imports_apps_data_and_registry_without_source_mutation_or_runtime() {
        let fixture = Fixture::new();
        pe(&fixture.source.join("drive_c/Program Files/Atlas/Atlas.exe"));
        fs::create_dir_all(fixture.source.join("drive_c/users/Ada/Documents")).unwrap();
        fs::write(
            fixture.source.join("drive_c/users/Ada/Documents/save.bin"),
            b"user save",
        )
        .unwrap();
        fs::write(fixture.source.join("cxbottle.conf"), b"source metadata").unwrap();
        pe(&fixture.source.join("drive_c/windows/system32/runtime.dll"));
        let original_registry = fs::read(fixture.source.join("system.reg")).unwrap();
        let result = fixture.import().unwrap();
        let bottle = fixture.home.join("bottles").join(&result.bottle.id);
        assert_eq!(
            fs::read(bottle.join("prefix/drive_c/users/Ada/Documents/save.bin")).unwrap(),
            b"user save"
        );
        assert!(!bottle.join("prefix/drive_c/windows").exists());
        assert!(!bottle.join("prefix-record.json").exists());
        assert!(!bottle.join("cxbottle.conf").exists());
        assert_eq!(result.launchers.len(), 1);
        assert!(Path::new(&result.launchers[0].path).starts_with(&bottle));
        assert!(Path::new(&result.launchers[0].path).is_file());
        assert_eq!(
            fs::read(fixture.source.join("system.reg")).unwrap(),
            original_registry
        );
        assert!(
            fixture
                .source
                .join("drive_c/windows/system32/runtime.dll")
                .exists()
        );
        assert_eq!(
            fs::read_link(bottle.join("prefix/dosdevices/c:")).unwrap(),
            Path::new("../drive_c")
        );
        assert!(!bottle.join("prefix/dosdevices/z:").exists());
    }

    #[test]
    fn skips_external_links_native_binaries_and_wine_builtin_pe() {
        let fixture = Fixture::new();
        let outside = fixture.root.join("external");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.bin"), b"outside").unwrap();
        symlink(&outside, fixture.source.join("drive_c/External")).unwrap();
        fs::write(
            fixture.source.join("drive_c/engine.exe"),
            b"\xcf\xfa\xed\xfeengine",
        )
        .unwrap();
        let mut builtin = b"MZ".to_vec();
        builtin.extend_from_slice(b"Wine builtin DLL");
        fs::write(fixture.source.join("drive_c/builtin.dll"), builtin).unwrap();
        let result = fixture.import().unwrap();
        let drive = fixture
            .home
            .join("bottles")
            .join(result.bottle.id)
            .join("prefix/drive_c");
        assert!(!drive.join("External").exists());
        assert!(!drive.join("engine.exe").exists());
        assert!(!drive.join("builtin.dll").exists());
        assert_eq!(result.skipped_entries, 3);
        assert_eq!(fs::read(outside.join("secret.bin")).unwrap(), b"outside");
    }

    #[test]
    fn imported_steam_games_keep_manifests_and_launch_via_steam() {
        let fixture = Fixture::new();
        let steam = fixture.source.join("drive_c/Program Files (x86)/Steam");
        pe(&steam.join("Steam.exe"));
        pe(&steam.join("steamapps/common/Atlas/Atlas.exe"));
        fs::write(steam.join("steamapps/appmanifest_480.acf"),
            "\"AppState\" { \"appid\" \"480\" \"name\" \"Atlas\" \"StateFlags\" \"4\" \"installdir\" \"Atlas\" }").unwrap();
        let result = fixture.import().unwrap();
        assert_eq!(result.launchers.len(), 1);
        assert_eq!(result.launchers[0].name, "Steam");
        assert!(result.launchers[0].path.ends_with("Steam.exe"));
        let prefix = fixture
            .home
            .join("bottles")
            .join(result.bottle.id)
            .join("prefix");
        let games = steam_library::discover(&prefix).unwrap();
        assert_eq!(games.games.len(), 1);
        assert_eq!(games.games[0].app_id, 480);
        assert_eq!(games.games[0].name, "Atlas");
        assert_eq!(games.games[0].launch_arguments(), vec!["-applaunch", "480"]);
    }

    #[test]
    fn filters_runtime_and_host_registry_sections_but_keeps_app_settings() {
        let text = b"WINE REGISTRY Version 2\n#arch=win64\n\n[Software\\\\Atlas]\n\"Save\"=\"C:\\\\users\\\\Ada\\\\save\"\n\n[Software\\\\Wine\\\\DllOverrides]\n\"d3d11\"=\"native\"\n\n[Software\\\\CodeWeavers]\n\"Path\"=\"/Applications/CrossOver.app/engine\"\n\n[Software\\\\Folder]\n\"Desktop\"=\"Z:\\\\Users\\\\Ada\\\\Desktop\"\n";
        let (result, removed) = filter_registry(text).unwrap();
        assert!(result.contains("Atlas"));
        assert!(!result.contains("DllOverrides"));
        assert!(!result.contains("CodeWeavers"));
        assert!(!result.contains("Desktop"));
        assert_eq!(removed, 3);
    }

    #[test]
    fn refuses_active_source_before_creating_storage() {
        let fixture = Fixture::new();
        let result = import_with_guard(&fixture.home, &fixture.source, None, |_| {
            Err("active source".into())
        });
        assert_eq!(result.unwrap_err(), "active source");
        assert!(!fixture.home.exists());
    }

    #[test]
    fn aborts_and_removes_only_stage_when_source_changes_after_copy() {
        let fixture = Fixture::new();
        fs::write(fixture.source.join("drive_c/save.bin"), b"before").unwrap();
        let mut checks = 0;
        let result = import_with_guard(&fixture.home, &fixture.source, None, |_| {
            checks += 1;
            if checks == 2 {
                fs::write(
                    fixture.source.join("drive_c/save.bin"),
                    b"changed after copy",
                )
                .unwrap();
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("Source changed during import"));
        assert_eq!(
            fs::read_dir(fixture.home.join("bottles")).unwrap().count(),
            0
        );
        assert_eq!(
            fs::read(fixture.source.join("drive_c/save.bin")).unwrap(),
            b"changed after copy"
        );
    }

    #[test]
    fn failed_import_does_not_delete_a_source_renamed_over_its_staging_folder() {
        let fixture = Fixture::new();
        fs::write(fixture.source.join("drive_c/save.bin"), b"source data").unwrap();
        let mut checks = 0;
        let mut moved_source = None;
        let result = import_with_guard(&fixture.home, &fixture.source, None, |_| {
            checks += 1;
            if checks == 2 {
                let stage = fs::read_dir(fixture.home.join("bottles"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                fs::rename(&stage, fixture.root.join("original staging folder")).unwrap();
                fs::rename(&fixture.source, &stage).unwrap();
                moved_source = Some(stage);
                return Err("changed destination".into());
            }
            Ok(())
        });
        assert_eq!(result.unwrap_err(), "changed destination");
        assert_eq!(
            fs::read(moved_source.unwrap().join("drive_c/save.bin")).unwrap(),
            b"source data"
        );
    }

    #[test]
    fn rejects_symlink_source_nested_storage_and_win32_prefixes() {
        let fixture = Fixture::new();
        let link = fixture.root.join("link");
        symlink(&fixture.source, &link).unwrap();
        assert!(import_with_guard(&fixture.home, &link, None, |_| Ok(())).is_err());
        assert!(
            import_with_guard(
                &fixture.source.join("storage"),
                &fixture.source,
                None,
                |_| Ok(())
            )
            .is_err()
        );
        fs::write(
            fixture.source.join("system.reg"),
            "WINE REGISTRY Version 2\n#arch=win32\n",
        )
        .unwrap();
        assert!(fixture.import().unwrap_err().contains("32-bit-only"));
        assert!(!fixture.source.join("storage").exists());
    }

    #[test]
    fn conservative_process_classifier_covers_wine_server_and_windows_exes() {
        assert!(is_windows_process("/opt/wine/bin/wineserver"));
        assert!(is_windows_process("/opt/wine/bin/wine64-preloader"));
        assert!(is_windows_process("C:\\Games\\Atlas.exe"));
        assert!(!is_windows_process("/usr/bin/wine-notes"));
        assert!(!is_windows_process(
            "/Applications/Caskrun.app/Contents/MacOS/Caskrun"
        ));
    }
}
