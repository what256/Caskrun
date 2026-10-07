//! Give independently launched Windows processes their own macOS identity.
//!
//! The Wine callback reads bounded PE metadata; it never executes the PE or
//! chooses a host executable from its contents. Generated apps copy only the
//! selected package's signed native loader. No bottle lock is taken here:
//! Wine can call this while the original launch holds that same lock.

use super::*;
use std::os::unix::ffi::OsStrExt;

const MAX_LOADER_BYTES: u64 = 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 512 * 1024;
const ENVIRONMENT: &[&str] = &[
    "CASKRUN_NATIVE_CORE",
    "CASKRUN_NATIVE_BOTTLE",
    "CASKRUN_NATIVE_DATA",
    "CASKRUN_NATIVE_NTDLL",
    "CASKRUN_NATIVE_IMAGE",
    "CASKRUN_NATIVE_INITIAL_IMAGE",
    "CASKRUN_NATIVE_EXEC",
];

struct RuntimeInputs {
    root: PathBuf,
    loader: Vec<u8>,
    ntdll: PathBuf,
    icon_renderer: Option<PathBuf>,
    renderer_bytes: Vec<u8>,
    fallback_icon: Option<Vec<u8>>,
    fallback_artwork: Option<(Vec<u8>, Vec<u8>)>,
}

fn read_stable_file(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    read_stable_open_file(path, maximum).map(|(_, bytes)| bytes)
}

// Darwin O_NONBLOCK ensures a cache/input raced from a regular file to a
// FIFO cannot trap the callback or bottle shutdown in an unbounded open.
fn open_stable_regular_file(path: &Path, before: &fs::Metadata) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !before.file_type().is_file()
        || !same_snapshot(before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            before,
            &fs::symlink_metadata(path).map_err(|e| e.to_string())?,
        )
    {
        return Err("Native identity input changed while opening".into());
    }
    Ok(file)
}

fn read_stable_open_file(path: &Path, maximum: u64) -> Result<(File, Vec<u8>), String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() || before.len() > maximum {
        return Err("Native identity input must be a bounded regular file".into());
    }
    let mut file = open_stable_regular_file(path, &before)?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.by_ref()
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > maximum
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(path).map_err(|e| e.to_string())?,
        )
    {
        return Err("Native identity input changed while reading".into());
    }
    Ok((file, bytes))
}

fn runtime_inputs(wine: &Path) -> Result<Option<RuntimeInputs>, String> {
    let Some(root) = wine.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let marker = root.join("native-app-identity.txt");
    if !marker.exists() {
        return Ok(None);
    }
    if read_stable_file(&marker, 128)? != b"caskrun-native-app-identity-v1\n" {
        return Err("Unrecognized native identity runtime marker".into());
    }
    if !is_usable_wine(wine)
        || !root.join("SOURCES.txt").is_file()
        || !root.join("bin/_dispatch").is_file()
    {
        return Err("Native app identity needs a complete independent Caskrun package".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let loader_path = root.join("runtime/lib/wine/x86_64-unix/WindowsApp");
    let loader = read_stable_file(&loader_path, MAX_LOADER_BYTES)?;
    if loader.len() < 32
        || loader[..4] != [0xcf, 0xfa, 0xed, 0xfe]
        || u32::from_le_bytes(loader[4..8].try_into().unwrap()) != 0x0100_0007
        || !is_executable(&loader_path)
    {
        return Err("Native app loader is not the package's executable x86_64 Mach-O image".into());
    }
    let ntdll = root.join("runtime/lib/wine/x86_64-unix/ntdll.so");
    let (_guard, _) = open_stable_directory(ntdll.parent().unwrap())?;
    if !fs::symlink_metadata(&ntdll).is_ok_and(|m| m.file_type().is_file()) {
        return Err("Native app loader has no packaged NTDLL".into());
    }
    let icon_renderer = root.join("bin/native-icon");
    let renderer_bytes = read_stable_file(&icon_renderer, MAX_LOADER_BYTES)?;
    if !is_executable(&icon_renderer)
        || renderer_bytes.len() < 32
        || renderer_bytes[..4] != [0xcf, 0xfa, 0xed, 0xfe]
        || u32::from_le_bytes(renderer_bytes[4..8].try_into().unwrap()) != 0x0100_000c
    {
        return Err("Native icon renderer is not the package's ARM64 utility".into());
    }
    let fallback_icon = read_stable_file(&root.join("native-default-icon.png"), 4 * 1024 * 1024)?;
    let fallback_icns = read_stable_file(&root.join("native-default-icon.icns"), 16 * 1024 * 1024)?;
    let fallback_png =
        read_stable_file(&root.join("native-default-rendered.png"), 4 * 1024 * 1024)?;
    validate_native_artwork(&fallback_icns, &fallback_png)?;
    Ok(Some(RuntimeInputs {
        root,
        loader,
        ntdll,
        icon_renderer: Some(icon_renderer),
        renderer_bytes,
        fallback_icon: Some(fallback_icon),
        fallback_artwork: Some((fallback_icns, fallback_png)),
    }))
}

/// A narrow shutdown proof for Caskrun's generated native aliases. The held
/// files retain the exact observed inodes while the caller checks process maps
/// and signals by audit token. This does not authorize raw PID signals.
pub(crate) struct ShutdownAlias {
    pub(crate) loader_path: PathBuf,
    pub(crate) loader_dev: u64,
    pub(crate) loader_ino: u64,
    pub(crate) ntdll_path: PathBuf,
    pub(crate) ntdll_dev: u64,
    pub(crate) ntdll_ino: u64,
    _loader_file: File,
    _ntdll_file: File,
}

/// Call only after ordinary runtime verification, while holding the bottle
/// lock and unchanged prefix guard. Unaliased Wine/Steam are never candidates.
pub(crate) fn validate_shutdown_alias(
    dir: &Path,
    wine: &Path,
    candidate: &Path,
) -> Result<Option<ShutdownAlias>, String> {
    let Some(inputs) = runtime_inputs(wine)? else {
        return Ok(None);
    };
    validate_shutdown_alias_with_inputs(dir, candidate, &inputs)
}

fn validate_shutdown_alias_with_inputs(
    dir: &Path,
    candidate: &Path,
    inputs: &RuntimeInputs,
) -> Result<Option<ShutdownAlias>, String> {
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let cache = dir.join("native-apps");
    if !candidate.is_absolute() || !candidate.starts_with(&cache) {
        return Ok(None);
    }
    let Some(macos) = candidate.parent() else {
        return Ok(None);
    };
    let Some(contents) = macos.parent() else {
        return Ok(None);
    };
    let Some(app) = contents.parent() else {
        return Ok(None);
    };
    let app_name = app
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".app"));
    // Previous versions kept the digest in the app filename. Accept only that
    // exact legacy shape for already-running aliases; new app filenames have
    // a friendly name and keep the full identity digest in their parent.
    let legacy = app.parent() == Some(cache.as_path())
        && app_name
            .and_then(|name| name.rsplit_once('-'))
            .is_some_and(|(name, key)| !name.is_empty() && native_key(key, 16));
    let identity = app.parent().filter(|parent| {
        parent.parent() == Some(cache.as_path())
            && parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|key| native_key(key, 64))
            && app_name.is_some_and(|name| {
                display_name(name).is_some() && filename_component(name) == name
            })
    });
    if candidate.file_name() != Some(std::ffi::OsStr::new("WindowsApp"))
        || macos.file_name() != Some(std::ffi::OsStr::new("MacOS"))
        || contents.file_name() != Some(std::ffi::OsStr::new("Contents"))
        || (!legacy && identity.is_none())
    {
        return Err("Shutdown candidate is not a direct generated native app".into());
    }
    let _cache = open_cache_directory(&cache)?;
    let _identity = identity.map(open_cache_directory).transpose()?;
    let (_app, _) = open_stable_directory(app)?;
    let (_contents, _) = open_stable_directory(contents)?;
    let (_macos, _) = open_stable_directory(macos)?;
    if candidate.canonicalize().map_err(|e| e.to_string())? != candidate {
        return Err("Shutdown native loader path must be canonical".into());
    }
    let (loader_file, bytes) = read_stable_open_file(candidate, MAX_LOADER_BYTES)?;
    if bytes != inputs.loader || !is_executable(candidate) {
        return Err("Shutdown candidate differs from the selected package's native loader".into());
    }
    let loader_metadata = loader_file.metadata().map_err(|e| e.to_string())?;
    let before = fs::symlink_metadata(&inputs.ntdll).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() {
        return Err("Selected native NTDLL is not a regular file".into());
    }
    let ntdll_file = open_stable_regular_file(&inputs.ntdll, &before)?;
    if !same_snapshot(&before, &ntdll_file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(&inputs.ntdll).map_err(|e| e.to_string())?,
        )
    {
        return Err("Selected native NTDLL changed during shutdown proof".into());
    }
    Ok(Some(ShutdownAlias {
        loader_path: candidate.to_path_buf(),
        loader_dev: loader_metadata.dev(),
        loader_ino: loader_metadata.ino(),
        ntdll_path: inputs.ntdll.clone(),
        ntdll_dev: before.dev(),
        ntdll_ino: before.ino(),
        _loader_file: loader_file,
        _ntdll_file: ntdll_file,
    }))
}

