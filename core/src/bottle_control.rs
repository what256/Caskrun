//! Explicit controls for one Caskrun-owned, initialized Wine bottle.
//!
//! Wine's wineserver selects its target by WINEPREFIX. Never send signals to
//! recorded PIDs here: an old PID can belong to an unrelated macOS process.
//! Native clients that outlive their server use the shared audit-token and
//! current code/prefix ownership proof before receiving a signal.

use super::*;
use std::io::Write;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(8);
const POLITE_QUIT_TIMEOUT: Duration = Duration::from_secs(120);
const SESSION_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Serialize)]
pub(super) struct Activity {
    pub(super) bottle_id: String,
    pub(super) running: bool,
    pub(super) owner_count: usize,
}

/// Activity is current file ownership, never a saved PID or a Wine process
/// name. A failed inspection remains an error so the UI cannot call it idle.
/// An uninitialized prefix is not started or repaired just to inspect it.
pub(super) fn activity(home: &Path, id: &str) -> Result<Activity, String> {
    let dir = bottle_dir(home, id)?;
    match fs::symlink_metadata(dir.join("prefix")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if metadata.is_dir() => {
            let _ = open_stable_directory(&dir.join("prefix"))?;
        }
        Ok(_) => return Err("Bottle prefix must be a real directory".into()),
    }
    // Initialization and a stuck app can hold files before a registry exists,
    // or after one disappears. Neither case is evidence of an idle bottle.
    let owner_count = prefix_processes::owner_count(&dir)?;
    Ok(Activity {
        bottle_id: id.into(),
        running: owner_count > 0,
        owner_count,
    })
}

#[derive(Debug, Serialize)]
pub(super) struct QuitRequest {
    pub(super) bottle_id: String,
    pub(super) request_submitted: bool,
    pub(super) log_path: Option<String>,
    pub(super) message: String,
}

fn idle_request(id: &str) -> QuitRequest {
    QuitRequest {
        bottle_id: id.into(),
        request_submitted: false,
        log_path: None,
        message: "No Windows apps are running in this bottle.".into(),
    }
}

/// Ask the exact bottle's existing Windows session to close politely. Wineboot
/// `-e -s` sends the end-session request without explicit Core initialization,
/// migration, repair, update, reboot, force or kill flags. Apps may refuse or
/// show unsaved-work UI. Require an existing server immediately beforehand:
/// Wine's loader can still bootstrap if that server exits in the spawn race,
/// even though the requested Wineboot body skips its initialization section.
/// Cancel/timeout stops only this invocation's still-unreaped control child;
/// it never escalates to the bottle's Wine server or Windows applications.
pub(super) fn request_quit(home: &Path, id: &str) -> Result<QuitRequest, String> {
    let wine = wine_path().ok_or("Caskrun could not find its included Wine runtime")?;
    request_quit_with_wine_checked(
        home,
        id,
        &wine,
        POLITE_QUIT_TIMEOUT,
        prefix_processes::owner_count,
    )
}

fn request_quit_with_wine_checked(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
    mut owners: impl FnMut(&Path) -> Result<usize, String>,
) -> Result<QuitRequest, String> {
    cancellation::check()?;
    let lock = lock_bottle(home, id)?;
    let (dir, prefix, _server, prefix_guard, prefix_snapshot) =
        checked_control_paths(home, id, wine)?;
    let (dir_guard, dir_snapshot) = open_stable_directory(&dir)?;
    if owners(&dir)? == 0 {
        return Ok(idle_request(id));
    }
    let wineboot = checked_polite_wineboot(wine)?;
    let wineboot_target = wineboot.canonicalize().map_err(|e| e.to_string())?;
    let wineboot_snapshot = fs::metadata(&wineboot_target).map_err(|e| e.to_string())?;
    let mut command = Command::new(&wineboot);
    configure_control_environment(&mut command, &dir, wine)?;
    require_running_session(&_server, &dir, &prefix)?;
    let mut tracked = read_active_processes(&dir)?;
    let logs = dir.join("logs");
    let (logs_guard, logs_snapshot) = open_stable_directory(&logs)?;
    let _ = checked_control_paths(home, id, wine)?;
    assert_same_prefix(&dir, &dir_guard, &dir_snapshot)?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    if owners(&dir)? == 0 {
        return Ok(idle_request(id));
    }
    require_running_session(&_server, &dir, &prefix)?;
    let _ = checked_control_paths(home, id, wine)?;
    if checked_polite_wineboot(wine)?
        .canonicalize()
        .map_err(|e| e.to_string())?
        != wineboot_target
        || !same_snapshot(
            &wineboot_snapshot,
            &fs::metadata(&wineboot_target).map_err(|e| e.to_string())?,
        )
    {
        return Err("The Windows session helper changed before its Quit request".into());
    }
    assert_same_prefix(&dir, &dir_guard, &dir_snapshot)?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    assert_same_prefix(&logs, &logs_guard, &logs_snapshot)?;
    cancellation::check()?;
    let log_path = logs.join(format!("launch-{}-{}.log", now_seconds()?, Uuid::new_v4()));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let errors = log.try_clone().map_err(|e| e.to_string())?;
    command
        .args(["-e", "-s"])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors));
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not request Windows apps to quit: {e}"))?;
    let pid = child.id();
    let original_pid_count = tracked.iter().filter(|value| **value == pid).count();
    tracked.push(pid);
    let tracking = save_json(&dir.join("active-processes.json"), &tracked);
    drop(command);
    drop(logs_guard);
    drop(prefix_guard);
    drop(dir_guard);
    drop(lock);
    let waited = wait_child_bounded_status(&mut child, timeout);
    // Do not remove a record unless our owned control child was reaped. A
    // failure to stop/reap the helper retains the record and every app remains
    // untouched by Caskrun's cancellation/timeout handling.
    let reaped = child
        .try_wait()
        .map_err(|error| {
            format!("Could not check the Quit request; its tracking record was retained: {error}")
        })?
        .is_some();
    if let Err(error) = tracking {
        return Err(format!(
            "The Quit request started but could not be tracked: {error}"
        ));
    }
    if reaped {
        remove_control_child_record(
            &dir,
            &prefix,
            &dir_snapshot,
            &prefix_snapshot,
            pid,
            original_pid_count,
        )?;
    }
    let (status, timed_out) = waited?;
    if timed_out {
        return Err("The polite Quit request timed out. Windows apps may still be running; choose Force Quit only when you want to stop them.".into());
    }
    if !status.success() {
        return Err(format!(
            "The Windows session did not accept its Quit request (exit {:?}, signal {:?}). Windows apps were not force-quit.",
            status.code(),
            status.signal()
        ));
    }
    Ok(QuitRequest {
        bottle_id: id.into(),
        request_submitted: true,
        log_path: Some(log_path.display().to_string()),
        message: "Quit requested. Windows apps can still ask to save work or decline to close."
            .into(),
    })
}

