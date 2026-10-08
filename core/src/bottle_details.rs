//! Small bottle notes and Wine's actual global Windows-version override.
//! The override is HKCU\Software\Wine\Version, read by pinned Wine 11's
//! ntdll/version.c after any per-app AppDefaults override. It changes the
//! compatibility version reported to programs, without rewriting machine
//! registry metadata or replacing per-app settings.

use super::*;
use std::cell::Cell;
use std::io::Write;

const NOTES_FILE: &str = "notes.json";
const MAX_NOTES_BYTES: usize = 16 * 1024;
const MAX_NOTES_FILE_BYTES: u64 = 64 * 1024;
const MAX_REGISTRY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REGISTRY_LINE_BYTES: usize = 64 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const REG_TIMEOUT: Duration = Duration::from_secs(15);
const VERSION_KEY: &str = r"HKCU\Software\Wine";
const WINE_KEY: &str = "software\\wine";
const APP_DEFAULTS: &str = "software\\wine\\appdefaults\\";
const NT_VERSION_KEY: &str = "software\\microsoft\\windows nt\\currentversion";
const PRODUCT_KEY: &str = "system\\currentcontrolset\\control\\productoptions";
const O_NONBLOCK: i32 = 0x0004;

#[derive(Debug, Serialize)]
pub(super) struct NotesResult {
    bottle_id: String,
    notes: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Notes {
    version: u32,
    notes: String,
}

fn checked_notes(notes: &str) -> Result<(), String> {
    if notes.len() > MAX_NOTES_BYTES {
        return Err("Bottle notes must be no larger than 16 KiB".into());
    }
    if notes
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'))
    {
        return Err("Bottle notes cannot contain hidden control characters".into());
    }
    Ok(())
}

fn read_notes(dir: &Path) -> Result<(String, Option<Vec<u8>>), String> {
    let raw = read_file(&dir.join(NOTES_FILE), MAX_NOTES_FILE_BYTES)?;
    let Some(bytes) = raw else {
        return Ok((String::new(), None));
    };
    let notes: Notes = serde_json::from_slice(&bytes)
        .map_err(|_| "Bottle notes have an invalid format".to_string())?;
    if notes.version != 1 {
        return Err("Bottle notes use an unsupported version".into());
    }
    checked_notes(&notes.notes)?;
    Ok((notes.notes, Some(bytes)))
}

pub(super) fn bottle_notes(home: &Path, id: &str) -> Result<NotesResult, String> {
    let dir = bottle_dir(home, id)?;
    let (guard, snapshot) = open_stable_directory(&dir)?;
    let (notes, _) = read_notes(&dir)?;
    bottle_control::assert_same_prefix(&dir, &guard, &snapshot)?;
    Ok(NotesResult {
        bottle_id: id.into(),
        notes,
    })
}

pub(super) fn set_bottle_notes(home: &Path, id: &str, notes: &str) -> Result<NotesResult, String> {
    checked_notes(notes)?;
    let _lock = lock_bottle(home, id)?;
    let dir = bottle_dir(home, id)?;
    let (guard, snapshot) = open_stable_directory(&dir)?;
    let (original_notes, original) = read_notes(&dir)?;
    if original_notes == notes {
        return Ok(NotesResult {
            bottle_id: id.into(),
            notes: notes.into(),
        });
    }
    let bytes = serde_json::to_vec_pretty(&Notes {
        version: 1,
        notes: notes.into(),
    })
    .map_err(|error| error.to_string())?;
    cancellation::check()?;
    bottle_control::assert_same_prefix(&dir, &guard, &snapshot)?;
    if read_notes(&dir)?.1 != original {
        return Err("Bottle notes changed before saving; reopen them and try again".into());
    }
    write_notes(&dir, &bytes)?;
    bottle_control::assert_same_prefix(&dir, &guard, &snapshot)?;
    match read_notes(&dir) {
        Ok((saved, Some(saved_bytes))) if saved == notes && saved_bytes == bytes => {
            Ok(NotesResult {
                bottle_id: id.into(),
                notes: saved,
            })
        }
        _ => {
            Err("Bottle notes changed during verification; reopen them before editing again".into())
        }
    }
}

fn write_notes(dir: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = dir.join(format!("notes.tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temporary)
            .map_err(|e| format!("Could not create bottle notes: {e}"))?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, dir.join(NOTES_FILE)).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// Notes travel with the generic portable bottle copy. Validate them before
/// copying and before accepting imports, without creating a notes sidecar.
pub(super) fn validate_portable_settings(dir: &Path) -> Result<(), String> {
    read_notes(dir).map(|_| ())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum WindowsVersion {
    Default,
    Win11,
    Win10,
    Win81,
    Win8,
    Win7,
    Vista,
    Winxp,
}

impl WindowsVersion {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "default" => Ok(Self::Default),
            "win11" => Ok(Self::Win11),
            "win10" => Ok(Self::Win10),
            "win81" => Ok(Self::Win81),
            "win8" => Ok(Self::Win8),
            "win7" => Ok(Self::Win7),
            "vista" => Ok(Self::Vista),
            "winxp" => Ok(Self::Winxp),
            _ => Err("Choose Default, Windows 11, 10, 8.1, 8, 7, Vista, or XP".into()),
        }
    }
    fn registry_value(self) -> Option<&'static str> {
        Some(match self {
            Self::Default => return None,
            Self::Win11 => "win11",
            Self::Win10 => "win10",
            Self::Win81 => "win81",
            Self::Win8 => "win8",
            Self::Win7 => "win7",
            Self::Vista => "vista",
            Self::Winxp => "winxp",
        })
    }
    fn from_registry(value: &str) -> Result<Self, String> {
        // The loader compares these strings case-sensitively. An uppercase or
        // unsupported value is custom evidence, not a safe editable default.
        Self::parse(if value == "winvista" { "vista" } else { value })
            .and_then(|version| if version == Self::Default {
                Err("The registry cannot store a 'default' version override".into())
            } else { Ok(version) })
            .map_err(|_| "This bottle has a custom Windows-version override. Review it in Wine Configuration before changing it".into())
    }
}

#[derive(Debug, Serialize)]
pub(super) struct WindowsVersionResult {
    bottle_id: String,
    selected_version: WindowsVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_version: Option<WindowsVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RegistryValue {
    Text(String),
    Dword(u32),
}

type RegistryFields = BTreeMap<(String, String), RegistryValue>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct VersionSnapshot {
    original_override: Option<String>,
    machine: RegistryFields,
    per_app: RegistryFields,
}

impl VersionSnapshot {
    fn selected(&self) -> Result<WindowsVersion, String> {
        self.original_override
            .as_deref()
            .map(WindowsVersion::from_registry)
            .unwrap_or(Ok(WindowsVersion::Default))
    }
    fn result(&self, id: &str) -> Result<WindowsVersionResult, String> {
        let selected = self.selected()?;
        let effective = if selected == WindowsVersion::Default {
            machine_version(&self.machine)
        } else {
            Some(selected)
        };
        let mut warnings = Vec::new();
        if effective.is_none() {
            warnings.push("The machine registry has a custom or unrecognized default version; it has been preserved.");
        }
        if !self.per_app.is_empty() {
            warnings.push("Apps with their own Wine version override keep that override.");
        }
        Ok(WindowsVersionResult {
            bottle_id: id.into(),
            selected_version: selected,
            effective_version: effective,
            warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
        })
    }
}

pub(super) fn bottle_windows_version(
    home: &Path,
    id: &str,
) -> Result<WindowsVersionResult, String> {
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    let (guard, snapshot) = open_stable_directory(&prefix)?;
    let result = read_version(&prefix)?.result(id)?;
    bottle_control::assert_same_prefix(&prefix, &guard, &snapshot)?;
    Ok(result)
}

pub(super) fn set_bottle_windows_version(
    home: &Path,
    id: &str,
    version: WindowsVersion,
) -> Result<WindowsVersionResult, String> {
    let wine = wine_path().ok_or("The Windows engine is unavailable")?;
    set_windows_version_with_wine(home, id, version, &wine)
}

fn set_windows_version_with_wine(
    home: &Path,
    id: &str,
    version: WindowsVersion,
    wine: &Path,
) -> Result<WindowsVersionResult, String> {
    set_version_with_verifier(home, id, version, wine, require_version_capability)
}

fn require_version_capability(wine: &Path) -> Result<(), String> {
    // Registry readback alone cannot establish that an arbitrary older/custom
    // engine implements these version tokens. Require the complete package
    // whose ntdll behavior was inspected and verified by the native API probe.
    // This runs before migration, idle-server checks, or any Wine process.
    let attestation = runtime_identity::bundled_runtime_attestation(wine)?;
    if attestation.as_ref().map(|value| value.identity.as_str())
        != Some(runtime_migration::SWITCHABLE_WINE_11_IDENTITY)
    {
        return Err("Windows-version changes require Caskrun's verified included Wine 11 engine. This engine's version support has not been verified".into());
    }
    Ok(())
}

fn set_version_with_verifier(
    home: &Path,
    id: &str,
    version: WindowsVersion,
    wine: &Path,
    verify_capability: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<WindowsVersionResult, String> {
    verify_capability(wine)?;
    let _lock = lock_bottle(home, id)?;
    let migration = runtime_migration::maybe_migrate_bundled_runtime(home, id, wine)?;
    let started = Cell::new(false);
    let result = set_version_locked(home, id, version, wine, &started);
    match result {
        Err(error) if !started.get() => match migration {
            Some(backup) => match backup.restore() {
                Ok(()) => Err(error),
                Err(restore) => Err(format!(
                    "{error}. The previous runtime record could not be restored: {restore}"
                )),
            },
            None => Err(error),
        },
        result => result,
    }
}

struct VersionContext<'a> {
    dir: &'a Path,
    prefix: &'a Path,
    wine: &'a Path,
    wineserver: &'a Path,
    prefix_guard: &'a File,
    prefix_snapshot: &'a fs::Metadata,
    log: &'a File,
    started: &'a Cell<bool>,
}

fn set_version_locked(
    home: &Path,
    id: &str,
    version: WindowsVersion,
    wine: &Path,
    started: &Cell<bool>,
) -> Result<WindowsVersionResult, String> {
    let (dir, prefix, wineserver, guard, snapshot) =
        bottle_control::checked_control_paths(home, id, wine)?;
    refuse_running_bottle(&dir)?;
    refuse_live_wine_server(&dir, IDLE_TIMEOUT, Some(wine))?;
    prefix_processes::wait_until_idle(&dir, IDLE_TIMEOUT)?;
    refuse_running_bottle(&dir)?;
    bottle_control::assert_same_prefix(&prefix, &guard, &snapshot)?;
    let original = read_version(&prefix)?;
    if original.selected()? == version {
        return original.result(id);
    }
    let logs = real_subdirectory(&dir, "logs")?;
    // Existing log-list rules recognize this fixed launch prefix; no app names
    // or arguments are used to choose a filesystem path.
    let log_path = logs.join(format!("launch-{}-{}.log", now_seconds()?, Uuid::new_v4()));
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|e| format!("Could not create Windows-version log: {e}"))?;
    writeln!(
        log,
        "Caskrun: changing this bottle's global Windows compatibility version."
    )
    .map_err(|e| e.to_string())?;
    let context = VersionContext {
        dir: &dir,
        prefix: &prefix,
        wine,
        wineserver: &wineserver,
        prefix_guard: &guard,
        prefix_snapshot: &snapshot,
        log: &log,
        started,
    };
    let changed = (|| {
        if read_version(&prefix)? != original {
            return Err("Windows-version settings changed before saving".into());
        }
        run_version_change(&context, version.registry_value())?;
        let saved = read_version(&prefix)?;
        if saved.original_override.as_deref() != version.registry_value()
            || saved.machine != original.machine
            || saved.per_app != original.per_app
        {
            return Err("Wine did not save the requested Windows version without changing other version settings".into());
        }
        saved.result(id)
    })();
    match changed {
        Ok(result) => Ok(result),
        Err(error) if !started.get() => Err(error),
        Err(error) => {
            let rollback = (|| {
                // A failing registry command may have left the original value
                // untouched. Deleting an already-absent value would itself
                // fail and must not create a false rollback alarm.
                if read_version(&prefix)? == original {
                    return Ok(());
                }
                run_version_change(&context, original.original_override.as_deref())?;
                if read_version(&prefix)? != original {
                    return Err(
                        "Wine did not restore the prior Windows-version settings".to_string()
                    );
                }
                Ok(())
            })();
            match rollback {
                Ok(()) => Err(format!(
                    "{error}. The previous Windows-version settings were restored. Check {}",
                    log_path.display()
                )),
                Err(rollback) => Err(format!(
                    "{error}. Automatic rollback could not be verified: {rollback}. Check {} before opening this bottle",
                    log_path.display()
                )),
            }
        }
    }
}

fn run_version_change(context: &VersionContext<'_>, version: Option<&str>) -> Result<(), String> {
    bottle_control::assert_same_prefix(
        context.prefix,
        context.prefix_guard,
        context.prefix_snapshot,
    )?;
    refuse_running_bottle(context.dir)?;
    version_server_idle(context)?;
    cancellation::check()?;
    let mut command = Command::new(context.wine);
    bottle_xdg::for_bottle(context.dir)?.apply(&mut command);
    bottle_graphics::configure_launch(&mut command, context.dir, context.wine)?;
    match version {
        Some(value) => command.args([
            "reg",
            "add",
            VERSION_KEY,
            "/v",
            "Version",
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ]),
        None => command.args(["reg", "delete", VERSION_KEY, "/v", "Version", "/f"]),
    };
    let mut child = command
        .env("WINEPREFIX", context.prefix)
        .current_dir(context.prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            context.log.try_clone().map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            context.log.try_clone().map_err(|e| e.to_string())?,
        ))
        .spawn()
        .map_err(|e| format!("Could not start Wine's registry tool: {e}"))?;
    context.started.set(true);
    let (status, timed_out) = wait_child_bounded_status(&mut child, REG_TIMEOUT)?;
    version_server_idle(context)?;
    bottle_control::assert_same_prefix(
        context.prefix,
        context.prefix_guard,
        context.prefix_snapshot,
    )?;
    if timed_out || !status.success() {
        return Err(format!(
            "Wine's version change failed (exit {:?}, timeout {timed_out})",
            status.code()
        ));
    }
    Ok(())
}

