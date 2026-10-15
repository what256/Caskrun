//! Read-only evidence that no process still owns files in an exact bottle.
//!
//! A wineserver can exit before orphaned Wine clients release their prefix or
//! inherited logs. Inspect current-UID open handles instead of walking a game
//! installation tree, and never infer ownership from saved PIDs or app names.
//! After normal server shutdown, a narrow fallback can stop only generated
//! native loaders with current runtime, bottle-file, and audit-token proofs.
//! Unaliased Wine, Steam, and unrelated file holders are never signal targets.

use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;

const INSPECTOR: &str = "/usr/sbin/lsof";
const MAX_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ERROR_BYTES: u64 = 64 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY_DELAY: Duration = Duration::from_millis(100);
const CHECK_FAILURE: &str = "Caskrun could not verify this bottle's remaining processes. Close its Windows apps and try Quit again; the bottle was preserved.";

unsafe extern "C" {
    fn geteuid() -> u32;
}

struct InspectionChild(Child);
impl Drop for InspectionChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) && self.0.kill().is_ok() {
            let _ = self.0.wait();
        }
    }
}

struct Capture {
    output: File,
    errors: File,
    paths: [PathBuf; 2],
}

impl Capture {
    fn create(dir: &Path) -> Result<Self, String> {
        let token = Uuid::new_v4();
        let paths = [
            dir.join(format!(".caskrun-process-check-{token}.out")),
            dir.join(format!(".caskrun-process-check-{token}.err")),
        ];
        let open = |path: &Path| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(path)
        };
        let output = open(&paths[0]).map_err(|_| CHECK_FAILURE.to_owned())?;
        let errors = match open(&paths[1]) {
            Ok(file) => file,
            Err(_) => {
                let _ = fs::remove_file(&paths[0]);
                return Err(CHECK_FAILURE.into());
            }
        };
        Ok(Self {
            output,
            errors,
            paths,
        })
    }

    fn too_large(&self) -> Result<bool, String> {
        Ok(
            self.output.metadata().map_err(|_| CHECK_FAILURE)?.len() > MAX_OUTPUT_BYTES
                || self.errors.metadata().map_err(|_| CHECK_FAILURE)?.len() > MAX_ERROR_BYTES,
        )
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

/// Caller holds the bottle lock and checks its prefix identity before and after.
/// This additionally retains the bottle directory's inode throughout inspection.
pub(super) fn wait_until_idle(dir: &Path, timeout: Duration) -> Result<(), String> {
    wait_until_idle_with_inspector(dir, timeout, Path::new(INSPECTOR))
}

/// One bounded, read-only current-UID ownership snapshot. An inspection failure
/// is unknown activity, never evidence that the bottle is idle.
pub(super) fn owner_count(dir: &Path) -> Result<usize, String> {
    let (guard, snapshot) = open_stable_directory(dir)?;
    let dir = dir.canonicalize().map_err(|_| CHECK_FAILURE)?;
    bottle_control::assert_same_prefix(&dir, &guard, &snapshot)?;
    let count = inspect_owners(&dir, Path::new(INSPECTOR), PROBE_TIMEOUT)?;
    bottle_control::assert_same_prefix(&dir, &guard, &snapshot)?;
    Ok(count)
}

fn wait_until_idle_with_inspector(
    dir: &Path,
    timeout: Duration,
    inspector: &Path,
) -> Result<(), String> {
    let (guard, snapshot) = open_stable_directory(dir)?;
    let canonical_dir = dir.canonicalize().map_err(|_| CHECK_FAILURE.to_owned())?;
    let deadline = Instant::now() + timeout;
    let mut last_owners = None;
    loop {
        cancellation::check()?;
        bottle_control::assert_same_prefix(dir, &guard, &snapshot)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(last_owners
                .map(remaining_owner_error)
                .unwrap_or_else(|| CHECK_FAILURE.to_owned()));
        }
        let owners = match inspect_owners(
            &canonical_dir,
            inspector,
            remaining.min(PROBE_TIMEOUT).max(Duration::from_millis(1)),
        ) {
            Ok(owners) => owners,
            Err(_) if Instant::now() >= deadline && last_owners.is_some() => {
                return Err(remaining_owner_error(last_owners.unwrap()));
            }
            Err(error) => return Err(error),
        };
        bottle_control::assert_same_prefix(dir, &guard, &snapshot)?;
        if owners == 0 {
            return Ok(());
        }
        last_owners = Some(owners);
        if Instant::now() >= deadline {
            return Err(remaining_owner_error(owners));
        }
        std::thread::sleep(RETRY_DELAY.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn remaining_owner_error(owners: usize) -> String {
    format!(
        "{owners} process(es) still use this bottle's files after Wine's server exited. Close its remaining Windows apps and try Quit again. Caskrun did not signal unverified processes."
    )
}

fn inspect_owners(dir: &Path, inspector: &Path, timeout: Duration) -> Result<usize, String> {
    let (bytes, inspector_pid) = inspection_bytes(dir, inspector, timeout, "-F0pn")?;
    if bytes.is_empty() {
        return Ok(0);
    }
    parse_owners(&bytes, dir, &[std::process::id(), inspector_pid])
}

fn inspection_bytes(
    dir: &Path,
    inspector: &Path,
    timeout: Duration,
    fields: &str,
) -> Result<(Vec<u8>, u32), String> {
    let mut capture = Capture::create(dir)?;
    let stdout = capture
        .output
        .try_clone()
        .map_err(|_| CHECK_FAILURE.to_owned())?;
    let stderr = capture
        .errors
        .try_clone()
        .map_err(|_| CHECK_FAILURE.to_owned())?;
    // Numeric UID does not depend on HOME, USER, or LOGNAME in test storage.
    let uid = unsafe { geteuid() }.to_string();
    let child = Command::new(inspector)
        .args(["-nP", fields, "-a", "-u", &uid])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|_| CHECK_FAILURE.to_owned())?;
    let mut owned_child = InspectionChild(child);
    let child = &mut owned_child.0;
    let inspector_pid = child.id();
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| CHECK_FAILURE.to_owned())? {
            break status;
        }
        if capture.too_large()? || Instant::now() >= deadline || cancellation::is_requested() {
            // This still-unreaped Child belongs to this invocation. Its PID
            // cannot be reused; never signal a PID discovered by lsof.
            if child.kill().is_err() && child.try_wait().ok().flatten().is_none() {
                return Err(CHECK_FAILURE.into());
            }
            child.wait().map_err(|_| CHECK_FAILURE.to_owned())?;
            cancellation::check()?;
            return Err(CHECK_FAILURE.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if capture.too_large()? || capture.errors.metadata().map_err(|_| CHECK_FAILURE)?.len() != 0 {
        return Err(CHECK_FAILURE.into());
    }
    let mut bytes = Vec::new();
    capture
        .output
        .seek(SeekFrom::Start(0))
        .map_err(|_| CHECK_FAILURE.to_owned())?;
    capture
        .output
        .read_to_end(&mut bytes)
        .map_err(|_| CHECK_FAILURE.to_owned())?;
    if !status.success() {
        // Exit 1 with no output is lsof's well-defined no-matching-files case.
        return if status.code() == Some(1) && bytes.is_empty() {
            Ok((Vec::new(), inspector_pid))
        } else {
            Err(CHECK_FAILURE.into())
        };
    }
    if bytes.is_empty() {
        return Err(CHECK_FAILURE.into());
    }
    Ok((bytes, inspector_pid))
}

fn parse_owners(bytes: &[u8], dir: &Path, excluded: &[u32]) -> Result<usize, String> {
    if bytes.is_empty() || !bytes.ends_with(b"\0") && !bytes.ends_with(b"\n") {
        return Err(CHECK_FAILURE.into());
    }
    let base = dir.as_os_str().as_bytes();
    let mut owners = std::collections::BTreeSet::new();
    let mut current_pid = None;
    let mut saw_process = false;
    let mut saw_name = false;
    for field in bytes.split(|byte| *byte == 0) {
        let field = field.strip_prefix(b"\n").unwrap_or(field);
        if field.is_empty() || field == b"\n" {
            continue;
        }
        match field[0] {
            b'p' => {
                let value = std::str::from_utf8(&field[1..]).map_err(|_| CHECK_FAILURE)?;
                let pid = value.parse::<u32>().map_err(|_| CHECK_FAILURE)?;
                if pid == 0 {
                    return Err(CHECK_FAILURE.into());
                }
                current_pid = Some(pid);
                saw_process = true;
            }
            b'n' => {
                saw_name = true;
                let pid = current_pid.ok_or(CHECK_FAILURE)?;
                let path = &field[1..];
                let in_bottle = path == base
                    || path
                        .strip_prefix(base)
                        .is_some_and(|suffix| suffix.starts_with(b"/"));
                if in_bottle && !excluded.contains(&pid) {
                    owners.insert(pid);
                }
            }
            // lsof emits file-descriptor identifiers even when only process
            // and name fields are requested. Their contents are not ownership.
            b'f' => {
                if current_pid.is_none() {
                    return Err(CHECK_FAILURE.into());
                }
            }
            _ => return Err(CHECK_FAILURE.into()),
        }
    }
    if !saw_process || !saw_name {
        return Err(CHECK_FAILURE.into());
    }
    Ok(owners.len())
}

#[derive(Clone, Debug, Default)]
struct OpenFile {
    descriptor: String,
    kind: String,
    path: Option<PathBuf>,
    device: Option<u64>,
    inode: Option<u64>,
}
#[derive(Debug)]
struct ProcessFiles {
    pid: u32,
    files: Vec<OpenFile>,
}

/// NUL-delimited machine output preserves arbitrary Unix path bytes. All
/// identity-bearing fields must exist for a file to authorize a signal.
fn parse_inventory(bytes: &[u8]) -> Result<Vec<ProcessFiles>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(b"\0") && !bytes.ends_with(b"\n") {
        return Err(CHECK_FAILURE.into());
    }
    let mut processes: Vec<ProcessFiles> = Vec::new();
    let mut file: Option<OpenFile> = None;
    let mut seen_pids = std::collections::BTreeSet::new();
    let flush =
        |processes: &mut Vec<ProcessFiles>, file: &mut Option<OpenFile>| -> Result<(), String> {
            if let Some(file) = file.take() {
                processes.last_mut().ok_or(CHECK_FAILURE)?.files.push(file);
            }
            Ok(())
        };
    for field in bytes.split(|b| *b == 0) {
        let field = field.strip_prefix(b"\n").unwrap_or(field);
        if field.is_empty() || field == b"\n" {
            continue;
        }
        match field[0] {
            b'p' => {
                flush(&mut processes, &mut file)?;
                let pid = std::str::from_utf8(&field[1..])
                    .map_err(|_| CHECK_FAILURE)?
                    .parse::<u32>()
                    .map_err(|_| CHECK_FAILURE)?;
                if pid == 0 || !seen_pids.insert(pid) {
                    return Err(CHECK_FAILURE.into());
                }
                processes.push(ProcessFiles {
                    pid,
                    files: Vec::new(),
                });
            }
            b'f' => {
                if processes.is_empty() {
                    return Err(CHECK_FAILURE.into());
                }
                flush(&mut processes, &mut file)?;
                file = Some(OpenFile {
                    descriptor: std::str::from_utf8(&field[1..])
                        .map_err(|_| CHECK_FAILURE)?
                        .into(),
                    ..OpenFile::default()
                });
            }
            b'n' => {
                file.as_mut().ok_or(CHECK_FAILURE)?.path =
                    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&field[1..])))
            }
            b't' => {
                file.as_mut().ok_or(CHECK_FAILURE)?.kind = std::str::from_utf8(&field[1..])
                    .map_err(|_| CHECK_FAILURE)?
                    .into()
            }
            b'D' => {
                let value = std::str::from_utf8(&field[1..]).map_err(|_| CHECK_FAILURE)?;
                file.as_mut().ok_or(CHECK_FAILURE)?.device = value
                    .strip_prefix("0x")
                    .and_then(|v| u64::from_str_radix(v, 16).ok())
                    .or_else(|| value.parse().ok());
            }
            b'i' => {
                file.as_mut().ok_or(CHECK_FAILURE)?.inode = std::str::from_utf8(&field[1..])
                    .map_err(|_| CHECK_FAILURE)?
                    .parse()
                    .ok()
            }
            _ => return Err(CHECK_FAILURE.into()),
        }
    }
    flush(&mut processes, &mut file)?;
    if processes.is_empty() || processes.iter().any(|p| p.files.is_empty()) {
        return Err(CHECK_FAILURE.into());
    }
    Ok(processes)
}

