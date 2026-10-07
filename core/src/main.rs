mod app_icons;
mod battle_net_install;
mod bottle_control;
mod bottle_resolution;
mod bottle_xdg;
mod cancellation;
mod crossover_import;
mod epic_install;
mod gog_install;
mod installed_apps;
mod itch_install;
mod native_app_identity;
mod prefix_processes;
mod publisher_install;
mod rockstar_install;
mod runtime_identity;
mod runtime_migration;
mod runtime_probe;
mod shutdown;
mod steam_install;
mod steam_library;
mod wine_tools;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{CString, c_char};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn renameatx_np(
        from_fd: i32,
        from: *const c_char,
        to_fd: i32,
        to: *const c_char,
        flags: u32,
    ) -> i32;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Bottle {
    id: String,
    name: String,
    created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Launcher {
    id: String,
    name: String,
    path: String,
    created_at: u64,
    #[serde(default)]
    hidden_from_home: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    saved_arguments: Vec<String>,
}

#[derive(Serialize)]
struct Status {
    wine_path: Option<String>,
    bottle_count: usize,
}

#[derive(Serialize)]
struct Overview {
    status: Status,
    bottles: Vec<Bottle>,
    deleted_bottles: Vec<Bottle>,
    launchers: BTreeMap<String, Vec<Launcher>>,
    catalog_attempts: Vec<steam_install::CatalogAttempt>,
    catalog_attempts_error: Option<String>,
}

fn overview(home: &Path) -> Result<Overview, String> {
    let bottles = list(home)?;
    let deleted_bottles = list_deleted(home)?;
    let mut launchers = BTreeMap::new();
    let mut catalog_attempts = Vec::new();
    let mut catalog_attempts_error = None;
    for bottle in &bottles {
        launchers.insert(bottle.id.clone(), list_launchers(home, &bottle.id)?);
        match steam_install::catalog_attempt(home, &bottle.id) {
            Ok(Some(attempt)) => catalog_attempts.push(attempt),
            Ok(None) => {}
            Err(error) if catalog_attempts_error.is_none() => {
                catalog_attempts_error = Some(format!(
                    "Could not read setup state for {}: {error}",
                    bottle.name
                ));
            }
            Err(_) => {}
        }
    }
    Ok(Overview {
        status: Status {
            wine_path: wine_path().map(|path| path.display().to_string()),
            bottle_count: bottles.len(),
        },
        bottles,
        deleted_bottles,
        launchers,
        catalog_attempts,
        catalog_attempts_error,
    })
}

#[derive(Serialize)]
struct LaunchResult {
    pid: u32,
    log_path: String,
}

#[derive(Debug, Serialize)]
struct WaitLaunchResult {
    pid: u32,
    log_path: String,
    exit_code: Option<i32>,
    signal: Option<i32>,
    elapsed_ms: u64,
    timed_out: bool,
}

struct WaitLaunchOptions {
    quiet_msi: bool,
    timeout: Duration,
    arguments: Vec<String>,
}

const DEFAULT_LAUNCH_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_LAUNCH_TIMEOUT_SECONDS: u64 = 60 * 60;

#[derive(Serialize)]
struct PrefixResult {
    prefix_path: String,
    initialized: bool,
    log_path: Option<String>,
}

#[derive(Serialize)]
struct PrefixPathResult {
    path: String,
}

#[derive(Serialize, Deserialize)]
struct PrefixRecord {
    wine_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_fingerprint: Option<String>,
}

// A .caskrun-bottle backup is a directory containing manifest.json and payload/.
// The manifest is written last, so an interrupted export cannot be imported.
const BACKUP_FORMAT: &str = "caskrun-bottle";
const BACKUP_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BackupManifest {
    format: String,
    version: u32,
    bottle: Bottle,
    launchers: Vec<PortableLauncher>,
}

#[derive(Serialize, Deserialize)]
struct PortableLauncher {
    id: String,
    name: String,
    created_at: u64,
    #[serde(default)]
    hidden_from_home: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    saved_arguments: Vec<String>,
    path: PortableLauncherPath,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
enum PortableLauncherPath {
    PrefixRelative(String),
    ExternalAbsolute(String),
}

#[derive(Serialize)]
struct BackupResult {
    path: String,
    bottle: Bottle,
    external_launchers: usize,
}

#[derive(Serialize)]
struct ImportResult {
    bottle: Bottle,
    external_launchers: usize,
}

const LOG_TAIL_BYTES: u64 = 64 * 1024;
const MAX_PROCESS_RECORD_BYTES: u64 = 64 * 1024;
const MAX_BOTTLE_RECORD_BYTES: u64 = 64 * 1024;
const MAX_PREFIX_RECORD_BYTES: u64 = 64 * 1024;
const WINE_SERVER_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
// macOS fcntl.h: refuse symlinks anywhere in a path during file opens.
const O_NOFOLLOW_ANY: i32 = 0x2000_0000;
const O_DIRECTORY: i32 = 0x0010_0000;
// macOS sys/stdio.h: atomic no-overwrite rename, with symlinks refused.
const RENAME_EXCL: u32 = 0x0000_0004;
const RENAME_NOFOLLOW_ANY: u32 = 0x0000_0010;

#[derive(Serialize)]
struct LogEntry {
    name: String,
    size: u64,
    modified_at: u64,
}

#[derive(Serialize)]
struct LogContent {
    name: String,
    content: String,
    truncated: bool,
}

fn data_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CASKRUN_HOME") {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Ok(env::current_dir().map_err(|e| e.to_string())?.join(path))
        };
    }
    let home = env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Caskrun"))
}

fn wine_path() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("CASKRUN_WINE") {
        let path = PathBuf::from(explicit);
        return is_usable_wine(&path).then_some(path);
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join("wine"))
        .find(|path| is_usable_wine(path))
}

fn is_usable_wine(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let wineboot = parent.join("wineboot");
    is_executable(path)
        && is_independent_runtime(path)
        && is_executable(&wineboot)
        && is_independent_runtime(&wineboot)
}

fn is_independent_runtime(path: &Path) -> bool {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let normalized = resolved.to_string_lossy().to_lowercase();
    !normalized.contains("/crossover.app/") && !normalized.contains("/crossover-lab/")
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn check_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Name must contain 1–80 printable characters".into());
    }
    Ok(name)
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())
}

fn projected_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().map_err(|e| e.to_string())?.join(path)
    };
    let mut ancestor = absolute.as_path();
    while fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor
            .parent()
            .ok_or("Storage path has no existing ancestor")?;
    }
    let canonical = ancestor.canonicalize().map_err(|e| e.to_string())?;
    Ok(canonical.join(absolute.strip_prefix(ancestor).map_err(|e| e.to_string())?))
}

fn ensure_independent_storage(path: &Path) -> Result<(), String> {
    let projected = projected_path(path)?;
    let lower = projected.to_string_lossy().to_lowercase();
    if lower.contains("/crossover.app") || lower.contains("/crossover-lab") {
        return Err("Caskrun storage cannot be inside CrossOver".into());
    }
    Ok(())
}

fn storage_root(home: &Path, create: bool) -> Result<PathBuf, String> {
    ensure_independent_storage(home)?;
    if create {
        fs::create_dir_all(home).map_err(|e| e.to_string())?;
    }
    let root = home.join("bottles");
    if create && !root.exists() {
        fs::create_dir(&root).map_err(|e| e.to_string())?;
    }
    if root.exists()
        && !fs::symlink_metadata(&root)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_dir()
    {
        return Err("Bottle storage must be a real directory, not a symlink".into());
    }
    Ok(root)
}

fn bottle_dir(home: &Path, id: &str) -> Result<PathBuf, String> {
    if Uuid::parse_str(id).is_err() {
        return Err("Invalid bottle ID".into());
    }
    let dir = storage_root(home, false)?.join(id);
    if !fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir()) {
        return Err("Bottle not found".into());
    }
    read_bottle_record(&dir, id)?;
    Ok(dir)
}

fn deleted_root(home: &Path, create: bool) -> Result<Option<PathBuf>, String> {
    ensure_independent_storage(home)?;
    let root = home.join("deleted-bottles");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(root)),
        Ok(_) => Err("Deleted-bottle storage must be a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&root)
                .map_err(|e| format!("Could not create deleted-bottle storage: {e}"))?;
            Ok(Some(root))
        }
        Err(error) => Err(error.to_string()),
    }
}

fn read_bottle_record(dir: &Path, id: &str) -> Result<Bottle, String> {
    let path = dir.join("bottle.json");
    let before = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() || before.len() > MAX_BOTTLE_RECORD_BYTES {
        return Err("Bottle metadata must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|e| format!("Could not safely open bottle metadata: {e}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle metadata changed while opening it".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_BOTTLE_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BOTTLE_RECORD_BYTES
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Bottle metadata changed while reading it".into());
    }
    let bottle: Bottle =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid bottle metadata: {e}"))?;
    if bottle.id != id {
        return Err("Bottle ID does not match its directory".into());
    }
    Ok(bottle)
}

fn lock_bottle_directory(dir: &Path) -> Result<File, String> {
    lock_bottle_directory_with_cancel(dir, true)
}

fn lock_bottle_directory_with_cancel(dir: &Path, cancelable: bool) -> Result<File, String> {
    let path = dir.join(".caskrun.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| format!("Could not open bottle lock: {e}"))?;
    if cancelable && cancellation::is_active() {
        loop {
            cancellation::check()?;
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("Could not lock bottle: {error}")),
            }
        }
    } else {
        file.lock()
            .map_err(|e| format!("Could not lock bottle: {e}"))?;
    }
    Ok(file)
}

fn lock_bottle(home: &Path, id: &str) -> Result<File, String> {
    lock_bottle_directory(&bottle_dir(home, id)?)
}

fn process_alive(pid: u32) -> Result<bool, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err("Invalid tracked Wine process ID".into());
    }
    // kill(pid, 0) checks existence without sending a signal.
    if unsafe { kill(pid as i32, 0) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(3) => Ok(false), // ESRCH: no such process on macOS.
        Some(1) => Ok(true),  // EPERM: fail closed if the process still exists.
        _ => Err(format!("Could not check Wine process {pid}: {error}")),
    }
}

fn read_active_processes(dir: &Path) -> Result<Vec<u32>, String> {
    let path = dir.join("active-processes.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata)
            if metadata.file_type().is_file() && metadata.len() <= MAX_PROCESS_RECORD_BYTES => {}
        Ok(_) => return Err("Invalid Wine process record".into()),
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_PROCESS_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PROCESS_RECORD_BYTES {
        return Err("Wine process record is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid Wine process record: {e}"))
}

fn live_processes(dir: &Path) -> Result<Vec<u32>, String> {
    let saved = read_active_processes(dir)?;
    let mut live = Vec::new();
    for pid in &saved {
        if process_alive(*pid)? && !live.contains(pid) {
            live.push(*pid);
        }
    }
    if live != saved {
        save_json(&dir.join("active-processes.json"), &live)?;
    }
    Ok(live)
}

fn refuse_running_bottle(dir: &Path) -> Result<(), String> {
    if !live_processes(dir)?.is_empty() {
        return Err("Close Windows apps in this bottle before changing it".into());
    }
    Ok(())
}

fn recorded_runtime_for_prefix(
    dir: &Path,
    selected_wine: Option<&Path>,
) -> Result<Option<PrefixRecord>, String> {
    // A runtime record can survive registry damage or partial initialization,
    // so it must still trigger a Wine-server check even without system.reg.
    let registry = dir.join("prefix/system.reg");
    let has_registry = match fs::symlink_metadata(&registry) {
        Ok(metadata) if metadata.file_type().is_file() => true,
        Ok(_) => return Err("Bottle registry must be a regular file before moving it".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("Could not inspect bottle registry: {error}")),
    };

    let record_path = dir.join("prefix-record.json");
    let record_metadata = match fs::symlink_metadata(&record_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Could not inspect bottle runtime record: {error}")),
    };
    if let Some(before) = record_metadata {
        if !before.file_type().is_file() || before.len() > MAX_PREFIX_RECORD_BYTES {
            return Err("Bottle runtime record must be a bounded regular file".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&record_path)
            .map_err(|e| format!("Could not safely open bottle runtime record: {e}"))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("Bottle runtime record changed while opening it".into());
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_PREFIX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PREFIX_RECORD_BYTES
            || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        {
            return Err("Bottle runtime record changed while reading it".into());
        }
        let record: PrefixRecord = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid bottle runtime record: {e}"))?;
        return Ok(Some(record));
    }

    if !has_registry {
        return Ok(None);
    }
    // Imported initialized prefixes have no runtime record. Check them using
    // the independently selected Wine runtime.
    let selected = selected_wine.ok_or(
        "Choose an independent Wine runtime to check this initialized bottle before moving it",
    )?;
    Ok(Some(PrefixRecord {
        wine_path: selected.display().to_string(),
        runtime_identity: None,
        runtime_fingerprint: None,
    }))
}

enum VerifiedRuntime {
    PathPinned,
    Bundled(runtime_identity::BundleAttestation),
}

fn verify_runtime_record(
    record: &PrefixRecord,
    selected_wine: &Path,
) -> Result<Option<VerifiedRuntime>, String> {
    if let Some(expected) = &record.runtime_identity {
        let selected = selected_wine
            .canonicalize()
            .map_err(|_| "The selected Wine runtime is unavailable")?;
        if Path::new(&record.wine_path) == selected
            && let Some(saved_fingerprint) = &record.runtime_fingerprint
            && let Some(metadata) =
                runtime_identity::bundled_runtime_metadata_fingerprint(selected_wine)?
            && runtime_identity::bind_record_fingerprint(expected, &metadata) == *saved_fingerprint
        {
            return Ok(Some(VerifiedRuntime::Bundled(
                runtime_identity::BundleAttestation {
                    identity: expected.clone(),
                    metadata_fingerprint: metadata,
                },
            )));
        }
        // A moved engine, old/unbound record, or changed metadata needs proof
        // from the complete package bytes. The binding detects one-field
        // record edits; a same-user attacker can still forge both fields.
        let Some(attestation) = runtime_identity::bundled_runtime_attestation(selected_wine)?
        else {
            return Ok(None);
        };
        return Ok(
            (attestation.identity == *expected).then_some(VerifiedRuntime::Bundled(attestation))
        );
    }
    // Older records and custom Wine installations did not carry a package
    // digest. They remain usable only at the exact original runtime path.
    let recorded = Path::new(&record.wine_path)
        .canonicalize()
        .map_err(|_| "The Wine runtime recorded for this bottle is unavailable")?;
    let selected = selected_wine
        .canonicalize()
        .map_err(|_| "The selected Wine runtime is unavailable")?;
    Ok((recorded == selected).then_some(VerifiedRuntime::PathPinned))
}

fn runtime_record_matches(record: &PrefixRecord, selected_wine: &Path) -> Result<bool, String> {
    Ok(verify_runtime_record(record, selected_wine)?.is_some())
}

fn refuse_live_wine_server(
    dir: &Path,
    timeout: Duration,
    selected_wine: Option<&Path>,
) -> Result<(), String> {
    let prefix = dir.join("prefix");
    let (_prefix_guard, _) = open_stable_directory(&prefix)?;
    let Some(record) = recorded_runtime_for_prefix(dir, selected_wine)? else {
        return Ok(());
    };
    // A Windows app can edit files in its bottle. Do not execute a path read
    // only from the bottle record; require the user's selected runtime to match.
    let wine = selected_wine.ok_or(
        "Choose this bottle's independent Wine runtime before moving it so Caskrun can verify it is idle",
    )?;
    if !is_usable_wine(wine) {
        return Err("Selected Wine runtime is unavailable or not independent".into());
    }
    if !runtime_record_matches(&record, wine)? {
        return Err(
            "Select the Wine runtime used to initialize this bottle before moving it".into(),
        );
    }
    let wineserver = wine
        .parent()
        .ok_or("Bottle Wine path has no parent directory")?
        .join("wineserver");
    if !wine.is_absolute() || !is_executable(&wineserver) || !is_independent_runtime(&wineserver) {
        return Err("Cannot verify that this bottle is idle. Restore its independent Wine runtime before moving it".into());
    }
    // -w waits for this WINEPREFIX's wineserver to exit. It also observes Wine
    // descendants that outlive the direct process launched by Caskrun. This
    // probe never asks wineserver to stop applications.
    let xdg = bottle_xdg::for_bottle(dir)?;
    let mut probe = Command::new(&wineserver);
    xdg.apply(&mut probe);
    let mut probe = probe
        .arg("-w")
        .env("WINEPREFIX", &prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not check this bottle's Wine server: {e}"))?;
    let (status, timed_out) = wait_child_bounded_status(&mut probe, timeout)?;
    if timed_out {
        return Err(
            "Close all Windows apps in this bottle and wait for Wine to finish before moving it"
                .into(),
        );
    }
    if !status.success() {
        return Err("Could not verify that this bottle's Wine server is idle. Check its independent Wine runtime before moving it".into());
    }
    Ok(())
}

fn real_subdirectory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|e| format!("Could not create {name}: {e}"))?;
        }
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(format!("Bottle {name} must be a real directory")),
    }
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(format!("Bottle {name} changed while opening it"));
    }
    let _ = open_stable_directory(&path)?;
    Ok(path)
}