fn checked_polite_wineboot(wine: &Path) -> Result<PathBuf, String> {
    let helper = wine
        .parent()
        .ok_or("The Windows engine has no package directory")?
        .join("wineboot");
    if !helper.is_absolute() || !is_executable(&helper) || !is_independent_runtime(&helper) {
        return Err("This independent Windows engine has no usable Wineboot helper".into());
    }
    Ok(helper)
}

/// `wineserver -w` only waits on an existing server's lock; it never creates a
/// server or requests shutdown. A bounded wait is evidence of a live session.
/// Kill/reap only this invocation's own waiter when its observation times out.
/// Arbitrary prefix file holders alone must never cause a Wineboot launch.
fn require_running_session(server: &Path, dir: &Path, prefix: &Path) -> Result<(), String> {
    if !server.is_absolute() || !is_executable(server) || !is_independent_runtime(server) {
        return Err("Cannot verify this bottle's running Windows session".into());
    }
    let mut command = Command::new(server);
    isolate_wineserver_environment(&mut command);
    command
        .arg("-w")
        .env("WINEPREFIX", prefix)
        .env("XDG_CONFIG_HOME", dir.join("xdg/config"))
        .env("XDG_DATA_HOME", dir.join("xdg/data"))
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not inspect this Windows session: {error}"))?;
    let (status, waiting) = wait_child_bounded_status(&mut child, SESSION_PROBE_TIMEOUT)?;
    if waiting {
        return Ok(());
    }
    if status.success() {
        Err("No running Windows session was found for this bottle. Its files may still be open in another app; use Force Quit only to recover a remaining recorded session.".into())
    } else {
        Err("Caskrun could not verify this bottle's running Windows session. No polite Quit request was launched.".into())
    }
}

#[derive(Debug, Serialize)]
pub(super) struct QuitResult {
    pub(super) bottle_id: String,
    pub(super) forced: bool,
    pub(super) server_stopped: bool,
    pub(super) tracked_before: usize,
    pub(super) tracked_after: usize,
    pub(super) log_path: String,
    pub(super) message: String,
}

/// Force-quit Windows applications in this bottle only. The ordinary UI makes
/// this an explicit user action; catalog timeout recovery can also use it.
/// `wineserver -k` can discard unsaved app data.
pub(super) fn quit_windows_apps(home: &Path, id: &str) -> Result<QuitResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    quit_windows_apps_with_wine(home, id, &wine, CONTROL_TIMEOUT)
}

pub(super) fn quit_windows_apps_with_wine(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
) -> Result<QuitResult, String> {
    let _lock = lock_bottle(home, id)?;
    // A recorded running session can outlive a removed registry. Force Quit
    // still requires its exact independent runtime and original prefix inode;
    // it never initializes a replacement registry just to stop that session.
    quit_windows_apps_locked(home, id, wine, timeout, false)
}

/// Shutdown also handles a recorded, initialized prefix whose registry was
/// removed while an app remained open. The caller must hold this bottle's lock.
/// All selected-runtime and current prefix identity checks still apply.
pub(super) fn quit_recorded_windows_apps_locked(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
) -> Result<QuitResult, String> {
    quit_windows_apps_locked(home, id, wine, timeout, false)
}

fn quit_windows_apps_locked(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
    require_registry: bool,
) -> Result<QuitResult, String> {
    let (dir, prefix, wineserver, prefix_guard, prefix_snapshot) =
        checked_recorded_control_paths(home, id, wine, require_registry)?;
    let tracked_before = live_processes(&dir)?.len();
    let (mut log, log_path) = new_control_log(&dir)?;
    writeln!(
        log,
        "Caskrun: force-quit requested for bottle {id}. WINEPREFIX={}",
        prefix.display()
    )
    .map_err(|e| format!("Could not write bottle-control log: {e}"))?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    // Hold the cleanup claim only after lock and path preflight. A Cancel
    // requested while those steps wait must win before we ask Wine to stop
    // every Windows process in this bottle.
    let _timeout_cleanup = cancellation::begin_timeout_cleanup()?;

    let (kill_status, kill_timed_out) = run_wineserver(&wineserver, &prefix, "-k", &log, timeout)?;
    if kill_timed_out {
        return Err(format!(
            "Wine's quit request timed out for this bottle. Check {}",
            log_path.display()
        ));
    }
    // Wine returns status 1 when -k finds no running server. A failed kill
    // can also return 1, so only a subsequent successful -w can establish
    // that the server is idle. A nonzero status alone cannot establish that
    // apps stopped; the native-owner fallback below has its own proof.
    writeln!(
        log,
        "Caskrun: wineserver -k exited with status {:?}; checking whether the server is idle.",
        kill_status.code()
    )
    .map_err(|e| format!("Could not write bottle-control log: {e}"))?;

    // Keep the prefix directory open throughout both commands and verify its
    // path still names the same directory before asking Wine to wait for it.
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    let (wait_status, wait_timed_out) = run_wineserver(&wineserver, &prefix, "-w", &log, timeout)?;
    if wait_timed_out || !wait_status.success() {
        return Err(format!(
            "Wine received the quit request, but this bottle's server did not stop within the limit. Check {}",
            log_path.display()
        ));
    }
    writeln!(log, "Caskrun: this bottle's wineserver stopped.")
        .map_err(|e| format!("Could not write bottle-control log: {e}"))?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    let record = recorded_runtime_for_prefix(&dir, Some(wine))?
        .ok_or("This bottle has no recorded Wine runtime")?;
    if !runtime_record_matches(&record, wine)? {
        return Err("This bottle's selected Windows engine changed during shutdown".into());
    }
    // A successful server wait alone does not prove that native clients have
    // exited. Recheck the same selected server before the independently proven
    // native-owner fallback, retaining the bottle lock and cancellation claim.
    refuse_live_wine_server(&dir, timeout, Some(wine))?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    let native_stopped = prefix_processes::stop_verified_native_owners(
        &dir,
        &prefix,
        &prefix_guard,
        &prefix_snapshot,
        wine,
        timeout,
    )?;
    prefix_processes::wait_until_idle(&dir, timeout)?;
    assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    let tracked_after = live_processes(&dir)?.len();
    if tracked_after > 0 {
        return Err("This bottle still has tracked processes after shutdown. Caskrun did not signal saved process IDs.".into());
    }
    let message = if !kill_status.success() && native_stopped == 0 {
        "No active Wine server or Windows apps remained in this bottle.".to_string()
    } else {
        "All Windows apps in this bottle stopped.".to_string()
    };
    Ok(QuitResult {
        bottle_id: id.to_string(),
        forced: kill_status.success() || native_stopped > 0,
        server_stopped: true,
        tracked_before,
        tracked_after,
        log_path: log_path.display().to_string(),
        message,
    })
}