fn version_server_idle(context: &VersionContext<'_>) -> Result<(), String> {
    let (status, timed_out) = bottle_control::run_wineserver(
        context.wineserver,
        context.prefix,
        "-w",
        context.log,
        IDLE_TIMEOUT,
    )?;
    if timed_out || !status.success() {
        return Err("Close Windows apps in this bottle before changing its Windows version".into());
    }
    Ok(())
}

fn read_version(prefix: &Path) -> Result<VersionSnapshot, String> {
    let user = read_registry(prefix, "user.reg")?;
    let system = read_registry(prefix, "system.reg")?;
    let user = registry_fields(&user, |key, field| {
        (key == WINE_KEY || key.starts_with(APP_DEFAULTS)) && field == "version"
    })?;
    let machine = registry_fields(&system, |key, field| {
        (key == NT_VERSION_KEY
            && matches!(
                field,
                "currentmajorversionnumber"
                    | "currentminorversionnumber"
                    | "currentversion"
                    | "currentbuildnumber"
            ))
            || (key == PRODUCT_KEY && field == "producttype")
    })?;
    let mut per_app = RegistryFields::new();
    let mut original_override = None;
    for ((key, field), value) in user {
        if key == WINE_KEY {
            let RegistryValue::Text(value) = value else {
                return Err("The global Windows-version override is not a string; review Wine Configuration".into());
            };
            original_override = Some(value);
        } else {
            per_app.insert((key, field), value);
        }
    }
    Ok(VersionSnapshot {
        original_override,
        machine,
        per_app,
    })
}

