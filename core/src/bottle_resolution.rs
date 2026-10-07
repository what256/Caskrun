//! Per-bottle Wine Retina rendering. Registry changes go through Wine's `reg`
//! program while the prefix is idle; Caskrun never edits user.reg in place.

use super::*;
use std::io::Write;

const REGISTRY_LIMIT: u64 = 32 * 1024 * 1024;
const SERVER_TIMEOUT: Duration = Duration::from_secs(5);
const REG_TIMEOUT: Duration = Duration::from_secs(15);
const MAC_DRIVER_KEY: &str = r"HKCU\Software\Wine\Mac Driver";
// Wine 11 winecfg reads and writes the user DPI here. Software\Wine\Fonts
// contains a separate generated font value and is not the display setting.
const DESKTOP_KEY: &str = r"HKCU\Control Panel\Desktop";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RegistrySnapshot {
    retina: Option<bool>,
    dpi: Option<u32>,
}

impl RegistrySnapshot {
    fn effective_dpi(self) -> u32 {
        self.dpi.unwrap_or(96)
    }

    fn is_supported(self) -> bool {
        matches!(
            (self.retina.unwrap_or(false), self.effective_dpi()),
            (false, 96) | (true, 192)
        )
    }

    fn result(self, id: &str) -> Result<ResolutionResult, String> {
        if !self.is_supported() {
            return Err("This bottle has custom or inconsistent Wine display settings. Review them in Wine Configuration before changing High Resolution Mode".into());
        }
        Ok(ResolutionResult {
            bottle_id: id.to_owned(),
            high_resolution: self.retina.unwrap_or(false),
            dpi: self.effective_dpi(),
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ResolutionResult {
    bottle_id: String,
    high_resolution: bool,
    dpi: u32,
}

#[derive(Clone, Copy)]
enum RegistryChange {
    Retina(Option<bool>),
    Dpi(Option<u32>),
}

impl RegistryChange {
    fn arguments(self) -> Result<Vec<&'static str>, String> {
        let (key, name, value_type, value) = match self {
            Self::Retina(Some(true)) => (MAC_DRIVER_KEY, "RetinaMode", "REG_SZ", Some("y")),
            Self::Retina(Some(false)) => (MAC_DRIVER_KEY, "RetinaMode", "REG_SZ", Some("n")),
            Self::Retina(None) => (MAC_DRIVER_KEY, "RetinaMode", "REG_SZ", None),
            Self::Dpi(Some(192)) => (DESKTOP_KEY, "LogPixels", "REG_DWORD", Some("192")),
            Self::Dpi(Some(96)) => (DESKTOP_KEY, "LogPixels", "REG_DWORD", Some("96")),
            Self::Dpi(None) => (DESKTOP_KEY, "LogPixels", "REG_DWORD", None),
            Self::Dpi(Some(_)) => return Err("Unsupported Wine DPI value".into()),
        };
        Ok(match value {
            Some(data) => vec![
                "reg", "add", key, "/v", name, "/t", value_type, "/d", data, "/f",
            ],
            None => vec!["reg", "delete", key, "/v", name, "/f"],
        })
    }
}

pub(super) fn bottle_resolution(home: &Path, id: &str) -> Result<ResolutionResult, String> {
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    let (_guard, _) = open_stable_directory(&prefix)?;
    require_initialized_registry(&prefix)?;
    read_registry_snapshot(&prefix)?.result(id)
}

pub(super) fn set_bottle_resolution(
    home: &Path,
    id: &str,
    high_resolution: bool,
) -> Result<ResolutionResult, String> {
    let wine = wine_path()
        .ok_or("Caskrun could not find its included Wine runtime. Reinstall Caskrun.")?;
    set_bottle_resolution_with_wine(home, id, high_resolution, &wine)
}

fn set_bottle_resolution_with_wine(
    home: &Path,
    id: &str,
    high_resolution: bool,
    wine: &Path,
) -> Result<ResolutionResult, String> {
    let _lock = lock_bottle(home, id)?;
    let (dir, prefix, wineserver, prefix_guard, prefix_snapshot) =
        bottle_control::checked_control_paths(home, id, wine)?;
    require_initialized_registry(&prefix)?;
    refuse_running_bottle(&dir)?;
    refuse_live_wine_server(&dir, SERVER_TIMEOUT, Some(wine))?;
    refuse_running_bottle(&dir)?;
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;

    let original = read_registry_snapshot(&prefix)?;
    original.result(id)?;
    if original.retina.unwrap_or(false) == high_resolution {
        return original.result(id);
    }

    let target = RegistrySnapshot {
        retina: Some(high_resolution),
        dpi: Some(if high_resolution { 192 } else { 96 }),
    };
    let logs = real_subdirectory(&dir, "logs")?;
    let log_path = logs.join(format!("resolution-{}.log", Uuid::new_v4()));
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| format!("Could not create resolution log: {e}"))?;
    writeln!(
        log,
        "Caskrun: changing High Resolution Mode for bottle {id}."
    )
    .map_err(|e| format!("Could not write resolution log: {e}"))?;

    let context = ChangeContext {
        dir: &dir,
        prefix: &prefix,
        wine,
        wineserver: &wineserver,
        prefix_guard: &prefix_guard,
        prefix_snapshot: &prefix_snapshot,
        log: &log,
    };
    let changed: Result<RegistrySnapshot, String> = (|| {
        run_registry_change(&context, RegistryChange::Retina(target.retina))?;
        run_registry_change(&context, RegistryChange::Dpi(target.dpi))?;
        let result = read_registry_snapshot(&prefix)?;
        if result != target {
            return Err("Wine did not save the requested display settings".into());
        }
        Ok(result)
    })();
    match changed {
        Ok(result) => result.result(id),
        Err(error) => {
            // Restore both original values, including their absence, if either
            // command failed or Wine saved only part of the requested change.
            let rollback: Result<(), String> = (|| {
                run_registry_change(&context, RegistryChange::Retina(original.retina))?;
                run_registry_change(&context, RegistryChange::Dpi(original.dpi))?;
                if read_registry_snapshot(&prefix)? != original {
                    return Err("Wine did not restore the prior display settings".into());
                }
                Ok(())
            })();
            match rollback {
                Ok(()) => Err(format!(
                    "{error}. The previous display settings were restored. Check {}",
                    log_path.display()
                )),
                Err(rollback_error) => Err(format!(
                    "{error}. Automatic rollback could not be verified: {rollback_error}. Check {} before opening this bottle",
                    log_path.display()
                )),
            }
        }
    }
}

struct ChangeContext<'a> {
    dir: &'a Path,
    prefix: &'a Path,
    wine: &'a Path,
    wineserver: &'a Path,
    prefix_guard: &'a File,
    prefix_snapshot: &'a fs::Metadata,
    log: &'a File,
}

fn run_registry_change(context: &ChangeContext<'_>, change: RegistryChange) -> Result<(), String> {
    bottle_control::assert_same_prefix(
        context.prefix,
        context.prefix_guard,
        context.prefix_snapshot,
    )?;
    refuse_running_bottle(context.dir)?;
    check_idle_server(context)?;
    let stdout = context.log.try_clone().map_err(|e| e.to_string())?;
    let stderr = context.log.try_clone().map_err(|e| e.to_string())?;
    let xdg = bottle_xdg::for_bottle(context.dir)?;
    let mut command = Command::new(context.wine);
    xdg.apply(&mut command);
    let mut child = command
        .args(change.arguments()?)
        .env("WINEPREFIX", context.prefix)
        .current_dir(context.prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("Could not start Wine's registry tool: {e}"))?;
    let (status, timed_out) = wait_child_bounded_status(&mut child, REG_TIMEOUT)?;
    // A registry command may exit before its wineserver persists user.reg.
    // Wait for the server to stop even when the command itself failed.
    check_idle_server(context)?;
    bottle_control::assert_same_prefix(
        context.prefix,
        context.prefix_guard,
        context.prefix_snapshot,
    )?;
    if timed_out || !status.success() {
        return Err(format!(
            "Wine's registry tool failed (exit {:?}, timeout {timed_out})",
            status.code()
        ));
    }
    Ok(())
}

fn check_idle_server(context: &ChangeContext<'_>) -> Result<(), String> {
    let (status, timed_out) = bottle_control::run_wineserver(
        context.wineserver,
        context.prefix,
        "-w",
        context.log,
        SERVER_TIMEOUT,
    )?;
    if timed_out || !status.success() {
        return Err(
            "Close Windows apps in this bottle before changing High Resolution Mode".into(),
        );
    }
    Ok(())
}

fn require_initialized_registry(prefix: &Path) -> Result<(), String> {
    if !path_is_real_file(&prefix.join("system.reg"))
        || !path_is_real_file(&prefix.join("user.reg"))
    {
        return Err("Initialize this bottle before changing High Resolution Mode".into());
    }
    Ok(())
}

fn read_registry_snapshot(prefix: &Path) -> Result<RegistrySnapshot, String> {
    let path = prefix.join("user.reg");
    let before = fs::symlink_metadata(&path)
        .map_err(|e| format!("Could not inspect bottle registry: {e}"))?;
    if !before.file_type().is_file() || before.len() > REGISTRY_LIMIT {
        return Err("Bottle user registry must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|e| format!("Could not safely open bottle registry: {e}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle registry changed while opening it".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(REGISTRY_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > REGISTRY_LIMIT
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Bottle registry changed while reading it".into());
    }
    let contents = String::from_utf8(bytes).map_err(|_| "Bottle registry is not valid UTF-8")?;
    parse_registry_snapshot(&contents)
}

fn parse_registry_snapshot(contents: &str) -> Result<RegistrySnapshot, String> {
    if !contents.starts_with("WINE REGISTRY Version 2") {
        return Err("Bottle user registry is not in Wine's supported format".into());
    }
    let mut in_mac_driver = false;
    let mut in_desktop = false;
    let mut retina = None;
    let mut dpi = None;
    let mut saw_retina = false;
    let mut saw_dpi = false;
    for line in contents.lines() {
        let line = line.trim();
        if let Some(key) = line.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            in_mac_driver = key.0.eq_ignore_ascii_case(r"Software\\Wine\\Mac Driver");
            in_desktop = key.0.eq_ignore_ascii_case(r"Control Panel\\Desktop");
            continue;
        }
        if in_mac_driver && line.to_ascii_lowercase().starts_with("\"retinamode\"=") {
            if saw_retina {
                return Err("Duplicate Wine RetinaMode registry value".into());
            }
            saw_retina = true;
            retina = match line.split_once('=').map(|(_, value)| value) {
                Some("\"y\"") | Some("\"Y\"") => Some(true),
                Some("\"n\"") | Some("\"N\"") => Some(false),
                _ => return Err("Unsupported Wine RetinaMode registry value".into()),
            };
        }
        if in_desktop && line.to_ascii_lowercase().starts_with("\"logpixels\"=") {
            if saw_dpi {
                return Err("Duplicate Wine LogPixels registry value".into());
            }
            saw_dpi = true;
            let value = line
                .split_once('=')
                .map(|(_, value)| value)
                .and_then(|value| value.strip_prefix("dword:"))
                .ok_or("Unsupported Wine LogPixels registry value")?;
            dpi = Some(
                u32::from_str_radix(value, 16)
                    .map_err(|_| "Invalid Wine LogPixels registry value")?,
            );
        }
    }
    Ok(RegistrySnapshot { retina, dpi })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-resolution-test-{}", Uuid::new_v4()))
    }

