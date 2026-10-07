//! Wine's own configuration tools for a single, initialized Caskrun bottle.
//!
//! These are fixed upstream Wine programs, not an arbitrary command runner.

use super::*;
use std::io::Write;

const REBOOT_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Serialize)]
pub(super) struct RebootResult {
    pub(super) message: String,
    pub(super) log_path: String,
}

#[derive(Clone, Copy)]
enum WineTool {
    Configuration,
    RegistryEditor,
    GameControllers,
    TaskManager,
    InternetSettings,
    CommandPrompt,
}

impl WineTool {
    fn components(self) -> &'static [&'static str] {
        match self {
            Self::Configuration => &["drive_c", "windows", "system32", "winecfg.exe"],
            Self::RegistryEditor => &["drive_c", "windows", "regedit.exe"],
            Self::GameControllers | Self::InternetSettings => {
                &["drive_c", "windows", "system32", "control.exe"]
            }
            Self::TaskManager => &["drive_c", "windows", "system32", "taskmgr.exe"],
            Self::CommandPrompt => &["drive_c", "windows", "system32", "wineconsole.exe"],
        }
    }

    fn companion_components(self) -> Option<&'static [&'static str]> {
        match self {
            Self::GameControllers => Some(&["drive_c", "windows", "system32", "joy.cpl"]),
            Self::InternetSettings => Some(&["drive_c", "windows", "system32", "inetcpl.cpl"]),
            Self::CommandPrompt => Some(&["drive_c", "windows", "system32", "cmd.exe"]),
            _ => None,
        }
    }

    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::GameControllers => &["joy.cpl"],
            Self::InternetSettings => &["inetcpl.cpl"],
            Self::CommandPrompt => &[r"C:\windows\system32\cmd.exe"],
            _ => &[],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Configuration => "Wine Configuration",
            Self::RegistryEditor => "Wine Registry Editor",
            Self::GameControllers => "Game Controllers",
            Self::TaskManager => "Wine Task Manager",
            Self::InternetSettings => "Internet Settings",
            Self::CommandPrompt => "Windows Command Prompt",
        }
    }
}

/// Open Wine's graphical configuration editor for this bottle. Its settings
/// affect only this prefix. This should be exposed as an Advanced action.
pub(super) fn open_wine_configuration(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::Configuration)
}

/// Open Wine's graphical registry editor for this bottle. Registry edits can
/// affect installed Windows apps, so this should be an Advanced action.
pub(super) fn open_registry_editor(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::RegistryEditor)
}

/// Open Wine's built-in controller configuration panel. Controller detection
/// still depends on the host device and Wine's driver support.
pub(super) fn open_game_controllers(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::GameControllers)
}

/// Show Wine's process list for this bottle.
pub(super) fn open_task_manager(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::TaskManager)
}

/// Open Wine's Internet Options control panel. Individual settings may be
/// partially implemented by upstream Wine.
pub(super) fn open_internet_settings(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::InternetSettings)
}

/// Open this bottle's fixed cmd.exe in Wine's graphical console window.
/// The console wrapper is needed because the normal launcher redirects
/// standard input and output to a log rather than an interactive terminal.
pub(super) fn open_command_prompt(home: &Path, id: &str) -> Result<LaunchResult, String> {
    open_tool(home, id, WineTool::CommandPrompt)
}

/// Ask Wine to end this bottle's Windows session and start it again. This
/// closes running apps in the bottle, so callers should request confirmation.
pub(super) fn simulate_reboot(home: &Path, id: &str) -> Result<RebootResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    simulate_reboot_with_wine(home, id, &wine, REBOOT_TIMEOUT)
}

