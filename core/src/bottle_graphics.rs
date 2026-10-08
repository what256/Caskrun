//! Bottle-scoped renderer selection. Switching changes one small Caskrun
//! preference while idle; it never replaces an application's DLLs or registry.

use super::*;

const SETTINGS: &str = "graphics.json";
const SETTINGS_LIMIT: u64 = 4096;
const IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const DXMT_ROOT: &str = "components/dxmt/runtime";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Backend {
    #[default]
    Auto,
    Wined3d,
    Dxmt,
}

impl Backend {
    fn effective(self) -> Self {
        if self == Self::Auto {
            Self::Wined3d
        } else {
            self
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Wined3d => "wined3d",
            Self::Dxmt => "dxmt",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "wined3d" => Ok(Self::Wined3d),
            "dxmt" => Ok(Self::Dxmt),
            _ => Err("Choose automatic, WineD3D, or DXMT graphics".into()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    backend: Backend,
}

#[derive(Debug, Serialize)]
pub(super) struct GraphicsResult {
    bottle_id: String,
    selected_backend: Backend,
    effective_backend: Backend,
    dxmt_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn read_settings(dir: &Path) -> Result<(Backend, Option<Vec<u8>>), String> {
    let path = dir.join(SETTINGS);
    let before = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Backend::Auto, None));
        }
        Err(error) => return Err(format!("Could not inspect bottle graphics: {error}")),
    };
    if !before.file_type().is_file() || before.len() > SETTINGS_LIMIT {
        return Err("Bottle graphics preference must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| format!("Could not open bottle graphics: {error}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle graphics changed while opening it".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(SETTINGS_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > SETTINGS_LIMIT
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(&path).map_err(|e| e.to_string())?,
        )
    {
        return Err("Bottle graphics changed while reading it".into());
    }
    let settings: Settings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid bottle graphics preference: {error}"))?;
    if settings.version != 1 {
        return Err("Unsupported bottle graphics preference version".into());
    }
    Ok((settings.backend, Some(bytes)))
}

// The complete reviewed package identity covers both renderer views, the
// dispatcher, receipts and source notices. A manifest by itself is not proof
// that a random/custom Wine runtime supports Caskrun's selection protocol.
fn dxmt_support(dir: &Path, wine: &Path) -> Result<(), String> {
    let approved = runtime_migration::SWITCHABLE_WINE_11_IDENTITY;
    let record = recorded_runtime_for_prefix(dir, Some(wine))?;
    let identity = match record {
        Some(record) if record.runtime_identity.as_deref() == Some(approved) => {
            if !runtime_record_matches(&record, wine)? {
                return Err("This bottle uses a different Windows engine".into());
            }
            approved.to_owned()
        }
        _ => {
            runtime_identity::bundled_runtime_attestation(wine)?
                .ok_or("DXMT requires Caskrun's verified graphics engine package")?
                .identity
        }
    };
    if identity != approved {
        return Err("DXMT is not included in this Windows engine package".into());
    }
    let root = wine
        .parent()
        .and_then(Path::parent)
        .ok_or("The Windows engine has no package directory")?;
    let receipt_path = root.join("components.json");
    let metadata = fs::symlink_metadata(&receipt_path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err("The graphics component receipt is missing or invalid".into());
    }
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid graphics component receipt: {e}"))?;
    validate_receipt(&receipt)?;
    for architecture in ["x86_64", "i386"] {
        for dll in ["d3d11.dll", "dxgi.dll", "d3d10core.dll", "winemetal.dll"] {
            if !path_is_real_file(
                &root
                    .join(DXMT_ROOT)
                    .join("lib/wine")
                    .join(format!("{architecture}-windows"))
                    .join(dll),
            ) {
                return Err("The DXMT graphics component is incomplete".into());
            }
        }
    }
    Ok(())
}

fn validate_receipt(receipt: &serde_json::Value) -> Result<(), String> {
    if receipt["format"] != "caskrun-runtime-components"
        || receipt["version"] != 1
        || receipt["layout"] != "switchable-v1"
        || receipt["default_backend"] != "wined3d"
        || receipt["backend_roots"]["wined3d"] != "runtime"
        || receipt["backend_roots"]["dxmt"] != DXMT_ROOT
        || receipt["game_validated"] != false
    {
        return Err("This engine does not have the reviewed graphics selection layout".into());
    }
    Ok(())
}

fn result(dir: &Path, id: &str, selected: Backend, wine: Option<&Path>) -> GraphicsResult {
    let support = wine
        .ok_or_else(|| "The Windows engine is unavailable".to_owned())
        .and_then(|wine| dxmt_support(dir, wine));
    let available = support.is_ok();
    GraphicsResult {
        bottle_id: id.to_owned(),
        selected_backend: selected,
        effective_backend: selected.effective(),
        dxmt_available: available,
        unavailable_reason: support.err(),
        warning: if selected == Backend::Dxmt {
            Some(if available {
                "DXMT is experimental. Rendering probes pass; game compatibility still needs testing."
            } else {
                "This bottle requests DXMT, but it is unavailable. Choose Automatic or restore the matching engine before opening apps."
            }.into())
        } else {
            None
        },
    }
}

pub(super) fn bottle_graphics(home: &Path, id: &str) -> Result<GraphicsResult, String> {
    let dir = bottle_dir(home, id)?;
    let (backend, _) = read_settings(&dir)?;
    Ok(result(&dir, id, backend, wine_path().as_deref()))
}

pub(super) fn validate_portable_settings(dir: &Path) -> Result<(), String> {
    read_settings(dir).map(|_| ())
}

pub(super) fn set_bottle_graphics(
    home: &Path,
    id: &str,
    backend: Backend,
) -> Result<GraphicsResult, String> {
    let wine = wine_path().ok_or("The Windows engine is unavailable")?;
    set_with_wine(home, id, backend, &wine)
}

fn set_with_wine(
    home: &Path,
    id: &str,
    backend: Backend,
    wine: &Path,
) -> Result<GraphicsResult, String> {
    let _lock = lock_bottle(home, id)?;
    let migration = runtime_migration::maybe_migrate_bundled_runtime(home, id, wine)?;
    let result = set_locked(home, id, backend, wine);
    if result.is_err()
        && let Some(backup) = migration
    {
        backup.restore().map_err(|error| format!("Graphics update failed and its previous runtime record could not be restored: {error}"))?;
    }
    result
}

fn set_locked(
    home: &Path,
    id: &str,
    backend: Backend,
    wine: &Path,
) -> Result<GraphicsResult, String> {
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    let (guard, snapshot) = open_stable_directory(&prefix)?;
    let (_, original) = read_settings(&dir)?;
    if backend == Backend::Dxmt {
        dxmt_support(&dir, wine)?;
    }
    refuse_running_bottle(&dir)?;
    refuse_live_wine_server(&dir, IDLE_TIMEOUT, Some(wine))?;
    prefix_processes::wait_until_idle(&dir, IDLE_TIMEOUT)?;
    refuse_running_bottle(&dir)?;
    bottle_control::assert_same_prefix(&prefix, &guard, &snapshot)?;
    if read_settings(&dir)?.1 != original {
        return Err("Bottle graphics changed before saving".into());
    }
    cancellation::check()?;
    let updated = serde_json::to_vec_pretty(&Settings {
        version: 1,
        backend,
    })
    .map_err(|e| e.to_string())?;
    write_preference(&dir.join(SETTINGS), &updated)?;
    if !read_settings(&dir).is_ok_and(|value| value.0 == backend) {
        // Roll back only our exact write. An independent editor's later write
        // must not be silently replaced by a stale value.
        if fs::read(dir.join(SETTINGS)).is_ok_and(|bytes| bytes == updated) {
            if let Some(bytes) = original {
                write_preference(&dir.join(SETTINGS), &bytes)?;
            } else {
                fs::remove_file(dir.join(SETTINGS)).map_err(|e| e.to_string())?;
            }
            return Err("The graphics preference could not be verified. The previous preference was restored".into());
        }
        return Err(
            "The graphics preference changed during verification; review it before opening apps"
                .into(),
        );
    }
    Ok(result(&dir, id, backend, Some(wine)))
}

fn write_preference(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(super) fn selected_runtime_root(dir: &Path, wine: &Path) -> Result<PathBuf, String> {
    let package = wine
        .parent()
        .and_then(Path::parent)
        .ok_or("The Windows engine has no package directory")?;
    match read_settings(dir)?.0 {
        Backend::Dxmt => {
            dxmt_support(dir, wine)?;
            Ok(package.join(DXMT_ROOT))
        }
        _ => Ok(package.join("runtime")),
    }
}

/// Apply to every Wine client, including setup and tools. Child processes
/// inherit this fixed selection. The dispatcher accepts only known views.
pub(super) fn configure_launch(
    command: &mut Command,
    dir: &Path,
    wine: &Path,
) -> Result<(), String> {
    let (backend, _) = read_settings(dir)?;
    command.env("CASKRUN_GRAPHICS_BACKEND", backend.effective().value());
    if backend == Backend::Dxmt {
        dxmt_support(dir, wine)?;
        let inherited = env::var("WINEDLLOVERRIDES").unwrap_or_default();
        let overrides = dxmt_overrides(&inherited)?;
        let prefix = dir.join("prefix");
        if path_is_real_file(&prefix.join("system.reg"))
            && ["system32", "syswow64"].iter().any(|folder| {
                fs::symlink_metadata(
                    prefix
                        .join("drive_c/windows")
                        .join(folder)
                        .join("winemetal.dll"),
                )
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            })
        {
            refuse_running_bottle(dir)?;
            refuse_live_wine_server(dir, IDLE_TIMEOUT, Some(wine))?;
            prefix_processes::wait_until_idle(dir, IDLE_TIMEOUT)?;
        }
        prepare_bridge_placeholders(dir, wine)?;
        let cache = real_subdirectory(dir, "dxmt-cache")?;
        let logs = real_subdirectory(dir, "logs")?;
        command.env("DXMT_SHADER_CACHE_PATH", cache).env(
            "DXMT_LOG_PATH",
            format!("Z:{}", logs.to_string_lossy().replace('/', "\\")),
        );
        // Explicit DXMT selection overrides only its graphics modules for this
        // launch. Preserve unrelated environment overrides and all registry /
        // application-local DLL files so switching back is lossless.
        command.env("WINEDLLOVERRIDES", overrides);
    }
    Ok(())
}

/// Older approved prefixes predate winemetal. Add only missing bootstrap
/// placeholders; existing app files, symlinks and graphics DLLs are preserved.
fn prepare_bridge_placeholders(dir: &Path, wine: &Path) -> Result<(), String> {
    use std::io::Write;
    let prefix = dir.join("prefix");
    if !path_is_real_file(&prefix.join("system.reg")) {
        return Ok(());
    }
    let package = wine
        .parent()
        .and_then(Path::parent)
        .ok_or("Missing engine package")?;
    let mut created: Vec<(PathBuf, fs::Metadata)> = Vec::new();
    let result = (|| {
        for (folder, architecture) in [("system32", "x86_64"), ("syswow64", "i386")] {
            let mut parent = prefix.clone();
            let mut guards = Vec::new();
            for part in ["drive_c", "windows", folder] {
                parent.push(part);
                guards.push((parent.clone(), open_stable_directory(&parent)?));
            }
            let path = parent.join("winemetal.dll");
            match fs::symlink_metadata(&path) {
                Ok(value) if value.file_type().is_file() => continue,
                Ok(_) => return Err("The bottle's Metal bridge path is not a regular file; its existing contents were preserved".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.to_string()),
            }
            let source = package
                .join("runtime/lib/wine")
                .join(format!("{architecture}-windows/winemetal.dll"));
            let mut input = OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(source)
                .map_err(|e| e.to_string())?;
            if input.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
                return Err("The Metal bridge exceeds the expected size".into());
            }
            for (path, (guard, snapshot)) in &guards {
                bottle_control::assert_same_prefix(path, guard, snapshot)?;
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(&path)
                .map_err(|e| e.to_string())?;
            created.push((path, output.metadata().map_err(|e| e.to_string())?));
            std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            for (path, (guard, snapshot)) in &guards {
                bottle_control::assert_same_prefix(path, guard, snapshot)?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for (path, snapshot) in created {
            if fs::symlink_metadata(&path).is_ok_and(|current| {
                current.file_type().is_file()
                    && current.dev() == snapshot.dev()
                    && current.ino() == snapshot.ino()
            }) {
                let _ = fs::remove_file(path);
            }
        }
    }
    result
}

fn dxmt_overrides(inherited: &str) -> Result<String, String> {
    const MANAGED: [&str; 4] = ["d3d11", "dxgi", "d3d10core", "winemetal"];
    let mut output = Vec::new();
    for entry in inherited
        .split(';')
        .filter(|value| !value.trim().is_empty())
    {
        let (names, order) = entry
            .split_once('=')
            .ok_or("Invalid DLL override environment; clear it before choosing DXMT")?;
        let preserved: Vec<_> = names
            .split(',')
            .filter(|name| {
                let name = name.trim().trim_start_matches('*').to_ascii_lowercase();
                !MANAGED.contains(&name.trim_end_matches(".dll"))
            })
            .collect();
        if !preserved.is_empty() {
            output.push(format!("{}={order}", preserved.join(",")));
        }
    }
    output.push("d3d11,dxgi,d3d10core,winemetal=b".into());
    Ok(output.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("graphics-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_preference_defaults_to_wined3d_without_writing() {
        let dir = fixture();
        assert_eq!(read_settings(&dir).unwrap(), (Backend::Auto, None));
        assert_eq!(Backend::Auto.effective(), Backend::Wined3d);
        assert!(!dir.join(SETTINGS).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preference_is_bounded_versioned_and_closed() {
        let dir = fixture();
        let path = dir.join(SETTINGS);
        for bytes in [
            r#"{"version":2,"backend":"dxmt"}"#,
            r#"{"version":1,"backend":"dxvk"}"#,
            r#"{"version":1,"backend":"dxmt","path":"elsewhere"}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(read_settings(&dir).is_err());
        }
        fs::write(&path, vec![b' '; 4097]).unwrap();
        assert!(read_settings(&dir).is_err());
        fs::remove_file(&path).unwrap();
        symlink("absent", &path).unwrap();
        assert!(read_settings(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dxmt_preference_does_not_silently_fallback_when_unavailable() {
        let dir = fixture();
        let status = result(&dir, "example", Backend::Dxmt, None);
        assert!(!status.dxmt_available);
        assert_eq!(status.effective_backend, Backend::Dxmt);
        assert!(status.warning.is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn graphics_overrides_preserve_other_modules_and_replace_only_selected_modules() {
        assert_eq!(
            dxmt_overrides("mscoree,mshtml=;d3d11,foo=n;*DXGI.dll=n,b;vcruntime140=n,b").unwrap(),
            "mscoree,mshtml=;foo=n;vcruntime140=n,b;d3d11,dxgi,d3d10core,winemetal=b"
        );
        assert!(dxmt_overrides("malformed").is_err());
    }

    #[test]
    fn a_manifest_cannot_choose_arbitrary_runtime_paths() {
        let mut receipt = serde_json::json!({"format":"caskrun-runtime-components", "version":1,
            "layout":"switchable-v1", "default_backend":"wined3d", "game_validated":false,
            "backend_roots":{"wined3d":"runtime","dxmt":DXMT_ROOT}});
        assert!(validate_receipt(&receipt).is_ok());
        receipt["backend_roots"]["dxmt"] = "../../elsewhere".into();
        assert!(validate_receipt(&receipt).is_err());
    }

    #[test]
    fn preferences_are_bottle_scoped_and_unavailable_dxmt_leaves_original_intact() {
        let home = fixture();
        let first = create(&home, "First graphics bottle").unwrap();
        let second = create(&home, "Second graphics bottle").unwrap();
        let wine = home.join("custom-wine");
        fs::write(&wine, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&wine, fs::Permissions::from_mode(0o755)).unwrap();
        let status = set_with_wine(&home, &first.id, Backend::Wined3d, &wine).unwrap();
        assert_eq!(status.selected_backend, Backend::Wined3d);
        let first_dir = bottle_dir(&home, &first.id).unwrap();
        let original = fs::read(first_dir.join(SETTINGS)).unwrap();
        assert!(set_with_wine(&home, &first.id, Backend::Dxmt, &wine).is_err());
        assert_eq!(fs::read(first_dir.join(SETTINGS)).unwrap(), original);
        assert_eq!(
            read_settings(&bottle_dir(&home, &second.id).unwrap())
                .unwrap()
                .0,
            Backend::Auto
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn live_tracked_process_refuses_preference_changes_and_keeps_exact_original() {
        let home = fixture();
        let bottle = create(&home, "Busy graphics bottle").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let original = b"{\"version\":1,\"backend\":\"auto\"}";
        fs::write(dir.join(SETTINGS), original).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let error =
            set_with_wine(&home, &bottle.id, Backend::Wined3d, &home.join("wine")).unwrap_err();
        assert!(error.contains("Close Windows apps"));
        assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), original);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn untracked_file_owner_blocks_switch_without_being_signalled() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let home = fixture();
        let bottle = create(&home, "Held graphics bottle").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let held = dir.join("held-fixture.txt");
        fs::write(&held, b"fixture").unwrap();
        let original = b"{\"version\":1,\"backend\":\"auto\"}";
        fs::write(dir.join(SETTINGS), original).unwrap();
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let wine = home.join("wine");
        fs::write(&wine, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&wine, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(set_with_wine(&home, &bottle.id, Backend::Wined3d, &wine).is_err());
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), original);
        drop(owner);
        assert_eq!(
            set_with_wine(&home, &bottle.id, Backend::Wined3d, &wine)
                .unwrap()
                .selected_backend,
            Backend::Wined3d
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn backup_and_duplicate_preserve_renderer_preference_without_paths() {
        let home = fixture();
        let bottle = create(&home, "Graphics source").unwrap();
        let source = bottle_dir(&home, &bottle.id).unwrap();
        save_json(
            &source.join(SETTINGS),
            &Settings {
                version: 1,
                backend: Backend::Dxmt,
            },
        )
        .unwrap();
        let duplicated = duplicate_bottle_with_runtime(&home, &bottle.id, None, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &duplicated.id).unwrap())
                .unwrap()
                .0,
            Backend::Dxmt
        );
        let backup = home.join("graphics.caskrun-bottle");
        export_bottle_with_runtime(&home, &bottle.id, &backup, None).unwrap();
        let imported = import_bottle(&home, &backup, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &imported.bottle.id).unwrap())
                .unwrap()
                .0,
            Backend::Dxmt
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_bridge_placeholders_are_added_without_replacing_existing_files() {
        let dir = fixture();
        let prefix = dir.join("prefix");
        let package = dir.join("engine");
        let wine = package.join("bin/wine");
        fs::create_dir_all(wine.parent().unwrap()).unwrap();
        for (folder, architecture) in [("system32", "x86_64"), ("syswow64", "i386")] {
            fs::create_dir_all(prefix.join("drive_c/windows").join(folder)).unwrap();
            let source = package
                .join("runtime/lib/wine")
                .join(format!("{architecture}-windows"));
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("winemetal.dll"), architecture).unwrap();
        }
        fs::write(prefix.join("system.reg"), "registry").unwrap();
        let preserved = prefix.join("drive_c/windows/system32/winemetal.dll");
        fs::write(&preserved, "existing application file").unwrap();
        prepare_bridge_placeholders(&dir, &wine).unwrap();
        assert_eq!(
            fs::read_to_string(&preserved).unwrap(),
            "existing application file"
        );
        assert_eq!(
            fs::read_to_string(prefix.join("drive_c/windows/syswow64/winemetal.dll")).unwrap(),
            "i386"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bridge_failure_removes_only_newly_created_placeholders() {
        let dir = fixture();
        let prefix = dir.join("prefix");
        let package = dir.join("engine");
        for folder in ["system32", "syswow64"] {
            fs::create_dir_all(prefix.join("drive_c/windows").join(folder)).unwrap();
        }
        fs::write(prefix.join("system.reg"), "registry").unwrap();
        let source = package.join("runtime/lib/wine/x86_64-windows");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("winemetal.dll"), "bridge").unwrap();
        let preserved = prefix.join("drive_c/windows/syswow64/winemetal.dll");
        symlink("absent", &preserved).unwrap();
        assert!(prepare_bridge_placeholders(&dir, &package.join("bin/wine")).is_err());
        assert!(
            !prefix
                .join("drive_c/windows/system32/winemetal.dll")
                .exists()
        );
        assert_eq!(fs::read_link(preserved).unwrap(), PathBuf::from("absent"));
        fs::remove_dir_all(dir).unwrap();
    }
}