fn machine_version(fields: &RegistryFields) -> Option<WindowsVersion> {
    let lookup = |key: &str, field: &str| fields.get(&(key.into(), field.into()));
    let product = lookup(PRODUCT_KEY, "producttype")?;
    if !matches!(product, RegistryValue::Text(value) if value.eq_ignore_ascii_case("WinNT")) {
        return None;
    }
    let numbers = match (
        lookup(NT_VERSION_KEY, "currentmajorversionnumber"),
        lookup(NT_VERSION_KEY, "currentminorversionnumber"),
    ) {
        (Some(RegistryValue::Dword(major)), Some(RegistryValue::Dword(minor))) if *major != 0 => {
            (*major, *minor)
        }
        _ => {
            let RegistryValue::Text(version) = lookup(NT_VERSION_KEY, "currentversion")? else {
                return None;
            };
            let (major, minor) = version.split_once('.')?;
            (major.parse().ok()?, minor.parse().ok()?)
        }
    };
    let RegistryValue::Text(build) = lookup(NT_VERSION_KEY, "currentbuildnumber")? else {
        return None;
    };
    Some(match (numbers, build.parse::<u32>().ok()?) {
        ((10, 0), 22000) => WindowsVersion::Win11,
        ((10, 0), 19045) => WindowsVersion::Win10,
        ((6, 3), 9600) => WindowsVersion::Win81,
        ((6, 2), 9200) => WindowsVersion::Win8,
        ((6, 1), 7601) => WindowsVersion::Win7,
        ((6, 0), 6002) => WindowsVersion::Vista,
        ((5, 1), 2600) => WindowsVersion::Winxp,
        _ => return None,
    })
}