/// Called after normal prefix/runtime verification, before spawning Wine.
/// Older independent engines retain their original behavior.
pub(super) fn configure_launch(
    command: &mut Command,
    home: &Path,
    id: &str,
    wine: &Path,
    image: &Path,
) -> Result<(), String> {
    for name in ENVIRONMENT {
        command.env_remove(name);
    }
    let Some(inputs) = runtime_inputs(wine)? else {
        return Ok(());
    };
    let dir = bottle_dir(home, id)?;
    let (_guard, _) = open_stable_directory(&dir.join("prefix"))?;
    let data = home.canonicalize().map_err(|e| e.to_string())?;
    let core = env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| e.to_string())?;
    let process_image = if image
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("msi"))
    {
        dir.join("prefix/drive_c/windows/system32/msiexec.exe")
    } else {
        image.to_path_buf()
    };
    // Steam retains its independently tested process-role/focus handling.
    // Reuse the same native artwork renderer without changing its loader path.
    if command
        .get_envs()
        .any(|(key, value)| key == "CASKRUN_STEAM_MODE" && value == Some(std::ffi::OsStr::new("1")))
    {
        if let Ok(loader) = prepare_with_inputs(home, id, image, Some(image), &inputs) {
            if let Some(contents) = loader.parent().and_then(Path::parent) {
                let png = contents.join("Resources/app.png");
                if read_stable_file(&png, 4 * 1024 * 1024).is_ok() {
                    command.env("CASKRUN_STEAM_ICON", png);
                }
            }
        }
    }
    command
        .env("CASKRUN_NATIVE_CORE", core)
        .env("CASKRUN_NATIVE_BOTTLE", id)
        .env("CASKRUN_NATIVE_DATA", data)
        .env("CASKRUN_NATIVE_NTDLL", inputs.ntdll)
        .env("CASKRUN_NATIVE_IMAGE", process_image)
        // This permits only the user-selected initial host EXE. Children must
        // provide their own resolved image and cannot inherit its label.
        .env("CASKRUN_NATIVE_INITIAL_IMAGE", image);
    Ok(())
}

pub(super) fn prepare(home: &Path, id: &str, image: &Path) -> Result<PathBuf, String> {
    let wine = wine_path().ok_or("Native identity runtime is unavailable")?;
    let inputs = runtime_inputs(&wine)?.ok_or("This engine has no native app identity support")?;
    let initial = env::var_os("CASKRUN_NATIVE_INITIAL_IMAGE").map(PathBuf::from);
    prepare_with_inputs(home, id, image, initial.as_deref(), &inputs)
}

/// Reuse a previously generated, fully validated native PNG in Home. This
/// never generates a bundle, spawns a utility, or starts a Windows process.
pub(super) fn cached_icon(home: &Path, id: &str, image: &Path) -> Option<PathBuf> {
    let wine = wine_path()?;
    let inputs = runtime_inputs(&wine).ok()??;
    let loader = prepare_with_inputs_mode(home, id, image, None, &inputs, false).ok()?;
    let png = loader.parent()?.parent()?.join("Resources/app.png");
    read_stable_file(&png, 4 * 1024 * 1024).ok()?;
    Some(png)
}

fn prepare_with_inputs(
    home: &Path,
    id: &str,
    image: &Path,
    initial: Option<&Path>,
    inputs: &RuntimeInputs,
) -> Result<PathBuf, String> {
    prepare_with_inputs_mode(home, id, image, initial, inputs, true)
}