/// Open Wine's installed-programs panel for an initialized bottle. This does
/// not uninstall anything by itself; the user chooses a program in Wine's UI.
pub(super) fn open_uninstaller(home: &Path, id: &str) -> Result<LaunchResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    open_uninstaller_with_wine(home, id, &wine)
}

fn open_uninstaller_with_wine(home: &Path, id: &str, wine: &Path) -> Result<LaunchResult, String> {
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    require_initialized_prefix(&dir, &prefix)?;
    let uninstaller = checked_uninstaller(&prefix)?;
    let (child, log_path) = start_launch_with_wine_checked(
        home,
        id,
        &uninstaller,
        wine,
        LaunchFlags {
            quiet_msi: false,
            steam_mode: false,
            working_directory: None,
        },
        &[],
        |resolved| {
            let dir = bottle_dir(home, id)?;
            let prefix = dir.join("prefix");
            let checked = checked_uninstaller(&prefix)?;
            if checked != resolved {
                return Err("Wine's uninstaller changed before launch".into());
            }
            Ok(())
        },
    )?;
    Ok(LaunchResult {
        pid: child.id(),
        log_path: log_path.display().to_string(),
    })
}

pub(super) fn checked_control_paths(
    home: &Path,
    id: &str,
    wine: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, File, fs::Metadata), String> {
    checked_recorded_control_paths(home, id, wine, true)
}

fn checked_recorded_control_paths(
    home: &Path,
    id: &str,
    wine: &Path,
    require_registry: bool,
) -> Result<(PathBuf, PathBuf, PathBuf, File, fs::Metadata), String> {
    if !wine.is_absolute() || !is_usable_wine(wine) {
        return Err("Selected Wine runtime is unavailable or not independent".into());
    }
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    if require_registry {
        require_initialized_prefix(&dir, &prefix)?;
    } else {
        require_recorded_prefix(&dir, &prefix)?;
    }
    let (guard, snapshot) = open_stable_directory(&prefix)?;
    let record = recorded_runtime_for_prefix(&dir, Some(wine))?
        .ok_or("This bottle has no recorded Wine runtime")?;
    if !runtime_record_matches(&record, wine)? {
        return Err(
            "This bottle uses a different Windows engine. Select its recorded engine.".into(),
        );
    }
    let wineserver = wine
        .parent()
        .ok_or("Wine runtime has no parent directory")?
        .join("wineserver");
    if !is_executable(&wineserver) || !is_independent_runtime(&wineserver) {
        return Err("This independent Wine runtime has no companion wineserver".into());
    }
    Ok((dir, prefix, wineserver, guard, snapshot))
}

fn require_initialized_prefix(dir: &Path, prefix: &Path) -> Result<(), String> {
    require_recorded_prefix(dir, prefix)?;
    if !path_is_real_file(&prefix.join("system.reg")) {
        return Err("Initialize this bottle before using its Windows controls".into());
    }
    Ok(())
}

fn require_recorded_prefix(dir: &Path, prefix: &Path) -> Result<(), String> {
    let _ = open_stable_directory(prefix)?;
    if !path_is_real_file(&dir.join("prefix-record.json")) {
        return Err(
            "Open this bottle with its independent Wine engine before using Windows controls"
                .into(),
        );
    }
    Ok(())
}

pub(super) fn assert_same_prefix(
    prefix: &Path,
    guard: &File,
    snapshot: &fs::Metadata,
) -> Result<(), String> {
    let current = fs::symlink_metadata(prefix)
        .map_err(|e| format!("Could not recheck bottle prefix: {e}"))?;
    let held = guard
        .metadata()
        .map_err(|e| format!("Could not recheck open bottle prefix: {e}"))?;
    // Registry files can change while Wine exits, so timestamps and directory
    // size are not stable. The device/inode pair must continue to match.
    if !same_identity(snapshot, &current) || !same_identity(snapshot, &held) {
        return Err("Bottle prefix changed while controlling Windows apps".into());
    }
    Ok(())
}

fn checked_uninstaller(prefix: &Path) -> Result<PathBuf, String> {
    let _ = open_stable_directory(prefix)?;
    let mut parent = prefix.to_path_buf();
    for component in ["drive_c", "windows", "system32"] {
        parent.push(component);
        let _ = open_stable_directory(&parent)?;
    }
    let path = parent.join("uninstaller.exe");
    if !path_is_real_file(&path) {
        return Err("Wine's installed-programs panel is missing from this bottle".into());
    }
    let canonical_prefix = prefix
        .canonicalize()
        .map_err(|e| format!("Could not resolve bottle prefix: {e}"))?;
    let expected = canonical_prefix.join("drive_c/windows/system32/uninstaller.exe");
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("Could not resolve Wine's uninstaller: {e}"))?;
    if resolved != expected {
        return Err("Wine's uninstaller must remain inside this bottle".into());
    }
    Ok(resolved)
}