fn read_registry(prefix: &Path, name: &str) -> Result<String, String> {
    let bytes = read_file(&prefix.join(name), MAX_REGISTRY_BYTES)?
        .ok_or("Initialize this bottle before changing its Windows version")?;
    String::from_utf8(bytes).map_err(|_| "Bottle registry is not valid UTF-8".into())
}

fn read_file(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect bottle settings: {error}")),
    };
    if !before.file_type().is_file() || before.len() > limit {
        return Err("Bottle settings must be bounded regular files".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | O_NONBLOCK)
        .open(path)
        .map_err(|e| format!("Could not safely open bottle settings: {e}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle settings changed while opening them".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !fs::symlink_metadata(path).is_ok_and(|now| same_snapshot(&before, &now))
    {
        return Err("Bottle settings changed while reading them".into());
    }
    Ok(Some(bytes))
}

fn registry_fields(
    contents: &str,
    wanted: impl Fn(&str, &str) -> bool,
) -> Result<RegistryFields, String> {
    let mut lines = contents.lines();
    if lines.next() != Some("WINE REGISTRY Version 2") || !contents.ends_with('\n') {
        return Err("Bottle registry has an unsupported or incomplete format".into());
    }
    let mut key = String::new();
    let mut fields = RegistryFields::new();
    let mut continuing = false;
    for raw in lines {
        let line = raw.trim();
        if line.len() > MAX_REGISTRY_LINE_BYTES {
            return Err("Bottle registry exceeds the Windows-version inspection limit".into());
        }
        if continuing {
            if line.starts_with(['[', '"', '@', '#']) {
                return Err("Bottle registry has an incomplete continued value".into());
            }
            continuing = line.ends_with('\\');
            continue;
        }
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            let (name, _) = decode_registry_string(section, ']')?;
            key = name.to_lowercase();
            continue;
        }
        let Some(value) = line.strip_prefix('"') else {
            continuing = line.ends_with('\\');
            continue;
        };
        let (name, consumed) = decode_registry_string(value, '"')?;
        let name = name.to_lowercase();
        let value = value[consumed..]
            .trim_start()
            .strip_prefix('=')
            .ok_or("Bottle registry has an invalid value assignment")?
            .trim_start();
        if !wanted(&key, &name) {
            continuing = value.ends_with('\\');
            continue;
        }
        let value = if let Some(quoted) = value
            .strip_prefix('"')
            .or_else(|| value.strip_prefix("str:\""))
            .or_else(|| value.strip_prefix("str(2):\""))
        {
            let (text, consumed) = decode_registry_string(quoted, '"')?;
            if !quoted[consumed..].trim().is_empty() || text.contains('\0') {
                return Err("Bottle registry has an invalid Windows-version string".into());
            }
            RegistryValue::Text(text)
        } else if let Some(number) = value.strip_prefix("dword:") {
            if number.is_empty()
                || number.len() > 8
                || !number.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("Bottle registry has an invalid Windows-version number".into());
            }
            RegistryValue::Dword(
                u32::from_str_radix(number, 16).map_err(|_| "Invalid Windows-version number")?,
            )
        } else {
            return Err(
                "Bottle registry has custom Windows-version value data; review Wine Configuration"
                    .into(),
            );
        };
        if fields.insert((key.clone(), name), value).is_some() {
            return Err("Bottle registry has ambiguous duplicate Windows-version values".into());
        }
    }
    if continuing {
        return Err("Bottle registry has an incomplete continued value".into());
    }
    Ok(fields)
}