fn list(home: &Path) -> Result<Vec<Bottle>, String> {
    let dir = storage_root(home, false)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut bottles = Vec::new();
    for item in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let id = item.file_name();
        let id = id.to_string_lossy();
        if let Ok(path) = bottle_dir(home, &id) {
            bottles.push(read_bottle_record(&path, &id)?);
        }
    }
    bottles.sort_by_key(|item| item.name.to_lowercase());
    Ok(bottles)
}

fn list_deleted(home: &Path) -> Result<Vec<Bottle>, String> {
    let Some(root) = deleted_root(home, false)? else {
        return Ok(Vec::new());
    };
    let (_root_guard, _) = open_stable_directory(&root)?;
    let mut bottles = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if Uuid::parse_str(&id).is_err() {
            continue;
        }
        if let Ok((_guard, _)) = open_stable_directory(&entry.path())
            && let Ok(bottle) = read_bottle_record(&entry.path(), &id)
        {
            bottles.push(bottle);
        }
    }
    bottles.sort_by_key(|item| item.name.to_lowercase());
    Ok(bottles)
}

fn move_bottle_no_replace(from_root: &File, to_root: &File, id: &str) -> Result<(), String> {
    let name = CString::new(id).map_err(|_| "Invalid bottle ID")?;
    let result = unsafe {
        renameatx_np(
            from_root.as_raw_fd(),
            name.as_ptr(),
            to_root.as_raw_fd(),
            name.as_ptr(),
            RENAME_EXCL | RENAME_NOFOLLOW_ANY,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return Err("A bottle with this ID already exists at the destination".into());
    }
    Err(format!(
        "Could not move bottle without replacing another entry: {error}"
    ))
}

fn delete_bottle(home: &Path, id: &str) -> Result<Bottle, String> {
    let selected_wine = wine_path();
    delete_bottle_with_timeout(
        home,
        id,
        WINE_SERVER_CHECK_TIMEOUT,
        selected_wine.as_deref(),
    )
}

fn delete_bottle_with_timeout(
    home: &Path,
    id: &str,
    timeout: Duration,
    selected_wine: Option<&Path>,
) -> Result<Bottle, String> {
    let _lock = lock_bottle(home, id)?;
    let source = bottle_dir(home, id)?;
    refuse_running_bottle(&source)?;
    refuse_live_wine_server(&source, timeout, selected_wine)?;
    let (source_guard, source_snapshot) = open_stable_directory(&source)?;
    let bottle = read_bottle_record(&source, id)?;
    let active_root = storage_root(home, false)?;
    let (active_guard, _) = open_stable_directory(&active_root)?;
    let deleted_root = deleted_root(home, true)?.ok_or("Deleted-bottle storage is unavailable")?;
    let (deleted_guard, _) = open_stable_directory(&deleted_root)?;
    assert_stable_path(&source, &source_snapshot)?;
    move_bottle_no_replace(&active_guard, &deleted_guard, id)?;
    let moved = fs::symlink_metadata(deleted_root.join(id)).map_err(|e| e.to_string())?;
    if !same_identity(&source_guard.metadata().map_err(|e| e.to_string())?, &moved) {
        return Err("Bottle source changed during deletion; inspect deleted-bottle storage".into());
    }
    Ok(bottle)
}

fn restore_deleted_bottle(home: &Path, id: &str) -> Result<Bottle, String> {
    if Uuid::parse_str(id).is_err() {
        return Err("Invalid bottle ID".into());
    }
    let deleted_root = deleted_root(home, false)?.ok_or("Deleted bottle not found")?;
    let (deleted_guard, _) = open_stable_directory(&deleted_root)?;
    let source = deleted_root.join(id);
    let _lock = lock_bottle_directory(&source)?;
    refuse_running_bottle(&source)?;
    let (source_guard, source_snapshot) = open_stable_directory(&source)?;
    let bottle = read_bottle_record(&source, id)?;
    let active_root = storage_root(home, true)?;
    let (active_guard, _) = open_stable_directory(&active_root)?;
    assert_stable_path(&source, &source_snapshot)?;
    move_bottle_no_replace(&deleted_guard, &active_guard, id)?;
    let moved = fs::symlink_metadata(active_root.join(id)).map_err(|e| e.to_string())?;
    if !same_identity(&source_guard.metadata().map_err(|e| e.to_string())?, &moved) {
        return Err("Deleted-bottle source changed during restore; inspect bottle storage".into());
    }
    Ok(bottle)
}

fn create(home: &Path, name: &str) -> Result<Bottle, String> {
    let name = check_name(name)?;
    let bottle = Bottle {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs(),
    };
    let dir = storage_root(home, true)?.join(&bottle.id);
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    fs::create_dir(dir.join("prefix")).map_err(|e| e.to_string())?;
    save_json(&dir.join("bottle.json"), &bottle)?;
    Ok(bottle)
}

fn rename_bottle(home: &Path, id: &str, name: &str) -> Result<Bottle, String> {
    let _lock = lock_bottle(home, id)?;
    let dir = bottle_dir(home, id)?;
    let mut bottle: Bottle =
        serde_json::from_slice(&fs::read(dir.join("bottle.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    bottle.name = check_name(name)?.to_owned();
    save_json(&dir.join("bottle.json"), &bottle)?;
    Ok(bottle)
}

fn now_seconds() -> Result<u64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs())
}

fn path_is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

fn path_is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("Backup contains an unsafe relative path".into());
    }
    Ok(())
}

fn validate_backup_symlink(relative: &Path, target: &Path) -> Result<(), String> {
    // Wine's standard Z: drive deliberately maps the host filesystem. Preserve only
    // that well-known absolute link; never follow it while copying a bottle.
    if relative == Path::new("prefix/dosdevices/z:") && target == Path::new("/") {
        return Ok(());
    }
    if target.is_absolute() {
        return Err(format!(
            "Backup contains an absolute symlink at {}",
            relative.display()
        ));
    }
    let mut depth = relative.parent().map_or(0, |p| p.components().count());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                if name == "z:" {
                    return Err("Backup symlink traverses Wine's host drive".into());
                }
                depth += 1;
            }
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err("Backup symlink leaves the bottle".into()),
        }
    }
    Ok(())
}

fn same_snapshot(first: &fs::Metadata, second: &fs::Metadata) -> bool {
    first.dev() == second.dev()
        && first.ino() == second.ino()
        && first.len() == second.len()
        && first.mtime() == second.mtime()
        && first.mtime_nsec() == second.mtime_nsec()
        && first.ctime() == second.ctime()
        && first.ctime_nsec() == second.ctime_nsec()
}

fn same_identity(first: &fs::Metadata, second: &fs::Metadata) -> bool {
    first.file_type().is_dir()
        && second.file_type().is_dir()
        && first.dev() == second.dev()
        && first.ino() == second.ino()
}

fn open_stable_directory(path: &Path) -> Result<(File, fs::Metadata), String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !before.file_type().is_dir() {
        return Err(format!(
            "Directory changed or is a symlink: {}",
            path.display()
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | O_DIRECTORY)
        .open(path)
        .map_err(|e| format!("Could not safely open directory {}: {e}", path.display()))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err(format!(
            "Directory changed while opening it: {}",
            path.display()
        ));
    }
    Ok((file, before))
}

fn assert_stable_path(path: &Path, snapshot: &fs::Metadata) -> Result<(), String> {
    let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !same_snapshot(snapshot, &current) {
        return Err(format!("Source changed during backup: {}", path.display()));
    }
    Ok(())
}

fn copy_stable_file(from: &Path, to: &Path, before: &fs::Metadata) -> Result<(), String> {
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(from)
        .map_err(|e| format!("Could not safely open {}: {e}", from.display()))?;
    let opened = input.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || !same_snapshot(before, &opened) {
        return Err(format!(
            "Source changed while opening it: {}",
            from.display()
        ));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(to)
        .map_err(|e| format!("Could not safely create {}: {e}", to.display()))?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    output
        .set_permissions(before.permissions())
        .map_err(|e| e.to_string())?;
    if !same_snapshot(before, &input.metadata().map_err(|e| e.to_string())?) {
        return Err(format!(
            "Source changed while copying it: {}",
            from.display()
        ));
    }
    assert_stable_path(from, before)
}

fn copy_tree_contents(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    relative: &Path,
    excluded_at_root: &[&str],
) -> Result<(), String> {
    let (_directory_guard, directory_snapshot) = open_stable_directory(source)?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        assert_stable_path(source, &directory_snapshot)?;
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if relative.as_os_str().is_empty()
            && excluded_at_root.iter().any(|excluded| name == *excluded)
        {
            continue;
        }
        let child_relative = relative.join(&name);
        validate_relative_path(&child_relative)?;
        let from = entry.path();
        let to = destination.join(&name);
        let before = fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
        let kind = before.file_type();
        if kind.is_symlink() {
            if relative.as_os_str().is_empty() {
                return Err("Bottle payload cannot contain a top-level symlink".into());
            }
            let target = fs::read_link(&from).map_err(|e| e.to_string())?;
            assert_stable_path(&from, &before)?;
            let target = bottle_xdg::portable_desktop_link(source_root, &child_relative, &target)
                .unwrap_or(target);
            validate_backup_symlink(&child_relative, &target)?;
            symlink(target, &to).map_err(|e| e.to_string())?;
        } else if kind.is_dir() {
            let (_guard, opened) = open_stable_directory(&from)?;
            if !same_snapshot(&before, &opened) {
                return Err(format!(
                    "Directory changed during backup: {}",
                    from.display()
                ));
            }
            fs::create_dir(&to).map_err(|e| e.to_string())?;
            copy_tree_contents(source_root, &from, &to, &child_relative, &[])?;
            fs::set_permissions(&to, before.permissions()).map_err(|e| e.to_string())?;
        } else if kind.is_file() {
            copy_stable_file(&from, &to, &before)?;
        } else {
            return Err(format!(
                "Bottle contains an unsupported file type: {}",
                child_relative.display()
            ));
        }
    }
    assert_stable_path(source, &directory_snapshot)?;
    Ok(())
}

fn portable_launchers(source: &Path, launchers: Vec<Launcher>) -> Vec<PortableLauncher> {
    let source_prefix = source
        .join("prefix")
        .canonicalize()
        .unwrap_or_else(|_| source.join("prefix"));
    launchers
        .into_iter()
        .map(|launcher| {
            let path = Path::new(&launcher.path);
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let portable_path = match resolved.strip_prefix(&source_prefix) {
                Ok(relative) if validate_relative_path(relative).is_ok() => {
                    PortableLauncherPath::PrefixRelative(relative.display().to_string())
                }
                _ => PortableLauncherPath::ExternalAbsolute(launcher.path),
            };
            PortableLauncher {
                id: launcher.id,
                name: launcher.name,
                created_at: launcher.created_at,
                hidden_from_home: launcher.hidden_from_home,
                saved_arguments: launcher.saved_arguments,
                path: portable_path,
            }
        })
        .collect()
}

fn restore_launchers(
    destination: &Path,
    portable: &[PortableLauncher],
) -> Result<Vec<Launcher>, String> {
    portable
        .iter()
        .map(|item| {
            check_name(&item.name)?;
            validate_app_arguments(&item.saved_arguments)?;
            let path = match &item.path {
                PortableLauncherPath::PrefixRelative(value) => {
                    let relative = Path::new(value);
                    validate_relative_path(relative)?;
                    if relative.components().any(
                        |component| matches!(component, Component::Normal(name) if name == "z:"),
                    ) {
                        return Err("Backup launcher cannot use Wine's host drive".into());
                    }
                    destination.join("prefix").join(relative)
                }
                PortableLauncherPath::ExternalAbsolute(value) => {
                    let path = PathBuf::from(value);
                    if !path.is_absolute() {
                        return Err("Backup contains a non-absolute external launcher".into());
                    }
                    path
                }
            };
            if path
                .extension()
                .and_then(|part| part.to_str())
                .map(str::to_lowercase)
                .as_deref()
                != Some("exe")
            {
                return Err("Backup launcher must point to a .exe program".into());
            }
            Ok(Launcher {
                id: Uuid::new_v4().to_string(),
                name: item.name.clone(),
                path: path.display().to_string(),
                created_at: item.created_at,
                hidden_from_home: item.hidden_from_home,
                saved_arguments: item.saved_arguments.clone(),
            })
        })
        .collect()
}

fn external_launcher_count(launchers: &[PortableLauncher]) -> usize {
    launchers
        .iter()
        .filter(|item| matches!(item.path, PortableLauncherPath::ExternalAbsolute(_)))
        .count()
}

fn duplicate_bottle(home: &Path, id: &str, name: Option<&str>) -> Result<Bottle, String> {
    let selected_wine = wine_path();
    duplicate_bottle_with_runtime(home, id, name, selected_wine.as_deref())
}