fn new_control_log(dir: &Path) -> Result<(File, PathBuf), String> {
    let logs = real_subdirectory(dir, "logs")?;
    // Existing log browsing accepts launch-<timestamp>-<UUID>.log. Keep the
    // bottle control log visible there until it gets a dedicated UI label.
    let log_path = logs.join(format!("launch-{}-{}.log", now_seconds()?, Uuid::new_v4()));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| format!("Could not create bottle-control log: {e}"))?;
    Ok((log, log_path))
}

fn isolate_wineserver_environment(command: &mut Command) {
    // A verified package must not inherit loader/library overrides or resolve
    // its wrapper's tools through an unrelated PATH. Keep the login/temp
    // context needed to find this user's existing server, then set the exact
    // bottle's XDG paths and WINEPREFIX at the call site.
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    for key in [
        "HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE",
    ] {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
}

pub(super) fn run_wineserver(
    wineserver: &Path,
    prefix: &Path,
    argument: &str,
    log: &File,
    timeout: Duration,
) -> Result<(ExitStatus, bool), String> {
    let stdout = log.try_clone().map_err(|e| e.to_string())?;
    let stderr = log.try_clone().map_err(|e| e.to_string())?;
    let dir = prefix
        .parent()
        .ok_or("Bottle prefix has no parent directory")?;
    let xdg = bottle_xdg::for_bottle(dir)?;
    let mut command = Command::new(wineserver);
    isolate_wineserver_environment(&mut command);
    xdg.apply(&mut command);
    if argument == "-k" {
        cancellation::check()?;
    }
    let mut child = command
        .arg(argument)
        .env("WINEPREFIX", prefix)
        .current_dir(prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("Could not run this bottle's wineserver {argument}: {e}"))?;
    wait_child_bounded_status(&mut child, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_environment_discards_loader_and_foreign_prefix_overrides() {
        let mut command = Command::new("/bin/sh");
        for key in [
            "PATH",
            "WINEPREFIX",
            "WINESERVER",
            "WINELOADER",
            "WINEDLLPATH",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "XDG_CONFIG_HOME",
            "CASKRUN_FOREIGN_OVERRIDE",
        ] {
            command.env(key, "foreign-fixture-value");
        }
        isolate_wineserver_environment(&mut command);
        command.env("WINEPREFIX", "/fixture/selected-prefix");
        command.args(["-c", "printf '%s\\n' \"$PATH\" \"$WINEPREFIX\"; printf '%s' \"${WINESERVER-}${WINELOADER-}${WINEDLLPATH-}${DYLD_INSERT_LIBRARIES-}${DYLD_LIBRARY_PATH-}${DYLD_FALLBACK_LIBRARY_PATH-}${XDG_CONFIG_HOME-}${CASKRUN_FOREIGN_OVERRIDE-}\""]);
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            b"/usr/bin:/bin:/usr/sbin:/sbin\n/fixture/selected-prefix\n"
        );
    }

    // Parallel installer tests can briefly delay these tiny mock processes.
    const MOCK_PROCESS_TIMEOUT: Duration = Duration::from_secs(8);

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-control-test-{}", Uuid::new_v4()))
    }

    fn fake_runtime(home: &Path) -> PathBuf {
        let runtime = home.join("test-runtime");
        fs::create_dir_all(&runtime).unwrap();
        let wine = runtime.join("wine");
        let wineboot = runtime.join("wineboot");
        let wineserver = runtime.join("wineserver");
        fs::write(
            &wine,
            b"#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$WINEPREFIX/opened-uninstaller\"\n",
        )
        .unwrap();
        fs::write(&wineboot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        fs::write(
            &wineserver,
            b"#!/bin/sh\ncase \"$1\" in\n-k) if test -f \"$WINEPREFIX/mock.pid\"; then printf '%s\\n' \"$WINEPREFIX\" > \"$WINEPREFIX/quit-request\"; kill \"$(cat \"$WINEPREFIX/mock.pid\")\"; rm \"$WINEPREFIX/mock.pid\"; else exit 1; fi;;\n-w) test ! -f \"$WINEPREFIX/mock.pid\";;\n*) exit 64;;\nesac\n",
        )
        .unwrap();
        for executable in [&wine, &wineboot, &wineserver] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        wine
    }

    struct OwnedTestChild(Child);

    impl Drop for OwnedTestChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    fn file_holder(path: &Path) -> OwnedTestChild {
        use std::io::BufRead;
        let mut child = OwnedTestChild(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        child
    }

    struct PoliteFixture {
        home: PathBuf,
        bottle: Bottle,
        dir: PathBuf,
        wine: PathBuf,
    }

    impl PoliteFixture {
        fn new(script: &[u8]) -> Self {
            let home = test_home();
            let wine = fake_runtime(&home);
            let bottle = create(&home, "Polite quit fixture").unwrap();
            ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            for entry in fs::read_dir(dir.join("logs")).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
            fs::write(wine.parent().unwrap().join("wineboot"), script).unwrap();
            fs::write(
                wine.parent().unwrap().join("wineserver"),
                b"#!/bin/sh\ncase \"$1\" in -w) exec /bin/sleep 30;; *) touch \"$WINEPREFIX/unexpected-server-control\"; exit 9;; esac\n",
            )
            .unwrap();
            Self {
                home,
                bottle,
                dir,
                wine,
            }
        }

        fn request(&self) -> Result<QuitRequest, String> {
            request_quit_with_wine_checked(
                &self.home,
                &self.bottle.id,
                &self.wine,
                MOCK_PROCESS_TIMEOUT,
                |_| Ok(1),
            )
        }
    }

    impl Drop for PoliteFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    #[test]
    fn polite_activity_uses_real_owners_even_without_a_registry_or_saved_pid() {
        let home = test_home();
        let bottle = create(&home, "Uninitialized owner").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        assert!(!activity(&home, &bottle.id).unwrap().running);
        let held_file = dir.join("prefix/initialization.log");
        fs::write(&held_file, b"owned original fixture").unwrap();
        let mut holder = file_holder(&held_file);
        let state = activity(&home, &bottle.id).unwrap();
        assert!(state.running);
        assert_eq!(state.owner_count, 1);
        assert!(!dir.join("prefix/system.reg").exists());
        assert!(!dir.join("prefix-record.json").exists());
        assert!(!dir.join("active-processes.json").exists());
        drop(holder.0.stdin.take());
        assert!(holder.0.wait().unwrap().success());
        assert!(!activity(&home, &bottle.id).unwrap().running);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn polite_quit_uses_exact_end_session_flags_and_preserves_sibling_and_prefix() {
        let fixture = PoliteFixture::new(br#"#!/bin/sh
printf 'COUNT=%s\n' "$#"
printf 'ARG=%s\n' "$@"
printf 'PREFIX=%s\nBACKEND=%s\nCONFIG=%s\nDATA=%s\n' "$WINEPREFIX" "$CASKRUN_GRAPHICS_BACKEND" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"
printf 'PRIVATE=%s\n' "${WINESERVERSOCKET-}${WINELOADERNOEXEC-}${WINEPRELOADRESERVE-}${CASKRUN_NATIVE_CORE-}${CASKRUN_NATIVE_EXEC-}${CASKRUN_STEAM_MODE-}"
"#);
        let record = fs::read(fixture.dir.join("prefix-record.json")).unwrap();
        let registry = fs::read(fixture.dir.join("prefix/system.reg")).unwrap();
        let mapping = fs::read(fixture.dir.join("xdg/config/user-dirs.dirs")).unwrap();
        save_json(
            &fixture.dir.join("active-processes.json"),
            &vec![321_u32, 654],
        )
        .unwrap();
        let sibling = create(&fixture.home, "Untouched sibling").unwrap();
        let sibling_dir = bottle_dir(&fixture.home, &sibling.id).unwrap();
        fs::write(sibling_dir.join("active-processes.json"), b"[987]\n").unwrap();
        fs::write(sibling_dir.join("bottle.json"), b"broken sibling fixture").unwrap();
        let result = fixture.request().unwrap();
        assert!(result.request_submitted);
        assert!(!result.message.contains("All Windows apps"));
        let log = fs::read_to_string(result.log_path.unwrap()).unwrap();
        assert!(log.starts_with("COUNT=2\nARG=-e\nARG=-s\n"), "{log}");
        assert!(log.contains(&format!(
            "PREFIX={}\nBACKEND=wined3d\n",
            fixture.dir.join("prefix").display()
        )));
        assert!(log.contains(&format!(
            "CONFIG={}\nDATA={}\nPRIVATE=\n",
            fixture.dir.join("xdg/config").display(),
            fixture.dir.join("xdg/data").display()
        )));
        assert_eq!(read_active_processes(&fixture.dir).unwrap(), vec![321, 654]);
        assert_eq!(
            fs::read(sibling_dir.join("active-processes.json")).unwrap(),
            b"[987]\n"
        );
        assert_eq!(
            fs::read(fixture.dir.join("prefix-record.json")).unwrap(),
            record
        );
        assert_eq!(
            fs::read(fixture.dir.join("prefix/system.reg")).unwrap(),
            registry
        );
        assert_eq!(
            fs::read(fixture.dir.join("xdg/config/user-dirs.dirs")).unwrap(),
            mapping
        );
        assert!(!fixture.dir.join("native-apps").exists());
        assert!(
            !fixture
                .dir
                .join("prefix/unexpected-server-control")
                .exists()
        );
    }

    #[test]
    fn polite_quit_idle_unknown_and_runtime_mismatch_do_not_spawn_or_repair() {
        let fixture = PoliteFixture::new(b"#!/bin/sh\ntouch \"$WINEPREFIX/unexpected-spawn\"\n");
        let record = fs::read(fixture.dir.join("prefix-record.json")).unwrap();
        for counts in [vec![0], vec![1, 0]] {
            let mut counts = counts.into_iter();
            let result = request_quit_with_wine_checked(
                &fixture.home,
                &fixture.bottle.id,
                &fixture.wine,
                MOCK_PROCESS_TIMEOUT,
                |_| Ok(counts.next().unwrap()),
            )
            .unwrap();
            assert!(!result.request_submitted);
            assert!(result.log_path.is_none());
        }
        let error = request_quit_with_wine_checked(
            &fixture.home,
            &fixture.bottle.id,
            &fixture.wine,
            MOCK_PROCESS_TIMEOUT,
            |_| Err("ownership unknown".into()),
        )
        .unwrap_err();
        assert_eq!(error, "ownership unknown");
        let other_wine = fake_runtime(&fixture.home.join("other"));
        let error = request_quit_with_wine_checked(
            &fixture.home,
            &fixture.bottle.id,
            &other_wine,
            MOCK_PROCESS_TIMEOUT,
            |_| -> Result<usize, String> { panic!("runtime mismatch must precede inspection") },
        )
        .unwrap_err();
        assert!(error.contains("different Windows engine"));
        fs::remove_file(fixture.dir.join("xdg/config/user-dirs.dirs")).unwrap();
        fs::remove_dir(fixture.dir.join("xdg/config")).unwrap();
        assert!(fixture.request().is_err());
        assert!(!fixture.dir.join("xdg/config").exists());
        assert!(!fixture.dir.join("prefix/unexpected-spawn").exists());
        assert!(!fixture.dir.join("active-processes.json").exists());
        assert_eq!(
            fs::read(fixture.dir.join("prefix-record.json")).unwrap(),
            record
        );
        assert_eq!(fs::read_dir(fixture.dir.join("logs")).unwrap().count(), 0);
    }

    #[test]
    fn polite_quit_releases_all_bottle_handles_and_lock_while_its_child_waits() {
        let fixture = PoliteFixture::new(
            br#"#!/bin/sh
echo "$$" > "$WINEPREFIX/polite-control.pid"
while test ! -f "$WINEPREFIX/release-polite-control"; do sleep 0.02; done
exit 0
"#,
        );
        let dir = fixture.dir.clone();
        let observer = std::thread::spawn(move || {
            struct Release(PathBuf);
            impl Drop for Release {
                fn drop(&mut self) {
                    let _ = fs::write(&self.0, b"continue");
                }
            }
            let _release = Release(dir.join("prefix/release-polite-control"));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !dir.join("prefix/polite-control.pid").is_file() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
            let path = dir.join(".caskrun.lock");
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert!(
                lock.try_lock().is_ok(),
                "polite control retained its bottle lock"
            );
            let pid: u32 = fs::read_to_string(dir.join("prefix/polite-control.pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(read_active_processes(&dir).unwrap().contains(&pid));
            let output = Command::new("/usr/sbin/lsof")
                .args(["-nP", "-p", &std::process::id().to_string(), "-Fn"])
                .output()
                .unwrap();
            assert!(
                !String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(
                        |line| line.strip_prefix('n').is_some_and(|name| Path::new(name)
                            .starts_with(&dir)
                            && Path::new(name) != path)
                    ),
                "polite control retained a bottle handle"
            );
            pid
        });
        assert!(fixture.request().unwrap().request_submitted);
        let pid = observer.join().unwrap();
        assert!(!process_alive(pid).unwrap());
        assert!(read_active_processes(&fixture.dir).unwrap().is_empty());
    }

    #[test]
    fn polite_quit_cancel_and_timeout_reap_only_their_control_child() {
        for cancel in [true, false] {
            let fixture = PoliteFixture::new(b"#!/bin/sh\necho \"$$\" > \"$WINEPREFIX/polite-control.pid\"\nexec /bin/sleep 30\n");
            let mut holder = file_holder(&fixture.dir.join("prefix/system.reg"));
            save_json(
                &fixture.dir.join("active-processes.json"),
                &vec![holder.0.id(), 246_u32],
            )
            .unwrap();
            let (request, _guard) = cancellation::test_request();
            let dir = fixture.dir.clone();
            let observer = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !dir.join("prefix/polite-control.pid").is_file() {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
                if cancel {
                    request.store(true, std::sync::atomic::Ordering::Release);
                }
            });
            let timeout = if cancel {
                MOCK_PROCESS_TIMEOUT
            } else {
                Duration::from_millis(250)
            };
            let error = request_quit_with_wine_checked(
                &fixture.home,
                &fixture.bottle.id,
                &fixture.wine,
                timeout,
                |_| Ok(1),
            )
            .unwrap_err();
            observer.join().unwrap();
            assert!(
                if cancel {
                    cancellation::is_error(&error)
                } else {
                    error.contains("timed out")
                },
                "{error}"
            );
            let pid: u32 = fs::read_to_string(fixture.dir.join("prefix/polite-control.pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(!process_alive(pid).unwrap());
            assert!(holder.0.try_wait().unwrap().is_none());
            assert_eq!(
                read_active_processes(&fixture.dir).unwrap(),
                vec![holder.0.id(), 246]
            );
            assert!(
                !fixture
                    .dir
                    .join("prefix/unexpected-server-control")
                    .exists()
            );
            drop(holder.0.stdin.take());
            assert!(holder.0.wait().unwrap().success());
        }
    }

    #[test]
    fn polite_quit_refuses_file_holder_without_a_running_session_and_unknown_probe() {
        let fixture = PoliteFixture::new(b"#!/bin/sh\ntouch \"$WINEPREFIX/unexpected-spawn\"\n");
        let mut holder = file_holder(&fixture.dir.join("prefix/system.reg"));
        for exit in [0, 7] {
            fs::write(
                fixture.wine.parent().unwrap().join("wineserver"),
                format!("#!/bin/sh\ntest \"$1\" = '-w' || exit 9\nexit {exit}\n"),
            )
            .unwrap();
            let error = request_quit_with_wine_checked(
                &fixture.home,
                &fixture.bottle.id,
                &fixture.wine,
                MOCK_PROCESS_TIMEOUT,
                prefix_processes::owner_count,
            )
            .unwrap_err();
            assert!(
                error.contains(if exit == 0 {
                    "No running Windows session"
                } else {
                    "could not verify"
                }),
                "{error}"
            );
            assert!(holder.0.try_wait().unwrap().is_none());
            assert!(!fixture.dir.join("prefix/unexpected-spawn").exists());
            assert_eq!(fs::read_dir(fixture.dir.join("logs")).unwrap().count(), 0);
            assert!(!fixture.dir.join("active-processes.json").exists());
        }
        drop(holder.0.stdin.take());
        assert!(holder.0.wait().unwrap().success());
    }

    #[test]
    fn polite_quit_rechecks_independent_companion_and_refuses_a_changed_helper() {
        let fixture = PoliteFixture::new(b"#!/bin/sh\ntouch \"$WINEPREFIX/unexpected-spawn\"\n");
        let helper = fixture.wine.parent().unwrap().join("wineboot");
        let replacement = fixture.wine.parent().unwrap().join("replacement-helper");
        fs::copy(&helper, &replacement).unwrap();
        let mut calls = 0;
        let error = request_quit_with_wine_checked(
            &fixture.home,
            &fixture.bottle.id,
            &fixture.wine,
            MOCK_PROCESS_TIMEOUT,
            |_| {
                calls += 1;
                if calls == 2 {
                    fs::remove_file(&helper).unwrap();
                    symlink(&replacement, &helper).unwrap();
                }
                Ok(1)
            },
        )
        .unwrap_err();
        assert!(error.contains("helper changed"), "{error}");
        fs::remove_file(&helper).unwrap();
        let prohibited = fixture.home.join("CrossOver.app/Contents/test-helper");
        fs::create_dir_all(prohibited.parent().unwrap()).unwrap();
        fs::copy(&replacement, &prohibited).unwrap();
        symlink(&prohibited, &helper).unwrap();
        assert!(fixture.request().is_err());
        assert!(!fixture.dir.join("prefix/unexpected-spawn").exists());
        assert_eq!(fs::read_dir(fixture.dir.join("logs")).unwrap().count(), 0);
    }

    #[test]
    fn quit_stops_verified_native_clients_after_their_server_is_idle() {
        for hold in [false, true] {
            prefix_processes::with_native_shutdown_fixture(hold, |home, dir, wine, child| {
                let id = dir.file_name().unwrap().to_str().unwrap();
                save_json(
                    &dir.join("bottle.json"),
                    &Bottle {
                        id: id.into(),
                        name: "Original native quit fixture".into(),
                        created_at: 1,
                    },
                )
                .unwrap();
                save_json(
                    &dir.join("prefix-record.json"),
                    &PrefixRecord {
                        wine_path: wine.canonicalize().unwrap().display().to_string(),
                        runtime_identity: None,
                        runtime_fingerprint: None,
                    },
                )
                .unwrap();
                fs::write(dir.join("prefix/system.reg"), b"original fixture registry").unwrap();
                let server = wine.parent().unwrap().join("wineserver");
                fs::write(
                    &server,
                    b"#!/bin/sh\ncase \"$1\" in\n-k) exit 1;;\n-w) exit 0;;\n*) exit 64;;\nesac\n",
                )
                .unwrap();
                fs::set_permissions(&server, fs::Permissions::from_mode(0o700)).unwrap();
                let result =
                    quit_windows_apps_with_wine(home, id, wine, MOCK_PROCESS_TIMEOUT).unwrap();
                assert!(result.forced && result.server_stopped);
                assert_eq!(result.tracked_before, 0);
                assert_eq!(result.tracked_after, 0);
                assert_eq!(
                    child.wait().unwrap().signal(),
                    Some(if hold { 9 } else { 15 })
                );
            });
        }
    }

    #[test]
    fn quit_refuses_uncertain_owner_and_preserves_another_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let first = create(&home, "Uncertain owner").unwrap();
        let second = create(&home, "Other bottle").unwrap();
        ensure_prefix_with_wine(&home, &first.id, &wine).unwrap();
        ensure_prefix_with_wine(&home, &second.id, &wine).unwrap();
        let first_dir = bottle_dir(&home, &first.id).unwrap();
        let second_prefix = bottle_dir(&home, &second.id).unwrap().join("prefix");
        let log = real_subdirectory(&first_dir, "logs")
            .unwrap()
            .join("owner-fixture.log");
        fs::write(&log, b"fixture").unwrap();
        let mut owner = file_holder(&log);
        let mut other = OwnedTestChild(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        fs::write(second_prefix.join("mock.pid"), other.0.id().to_string()).unwrap();

        let result =
            quit_windows_apps_with_wine(&home, &first.id, &wine, Duration::from_millis(500));
        assert!(result.unwrap_err().contains("still use this bottle"));
        assert!(owner.0.try_wait().unwrap().is_none());
        assert!(other.0.try_wait().unwrap().is_none());
        assert!(!second_prefix.join("quit-request").exists());
        drop(owner.0.stdin.take());
        owner.0.wait().unwrap();
        other.0.kill().unwrap();
        other.0.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn quit_does_not_claim_success_or_signal_an_uncertain_tracked_pid() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Uncertain tracking").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let mut child = OwnedTestChild(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        save_json(&dir.join("active-processes.json"), &vec![child.0.id()]).unwrap();
        let error = quit_windows_apps_with_wine(&home, &bottle.id, &wine, MOCK_PROCESS_TIMEOUT)
            .unwrap_err();
        assert!(error.contains("still has tracked processes"));
        assert!(child.0.try_wait().unwrap().is_none());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn quit_keeps_the_prefix_identity_through_the_server_wait() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Replaced prefix").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(
            wine.parent().unwrap().join("wineserver"),
            b"#!/bin/sh\ncase \"$1\" in\n-k) exit 1;;\n-w) mv \"$WINEPREFIX\" \"$WINEPREFIX-original\"; mkdir \"$WINEPREFIX\"; touch \"$WINEPREFIX/system.reg\"; exit 0;;\nesac\n",
        )
        .unwrap();
        let error = quit_windows_apps_with_wine(&home, &bottle.id, &wine, MOCK_PROCESS_TIMEOUT)
            .unwrap_err();
        assert!(error.contains("prefix changed"));
        assert!(dir.join("prefix-original").is_dir());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancel_before_quit_prevents_the_server_stop_request() {
        use std::sync::atomic::Ordering;
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Canceled quit").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let mut child = OwnedTestChild(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        fs::write(prefix.join("mock.pid"), child.0.id().to_string()).unwrap();
        let (request, _guard) = cancellation::test_request();
        request.store(true, Ordering::Release);
        let error = quit_windows_apps_with_wine(&home, &bottle.id, &wine, MOCK_PROCESS_TIMEOUT)
            .unwrap_err();
        assert!(cancellation::is_error(&error));
        assert!(!prefix.join("quit-request").exists());
        assert!(child.0.try_wait().unwrap().is_none());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancel_during_quit_is_deferred_through_final_owner_verification() {
        use std::sync::atomic::Ordering;
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Quit cleanup").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        fs::write(
            wine.parent().unwrap().join("wineserver"),
            b"#!/bin/sh\ncase \"$1\" in\n-k) touch \"$WINEPREFIX/quit-request\"; while test ! -f \"$WINEPREFIX/continue-quit\"; do sleep 0.02; done; exit 0;;\n-w) exit 0;;\n*) exit 64;;\nesac\n",
        )
        .unwrap();
        let (request, _guard) = cancellation::test_request();
        let cancel_prefix = prefix.clone();
        let cancel = std::thread::spawn(move || {
            let deadline = Instant::now() + MOCK_PROCESS_TIMEOUT;
            while !cancel_prefix.join("quit-request").exists() {
                assert!(Instant::now() < deadline, "quit request did not start");
                std::thread::sleep(Duration::from_millis(10));
            }
            request.store(true, Ordering::Release);
            fs::write(cancel_prefix.join("continue-quit"), b"continue").unwrap();
        });
        let result = quit_windows_apps_with_wine(&home, &bottle.id, &wine, MOCK_PROCESS_TIMEOUT);
        cancel.join().unwrap();
        let result = result.unwrap();
        assert!(result.forced && result.server_stopped);
        assert_eq!(result.tracked_after, 0);
        assert!(cancellation::is_requested());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn quit_targets_only_the_selected_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let first = create(&home, "First").unwrap();
        let second = create(&home, "Second").unwrap();
        ensure_prefix_with_wine(&home, &first.id, &wine).unwrap();
        ensure_prefix_with_wine(&home, &second.id, &wine).unwrap();
        let first_prefix = bottle_dir(&home, &first.id).unwrap().join("prefix");
        let second_prefix = bottle_dir(&home, &second.id).unwrap().join("prefix");
        let mut first_child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let mut second_child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        fs::write(first_prefix.join("mock.pid"), first_child.id().to_string()).unwrap();
        fs::write(
            second_prefix.join("mock.pid"),
            second_child.id().to_string(),
        )
        .unwrap();
        let result =
            quit_windows_apps_with_wine(&home, &first.id, &wine, MOCK_PROCESS_TIMEOUT).unwrap();
        assert!(result.forced && result.server_stopped);
        assert_eq!(result.bottle_id, first.id);
        assert_eq!(
            fs::read_to_string(first_prefix.join("quit-request"))
                .unwrap()
                .trim(),
            first_prefix.display().to_string()
        );
        assert!(!second_prefix.join("quit-request").exists());
        assert!(first_child.wait().unwrap().signal().is_some());
        assert!(second_child.try_wait().unwrap().is_none());
        assert!(Path::new(&result.log_path).exists());
        second_child.kill().unwrap();
        second_child.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn force_quit_recovers_recorded_session_without_registry_and_preserves_sibling() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let first = create(&home, "Recorded session").unwrap();
        let second = create(&home, "Unrelated session").unwrap();
        ensure_prefix_with_wine(&home, &first.id, &wine).unwrap();
        ensure_prefix_with_wine(&home, &second.id, &wine).unwrap();
        let first_dir = bottle_dir(&home, &first.id).unwrap();
        let second_dir = bottle_dir(&home, &second.id).unwrap();
        let record = fs::read(first_dir.join("prefix-record.json")).unwrap();
        let mut target = OwnedTestChild(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let mut sibling = file_holder(&second_dir.join("prefix/system.reg"));
        fs::write(first_dir.join("prefix/mock.pid"), target.0.id().to_string()).unwrap();
        fs::remove_file(first_dir.join("prefix/system.reg")).unwrap();
        let result =
            quit_windows_apps_with_wine(&home, &first.id, &wine, MOCK_PROCESS_TIMEOUT).unwrap();
        assert!(result.forced && result.server_stopped);
        assert!(target.0.wait().unwrap().signal().is_some());
        assert!(sibling.0.try_wait().unwrap().is_none());
        assert!(!first_dir.join("prefix/system.reg").exists());
        assert_eq!(
            fs::read(first_dir.join("prefix-record.json")).unwrap(),
            record
        );
        assert!(!second_dir.join("prefix/quit-request").exists());
        drop(sibling.0.stdin.take());
        assert!(sibling.0.wait().unwrap().success());
        let empty = create(&home, "Never initialized").unwrap();
        assert!(
            quit_windows_apps_with_wine(&home, &empty.id, &wine, MOCK_PROCESS_TIMEOUT).is_err()
        );
        assert!(
            !bottle_dir(&home, &empty.id)
                .unwrap()
                .join("prefix-record.json")
                .exists()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn quit_refuses_other_runtime_and_symlinked_prefix() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Control").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let other = home.join("other-runtime");
        fs::create_dir(&other).unwrap();
        for name in ["wine", "wineboot", "wineserver"] {
            fs::copy(wine.parent().unwrap().join(name), other.join(name)).unwrap();
        }
        assert!(
            quit_windows_apps_with_wine(
                &home,
                &bottle.id,
                &other.join("wine"),
                Duration::from_secs(1)
            )
            .is_err()
        );
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        assert!(!dir.join("prefix/quit-request").exists());
        fs::rename(dir.join("prefix"), dir.join("old-prefix")).unwrap();
        symlink(dir.join("old-prefix"), dir.join("prefix")).unwrap();
        assert!(
            quit_windows_apps_with_wine(&home, &bottle.id, &wine, Duration::from_secs(1)).is_err()
        );
        assert!(!dir.join("old-prefix/quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn already_idle_bottle_is_reported_without_claiming_a_force_quit() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Idle").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let result =
            quit_windows_apps_with_wine(&home, &bottle.id, &wine, MOCK_PROCESS_TIMEOUT).unwrap();
        assert!(result.server_stopped);
        assert!(!result.forced);
        assert!(result.message.contains("No active Wine server"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn opens_only_the_bottle_local_uninstaller() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Apps").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let system32 = prefix.join("drive_c/windows/system32");
        fs::create_dir_all(&system32).unwrap();
        let uninstaller = system32.join("uninstaller.exe");
        fs::write(&uninstaller, b"MZmock").unwrap();
        let launch = open_uninstaller_with_wine(&home, &bottle.id, &wine).unwrap();
        let deadline = Instant::now() + MOCK_PROCESS_TIMEOUT;
        while !prefix.join("opened-uninstaller").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            fs::read_to_string(prefix.join("opened-uninstaller"))
                .unwrap()
                .trim(),
            uninstaller.display().to_string()
        );
        assert!(Path::new(&launch.log_path).exists());
        fs::remove_file(&uninstaller).unwrap();
        symlink("/bin/sleep", &uninstaller).unwrap();
        assert!(open_uninstaller_with_wine(&home, &bottle.id, &wine).is_err());
        fs::remove_dir_all(home).unwrap();
    }
}