fn inventory(dir: &Path, timeout: Duration) -> Result<Vec<ProcessFiles>, String> {
    let (bytes, inspector_pid) = inspection_bytes(dir, Path::new(INSPECTOR), timeout, "-F0pftnDi")?;
    let mut processes = parse_inventory(&bytes)?;
    processes.retain(|p| p.pid != std::process::id() && p.pid != inspector_pid);
    Ok(processes)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AuditToken {
    values: [u32; 8],
}

#[repr(C)]
#[derive(Default)]
struct BsdInfo {
    flags: u32,
    status: u32,
    xstatus: u32,
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    reserved: u32,
    command: [u8; 16],
    name: [u8; 32],
    nfiles: u32,
    pgid: u32,
    job_count: u32,
    terminal: u32,
    terminal_pgid: u32,
    nice: i32,
    start_seconds: u64,
    start_microseconds: u64,
}

unsafe extern "C" {
    static mach_task_self_: u32;
    fn task_name_for_pid(task: u32, pid: i32, port: *mut u32) -> i32;
    fn task_info(port: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    fn mach_port_deallocate(task: u32, port: u32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        size: i32,
    ) -> i32;
    fn proc_pidpath_audittoken(
        token: *const AuditToken,
        buffer: *mut std::ffi::c_void,
        size: u32,
    ) -> i32;
    fn proc_signal_with_audittoken(token: *const AuditToken, signal: i32) -> i32;
}

struct TaskNamePort(u32);
impl Drop for TaskNamePort {
    fn drop(&mut self) {
        unsafe {
            mach_port_deallocate(mach_task_self_, self.0);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessIdentity {
    token: AuditToken,
    birth: (u64, u64),
    executable: PathBuf,
}

fn current_identity(pid: u32) -> Option<ProcessIdentity> {
    let pid = i32::try_from(pid).ok().filter(|pid| *pid > 1)?;
    let mut port = 0;
    if unsafe { task_name_for_pid(mach_task_self_, pid, &mut port) } != 0 || port == 0 {
        return None;
    }
    let port = TaskNamePort(port);
    let read_token = || {
        let mut token = AuditToken { values: [0; 8] };
        let mut count = 8;
        (unsafe { task_info(port.0, 15, token.values.as_mut_ptr().cast(), &mut count) } == 0
            && count == 8)
            .then_some(token)
    };
    let token = read_token()?;
    let uid = unsafe { geteuid() };
    if token.values[5] != pid as u32 || token.values[1] != uid || token.values[3] != uid {
        return None;
    }
    let mut info = BsdInfo::default();
    if unsafe {
        proc_pidinfo(
            pid,
            3,
            0,
            (&mut info as *mut BsdInfo).cast(),
            std::mem::size_of::<BsdInfo>() as i32,
        )
    } != std::mem::size_of::<BsdInfo>() as i32
        || info.pid != pid as u32
        || info.uid != uid
        || info.ruid != uid
        || info.status == 5
        || info.start_seconds == 0
    {
        return None;
    }
    let mut path = [0_u8; 4096];
    let length =
        unsafe { proc_pidpath_audittoken(&token, path.as_mut_ptr().cast(), path.len() as u32) };
    if length <= 0 || length as usize >= path.len() || read_token()? != token {
        return None;
    }
    let executable = PathBuf::from(std::ffi::OsStr::from_bytes(&path[..length as usize]));
    if !executable.is_absolute() {
        return None;
    }
    Some(ProcessIdentity {
        token,
        birth: (info.start_seconds, info.start_microseconds),
        executable,
    })
}

/// The kernel checks the token's PID generation. Never fall back to kill(pid)
/// on an unavailable API, a dead token, or changed process identity.
fn signal_token(identity: &ProcessIdentity, signal: i32) -> Result<bool, String> {
    match unsafe { proc_signal_with_audittoken(&identity.token, signal) } {
        0 => Ok(true),
        3 => Ok(false), // ESRCH: that execution identity already exited.
        _ => Err(CHECK_FAILURE.into()),
    }
}

struct MappedAlias<'a> {
    loader: &'a Path,
    loader_dev: u64,
    loader_ino: u64,
    ntdll: &'a Path,
    ntdll_dev: u64,
    ntdll_ino: u64,
}
impl<'a> From<&'a native_app_identity::ShutdownAlias> for MappedAlias<'a> {
    fn from(alias: &'a native_app_identity::ShutdownAlias) -> Self {
        Self {
            loader: &alias.loader_path,
            loader_dev: alias.loader_dev,
            loader_ino: alias.loader_ino,
            ntdll: &alias.ntdll_path,
            ntdll_dev: alias.ntdll_dev,
            ntdll_ino: alias.ntdll_ino,
        }
    }
}

fn mapped_file(file: &OpenFile, path: &Path, device: u64, inode: u64) -> bool {
    file.descriptor == "txt"
        && file.kind == "REG"
        && file.path.as_deref() == Some(path)
        && file.device == Some(device)
        && file.inode == Some(inode)
}
fn other_bottle_path(path: &Path, dir: &Path) -> bool {
    let Some(root) = dir.parent() else {
        return true;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    let name = component.as_os_str();
    name != dir.file_name().unwrap_or_default()
        && name
            .to_str()
            .is_some_and(|name| Uuid::parse_str(name).is_ok())
}
fn launch_log_name(path: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .and_then(|v| v.to_str())
        .and_then(|v| v.strip_suffix(".log"))
    else {
        return false;
    };
    if let Some(id) = name.strip_prefix("wineboot-") {
        return Uuid::parse_str(id).is_ok();
    }
    name.strip_prefix("launch-")
        .and_then(|v| v.split_once('-'))
        .is_some_and(|(time, id)| time.parse::<u64>().is_ok() && Uuid::parse_str(id).is_ok())
}

/// Require both mapped code inodes and an independently checked current bottle
/// log/cwd vnode. Merely opening a native loader or NTDLL is never enough.
fn scope_file(
    files: &ProcessFiles,
    dir: &Path,
    alias: MappedAlias<'_>,
    prefix: &fs::Metadata,
) -> Result<Option<(File, PathBuf, fs::Metadata)>, String> {
    if files
        .files
        .iter()
        .filter_map(|f| f.path.as_deref())
        .any(|p| other_bottle_path(p, dir))
        || !files
            .files
            .iter()
            .any(|f| mapped_file(f, alias.loader, alias.loader_dev, alias.loader_ino))
        || !files
            .files
            .iter()
            .any(|f| mapped_file(f, alias.ntdll, alias.ntdll_dev, alias.ntdll_ino))
    {
        return Ok(None);
    }
    for file in &files.files {
        let Some(path) = file.path.as_deref() else {
            continue;
        };
        if file.descriptor == "cwd"
            && file.kind == "DIR"
            && path == dir.join("prefix")
            && file.device == Some(prefix.dev())
            && file.inode == Some(prefix.ino())
        {
            let (guard, current) = open_stable_directory(path).map_err(|_| CHECK_FAILURE)?;
            if !same_identity(prefix, &current) {
                return Err(CHECK_FAILURE.into());
            }
            return Ok(Some((guard, path.to_path_buf(), current)));
        }
        if file.kind != "REG"
            || file.descriptor == "txt"
            || path.parent() != Some(dir.join("logs").as_path())
            || !launch_log_name(path)
        {
            continue;
        }
        let (_logs, _) = open_stable_directory(&dir.join("logs")).map_err(|_| CHECK_FAILURE)?;
        let guard = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | 0x4) // Darwin O_NONBLOCK: a swapped FIFO cannot stall Quit.
            .open(path)
            .map_err(|_| CHECK_FAILURE)?;
        let current = guard.metadata().map_err(|_| CHECK_FAILURE)?;
        if current.is_file()
            && current.uid() == unsafe { geteuid() }
            && file.device == Some(current.dev())
            && file.inode == Some(current.ino())
        {
            return Ok(Some((guard, path.to_path_buf(), current)));
        }
    }
    Ok(None)
}

struct NativeOwner {
    identity: ProcessIdentity,
    alias: native_app_identity::ShutdownAlias,
    loader_snapshot: fs::Metadata,
    ntdll_snapshot: fs::Metadata,
    _scope_guard: File,
    scope_path: PathBuf,
    scope_snapshot: fs::Metadata,
}

fn prove_owner(
    dir: &Path,
    wine: &Path,
    identity: &ProcessIdentity,
    files: &ProcessFiles,
    prefix: &fs::Metadata,
) -> Result<Option<NativeOwner>, String> {
    if current_identity(files.pid).as_ref() != Some(identity) {
        return Ok(None);
    }
    let Some(alias) = native_app_identity::validate_shutdown_alias(dir, wine, &identity.executable)
        .map_err(|_| CHECK_FAILURE)?
    else {
        return Ok(None);
    };
    let Some((scope_guard, scope_path, scope_snapshot)) =
        scope_file(files, dir, (&alias).into(), prefix)?
    else {
        return Ok(None);
    };
    let loader_snapshot = fs::symlink_metadata(&alias.loader_path).map_err(|_| CHECK_FAILURE)?;
    let ntdll_snapshot = fs::symlink_metadata(&alias.ntdll_path).map_err(|_| CHECK_FAILURE)?;
    if loader_snapshot.dev() != alias.loader_dev
        || loader_snapshot.ino() != alias.loader_ino
        || ntdll_snapshot.dev() != alias.ntdll_dev
        || ntdll_snapshot.ino() != alias.ntdll_ino
        || current_identity(files.pid).as_ref() != Some(identity)
    {
        return Ok(None);
    }
    Ok(Some(NativeOwner {
        identity: identity.clone(),
        alias,
        loader_snapshot,
        ntdll_snapshot,
        _scope_guard: scope_guard,
        scope_path,
        scope_snapshot,
    }))
}

fn unchanged_owner(
    dir: &Path,
    wine: &Path,
    owner: &NativeOwner,
    files: &ProcessFiles,
    prefix: &fs::Metadata,
) -> Result<bool, String> {
    let Some(current) = prove_owner(dir, wine, &owner.identity, files, prefix)? else {
        return Ok(false);
    };
    // Logs can grow while apps finish, so retain only their vnode identity.
    Ok(owner.alias.loader_path == current.alias.loader_path
        && owner.alias.ntdll_path == current.alias.ntdll_path
        && same_snapshot(&owner.loader_snapshot, &current.loader_snapshot)
        && same_snapshot(&owner.ntdll_snapshot, &current.ntdll_snapshot)
        && owner.scope_path == current.scope_path
        && owner.scope_snapshot.dev() == current.scope_snapshot.dev()
        && owner.scope_snapshot.ino() == current.scope_snapshot.ino())
}

const STEAM_OWNER_FAILURE: &str = "Caskrun could not verify one running Steam client in this bottle. Open Steam from this bottle before requesting Quit.";

struct SteamCodeFile {
    path: PathBuf,
    guard: File,
    snapshot: fs::Metadata,
}

impl SteamCodeFile {
    fn open(path: PathBuf) -> Result<Self, String> {
        let before = fs::symlink_metadata(&path).map_err(|_| STEAM_OWNER_FAILURE)?;
        if !before.is_file() || path.canonicalize().ok().as_ref() != Some(&path) {
            return Err(STEAM_OWNER_FAILURE.into());
        }
        let guard = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | 0x4)
            .open(&path)
            .map_err(|_| STEAM_OWNER_FAILURE)?;
        let file = Self {
            path,
            guard,
            snapshot: before,
        };
        file.revalidate()?;
        Ok(file)
    }

    fn revalidate(&self) -> Result<(), String> {
        let current = fs::symlink_metadata(&self.path).map_err(|_| STEAM_OWNER_FAILURE)?;
        let held = self.guard.metadata().map_err(|_| STEAM_OWNER_FAILURE)?;
        if !current.is_file()
            || !same_snapshot(&self.snapshot, &current)
            || !same_snapshot(&self.snapshot, &held)
        {
            return Err(STEAM_OWNER_FAILURE.into());
        }
        Ok(())
    }

    fn mapped(&self, files: &ProcessFiles) -> bool {
        files
            .files
            .iter()
            .any(|file| mapped_file(file, &self.path, self.snapshot.dev(), self.snapshot.ino()))
    }
}

/// Opaque, read-only evidence for a raw Steam client, never a signal target.
/// The caller holds the bottle lock and its initialized prefix inode. Saved
/// PIDs and process names are not ownership evidence. All three mapped code
/// vnodes, a current bottle log/cwd and the kernel execution identity must agree.
pub(super) struct SteamClientOwner {
    pid: u32,
    identity: ProcessIdentity,
    code: [SteamCodeFile; 3],
    _scope_guard: File,
    scope_path: PathBuf,
    scope_snapshot: fs::Metadata,
}

fn steam_scope(
    dir: &Path,
    prefix: &fs::Metadata,
    code: &[SteamCodeFile; 3],
    files: &ProcessFiles,
) -> Result<Option<(File, PathBuf, fs::Metadata)>, String> {
    if !code[2].mapped(files) {
        return Ok(None);
    }
    scope_file(
        files,
        dir,
        MappedAlias {
            loader: &code[0].path,
            loader_dev: code[0].snapshot.dev(),
            loader_ino: code[0].snapshot.ino(),
            ntdll: &code[1].path,
            ntdll_dev: code[1].snapshot.dev(),
            ntdll_ino: code[1].snapshot.ino(),
        },
        prefix,
    )
}

fn steam_code_paths(dir: &Path, wine: &Path, client: &Path) -> Result<[PathBuf; 3], String> {
    let root = bottle_graphics::selected_runtime_root(dir, wine)?
        .canonicalize()
        .map_err(|_| STEAM_OWNER_FAILURE)?;
    Ok([
        root.join("lib/wine/x86_64-unix/Steam"),
        root.join("lib/wine/x86_64-unix/ntdll.so"),
        client.to_path_buf(),
    ])
}

pub(super) fn steam_client_owner(
    dir: &Path,
    wine: &Path,
    client: &Path,
    prefix: &fs::Metadata,
) -> Result<SteamClientOwner, String> {
    let code = steam_code_paths(dir, wine, client)?.map(SteamCodeFile::open);
    let [loader, ntdll, image] = code;
    let code = [loader?, ntdll?, image?];
    if !is_executable(&code[0].path) {
        return Err(STEAM_OWNER_FAILURE.into());
    }
    let discovery = inventory(dir, PROBE_TIMEOUT)?;
    let identities: Vec<_> = discovery
        .iter()
        .filter(|files| code.iter().all(|file| file.mapped(files)))
        .filter_map(|files| current_identity(files.pid).map(|identity| (files.pid, identity)))
        .filter(|(_, identity)| identity.executable == code[0].path)
        .collect();
    let proof = inventory(dir, PROBE_TIMEOUT)?;
    let mut matches = Vec::new();
    for (pid, identity) in identities {
        let Some(files) = proof.iter().find(|files| files.pid == pid) else {
            continue;
        };
        if current_identity(pid).as_ref() != Some(&identity) {
            continue;
        }
        if let Some(scope) = steam_scope(dir, prefix, &code, files)? {
            matches.push((pid, identity, scope));
        }
    }
    if matches.len() != 1 {
        return Err(STEAM_OWNER_FAILURE.into());
    }
    for file in &code {
        file.revalidate()?;
    }
    let (pid, identity, (scope_guard, scope_path, scope_snapshot)) = matches.pop().unwrap();
    Ok(SteamClientOwner {
        pid,
        identity,
        code,
        _scope_guard: scope_guard,
        scope_path,
        scope_snapshot,
    })
}

impl SteamClientOwner {
    /// Recheck immediately before spawning the shutdown request. No files are
    /// repaired and no process is signalled, including when proof becomes stale.
    pub(super) fn revalidate(
        &self,
        dir: &Path,
        wine: &Path,
        client: &Path,
        prefix: &fs::Metadata,
    ) -> Result<(), String> {
        let paths = steam_code_paths(dir, wine, client)?;
        if self
            .code
            .iter()
            .zip(paths)
            .any(|(file, path)| file.path != path)
            || current_identity(self.pid).as_ref() != Some(&self.identity)
        {
            return Err(STEAM_OWNER_FAILURE.into());
        }
        for file in &self.code {
            file.revalidate()?;
        }
        let current = inventory(dir, PROBE_TIMEOUT)?;
        let files = current
            .iter()
            .find(|files| files.pid == self.pid)
            .ok_or(STEAM_OWNER_FAILURE)?;
        let Some((_guard, path, snapshot)) = steam_scope(dir, prefix, &self.code, files)? else {
            return Err(STEAM_OWNER_FAILURE.into());
        };
        if path != self.scope_path
            || snapshot.dev() != self.scope_snapshot.dev()
            || snapshot.ino() != self.scope_snapshot.ino()
            || current_identity(self.pid).as_ref() != Some(&self.identity)
        {
            return Err(STEAM_OWNER_FAILURE.into());
        }
        for file in &self.code {
            file.revalidate()?;
        }
        Ok(())
    }
}

/// Caller has already stopped this exact prefix's verified server, and holds
/// its bottle lock plus the original prefix inode. Only generated native aliases
/// receive audit-token signals; every other owner remains detection-only.
pub(super) fn stop_verified_native_owners(
    dir: &Path,
    prefix_path: &Path,
    prefix_guard: &File,
    prefix_snapshot: &fs::Metadata,
    wine: &Path,
    timeout: Duration,
) -> Result<usize, String> {
    let (dir_guard, dir_snapshot) = open_stable_directory(dir)?;
    let dir = dir.canonicalize().map_err(|_| CHECK_FAILURE)?;
    let deadline = Instant::now() + timeout;
    let check = || {
        cancellation::check()?;
        bottle_control::assert_same_prefix(&dir, &dir_guard, &dir_snapshot)?;
        bottle_control::assert_same_prefix(prefix_path, prefix_guard, prefix_snapshot)
    };
    let scan = || {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CHECK_FAILURE.into());
        }
        inventory(&dir, remaining.min(PROBE_TIMEOUT))
    };
    check()?;
    let discovery = scan()?;
    let identities: Vec<_> = discovery
        .iter()
        .filter(|p| {
            p.files
                .iter()
                .filter_map(|f| f.path.as_deref())
                .any(|p| p.starts_with(&dir))
        })
        .filter_map(|p| current_identity(p.pid).map(|identity| (p.pid, identity)))
        .collect();
    if identities.is_empty() {
        return Ok(0);
    }
    // Read maps after binding each candidate's kernel execution identity.
    let proof_inventory = scan()?;
    let mut pending = Vec::new();
    let mut stopped = 0;
    for (pid, identity) in identities {
        check()?;
        if Instant::now() >= deadline {
            return Err(CHECK_FAILURE.into());
        }
        let Some(files) = proof_inventory.iter().find(|p| p.pid == pid) else {
            continue;
        };
        if let Some(owner) = prove_owner(&dir, wine, &identity, files, prefix_snapshot)? {
            check()?;
            if Instant::now() >= deadline {
                return Err(CHECK_FAILURE.into());
            }
            if signal_token(&owner.identity, 15)? {
                stopped += 1;
                pending.push(owner);
            }
        }
    }
    let grace = (Instant::now() + Duration::from_secs(1)).min(deadline);
    while Instant::now() < grace
        && pending
            .iter()
            .any(|p| current_identity(p.identity.token.values[5]).as_ref() == Some(&p.identity))
    {
        check()?;
        std::thread::sleep(Duration::from_millis(25));
    }
    if pending
        .iter()
        .any(|p| current_identity(p.identity.token.values[5]).as_ref() == Some(&p.identity))
    {
        let current = scan()?;
        for owner in &pending {
            check()?;
            if Instant::now() >= deadline {
                return Err(CHECK_FAILURE.into());
            }
            if let Some(files) = current
                .iter()
                .find(|p| p.pid == owner.identity.token.values[5])
            {
                if unchanged_owner(&dir, wine, owner, files, prefix_snapshot)? {
                    check()?;
                    if Instant::now() >= deadline {
                        return Err(CHECK_FAILURE.into());
                    }
                    signal_token(&owner.identity, 9)?;
                }
            }
        }
    }
    check()?;
    Ok(stopped)
}