fn duplicate_bottle_with_runtime(
    home: &Path,
    id: &str,
    name: Option<&str>,
    selected_wine: Option<&Path>,
) -> Result<Bottle, String> {
    let _lock = lock_bottle(home, id)?;
    let source = bottle_dir(home, id)?;
    refuse_running_bottle(&source)?;
    refuse_live_wine_server(&source, WINE_SERVER_CHECK_TIMEOUT, selected_wine)?;
    let xdg = bottle_xdg::for_bottle(&source)?;
    bottle_xdg::migrate_host_desktop_links(&source.join("prefix"), &xdg)?;
    if !path_is_real_directory(&source.join("prefix")) {
        return Err("Bottle prefix must be a real directory".into());
    }
    let original: Bottle =
        serde_json::from_slice(&fs::read(source.join("bottle.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let default_name = format!(
        "{} copy",
        original.name.chars().take(75).collect::<String>()
    );
    let name = check_name(name.unwrap_or(&default_name))?;
    let launchers = portable_launchers(&source, list_launchers(home, id)?);
    let bottle = Bottle {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        created_at: now_seconds()?,
    };
    let destination = storage_root(home, true)?.join(&bottle.id);
    fs::create_dir(&destination).map_err(|e| e.to_string())?;
    let result = (|| {
        copy_tree_contents(
            &source,
            &source,
            &destination,
            Path::new(""),
            &[
                "bottle.json",
                "launchers.json",
                ".caskrun.lock",
                "active-processes.json",
                "native-apps",
            ],
        )?;
        bottle_xdg::for_bottle(&destination)?;
        let launchers = restore_launchers(&destination, &launchers)?;
        if !launchers.is_empty() {
            save_json(&destination.join("launchers.json"), &launchers)?;
        }
        save_json(&destination.join("bottle.json"), &bottle)?;
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result.map(|()| bottle)
}

fn export_bottle(home: &Path, id: &str, output: &Path) -> Result<BackupResult, String> {
    let selected_wine = wine_path();
    export_bottle_with_runtime(home, id, output, selected_wine.as_deref())
}

fn export_bottle_with_runtime(
    home: &Path,
    id: &str,
    output: &Path,
    selected_wine: Option<&Path>,
) -> Result<BackupResult, String> {
    let _lock = lock_bottle(home, id)?;
    let source = bottle_dir(home, id)?;
    refuse_running_bottle(&source)?;
    refuse_live_wine_server(&source, WINE_SERVER_CHECK_TIMEOUT, selected_wine)?;
    let xdg = bottle_xdg::for_bottle(&source)?;
    bottle_xdg::migrate_host_desktop_links(&source.join("prefix"), &xdg)?;
    if !path_is_real_directory(&source.join("prefix")) {
        return Err("Bottle prefix must be a real directory".into());
    }
    if output.extension().and_then(|s| s.to_str()) != Some("caskrun-bottle") {
        return Err("Backup destination must end in .caskrun-bottle".into());
    }
    if fs::symlink_metadata(output).is_ok() {
        return Err("Backup destination already exists".into());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    ensure_independent_storage(&parent)?;
    let source_resolved = source.canonicalize().map_err(|e| e.to_string())?;
    let bottles_resolved = storage_root(home, false)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if parent.starts_with(&source_resolved) || parent.starts_with(&bottles_resolved) {
        return Err("Store backups outside the bottle storage directory".into());
    }
    let bottle: Bottle =
        serde_json::from_slice(&fs::read(source.join("bottle.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let launchers = portable_launchers(&source, list_launchers(home, id)?);
    let external_launchers = external_launcher_count(&launchers);
    let manifest = BackupManifest {
        format: BACKUP_FORMAT.into(),
        version: BACKUP_VERSION,
        bottle: bottle.clone(),
        launchers,
    };
    fs::create_dir(output).map_err(|e| e.to_string())?;
    let result = (|| {
        let payload = output.join("payload");
        fs::create_dir(&payload).map_err(|e| e.to_string())?;
        copy_tree_contents(
            &source,
            &source,
            &payload,
            Path::new(""),
            &[
                "bottle.json",
                "launchers.json",
                "prefix-record.json",
                ".caskrun.lock",
                "active-processes.json",
                "native-apps",
            ],
        )?;
        save_json(&output.join("manifest.json"), &manifest)?;
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(output);
    }
    result.map(|()| BackupResult {
        path: output.display().to_string(),
        bottle,
        external_launchers,
    })
}

fn import_bottle(home: &Path, source: &Path, name: Option<&str>) -> Result<ImportResult, String> {
    if !path_is_real_directory(source) {
        return Err("Choose a .caskrun-bottle backup directory".into());
    }
    let metadata = source.join("manifest.json");
    let manifest_size = fs::symlink_metadata(&metadata).map_err(|e| e.to_string())?;
    if !manifest_size.file_type().is_file() || manifest_size.len() > 4 * 1024 * 1024 {
        return Err("Backup manifest is missing or too large".into());
    }
    let manifest: BackupManifest =
        serde_json::from_slice(&fs::read(metadata).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid backup manifest: {e}"))?;
    if manifest.format != BACKUP_FORMAT || manifest.version != BACKUP_VERSION {
        return Err("Unsupported Caskrun backup format".into());
    }
    if Uuid::parse_str(&manifest.bottle.id).is_err() {
        return Err("Backup has an invalid bottle ID".into());
    }
    let name = check_name(name.unwrap_or(&manifest.bottle.name))?;
    let payload = source.join("payload");
    if !path_is_real_directory(&payload) || !path_is_real_directory(&payload.join("prefix")) {
        return Err("Backup payload is missing its prefix".into());
    }
    for reserved in [
        "bottle.json",
        "launchers.json",
        "prefix-record.json",
        ".caskrun.lock",
        "active-processes.json",
        "native-apps",
    ] {
        if fs::symlink_metadata(payload.join(reserved)).is_ok() {
            return Err(format!(
                "Backup payload contains reserved metadata: {reserved}"
            ));
        }
    }
    let bottle = Bottle {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        created_at: now_seconds()?,
    };
    let destination = storage_root(home, true)?.join(&bottle.id);
    // Importing a backup from inside the destination would recursively copy it.
    if destination.starts_with(source.canonicalize().map_err(|e| e.to_string())?) {
        return Err("Backup cannot contain the destination bottle".into());
    }
    fs::create_dir(&destination).map_err(|e| e.to_string())?;
    let result = (|| {
        copy_tree_contents(&payload, &payload, &destination, Path::new(""), &[])?;
        bottle_xdg::for_bottle(&destination)?;
        let launchers = restore_launchers(&destination, &manifest.launchers)?;
        if !launchers.is_empty() {
            save_json(&destination.join("launchers.json"), &launchers)?;
        }
        // The Wine path from another machine is intentionally not restored. On first
        // launch wineboot will inspect/update the imported prefix with the chosen runtime.
        save_json(&destination.join("bottle.json"), &bottle)?;
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result.map(|()| ImportResult {
        bottle,
        external_launchers: external_launcher_count(&manifest.launchers),
    })
}

fn list_launchers(home: &Path, id: &str) -> Result<Vec<Launcher>, String> {
    let path = bottle_dir(home, id)?.join("launchers.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn discover_apps(home: &Path, id: &str) -> Result<installed_apps::DiscoveryResult, String> {
    let prefix = bottle_dir(home, id)?.join("prefix");
    let (_guard, _) = open_stable_directory(&prefix)?;
    installed_apps::discover(&prefix)
}

fn is_log_name(name: &str) -> bool {
    let canonical_uuid =
        |value: &str| Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value);
    if let Some(id) = name
        .strip_prefix("wineboot-")
        .and_then(|value| value.strip_suffix(".log"))
    {
        return canonical_uuid(id);
    }
    if let Some(id) = name
        .strip_prefix("resolution-")
        .and_then(|value| value.strip_suffix(".log"))
    {
        return canonical_uuid(id);
    }
    if let Some(rest) = name
        .strip_prefix("launch-")
        .and_then(|value| value.strip_suffix(".log"))
        && let Some((stamp, id)) = rest.split_once('-')
    {
        return !stamp.is_empty()
            && stamp.bytes().all(|byte| byte.is_ascii_digit())
            && stamp.parse::<u64>().is_ok()
            && canonical_uuid(id);
    }
    false
}

fn logs_dir(home: &Path, id: &str) -> Result<Option<PathBuf>, String> {
    let bottle = bottle_dir(home, id)?;
    let path = bottle.join("logs");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
        Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(path)),
        Ok(_) => Err("Bottle logs must be a real directory".into()),
    }
}

fn list_logs(home: &Path, id: &str) -> Result<Vec<LogEntry>, String> {
    let Some(directory) = logs_dir(home, id)? else {
        return Ok(Vec::new());
    };
    let mut logs = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_log_name(&name) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_secs());
        logs.push(LogEntry {
            name,
            size: metadata.len(),
            modified_at,
        });
    }
    logs.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(logs)
}

fn read_log(home: &Path, id: &str, name: &str) -> Result<LogContent, String> {
    if !is_log_name(name) {
        return Err("Invalid log filename".into());
    }
    let directory = logs_dir(home, id)?.ok_or("Log not found")?;
    let path = directory.join(name);
    let before = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() {
        return Err("Log must be a regular file, not a symlink".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err("Log changed while opening it".into());
    }
    let start = opened.len().saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(LOG_TAIL_BYTES as usize);
    file.take(LOG_TAIL_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(LogContent {
        name: name.to_owned(),
        content: String::from_utf8_lossy(&bytes).into_owned(),
        truncated: start > 0,
    })
}

fn add_launcher(home: &Path, id: &str, name: &str, exe: &Path) -> Result<Launcher, String> {
    let _lock = lock_bottle(home, id)?;
    let name = check_name(name)?;
    let path = exe.canonicalize().map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err("Choose an existing Windows program".into());
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "exe" {
        return Err("Choose a .exe program. MSI files are installers, not saved apps.".into());
    }
    let mut launchers = list_launchers(home, id)?;
    let launcher = Launcher {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        path: path.display().to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs(),
        hidden_from_home: false,
        saved_arguments: Vec::new(),
    };
    launchers.push(launcher.clone());
    save_json(&bottle_dir(home, id)?.join("launchers.json"), &launchers)?;
    Ok(launcher)
}

fn set_launcher_visibility(
    home: &Path,
    bottle_id: &str,
    launcher_id: &str,
    hidden: bool,
) -> Result<Launcher, String> {
    let _lock = lock_bottle(home, bottle_id)?;
    let mut launchers = list_launchers(home, bottle_id)?;
    let launcher = launchers
        .iter_mut()
        .find(|item| item.id == launcher_id)
        .ok_or("Launcher not found in this bottle")?;
    launcher.hidden_from_home = hidden;
    let updated = launcher.clone();
    save_json(
        &bottle_dir(home, bottle_id)?.join("launchers.json"),
        &launchers,
    )?;
    Ok(updated)
}

fn set_launcher_arguments(
    home: &Path,
    bottle_id: &str,
    launcher_id: &str,
    arguments: &[String],
) -> Result<Launcher, String> {
    validate_app_arguments(arguments)?;
    let _lock = lock_bottle(home, bottle_id)?;
    let mut launchers = list_launchers(home, bottle_id)?;
    let launcher = launchers
        .iter_mut()
        .find(|item| item.id == launcher_id)
        .ok_or("Launcher not found in this bottle")?;
    launcher.saved_arguments = arguments.to_vec();
    let updated = launcher.clone();
    save_json(
        &bottle_dir(home, bottle_id)?.join("launchers.json"),
        &launchers,
    )?;
    Ok(updated)
}

fn steam_games(home: &Path, bottle_id: &str) -> Result<steam_library::SteamLibrary, String> {
    let prefix = bottle_dir(home, bottle_id)?.join("prefix");
    steam_library::discover(&prefix)
}

fn steam_game_launcher(
    home: &Path,
    bottle_id: &str,
    app_id: u32,
) -> Result<(Launcher, steam_library::SteamGame), String> {
    let prefix = bottle_dir(home, bottle_id)?.join("prefix");
    let game = steam_library::launch_target(&prefix, app_id)?;
    let steam_path = Path::new(&game.steam_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let launcher = list_launchers(home, bottle_id)?
        .into_iter()
        .find(|launcher| {
            Path::new(&launcher.path).canonicalize().ok().as_ref() == Some(&steam_path)
        })
        .ok_or("Save the Steam app in this bottle before opening its games")?;
    Ok((launcher, game))
}

fn run_steam_game(home: &Path, bottle_id: &str, app_id: u32) -> Result<LaunchResult, String> {
    let (launcher, game) = steam_game_launcher(home, bottle_id, app_id)?;
    run_launcher(home, bottle_id, &launcher.id, &game.launch_arguments())
}

fn run_launcher(
    home: &Path,
    bottle_id: &str,
    launcher_id: &str,
    user_arguments: &[String],
) -> Result<LaunchResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    run_launcher_with_wine(home, bottle_id, launcher_id, &wine, user_arguments)
}

fn run_launcher_with_wine(
    home: &Path,
    bottle_id: &str,
    launcher_id: &str,
    wine: &Path,
    user_arguments: &[String],
) -> Result<LaunchResult, String> {
    let launcher = list_launchers(home, bottle_id)?
        .into_iter()
        .find(|item| item.id == launcher_id)
        .ok_or("Launcher not found")?;
    let steam_mode = steam_install::is_recipe_steam_launcher(home, bottle_id, &launcher);
    let itch_mode = itch_install::is_recipe_itch_launcher(home, bottle_id, &launcher);
    let arguments = launcher_arguments(
        steam_mode,
        itch_mode,
        &launcher.saved_arguments,
        user_arguments,
    )?;
    launch_with_wine_args(
        home,
        bottle_id,
        Path::new(&launcher.path),
        wine,
        &arguments,
        steam_mode,
    )
}

fn launcher_arguments(
    steam_mode: bool,
    itch_mode: bool,
    saved_arguments: &[String],
    user_arguments: &[String],
) -> Result<Vec<String>, String> {
    let extra_arguments: Vec<String> = saved_arguments
        .iter()
        .chain(user_arguments.iter())
        .cloned()
        .collect();
    validate_app_arguments(&extra_arguments)?;
    let mut arguments = if steam_mode {
        vec![steam_install::STEAM_CEF_SOFTWARE_FLAG.to_owned()]
    } else if itch_mode {
        itch_install::ITCH_LAUNCH_ARGS
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    } else {
        Vec::new()
    };
    arguments.extend(extra_arguments);
    Ok(arguments)
}

fn parse_run_app_arguments(options: &[String]) -> Result<Vec<String>, String> {
    if options.is_empty() {
        return Ok(Vec::new());
    }
    if options[0] != "--" {
        return Err("Put app arguments after --".into());
    }
    let arguments = &options[1..];
    validate_app_arguments(arguments)?;
    Ok(arguments.to_vec())
}

fn validate_app_arguments(arguments: &[String]) -> Result<(), String> {
    if arguments.len() > 64 {
        return Err("Use at most 64 app arguments".into());
    }
    let mut total_bytes = 0usize;
    for argument in arguments {
        if argument.len() > 4096 {
            return Err("An app argument is longer than 4096 bytes".into());
        }
        total_bytes = total_bytes
            .checked_add(argument.len())
            .ok_or("App arguments are too long")?;
    }
    if total_bytes > 16 * 1024 {
        return Err("App arguments are longer than 16 KiB together".into());
    }
    Ok(())
}

fn launch(home: &Path, id: &str, installer: &Path) -> Result<LaunchResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    launch_with_wine(home, id, installer, &wine)
}

fn parse_wait_launch_options(args: &[String]) -> Result<WaitLaunchOptions, String> {
    let mut options = WaitLaunchOptions {
        quiet_msi: false,
        timeout: Duration::from_secs(DEFAULT_LAUNCH_TIMEOUT_SECONDS),
        arguments: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--quiet-msi" if !options.quiet_msi => options.quiet_msi = true,
            "--timeout-seconds" => {
                let value = args
                    .get(index + 1)
                    .ok_or("--timeout-seconds requires a whole number")?;
                let seconds: u64 = value.parse().map_err(|_| {
                    format!("--timeout-seconds must be 1–{MAX_LAUNCH_TIMEOUT_SECONDS}")
                })?;
                if !(1..=MAX_LAUNCH_TIMEOUT_SECONDS).contains(&seconds) {
                    return Err(format!(
                        "--timeout-seconds must be 1–{MAX_LAUNCH_TIMEOUT_SECONDS}"
                    ));
                }
                options.timeout = Duration::from_secs(seconds);
                index += 1;
            }
            "--" => {
                options.arguments.extend_from_slice(&args[index + 1..]);
                break;
            }
            _ => {
                return Err(format!(
                    "Unknown launch-wait option: {}. Put Windows program arguments after --",
                    args[index]
                ));
            }
        }
        index += 1;
    }
    Ok(options)
}

fn launch_wait(
    home: &Path,
    id: &str,
    installer: &Path,
    options: WaitLaunchOptions,
) -> Result<WaitLaunchResult, String> {
    cancellation::check()?;
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    launch_wait_with_wine(home, id, installer, &wine, options)
}

fn initialize(home: &Path, id: &str) -> Result<PrefixResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    let _lock = lock_bottle(home, id)?;
    ensure_prefix_with_wine(home, id, &wine)
}

fn prefix_path(home: &Path, id: &str) -> Result<PrefixPathResult, String> {
    let prefix = bottle_dir(home, id)?.join("prefix");
    let (_guard, _) = open_stable_directory(&prefix)?;
    Ok(PrefixPathResult {
        path: prefix.display().to_string(),
    })
}

fn wait_child_bounded(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>, String> {
    wait_child_bounded_status(child, timeout)
        .map(|(status, timed_out)| (!timed_out).then_some(status))
}

fn wait_child_bounded_status(
    child: &mut Child,
    timeout: Duration,
) -> Result<(ExitStatus, bool), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) => {
                if cancellation::is_requested() {
                    // `child` has not been reaped, so the kernel cannot reuse
                    // its PID while we stop this exact invocation's process.
                    match child.kill() {
                        Ok(()) => {
                            child.wait().map_err(|error| {
                                format!("Could not confirm installer stopped after cancellation: {error}")
                            })?;
                        }
                        Err(error) => match child.try_wait() {
                            Ok(Some(_)) => {}
                            Ok(None) => {
                                return Err(format!(
                                    "Could not stop installer after cancellation: {error}"
                                ));
                            }
                            Err(wait_error) => {
                                return Err(format!(
                                    "Could not stop installer after cancellation: {error}; could not check its status: {wait_error}"
                                ));
                            }
                        },
                    }
                    return Err(cancellation::ERROR.into());
                }
                if Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                match child.kill() {
                    Ok(()) => {
                        let status = child
                            .wait()
                            .map_err(|e| format!("Could not reap timed-out Wine process: {e}"))?;
                        return Ok((status, true));
                    }
                    Err(error) => match child.try_wait() {
                        Ok(Some(status)) => return Ok((status, false)),
                        _ => return Err(format!("Could not stop timed-out Wine process: {error}")),
                    },
                }
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not wait for Wine process: {error}"));
            }
        }
    }
}

fn stop_prefix_wineserver(wine: &Path, prefix: &Path) -> String {
    let Some(parent) = wine.parent() else {
        return "Wine path has no companion wineserver; child processes may remain".into();
    };
    let wineserver = parent.join("wineserver");
    if !is_executable(&wineserver) || !is_independent_runtime(&wineserver) {
        return "No independent companion wineserver was available; child processes may remain"
            .into();
    }
    let Some(dir) = prefix.parent() else {
        return "Bottle prefix has no parent directory; child processes may remain".into();
    };
    let Ok(xdg) = bottle_xdg::for_bottle(dir) else {
        return "Could not prepare this bottle's private desktop; child processes may remain"
            .into();
    };
    let mut command = Command::new(wineserver);
    xdg.apply(&mut command);
    let started = command
        .arg("-k")
        .env("WINEPREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = started else {
        return "Could not ask wineserver to stop this bottle; child processes may remain".into();
    };
    match wait_child_bounded(&mut child, Duration::from_secs(5)) {
        Ok(Some(status)) if status.success() => {
            "Asked wineserver to stop processes in this bottle".into()
        }
        _ => "Wineserver shutdown did not finish; child processes may remain".into(),
    }
}

fn wait_for_registry(prefix: &Path, log_path: &Path, timeout: Duration) -> Result<(), String> {
    let registry = prefix.join("system.reg");
    let deadline = Instant::now() + timeout;
    loop {
        cancellation::check()?;
        if !path_is_real_directory(prefix) {
            return Err(format!(
                "Bottle prefix changed while Wine was initializing. Check {}",
                log_path.display()
            ));
        }
        match fs::symlink_metadata(&registry) {
            Ok(metadata) if metadata.file_type().is_file() => return Ok(()),
            Ok(_) => {
                return Err(format!(
                    "Wine created an invalid registry file. Check {}",
                    log_path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Could not inspect the Wine registry: {error}. Check {}",
                    log_path.display()
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Wine exited, but its registry did not appear within {} seconds. Check {}",
                timeout.as_secs(),
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ensure_prefix_with_wine(home: &Path, id: &str, wine: &Path) -> Result<PrefixResult, String> {
    ensure_prefix_with_wine_timeout(home, id, wine, Duration::from_secs(120))
}

fn ensure_prefix_with_wine_timeout(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
) -> Result<PrefixResult, String> {
    cancellation::check()?;
    let dir = bottle_dir(home, id)?;
    let prefix = real_subdirectory(&dir, "prefix")?;
    let xdg = bottle_xdg::for_bottle(&dir)?;
    let record_path = dir.join("prefix-record.json");
    let resolved_wine = wine
        .canonicalize()
        .map_err(|e| e.to_string())?
        .display()
        .to_string();
    let record_metadata = fs::symlink_metadata(&record_path);
    if record_metadata
        .as_ref()
        .is_ok_and(|metadata| !metadata.file_type().is_file())
    {
        return Err("Bottle initialization record must be a regular file".into());
    }
    if record_metadata.is_ok() && path_is_real_file(&prefix.join("system.reg")) {
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid bottle initialization record: {e}"))?;
        let Some(verification) = verify_runtime_record(&record, wine)? else {
            return Err("This bottle uses a different Windows engine. Choose its original engine or an identical Caskrun engine package.".into());
        };
        // Upgrade old records while the original package is still available,
        // and remember the latest verified location after a safe relocation.
        let attestation = match verification {
            VerifiedRuntime::PathPinned => runtime_identity::bundled_runtime_attestation(wine)?,
            VerifiedRuntime::Bundled(attestation) => Some(attestation),
        };
        let identity = attestation.as_ref().map(|value| value.identity.clone());
        let fingerprint = attestation.as_ref().map(|value| {
            runtime_identity::bind_record_fingerprint(&value.identity, &value.metadata_fingerprint)
        });
        if record.wine_path != resolved_wine
            || record.runtime_identity != identity
            || record.runtime_fingerprint != fingerprint
        {
            record.wine_path = resolved_wine;
            record.runtime_identity = identity;
            record.runtime_fingerprint = fingerprint;
            save_json(&record_path, &record)?;
        }
        if !bottle_xdg::host_desktop_links(&prefix)?.is_empty() {
            refuse_live_wine_server(&dir, Duration::from_secs(1), Some(wine))?;
            bottle_xdg::migrate_host_desktop_links(&prefix, &xdg)?;
        }
        cancellation::check()?;
        return Ok(PrefixResult {
            prefix_path: prefix.display().to_string(),
            initialized: false,
            log_path: None,
        });
    }
    let wineboot = wine
        .parent()
        .ok_or("Wine path has no parent directory")?
        .join("wineboot");
    if !is_executable(&wineboot) {
        return Err(format!(
            "Missing wineboot next to Wine: {}",
            wineboot.display()
        ));
    }
    let logs = real_subdirectory(&dir, "logs")?;
    let log_path = logs.join(format!("wineboot-{}.log", Uuid::new_v4()));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let errors = log.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(wineboot);
    xdg.apply(&mut command);
    native_app_identity::configure_launch(
        &mut command,
        home,
        id,
        wine,
        &wine
            .parent()
            .and_then(Path::parent)
            .ok_or("Wine runtime has no package directory")?
            .join("runtime/lib/wine/x86_64-windows/wineboot.exe"),
    )?;
    cancellation::check()?;
    let mut child = command
        .arg("--init")
        .env("WINEPREFIX", &prefix)
        .env("WINEARCH", "win64")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|e| format!("Could not initialize bottle: {e}"))?;
    let status = match wait_child_bounded(&mut child, timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            cancellation::check()?;
            let cleanup = stop_prefix_wineserver(wine, &prefix);
            let elapsed = if timeout < Duration::from_secs(1) {
                format!("{} milliseconds", timeout.as_millis())
            } else {
                format!("{} seconds", timeout.as_secs())
            };
            return Err(format!(
                "Wine bottle initialization timed out after {elapsed}. {cleanup}. Check {} and retry after resolving any Wine prompts or downloads.",
                log_path.display()
            ));
        }
        Err(error) => {
            if cancellation::is_error(&error) {
                return Err(error);
            }
            let cleanup = stop_prefix_wineserver(wine, &prefix);
            return Err(format!("{error}. {cleanup}. Check {}", log_path.display()));
        }
    };
    if !status.success() {
        return Err(format!(
            "Wine could not initialize this bottle. Check {}",
            log_path.display()
        ));
    }
    wait_for_registry(&prefix, &log_path, Duration::from_secs(10))?;
    let attestation = runtime_identity::bundled_runtime_attestation(wine)?;
    save_json(
        &record_path,
        &PrefixRecord {
            wine_path: resolved_wine,
            runtime_identity: attestation.as_ref().map(|value| value.identity.clone()),
            runtime_fingerprint: attestation.as_ref().map(|value| {
                runtime_identity::bind_record_fingerprint(
                    &value.identity,
                    &value.metadata_fingerprint,
                )
            }),
        },
    )?;
    Ok(PrefixResult {
        prefix_path: prefix.display().to_string(),
        initialized: true,
        log_path: Some(log_path.display().to_string()),
    })
}

fn launch_with_wine(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
) -> Result<LaunchResult, String> {
    launch_with_wine_args(home, id, installer, wine, &[], false)
}

fn launch_with_wine_args(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
    arguments: &[String],
    steam_mode: bool,
) -> Result<LaunchResult, String> {
    let (child, log_path) =
        start_launch_with_wine(home, id, installer, wine, false, arguments, steam_mode)?;
    Ok(LaunchResult {
        pid: child.id(),
        log_path: log_path.display().to_string(),
    })
}

fn launch_wait_with_wine(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
    options: WaitLaunchOptions,
) -> Result<WaitLaunchResult, String> {
    launch_wait_with_wine_checked(home, id, installer, wine, options, |_| Ok(()))
}

fn launch_wait_with_wine_checked(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
    options: WaitLaunchOptions,
    pre_spawn: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<WaitLaunchResult, String> {
    cancellation::check()?;
    let (mut child, log_path) = start_launch_with_wine_checked(
        home,
        id,
        installer,
        wine,
        LaunchFlags {
            quiet_msi: options.quiet_msi,
            steam_mode: false,
        },
        &options.arguments,
        pre_spawn,
    )?;
    let pid = child.id();
    let started = Instant::now();
    let wait_result = wait_child_bounded_status(&mut child, options.timeout);
    // Clear the record on every completed wait path, including cancellation.
    // If stopping the owned child failed, keep its record: it may still run.
    let tracking_result = (|| -> Result<(), String> {
        match child.try_wait() {
            Ok(Some(_)) => {
                let dir = bottle_dir(home, id)?;
                // Cancellation must not interrupt bookkeeping after Wine is reaped.
                let _lock = lock_bottle_directory_with_cancel(&dir, false)?;
                let mut tracked = read_active_processes(&dir)?;
                tracked.retain(|tracked_pid| *tracked_pid != pid);
                save_json(&dir.join("active-processes.json"), &tracked)
            }
            Ok(None) => {
                Err("The installer may still be running; its tracking record was retained".into())
            }
            Err(error) => Err(format!(
                "Could not confirm whether the installer exited; its tracking record was retained: {error}"
            )),
        }
    })();
    if let Err(error) = tracking_result {
        return Err(match wait_result {
            Err(wait_error) if cancellation::is_error(&wait_error) => {
                format!("Installer stopped, but could not update its tracking record: {error}")
            }
            Err(wait_error) => format!("{wait_error}; {error}"),
            Ok(_) => error,
        });
    }
    let (status, timed_out) = wait_result?;
    Ok(WaitLaunchResult {
        pid,
        log_path: log_path.display().to_string(),
        exit_code: status.code(),
        signal: status.signal(),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        timed_out,
    })
}

fn start_launch_with_wine(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
    quiet_msi: bool,
    arguments: &[String],
    steam_mode: bool,
) -> Result<(Child, PathBuf), String> {
    start_launch_with_wine_checked(
        home,
        id,
        installer,
        wine,
        LaunchFlags {
            quiet_msi,
            steam_mode,
        },
        arguments,
        |_| Ok(()),
    )
}

#[derive(Clone, Copy)]
struct LaunchFlags {
    quiet_msi: bool,
    steam_mode: bool,
}

fn start_launch_with_wine_checked(
    home: &Path,
    id: &str,
    installer: &Path,
    wine: &Path,
    flags: LaunchFlags,
    arguments: &[String],
    pre_spawn: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(Child, PathBuf), String> {
    cancellation::check()?;
    let _lock = lock_bottle(home, id)?;
    let installer = installer.canonicalize().map_err(|e| e.to_string())?;
    if !installer.is_file() {
        return Err("Choose an existing installer file".into());
    }
    let ext = installer
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "exe" && ext != "msi" {
        return Err("Choose a .exe or .msi file".into());
    }
    if flags.quiet_msi && ext != "msi" {
        return Err("--quiet-msi only applies to .msi installers".into());
    }
    let migrated = if flags.steam_mode {
        runtime_migration::maybe_migrate_steam_runtime(home, id, wine, &installer)?
    } else {
        None
    };
    if let Err(error) = ensure_prefix_with_wine(home, id, wine) {
        if let Some(backup) = migrated {
            return match backup.restore() {
                Ok(()) => Err(format!(
                    "{error}. The previous Steam runtime record was restored; the bottle was not launched."
                )),
                Err(rollback_error) => Err(format!(
                    "{error}. Automatic runtime-record rollback failed: {rollback_error}"
                )),
            };
        }
        return Err(error);
    }
    cancellation::check()?;
    let dir = bottle_dir(home, id)?;
    let logs = real_subdirectory(&dir, "logs")?;
    let mut tracked_processes = live_processes(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let log_path = logs.join(format!("launch-{stamp}-{}.log", Uuid::new_v4()));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let errors: File = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(wine);
    bottle_xdg::for_bottle(&dir)?.apply(&mut cmd);
    cmd.env_remove("CASKRUN_STEAM_MODE");
    cmd.env_remove("CASKRUN_STEAM_ICON");
    // Only the verified catalog Steam launcher enables Caskrun's Wine Steam
    // compatibility path. A user-selected EXE, including another Steam.exe,
    // must not inherit Steam-specific process handling.
    if flags.steam_mode {
        cmd.env("CASKRUN_STEAM_MODE", "1");
        // The artwork comes from this bottle's installed Steam executable.
        // A missing or malformed icon must never prevent Steam from opening.
        let steam_icon = list_launchers(home, id)
            .ok()
            .and_then(|launchers| {
                launchers
                    .into_iter()
                    .find(|item| Path::new(&item.path) == installer)
            })
            .and_then(|launcher| app_icons::app_icon(home, id, &launcher.id).ok())
            .and_then(|icon| icon.path);
        if let Some(path) = steam_icon {
            cmd.env("CASKRUN_STEAM_ICON", path);
        }
    }
    if ext == "msi" {
        cmd.arg("msiexec").arg("/i");
    }
    cmd.arg(&installer);
    if flags.quiet_msi {
        cmd.arg("/qn");
    }
    native_app_identity::configure_launch(&mut cmd, home, id, wine, &installer)?;
    cmd.args(arguments)
        .env("WINEPREFIX", dir.join("prefix"))
        .current_dir(
            installer
                .parent()
                .ok_or("Installer has no parent directory")?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors));
    // Catalog retries recheck their already-hashed installer and bottle state
    // here, while the same lock used by ordinary launches is still held.
    pre_spawn(&installer)?;
    cancellation::check()?;
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not launch Wine: {e}"))?;
    let pid = child.id();
    tracked_processes.push(pid);
    if let Err(error) = save_json(&dir.join("active-processes.json"), &tracked_processes) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Could not track the running Windows app: {error}"));
    }
    Ok((child, log_path))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|command| {
        matches!(
            command.as_str(),
            "launch-wait" | "install-catalog" | "retry-catalog" | "quit-all-bottles"
        )
    }) {
        cancellation::activate_from_stdin();
    }
    cancellation::check()?;
    let home = data_home()?;
    ensure_independent_storage(&home)?;
    // Internal native-loader callback. This returns a plain path over a
    // bounded pipe, and never launches an app or takes its parent's lock.
    if let [command, id, image] = args.as_slice()
        && command == "prepare-native-app"
    {
        let loader = native_app_identity::prepare(&home, id, Path::new(image))?;
        println!("{}", loader.display());
        return Ok(());
    }
    let json = match args.as_slice() {
        [command] if command == "status" => serde_json::to_value(Status {
            wine_path: wine_path().map(|p| p.display().to_string()),
            bottle_count: list(&home)?.len(),
        }),
        [command] if command == "list" => serde_json::to_value(list(&home)?),
        [command] if command == "deleted" => serde_json::to_value(list_deleted(&home)?),
        [command] if command == "overview" => serde_json::to_value(overview(&home)?),
        [command] if command == "catalog-attempts" =>
            serde_json::to_value(steam_install::catalog_attempts(&home)?),
        [command] if command == "probe-runtime" => {
            let wine = wine_path().ok_or("Choose an independent Wine executable with wineboot beside it")?;
            serde_json::to_value(runtime_probe::probe(&home, &wine)?)
        },
        [command, name] if command == "create" => serde_json::to_value(create(&home, name)?),
        [command, id] if command == "install-catalog" && id == "epic-games-store" =>
            serde_json::to_value(epic_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog" && id == gog_install::CATALOG_ID =>
            serde_json::to_value(gog_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog" && id == battle_net_install::CATALOG_ID =>
            serde_json::to_value(battle_net_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog"
            && matches!(id.as_str(), publisher_install::EA_ID | publisher_install::UBISOFT_ID) =>
            serde_json::to_value(publisher_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog" && id == itch_install::CATALOG_ID =>
            serde_json::to_value(itch_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog" && id == rockstar_install::CATALOG_ID =>
            serde_json::to_value(rockstar_install::install_catalog(&home, id)?),
        [command, id] if command == "install-catalog" =>
            serde_json::to_value(steam_install::install_catalog(&home, id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == "steam" =>
            serde_json::to_value(steam_install::retry_catalog(&home, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == "epic-games-store" =>
            serde_json::to_value(epic_install::retry_catalog(&home, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == gog_install::CATALOG_ID =>
            serde_json::to_value(gog_install::retry_catalog(&home, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == battle_net_install::CATALOG_ID =>
            serde_json::to_value(battle_net_install::retry_catalog(&home, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog"
                && matches!(catalog_id.as_str(), publisher_install::EA_ID | publisher_install::UBISOFT_ID) =>
            serde_json::to_value(publisher_install::retry_catalog(&home, catalog_id, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == itch_install::CATALOG_ID =>
            serde_json::to_value(itch_install::retry_catalog(&home, bottle_id)?),
        [command, catalog_id, bottle_id]
            if command == "retry-catalog" && catalog_id == rockstar_install::CATALOG_ID =>
            serde_json::to_value(rockstar_install::retry_catalog(&home, bottle_id)?),
        [command, id, name] if command == "rename" => serde_json::to_value(rename_bottle(&home, id, name)?),
        [command, id] if command == "delete" => serde_json::to_value(delete_bottle(&home, id)?),
        [command, id] if command == "restore-deleted" => serde_json::to_value(restore_deleted_bottle(&home, id)?),
        [command, id] if command == "duplicate" => serde_json::to_value(duplicate_bottle(&home, id, None)?),
        [command, id, name] if command == "duplicate" => serde_json::to_value(duplicate_bottle(&home, id, Some(name))?),
        [command, id, path] if command == "export" => serde_json::to_value(export_bottle(&home, id, Path::new(path))?),
        [command, path] if command == "import" => serde_json::to_value(import_bottle(&home, Path::new(path), None)?),
        [command, path, name] if command == "import" => serde_json::to_value(import_bottle(&home, Path::new(path), Some(name))?),
        [command, path] if command == "import-crossover" => serde_json::to_value(crossover_import::import(&home, Path::new(path), None)?),
        [command, path, name] if command == "import-crossover" => serde_json::to_value(crossover_import::import(&home, Path::new(path), Some(name))?),
        [command, id] if command == "steam-games" => serde_json::to_value(steam_games(&home, id)?),
        [command, id, app_id] if command == "run-steam-game" => serde_json::to_value(run_steam_game(&home, id, app_id.parse().map_err(|_| "Steam AppID must be an unsigned number")?)?),
        [command, id] if command == "apps" => serde_json::to_value(list_launchers(&home, id)?),
        [command, bottle_id, launcher_id] if command == "app-icon" =>
            serde_json::to_value(app_icons::app_icon(&home, bottle_id, launcher_id)?),
        [command, bottle_id] if command == "bottle-icon" =>
            serde_json::to_value(app_icons::bottle_icon(&home, bottle_id)?),
        [command] if command == "quit-all-bottles" =>
            serde_json::to_value(shutdown::quit_all(&home)?),
        [command, id] if command == "quit-bottle" =>
            serde_json::to_value(bottle_control::quit_windows_apps(&home, id)?),
        [command, id] if command == "bottle-resolution" =>
            serde_json::to_value(bottle_resolution::bottle_resolution(&home, id)?),
        [command, id, mode] if command == "set-bottle-resolution" && (mode == "on" || mode == "off") =>
            serde_json::to_value(bottle_resolution::set_bottle_resolution(&home, id, mode == "on")?),
        [command, id] if command == "open-uninstaller" =>
            serde_json::to_value(bottle_control::open_uninstaller(&home, id)?),
        [command, id] if command == "open-winecfg" =>
            serde_json::to_value(wine_tools::open_wine_configuration(&home, id)?),
        [command, id] if command == "open-regedit" =>
            serde_json::to_value(wine_tools::open_registry_editor(&home, id)?),
        [command, id] if command == "open-controller-panel" =>
            serde_json::to_value(wine_tools::open_game_controllers(&home, id)?),
        [command, id] if command == "open-taskmgr" =>
            serde_json::to_value(wine_tools::open_task_manager(&home, id)?),
        [command, id] if command == "open-internet-settings" =>
            serde_json::to_value(wine_tools::open_internet_settings(&home, id)?),
        [command, id] if command == "open-command-prompt" =>
            serde_json::to_value(wine_tools::open_command_prompt(&home, id)?),
        [command, id] if command == "simulate-reboot" =>
            serde_json::to_value(wine_tools::simulate_reboot(&home, id)?),
        [command, id] if command == "discover-apps" => serde_json::to_value(discover_apps(&home, id)?),
        [command, id] if command == "logs" => serde_json::to_value(list_logs(&home, id)?),
        [command, id, name] if command == "read-log" => serde_json::to_value(read_log(&home, id, name)?),
        [command, id] if command == "initialize" => serde_json::to_value(initialize(&home, id)?),
        [command, id] if command == "prefix-path" => serde_json::to_value(prefix_path(&home, id)?),
        [command, id, name, path] if command == "add-app" =>
            serde_json::to_value(add_launcher(&home, id, name, Path::new(path))?),
        [command, bottle_id, launcher_id, visibility]
            if command == "set-app-visibility" && (visibility == "show" || visibility == "hide") =>
            serde_json::to_value(set_launcher_visibility(&home, bottle_id, launcher_id, visibility == "hide")?),
        [command, bottle_id, launcher_id, separator, arguments @ ..]
            if command == "set-app-arguments" && separator == "--" =>
            serde_json::to_value(set_launcher_arguments(&home, bottle_id, launcher_id, arguments)?),
        [command, bottle_id, launcher_id, options @ ..] if command == "run-app" =>
            serde_json::to_value(run_launcher(
                &home,
                bottle_id,
                launcher_id,
                &parse_run_app_arguments(options)?,
            )?),
        [command, id, path] if command == "launch" =>
            serde_json::to_value(launch(&home, id, Path::new(path))?),
        [command, id, path, options @ ..] if command == "launch-wait" =>
            serde_json::to_value(launch_wait(
                &home,
                id,
                Path::new(path),
                parse_wait_launch_options(options)?,
            )?),
        _ => return Err("Usage: caskrun-core overview | status | probe-runtime | list | deleted | catalog-attempts | create <name> | install-catalog steam|epic-games-store|gog-galaxy|battle-net|ea-app|ubisoft-connect|itch-io-app|rockstar-games-launcher | retry-catalog steam|epic-games-store|gog-galaxy|battle-net|ea-app|ubisoft-connect|itch-io-app|rockstar-games-launcher <bottle-id> | rename <id> <name> | delete <id> | restore-deleted <id> | duplicate <id> [name] | export <id> <path.caskrun-bottle> | import <path.caskrun-bottle> [name] | import-crossover <bottle-folder> [name] | steam-games <id> | run-steam-game <id> <appid> | initialize <id> | prefix-path <id> | apps <id> | app-icon <bottle-id> <launcher-id> | bottle-icon <bottle-id> | bottle-resolution <id> | set-bottle-resolution <id> on|off | quit-all-bottles | quit-bottle <id> | open-uninstaller <id> | open-winecfg <id> | open-regedit <id> | open-controller-panel <id> | open-taskmgr <id> | open-internet-settings <id> | open-command-prompt <id> | simulate-reboot <id> | discover-apps <id> | logs <id> | read-log <id> <log-file-name> | add-app <id> <name> <file.exe> | set-app-visibility <id> <app-id> show|hide | set-app-arguments <id> <app-id> -- [arguments...] | run-app <id> <app-id> [-- <program-args...>] | launch <id> <file.exe|file.msi> | launch-wait <id> <file.exe|file.msi> [--quiet-msi] [--timeout-seconds 1..3600] [-- <program-args...>]".into()),
    }.map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&json).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(if cancellation::is_error(&message) {
            cancellation::EXIT_CODE
        } else {
            1
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-test-{}", Uuid::new_v4()))
    }

    #[test]
    fn bottles_are_isolated_and_round_trip() {
        let home = test_home();
        let first = create(&home, "Steam").unwrap();
        let second = create(&home, "My App").unwrap();
        assert_ne!(first.id, second.id);
        assert!(
            bottle_dir(&home, &first.id)
                .unwrap()
                .join("prefix")
                .is_dir()
        );
        assert_eq!(list(&home).unwrap().len(), 2);
        assert!(bottle_dir(&home, "../../CrossOver").is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn overview_keeps_the_library_visible_when_one_setup_receipt_is_invalid() {
        let home = test_home();
        let first = create(&home, "Steam attempt").unwrap();
        let second = create(&home, "Another app").unwrap();
        let executable = bottle_dir(&home, &second.id)
            .unwrap()
            .join("prefix/drive_c/program.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"app").unwrap();
        add_launcher(&home, &second.id, "Program", &executable).unwrap();
        fs::write(
            bottle_dir(&home, &first.id)
                .unwrap()
                .join("catalog-attempt.json"),
            b"not json",
        )
        .unwrap();

        let snapshot = overview(&home).unwrap();
        assert_eq!(snapshot.status.bottle_count, 2);
        assert_eq!(snapshot.bottles.len(), 2);
        assert_eq!(snapshot.launchers[&second.id][0].name, "Program");
        assert!(snapshot.catalog_attempts.is_empty());
        assert!(
            snapshot
                .catalog_attempts_error
                .unwrap()
                .contains("Steam attempt")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn deleted_bottle_round_trip_preserves_prefix_and_launchers() {
        let home = test_home();
        let bottle = create(&home, "Keep my files").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let exe = dir.join("prefix/drive_c/program.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"saved program").unwrap();
        let launcher = add_launcher(&home, &bottle.id, "Program", &exe).unwrap();
        let prefix = prefix_path(&home, &bottle.id).unwrap();
        assert_eq!(prefix.path, dir.join("prefix").display().to_string());
        assert!(!dir.join("prefix-record.json").exists());

        let deleted = delete_bottle(&home, &bottle.id).unwrap();
        assert_eq!(deleted.id, bottle.id);
        assert_eq!(deleted.name, bottle.name);
        assert!(list(&home).unwrap().is_empty());
        assert!(prefix_path(&home, &bottle.id).is_err());
        assert_eq!(list_deleted(&home).unwrap()[0].id, bottle.id);
        let trashed = home.join("deleted-bottles").join(&bottle.id);
        assert_eq!(
            fs::read(trashed.join("prefix/drive_c/program.exe")).unwrap(),
            b"saved program"
        );
        assert!(trashed.join("launchers.json").is_file());

        let restored = restore_deleted_bottle(&home, &bottle.id).unwrap();
        assert_eq!(restored.id, bottle.id);
        assert_eq!(restored.name, bottle.name);
        assert!(list_deleted(&home).unwrap().is_empty());
        assert_eq!(list(&home).unwrap()[0].id, bottle.id);
        assert_eq!(fs::read(&exe).unwrap(), b"saved program");
        assert_eq!(
            list_launchers(&home, &bottle.id).unwrap()[0].path,
            launcher.path
        );
        assert_eq!(prefix_path(&home, &bottle.id).unwrap().path, prefix.path);
        assert!(!dir.join("prefix-record.json").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn deletion_refuses_running_tracked_wine_process() {
        let home = test_home();
        let bottle = create(&home, "Busy").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let mut child = Command::new("/bin/sleep").arg("3").spawn().unwrap();
        save_json(&dir.join("active-processes.json"), &vec![child.id()]).unwrap();
        let error = delete_bottle(&home, &bottle.id).err().unwrap();
        assert!(error.contains("Close Windows apps"));
        assert!(dir.exists());
        assert!(list_deleted(&home).unwrap().is_empty());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(delete_bottle(&home, &bottle.id).unwrap().id, bottle.id);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn deletion_refuses_child_after_direct_wine_process_exits() {
        let home = test_home();
        let bottle = create(&home, "Long-running child").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let prefix = dir.join("prefix");
        fs::write(prefix.join("system.reg"), b"initialized").unwrap();

        let runtime = home.join("runtime");
        fs::create_dir(&runtime).unwrap();
        let wine = runtime.join("wine");
        let wineboot = runtime.join("wineboot");
        let wineserver = runtime.join("wineserver");
        fs::write(
            &wine,
            b"#!/bin/sh\ntouch \"$WINEPREFIX/active-child\"\n( sleep 2; rm \"$WINEPREFIX/active-child\" ) >/dev/null 2>&1 &\necho $! > \"$WINEPREFIX/surviving-child.pid\"\nexit 0\n",
        )
        .unwrap();
        fs::write(&wineboot, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::write(
            &wineserver,
            b"#!/bin/sh\ntest \"$1\" = '-w' || exit 2\nwhile test -f \"$WINEPREFIX/active-child\"; do sleep 0.05; done\n",
        )
        .unwrap();
        for executable in [&wine, &wineboot, &wineserver] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        save_json(
            &dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: wine.display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        let installer = home.join("installer.exe");
        fs::write(&installer, b"test installer").unwrap();
        let result = launch_wait_with_wine(
            &home,
            &bottle.id,
            &installer,
            &wine,
            parse_wait_launch_options(&[]).unwrap(),
        )
        .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(read_active_processes(&dir).unwrap().is_empty());
        let child_pid: u32 = fs::read_to_string(prefix.join("surviving-child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(process_alive(child_pid).unwrap());

        let error =
            delete_bottle_with_timeout(&home, &bottle.id, Duration::from_millis(200), Some(&wine))
                .err()
                .unwrap();
        assert!(error.contains("Close all Windows apps"));
        assert!(dir.is_dir());
        assert!(list_deleted(&home).unwrap().is_empty());

        let deadline = Instant::now() + Duration::from_secs(4);
        while prefix.join("active-child").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(!prefix.join("active-child").exists());
        assert_eq!(
            delete_bottle_with_timeout(&home, &bottle.id, Duration::from_secs(1), Some(&wine))
                .unwrap()
                .id,
            bottle.id
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn initialized_bottle_without_checkable_wineserver_cannot_be_moved() {
        let home = test_home();
        let bottle = create(&home, "Missing runtime").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(dir.join("prefix/system.reg"), b"initialized").unwrap();
        save_json(
            &dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: home.join("missing/wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        let error = delete_bottle(&home, &bottle.id).err().unwrap();
        assert!(error.contains("runtime") || error.contains("Wine"));
        assert!(dir.is_dir());

        let partial = create(&home, "Partial prefix").unwrap();
        let partial_dir = bottle_dir(&home, &partial.id).unwrap();
        save_json(
            &partial_dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: home.join("missing/wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        assert!(delete_bottle(&home, &partial.id).is_err());
        assert!(partial_dir.is_dir());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn restore_remains_available_after_runtime_is_removed() {
        let home = test_home();
        let bottle = create(&home, "Recoverable").unwrap();
        delete_bottle(&home, &bottle.id).unwrap();
        let deleted = home.join("deleted-bottles").join(&bottle.id);
        fs::write(deleted.join("prefix/system.reg"), b"initialized").unwrap();
        save_json(
            &deleted.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: home.join("removed/wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();

        assert_eq!(
            restore_deleted_bottle(&home, &bottle.id).unwrap().id,
            bottle.id
        );
        assert!(
            bottle_dir(&home, &bottle.id)
                .unwrap()
                .join("prefix/system.reg")
                .is_file()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bottle_runtime_record_cannot_select_an_executable_to_run() {
        let home = test_home();
        let bottle = create(&home, "Tampered record").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(dir.join("prefix/system.reg"), b"initialized").unwrap();

        let selected = home.join("selected");
        let recorded = home.join("recorded");
        fs::create_dir(&selected).unwrap();
        fs::create_dir(&recorded).unwrap();
        for executable in [
            selected.join("wine"),
            selected.join("wineboot"),
            recorded.join("wine"),
        ] {
            fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        let marker = home.join("recorded-runtime-ran");
        let wineserver = recorded.join("wineserver");
        fs::write(
            &wineserver,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        let mut permissions = fs::metadata(&wineserver).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineserver, permissions).unwrap();
        save_json(
            &dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: recorded.join("wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();

        let error = delete_bottle_with_timeout(
            &home,
            &bottle.id,
            Duration::from_millis(200),
            Some(&selected.join("wine")),
        )
        .err()
        .unwrap();
        assert!(error.contains("Select the Wine runtime used"));
        assert!(!marker.exists());
        assert!(dir.is_dir());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn deletion_and_restore_reject_symlinks_traversal_and_overwrite() {
        let home = test_home();
        let bottle = create(&home, "Protected").unwrap();
        let active = bottle_dir(&home, &bottle.id).unwrap();
        assert!(delete_bottle(&home, "../CrossOver").is_err());
        assert!(restore_deleted_bottle(&home, "../CrossOver").is_err());

        let outside = home.join("outside");
        fs::create_dir(&outside).unwrap();
        let trash = home.join("deleted-bottles");
        symlink(&outside, &trash).unwrap();
        assert!(delete_bottle(&home, &bottle.id).is_err());
        assert!(active.exists());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_file(&trash).unwrap();

        delete_bottle(&home, &bottle.id).unwrap();
        let moved = trash.join(&bottle.id);
        let metadata = moved.join("bottle.json");
        let saved_metadata = moved.join("bottle.json.saved");
        fs::rename(&metadata, &saved_metadata).unwrap();
        symlink(&saved_metadata, &metadata).unwrap();
        assert!(restore_deleted_bottle(&home, &bottle.id).is_err());
        assert!(!active.exists());
        fs::remove_file(&metadata).unwrap();
        fs::rename(saved_metadata, &metadata).unwrap();

        fs::create_dir(&active).unwrap();
        fs::write(active.join("sentinel"), b"keep").unwrap();
        let error = restore_deleted_bottle(&home, &bottle.id).err().unwrap();
        assert!(error.contains("already exists"));
        assert_eq!(fs::read(active.join("sentinel")).unwrap(), b"keep");
        assert!(moved.join("bottle.json").is_file());
        fs::remove_dir_all(&active).unwrap();
        assert_eq!(
            restore_deleted_bottle(&home, &bottle.id).unwrap().id,
            bottle.id
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rejects_invalid_names() {
        let home = test_home();
        assert!(create(&home, "  ").is_err());
        assert!(create(&home, "bad\nname").is_err());
        assert!(!home.exists());
        assert!(!is_independent_runtime(Path::new(
            "/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/wine"
        )));
    }

    #[test]
    fn saved_launcher_is_scoped_to_its_bottle() {
        let home = test_home();
        let first = create(&home, "First").unwrap();
        let second = create(&home, "Second").unwrap();
        let exe = home.join("test.exe");
        fs::write(&exe, b"not a real executable").unwrap();
        let saved = add_launcher(&home, &first.id, "Test", &exe).unwrap();
        let msi = home.join("setup.msi");
        fs::write(&msi, b"installer").unwrap();
        assert!(add_launcher(&home, &first.id, "Setup", &msi).is_err());
        assert_eq!(list_launchers(&home, &first.id).unwrap()[0].id, saved.id);
        assert!(list_launchers(&home, &second.id).unwrap().is_empty());
        let renamed = rename_bottle(&home, &first.id, "Renamed").unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert_eq!(list_launchers(&home, &first.id).unwrap().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn saved_launcher_forwards_each_option_as_one_argument() {
        let home = test_home();
        let bottle = create(&home, "Options").unwrap();
        let wine = home.join("fake-wine");
        fs::write(
            &wine,
            b"#!/bin/sh\nindex=0\nfor argument in \"$@\"; do\n  printf 'arg%s=%s\\n' \"$index\" \"$argument\"\n  index=$((index + 1))\ndone\n",
        )
        .unwrap();
        let wineboot = home.join("wineboot");
        fs::write(&wineboot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        for executable in [&wine, &wineboot] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        let exe = home.join("sample.exe");
        fs::write(&exe, b"placeholder").unwrap();
        let launcher = add_launcher(&home, &bottle.id, "Sample", &exe).unwrap();
        let saved_arguments = vec!["--account".to_owned(), "two words".to_owned()];
        set_launcher_arguments(&home, &bottle.id, &launcher.id, &saved_arguments).unwrap();
        let arguments = vec![r"--path=C:\Program Files\Game".to_owned()];
        let result =
            run_launcher_with_wine(&home, &bottle.id, &launcher.id, &wine, &arguments).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        while Instant::now() < deadline {
            output = fs::read_to_string(&result.log_path).unwrap();
            if output.contains("arg3=--path=") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            output.contains(&format!("arg0={}\n", exe.canonicalize().unwrap().display())),
            "{output}"
        );
        assert!(output.contains("arg1=--account\n"), "{output}");
        assert!(output.contains("arg2=two words\n"), "{output}");
        assert!(
            output.contains("arg3=--path=C:\\Program Files\\Game\n"),
            "{output}"
        );
        assert!(!output.contains("arg4="), "{output}");
        let normal = run_launcher_with_wine(&home, &bottle.id, &launcher.id, &wine, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut normal_output = String::new();
        while Instant::now() < deadline {
            normal_output = fs::read_to_string(&normal.log_path).unwrap();
            if normal_output.contains("arg2=two words") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            normal_output.contains("arg1=--account\n"),
            "{normal_output}"
        );
        assert!(
            normal_output.contains("arg2=two words\n"),
            "{normal_output}"
        );
        assert!(!normal_output.contains("arg3="), "{normal_output}");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn run_app_options_require_separator_and_preserve_steam_flag() {
        let user_arguments = vec!["-login".to_owned(), "two words".to_owned()];
        let mut options = vec!["--".to_owned()];
        options.extend_from_slice(&user_arguments);
        assert_eq!(parse_run_app_arguments(&options).unwrap(), user_arguments);
        assert!(parse_run_app_arguments(&user_arguments).is_err());
        assert!(parse_run_app_arguments(&["--".to_owned(), "x".repeat(4097)]).is_err());
        assert!(parse_run_app_arguments(&vec!["--".to_owned(); 66]).is_err());
        assert_eq!(
            launcher_arguments(true, false, &["--saved".to_owned()], &user_arguments).unwrap(),
            vec![
                steam_install::STEAM_CEF_SOFTWARE_FLAG.to_owned(),
                "--saved".to_owned(),
                "-login".to_owned(),
                "two words".to_owned(),
            ]
        );
        assert!(
            launcher_arguments(false, false, &vec!["saved".to_owned(); 64], &user_arguments)
                .is_err()
        );
    }

    #[test]
    fn itch_app_options_follow_bootstrap_separator_without_changing_manual_apps() {
        let saved = vec!["--profile".to_owned(), "two words".to_owned()];
        let temporary = parse_run_app_arguments(&[
            "--".to_owned(),
            "--disable-gpu".to_owned(),
            "itch://install?game_id=123&launch".to_owned(),
        ])
        .unwrap();
        let itch_arguments = launcher_arguments(false, true, &saved, &temporary).unwrap();
        assert_eq!(
            itch_arguments,
            vec![
                "--prefer-launch",
                "--appname",
                "itch",
                "--",
                "--profile",
                "two words",
                "--disable-gpu",
                "itch://install?game_id=123&launch",
            ]
        );
        assert_eq!(
            itch_arguments
                .iter()
                .filter(|argument| *argument == "--")
                .count(),
            1
        );
        assert_eq!(
            launcher_arguments(false, false, &saved, &temporary).unwrap(),
            saved.iter().chain(&temporary).cloned().collect::<Vec<_>>()
        );
        // A separator supplied as an app argument is literal data after the
        // proxy's separator; it must not be removed or treated as helper flags.
        assert_eq!(
            launcher_arguments(false, true, &[], &["--".to_owned(), "value".to_owned()]).unwrap(),
            vec!["--prefer-launch", "--appname", "itch", "--", "--", "value"]
        );
    }

    #[test]
    fn saved_arguments_are_scoped_resettable_and_portable() {
        let home = test_home();
        let bottle = create(&home, "Options").unwrap();
        let other = create(&home, "Other").unwrap();
        let exe = home.join("sample.exe");
        fs::write(&exe, b"placeholder").unwrap();
        let launcher = add_launcher(&home, &bottle.id, "Sample", &exe).unwrap();
        let saved = vec!["--profile".to_owned(), "My Games".to_owned()];
        assert!(set_launcher_arguments(&home, &other.id, &launcher.id, &saved).is_err());
        assert!(
            set_launcher_arguments(&home, &bottle.id, &launcher.id, &["x".repeat(4097)]).is_err()
        );
        assert!(
            list_launchers(&home, &bottle.id).unwrap()[0]
                .saved_arguments
                .is_empty()
        );
        set_launcher_arguments(&home, &bottle.id, &launcher.id, &saved).unwrap();
        assert_eq!(
            list_launchers(&home, &bottle.id).unwrap()[0].saved_arguments,
            saved
        );

        let backup = home
            .parent()
            .unwrap()
            .join(format!("{}.caskrun-bottle", Uuid::new_v4()));
        export_bottle(&home, &bottle.id, &backup).unwrap();
        let restored_home = test_home();
        let restored = import_bottle(&restored_home, &backup, None).unwrap();
        assert_eq!(
            list_launchers(&restored_home, &restored.bottle.id).unwrap()[0].saved_arguments,
            saved
        );
        set_launcher_arguments(&home, &bottle.id, &launcher.id, &[]).unwrap();
        assert!(
            list_launchers(&home, &bottle.id).unwrap()[0]
                .saved_arguments
                .is_empty()
        );
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(restored_home).unwrap();
        fs::remove_dir_all(backup).unwrap();
    }

    #[test]
    fn hidden_launcher_remains_installed_and_survives_backup() {
        let home = test_home();
        let bottle = create(&home, "Library").unwrap();
        let other = create(&home, "Other").unwrap();
        let exe = home.join("game.exe");
        fs::write(&exe, b"test program").unwrap();
        let saved = add_launcher(&home, &bottle.id, "Game", &exe).unwrap();
        assert!(!saved.hidden_from_home);
        assert!(set_launcher_visibility(&home, &other.id, &saved.id, true).is_err());
        let hidden = set_launcher_visibility(&home, &bottle.id, &saved.id, true).unwrap();
        assert!(hidden.hidden_from_home);
        assert!(list_launchers(&home, &bottle.id).unwrap()[0].hidden_from_home);

        let backup = home
            .parent()
            .unwrap()
            .join(format!("{}.caskrun-bottle", Uuid::new_v4()));
        export_bottle(&home, &bottle.id, &backup).unwrap();
        let imported_home = test_home();
        let imported = import_bottle(&imported_home, &backup, None).unwrap();
        assert!(list_launchers(&imported_home, &imported.bottle.id).unwrap()[0].hidden_from_home);

        let shown = set_launcher_visibility(&home, &bottle.id, &saved.id, false).unwrap();
        assert!(!shown.hidden_from_home);
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(imported_home).unwrap();
        fs::remove_dir_all(backup).unwrap();
    }

    #[test]
    fn launches_in_selected_prefix_and_writes_log() {
        let home = test_home();
        let bottle = create(&home, "Test").unwrap();
        let wine = home.join("fake-wine");
        fs::write(
            &wine,
            b"#!/bin/sh\nprintf 'prefix=%s\\nfile=%s\\n' \"$WINEPREFIX\" \"$1\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        let wineboot = home.join("wineboot");
        fs::write(&wineboot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        let mut permissions = fs::metadata(&wineboot).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineboot, permissions).unwrap();
        let exe = home.join("sample.exe");
        fs::write(&exe, b"placeholder").unwrap();
        let result = launch_with_wine(&home, &bottle.id, &exe, &wine).unwrap();
        assert!(
            read_active_processes(&bottle_dir(&home, &bottle.id).unwrap())
                .unwrap()
                .contains(&result.pid)
        );
        let log_path = Path::new(&result.log_path);
        let mut output = String::new();
        let expected_prefix = format!(
            "prefix={}",
            home.join("bottles")
                .join(&bottle.id)
                .join("prefix")
                .display()
        );
        let expected_file = format!("file={}", exe.canonicalize().unwrap().display());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            output = fs::read_to_string(log_path).unwrap();
            if output.contains(&expected_prefix) && output.contains(&expected_file) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            output.contains(&expected_prefix),
            "expected prefix {expected_prefix:?} in launch log {output:?}"
        );
        assert!(
            output.contains(&expected_file),
            "expected file {expected_file:?} in launch log {output:?}"
        );
        assert!(
            home.join("bottles")
                .join(&bottle.id)
                .join("prefix/system.reg")
                .is_file()
        );
        assert!(
            home.join("bottles")
                .join(&bottle.id)
                .join("prefix-record.json")
                .is_file()
        );
        assert!(
            !ensure_prefix_with_wine(&home, &bottle.id, &wine)
                .unwrap()
                .initialized
        );
        let other_wine = home.join("other-wine");
        fs::copy(&wine, &other_wine).unwrap();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &other_wine).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    fn fake_wait_runtime(home: &Path, wine_script: &[u8]) -> PathBuf {
        let wine = home.join("wine");
        fs::write(&wine, wine_script).unwrap();
        let wineboot = home.join("wineboot");
        fs::write(&wineboot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        for executable in [&wine, &wineboot] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        wine
    }

    fn fake_bundled_runtime(root: &Path) -> PathBuf {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("runtime/bin")).unwrap();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::write(root.join("SOURCES.txt"), b"independent test package\n").unwrap();
        fs::write(root.join("lib/libfreetype.6.dylib"), b"test freetype").unwrap();
        for name in ["wine", "wineboot", "wineserver"] {
            fs::write(root.join("runtime/bin").join(name), b"test engine bytes").unwrap();
            symlink("_dispatch", root.join("bin").join(name)).unwrap();
        }
        let dispatch = root.join("bin/_dispatch");
        fs::write(
            &dispatch,
            b"#!/bin/sh\ncase \"${0##*/}\" in wineboot) touch \"$WINEPREFIX/system.reg\";; esac\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&dispatch).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&dispatch, permissions).unwrap();
        root.join("bin/wine")
    }

    #[test]
    fn unchanged_bundled_runtime_uses_metadata_and_rehashes_on_change() {
        let home = test_home();
        let bottle = create(&home, "Fast engine check").unwrap();
        let root = home.join("Engine");
        let wine = fake_bundled_runtime(&root);
        let before = runtime_identity::full_content_hash_count();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        let record_path = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        let initial: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert!(initial.runtime_fingerprint.is_some());

        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);

        // The bytes still match, but a rewrite changes kernel-controlled file
        // metadata. The slower content check is required once, then recached.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(root.join("runtime/bin/wine"), b"test engine bytes").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 2);
        let refreshed: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert_ne!(initial.runtime_fingerprint, refreshed.runtime_fingerprint);
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 2);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn tampered_runtime_identity_is_not_accepted_by_metadata_fast_path() {
        let home = test_home();
        let bottle = create(&home, "Tampered engine identity").unwrap();
        let wine = fake_bundled_runtime(&home.join("Engine"));
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let record_path = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        record.runtime_identity = Some(format!("sha256:{}", "0".repeat(64)));
        save_json(&record_path, &record).unwrap();

        let before = runtime_identity::full_content_hash_count();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn tampered_or_older_runtime_fingerprint_rehashes_and_upgrades() {
        let home = test_home();
        let bottle = create(&home, "Old engine fingerprint").unwrap();
        let wine = fake_bundled_runtime(&home.join("Engine"));
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let record_path = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        let expected_bound = record.runtime_fingerprint.clone();

        record.runtime_fingerprint = Some("sha256-bound-v2:wrong".into());
        save_json(&record_path, &record).unwrap();
        let before = runtime_identity::full_content_hash_count();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        let repaired: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert_eq!(repaired.runtime_fingerprint, expected_bound);

        // The first fast-path version stored only the raw metadata hash.
        record.runtime_fingerprint =
            runtime_identity::bundled_runtime_metadata_fingerprint(&wine).unwrap();
        save_json(&record_path, &record).unwrap();
        let before = runtime_identity::full_content_hash_count();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        let upgraded: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert_eq!(upgraded.runtime_fingerprint, expected_bound);
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn relocated_modified_bundle_requires_and_fails_full_content_check() {
        let home = test_home();
        let bottle = create(&home, "Changed moved engine").unwrap();
        let original = home.join("Original engine");
        let wine = fake_bundled_runtime(&original);
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let relocated = home.join("Relocated engine");
        fs::rename(&original, &relocated).unwrap();
        fs::write(relocated.join("runtime/bin/wine"), b"modified engine").unwrap();
        let before = runtime_identity::full_content_hash_count();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &relocated.join("bin/wine")).is_err());
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bundled_runtime_can_move_only_with_identical_contents() {
        let home = test_home();
        let bottle = create(&home, "Portable engine").unwrap();
        let original = home.join("Original engine");
        let wine = fake_bundled_runtime(&original);
        assert!(
            ensure_prefix_with_wine(&home, &bottle.id, &wine)
                .unwrap()
                .initialized
        );
        let record_path = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert!(
            record
                .runtime_identity
                .as_deref()
                .unwrap()
                .starts_with("sha256:")
        );
        assert!(record.runtime_fingerprint.is_some());

        let relocated = home.join("Relocated engine");
        fs::rename(&original, &relocated).unwrap();
        let moved_wine = relocated.join("bin/wine");
        let before = runtime_identity::full_content_hash_count();
        assert!(
            !ensure_prefix_with_wine(&home, &bottle.id, &moved_wine)
                .unwrap()
                .initialized
        );
        assert_eq!(runtime_identity::full_content_hash_count(), before + 1);
        let moved_record: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert_eq!(moved_record.runtime_identity, record.runtime_identity);
        assert_eq!(
            moved_record.wine_path,
            moved_wine.canonicalize().unwrap().display().to_string()
        );

        fs::write(
            relocated.join("runtime/bin/wine"),
            b"different engine bytes",
        )
        .unwrap();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &moved_wine).is_err());
        assert!(
            delete_bottle_with_timeout(
                &home,
                &bottle.id,
                Duration::from_secs(1),
                Some(&moved_wine)
            )
            .is_err()
        );
        assert!(bottle_dir(&home, &bottle.id).unwrap().exists());
        let outside = home.join("outside-runtime-file");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, relocated.join("runtime/external-link")).unwrap();
        let error = runtime_identity::bundled_runtime_identity(&moved_wine)
            .err()
            .unwrap();
        assert!(error.contains("external symlink"));
        let error = runtime_identity::bundled_runtime_metadata_fingerprint(&moved_wine)
            .err()
            .unwrap();
        assert!(error.contains("external symlink"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn legacy_records_upgrade_when_original_package_is_still_present() {
        let home = test_home();
        let bottle = create(&home, "Legacy package").unwrap();
        let original = home.join("Engine");
        let wine = fake_bundled_runtime(&original);
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let record_path = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        // This is the exact JSON shape written by earlier versions.
        fs::write(
            &record_path,
            serde_json::to_vec(&serde_json::json!({
                "wine_path": wine.canonicalize().unwrap().display().to_string()
            }))
            .unwrap(),
        )
        .unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let upgraded: PrefixRecord =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        assert!(upgraded.runtime_identity.is_some());
        assert!(upgraded.runtime_fingerprint.is_some());
        let relocated = home.join("Moved engine");
        fs::rename(&original, &relocated).unwrap();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &relocated.join("bin/wine")).is_ok());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn legacy_records_cannot_move_without_prior_content_proof() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let package = home.join("Custom A");
        fs::create_dir(&package).unwrap();
        let wine = package.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        let record = PrefixRecord {
            wine_path: wine.display().to_string(),
            runtime_identity: None,
            runtime_fingerprint: None,
        };
        let copy = home.join("Custom B");
        fs::create_dir(&copy).unwrap();
        fs::copy(&wine, copy.join("wine")).unwrap();
        assert!(!runtime_record_matches(&record, &copy.join("wine")).unwrap());
        fs::remove_file(&wine).unwrap();
        assert!(runtime_record_matches(&record, &copy.join("wine")).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn wait_launch_reports_exit_status_and_preserves_arguments() {
        let home = test_home();
        let bottle = create(&home, "Wait test").unwrap();
        let wine = fake_wait_runtime(
            &home,
            b"#!/bin/sh\nprintf 'arguments:'; printf '<%s>' \"$@\"; printf '\\nprefix=%s\\n' \"$WINEPREFIX\"; exit 37\n",
        );
        let exe = home.join("tiny.exe");
        let msi = home.join("tiny.msi");
        fs::write(&exe, b"EXE").unwrap();
        fs::write(&msi, b"MSI").unwrap();

        let exe_result = launch_wait_with_wine(
            &home,
            &bottle.id,
            &exe,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_secs(2),
                arguments: vec!["--flag".into(), "two words".into()],
            },
        )
        .unwrap();
        assert_eq!(exe_result.exit_code, Some(37));
        assert_eq!(exe_result.signal, None);
        assert!(!exe_result.timed_out);
        let exe_log = fs::read_to_string(exe_result.log_path).unwrap();
        assert!(exe_log.contains(&format!(
            "arguments:<{}><--flag><two words>",
            exe.canonicalize().unwrap().display()
        )));
        assert!(exe_log.contains(&format!(
            "prefix={}",
            bottle_dir(&home, &bottle.id).unwrap().join("prefix").display()
        )));

        let normal_msi = launch_wait_with_wine(
            &home,
            &bottle.id,
            &msi,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_secs(2),
                arguments: Vec::new(),
            },
        )
        .unwrap();
        let normal_log = fs::read_to_string(normal_msi.log_path).unwrap();
        assert!(normal_log.contains(&format!(
            "arguments:<msiexec></i><{}>",
            msi.canonicalize().unwrap().display()
        )));
        assert!(!normal_log.contains("/qn"));

        let quiet_msi = launch_wait_with_wine(
            &home,
            &bottle.id,
            &msi,
            &wine,
            WaitLaunchOptions {
                quiet_msi: true,
                timeout: Duration::from_secs(2),
                arguments: vec!["/L*v".into(), "two words.log".into()],
            },
        )
        .unwrap();
        let quiet_log = fs::read_to_string(quiet_msi.log_path).unwrap();
        assert!(quiet_log.contains(&format!(
            "arguments:<msiexec></i><{}></qn></L*v><two words.log>",
            msi.canonicalize().unwrap().display()
        )));
        assert!(
            read_active_processes(&bottle_dir(&home, &bottle.id).unwrap())
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn wait_launch_reports_signal_and_bounded_timeout() {
        let home = test_home();
        let bottle = create(&home, "Wait signals").unwrap();
        let wine = fake_wait_runtime(&home, b"#!/bin/sh\nkill -TERM $$\n");
        let exe = home.join("tiny.exe");
        fs::write(&exe, b"EXE").unwrap();
        let terminated = launch_wait_with_wine(
            &home,
            &bottle.id,
            &exe,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_secs(2),
                arguments: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(terminated.exit_code, None);
        assert_eq!(terminated.signal, Some(15));
        assert!(!terminated.timed_out);

        fs::write(&wine, b"#!/bin/sh\nexec sleep 5\n").unwrap();
        let timeout = launch_wait_with_wine(
            &home,
            &bottle.id,
            &exe,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_millis(100),
                arguments: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(timeout.exit_code, None);
        assert_eq!(timeout.signal, Some(9));
        assert!(timeout.timed_out);
        assert!(!process_alive(timeout.pid).unwrap());
        assert!(
            read_active_processes(&bottle_dir(&home, &bottle.id).unwrap())
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancel_wait_stops_only_its_owned_installer_and_clears_its_record() {
        use std::sync::atomic::Ordering;

        let home = test_home();
        let installing = create(&home, "Installing").unwrap();
        let unrelated = create(&home, "Unrelated").unwrap();
        let mut other_process = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let other_pid = other_process.id();
        let other_dir = bottle_dir(&home, &unrelated.id).unwrap();
        save_json(&other_dir.join("active-processes.json"), &vec![other_pid]).unwrap();

        let wine = fake_wait_runtime(
            &home,
            b"#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$WINEPREFIX/installer-started.pid\"\nexec sleep 30\n",
        );
        let installer = home.join("installer.exe");
        fs::write(&installer, b"EXE").unwrap();
        let installing_dir = bottle_dir(&home, &installing.id).unwrap();
        let started = installing_dir.join("prefix/installer-started.pid");
        let (requested, _guard) = cancellation::test_request();
        let request_thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !started.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            requested.store(true, Ordering::Release);
        });
        let error = launch_wait_with_wine(
            &home,
            &installing.id,
            &installer,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_secs(10),
                arguments: Vec::new(),
            },
        )
        .unwrap_err();
        request_thread.join().unwrap();
        assert_eq!(error, cancellation::ERROR);
        assert!(read_active_processes(&installing_dir).unwrap().is_empty());
        assert!(other_process.try_wait().unwrap().is_none());
        assert_eq!(read_active_processes(&other_dir).unwrap(), vec![other_pid]);
        other_process.kill().unwrap();
        other_process.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancel_during_wineboot_does_not_shutdown_the_bottle_server() {
        use std::sync::atomic::Ordering;

        let home = test_home();
        let bottle = create(&home, "Initializing").unwrap();
        let wine = fake_wait_runtime(&home, b"#!/bin/sh\ntouch \"$WINEPREFIX/wine-ran\"\n");
        let wineboot = home.join("wineboot");
        fs::write(
            &wineboot,
            b"#!/bin/sh\ntouch \"$WINEPREFIX/wineboot-started\"\nexec sleep 30\n",
        )
        .unwrap();
        let wineserver = home.join("wineserver");
        fs::write(
            &wineserver,
            b"#!/bin/sh\ntouch \"$WINEPREFIX/wineserver-called\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&wineserver).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineserver, permissions).unwrap();
        let installer = home.join("installer.exe");
        fs::write(&installer, b"EXE").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let started = prefix.join("wineboot-started");
        let (requested, _guard) = cancellation::test_request();
        let request_thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !started.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            requested.store(true, Ordering::Release);
        });
        let error = launch_wait_with_wine(
            &home,
            &bottle.id,
            &installer,
            &wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: Duration::from_secs(10),
                arguments: Vec::new(),
            },
        )
        .unwrap_err();
        request_thread.join().unwrap();
        assert_eq!(error, cancellation::ERROR);
        assert!(!prefix.join("wineserver-called").exists());
        assert!(!prefix.join("wine-ran").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancel_while_waiting_for_bottle_lock_keeps_the_lock_file_intact() {
        use std::sync::atomic::Ordering;

        let home = test_home();
        let bottle = create(&home, "Locked").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let held = lock_bottle(&home, &bottle.id).unwrap();
        let (requested, guard) = cancellation::test_request();
        let request_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            requested.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error = lock_bottle(&home, &bottle.id).unwrap_err();
        request_thread.join().unwrap();
        assert_eq!(error, cancellation::ERROR);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(dir.join(".caskrun.lock").is_file());
        drop(held);
        drop(guard);
        assert!(lock_bottle(&home, &bottle.id).is_ok());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn wait_launch_options_are_bounded_and_require_explicit_argument_separator() {
        let parsed = parse_wait_launch_options(&[
            "--timeout-seconds".into(),
            "42".into(),
            "--quiet-msi".into(),
            "--".into(),
            "--flag".into(),
        ])
        .unwrap();
        assert_eq!(parsed.timeout, Duration::from_secs(42));
        assert!(parsed.quiet_msi);
        assert_eq!(parsed.arguments, ["--flag"]);
        for invalid in [
            vec!["--timeout-seconds".into(), "0".into()],
            vec!["--timeout-seconds".into(), "3601".into()],
            vec!["--timeout-seconds".into()],
            vec!["--flag".into()],
        ] {
            assert!(parse_wait_launch_options(&invalid).is_err());
        }
        let home = test_home();
        let bottle = create(&home, "Options").unwrap();
        let wine = fake_wait_runtime(&home, b"#!/bin/sh\nexit 0\n");
        let exe = home.join("tiny.exe");
        fs::write(&exe, b"EXE").unwrap();
        assert!(
            launch_wait_with_wine(
                &home,
                &bottle.id,
                &exe,
                &wine,
                WaitLaunchOptions {
                    quiet_msi: true,
                    timeout: Duration::from_secs(2),
                    arguments: Vec::new(),
                },
            )
            .unwrap_err()
            .contains("only applies to .msi")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn timed_out_wineboot_is_stopped_and_reports_its_log() {
        let home = test_home();
        let bottle = create(&home, "Stalled").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let wine = home.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        let wineboot = home.join("wineboot");
        fs::write(
            &wineboot,
            b"#!/bin/sh\necho $$ > \"$WINEPREFIX/wineboot.pid\"\nexec sleep 3\n",
        )
        .unwrap();
        let wineserver = home.join("wineserver");
        fs::write(
            &wineserver,
            b"#!/bin/sh\nprintf '%s\\n' \"$WINEPREFIX\" > \"$WINEPREFIX/wineserver-stopped\"\n",
        )
        .unwrap();
        for executable in [&wineboot, &wineserver] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        let error =
            ensure_prefix_with_wine_timeout(&home, &bottle.id, &wine, Duration::from_millis(800))
                .err()
                .unwrap();
        assert!(error.contains("timed out after 800 milliseconds"));
        assert!(error.contains("Asked wineserver to stop"));
        assert!(error.contains("wineboot-"));
        assert!(error.contains(".log"));
        assert_eq!(
            fs::read_to_string(dir.join("prefix/wineserver-stopped"))
                .unwrap()
                .trim(),
            dir.join("prefix").display().to_string()
        );
        assert!(!dir.join("prefix-record.json").exists());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bounded_wait_kills_and_reaps_a_child() {
        let mut child = Command::new("/bin/sleep").arg("3").spawn().unwrap();
        let pid = child.id();
        assert!(
            wait_child_bounded(&mut child, Duration::from_millis(100))
                .unwrap()
                .is_none()
        );
        assert!(!process_alive(pid).unwrap());
    }

    #[test]
    fn waits_for_registry_written_after_wineboot_exits() {
        let home = test_home();
        let bottle = create(&home, "Delayed registry").unwrap();
        let wine = home.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        let wineboot = home.join("wineboot");
        fs::write(
            &wineboot,
            b"#!/bin/sh\n(sleep 0.3; touch \"$WINEPREFIX/system.reg\") &\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&wineboot).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineboot, permissions).unwrap();
        let result =
            ensure_prefix_with_wine_timeout(&home, &bottle.id, &wine, Duration::from_secs(8))
                .unwrap();
        assert!(result.initialized);
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        assert!(dir.join("prefix/system.reg").is_file());
        assert!(dir.join("prefix-record.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn runtime_requires_independent_wineboot() {
        let dir = test_home();
        fs::create_dir_all(&dir).unwrap();
        let wine = dir.join("wine");
        fs::write(&wine, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        assert!(!is_usable_wine(&wine));
        let wineboot = dir.join("wineboot");
        fs::write(&wineboot, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&wineboot).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineboot, permissions).unwrap();
        assert!(is_usable_wine(&wine));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bottle_writes_reject_symlinked_prefix_and_logs() {
        let home = test_home();
        let bottle = create(&home, "Protected").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let outside = home.join("outside");
        fs::create_dir(&outside).unwrap();
        let wine = home.join("fake-wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        let wineboot = home.join("wineboot");
        fs::write(&wineboot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        let mut permissions = fs::metadata(&wineboot).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wineboot, permissions).unwrap();
        let prefix = dir.join("prefix");
        fs::remove_dir(&prefix).unwrap();
        symlink(&outside, &prefix).unwrap();
        assert!(ensure_prefix_with_wine(&home, &bottle.id, &wine).is_err());
        assert!(!outside.join("system.reg").exists());
        fs::remove_file(&prefix).unwrap();
        fs::create_dir(&prefix).unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let logs = dir.join("logs");
        fs::remove_dir_all(&logs).unwrap();
        symlink(&outside, &logs).unwrap();
        let exe = home.join("setup.exe");
        fs::write(&exe, b"placeholder").unwrap();
        assert!(launch_with_wine(&home, &bottle.id, &exe, &wine).is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn same_bottle_writes_are_serialized() {
        let home = test_home();
        let bottle = create(&home, "Original").unwrap();
        let held = lock_bottle(&home, &bottle.id).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread_home = home.clone();
        let thread_id = bottle.id.clone();
        let worker = std::thread::spawn(move || {
            sender
                .send(rename_bottle(&thread_home, &thread_id, "Renamed"))
                .unwrap();
        });
        assert!(
            receiver
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        drop(held);
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap()
                .name,
            "Renamed"
        );
        worker.join().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn backup_refuses_tracked_running_windows_processes() {
        let home = test_home();
        let bottle = create(&home, "Running").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let backup = home
            .parent()
            .unwrap()
            .join(format!("{}.caskrun-bottle", Uuid::new_v4()));
        assert!(duplicate_bottle(&home, &bottle.id, None).is_err());
        assert!(export_bottle(&home, &bottle.id, &backup).is_err());
        assert!(!backup.exists());
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        let result = export_bottle(&home, &bottle.id, &backup).unwrap();
        assert_eq!(result.bottle.id, bottle.id);
        assert!(!backup.join("payload/active-processes.json").exists());
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(backup).unwrap();
    }

    #[test]
    fn stable_copy_rejects_file_replacement_and_symlink_swap() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let source = home.join("source.exe");
        let outside = home.join("outside.exe");
        fs::write(&source, b"original").unwrap();
        fs::write(&outside, b"outside").unwrap();
        let snapshot = fs::symlink_metadata(&source).unwrap();
        fs::write(&source, b"changed contents").unwrap();
        assert!(copy_stable_file(&source, &home.join("copy.exe"), &snapshot).is_err());
        assert!(!home.join("copy.exe").exists());
        let snapshot = fs::symlink_metadata(&source).unwrap();
        fs::remove_file(&source).unwrap();
        symlink(&outside, &source).unwrap();
        assert!(copy_stable_file(&source, &home.join("copy.exe"), &snapshot).is_err());
        assert!(!home.join("copy.exe").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn lists_only_own_wine_logs_and_reads_bounded_tail() {
        let home = test_home();
        let first = create(&home, "First").unwrap();
        let second = create(&home, "Second").unwrap();
        assert!(list_logs(&home, &first.id).unwrap().is_empty());
        let logs = bottle_dir(&home, &first.id).unwrap().join("logs");
        fs::create_dir(&logs).unwrap();
        let wineboot_name = format!("wineboot-{}.log", Uuid::new_v4());
        let launch_name = format!("launch-123-{}.log", Uuid::new_v4());
        let resolution_name = format!("resolution-{}.log", Uuid::new_v4());
        fs::write(logs.join(&wineboot_name), b"wineboot output\n").unwrap();
        fs::write(logs.join(&resolution_name), b"resolution changed\n").unwrap();
        let mut long_log = vec![b'a'; LOG_TAIL_BYTES as usize + 32];
        long_log.extend_from_slice(b"end of launch\n");
        fs::write(logs.join(&launch_name), &long_log).unwrap();
        fs::write(logs.join("private.log"), b"not a generated log").unwrap();

        let entries = list_logs(&home, &first.id).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .any(|item| item.name == wineboot_name && item.size == 16)
        );
        assert!(
            entries
                .iter()
                .any(|item| item.name == launch_name && item.size == long_log.len() as u64)
        );
        assert!(entries.iter().any(|item| item.name == resolution_name));
        let content = read_log(&home, &first.id, &launch_name).unwrap();
        assert!(content.truncated);
        assert_eq!(content.content.len(), LOG_TAIL_BYTES as usize);
        assert!(content.content.ends_with("end of launch\n"));
        let wineboot = read_log(&home, &first.id, &wineboot_name).unwrap();
        assert!(!wineboot.truncated);
        assert_eq!(wineboot.content, "wineboot output\n");
        let resolution = read_log(&home, &first.id, &resolution_name).unwrap();
        assert_eq!(resolution.content, "resolution changed\n");
        assert!(read_log(&home, &second.id, &launch_name).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn log_reader_rejects_traversal_and_symlinked_paths() {
        let home = test_home();
        let bottle = create(&home, "Logs").unwrap();
        let bottle_path = bottle_dir(&home, &bottle.id).unwrap();
        let logs = bottle_path.join("logs");
        fs::create_dir(&logs).unwrap();
        let outside = home.join("outside.txt");
        fs::write(&outside, b"private data").unwrap();
        let name = format!("wineboot-{}.log", Uuid::new_v4());
        symlink(&outside, logs.join(&name)).unwrap();
        assert!(list_logs(&home, &bottle.id).unwrap().is_empty());
        assert!(read_log(&home, &bottle.id, &name).is_err());
        for bad_name in [
            "../outside.txt",
            "wineboot-../../outside.log",
            "resolution-../../outside.log",
            "resolution-not-a-uuid.log",
            "private.log",
            "/etc/passwd",
        ] {
            assert!(read_log(&home, &bottle.id, bad_name).is_err());
        }
        fs::remove_dir_all(&logs).unwrap();
        symlink(&home, &logs).unwrap();
        assert!(list_logs(&home, &bottle.id).is_err());
        assert!(read_log(&home, &bottle.id, &name).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn duplicate_and_portable_backup_restore_prefix_and_launchers() {
        let home = test_home();
        let original = create(&home, "Game").unwrap();
        let source = bottle_dir(&home, &original.id).unwrap();
        let installed = source.join("prefix/drive_c/Game/game.exe");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, b"installed program").unwrap();
        fs::create_dir(source.join("prefix/dosdevices")).unwrap();
        symlink("../drive_c", source.join("prefix/dosdevices/c:")).unwrap();
        symlink("/", source.join("prefix/dosdevices/z:")).unwrap();
        let xdg = bottle_xdg::for_bottle(&source).unwrap();
        fs::write(xdg.desktop().join("private-note.txt"), b"kept in bottle").unwrap();
        fs::create_dir_all(source.join("prefix/drive_c/users/example")).unwrap();
        symlink(
            xdg.desktop(),
            source.join("prefix/drive_c/users/example/Desktop"),
        )
        .unwrap();
        fs::create_dir(source.join("logs")).unwrap();
        fs::write(source.join("logs/first.log"), b"log data").unwrap();
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).unwrap();
        let wine = runtime.join("wine");
        for name in ["wine", "wineboot", "wineserver"] {
            let executable = runtime.join(name);
            fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        save_json(
            &source.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: wine.display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        let cache = source.join("native-apps/Example.app/Contents/MacOS");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("WindowsApp"), b"generated native loader").unwrap();
        symlink("/", source.join("native-apps/untrusted-host-link")).unwrap();
        let external = home.join("external.exe");
        fs::write(&external, b"external program").unwrap();
        add_launcher(&home, &original.id, "Installed", &installed).unwrap();
        add_launcher(&home, &original.id, "External", &external).unwrap();

        let duplicate =
            duplicate_bottle_with_runtime(&home, &original.id, Some("Game copy"), Some(&wine))
                .unwrap();
        assert_ne!(duplicate.id, original.id);
        let duplicate_dir = bottle_dir(&home, &duplicate.id).unwrap();
        assert!(!duplicate_dir.join("native-apps").exists());
        assert_eq!(
            fs::read(duplicate_dir.join("prefix/drive_c/Game/game.exe")).unwrap(),
            b"installed program"
        );
        assert_eq!(
            fs::read_link(duplicate_dir.join("prefix/dosdevices/z:")).unwrap(),
            Path::new("/")
        );
        assert_eq!(
            fs::read_link(duplicate_dir.join("prefix/drive_c/users/example/Desktop")).unwrap(),
            Path::new("../../../../xdg/desktop")
        );
        assert_eq!(
            fs::read(duplicate_dir.join("xdg/desktop/private-note.txt")).unwrap(),
            b"kept in bottle"
        );
        assert!(
            fs::read_to_string(duplicate_dir.join("xdg/config/user-dirs.dirs"))
                .unwrap()
                .contains(&duplicate_dir.join("xdg/desktop").display().to_string())
        );
        assert!(duplicate_dir.join("prefix-record.json").is_file());
        let duplicate_apps = list_launchers(&home, &duplicate.id).unwrap();
        assert_eq!(duplicate_apps.len(), 2);
        assert_eq!(
            duplicate_apps[0].path,
            duplicate_dir
                .join("prefix/drive_c/Game/game.exe")
                .display()
                .to_string()
        );
        assert_eq!(duplicate_apps[1].path, external.display().to_string());
        fs::write(
            duplicate_dir.join("prefix/drive_c/Game/game.exe"),
            b"changed copy",
        )
        .unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"installed program");

        let backup = home
            .parent()
            .unwrap()
            .join(format!("{}.caskrun-bottle", Uuid::new_v4()));
        let exported =
            export_bottle_with_runtime(&home, &original.id, &backup, Some(&wine)).unwrap();
        assert_eq!(exported.external_launchers, 1);
        assert!(backup.join("manifest.json").is_file());
        assert!(!backup.join("payload/prefix-record.json").exists());
        assert!(!backup.join("payload/bottle.json").exists());
        assert!(!backup.join("payload/native-apps").exists());
        assert_eq!(
            fs::read(cache.join("WindowsApp")).unwrap(),
            b"generated native loader"
        );
        // Imported archives cannot inject host executable cache either.
        fs::create_dir(backup.join("payload/native-apps")).unwrap();
        assert!(
            import_bottle(&home, &backup, None)
                .err()
                .unwrap()
                .contains("reserved metadata: native-apps")
        );
        fs::remove_dir(backup.join("payload/native-apps")).unwrap();
        let restored_home = test_home();
        let restored = import_bottle(&restored_home, &backup, Some("Restored game")).unwrap();
        assert_eq!(restored.external_launchers, 1);
        assert_eq!(restored.bottle.name, "Restored game");
        assert_ne!(restored.bottle.id, original.id);
        let restored_dir = bottle_dir(&restored_home, &restored.bottle.id).unwrap();
        assert_eq!(
            fs::read(restored_dir.join("prefix/drive_c/Game/game.exe")).unwrap(),
            b"installed program"
        );
        assert_eq!(
            fs::read(restored_dir.join("xdg/desktop/private-note.txt")).unwrap(),
            b"kept in bottle"
        );
        assert_eq!(
            fs::read_link(restored_dir.join("prefix/drive_c/users/example/Desktop")).unwrap(),
            Path::new("../../../../xdg/desktop")
        );
        assert!(
            fs::read_to_string(restored_dir.join("xdg/config/user-dirs.dirs"))
                .unwrap()
                .contains(&restored_dir.join("xdg/desktop").display().to_string())
        );
        assert!(!restored_dir.join("prefix-record.json").exists());
        let restored_apps = list_launchers(&restored_home, &restored.bottle.id).unwrap();
        assert_eq!(
            restored_apps[0].path,
            restored_dir
                .join("prefix/drive_c/Game/game.exe")
                .display()
                .to_string()
        );
        assert_eq!(restored_apps[1].path, external.display().to_string());
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(restored_home).unwrap();
        fs::remove_dir_all(backup).unwrap();
    }

    #[test]
    fn backups_reject_overwrite_traversal_and_symlinked_metadata() {
        let home = test_home();
        let bottle = create(&home, "Secure").unwrap();
        let source = bottle_dir(&home, &bottle.id).unwrap();
        let inside = source.join("nested.caskrun-bottle");
        assert!(export_bottle(&home, &bottle.id, &inside).is_err());
        assert!(!inside.exists());
        let backup = home
            .parent()
            .unwrap()
            .join(format!("{}.caskrun-bottle", Uuid::new_v4()));
        export_bottle(&home, &bottle.id, &backup).unwrap();
        assert!(export_bottle(&home, &bottle.id, &backup).is_err());
        let manifest_path = backup.join("manifest.json");
        let mut manifest: BackupManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.launchers.push(PortableLauncher {
            id: Uuid::new_v4().to_string(),
            name: "Escape".into(),
            created_at: 0,
            hidden_from_home: false,
            saved_arguments: Vec::new(),
            path: PortableLauncherPath::PrefixRelative("../escape.exe".into()),
        });
        save_json(&manifest_path, &manifest).unwrap();
        let other = test_home();
        assert!(import_bottle(&other, &backup, None).is_err());
        assert!(list(&other).unwrap().is_empty());
        manifest.launchers.clear();
        save_json(&manifest_path, &manifest).unwrap();
        symlink("/", backup.join("payload/prefix/escape")).unwrap();
        assert!(import_bottle(&other, &backup, None).is_err());
        assert!(list(&other).unwrap().is_empty());
        fs::remove_file(backup.join("payload/prefix/escape")).unwrap();
        symlink("../../../outside", backup.join("payload/prefix/escape")).unwrap();
        assert!(import_bottle(&other, &backup, None).is_err());
        assert!(list(&other).unwrap().is_empty());
        fs::remove_file(backup.join("payload/prefix/escape")).unwrap();
        symlink("prefix", backup.join("payload/alias")).unwrap();
        assert!(import_bottle(&other, &backup, None).is_err());
        assert!(list(&other).unwrap().is_empty());
        fs::remove_file(backup.join("payload/alias")).unwrap();
        let metadata = source.join("bottle.json");
        let saved_metadata = source.join("saved-metadata.json");
        fs::rename(&metadata, &saved_metadata).unwrap();
        symlink("saved-metadata.json", &metadata).unwrap();
        assert!(bottle_dir(&home, &bottle.id).is_err());
        fs::remove_file(&metadata).unwrap();
        fs::rename(saved_metadata, metadata).unwrap();
        let fake_crossover = home.join("CrossOver.app/Data");
        assert!(create(&fake_crossover, "Never create").is_err());
        assert!(!fake_crossover.exists());
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(backup).unwrap();
    }

    // Steam game routing tests use only synthetic manifests and saved paths.
    fn synthetic_steam_game(dir: &Path) -> PathBuf {
        let root = dir.join("prefix/drive_c/Program Files (x86)/Steam");
        fs::create_dir_all(root.join("steamapps/common/Spacewar")).unwrap();
        let executable = root.join("Steam.exe");
        fs::write(&executable, b"synthetic Steam path, never executed").unwrap();
        fs::write(
            root.join("steamapps/appmanifest_480.acf"),
            "\"AppState\" { \"appid\" \"480\" \"name\" \"Spacewar\" \"installdir\" \"Spacewar\" \"StateFlags\" \"4\" }",
        )
        .unwrap();
        executable
    }

    #[test]
    fn steam_game_launcher_requires_matching_saved_steam_in_same_bottle() {
        let home = test_home();
        let bottle = create(&home, "Steam library").unwrap();
        let other = create(&home, "Other bottle").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let steam = synthetic_steam_game(&dir);
        let other_launcher = add_launcher(&home, &other.id, "Steam", &steam).unwrap();
        assert!(
            steam_game_launcher(&home, &bottle.id, 480)
                .unwrap_err()
                .contains("Save the Steam app in this bottle")
        );
        let wrong_path = dir.join("prefix/drive_c/unrelated.exe");
        fs::write(&wrong_path, b"synthetic unrelated path").unwrap();
        add_launcher(&home, &bottle.id, "Steam", &wrong_path).unwrap();
        assert!(steam_game_launcher(&home, &bottle.id, 480).is_err());

        let saved = add_launcher(&home, &bottle.id, "My Steam", &steam).unwrap();
        let arguments = vec!["-silent".to_owned()];
        set_launcher_arguments(&home, &bottle.id, &saved.id, &arguments).unwrap();
        let (selected, game) = steam_game_launcher(&home, &bottle.id, 480).unwrap();
        assert_eq!(selected.id, saved.id);
        assert_ne!(selected.id, other_launcher.id);
        assert_eq!(selected.saved_arguments, arguments);
        assert_eq!(game.id, "steam:480");
        assert_eq!(game.launch_arguments(), ["-applaunch", "480"]);
        assert_eq!(list_launchers(&home, &bottle.id).unwrap().len(), 2);
        assert_eq!(list_launchers(&home, &other.id).unwrap().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn steam_game_launcher_rejects_removed_manifest_before_selecting_steam() {
        let home = test_home();
        let bottle = create(&home, "Steam library").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let steam = synthetic_steam_game(&dir);
        let saved = add_launcher(&home, &bottle.id, "Steam", &steam).unwrap();
        assert_eq!(
            steam_game_launcher(&home, &bottle.id, 480).unwrap().0.id,
            saved.id
        );
        fs::remove_file(
            steam
                .parent()
                .unwrap()
                .join("steamapps/appmanifest_480.acf"),
        )
        .unwrap();
        assert!(
            steam_game_launcher(&home, &bottle.id, 480)
                .unwrap_err()
                .contains("no longer installed")
        );
        assert_eq!(list_launchers(&home, &bottle.id).unwrap().len(), 1);
        assert!(steam.is_file());
        fs::remove_dir_all(home).unwrap();
    }
}