fn decode_registry_string(input: &str, delimiter: char) -> Result<(String, usize), String> {
    let malformed = || "Bottle registry contains a malformed escaped string".to_string();
    let mut units = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch == delimiter {
            return String::from_utf16(&units)
                .map(|value| (value, offset + ch.len_utf8()))
                .map_err(|_| malformed());
        }
        if ch != '\\' {
            units.extend(ch.encode_utf16(&mut [0; 2]).iter().copied());
            continue;
        }
        let (_, escaped) = chars.next().ok_or_else(malformed)?;
        let simple = match escaped {
            'a' => Some(7),
            'b' => Some(8),
            'e' => Some(27),
            'f' => Some(12),
            'n' => Some(10),
            'r' => Some(13),
            't' => Some(9),
            'v' => Some(11),
            _ => None,
        };
        if let Some(value) = simple {
            units.push(value);
        } else if escaped == 'x' || matches!(escaped, '0'..='7') {
            let hex = escaped == 'x';
            let mut value = if hex { 0 } else { escaped as u16 - b'0' as u16 };
            let mut count = 0;
            while count < if hex { 4 } else { 2 } {
                let Some((_, next)) = chars.peek() else {
                    break;
                };
                let Some(digit) = next.to_digit(if hex { 16 } else { 8 }) else {
                    break;
                };
                value = value * if hex { 16 } else { 8 } + digit as u16;
                chars.next();
                count += 1;
            }
            units.push(if hex && count == 0 {
                b'x' as u16
            } else {
                value
            });
        } else {
            units.extend(escaped.encode_utf16(&mut [0; 2]).iter().copied());
        }
    }
    Err(malformed())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_fixture_version(
        home: &Path,
        id: &str,
        version: WindowsVersion,
        wine: &Path,
    ) -> Result<WindowsVersionResult, String> {
        set_version_with_verifier(home, id, version, wine, |_| Ok(()))
    }

    #[test]
    fn unverified_custom_engine_refuses_before_migration_or_any_wine_process() {
        let fixture = Fixture::new();
        fixture.initialize(None);
        let wine = fixture.runtime();
        let record = fixture.dir().join("prefix-record.json");
        let before_record = fs::read(&record).unwrap();
        let before_user = fs::read(fixture.prefix().join("user.reg")).unwrap();
        let before_machine = fs::read(fixture.prefix().join("system.reg")).unwrap();
        // Every engine entry point leaves evidence if it is started. The
        // unsupported gate must run before even wineserver's idle check.
        for name in ["wine", "wineboot", "wineserver"] {
            let path = wine.parent().unwrap().join(name);
            fs::write(
                &path,
                "#!/bin/sh\nprintf called >> \"$WINEPREFIX/engine-starts\"\nexit 0\n",
            )
            .unwrap();
        }
        let error =
            set_windows_version_with_wine(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine)
                .unwrap_err();
        assert!(error.contains("verified included Wine 11"), "{error}");
        assert!(!fixture.prefix().join("engine-starts").exists());
        assert_eq!(fs::read(&record).unwrap(), before_record);
        assert_eq!(
            fs::read(fixture.prefix().join("user.reg")).unwrap(),
            before_user
        );
        assert_eq!(
            fs::read(fixture.prefix().join("system.reg")).unwrap(),
            before_machine
        );
        assert_eq!(
            bottle_windows_version(&fixture.home, &fixture.id)
                .unwrap()
                .selected_version,
            WindowsVersion::Default
        );
        assert!(!fs::read_dir(fixture.dir()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("backup")
        }));
    }

    struct Fixture {
        home: PathBuf,
        id: String,
    }
    impl Fixture {
        fn new() -> Self {
            let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("bottle-details-{}", Uuid::new_v4()));
            let bottle = create(&home, "Disposable details fixture").unwrap();
            Self {
                home,
                id: bottle.id,
            }
        }
        fn dir(&self) -> PathBuf {
            self.home.join("bottles").join(&self.id)
        }
        fn prefix(&self) -> PathBuf {
            self.dir().join("prefix")
        }
        fn initialize(&self, version: Option<&str>) {
            fs::write(self.prefix().join("system.reg"), machine_registry()).unwrap();
            fs::write(self.prefix().join("user.reg"), user_registry(version)).unwrap();
        }
        fn runtime(&self) -> PathBuf {
            let root = self.home.join("mock-engine");
            fs::create_dir(&root).unwrap();
            let script = root.join("registry.py");
            fs::write(
                &script,
                r#"import os,sys
from pathlib import Path
prefix=Path(os.environ['WINEPREFIX'])
args=sys.argv[1:]
assert args[0]=='reg' and args[2]==r'HKCU\Software\Wine'
action=args[1]
data=args[args.index('/d')+1] if action=='add' else None
with (prefix/'registry-calls').open('a') as file: file.write(action+'|'+str(data)+'\n')
failure=prefix/'fail-before-write'
if failure.exists(): failure.unlink(); sys.exit(72)
if (prefix/'force-wrong-version').exists(): data='win8'
lines=(prefix/'user.reg').read_text().splitlines()
result=[]
inside=False
inserted=False
for line in lines:
    if line.startswith('['):
        if inside and data is not None:
            result.append('"Version"="'+data+'"'); inserted=True
        inside=line.startswith(r'[Software\\Wine]')
    if inside and line.startswith('"Version"='): continue
    result.append(line)
if inside and data is not None: result.append('"Version"="'+data+'"'); inserted=True
if data is not None and not inserted: result += [r'[Software\\Wine]', '"Version"="'+data+'"']
(prefix/'user.reg').write_text('\n'.join(result)+'\n')
failure=prefix/'fail-next-version'
if failure.exists(): failure.unlink(); sys.exit(72)
"#,
            )
            .unwrap();
            for (name, contents) in [
                (
                    "wine",
                    format!(
                        "#!/bin/sh\nexec /usr/bin/python3 '{}' \"$@\"\n",
                        script.display()
                    ),
                ),
                ("wineboot", "#!/bin/sh\nexit 0\n".into()),
                (
                    "wineserver",
                    "#!/bin/sh\ntest \"$1\" = -w && test ! -f \"$WINEPREFIX/busy\"\n".into(),
                ),
            ] {
                let path = root.join(name);
                fs::write(&path, contents).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let wine = root.join("wine");
            save_json(
                &self.dir().join("prefix-record.json"),
                &PrefixRecord {
                    wine_path: wine.display().to_string(),
                    runtime_identity: None,
                    runtime_fingerprint: None,
                },
            )
            .unwrap();
            wine
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.home).unwrap();
        }
    }

    fn machine_registry() -> &'static str {
        "WINE REGISTRY Version 2\n#arch=win64\n[Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion]\n\"CurrentMajorVersionNumber\"=dword:0000000a\n\"CurrentMinorVersionNumber\"=dword:00000000\n\"CurrentVersion\"=\"6.3\"\n\"CurrentBuildNumber\"=\"19045\"\n[System\\\\CurrentControlSet\\\\Control\\\\ProductOptions]\n\"ProductType\"=\"WinNT\"\n"
    }
    fn user_registry(version: Option<&str>) -> String {
        let mut text = "WINE REGISTRY Version 2\n#arch=win64\n[Software\\\\Wine]\n\"Unrelated\"=\"Preserve me\"\n".to_string();
        if let Some(version) = version {
            text += &format!("\"Version\"=\"{version}\"\n");
        }
        text += "[Software\\\\Wine\\\\AppDefaults\\\\example.exe]\n\"Version\"=\"win7\"\n[Software\\\\Wine\\\\Mac Driver]\n\"RetinaMode\"=\"n\"\n[Control Panel\\\\Desktop]\n\"LogPixels\"=dword:00000060\n";
        text
    }

    #[test]
    fn notes_are_optional_bounded_atomic_private_and_do_not_initialize_wine() {
        let fixture = Fixture::new();
        assert_eq!(bottle_notes(&fixture.home, &fixture.id).unwrap().notes, "");
        assert!(!fixture.dir().join(NOTES_FILE).exists());
        let text = "A useful note.\nKeeps accents: café 🐾\n\tTabbed line\r\n";
        assert_eq!(
            set_bottle_notes(&fixture.home, &fixture.id, text)
                .unwrap()
                .notes,
            text
        );
        let path = fixture.dir().join(NOTES_FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            bottle_notes(&fixture.home, &fixture.id).unwrap().notes,
            text
        );
        assert!(validate_portable_settings(&fixture.dir()).is_ok());
        assert_eq!(fs::read_dir(fixture.prefix()).unwrap().count(), 0);
        assert!(
            set_bottle_notes(&fixture.home, &fixture.id, &"x".repeat(MAX_NOTES_BYTES + 1)).is_err()
        );
        assert!(set_bottle_notes(&fixture.home, &fixture.id, "hidden\0data").is_err());
        assert_eq!(
            bottle_notes(&fixture.home, &fixture.id).unwrap().notes,
            text
        );
        assert!(!fs::read_dir(fixture.dir()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("notes.tmp-")
        }));
    }

    #[test]
    fn notes_refuse_symlink_future_version_corruption_and_unknown_fields_without_overwrite() {
        let fixture = Fixture::new();
        let path = fixture.dir().join(NOTES_FILE);
        for bytes in [
            b"{broken".as_slice(),
            b"{\"version\":2,\"notes\":\"Future\"}".as_slice(),
            b"{\"version\":1,\"notes\":\"Saved\",\"unknown\":true}".as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(validate_portable_settings(&fixture.dir()).is_err());
            assert!(set_bottle_notes(&fixture.home, &fixture.id, "Replacement").is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_file(&path).unwrap();
        let outside = fixture.home.join("outside-notes");
        fs::write(&outside, b"private data").unwrap();
        symlink(&outside, &path).unwrap();
        assert!(bottle_notes(&fixture.home, &fixture.id).is_err());
        assert!(set_bottle_notes(&fixture.home, &fixture.id, "Replacement").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"private data");
    }

    #[test]
    fn version_getter_reads_real_keys_default_machine_values_and_per_app_priority_warning() {
        let fixture = Fixture::new();
        assert!(bottle_windows_version(&fixture.home, &fixture.id).is_err());
        fixture.initialize(None);
        let result = bottle_windows_version(&fixture.home, &fixture.id).unwrap();
        assert_eq!(result.selected_version, WindowsVersion::Default);
        assert_eq!(result.effective_version, Some(WindowsVersion::Win10));
        assert!(result.warning.unwrap().contains("own Wine version"));
        fixture.initialize(Some("win11"));
        let result = bottle_windows_version(&fixture.home, &fixture.id).unwrap();
        assert_eq!(result.selected_version, WindowsVersion::Win11);
        assert_eq!(result.effective_version, Some(WindowsVersion::Win11));
        assert!(WindowsVersion::parse("winxp & command").is_err());
        for (token, version) in [
            ("win11", WindowsVersion::Win11),
            ("win10", WindowsVersion::Win10),
            ("win81", WindowsVersion::Win81),
            ("win8", WindowsVersion::Win8),
            ("win7", WindowsVersion::Win7),
            ("vista", WindowsVersion::Vista),
            ("winxp", WindowsVersion::Winxp),
        ] {
            assert_eq!(WindowsVersion::parse(token).unwrap(), version);
            assert_eq!(version.registry_value(), Some(token));
            assert_eq!(WindowsVersion::from_registry(token).unwrap(), version);
            assert_eq!(serde_json::to_value(version).unwrap(), token);
        }
        assert_eq!(WindowsVersion::Default.registry_value(), None);
        assert_eq!(
            WindowsVersion::from_registry("winvista").unwrap(),
            WindowsVersion::Vista
        );
    }

    #[test]
    fn custom_global_values_and_ambiguous_registry_fields_refuse_unknown_machine_values_are_truthful()
     {
        let fixture = Fixture::new();
        for value in ["WIN10", "unsupported", "default", "win11\\0hidden"] {
            fixture.initialize(Some(value));
            assert!(
                bottle_windows_version(&fixture.home, &fixture.id).is_err(),
                "{value}"
            );
        }
        fixture.initialize(None);
        fs::write(
            fixture.prefix().join("system.reg"),
            machine_registry().replace("19045", "12345"),
        )
        .unwrap();
        let result = bottle_windows_version(&fixture.home, &fixture.id).unwrap();
        assert_eq!(result.effective_version, None);
        assert!(result.warning.unwrap().contains("custom or unrecognized"));
        let duplicate = user_registry(Some("win10")).replace(
            "\"Version\"=\"win10\"",
            "\"Version\"=\"win10\"\n\"version\"=\"win11\"",
        );
        fs::write(fixture.prefix().join("user.reg"), duplicate).unwrap();
        assert!(bottle_windows_version(&fixture.home, &fixture.id).is_err());
    }

    #[test]
    fn safe_registry_reads_refuse_symlinks_and_handle_actual_wine_escapes() {
        let fixture = Fixture::new();
        fixture.initialize(Some("win10"));
        let user = fixture.prefix().join("user.reg");
        let contents = fs::read_to_string(&user)
            .unwrap()
            .replace("Software", "\\x0053oftware")
            .replace("\"Version\"", "\"\\126ersion\"");
        fs::write(&user, contents).unwrap();
        assert_eq!(
            bottle_windows_version(&fixture.home, &fixture.id)
                .unwrap()
                .selected_version,
            WindowsVersion::Win10
        );
        fs::remove_file(&user).unwrap();
        symlink(fixture.home.join("missing-external"), &user).unwrap();
        assert!(bottle_windows_version(&fixture.home, &fixture.id).is_err());
    }

    #[test]
    fn setter_changes_only_global_override_and_restores_absence_without_touching_display_or_app_defaults()
     {
        let fixture = Fixture::new();
        fixture.initialize(None);
        let wine = fixture.runtime();
        let machine = fs::read(fixture.prefix().join("system.reg")).unwrap();
        let original = read_version(&fixture.prefix()).unwrap();
        let result =
            set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine).unwrap();
        assert_eq!(result.selected_version, WindowsVersion::Win11);
        assert_eq!(
            read_version(&fixture.prefix()).unwrap().per_app,
            original.per_app
        );
        assert_eq!(
            fs::read(fixture.prefix().join("system.reg")).unwrap(),
            machine
        );
        let user = fs::read_to_string(fixture.prefix().join("user.reg")).unwrap();
        assert!(user.contains("\"RetinaMode\"=\"n\""));
        assert!(user.contains("\"LogPixels\"=dword:00000060"));
        assert!(user.contains("\"Unrelated\"=\"Preserve me\""));
        set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Default, &wine).unwrap();
        assert_eq!(read_version(&fixture.prefix()).unwrap(), original);
    }

    #[test]
    fn busy_unknown_and_mismatched_runtime_refusals_never_start_registry_client() {
        let fixture = Fixture::new();
        fixture.initialize(None);
        let wine = fixture.runtime();
        fs::write(fixture.prefix().join("busy"), b"").unwrap();
        assert!(
            set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine).is_err()
        );
        assert!(!fixture.prefix().join("registry-calls").exists());
        fs::remove_file(fixture.prefix().join("busy")).unwrap();
        fixture.initialize(Some("custom-version"));
        assert!(
            set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine).is_err()
        );
        assert!(!fixture.prefix().join("registry-calls").exists());
        fixture.initialize(None);
        let record = fixture.dir().join("prefix-record.json");
        fs::write(&record, b"{\"wine_path\":\"/unavailable/old/wine\"}").unwrap();
        let before = fs::read(&record).unwrap();
        assert!(
            set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine).is_err()
        );
        assert_eq!(fs::read(&record).unwrap(), before);
        assert!(!fixture.prefix().join("registry-calls").exists());
    }

    #[test]
    fn failed_wine_change_rolls_back_original_alias_or_absence_and_verifies_it() {
        for original in [None, Some("winvista")] {
            let fixture = Fixture::new();
            fixture.initialize(original);
            let wine = fixture.runtime();
            let before = read_version(&fixture.prefix()).unwrap();
            fs::write(fixture.prefix().join("fail-next-version"), b"").unwrap();
            let error =
                set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine)
                    .unwrap_err();
            assert!(
                error.contains("previous Windows-version settings were restored"),
                "{error}"
            );
            assert_eq!(read_version(&fixture.prefix()).unwrap(), before);
            assert_eq!(
                fs::read_to_string(fixture.prefix().join("registry-calls"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
        }
    }

    #[test]
    fn unchanged_failure_needs_no_registry_rollback_but_failed_restoration_is_explicit() {
        let fixture = Fixture::new();
        fixture.initialize(None);
        let wine = fixture.runtime();
        let original = read_version(&fixture.prefix()).unwrap();
        fs::write(fixture.prefix().join("fail-before-write"), b"").unwrap();
        let error = set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine)
            .unwrap_err();
        assert!(error.contains("previous Windows-version settings were restored"));
        assert_eq!(read_version(&fixture.prefix()).unwrap(), original);
        assert_eq!(
            fs::read_to_string(fixture.prefix().join("registry-calls"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        fs::write(fixture.prefix().join("force-wrong-version"), b"").unwrap();
        let error = set_fixture_version(&fixture.home, &fixture.id, WindowsVersion::Win11, &wine)
            .unwrap_err();
        assert!(
            error.contains("Automatic rollback could not be verified"),
            "{error}"
        );
        assert_eq!(
            read_version(&fixture.prefix()).unwrap().selected().unwrap(),
            WindowsVersion::Win8
        );
        assert_eq!(
            read_version(&fixture.prefix()).unwrap().per_app,
            original.per_app
        );
    }
}
