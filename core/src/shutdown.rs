//! Shut down Windows apps in Caskrun-owned bottles before quitting the host UI.
//!
//! Every stop request goes through the existing per-bottle controls. We never
//! execute an engine named only by bottle metadata or signal saved process IDs.

use super::*;
use std::io::Write;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Serialize)]
pub(crate) struct ShutdownFailure {
    pub(crate) bottle_id: String,
    pub(crate) name: String,
    pub(crate) error: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShutdownResult {
    pub(crate) complete: bool,
    pub(crate) failures: Vec<ShutdownFailure>,
    /// Bottles that received a successful stop request. Already-idle bottles
    /// are verified or skipped, but do not inflate this count.
    pub(crate) stopped_bottles: usize,
}

pub(crate) fn quit_all(home: &Path) -> Result<ShutdownResult, String> {
    let wine = wine_path();
    quit_all_with_wine(home, wine.as_deref(), SHUTDOWN_TIMEOUT)
}

fn quit_all_with_wine(
    home: &Path,
    wine: Option<&Path>,
    timeout: Duration,
) -> Result<ShutdownResult, String> {
    let root = storage_root(home, false)?;
    // Do not let a dangling storage symlink masquerade as an empty library.
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShutdownResult {
                complete: true,
                failures: Vec::new(),
                stopped_bottles: 0,
            });
        }
        Err(error) => return Err(format!("Could not inspect Caskrun bottle storage: {error}")),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err("Caskrun bottle storage must be a real directory".into());
        }
        Ok(_) => {}
    }
    let (root_guard, root_snapshot) = open_stable_directory(&root)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("Could not list Caskrun bottles: {e}"))? {
        let entry = entry.map_err(|e| format!("Could not read Caskrun bottle entry: {e}"))?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if Uuid::parse_str(&id).is_ok() {
            candidates.push(id);
        }
    }
    candidates.sort();
    let mut result = ShutdownResult {
        complete: true,
        failures: Vec::new(),
        stopped_bottles: 0,
    };
    for id in candidates {
        // A malformed/symlinked UUID directory is reported rather than being
        // silently omitted by the normal library listing. Other valid bottles
        // must still receive their own independent shutdown attempt.
        let candidate = bottle_control::assert_same_prefix(&root, &root_guard, &root_snapshot)
            .and_then(|_| bottle_dir(home, &id))
            .and_then(|dir| read_bottle_record(&dir, &id));
        let bottle = match candidate {
            Ok(bottle) => bottle,
            Err(error) => {
                result.failures.push(ShutdownFailure {
                    bottle_id: id.clone(),
                    name: id,
                    error,
                });
                continue;
            }
        };
        match quit_one(home, &bottle.id, wine, timeout) {
            Ok(stopped) => result.stopped_bottles += usize::from(stopped),
            Err(error) => result.failures.push(ShutdownFailure {
                bottle_id: bottle.id,
                name: bottle.name,
                error,
            }),
        }
    }
    result.complete = result.failures.is_empty();
    Ok(result)
}

fn quit_one(home: &Path, id: &str, wine: Option<&Path>, timeout: Duration) -> Result<bool, String> {
    let registry_missing_result = {
        let _lock = lock_bottle(home, id)?;
        let dir = bottle_dir(home, id)?;
        let prefix = dir.join("prefix");
        let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
        let record = recorded_runtime_for_prefix(&dir, wine)?;
        let has_record = path_is_real_file(&dir.join("prefix-record.json"));

        if record.is_none() {
            // Fresh bottles cannot have a Wine server without initialization.
            // Live tracking means this is inconsistent, so do not claim idle.
            if !live_processes(&dir)?.is_empty() {
                return Err("This uninitialized bottle still has tracked processes. Caskrun cannot safely verify that its Windows apps stopped.".into());
            }
            bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
            return Ok(false);
        }

        if !has_record {
            // A copied/imported registry is not proof of a running Caskrun
            // engine. A read-only, prefix-scoped wait can establish idle using
            // the selected runtime without adopting or executing source code.
            refuse_live_wine_server(&dir, timeout, wine)?;
            if !live_processes(&dir)?.is_empty() {
                return Err("Wine is idle, but this bottle still has tracked processes. Caskrun cannot safely verify that they stopped.".into());
            }
            bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
            return Ok(false);
        }
        if !path_is_real_file(&prefix.join("system.reg")) {
            // A previously initialized app can keep running after registry
            // loss. Its independently verified runtime record and prefix inode
            // still identify its server; shutdown must not require a repair.
            Some(quit_recorded_prefix_without_registry(
                &dir,
                &prefix,
                &prefix_guard,
                &prefix_snapshot,
                wine,
                timeout,
            )?)
        } else {
            None
        }
    };
    // The existing control acquires its own lock and repeats all prefix and
    // runtime checks immediately before issuing WINEPREFIX-scoped commands.
    let wine = wine.ok_or("Caskrun cannot verify or stop this initialized bottle because its included Wine runtime is unavailable. Restore the runtime or close its Windows apps before quitting.")?;
    let (forced, tracked_after) = match registry_missing_result {
        Some(stopped) => stopped,
        None => {
            let stopped = bottle_control::quit_windows_apps_with_wine(home, id, wine, timeout)?;
            if !stopped.server_stopped {
                return Err("Wine did not confirm that this bottle's server stopped".into());
            }
            (stopped.forced, stopped.tracked_after)
        }
    };
    if tracked_after > 0 {
        // A direct Wine loader can briefly outlive its server. Wait for that
        // exact tracked process set to disappear; never send it a signal.
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = {
                let _lock = lock_bottle(home, id)?;
                live_processes(&bottle_dir(home, id)?)?.len()
            };
            if remaining == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("Wine's server stopped, but {remaining} tracked process(es) still appear active in this bottle. Caskrun did not signal saved process IDs."));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(forced)
}