fn prepare_with_inputs_mode(
    home: &Path,
    id: &str,
    image: &Path,
    initial: Option<&Path>,
    inputs: &RuntimeInputs,
    create: bool,
) -> Result<PathBuf, String> {
    if !image.is_absolute() || !is_independent_runtime(image) {
        return Err("Native identity requires an independent absolute Windows image path".into());
    }
    let dir = bottle_dir(home, id)?;
    let bottle = read_bottle_record(&dir, id)?;
    let prefix = dir.join("prefix");
    let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
    let canonical = image.canonicalize().map_err(|e| e.to_string())?;
    let inside_prefix = canonical.starts_with(&prefix);
    let inside_runtime = canonical.starts_with(inputs.root.join("runtime/lib/wine"));
    let selected_initial = initial.is_some_and(|initial| {
        initial.is_absolute() && initial.canonicalize().is_ok_and(|path| path == canonical)
    });
    if !inside_prefix && !inside_runtime && !selected_initial {
        return Err(
            "Native identity image is outside this bottle and its explicitly selected installer"
                .into(),
        );
    }
    // Only explicitly selected images or this independently packaged runtime
    // can be resolved outside the prefix. Never follow arbitrary source links.
    if !inside_runtime && fs::symlink_metadata(image).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Native identity image must not be a symbolic link".into());
    }
    let before = fs::symlink_metadata(&canonical).map_err(|e| e.to_string())?;
    if !before.file_type().is_file() {
        return Err("Native identity image is not a regular file".into());
    }
    let file = open_stable_regular_file(&canonical, &before)?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Windows image changed while preparing native identity".into());
    }
    check_pe(&file, before.len())?;
    let artwork = app_icons::executable_artwork(&file, before.len());
    let filename = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Windows App");
    let saved_name = saved_name(&dir, &canonical)?;
    let catalog_setup = catalog_installer_name(
        home,
        id,
        &dir,
        &prefix,
        &canonical,
        artwork.generic_installer,
        &bottle.name,
    )?;
    let builtin_name = match filename.to_ascii_lowercase().as_str() {
        "winecfg" => Some("Windows Configuration"),
        "wineconsole" | "cmd" | "conhost" => Some("Windows Console"),
        "regedit" => Some("Registry Editor"),
        "taskmgr" => Some("Task Manager"),
        "explorer" => Some("File Explorer"),
        "uninstaller" => Some("Installed Programs"),
        "control" => Some("Windows Control Panel"),
        "winebrowser" | "iexplore" => Some("Web Browser"),
        "wineboot" => Some("Windows Setup"),
        "notepad" => Some("Notepad"),
        _ => None,
    };
    let display = saved_name
        .or(catalog_setup)
        .or_else(|| builtin_name.map(str::to_owned))
        .or(artwork.display_name)
        .and_then(|name| display_name(&name))
        .or_else(|| display_name(filename))
        .unwrap_or_else(|| "Windows App".into());
    let display = if inside_runtime {
        display
            .strip_prefix("Wine ")
            .and_then(display_name)
            .unwrap_or(display)
    } else {
        display
    };
    let is_system_installer = filename.eq_ignore_ascii_case("msiexec")
        && (inside_runtime
            || canonical.starts_with(prefix.join("drive_c/windows/system32"))
            || canonical.starts_with(prefix.join("drive_c/windows/syswow64")));
    let display = if is_system_installer {
        format!(
            "{} Setup",
            display_name(&bottle.name).unwrap_or_else(|| "Windows App".into())
        )
    } else {
        display
    };
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(&canonical).map_err(|e| e.to_string())?,
        )
    {
        return Err("Windows image changed while reading native artwork".into());
    }
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"caskrun-native-app-v1\0");
    digest.update(canonical.as_os_str().as_bytes());
    digest.update(display.as_bytes());
    for value in [
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime() as u64,
        before.mtime_nsec() as u64,
        before.ctime() as u64,
        before.ctime_nsec() as u64,
    ] {
        digest.update(value.to_be_bytes());
    }
    digest.update(&inputs.loader);
    digest.update(&inputs.renderer_bytes);
    if let Some(fallback) = &inputs.fallback_icon {
        digest.update(fallback);
    }
    if let Some((icns, png)) = &inputs.fallback_artwork {
        digest.update(icns);
        digest.update(png);
    }
    if let Some(ico) = &artwork.ico {
        digest.update(ico);
    }
    let key = format!("{:x}", digest.finalize());
    let apps = dir.join("native-apps");
    if create {
        match fs::create_dir(&apps) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not create native app cache: {error}")),
        }
    }
    let _cache_guard = open_cache_directory(&apps)?;
    // macOS can show the .app filename in the Dock even when the bundle's
    // display name is clean. Keep the full cache key in an internal parent
    // directory so every user-visible filename is only the friendly name.
    let identity = apps.join(&key);
    if create {
        match fs::create_dir(&identity) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "Could not create native identity directory: {error}"
                ));
            }
        }
    }
    let _identity_guard = open_cache_directory(&identity)?;
    let app = identity.join(format!("{}.app", filename_component(&display)));
    let plist = plist(
        &display,
        id,
        &key,
        artwork.ico.is_some() || inputs.fallback_icon.is_some(),
        inputs.icon_renderer.is_some(),
    );
    let loader = app.join("Contents/MacOS/WindowsApp");
    if app.exists() || app.is_symlink() {
        validate_cached_app(&app, inputs, plist.as_bytes(), artwork.ico.as_deref())?;
        bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
        return Ok(loader);
    }
    if !create {
        return Err("No validated native artwork has been generated for this program yet".into());
    }
    let temporary = identity.join(format!(".native-app-{}", Uuid::new_v4()));
    let result = (|| {
        fs::create_dir(&temporary).map_err(|e| e.to_string())?;
        fs::create_dir(temporary.join("Contents")).map_err(|e| e.to_string())?;
        fs::create_dir(temporary.join("Contents/MacOS")).map_err(|e| e.to_string())?;
        fs::create_dir(temporary.join("Contents/Resources")).map_err(|e| e.to_string())?;
        write_new(
            &temporary.join("Contents/Info.plist"),
            plist.as_bytes(),
            0o600,
        )?;
        write_new(
            &temporary.join("Contents/MacOS/WindowsApp"),
            &inputs.loader,
            0o755,
        )?;
        let resources = temporary.join("Contents/Resources");
        let artwork_source = if let Some(ico) = &artwork.ico {
            let source = resources.join("app.ico");
            write_new(&source, ico, 0o600)?;
            Some(source)
        } else if let Some(fallback) = &inputs.fallback_icon {
            let source = resources.join("app-source.png");
            write_new(&source, fallback, 0o600)?;
            Some(source)
        } else {
            None
        };
        if let (Some(renderer), Some(source)) = (&inputs.icon_renderer, artwork_source) {
            // A malformed publisher image must not discard a valid app name.
            // Use one bounded render attempt, then the package's prebuilt
            // original fallback. No second subprocess can exceed Wine's 2s
            // callback budget or leave a retry renderer behind.
            let publisher = artwork
                .ico
                .as_ref()
                .and_then(|_| render_once(renderer, &source, &resources).ok());
            let (icns, png, mode) = if let Some((icns, png)) = publisher {
                (icns, png, b"publisher\n".as_slice())
            } else {
                let (icns, png) = inputs
                    .fallback_artwork
                    .as_ref()
                    .ok_or("Native package has no original fallback artwork")?;
                for name in ["app.icns", "app.png"] {
                    let path = resources.join(name);
                    if fs::symlink_metadata(&path).is_ok() {
                        fs::remove_file(&path).map_err(|e| e.to_string())?;
                    }
                }
                write_new(&resources.join("app.icns"), icns, 0o600)?;
                write_new(&resources.join("app.png"), png, 0o600)?;
                let original = inputs
                    .fallback_icon
                    .as_ref()
                    .ok_or("Native fallback source is missing")?;
                if artwork.ico.is_some() {
                    write_new(&resources.join("app-source.png"), original, 0o600)?;
                }
                (icns.clone(), png.clone(), b"fallback\n".as_slice())
            };
            validate_native_artwork(&icns, &png)?;
            use sha2::Digest;
            let receipt = format!(
                "{:x}\n{:x}\n",
                sha2::Sha256::digest(&icns),
                sha2::Sha256::digest(&png)
            );
            write_new(&resources.join("native-icon-mode"), mode, 0o600)?;
            write_new(
                &resources.join("native-icon.sha256"),
                receipt.as_bytes(),
                0o600,
            )?;
        }
        bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
        // Concurrent CEF helpers may prepare this exact identity. A losing
        // atomic rename accepts only an independently validated same bundle.
        match fs::rename(&temporary, &app) {
            Ok(()) => Ok(()),
            Err(error) if app.exists() => {
                validate_cached_app(&app, inputs, plist.as_bytes(), artwork.ico.as_deref())
                    .map_err(|detail| format!("Native app cache collision: {error}; {detail}"))
            }
            Err(error) => Err(format!("Could not save native app identity: {error}")),
        }
    })();
    if temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result?;
    Ok(loader)
}