    fn fake_runtime(home: &Path) -> PathBuf {
        let runtime = home.join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        let wine = runtime.join("wine");
        fs::write(
            &wine,
            r#"#!/bin/sh
set -eu
test "$1" = reg
action=$2
key=$3
shift 3
name=''
data=''
while test "$#" -gt 0; do
  case "$1" in
    /v) name=$2; shift 2;;
    /d) data=$2; shift 2;;
    /t) shift 2;;
    /f) shift;;
    *) exit 70;;
  esac
done
printf '%s|%s|%s|%s\n' "$action" "$key" "$name" "$data" >> "$WINEPREFIX/reg-calls"
if test "$name" = LogPixels && test "$data" = 192 && test -f "$WINEPREFIX/fail-next-dpi"; then
  rm "$WINEPREFIX/fail-next-dpi"
  exit 72
fi
case "$name" in
  RetinaMode)
    if test "$action" = add; then printf '%s' "$data" > "$WINEPREFIX/mock-retina";
    else rm -f "$WINEPREFIX/mock-retina"; fi;;
  LogPixels)
    if test "$action" = add; then printf '%s' "$data" > "$WINEPREFIX/mock-dpi";
    else rm -f "$WINEPREFIX/mock-dpi"; fi;;
  *) exit 71;;