#[cfg(test)]
pub(super) fn with_native_shutdown_fixture(
    hold: bool,
    test: impl FnOnce(&Path, &Path, &Path, &mut Child),
) {
    tests::with_native_shutdown_fixture(hold, test)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("prefix-process-check-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn inspector(&self, script: &[u8]) -> PathBuf {
            let path = self.0.join("fixture-inspector");
            fs::write(&path, script).unwrap();
            let mut mode = fs::metadata(&path).unwrap().permissions();
            mode.set_mode(0o700);
            fs::set_permissions(&path, mode).unwrap();
            path
        }
        fn assert_no_captures(&self) {
            assert!(!fs::read_dir(&self.0).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".caskrun-process-check-")
            }));
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn owners_are_exact_current_paths_and_never_command_names_or_saved_pids() {
        let base = Path::new("/example/bottle");
        let bytes = b"p11\0n/example/bottle/prefix/system.reg\0\np12\0n/example/bottle/logs/launch.log\0\np12\0n/example/bottle/prefix/drive_c/app.exe\0\np13\0n/example/bottle-other/logs/launch.log\0\np14\0n/unrelated/wine\0\np15\0n/example/bottle\0\n";
        assert_eq!(parse_owners(bytes, base, &[11]).unwrap(), 2);
        assert_eq!(parse_owners(bytes, base, &[11, 12, 15]).unwrap(), 0);
    }

    #[test]
    fn nul_fields_preserve_spaces_and_newlines_in_file_names() {
        let bytes = b"p20\0f3\0n/example/bottle dir/prefix/a\nb\0\np21\0n/example/other\0\n";
        assert_eq!(
            parse_owners(bytes, Path::new("/example/bottle dir"), &[]).unwrap(),
            1
        );
    }

    #[test]
    fn malformed_or_truncated_inventory_fails_closed() {
        for bytes in [
            b"".as_slice(),
            b"p2",
            b"n/example/bottle\0",
            b"pzero\0",
            b"p0\0",
            b"p2\0xunknown\0",
            b"p2\0",
            b"f3\0",
        ] {
            assert!(parse_owners(bytes, Path::new("/example/bottle"), &[]).is_err());
        }
    }

    #[test]
    fn missing_warning_and_nonzero_inspectors_preserve_bottle_and_remove_captures() {
        let fixture = Fixture::new();
        assert!(
            wait_until_idle_with_inspector(
                &fixture.0,
                Duration::from_millis(100),
                &fixture.0.join("missing")
            )
            .is_err()
        );
        fixture.assert_no_captures();
        for script in [
            b"#!/bin/sh\nprintf warning >&2\nprintf 'p42\\000n/unrelated\\000\\n'\n".as_slice(),
            b"#!/bin/sh\nexit 2\n",
        ] {
            let inspector = fixture.inspector(script);
            assert!(
                wait_until_idle_with_inspector(&fixture.0, Duration::from_millis(100), &inspector)
                    .is_err()
            );
            fixture.assert_no_captures();
            assert!(fixture.0.is_dir());
        }
    }

    #[test]
    fn no_matching_files_and_unrelated_owners_are_idle() {
        let fixture = Fixture::new();
        for script in [
            b"#!/bin/sh\nexit 1\n".as_slice(),
            b"#!/bin/sh\nprintf 'p42\\000n/unrelated\\000\\n'\n",
        ] {
            let inspector = fixture.inspector(script);
            let result =
                wait_until_idle_with_inspector(&fixture.0, Duration::from_secs(2), &inspector);
            assert!(result.is_ok(), "{result:?}");
            fixture.assert_no_captures();
        }
    }

    #[test]
    fn stalled_and_oversized_inspection_is_bounded_and_leaves_no_capture_residue() {
        let fixture = Fixture::new();
        let inspector = fixture.inspector(b"#!/bin/sh\nexec sleep 30\n");
        let started = Instant::now();
        assert!(
            wait_until_idle_with_inspector(&fixture.0, Duration::from_millis(100), &inspector)
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        fixture.assert_no_captures();
        let inspector = fixture.inspector(
            format!(
                "#!/bin/sh\nexec /usr/bin/head -c {} /dev/zero\n",
                MAX_OUTPUT_BYTES + 1
            )
            .as_bytes(),
        );
        assert!(
            wait_until_idle_with_inspector(&fixture.0, Duration::from_secs(2), &inspector).is_err()
        );
        fixture.assert_no_captures();
    }

    #[test]
    fn real_open_handle_blocks_until_its_owned_fixture_exits_without_signals() {
        use std::io::BufRead;
        let fixture = Fixture::new();
        let file = fixture.0.join("held.log");
        fs::write(&file, b"fixture").unwrap();
        let mut observer = Command::new("/bin/sh")
            .args([
                "-c",
                "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                "fixture",
            ])
            .arg(&file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        std::io::BufReader::new(observer.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let blocked = wait_until_idle(&fixture.0, Duration::from_millis(300));
        drop(observer.stdin.take());
        observer.wait().unwrap();
        assert!(blocked.is_err());
        assert!(wait_until_idle(&fixture.0, Duration::from_secs(2)).is_ok());
        fixture.assert_no_captures();
    }
    #[test]
    fn unopened_game_trees_need_no_recursive_directory_access() {
        struct PrivateGameTree(PathBuf);
        impl Drop for PrivateGameTree {
            fn drop(&mut self) {
                if let Ok(metadata) = fs::metadata(&self.0) {
                    let mut permissions = metadata.permissions();
                    permissions.set_mode(0o700);
                    let _ = fs::set_permissions(&self.0, permissions);
                }
            }
        }
        let fixture = Fixture::new();
        let games = fixture.0.join("unopened-games");
        fs::create_dir(&games).unwrap();
        fs::write(games.join("unopened-data"), b"fixture").unwrap();
        let _restore_permissions = PrivateGameTree(games.clone());
        let mut mode = fs::metadata(&games).unwrap().permissions();
        mode.set_mode(0o000);
        fs::set_permissions(&games, mode).unwrap();
        assert!(wait_until_idle(&fixture.0, Duration::from_secs(2)).is_ok());
        fixture.assert_no_captures();
    }

    #[test]
    fn audit_tokens_bind_own_child_identity_and_refuse_dead_or_wrong_generation() {
        assert_eq!(std::mem::size_of::<BsdInfo>(), 136);
        let mut child = InspectionChild(
            Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let identity = current_identity(child.0.id()).unwrap();
        assert!(identity.birth.0 > 0);
        assert_eq!(current_identity(child.0.id()).as_ref(), Some(&identity));
        let mut changed = identity.clone();
        changed.token.values[7] ^= 1;
        assert!(!signal_token(&changed, 15).unwrap());
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(signal_token(&identity, 15).unwrap());
        assert_eq!(child.0.wait().unwrap().signal(), Some(15));
        assert!(!signal_token(&identity, 15).unwrap());
    }

    #[test]
    fn rich_inventory_preserves_mapped_vnode_fields_and_rejects_incomplete_structure() {
        let parsed = parse_inventory(b"p42\0ftxt\0tREG\0D0x123\0i99\0n/example/native\0\nf2\0tREG\0D0x123\0i100\0n/example/log\0\n").unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(mapped_file(
            &parsed[0].files[0],
            Path::new("/example/native"),
            0x123,
            99
        ));
        assert!(!mapped_file(
            &parsed[0].files[1],
            Path::new("/example/log"),
            0x123,
            100
        ));
        for invalid in [
            b"p42\0n/example/log\0".as_slice(),
            b"f2\0n/example/log\0",
            b"p42\0",
            b"p42\0f2\0n/example/log",
            b"p42\0f2\0n/example/log\0p42\0f3\0n/example/other\0",
        ] {
            assert!(parse_inventory(invalid).is_err());
        }
    }

    #[test]
    fn steam_quit_owner_requires_mapped_code_and_exact_bottle_scope() {
        let root = Fixture::new();
        let dir = root.0.join("bottles").join(Uuid::new_v4().to_string());
        fs::create_dir_all(dir.join("prefix")).unwrap();
        let mut code = Vec::new();
        for name in ["Steam", "ntdll.so", "Steam.exe"] {
            let path = root.0.join(name);
            fs::write(&path, b"original owned fixture code").unwrap();
            code.push(SteamCodeFile::open(path).unwrap());
        }
        let code: [SteamCodeFile; 3] = code.try_into().ok().unwrap();
        let prefix = fs::metadata(dir.join("prefix")).unwrap();
        let file = |index: usize| OpenFile {
            descriptor: "txt".into(),
            kind: "REG".into(),
            path: Some(code[index].path.clone()),
            device: Some(code[index].snapshot.dev()),
            inode: Some(code[index].snapshot.ino()),
        };
        let cwd = OpenFile {
            descriptor: "cwd".into(),
            kind: "DIR".into(),
            path: Some(dir.join("prefix")),
            device: Some(prefix.dev()),
            inode: Some(prefix.ino()),
        };
        let good = vec![file(0), file(1), file(2), cwd];
        assert!(
            steam_scope(
                &dir,
                &prefix,
                &code,
                &ProcessFiles {
                    pid: 42,
                    files: good.clone()
                }
            )
            .unwrap()
            .is_some()
        );
        for index in 0..3 {
            for field in ["descriptor", "inode", "device"] {
                let mut files = good.clone();
                match field {
                    "descriptor" => files[index].descriptor = "3".into(),
                    "inode" => files[index].inode = Some(0),
                    _ => files[index].device = Some(0),
                }
                assert!(
                    steam_scope(&dir, &prefix, &code, &ProcessFiles { pid: 42, files })
                        .unwrap()
                        .is_none()
                );
            }
        }
        let sibling = dir.parent().unwrap().join(Uuid::new_v4().to_string());
        for path in [sibling.join("prefix"), sibling.join("logs/launch-1.log")] {
            let mut files = good.clone();
            files.push(OpenFile {
                descriptor: "4".into(),
                kind: "REG".into(),
                path: Some(path),
                ..OpenFile::default()
            });
            assert!(
                steam_scope(&dir, &prefix, &code, &ProcessFiles { pid: 42, files })
                    .unwrap()
                    .is_none()
            );
        }
        let mut files = good;
        files.pop();
        assert!(
            steam_scope(&dir, &prefix, &code, &ProcessFiles { pid: 42, files })
                .unwrap()
                .is_none()
        );
        for file in &code {
            let replacement = root.0.join("replacement");
            fs::write(&replacement, b"original owned fixture code").unwrap();
            fs::rename(&replacement, &file.path).unwrap();
            assert!(
                file.revalidate().is_err(),
                "a new vnode with identical bytes is stale proof"
            );
        }
    }

    #[test]
    fn steam_quit_owner_binds_live_original_native_fixture_and_rejects_stale_identity() {
        let fixture = NativeFixture::new();
        let library = fixture.ntdll.parent().unwrap();
        let loader = library.join("Steam");
        let client = fixture.dir.join("prefix/Steam.exe");
        fs::write(&client, vec![0_u8; 4096]).unwrap();
        let source = fixture.root.0.join("steam-owner.c");
        fs::write(&source, br#"#include <dlfcn.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
int main(int argc, char **argv) {
 if(argc != 3 || !dlopen(argv[1], RTLD_NOW)) return 2;
 int fd = open(argv[2], O_RDONLY); struct stat st;
 if(fd < 0 || fstat(fd, &st) || mmap(0, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0) == MAP_FAILED) return 3;
 close(fd); write(2,"ready\n",6); char byte;
 return read(0,&byte,1) < 0 ? 4 : 0;
}
"#).unwrap();
        let output = Command::new("/usr/bin/clang")
            .args(["-arch", "x86_64", "-O0"])
            .arg(&source)
            .arg("-o")
            .arg(&loader)
            .output()
            .unwrap();
        assert!(output.status.success(), "original fixture did not compile");
        let log = OpenOptions::new().append(true).open(&fixture.log).unwrap();
        let mut child = InspectionChild(
            Command::new(&loader)
                .arg(&fixture.ntdll)
                .arg(&client)
                .current_dir(fixture.dir.join("prefix"))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fs::read(&fixture.log)
            .unwrap()
            .windows(6)
            .any(|window| window == b"ready\n")
        {
            assert!(
                Instant::now() < deadline,
                "original Steam owner fixture did not become ready"
            );
            assert!(child.0.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(10));
        }
        let prefix = fs::metadata(fixture.dir.join("prefix")).unwrap();
        let mut owner = steam_client_owner(&fixture.dir, &fixture.wine, &client, &prefix).unwrap();
        assert_eq!(owner.pid, child.0.id());
        owner
            .revalidate(&fixture.dir, &fixture.wine, &client, &prefix)
            .unwrap();
        owner.identity.token.values[7] ^= 1;
        assert!(
            owner
                .revalidate(&fixture.dir, &fixture.wine, &client, &prefix)
                .is_err()
        );
        owner.identity.token.values[7] ^= 1;
        fs::write(&client, vec![1_u8; 4096]).unwrap();
        assert!(
            owner
                .revalidate(&fixture.dir, &fixture.wine, &client, &prefix)
                .is_err()
        );
        drop(child.0.stdin.take());
        assert!(child.0.wait().unwrap().success());
        assert!(steam_client_owner(&fixture.dir, &fixture.wine, &client, &prefix).is_err());
        assert!(!fs::read_dir(&fixture.dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".caskrun-process-check-")
        }));
    }

    struct NativeFixture {
        root: Fixture,
        dir: PathBuf,
        wine: PathBuf,
        loader: PathBuf,
        ntdll: PathBuf,
        log: PathBuf,
    }
    impl NativeFixture {
        fn new() -> Self {
            let root = Fixture::new();
            let dir = root.0.join("bottles").join(Uuid::new_v4().to_string());
            fs::create_dir_all(dir.join("prefix")).unwrap();
            fs::create_dir_all(dir.join("logs")).unwrap();
            let runtime = root.0.join("original-fixture-runtime");
            let bin = runtime.join("bin");
            let library = runtime.join("runtime/lib/wine/x86_64-unix");
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(&library).unwrap();
            fs::write(
                runtime.join("SOURCES.txt"),
                b"Original isolated shutdown test sources\n",
            )
            .unwrap();
            fs::write(
                runtime.join("native-app-identity.txt"),
                b"caskrun-native-app-identity-v1\n",
            )
            .unwrap();
            for name in ["wine", "wineboot", "_dispatch"] {
                fs::write(bin.join(name), b"#!/bin/sh\nexit 0\n").unwrap();
                fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
            }
            // The icon utility/artwork are validated inputs, never executed.
            let mut utility = [0_u8; 64];
            utility[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            utility[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
            fs::write(bin.join("native-icon"), utility).unwrap();
            fs::set_permissions(bin.join("native-icon"), fs::Permissions::from_mode(0o700))
                .unwrap();
            let mut png = vec![0_u8; 33];
            png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
            png[8..12].copy_from_slice(&13_u32.to_be_bytes());
            png[12..16].copy_from_slice(b"IHDR");
            png[16..20].copy_from_slice(&1024_u32.to_be_bytes());
            png[20..24].copy_from_slice(&1024_u32.to_be_bytes());
            let mut icns = Vec::from(b"icns".as_slice());
            icns.extend_from_slice(&49_u32.to_be_bytes());
            icns.extend_from_slice(b"ic10");
            icns.extend_from_slice(&41_u32.to_be_bytes());
            icns.extend_from_slice(&png);
            for name in ["native-default-icon.png", "native-default-rendered.png"] {
                fs::write(runtime.join(name), &png).unwrap();
            }
            fs::write(runtime.join("native-default-icon.icns"), icns).unwrap();
            let source = root.0.join("idle.c");
            fs::write(
                &source,
                br#"#include <dlfcn.h>
#include <signal.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>
static void term(int sig) { (void)sig; write(2,"term\n",5); }
int main(int argc, char **argv) {
 if(argc<2 || !dlopen(argv[1],RTLD_NOW)) return 2;
 if(argc>2 && !strcmp(argv[2],"hold")) signal(SIGTERM,term);
 write(2,"ready\n",6);
 char byte; while(read(0,&byte,1)<0 && errno==EINTR) {} return 0;
}
"#,
            )
            .unwrap();
            let lib_source = root.0.join("library.c");
            fs::write(&lib_source, b"void original_shutdown_fixture(void) {}\n").unwrap();
            let packaged_loader = library.join("WindowsApp");
            let ntdll = library.join("ntdll.so");
            for (input, output, shared) in [
                (&source, &packaged_loader, false),
                (&lib_source, &ntdll, true),
            ] {
                let mut compiler = Command::new("/usr/bin/clang");
                compiler.args(["-arch", "x86_64", "-O0"]);
                if shared {
                    compiler.arg("-dynamiclib");
                }
                let output = compiler.arg(input).arg("-o").arg(output).output().unwrap();
                assert!(
                    output.status.success(),
                    "original fixture compilation failed"
                );
            }
            let loader = dir
                .join("native-apps")
                .join("a".repeat(64))
                .join("Fixture.app/Contents/MacOS/WindowsApp");
            fs::create_dir_all(loader.parent().unwrap()).unwrap();
            fs::copy(&packaged_loader, &loader).unwrap();
            fs::set_permissions(&loader, fs::Permissions::from_mode(0o700)).unwrap();
            let log = dir
                .join("logs")
                .join(format!("launch-1-{}.log", Uuid::new_v4()));
            fs::write(&log, b"").unwrap();
            Self {
                root,
                dir,
                wine: bin.join("wine"),
                loader,
                ntdll,
                log,
            }
        }
        fn child(&self, hold: bool) -> InspectionChild {
            let log = OpenOptions::new()
                .write(true)
                .append(true)
                .open(&self.log)
                .unwrap();
            let mut command = Command::new(&self.loader);
            command.arg(&self.ntdll);
            if hold {
                command.arg("hold");
            }
            let mut child = InspectionChild(
                command
                    .current_dir(self.dir.join("prefix"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::from(log))
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while !fs::read(&self.log)
                .unwrap()
                .windows(6)
                .any(|w| w == b"ready\n")
            {
                assert!(
                    Instant::now() < deadline,
                    "original idle child did not become ready"
                );
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "original idle child exited early"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            child
        }
        fn files(&self, pid: u32) -> ProcessFiles {
            let entry = |descriptor: &str, path: &Path| {
                let metadata = fs::metadata(path).unwrap();
                OpenFile {
                    descriptor: descriptor.into(),
                    kind: "REG".into(),
                    path: Some(path.into()),
                    device: Some(metadata.dev()),
                    inode: Some(metadata.ino()),
                }
            };
            ProcessFiles {
                pid,
                files: vec![
                    entry("txt", &self.loader),
                    entry("txt", &self.ntdll),
                    entry("2", &self.log),
                ],
            }
        }
        fn stop(&self) -> Result<usize, String> {
            let prefix = self.dir.join("prefix");
            let (guard, snapshot) = open_stable_directory(&prefix).unwrap();
            stop_verified_native_owners(
                &self.dir,
                &prefix,
                &guard,
                &snapshot,
                &self.wine,
                Duration::from_secs(8),
            )
        }
        fn finish(&self, child: &mut InspectionChild) {
            drop(child.0.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if child.0.try_wait().unwrap().is_some() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "owned fixture failed to exit naturally"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            self.root.assert_no_captures();
        }
    }

    pub(super) fn with_native_shutdown_fixture(
        hold: bool,
        test: impl FnOnce(&Path, &Path, &Path, &mut Child),
    ) {
        let fixture = NativeFixture::new();
        let mut sibling = NativeFixture::new();
        // Keep both synthetic bottles under one home, with their own runtimes
        // and native aliases, so the control test also proves sibling isolation.
        let sibling_dir = fixture
            .root
            .0
            .join("bottles")
            .join(sibling.dir.file_name().unwrap());
        fs::rename(&sibling.dir, &sibling_dir).unwrap();
        sibling.loader = sibling_dir.join(sibling.loader.strip_prefix(&sibling.dir).unwrap());
        sibling.log = sibling_dir.join(sibling.log.strip_prefix(&sibling.dir).unwrap());
        sibling.dir = sibling_dir;
        let mut child = fixture.child(hold);
        let mut sibling_child = sibling.child(false);
        test(&fixture.root.0, &fixture.dir, &fixture.wine, &mut child.0);
        assert!(sibling_child.0.try_wait().unwrap().is_none());
        sibling.finish(&mut sibling_child);
        fixture.root.assert_no_captures();
    }

    #[test]
    fn original_native_fixture_is_stopped_only_after_current_code_and_scope_proofs() {
        let fixture = NativeFixture::new();
        let mut child = fixture.child(false);
        let identity = current_identity(child.0.id()).unwrap();
        let alias = native_app_identity::validate_shutdown_alias(
            &fixture.dir,
            &fixture.wine,
            &identity.executable,
        )
        .unwrap()
        .unwrap();
        let prefix = fs::metadata(fixture.dir.join("prefix")).unwrap();
        assert!(
            scope_file(
                &fixture.files(child.0.id()),
                &fixture.dir,
                (&alias).into(),
                &prefix
            )
            .unwrap()
            .is_some()
        );
        assert_eq!(fixture.stop().unwrap(), 1);
        assert_eq!(child.0.wait().unwrap().signal(), Some(15));
        wait_until_idle(&fixture.dir, Duration::from_secs(2)).unwrap();
        fixture.root.assert_no_captures();
    }

    #[test]
    fn unresponsive_original_native_fixture_requires_fresh_proof_before_token_kill() {
        let fixture = NativeFixture::new();
        let mut child = fixture.child(true);
        assert_eq!(fixture.stop().unwrap(), 1);
        assert_eq!(child.0.wait().unwrap().signal(), Some(9));
        wait_until_idle(&fixture.dir, Duration::from_secs(2)).unwrap();
        fixture.root.assert_no_captures();
    }

    #[test]
    fn other_bottle_stale_code_and_nonmapped_library_never_authorize_a_signal() {
        let fixture = NativeFixture::new();
        let alias = native_app_identity::validate_shutdown_alias(
            &fixture.dir,
            &fixture.wine,
            &fixture.loader,
        )
        .unwrap()
        .unwrap();
        let prefix = fs::metadata(fixture.dir.join("prefix")).unwrap();
        let mut entries = fixture.files(42);
        entries.files[1].descriptor = "3".into();
        assert!(
            scope_file(&entries, &fixture.dir, (&alias).into(), &prefix)
                .unwrap()
                .is_none()
        );
        entries.files[1].descriptor = "txt".into();
        entries.files[1].inode = Some(alias.ntdll_ino + 1);
        assert!(
            scope_file(&entries, &fixture.dir, (&alias).into(), &prefix)
                .unwrap()
                .is_none()
        );
        entries.files[1].inode = Some(alias.ntdll_ino);
        entries.files[0].inode = Some(alias.loader_ino + 1);
        assert!(
            scope_file(&entries, &fixture.dir, (&alias).into(), &prefix)
                .unwrap()
                .is_none()
        );
        entries.files[0].inode = Some(alias.loader_ino);
        entries.files[2].inode = Some(fs::metadata(&fixture.log).unwrap().ino() + 1);
        assert!(
            scope_file(&entries, &fixture.dir, (&alias).into(), &prefix)
                .unwrap()
                .is_none()
        );
        entries = fixture.files(42);
        entries.files.push(OpenFile {
            path: Some(
                fixture
                    .dir
                    .parent()
                    .unwrap()
                    .join(Uuid::new_v4().to_string())
                    .join("logs/app.log"),
            ),
            ..OpenFile::default()
        });
        assert!(
            scope_file(&entries, &fixture.dir, (&alias).into(), &prefix)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn external_idle_file_holder_is_preserved_even_with_exact_bottle_log() {
        let fixture = NativeFixture::new();
        let log = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&fixture.log)
            .unwrap();
        let mut child = InspectionChild(
            Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::from(log))
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        assert_eq!(fixture.stop().unwrap(), 0);
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(wait_until_idle(&fixture.dir, Duration::from_millis(150)).is_err());
        fixture.finish(&mut child);
    }

    #[test]
    fn changed_loader_library_or_prefix_after_term_prevents_escalation() {
        for changed in ["loader", "library", "prefix"] {
            let fixture = NativeFixture::new();
            let mut child = fixture.child(true);
            let log = fixture.log.clone();
            let path = match changed {
                "loader" => fixture.loader.clone(),
                "library" => fixture.ntdll.clone(),
                _ => fixture.dir.join("prefix"),
            };
            let replacement = path.with_extension("displaced");
            let mutate = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !fs::read(&log).unwrap().windows(5).any(|w| w == b"term\n") {
                    assert!(Instant::now() < deadline, "fixture received no TERM");
                    std::thread::sleep(Duration::from_millis(10));
                }
                fs::rename(&path, &replacement).unwrap();
                if changed == "prefix" {
                    fs::create_dir(&path).unwrap();
                } else {
                    fs::copy(&replacement, &path).unwrap();
                }
            });
            let _result = fixture.stop();
            mutate.join().unwrap();
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "changed identity was escalated"
            );
            assert!(wait_until_idle(&fixture.dir, Duration::from_millis(150)).is_err());
            fixture.finish(&mut child);
        }
    }
}