fn render_once(
    renderer: &Path,
    source: &Path,
    resources: &Path,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(renderer)
        .arg("--input")
        .arg(source)
        .arg("--output")
        .arg(resources.join("app.icns"))
        .arg("--png-output")
        .arg(resources.join("app.png"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if !wait_child_bounded(&mut child, Duration::from_millis(1100))?
        .is_some_and(|status| status.success())
    {
        return Err("Publisher icon could not be rendered within its deadline".into());
    }
    let icns = read_stable_file(&resources.join("app.icns"), 16 * 1024 * 1024)?;
    let png = read_stable_file(&resources.join("app.png"), 4 * 1024 * 1024)?;
    validate_native_artwork(&icns, &png)?;
    Ok((icns, png))
}

// A cache directory's children can be atomically added by simultaneous
// launcher helpers. Validate its type and inode without treating these expected
// mtime/ctime changes as a directory substitution.
fn open_cache_directory(path: &Path) -> Result<File, String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !before.file_type().is_dir() {
        return Err("Native cache must be a real directory".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(path)
        .map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !opened.file_type().is_dir()
        || !after.file_type().is_dir()
        || before.dev() != opened.dev()
        || before.ino() != opened.ino()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
    {
        return Err("Native cache directory was replaced".into());
    }
    Ok(file)
}

fn check_pe(file: &File, length: u64) -> Result<(), String> {
    use std::os::unix::fs::FileExt;
    if length < 70 {
        return Err("Native identity needs a Windows PE program".into());
    }
    let mut dos = [0_u8; 64];
    file.read_exact_at(&mut dos, 0).map_err(|e| e.to_string())?;
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    if &dos[..2] != b"MZ" || offset < 64 || offset > length.saturating_sub(6) {
        return Err("Native identity needs a valid Windows PE header".into());
    }
    let mut pe = [0_u8; 6];
    file.read_exact_at(&mut pe, offset)
        .map_err(|e| e.to_string())?;
    if &pe[..4] != b"PE\0\0" || !matches!(u16::from_le_bytes([pe[4], pe[5]]), 0x014c | 0x8664) {
        return Err("Native identity supports x86 and x64 Windows programs".into());
    }
    Ok(())
}

// Catalog receipts are bounded atomic reads and take no bottle lock. Wine
// invokes this callback while its parent may already hold that lock. Only
// the exact selected catalog download and recognisable temporary Inno child
// receive a contextual name; ordinary programs retain their own metadata.
fn catalog_installer_name(
    home: &Path,
    id: &str,
    dir: &Path,
    prefix: &Path,
    image: &Path,
    generic_installer: bool,
    bottle_name: &str,
) -> Result<Option<String>, String> {
    // A damaged optional receipt cannot erase a valid app name. Decline its
    // contextual label and retain the program's own bounded PE metadata.
    let Ok(Some(attempt)) = steam_install::catalog_attempt(home, id) else {
        return Ok(None);
    };
    if attempt.stage != steam_install::CatalogStage::Running {
        return Ok(None);
    }
    let installer_file = match attempt.catalog_id.as_str() {
        "steam" => "SteamSetup.exe",
        "epic-games-store" => "EpicInstaller.msi",
        "gog-galaxy" => "GOGGalaxySetup.exe",
        "battle-net" => "Battle.net-Setup.exe",
        "ea-app" => "EAappInstaller.exe",
        "ubisoft-connect" => "UbisoftConnectInstaller.exe",
        "itch-io-app" => "itch-setup.exe",
        "rockstar-games-launcher" => "Rockstar-Games-Launcher.exe",
        _ => return Ok(None),
    };
    let selected_installer = image == dir.join("installers").join(installer_file);
    let inno_child = generic_installer
        && image
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        && image
            .strip_prefix(prefix.join("drive_c"))
            .is_ok_and(|path| {
                let parts: Vec<_> = path
                    .components()
                    .map(|part| part.as_os_str().to_str().map(str::to_ascii_lowercase))
                    .collect();
                let temp_index = match parts.as_slice() {
                    [Some(users), Some(_), Some(temp), Some(_), Some(_)]
                        if users == "users" && temp == "temp" =>
                    {
                        2
                    }
                    [
                        Some(users),
                        Some(_),
                        Some(appdata),
                        Some(local),
                        Some(temp),
                        Some(_),
                        Some(_),
                    ] if users == "users"
                        && appdata == "appdata"
                        && local == "local"
                        && temp == "temp" =>
                    {
                        4
                    }
                    [Some(windows), Some(temp), Some(_), Some(_)]
                        if windows == "windows" && temp == "temp" =>
                    {
                        1
                    }
                    _ => return false,
                };
                parts[temp_index + 1].as_deref().is_some_and(|part| {
                    part.starts_with("is-") && part.ends_with(".tmp") && part.len() > 7
                })
            });
    if selected_installer || inno_child {
        Ok(display_name(bottle_name).map(|name| format!("{name} Setup")))
    } else {
        Ok(None)
    }
}

fn saved_name(dir: &Path, image: &Path) -> Result<Option<String>, String> {
    let path = dir.join("launchers.json");
    if !path.exists() {
        return Ok(None);
    }
    let launchers: Vec<Launcher> =
        serde_json::from_slice(&read_stable_file(&path, MAX_METADATA_BYTES)?)
            .map_err(|_| "Invalid saved launcher metadata for native identity")?;
    Ok(launchers
        .into_iter()
        .take(1024)
        .find(|item| {
            Path::new(&item.path)
                .canonicalize()
                .is_ok_and(|path| path == image)
        })
        .map(|item| item.name))
}

fn display_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        || name.chars().count() > 120
    {
        return None;
    }
    let name = name.strip_suffix(".exe").unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn native_key(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn filename_component(name: &str) -> String {
    let value: String = name
        .chars()
        .take(80)
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let value = value.trim_matches(['.', ' ']);
    if value.is_empty() {
        "Windows App".into()
    } else {
        value.into()
    }
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn plist(name: &str, id: &str, key: &str, icon: bool, native_icon: bool) -> String {
    let icon = if icon && native_icon {
        "<key>CFBundleIconFile</key><string>app.icns</string>"
    } else if icon {
        "<key>CFBundleIconFile</key><string>app.ico</string>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleDisplayName</key><string>{name}</string>
<key>CFBundleIdentifier</key><string>dev.caskrun.windows.{id}.{key}</string>
<key>CFBundleExecutable</key><string>WindowsApp</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSPrincipalClass</key><string>WineApplication</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSUIElement</key><true/>
{icon}
</dict></plist>
"#,
        name = xml(name),
        id = id.replace('-', ""),
        key = &key[..32]
    )
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

fn validate_native_artwork(icns: &[u8], png: &[u8]) -> Result<(), String> {
    fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        if bytes.len() < 33
            || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
            || &bytes[12..16] != b"IHDR"
            || u32::from_be_bytes(bytes[8..12].try_into().ok()?) != 13
        {
            return None;
        }
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        (w > 0 && h > 0 && w <= 1024 && h <= 1024).then_some((w, h))
    }
    if png_dimensions(png) != Some((1024, 1024))
        || icns.len() < 8
        || &icns[..4] != b"icns"
        || u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize != icns.len()
    {
        return Err("Generated native icon has invalid or excessive image dimensions".into());
    }
    let mut cursor = 8;
    let mut count = 0;
    while cursor < icns.len() {
        if cursor + 8 > icns.len() || count >= 16 {
            return Err("Generated ICNS has invalid representations".into());
        }
        let length = u32::from_be_bytes(icns[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let end = cursor
            .checked_add(length)
            .ok_or("Generated ICNS size overflow")?;
        if length < 41 || end > icns.len() || png_dimensions(&icns[cursor + 8..end]).is_none() {
            return Err("Generated ICNS contains an invalid or excessive image".into());
        }
        cursor = end;
        count += 1;
    }
    if count == 0 {
        return Err("Generated ICNS is empty".into());
    }
    Ok(())
}

fn validate_cached_app(
    app: &Path,
    inputs: &RuntimeInputs,
    plist: &[u8],
    icon: Option<&[u8]>,
) -> Result<(), String> {
    let (_app_guard, _) = open_stable_directory(app)?;
    let (_contents_guard, _) = open_stable_directory(&app.join("Contents"))?;
    let (_macos_guard, _) = open_stable_directory(&app.join("Contents/MacOS"))?;
    let (_resources_guard, _) = open_stable_directory(&app.join("Contents/Resources"))?;
    let executable = app.join("Contents/MacOS/WindowsApp");
    if read_stable_file(&executable, MAX_LOADER_BYTES)? != inputs.loader
        || !is_executable(&executable)
        || read_stable_file(&app.join("Contents/Info.plist"), 16 * 1024)? != plist
    {
        return Err("Generated native app was modified; refusing its executable".into());
    }
    if let Some(icon) = icon {
        if read_stable_file(&app.join("Contents/Resources/app.ico"), 16 * 1024 * 1024)? != icon {
            return Err("Generated native app artwork was modified".into());
        }
    }
    if icon.is_none() {
        if let Some(fallback) = &inputs.fallback_icon {
            if read_stable_file(
                &app.join("Contents/Resources/app-source.png"),
                4 * 1024 * 1024,
            )? != *fallback
            {
                return Err("Generated native fallback artwork was modified".into());
            }
        }
    }
    let receipt = app.join("Contents/Resources/native-icon.sha256");
    if inputs.icon_renderer.is_some() && (icon.is_some() || inputs.fallback_icon.is_some()) {
        use sha2::{Digest, Sha256};
        let icns = read_stable_file(&app.join("Contents/Resources/app.icns"), 16 * 1024 * 1024)?;
        let png = read_stable_file(&app.join("Contents/Resources/app.png"), 4 * 1024 * 1024)?;
        validate_native_artwork(&icns, &png)?;
        let mode = read_stable_file(&app.join("Contents/Resources/native-icon-mode"), 32)?;
        if mode == b"fallback\n" {
            let (original_icns, original_png) = inputs
                .fallback_artwork
                .as_ref()
                .ok_or("Native original fallback is missing")?;
            if &icns != original_icns
                || &png != original_png
                || read_stable_file(
                    &app.join("Contents/Resources/app-source.png"),
                    4 * 1024 * 1024,
                )? != *inputs
                    .fallback_icon
                    .as_ref()
                    .ok_or("Native fallback source is missing")?
            {
                return Err("Native original fallback artwork was modified".into());
            }
        } else if mode != b"publisher\n" || icon.is_none() {
            return Err("Invalid native icon source mode".into());
        }
        let expected = format!("{:x}\n{:x}\n", Sha256::digest(&icns), Sha256::digest(&png));
        if read_stable_file(&receipt, 160)? != expected.as_bytes() {
            return Err("Generated native icon was modified".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        home: PathBuf,
        bottle: Bottle,
        inputs: RuntimeInputs,
    }
    impl Fixture {
        fn new() -> Self {
            let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("native-app-test-{}", Uuid::new_v4()));
            let bottle = create(&home, "Publisher launcher").unwrap();
            let mut loader = vec![0_u8; 64];
            loader[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            loader[4..8].copy_from_slice(&0x0100_0007_u32.to_le_bytes());
            let inputs = RuntimeInputs {
                root: home.join("independent-runtime"),
                loader,
                ntdll: home.join("independent-runtime/ntdll.so"),
                icon_renderer: None,
                renderer_bytes: Vec::new(),
                fallback_icon: None,
                fallback_artwork: None,
            };
            Self {
                home,
                bottle,
                inputs,
            }
        }
        fn image(&self, name: &str) -> PathBuf {
            let path = bottle_dir(&self.home, &self.bottle.id)
                .unwrap()
                .join("prefix/drive_c/Program Files")
                .join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut bytes = vec![0_u8; 70];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            bytes[68..70].copy_from_slice(&0x8664_u16.to_le_bytes());
            fs::write(&path, bytes).unwrap();
            path
        }
        fn prepare(&self, image: &Path) -> Result<PathBuf, String> {
            prepare_with_inputs(&self.home, &self.bottle.id, image, None, &self.inputs)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    fn fixture_artwork() -> (Vec<u8>, Vec<u8>) {
        let mut png = vec![0_u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[8..12].copy_from_slice(&13_u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&1024_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1024_u32.to_be_bytes());
        let mut icns = b"icns".to_vec();
        icns.extend_from_slice(&49_u32.to_be_bytes());
        icns.extend_from_slice(b"ic10");
        icns.extend_from_slice(&41_u32.to_be_bytes());
        icns.extend_from_slice(&png);
        (icns, png)
    }

    fn fallback_fixture(renderer_script: &[u8]) -> (Fixture, PathBuf) {
        let mut fixture = Fixture::new();
        let image = fixture.image("ArtworkGame.exe");
        fs::write(&image, app_icons::fixture_pe_with_icon()).unwrap();
        let renderer = fixture.home.join("original-test-renderer");
        fs::write(&renderer, renderer_script).unwrap();
        fs::set_permissions(&renderer, fs::Permissions::from_mode(0o755)).unwrap();
        fixture.inputs.icon_renderer = Some(renderer);
        fixture.inputs.renderer_bytes = renderer_script.to_vec();
        fixture.inputs.fallback_icon = Some(b"original fixture fallback source".to_vec());
        fixture.inputs.fallback_artwork = Some(fixture_artwork());
        add_launcher(&fixture.home, &fixture.bottle.id, "Artwork Game", &image).unwrap();
        (fixture, image)
    }

    fn app_name(loader: &Path) -> String {
        fs::read_to_string(
            loader
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("Info.plist"),
        )
        .unwrap()
    }

    fn running_gog_attempt(fixture: &Fixture) -> steam_install::CatalogAttempt {
        let mut attempt = steam_install::CatalogAttempt::new(
            &fixture.bottle.id,
            "gog-galaxy",
            "a".repeat(64),
            "https://gog-cdn-fastly.gog.com/open/galaxy/client/setup_galaxy_2.0.0.0.exe".into(),
        )
        .unwrap();
        attempt.set_stage(steam_install::CatalogStage::Running);
        steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
        attempt
    }

    #[test]
    fn catalog_installer_and_inno_child_have_contextual_setup_names() {
        let fixture = Fixture::new();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        let mut attempt = running_gog_attempt(&fixture);
        let source = app_icons::fixture_pe_with_version("Setup/Uninstall", "Example Product");
        let installer = dir.join("installers/GOGGalaxySetup.exe");
        fs::create_dir_all(installer.parent().unwrap()).unwrap();
        fs::write(&installer, &source).unwrap();
        let loader = prepare_with_inputs(
            &fixture.home,
            &fixture.bottle.id,
            &installer,
            Some(&installer),
            &fixture.inputs,
        )
        .unwrap();
        assert!(app_name(&loader).contains("Publisher launcher Setup"));
        let mut child = PathBuf::new();
        for relative in [
            "prefix/drive_c/users/test/Temp/is-Example.tmp/setup.tmp",
            "prefix/drive_c/users/test/AppData/Local/Temp/is-Example.tmp/setup.tmp",
            "prefix/drive_c/windows/temp/is-Example.tmp/setup.tmp",
        ] {
            child = dir.join(relative);
            fs::create_dir_all(child.parent().unwrap()).unwrap();
            fs::write(&child, &source).unwrap();
            let loader = fixture.prepare(&child).unwrap();
            assert!(app_name(&loader).contains("Publisher launcher Setup"));
        }
        // The saved receipt only labels temporary children during an active
        // catalog attempt. A normally launched program keeps its own product.
        attempt.set_stage(steam_install::CatalogStage::Installed);
        steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
        let loader = fixture.prepare(&child).unwrap();
        assert!(app_name(&loader).contains("Example Product"));
        assert!(!app_name(&loader).contains("Publisher launcher Setup"));
    }

    #[test]
    fn ordinary_program_uses_specific_product_and_saved_name_without_catalog_override() {
        let fixture = Fixture::new();
        running_gog_attempt(&fixture);
        let image = fixture.image("setup.exe");
        fs::write(
            &image,
            app_icons::fixture_pe_with_version("Setup / Uninstall", "Original Example Game"),
        )
        .unwrap();
        let loader = fixture.prepare(&image).unwrap();
        assert!(app_name(&loader).contains("Original Example Game"));
        assert!(!app_name(&loader).contains("Publisher launcher Setup"));
        add_launcher(&fixture.home, &fixture.bottle.id, "Saved Example", &image).unwrap();
        let loader = fixture.prepare(&image).unwrap();
        assert!(app_name(&loader).contains("Saved Example"));
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        for relative in [
            "prefix/drive_c/users/test/Temp/is-other/setup.tmp",
            "prefix/drive_c/users/test/Temp/is-.tmp/setup.tmp",
        ] {
            let unrelated = dir.join(relative);
            fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
            fs::copy(&image, &unrelated).unwrap();
            let loader = fixture.prepare(&unrelated).unwrap();
            assert!(app_name(&loader).contains("Original Example Game"));
            assert!(!app_name(&loader).contains("Publisher launcher Setup"));
        }
    }

    #[test]
    fn malformed_optional_catalog_receipt_does_not_erase_program_identity() {
        let fixture = Fixture::new();
        let image = fixture.image("Example.exe");
        fs::write(
            &image,
            app_icons::fixture_pe_with_version("Setup / Uninstall", "Original Example Game"),
        )
        .unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        fs::write(dir.join("catalog-attempt.json"), b"not a receipt").unwrap();
        let loader = fixture.prepare(&image).unwrap();
        assert!(app_name(&loader).contains("Original Example Game"));
    }

    #[test]
    fn raced_regular_input_becoming_fifo_is_rejected_without_blocking() {
        let fixture = Fixture::new();
        let path = fixture.home.join("raced-native-input");
        fs::write(&path, b"original test regular file").unwrap();
        let before = fs::symlink_metadata(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            Command::new("/usr/bin/mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender
                .send(open_stable_regular_file(&path, &before).is_err())
                .unwrap();
        });
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("Raced native input open must be bounded")
        );
        worker.join().unwrap();
    }

    #[test]
    fn shutdown_alias_requires_exact_bottle_and_package_loader_inodes() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let loader = fixture.prepare(&image).unwrap();
        fs::create_dir_all(fixture.inputs.ntdll.parent().unwrap()).unwrap();
        fs::write(&fixture.inputs.ntdll, b"original test runtime NTDLL").unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        let proof = validate_shutdown_alias_with_inputs(&dir, &loader, &fixture.inputs)
            .unwrap()
            .unwrap();
        assert_eq!(proof.loader_dev, fs::metadata(&loader).unwrap().dev());
        assert_eq!(proof.loader_ino, fs::metadata(&loader).unwrap().ino());
        assert_eq!(
            proof.ntdll_dev,
            fs::metadata(&fixture.inputs.ntdll).unwrap().dev()
        );
        assert_eq!(
            proof.ntdll_ino,
            fs::metadata(&fixture.inputs.ntdll).unwrap().ino()
        );
        assert_eq!(proof.loader_path, loader);
        assert_eq!(proof.ntdll_path, fixture.inputs.ntdll);
        assert!(
            validate_shutdown_alias_with_inputs(
                &dir,
                &fixture.inputs.root.join("Steam"),
                &fixture.inputs
            )
            .unwrap()
            .is_none()
        );
        let another = create(&fixture.home, "Another bottle").unwrap();
        assert!(
            validate_shutdown_alias_with_inputs(
                &bottle_dir(&fixture.home, &another.id).unwrap(),
                &loader,
                &fixture.inputs
            )
            .unwrap()
            .is_none()
        );
        fs::write(&loader, b"unapproved native executable").unwrap();
        assert!(validate_shutdown_alias_with_inputs(&dir, &loader, &fixture.inputs).is_err());
    }

    #[test]
    fn shutdown_alias_refuses_cache_links_and_nested_host_payloads() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let loader = fixture.prepare(&image).unwrap();
        fs::create_dir_all(fixture.inputs.ntdll.parent().unwrap()).unwrap();
        fs::write(&fixture.inputs.ntdll, b"original test runtime NTDLL").unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        let copy = fixture.home.join("copied-host-loader");
        fs::rename(&loader, &copy).unwrap();
        symlink(&copy, &loader).unwrap();
        assert!(validate_shutdown_alias_with_inputs(&dir, &loader, &fixture.inputs).is_err());
        assert!(
            validate_shutdown_alias_with_inputs(
                &dir,
                &dir.join("native-apps/nested/Game-aaaaaaaaaaaaaaaa.app/Contents/MacOS/WindowsApp"),
                &fixture.inputs
            )
            .is_err()
        );
    }

    #[test]
    fn shutdown_alias_accepts_only_exact_legacy_or_keyed_layouts() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let loader = fixture.prepare(&image).unwrap();
        fs::create_dir_all(fixture.inputs.ntdll.parent().unwrap()).unwrap();
        fs::write(&fixture.inputs.ntdll, b"original test runtime NTDLL").unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        let legacy = dir.join("native-apps/Game-0123456789abcdef.app/Contents/MacOS/WindowsApp");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::copy(&loader, &legacy).unwrap();
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            validate_shutdown_alias_with_inputs(&dir, &legacy, &fixture.inputs)
                .unwrap()
                .is_some()
        );
        for relative in [
            "native-apps/Game.app/Contents/MacOS/WindowsApp".to_owned(),
            "native-apps/Game-0123456789abcdeg.app/Contents/MacOS/WindowsApp".to_owned(),
            format!(
                "native-apps/{}/Game.app/Contents/MacOS/WindowsApp",
                "g".repeat(64)
            ),
            format!(
                "native-apps/{}/extra/Game.app/Contents/MacOS/WindowsApp",
                "a".repeat(64)
            ),
            format!(
                "native-apps/{}/.Game.app/Contents/MacOS/WindowsApp",
                "a".repeat(64)
            ),
        ] {
            assert!(
                validate_shutdown_alias_with_inputs(&dir, &dir.join(relative), &fixture.inputs)
                    .is_err()
            );
        }
    }

    #[test]
    fn linked_identity_parent_cannot_supply_an_app_or_shutdown_proof() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let loader = fixture.prepare(&image).unwrap();
        fs::create_dir_all(fixture.inputs.ntdll.parent().unwrap()).unwrap();
        fs::write(&fixture.inputs.ntdll, b"original test runtime NTDLL").unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle.id).unwrap();
        let identity = loader
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let outside = fixture.home.join("copied-identity-parent");
        fs::rename(identity, &outside).unwrap();
        symlink(&outside, identity).unwrap();
        assert!(fixture.prepare(&image).is_err());
        assert!(
            prepare_with_inputs_mode(
                &fixture.home,
                &fixture.bottle.id,
                &image,
                None,
                &fixture.inputs,
                false,
            )
            .is_err()
        );
        assert!(validate_shutdown_alias_with_inputs(&dir, &loader, &fixture.inputs).is_err());
    }

    #[test]
    fn fresh_launch_clears_inherited_identity_even_for_older_runtime() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let wine = fixture.home.join("old-runtime/bin/wine");
        let mut command = Command::new(&wine);
        for name in ENVIRONMENT {
            command.env(name, "stale parent identity");
        }
        configure_launch(
            &mut command,
            &fixture.home,
            &fixture.bottle.id,
            &wine,
            &image,
        )
        .unwrap();
        for name in ENVIRONMENT {
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == *name && value.is_none())
            );
        }
        assert!(
            !bottle_dir(&fixture.home, &fixture.bottle.id)
                .unwrap()
                .join("native-apps")
                .exists()
        );
    }

    #[test]
    fn failed_publisher_renderer_retains_saved_name_and_original_fallback() {
        let (fixture, image) = fallback_fixture(b"#!/bin/sh\nexit 7\n");
        let source = fs::read(&image).unwrap();
        let loader = fixture.prepare(&image).unwrap();
        let contents = loader.parent().unwrap().parent().unwrap();
        let resources = contents.join("Resources");
        assert!(
            fs::read_to_string(contents.join("Info.plist"))
                .unwrap()
                .contains("Artwork Game")
        );
        assert_eq!(
            fs::read(resources.join("native-icon-mode")).unwrap(),
            b"fallback\n"
        );
        assert_eq!(
            fs::read(resources.join("app.png")).unwrap(),
            fixture.inputs.fallback_artwork.as_ref().unwrap().1
        );
        assert_eq!(
            fs::read(resources.join("app-source.png")).unwrap(),
            fixture.inputs.fallback_icon.as_ref().unwrap().as_slice()
        );
        assert!(resources.join("app.ico").is_file());
        assert_eq!(fs::read(&image).unwrap(), source);
        assert_eq!(fixture.prepare(&image).unwrap(), loader);
        fs::write(resources.join("native-icon-mode"), b"publisher\n").unwrap();
        // This mode cannot authorize altered original fallback bytes.
        fs::write(resources.join("app.png"), b"not an image").unwrap();
        assert!(fixture.prepare(&image).is_err());
    }

    #[test]
    fn timed_out_publisher_renderer_is_reaped_before_named_fallback() {
        let (fixture, image) =
            fallback_fixture(b"#!/bin/sh\necho $$ > \"$0.pid\"\nexec /bin/sleep 30\n");
        let started = Instant::now();
        let loader = fixture.prepare(&image).unwrap();
        assert!(started.elapsed() < Duration::from_millis(1800));
        let renderer = fixture.inputs.icon_renderer.as_ref().unwrap();
        let pid: u32 = fs::read_to_string(renderer.with_extension("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(!process_alive(pid).unwrap());
        assert!(loader.is_file());
        let cache = loader
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert_eq!(fs::read_dir(cache).unwrap().count(), 1);
        assert_eq!(fixture.prepare(&image).unwrap(), loader);
    }

    #[test]
    fn cached_compressed_artwork_cannot_claim_unbounded_dimensions() {
        let mut png = vec![0_u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[8..12].copy_from_slice(&13_u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&1024_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1024_u32.to_be_bytes());
        let mut icns = b"icns".to_vec();
        icns.extend_from_slice(&49_u32.to_be_bytes());
        icns.extend_from_slice(b"ic10");
        icns.extend_from_slice(&41_u32.to_be_bytes());
        icns.extend_from_slice(&png);
        assert!(validate_native_artwork(&icns, &png).is_ok());
        png[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(validate_native_artwork(&icns, &png).is_err());
        icns[32..36].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(validate_native_artwork(&icns, &icns[16..]).is_err());
        assert!(validate_native_artwork(b"icns", b"invalid").is_err());
    }

    #[test]
    fn read_only_artwork_lookup_never_creates_cache_or_bundles() {
        let fixture = Fixture::new();
        let image = fixture.image("Game.exe");
        let cache = bottle_dir(&fixture.home, &fixture.bottle.id)
            .unwrap()
            .join("native-apps");
        assert!(
            prepare_with_inputs_mode(
                &fixture.home,
                &fixture.bottle.id,
                &image,
                None,
                &fixture.inputs,
                false
            )
            .is_err()
        );
        assert!(!cache.exists());
        let generated = prepare_with_inputs(
            &fixture.home,
            &fixture.bottle.id,
            &image,
            None,
            &fixture.inputs,
        )
        .unwrap();
        let read = prepare_with_inputs_mode(
            &fixture.home,
            &fixture.bottle.id,
            &image,
            None,
            &fixture.inputs,
            false,
        )
        .unwrap();
        assert_eq!(read, generated);
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
    }

    #[test]
    fn own_bundle_has_saved_name_and_exact_package_loader() {
        let fixture = Fixture::new();
        let image = fixture.image("app.exe");
        add_launcher(
            &fixture.home,
            &fixture.bottle.id,
            "My <App> & Games",
            &image,
        )
        .unwrap();
        let loader = fixture.prepare(&image).unwrap();
        assert_eq!(fs::read(&loader).unwrap(), fixture.inputs.loader);
        let info = fs::read_to_string(
            loader
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("Info.plist"),
        )
        .unwrap();
        assert!(info.contains("My &lt;App&gt; &amp; Games"));
        assert!(info.contains("dev.caskrun.windows."));
        assert_eq!(fixture.prepare(&image).unwrap(), loader);
    }

    #[test]
    fn visible_bundle_names_are_clean_while_same_named_apps_remain_unique() {
        let fixture = Fixture::new();
        let first = fixture.image("FirstSetup.exe");
        let second = fixture.image("SecondSetup.exe");
        for image in [&first, &second] {
            add_launcher(&fixture.home, &fixture.bottle.id, "GOG Galaxy Setup", image).unwrap();
        }
        let first_loader = fixture.prepare(&first).unwrap();
        let second_loader = fixture.prepare(&second).unwrap();
        assert_ne!(first_loader, second_loader);
        let cache = bottle_dir(&fixture.home, &fixture.bottle.id)
            .unwrap()
            .join("native-apps");
        for loader in [&first_loader, &second_loader] {
            let app = loader.parent().unwrap().parent().unwrap().parent().unwrap();
            assert_eq!(app.file_name().unwrap(), "GOG Galaxy Setup.app");
            let identity = app.parent().unwrap();
            assert_eq!(identity.parent(), Some(cache.as_path()));
            assert!(native_key(
                identity.file_name().unwrap().to_str().unwrap(),
                64
            ));
            assert_eq!(fs::read_dir(identity).unwrap().count(), 1);
            let info = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
            assert!(info.contains("<key>CFBundleName</key><string>GOG Galaxy Setup</string>"));
            assert!(
                info.contains("<key>CFBundleDisplayName</key><string>GOG Galaxy Setup</string>")
            );
        }
        assert_eq!(fixture.prepare(&first).unwrap(), first_loader);
        assert_eq!(fixture.prepare(&second).unwrap(), second_loader);
    }

    #[test]
    fn child_game_gets_its_own_name_and_identity() {
        let fixture = Fixture::new();
        let launcher = fixture.image("Store.exe");
        let game = fixture.image("Chess.exe");
        add_launcher(&fixture.home, &fixture.bottle.id, "Storefront", &launcher).unwrap();
        let store_loader = fixture.prepare(&launcher).unwrap();
        let game_loader = fixture.prepare(&game).unwrap();
        assert_ne!(store_loader, game_loader);
        let info = fs::read_to_string(
            game_loader
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("Info.plist"),
        )
        .unwrap();
        assert!(info.contains("<string>Chess</string>"));
        assert!(!info.contains("Storefront"));
    }

    #[test]
    fn callback_does_not_take_the_launchers_bottle_lock() {
        let fixture = Fixture::new();
        let image = fixture.image("Tool.exe");
        let _lock = lock_bottle(&fixture.home, &fixture.bottle.id).unwrap();
        let started = Instant::now();
        assert!(fixture.prepare(&image).is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn unrelated_external_image_is_refused_and_explicit_initial_image_is_allowed() {
        let fixture = Fixture::new();
        let inside = fixture.image("Chosen.exe");
        let external = fixture.home.join("Chosen.exe");
        fs::copy(&inside, &external).unwrap();
        assert!(fixture.prepare(&external).is_err());
        let prepared = prepare_with_inputs(
            &fixture.home,
            &fixture.bottle.id,
            &external,
            Some(&external),
            &fixture.inputs,
        )
        .unwrap();
        assert!(prepared.starts_with(bottle_dir(&fixture.home, &fixture.bottle.id).unwrap()));
    }

    #[test]
    fn corrupt_and_linked_images_are_not_read_as_native_apps() {
        let fixture = Fixture::new();
        let image = fixture.image("Broken.exe");
        fs::write(&image, b"unrelated contents").unwrap();
        assert!(fixture.prepare(&image).is_err());
        fs::remove_file(&image).unwrap();
        let other = fixture.image("Other.exe");
        symlink(other, &image).unwrap();
        assert!(fixture.prepare(&image).is_err());
    }

    #[test]
    fn edited_or_linked_cached_host_loader_is_never_returned() {
        let fixture = Fixture::new();
        let image = fixture.image("App.exe");
        let loader = fixture.prepare(&image).unwrap();
        fs::write(&loader, b"#!/bin/sh\nexit 0\n").unwrap();
        assert!(fixture.prepare(&image).unwrap_err().contains("modified"));
        fs::remove_file(&loader).unwrap();
        let unrelated = fixture.home.join("other-host-program");
        fs::write(&unrelated, &fixture.inputs.loader).unwrap();
        symlink(unrelated, loader).unwrap();
        assert!(fixture.prepare(&image).is_err());
    }

    #[test]
    fn concurrent_helpers_use_one_validated_bundle() {
        let fixture = Fixture::new();
        let image = fixture.image("Launcher.exe");
        let results = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| fixture.prepare(&image)))
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(results.iter().all(|loader| *loader == results[0]));
        let apps = bottle_dir(&fixture.home, &fixture.bottle.id)
            .unwrap()
            .join("native-apps");
        assert_eq!(fs::read_dir(apps).unwrap().count(), 1);
    }

    #[test]
    fn names_are_bounded_escaped_and_cannot_escape_the_cache() {
        assert!(display_name("bad\nname").is_none());
        assert!(display_name(&"x".repeat(121)).is_none());
        assert_eq!(filename_component("../../Apps:Tool\\x"), "Apps Tool x");
        assert_eq!(xml("<&\"'"), "&lt;&amp;&quot;&apos;");
    }
}