esac
{
  printf '%s\n' 'WINE REGISTRY Version 2' '[Control Panel\\Desktop]'
  if test -f "$WINEPREFIX/mock-dpi"; then
    case "$(cat "$WINEPREFIX/mock-dpi")" in
      96) printf '%s\n' '"LogPixels"=dword:00000060';;
      192) printf '%s\n' '"LogPixels"=dword:000000c0';;
      *) exit 73;;
    esac
  fi
  printf '%s\n' '[Software\\Wine\\Mac Driver]'
  if test -f "$WINEPREFIX/mock-retina"; then
    printf '"RetinaMode"="%s"\n' "$(cat "$WINEPREFIX/mock-retina")"
  fi
} > "$WINEPREFIX/user.reg"
"#,
        )
        .unwrap();
        let wineboot = runtime.join("wineboot");
        fs::write(
            &wineboot,
            b"#!/bin/sh\nset -eu\ntouch \"$WINEPREFIX/system.reg\"\nprintf '%s\\n' 'WINE REGISTRY Version 2' '[Control Panel\\\\Desktop]' > \"$WINEPREFIX/user.reg\"\n",
        )
        .unwrap();
        let wineserver = runtime.join("wineserver");
        fs::write(
            &wineserver,
            b"#!/bin/sh\ntest \"$1\" = -w && test ! -f \"$WINEPREFIX/busy\"\n",
        )
        .unwrap();
        for executable in [&wine, &wineboot, &wineserver] {
            let mut permissions = fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(executable, permissions).unwrap();
        }
        wine
    }

    #[test]
    fn parses_off_on_and_rejects_ambiguous_registry() {
        let off =
            "WINE REGISTRY Version 2\n[Control Panel\\\\Desktop]\n\"LogPixels\"=dword:00000060\n";
        assert_eq!(
            parse_registry_snapshot(off).unwrap(),
            RegistrySnapshot {
                retina: None,
                dpi: Some(96)
            }
        );
        let on = "WINE REGISTRY Version 2\n[Control Panel\\\\Desktop]\n\"LogPixels\"=dword:000000c0\n[Software\\\\Wine\\\\Mac Driver]\n\"RetinaMode\"=\"y\"\n";
        assert_eq!(
            parse_registry_snapshot(on).unwrap(),
            RegistrySnapshot {
                retina: Some(true),
                dpi: Some(192)
            }
        );
        assert!(parse_registry_snapshot(&format!("{on}\"RetinaMode\"=\"n\"\n")).is_err());
        assert!(parse_registry_snapshot("WINE REGISTRY Version 2\n[Control Panel\\\\Desktop]\n\"LogPixels\"=dword:00000090\n").unwrap().result("id").is_err());
    }

    #[test]
    fn toggles_only_selected_idle_bottle_and_preserves_noop() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let first = create(&home, "High resolution").unwrap();
        let second = create(&home, "Other").unwrap();
        ensure_prefix_with_wine(&home, &first.id, &wine).unwrap();
        ensure_prefix_with_wine(&home, &second.id, &wine).unwrap();
        assert!(!bottle_resolution(&home, &first.id).unwrap().high_resolution);

        let on = set_bottle_resolution_with_wine(&home, &first.id, true, &wine).unwrap();
        assert!(on.high_resolution);
        assert_eq!(on.dpi, 192);
        let first_prefix = bottle_dir(&home, &first.id).unwrap().join("prefix");
        let second_prefix = bottle_dir(&home, &second.id).unwrap().join("prefix");
        assert!(
            fs::read_to_string(first_prefix.join("user.reg"))
                .unwrap()
                .contains("\"RetinaMode\"=\"y\"")
        );
        assert!(
            !bottle_resolution(&home, &second.id)
                .unwrap()
                .high_resolution
        );
        assert!(!second_prefix.join("reg-calls").exists());
        let calls_before = fs::read_to_string(first_prefix.join("reg-calls")).unwrap();
        set_bottle_resolution_with_wine(&home, &first.id, true, &wine).unwrap();
        assert_eq!(
            fs::read_to_string(first_prefix.join("reg-calls")).unwrap(),
            calls_before
        );

        let off = set_bottle_resolution_with_wine(&home, &first.id, false, &wine).unwrap();
        assert!(!off.high_resolution);
        assert_eq!(off.dpi, 96);
        assert!(
            fs::read_to_string(first_prefix.join("user.reg"))
                .unwrap()
                .contains("\"RetinaMode\"=\"n\"")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_second_registry_command_restores_original_values() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Rollback").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        fs::write(prefix.join("fail-next-dpi"), b"").unwrap();
        let error = set_bottle_resolution_with_wine(&home, &bottle.id, true, &wine).unwrap_err();
        assert!(
            error.contains("previous display settings were restored"),
            "{error}"
        );
        assert_eq!(
            read_registry_snapshot(&prefix).unwrap(),
            RegistrySnapshot {
                retina: None,
                dpi: None
            }
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_active_server_other_runtime_and_symlinked_registry() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Guards").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");

        fs::write(prefix.join("busy"), b"").unwrap();
        assert!(set_bottle_resolution_with_wine(&home, &bottle.id, true, &wine).is_err());
        assert!(!prefix.join("reg-calls").exists());
        fs::remove_file(prefix.join("busy")).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(
            dir.join("active-processes.json"),
            format!("[{}]", std::process::id()),
        )
        .unwrap();
        assert!(set_bottle_resolution_with_wine(&home, &bottle.id, true, &wine).is_err());
        assert!(!prefix.join("reg-calls").exists());
        fs::write(dir.join("active-processes.json"), b"[]").unwrap();

        let other = home.join("other-runtime");
        fs::create_dir(&other).unwrap();
        for name in ["wine", "wineboot", "wineserver"] {
            fs::copy(wine.parent().unwrap().join(name), other.join(name)).unwrap();
        }
        assert!(
            set_bottle_resolution_with_wine(&home, &bottle.id, true, &other.join("wine")).is_err()
        );
        assert!(!prefix.join("reg-calls").exists());

        fs::rename(prefix.join("user.reg"), prefix.join("saved-user.reg")).unwrap();
        symlink(prefix.join("saved-user.reg"), prefix.join("user.reg")).unwrap();
        assert!(bottle_resolution(&home, &bottle.id).is_err());
        assert!(set_bottle_resolution_with_wine(&home, &bottle.id, true, &wine).is_err());
        assert!(!prefix.join("reg-calls").exists());
        fs::remove_dir_all(home).unwrap();
    }
}