fn simulate_reboot_with_wine(
    home: &Path,
    id: &str,
    wine: &Path,
    timeout: Duration,
) -> Result<RebootResult, String> {
    let _lock = lock_bottle(home, id)?;
    let (dir, prefix, _wineserver, prefix_guard, prefix_snapshot) =
        bottle_control::checked_control_paths(home, id, wine)?;
    let wineboot = wine
        .parent()
        .ok_or("Wine runtime has no parent directory")?
        .join("wineboot");
    if !is_executable(&wineboot) || !is_independent_runtime(&wineboot) {
        return Err("This independent Wine runtime has no companion wineboot".into());
    }
    let logs = real_subdirectory(&dir, "logs")?;
    let log_path = logs.join(format!("launch-{}-{}.log", now_seconds()?, Uuid::new_v4()));
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| format!("Could not create reboot log: {e}"))?;
    writeln!(
        log,
        "Caskrun: Wine session restart requested for bottle {id}. WINEPREFIX={}",
        prefix.display()
    )
    .map_err(|e| format!("Could not write reboot log: {e}"))?;
    let stdout = log.try_clone().map_err(|e| e.to_string())?;
    let stderr = log.try_clone().map_err(|e| e.to_string())?;
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    let mut command = Command::new(wineboot);
    bottle_xdg::for_bottle(&dir)?.apply(&mut command);
    // In pinned Wine 11.0, --restart alone does not end the current session.
    // --end-session requests a clean close; --kill ends remaining processes
    // only after that succeeds. Normal startup then runs inside this prefix.
    let mut child = command
        .args(["--end-session", "--kill"])
        .env("WINEPREFIX", &prefix)
        .env_remove("CASKRUN_STEAM_MODE")
        .env_remove("CASKRUN_STEAM_ICON")
        .current_dir(&prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("Could not start Wine's simulated reboot: {e}"))?;
    let (status, timed_out) = wait_child_bounded_status(&mut child, timeout)?;
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    if timed_out {
        return Err(format!(
            "Wine's simulated reboot timed out. Check {}",
            log_path.display()
        ));
    }
    if !status.success() {
        return Err(format!(
            "Wine could not finish the simulated reboot (status {:?}). Check {}",
            status.code(),
            log_path.display()
        ));
    }
    Ok(RebootResult {
        message: "Windows session restarted in this bottle.".into(),
        log_path: log_path.display().to_string(),
    })
}

fn open_tool(home: &Path, id: &str, tool: WineTool) -> Result<LaunchResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    open_tool_with_wine(home, id, &wine, tool)
}

fn open_tool_with_wine(
    home: &Path,
    id: &str,
    wine: &Path,
    tool: WineTool,
) -> Result<LaunchResult, String> {
    if !wine.is_absolute() || !is_usable_wine(wine) {
        return Err("Selected Wine runtime is unavailable or not independent".into());
    }
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    require_initialized(&dir, &prefix)?;
    let record = recorded_runtime_for_prefix(&dir, Some(wine))?
        .ok_or("This bottle has no recorded Wine runtime")?;
    if !runtime_record_matches(&record, wine)? {
        return Err(
            "This bottle uses a different Windows engine. Select its recorded engine.".into(),
        );
    }
    let executable = checked_tool_path(&prefix, tool)?;
    let arguments: Vec<String> = tool
        .arguments()
        .iter()
        .map(|value| (*value).into())
        .collect();
    let (child, log_path) = start_launch_with_wine_checked(
        home,
        id,
        &executable,
        wine,
        LaunchFlags {
            quiet_msi: false,
            steam_mode: false,
        },
        &arguments,
        |resolved| {
            // The launch helper holds this bottle's lock here and rechecks
            // the selected runtime before spawning Wine. Revalidate the
            // exact, in-prefix program after that potentially long step.
            let dir = bottle_dir(home, id)?;
            let prefix = dir.join("prefix");
            require_initialized(&dir, &prefix)?;
            let checked = checked_tool_path(&prefix, tool)?;
            if resolved != checked {
                return Err(format!("{} changed before launch", tool.label()));
            }
            Ok(())
        },
    )?;
    Ok(LaunchResult {
        pid: child.id(),
        log_path: log_path.display().to_string(),
    })
}

fn require_initialized(dir: &Path, prefix: &Path) -> Result<(), String> {
    let _ = open_stable_directory(prefix)
        .map_err(|_| "Initialize this bottle before opening Wine tools".to_string())?;
    if !path_is_real_file(&prefix.join("system.reg")) {
        return Err("Initialize this bottle before opening Wine tools".into());
    }
    if !path_is_real_file(&dir.join("prefix-record.json")) {
        return Err(
            "Open this bottle with its independent Wine engine before using Wine tools".into(),
        );
    }
    Ok(())
}

fn checked_tool_path(prefix: &Path, tool: WineTool) -> Result<PathBuf, String> {
    let executable = checked_bottle_file(prefix, tool.components(), tool.label())?;
    if let Some(companion) = tool.companion_components() {
        // The control panel applets and the console's cmd.exe are fixed
        // in-prefix targets. Recheck them before and under the launch lock.
        let _ = checked_bottle_file(prefix, companion, tool.label())?;
    }
    Ok(executable)
}