/// Only shutdown allows an absent system.reg. The caller holds the bottle lock
/// and a no-follow prefix guard; every selected-engine check remains required.
fn quit_recorded_prefix_without_registry(
    dir: &Path,
    prefix: &Path,
    prefix_guard: &File,
    prefix_snapshot: &fs::Metadata,
    wine: Option<&Path>,
    timeout: Duration,
) -> Result<(bool, usize), String> {
    let wine = wine.ok_or("Caskrun cannot verify or stop this initialized bottle because its included Wine runtime is unavailable. Restore the runtime or close its Windows apps before quitting.")?;
    if !wine.is_absolute() || !is_usable_wine(wine) {
        return Err("Selected Wine runtime is unavailable or not independent".into());
    }
    let record = recorded_runtime_for_prefix(dir, Some(wine))?
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
    let logs = real_subdirectory(dir, "logs")?;
    let log_path = logs.join(format!("launch-{}-{}.log", now_seconds()?, Uuid::new_v4()));
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| format!("Could not create bottle-shutdown log: {e}"))?;
    writeln!(
        log,
        "Caskrun: quit requested for the verified prefix {} after registry loss.",
        prefix.display()
    )
    .map_err(|e| format!("Could not write bottle-shutdown log: {e}"))?;
    bottle_control::assert_same_prefix(prefix, prefix_guard, prefix_snapshot)?;
    let _timeout_cleanup = cancellation::begin_timeout_cleanup()?;
    let (kill_status, kill_timed_out) =
        bottle_control::run_wineserver(&wineserver, prefix, "-k", &log, timeout)?;
    if kill_timed_out {
        return Err(format!(
            "Wine's quit request timed out for this bottle. Check {}",
            log_path.display()
        ));
    }
    bottle_control::assert_same_prefix(prefix, prefix_guard, prefix_snapshot)?;
    let (wait_status, wait_timed_out) =
        bottle_control::run_wineserver(&wineserver, prefix, "-w", &log, timeout)?;
    if wait_timed_out || !wait_status.success() {
        return Err(format!("Wine received the quit request, but this bottle's server did not stop within the limit. Check {}", log_path.display()));
    }
    Ok((kill_status.success(), live_processes(dir)?.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-shutdown-test-{}", Uuid::new_v4()))
    }

    fn fake_runtime(home: &Path) -> PathBuf {
        let runtime = home.join("test-runtime");
        fs::create_dir_all(&runtime).unwrap();
        let wine = runtime.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::write(
            runtime.join("wineboot"),
            b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        fs::write(
            runtime.join("wineserver"),
            b"#!/bin/sh\ncase \"$1\" in\n-k) printf '%s\\n' \"$WINEPREFIX\" >> \"$WINEPREFIX/quit-request\"; if test -f \"$WINEPREFIX/fail-quit\"; then exit 1; elif test -f \"$WINEPREFIX/mock.pid\"; then kill \"$(cat \"$WINEPREFIX/mock.pid\")\"; rm \"$WINEPREFIX/mock.pid\"; else exit 1; fi;;\n-w) test ! -f \"$WINEPREFIX/mock.pid\" && test ! -f \"$WINEPREFIX/fail-quit\";;\n*) exit 64;;\nesac\n",
        )
        .unwrap();
        for name in ["wine", "wineboot", "wineserver"] {
            let path = runtime.join(name);
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
        wine
    }

    fn initialized(home: &Path, name: &str, wine: &Path) -> (Bottle, PathBuf) {
        let bottle = create(home, name).unwrap();
        ensure_prefix_with_wine(home, &bottle.id, wine).unwrap();
        let prefix = bottle_dir(home, &bottle.id).unwrap().join("prefix");
        (bottle, prefix)
    }

    fn fake_app(prefix: &Path) -> Child {
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        fs::write(prefix.join("mock.pid"), child.id().to_string()).unwrap();
        child
    }

    #[test]
    fn stops_all_own_bottles_without_touching_other_home() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, first_prefix) = initialized(&home, "Steam", &wine);
        let (_, second_prefix) = initialized(&home, "Editor", &wine);
        let other_home = home.join("other-owner");
        let (_, other_prefix) = initialized(&other_home, "Other Wine user", &wine);
        let mut first = fake_app(&first_prefix);
        let mut second = fake_app(&second_prefix);
        let mut other = fake_app(&other_prefix);

        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 2);
        assert!(first.wait().unwrap().signal().is_some());
        assert!(second.wait().unwrap().signal().is_some());
        assert!(other.try_wait().unwrap().is_none());
        assert!(!other_prefix.join("quit-request").exists());
        for prefix in [&first_prefix, &second_prefix] {
            assert_eq!(
                fs::read_to_string(prefix.join("quit-request"))
                    .unwrap()
                    .trim(),
                prefix.display().to_string()
            );
        }
        other.kill().unwrap();
        other.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failure_does_not_skip_other_bottles_and_identifies_failed_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (failed, failed_prefix) = initialized(&home, "Will not stop", &wine);
        let (_, healthy_prefix) = initialized(&home, "Stops normally", &wine);
        fs::write(failed_prefix.join("fail-quit"), b"mock failure").unwrap();
        let mut healthy = fake_app(&healthy_prefix);

        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.stopped_bottles, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].bottle_id, failed.id);
        assert_eq!(result.failures[0].name, "Will not stop");
        assert!(result.failures[0].error.contains("server did not stop"));
        assert!(healthy.wait().unwrap().signal().is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn empty_and_uninitialized_bottles_need_no_runtime() {
        let home = test_home();
        assert!(
            quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT)
                .unwrap()
                .complete
        );
        create(&home, "Not installed yet").unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 0);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn verifies_idle_import_without_record_and_does_not_initialize_it() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Imported data", &wine);
        let record = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        fs::remove_file(&record).unwrap();
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 0);
        assert!(!record.exists());
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_runtime_does_not_execute_the_recorded_engine() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Running or unverifiable", &wine);
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        assert!(result.failures[0].error.contains("runtime is unavailable"));
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_registry_does_not_prevent_stopping_a_verified_running_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, prefix) = initialized(&home, "Damaged registry", &wine);
        let mut child = fake_app(&prefix);
        fs::remove_file(prefix.join("system.reg")).unwrap();
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 1);
        assert!(child.wait().unwrap().signal().is_some());
        assert!(!prefix.join("system.reg").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_registry_fallback_keeps_the_selected_runtime_guard() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, prefix) = initialized(&home, "Damaged registry", &wine);
        fs::remove_file(prefix.join("system.reg")).unwrap();
        let other_wine = fake_runtime(&home.join("other-engine"));
        let result = quit_all_with_wine(&home, Some(&other_wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert!(result.failures[0]
            .error
            .contains("different Windows engine"));
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_wrong_engine_without_stopping_its_bottle() {
        let home = test_home();
        let first_wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Pinned engine", &first_wine);
        let second_wine = fake_runtime(&home.join("other-engine"));
        let result = quit_all_with_wine(&home, Some(&second_wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        assert!(result.failures[0]
            .error
            .contains("different Windows engine"));
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn symlinked_prefix_is_a_failure_but_valid_bottles_are_attempted() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (invalid, invalid_prefix) = initialized(&home, "Unsafe path", &wine);
        let (_, valid_prefix) = initialized(&home, "Valid", &wine);
        let outside = home.join("unowned-prefix");
        fs::rename(&invalid_prefix, &outside).unwrap();
        symlink(&outside, &invalid_prefix).unwrap();
        let mut valid = fake_app(&valid_prefix);
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, invalid.id);
        assert_eq!(result.stopped_bottles, 1);
        assert!(!outside.join("quit-request").exists());
        assert!(valid.wait().unwrap().signal().is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn symlinked_or_malformed_bottle_record_is_not_silently_skipped() {
        let home = test_home();
        let bottle = create(&home, "Unsafe metadata").unwrap();
        let metadata = bottle_dir(&home, &bottle.id).unwrap().join("bottle.json");
        let target = home.join("unowned.json");
        fs::rename(&metadata, &target).unwrap();
        symlink(&target, &metadata).unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn live_tracking_in_uninitialized_prefix_is_not_killed_or_reported_idle() {
        let home = test_home();
        let bottle = create(&home, "Inconsistent tracking").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        save_json(&dir.join("active-processes.json"), &vec![child.id()]).unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert!(result.failures[0].error.contains("tracked processes"));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_dangling_storage_symlink() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        symlink(home.join("nonexistent"), home.join("bottles")).unwrap();
        assert!(quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).is_err());
        fs::remove_dir_all(home).unwrap();
    }
}