fn checked_bottle_file(prefix: &Path, components: &[&str], label: &str) -> Result<PathBuf, String> {
    let _ = open_stable_directory(prefix)?;
    let mut parent = prefix.to_path_buf();
    for component in &components[..components.len() - 1] {
        parent.push(component);
        let _ = open_stable_directory(&parent)?;
    }
    let path = parent.join(components[components.len() - 1]);
    if !path_is_real_file(&path) {
        return Err(format!("{label} is missing from this bottle"));
    }
    let canonical_prefix = prefix
        .canonicalize()
        .map_err(|e| format!("Could not resolve bottle prefix: {e}"))?;
    let expected = components
        .iter()
        .fold(canonical_prefix, |path, component| path.join(component));
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("Could not resolve {label}: {e}"))?;
    if resolved != expected {
        return Err(format!("{label} must remain inside this bottle"));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-wine-tools-test-{}", Uuid::new_v4()))
    }

    fn fake_runtime(home: &Path) -> PathBuf {
        let runtime = home.join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        let wine = runtime.join("wine");
        let wineboot = runtime.join("wineboot");
        let wineserver = runtime.join("wineserver");
        fs::write(
            &wine,
            b"#!/bin/sh\nprintf '%s|%s|%s\\n' \"$WINEPREFIX\" \"$1\" \"${2-}\" >> \"$WINEPREFIX/tool-invocations\"\n",
        )
        .unwrap();
        fs::write(
            &wineboot,
            b"#!/bin/sh\nprintf '%s|%s|%s\n' \"$WINEPREFIX\" \"${1-}\" \"${2-}\" >> \"$WINEPREFIX/wineboot-invocations\"\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        fs::write(&wineserver, b"#!/bin/sh\nexit 0\n").unwrap();
        for executable in [&wine, &wineboot, &wineserver] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        wine
    }

    fn install_fake_tools(prefix: &Path) {
        let system32 = prefix.join("drive_c/windows/system32");
        fs::create_dir_all(&system32).unwrap();
        fs::write(system32.join("winecfg.exe"), b"MZfake winecfg").unwrap();
        fs::write(system32.join("control.exe"), b"MZfake control").unwrap();
        fs::write(system32.join("taskmgr.exe"), b"MZfake taskmgr").unwrap();
        fs::write(system32.join("wineconsole.exe"), b"MZfake wineconsole").unwrap();
        fs::write(system32.join("cmd.exe"), b"MZfake cmd").unwrap();
        fs::write(system32.join("joy.cpl"), b"MZfake joy").unwrap();
        fs::write(system32.join("inetcpl.cpl"), b"MZfake internet").unwrap();
        fs::write(
            prefix.join("drive_c/windows/regedit.exe"),
            b"MZfake regedit",
        )
        .unwrap();
    }

    fn wait_for_invocations(prefix: &Path, expected_lines: usize) -> String {
        let path = prefix.join("tool-invocations");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let contents = fs::read_to_string(&path).unwrap_or_default();
            if contents.lines().count() >= expected_lines || Instant::now() >= deadline {
                return contents;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn launches_only_fixed_tools_inside_the_selected_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Tools").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        install_fake_tools(&prefix);

        let config =
            open_tool_with_wine(&home, &bottle.id, &wine, WineTool::Configuration).unwrap();
        let registry =
            open_tool_with_wine(&home, &bottle.id, &wine, WineTool::RegistryEditor).unwrap();
        let invocations = wait_for_invocations(&prefix, 2);
        assert!(invocations.contains(&format!(
                "{}|{}",
                prefix.display(),
                prefix
                    .join("drive_c/windows/system32/winecfg.exe")
                    .display()
            )));
        assert!(invocations.contains(&format!(
            "{}|{}",
            prefix.display(),
            prefix.join("drive_c/windows/regedit.exe").display()
        )));
        assert!(Path::new(&config.log_path).exists());
        assert!(Path::new(&registry.log_path).exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn opens_controller_task_and_internet_panels_with_fixed_arguments() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "More tools").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        install_fake_tools(&prefix);

        for tool in [
            WineTool::GameControllers,
            WineTool::TaskManager,
            WineTool::InternetSettings,
        ] {
            let launched = open_tool_with_wine(&home, &bottle.id, &wine, tool).unwrap();
            assert!(Path::new(&launched.log_path).exists());
        }
        let invocations = wait_for_invocations(&prefix, 3);
        assert!(invocations.lines().any(|line| {
            line == format!(
                "{}|{}|joy.cpl",
                prefix.display(),
                prefix
                    .join("drive_c/windows/system32/control.exe")
                    .display()
            )
        }));
        assert!(invocations.lines().any(|line| {
            line == format!(
                "{}|{}|",
                prefix.display(),
                prefix
                    .join("drive_c/windows/system32/taskmgr.exe")
                    .display()
            )
        }));
        assert!(invocations.lines().any(|line| {
            line == format!(
                "{}|{}|inetcpl.cpl",
                prefix.display(),
                prefix
                    .join("drive_c/windows/system32/control.exe")
                    .display()
            )
        }));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn controller_and_internet_panels_reject_missing_or_symlinked_applets() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Applet guards").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        install_fake_tools(&prefix);
        fs::remove_file(prefix.join("drive_c/windows/system32/joy.cpl")).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::GameControllers).is_err());
        let internet = prefix.join("drive_c/windows/system32/inetcpl.cpl");
        fs::remove_file(&internet).unwrap();
        symlink("/bin/sleep", &internet).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::InternetSettings).is_err());
        assert!(!prefix.join("tool-invocations").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn command_prompt_uses_fixed_console_and_rejects_missing_or_symlinked_cmd() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Command prompt").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        install_fake_tools(&prefix);

        let cmd = prefix.join("drive_c/windows/system32/cmd.exe");
        fs::remove_file(&cmd).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::CommandPrompt).is_err());
        assert!(!prefix.join("tool-invocations").exists());

        symlink("/bin/sleep", &cmd).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::CommandPrompt).is_err());
        assert!(!prefix.join("tool-invocations").exists());

        fs::remove_file(&cmd).unwrap();
        fs::write(&cmd, b"MZfake cmd").unwrap();
        let launched =
            open_tool_with_wine(&home, &bottle.id, &wine, WineTool::CommandPrompt).unwrap();
        assert!(Path::new(&launched.log_path).exists());
        let invocations = wait_for_invocations(&prefix, 1);
        assert!(invocations.lines().any(|line| {
            line == format!(
                "{}|{}|{}",
                prefix.display(),
                prefix
                    .join("drive_c/windows/system32/wineconsole.exe")
                    .display(),
                r"C:\windows\system32\cmd.exe"
            )
        }));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn simulated_reboot_uses_only_the_selected_bottles_wineboot_and_waits() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Reboot tools").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");

        let result =
            simulate_reboot_with_wine(&home, &bottle.id, &wine, Duration::from_secs(2)).unwrap();
        assert!(result.message.contains("restarted"));
        assert!(Path::new(&result.log_path).exists());
        let invocations = fs::read_to_string(prefix.join("wineboot-invocations")).unwrap();
        assert!(
            invocations
                .lines()
                .any(|line| line == format!("{}|--end-session|--kill", prefix.display()))
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rejects_uninitialized_bottle_and_different_runtime() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Not initialized").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        install_fake_tools(&prefix);
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::Configuration).is_err());
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let other = home.join("other-runtime");
        fs::create_dir(&other).unwrap();
        for name in ["wine", "wineboot"] {
            fs::copy(wine.parent().unwrap().join(name), other.join(name)).unwrap();
        }
        assert!(
            open_tool_with_wine(
                &home,
                &bottle.id,
                &other.join("wine"),
                WineTool::RegistryEditor
            )
            .is_err()
        );
        assert!(!prefix.join("tool-invocations").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_symlinked_tool_or_prefix() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Symlink guard").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let prefix = dir.join("prefix");
        install_fake_tools(&prefix);
        let regedit = prefix.join("drive_c/windows/regedit.exe");
        fs::remove_file(&regedit).unwrap();
        symlink("/bin/sleep", &regedit).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::RegistryEditor).is_err());
        fs::rename(&prefix, dir.join("old-prefix")).unwrap();
        symlink(dir.join("old-prefix"), &prefix).unwrap();
        assert!(open_tool_with_wine(&home, &bottle.id, &wine, WineTool::Configuration).is_err());
        assert!(!dir.join("old-prefix/tool-invocations").exists());
        fs::remove_dir_all(home).unwrap();
    }
}
