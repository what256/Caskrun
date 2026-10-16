//! Read-only Steam library discovery within a bottle's real C: drive.
//!
//! Manifest identity comes from Steam, never from a guessed game executable.
//! Games launch via Steam's documented `-applaunch <AppID>` interface:
//! https://developer.valvesoftware.com/wiki/Command_line_options#Steam

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::ops::Deref;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::{
    O_DIRECTORY, O_NOFOLLOW_ANY, cancellation, open_stable_directory, same_identity, same_snapshot,
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_READ_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ICON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOKENS: usize = 65_536;
const MAX_DEPTH: usize = 16;
const MAX_LIBRARIES: usize = 64;
const MAX_SCAN_ENTRIES: usize = 32_768;
const MAX_GAMES: usize = 4_096;
const MAX_SHORTCUT_BYTES: u64 = 32 * 1024;
const MAX_SHORTCUT_ENTRIES: usize = 4_096;
const MAX_SHORTCUT_DEPTH: usize = 5;
const FULLY_INSTALLED: u32 = 4;
// Steam keeps the installed bit and the game folder while it removes files.
// An uninstalling game is not launchable and must not stay in the library.
const UNINSTALLING: u32 = 2048;
// Darwin O_NONBLOCK prevents a regular-file-to-FIFO race from waiting in open.
const O_NONBLOCK: i32 = 0x0000_0004;

#[derive(Debug, Serialize)]
pub(crate) struct SteamLibrary {
    pub(crate) games: Vec<SteamGame>,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SteamGame {
    /// Stable within this bottle; moving a game between its libraries keeps it.
    pub(crate) id: String,
    pub(crate) app_id: u32,
    pub(crate) name: String,
    pub(crate) install_path: String,
    pub(crate) steam_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon_path: Option<String>,
    /// Public Steam client-icon digest from an exact same-app shortcut. No
    /// account identity or local path is needed to fetch this icon from Steam.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon_hash: Option<String>,
}

struct CheckedDrive {
    path: PathBuf,
    original_path: PathBuf,
    identity: fs::Metadata,
    _guard: File,
}

impl CheckedDrive {
    fn open(path: &Path) -> Result<Self, String> {
        let (guard, identity) = open_stable_directory(path)?;
        let canonical = path.canonicalize().map_err(|error| error.to_string())?;
        let drive = Self {
            path: canonical,
            original_path: path.to_owned(),
            identity,
            _guard: guard,
        };
        if !drive.is_current() {
            return Err("Bottle C: drive changed while reading its Steam library".into());
        }
        Ok(drive)
    }

    fn is_current(&self) -> bool {
        [&self.original_path, &self.path].into_iter().all(|path| {
            if !fs::symlink_metadata(path)
                .is_ok_and(|metadata| same_identity(&self.identity, &metadata))
            {
                return false;
            }
            // symlink_metadata does not reject symlinks in parent components.
            // Reopening with Darwin's NOFOLLOW_ANY keeps the original prefix
            // path anchored even if an ancestor moves after the first open.
            OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW_ANY | O_DIRECTORY)
                .open(path)
                .and_then(|file| file.metadata())
                .is_ok_and(|metadata| same_identity(&self.identity, &metadata))
        })
    }
}

impl Deref for CheckedDrive {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for CheckedDrive {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

struct ReadBudget {
    remaining: u64,
    exhausted: bool,
}

impl SteamGame {
    pub(crate) fn launch_arguments(&self) -> Vec<String> {
        vec!["-applaunch".into(), self.app_id.to_string()]
    }
}

/// Re-read current manifests before launching, so removed or changed games
/// cannot launch using a stale UI record. This function never starts a process.
pub(crate) fn launch_target(prefix: &Path, app_id: u32) -> Result<SteamGame, String> {
    discover(prefix)?
        .games
        .into_iter()
        .find(|game| game.app_id == app_id)
        .ok_or_else(|| "Steam game is no longer installed in this bottle".into())
}

pub(crate) fn discover(prefix: &Path) -> Result<SteamLibrary, String> {
    discover_with_budget(prefix, MAX_TOTAL_READ_BYTES)
}

/// Checked standard client locations, independent of account data and whether
/// this client has downloaded a game yet. Used to route a typed install URI.
pub(crate) fn client_paths(prefix: &Path) -> Result<Vec<PathBuf>, String> {
    cancellation::check()?;
    let path = prefix.join("drive_c");
    if !path.try_exists().map_err(|error| error.to_string())? {
        return Ok(Vec::new());
    }
    let drive = CheckedDrive::open(&path)?;
    let mut clients = Vec::new();
    for location in ["Program Files (x86)/Steam", "Program Files/Steam"] {
        cancellation::check()?;
        let Some(root) = checked_directory(&drive, &drive.join(location)) else {
            continue;
        };
        for name in ["steam.exe", "Steam.exe"] {
            let path = root.join(name);
            if checked_regular_file(&drive, &path) {
                let canonical = path.canonicalize().map_err(|error| error.to_string())?;
                if drive.is_current()
                    && canonical.starts_with(&drive.path)
                    && !clients.contains(&canonical)
                {
                    clients.push(canonical);
                }
            }
        }
    }
    cancellation::check()?;
    if !drive.is_current() {
        return Err("Bottle C: drive changed while finding Steam".into());
    }
    Ok(clients)
}

fn discover_with_budget(prefix: &Path, max_read_bytes: u64) -> Result<SteamLibrary, String> {
    cancellation::check()?;
    let mut result = SteamLibrary {
        games: Vec::new(),
        truncated: false,
    };
    let drive = prefix.join("drive_c");
    match fs::symlink_metadata(&drive) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Cannot inspect the bottle's C: drive: {error}")),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err("Bottle C: drive must be a real directory".into());
        }
        Ok(_) => {}
    }
    let drive = CheckedDrive::open(&drive)?;
    let mut read_budget = ReadBudget {
        remaining: max_read_bytes,
        exhausted: false,
    };
    let mut games = BTreeMap::new();
    let mut examined = 0usize;
    let mut scanned = BTreeSet::new();

    for location in ["Program Files (x86)/Steam", "Program Files/Steam"] {
        cancellation::check()?;
        let Some(steam_root) = checked_directory(&drive, &drive.join(location)) else {
            continue;
        };
        let Some(steam_path) = ["steam.exe", "Steam.exe"].into_iter().find_map(|name| {
            let path = steam_root.join(name);
            if !checked_regular_file(&drive, &path) {
                return None;
            }
            let canonical = path.canonicalize().ok()?;
            (drive.is_current() && canonical.starts_with(&drive.path)).then_some(canonical)
        }) else {
            continue;
        };
        let mut libraries = BTreeSet::from([steam_root.clone()]);
        // Steam has used both locations, and both flat and nested entries.
        for relative in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            cancellation::check()?;
            let path = steam_root.join(relative);
            if let Some(text) = read_checked_text(&drive, &path, &mut read_budget) {
                let Some(document) = parse_key_values(&text) else {
                    continue;
                };
                let Some(folders) = unique_object(&document, "libraryfolders") else {
                    continue;
                };
                for (key, value) in folders {
                    cancellation::check()?;
                    if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_digit()) {
                        continue;
                    }
                    let path = match value {
                        Value::Text(path) => Some(path.as_str()),
                        Value::Object(fields) => unique_text(fields, "path"),
                    };
                    let Some(path) = path.and_then(|path| bottle_library_path(&drive, path)) else {
                        continue;
                    };
                    if libraries.len() >= MAX_LIBRARIES && !libraries.contains(&path) {
                        result.truncated = true;
                        break;
                    }
                    libraries.insert(path);
                }
            }
        }
        // Scan the primary library first so duplicate manifests have a stable
        // winner, followed by the configured bottle-local libraries in order.
        let libraries = std::iter::once(steam_root.clone())
            .chain(libraries.into_iter().filter(|path| path != &steam_root));
        for library in libraries {
            cancellation::check()?;
            if !scanned.insert(library.clone()) {
                continue;
            }
            scan_library(
                &drive,
                &library,
                &steam_path,
                &mut games,
                &mut examined,
                &mut result.truncated,
                &mut read_budget,
            );
            cancellation::check()?;
            if examined >= MAX_SCAN_ENTRIES || games.len() >= MAX_GAMES || read_budget.exhausted {
                break;
            }
        }
    }
    if !games.is_empty() && !read_budget.exhausted {
        let icons = shortcut_icon_associations(&drive, &mut read_budget, &mut result.truncated);
        for game in games.values_mut() {
            cancellation::check()?;
            let Some(steam_root) = Path::new(&game.steam_path).parent() else {
                continue;
            };
            if let Some(Some(hash)) = icons.get(&(steam_root.to_owned(), game.app_id)) {
                game.icon_hash = Some(hash.clone());
                if game.icon_path.is_none() {
                    game.icon_path = checked_icon_path(
                        &drive,
                        &steam_root.join("steam/games").join(format!("{hash}.ico")),
                    );
                }
            }
        }
    }
    cancellation::check()?;
    if !drive.is_current() {
        return Err("Bottle C: drive changed while reading its Steam library".into());
    }
    result.truncated |= read_budget.exhausted;
    result.games = games.into_values().collect();
    result.games.sort_by(|first, second| {
        first
            .name
            .to_lowercase()
            .cmp(&second.name.to_lowercase())
            .then(first.app_id.cmp(&second.app_id))
    });
    Ok(result)
}

fn scan_library(
    drive: &CheckedDrive,
    library: &Path,
    steam_path: &Path,
    games: &mut BTreeMap<u32, SteamGame>,
    examined: &mut usize,
    truncated: &mut bool,
    read_budget: &mut ReadBudget,
) {
    let Some(steamapps) = checked_directory(drive, &library.join("steamapps")) else {
        return;
    };
    let Ok((_guard, _)) = open_stable_directory(&steamapps) else {
        *truncated = true;
        return;
    };
    let entries = match fs::read_dir(&steamapps) {
        Ok(entries) => entries,
        Err(_) => {
            *truncated = true;
            return;
        }
    };
    let mut manifests = Vec::new();
    for entry in entries {
        if cancellation::is_requested() {
            return;
        }
        if *examined >= MAX_SCAN_ENTRIES {
            *truncated = true;
            break;
        }
        *examined += 1;
        match entry {
            Ok(entry) => {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if let Some(app_id) = name
                    .strip_prefix("appmanifest_")
                    .and_then(|name| name.strip_suffix(".acf"))
                    .and_then(parse_app_id)
                {
                    manifests.push((app_id, entry.path()));
                }
            }
            Err(_) => *truncated = true,
        }
    }
    manifests.sort();
    for (expected_app_id, path) in manifests {
        if cancellation::is_requested() {
            return;
        }
        if games.contains_key(&expected_app_id) {
            continue;
        }
        if games.len() >= MAX_GAMES {
            *truncated = true;
            break;
        }
        let Some(text) = read_checked_text(drive, &path, read_budget) else {
            if read_budget.exhausted {
                *truncated = true;
                break;
            }
            continue;
        };
        let Some(document) = parse_key_values(&text) else {
            continue;
        };
        let Some(state) = unique_object(&document, "AppState") else {
            continue;
        };
        let Some(app_id) = unique_text(state, "appid").and_then(parse_app_id) else {
            continue;
        };
        if app_id != expected_app_id {
            continue;
        }
        let Some(flags) =
            unique_text(state, "StateFlags").and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if flags & FULLY_INSTALLED == 0 || flags & UNINSTALLING != 0 {
            continue;
        }
        let Some(name) = unique_text(state, "name").map(str::trim) else {
            continue;
        };
        if name.is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
            continue;
        }
        let Some(install_dir) = unique_text(state, "installdir") else {
            continue;
        };
        if !safe_windows_component(install_dir) {
            continue;
        }
        let Some(install_path) =
            checked_directory(drive, &steamapps.join("common").join(install_dir))
        else {
            continue;
        };
        games.insert(
            app_id,
            SteamGame {
                id: format!("steam:{app_id}"),
                app_id,
                name: name.to_owned(),
                install_path: install_path.display().to_string(),
                steam_path: steam_path.display().to_string(),
                icon_path: cached_icon_path(drive, steam_path, app_id),
                icon_hash: None,
            },
        );
    }
}

fn parse_app_id(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = value.parse::<u32>().ok()?;
    (number != 0 && number.to_string() == value).then_some(number)
}

fn safe_windows_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && !value.ends_with(['.', ' '])
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
                )
        })
}

/// Resolve only absolute Windows C: paths. Wine's Z: and dosdevices mappings
/// can expose arbitrary host files and intentionally never participate here.
fn bottle_library_path(drive: &CheckedDrive, windows_path: &str) -> Option<PathBuf> {
    let bytes = windows_path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].eq_ignore_ascii_case(&b'c')
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || windows_path.len() > 4_096
    {
        return None;
    }
    let mut path = drive.path.clone();
    let mut depth = 0;
    for component in windows_path[3..]
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
    {
        depth += 1;
        if depth > 64 || !safe_windows_component(component) {
            return None;
        }
        path.push(component);
    }
    checked_directory(drive, &path)
}

fn checked_directory(drive: &CheckedDrive, path: &Path) -> Option<PathBuf> {
    if !drive.is_current() {
        return None;
    }
    let relative = path.strip_prefix(drive).ok()?;
    let mut current = drive.path.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        current.push(name);
        if !fs::symlink_metadata(&current).ok()?.file_type().is_dir() {
            return None;
        }
    }
    let (_guard, _) = open_stable_directory(&current).ok()?;
    let resolved = current.canonicalize().ok()?;
    (drive.is_current() && resolved.starts_with(drive)).then_some(resolved)
}

fn checked_regular_file(drive: &CheckedDrive, path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if checked_directory(drive, parent).is_none() {
        return false;
    }
    let Ok(before) = fs::symlink_metadata(path) else {
        return false;
    };
    if !before.file_type().is_file() {
        return false;
    }
    let Some(file) = open_regular_library_file(path, &before) else {
        return false;
    };
    drive.is_current()
        && file
            .metadata()
            .is_ok_and(|metadata| same_snapshot(&before, &metadata))
}

fn open_regular_library_file(path: &Path, before: &fs::Metadata) -> Option<File> {
    if !before.is_file() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | O_NONBLOCK)
        .open(path)
        .ok()?;
    let opened = file.metadata().ok()?;
    (opened.is_file() && same_snapshot(before, &opened)).then_some(file)
}

fn cached_icon_path(drive: &CheckedDrive, steam_path: &Path, app_id: u32) -> Option<String> {
    let cache = steam_path.parent()?.join("appcache/librarycache");
    // Collection uses original app icons, never cropped store covers or
    // library wordmarks. Support Steam's modern and legacy exact icon names.
    let icon_names = [
        "clienticon.ico",
        "icon.ico",
        "icon.png",
        "icon.jpg",
        "clienticon.png",
    ];
    let app_cache = cache.join(app_id.to_string());
    icon_names
        .into_iter()
        .map(|name| app_cache.join(name))
        .chain(
            icon_names
                .into_iter()
                .map(|name| cache.join(format!("{app_id}_{name}"))),
        )
        .find_map(|path| checked_icon_path(drive, &path))
}

fn checked_icon_path(drive: &CheckedDrive, path: &Path) -> Option<String> {
    (checked_regular_file(drive, path)
        && fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.len() > 0 && metadata.len() <= MAX_ICON_BYTES))
    .then(|| path.display().to_string())
}

/// Steam records the original shortcut icon beside its documented AppID URI.
/// Read those associations once, from bounded bottle-local standard shortcut
/// locations. Never follow host Desktop mappings or arbitrary IconFile paths.
fn shortcut_icon_associations(
    drive: &CheckedDrive,
    budget: &mut ReadBudget,
    truncated: &mut bool,
) -> BTreeMap<(PathBuf, u32), Option<String>> {
    let mut icons = BTreeMap::new();
    let Some(users) = checked_directory(drive, &drive.join("users")) else {
        return icons;
    };
    let mut examined = 0;
    let mut pending = vec![(users, 0usize, false)];
    while let Some((folder, depth, shortcut_folder)) = pending.pop() {
        if cancellation::is_requested() || budget.exhausted {
            break;
        }
        let Some(folder) = checked_directory(drive, &folder) else {
            continue;
        };
        let Ok((_guard, _)) = open_stable_directory(&folder) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut children = Vec::new();
        for entry in entries {
            if examined >= MAX_SHORTCUT_ENTRIES {
                *truncated = true;
                return icons;
            }
            examined += 1;
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !shortcut_folder {
                if metadata.file_type().is_dir() {
                    // Only real account directories. Existing profile aliases
                    // are skipped, so each account is examined once.
                    children.push((path.join("Desktop"), 0, true));
                    children.push((
                        path.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs"),
                        0,
                        true,
                    ));
                }
            } else if metadata.file_type().is_dir() && depth < MAX_SHORTCUT_DEPTH {
                children.push((path, depth + 1, true));
            } else if metadata.file_type().is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("url"))
                && let Some(bytes) = read_checked_bytes(drive, &path, budget, MAX_SHORTCUT_BYTES)
                && let Some(text) = shortcut_text(bytes)
                && let Some((root, app_id, hash)) = shortcut_icon_association(drive, &text)
            {
                icons
                    .entry((root, app_id))
                    .and_modify(|existing| {
                        if existing.as_deref() != Some(hash.as_str()) {
                            *existing = None;
                        }
                    })
                    .or_insert(Some(hash));
            }
        }
        // Deterministic bounded scan independent of directory enumeration order.
        children.sort();
        pending.extend(children.into_iter().rev());
    }
    icons
}

fn shortcut_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if !bytes.len().is_multiple_of(2) {
            return None;
        }
        let little = bytes[0] == 0xff;
        let chars = bytes[2..]
            .chunks_exact(2)
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&chars).ok()
    } else {
        String::from_utf8(bytes).ok()
    }
}

fn shortcut_icon_association(drive: &CheckedDrive, text: &str) -> Option<(PathBuf, u32, String)> {
    let mut in_shortcut = false;
    let mut fields = BTreeMap::new();
    for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if number >= 256 || line.len() > 4_096 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') {
            in_shortcut = line.eq_ignore_ascii_case("[InternetShortcut]");
            continue;
        }
        if !in_shortcut {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim().to_ascii_lowercase();
        if matches!(key.as_str(), "url" | "iconfile" | "iconindex")
            && fields.insert(key, value.trim()).is_some()
        {
            return None;
        }
    }
    let app_id = fields
        .get("url")?
        .strip_prefix("steam://rungameid/")
        .and_then(parse_app_id)?;
    if fields.get("iconindex").is_some_and(|index| *index != "0") {
        return None;
    }
    let icon_file = fields.get("iconfile")?;
    let icon_file = icon_file
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(icon_file);
    let bytes = icon_file.as_bytes();
    if bytes.len() < 3
        || !bytes[0].eq_ignore_ascii_case(&b'c')
        || bytes[1] != b':'
        || !matches!(bytes[2], b'/' | b'\\')
    {
        return None;
    }
    let components = icon_file[3..].split(['/', '\\']).collect::<Vec<_>>();
    if components.len() != 5
        || !components.iter().all(|part| safe_windows_component(part))
        || !matches!(
            components[0].to_ascii_lowercase().as_str(),
            "program files" | "program files (x86)"
        )
        || !components[1].eq_ignore_ascii_case("Steam")
        || !components[2].eq_ignore_ascii_case("steam")
        || !components[3].eq_ignore_ascii_case("games")
    {
        return None;
    }
    let filename = components[4].to_ascii_lowercase();
    let hash = filename.strip_suffix(".ico")?;
    if hash.len() != 40 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let location = if components[0].eq_ignore_ascii_case("Program Files (x86)") {
        "Program Files (x86)/Steam"
    } else {
        "Program Files/Steam"
    };
    let root = checked_directory(drive, &drive.join(location))?;
    // A publisher hash remains usable if the local ICO is missing, but its
    // directory must still be the real Steam client directory in this bottle.
    checked_directory(drive, &root.join("steam/games"))?;
    Some((root, app_id, hash.to_owned()))
}

fn read_checked_text(drive: &CheckedDrive, path: &Path, budget: &mut ReadBudget) -> Option<String> {
    String::from_utf8(read_checked_bytes(drive, path, budget, MAX_FILE_BYTES)?).ok()
}

fn read_checked_bytes(
    drive: &CheckedDrive,
    path: &Path,
    budget: &mut ReadBudget,
    maximum: u64,
) -> Option<Vec<u8>> {
    if budget.exhausted || cancellation::is_requested() {
        return None;
    }
    checked_directory(drive, path.parent()?)?;
    let before = fs::symlink_metadata(path).ok()?;
    if !before.file_type().is_file() || before.len() > maximum {
        return None;
    }
    if before.len() > budget.remaining {
        budget.exhausted = true;
        return None;
    }
    let mut file = open_regular_library_file(path, &before)?;
    let mut bytes = Vec::new();
    let limit = maximum.min(budget.remaining);
    // Charge actual bytes even if a read fails part way through. The shared
    // copier observes cancellation between chunks and immediately before EOF.
    let read_result = crate::copy_cancellable(&mut (&mut file).take(limit + 1), &mut bytes);
    if bytes.len() as u64 > budget.remaining {
        budget.exhausted = true;
    }
    budget.remaining = budget.remaining.saturating_sub(bytes.len() as u64);
    if read_result.is_err()
        || cancellation::is_requested()
        || bytes.len() as u64 > limit
        || !drive.is_current()
        || !same_snapshot(&before, &file.metadata().ok()?)
        || !same_snapshot(&before, &fs::symlink_metadata(path).ok()?)
    {
        return None;
    }
    Some(bytes)
}

#[derive(Debug)]
enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}

#[derive(Debug)]
enum Token {
    Text(String),
    Open,
    Close,
}

fn unique_value<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    let mut matches = fields
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(key));
    let (_, value) = matches.next()?;
    matches.next().is_none().then_some(value)
}

fn unique_text<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    match unique_value(fields, key)? {
        Value::Text(value) => Some(value),
        Value::Object(_) => None,
    }
}

fn unique_object<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a [(String, Value)]> {
    match unique_value(fields, key)? {
        Value::Object(value) => Some(value),
        Value::Text(_) => None,
    }
}

/// A bounded parser for the text KeyValues subset written by Steam. Keeping
/// pairs preserves duplicate fields so ambiguous identity/path keys are rejected.
fn parse_key_values(text: &str) -> Option<Vec<(String, Value)>> {
    let tokens = tokenize(text.strip_prefix('\u{feff}').unwrap_or(text))?;
    let mut position = 0;
    parse_fields(&tokens, &mut position, 0, false)
}

fn parse_fields(
    tokens: &[Token],
    position: &mut usize,
    depth: usize,
    nested: bool,
) -> Option<Vec<(String, Value)>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut fields = Vec::new();
    loop {
        let key = match tokens.get(*position) {
            None if !nested => return Some(fields),
            Some(Token::Close) if nested => {
                *position += 1;
                return Some(fields);
            }
            Some(Token::Text(key)) => key.clone(),
            _ => return None,
        };
        *position += 1;
        let value = match tokens.get(*position) {
            Some(Token::Text(value)) => {
                *position += 1;
                Value::Text(value.clone())
            }
            Some(Token::Open) => {
                *position += 1;
                Value::Object(parse_fields(tokens, position, depth + 1, true)?)
            }
            _ => return None,
        };
        fields.push((key, value));
    }
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let mut position = 0;
    let mut tokens = Vec::new();
    while position < bytes.len() {
        let byte = bytes[position];
        if byte.is_ascii_whitespace() {
            position += 1;
            continue;
        }
        if byte == b'/' && bytes.get(position + 1) == Some(&b'/') {
            while position < bytes.len() && bytes[position] != b'\n' {
                position += 1;
            }
            continue;
        }
        if tokens.len() >= MAX_TOKENS {
            return None;
        }
        if byte == b'{' || byte == b'}' {
            tokens.push(if byte == b'{' {
                Token::Open
            } else {
                Token::Close
            });
            position += 1;
            continue;
        }
        let mut value = Vec::new();
        if byte == b'"' {
            position += 1;
            loop {
                let byte = *bytes.get(position)?;
                position += 1;
                match byte {
                    b'"' => break,
                    b'\\' => {
                        let escaped = *bytes.get(position)?;
                        position += 1;
                        match escaped {
                            b'"' | b'\\' => value.push(escaped),
                            _ => {
                                value.push(b'\\');
                                value.push(escaped);
                            }
                        }
                    }
                    _ => value.push(byte),
                }
            }
        } else {
            while position < bytes.len()
                && !bytes[position].is_ascii_whitespace()
                && !matches!(bytes[position], b'{' | b'}')
            {
                if bytes[position] == b'"' {
                    return None;
                }
                value.push(bytes[position]);
                position += 1;
            }
        }
        tokens.push(Token::Text(String::from_utf8(value).ok()?));
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, symlink};
    use uuid::Uuid;

    struct Fixture {
        root: PathBuf,
        prefix: PathBuf,
        steam: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            // Keep generated fixtures within the repository, as AGENTS.md requires.
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/steam-library-tests")
                .join(Uuid::new_v4().to_string());
            let prefix = root.join("prefix");
            let steam = prefix.join("drive_c/Program Files (x86)/Steam");
            fs::create_dir_all(steam.join("steamapps/common")).unwrap();
            fs::write(steam.join("Steam.exe"), b"fake Steam path only").unwrap();
            Self {
                root,
                prefix,
                steam,
            }
        }

        fn manifest(
            &self,
            library: &Path,
            filename_id: u32,
            app_id: u32,
            name: &str,
            folder: &str,
            flags: u32,
        ) {
            fs::create_dir_all(library.join("steamapps")).unwrap();
            fs::write(
                library.join(format!("steamapps/appmanifest_{filename_id}.acf")),
                format!(
                    "\"AppState\"\n{{\n\"appid\" \"{app_id}\"\n\"name\" \"{name}\"\n\"installdir\" \"{folder}\"\n\"StateFlags\" \"{flags}\"\n\"UserConfig\" {{ \"language\" \"english\" }}\n}}"
                ),
            )
            .unwrap();
        }

        fn game(&self, app_id: u32, name: &str, folder: &str) {
            fs::create_dir_all(self.steam.join("steamapps/common").join(folder)).unwrap();
            self.manifest(&self.steam, app_id, app_id, name, folder, 4);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn detects_manifest_games_without_guessing_executables_or_writing_launchers() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        fixture.game(620, "Portal 2", "Portal 2");
        let library = discover(&fixture.prefix).unwrap();
        assert!(!library.truncated);
        assert_eq!(
            library
                .games
                .iter()
                .map(|game| game.name.as_str())
                .collect::<Vec<_>>(),
            ["Portal 2", "Spacewar"]
        );
        let game = launch_target(&fixture.prefix, 480).unwrap();
        assert_eq!(game.id, "steam:480");
        assert_eq!(game.launch_arguments(), ["-applaunch", "480"]);
        assert_eq!(
            game.steam_path,
            fixture.steam.join("Steam.exe").display().to_string()
        );
        assert!(!fixture.root.join("launchers.json").exists());
        assert!(!fixture.prefix.join("launchers.json").exists());
        fs::remove_dir_all(fixture.steam.join("steamapps/common/Spacewar")).unwrap();
        assert!(launch_target(&fixture.prefix, 480).is_err());
    }

    #[test]
    fn reads_modern_and_legacy_libraries_only_inside_real_c_drive() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let second = fixture.prefix.join("drive_c/Games/SteamLibrary");
        fs::create_dir_all(second.join("steamapps/common/Portal 2")).unwrap();
        fixture.manifest(&second, 620, 620, "Portal 2", "Portal 2", 6);
        let legacy = fixture.prefix.join("drive_c/Legacy Library");
        fs::create_dir_all(legacy.join("steamapps/common/Half-Life")).unwrap();
        fixture.manifest(&legacy, 70, 70, "Half-Life", "Half-Life", 4);
        fs::write(
            fixture.steam.join("steamapps/libraryfolders.vdf"),
            r#"
            // Steam writes escaped Windows paths.
            "libraryfolders" {
                "0" { "path" "C:\\Program Files (x86)\\Steam" "apps" { "480" "1" } }
                "1" { "path" "C:\\Games\\SteamLibrary" }
                "2" "C:\\Legacy Library"
                "3" { "path" "Z:\\host-library" }
                "4" { "path" "C:\\..\\host-library" }
                "5" { "path" "/host-library" }
                "6" { "path" "C:\\linked-library" }
            }
        "#,
        )
        .unwrap();
        let external = fixture.root.join("host-library");
        fs::create_dir_all(external.join("steamapps/common/External")).unwrap();
        fixture.manifest(&external, 999, 999, "External", "External", 4);
        symlink(&external, fixture.prefix.join("drive_c/linked-library")).unwrap();
        let games = discover(&fixture.prefix).unwrap().games;
        assert_eq!(
            games.iter().map(|game| game.app_id).collect::<Vec<_>>(),
            [70, 620, 480]
        );
        assert!(
            games
                .iter()
                .all(|game| Path::new(&game.install_path)
                    .starts_with(fixture.prefix.join("drive_c")))
        );
    }

    #[test]
    fn ignores_incomplete_malformed_mismatched_and_unsafe_manifests() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let common = fixture.steam.join("steamapps/common");
        fs::create_dir_all(common.join("Incomplete")).unwrap();
        fixture.manifest(&fixture.steam, 1, 1, "Incomplete", "Incomplete", 1024);
        fixture.manifest(&fixture.steam, 2, 22, "Wrong ID", "Spacewar", 4);
        fixture.manifest(&fixture.steam, 3, 3, "Missing folder", "Missing", 4);
        fixture.manifest(&fixture.steam, 4, 4, "Traversal", "..", 4);
        fixture.manifest(&fixture.steam, 5, 5, "Traversal", "../Spacewar", 4);
        fixture.manifest(&fixture.steam, 6, 6, "Windows traversal", r"..\Spacewar", 4);
        fixture.manifest(&fixture.steam, 7, 7, "", "Spacewar", 4);
        fs::create_dir_all(common.join("Leaving")).unwrap();
        fixture.manifest(&fixture.steam, 11, 11, "Uninstalling", "Leaving", 4 | 2048);
        fs::write(
            fixture.steam.join("steamapps/appmanifest_8.acf"),
            "\"AppState\" { \"appid\" \"8\"",
        )
        .unwrap();
        fs::write(fixture.steam.join("steamapps/appmanifest_9.acf"), "\"AppState\" { \"appid\" \"9\" \"appid\" \"9\" \"name\" \"Duplicate\" \"installdir\" \"Spacewar\" \"StateFlags\" \"4\" }").unwrap();
        fs::write(
            fixture.steam.join("steamapps/appmanifest_10.acf"),
            vec![b'x'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(discover(&fixture.prefix).unwrap().games.len(), 1);
    }

    #[test]
    fn rejects_symlinked_manifest_game_directory_steam_exe_and_c_drive() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let external = fixture.root.join("external");
        fs::create_dir_all(&external).unwrap();
        symlink(
            fixture.steam.join("steamapps/appmanifest_480.acf"),
            fixture.steam.join("steamapps/appmanifest_620.acf"),
        )
        .unwrap();
        fixture.manifest(&fixture.steam, 70, 70, "Linked game", "Linked game", 4);
        symlink(
            &external,
            fixture.steam.join("steamapps/common/Linked game"),
        )
        .unwrap();
        assert_eq!(discover(&fixture.prefix).unwrap().games.len(), 1);
        fs::remove_file(fixture.steam.join("Steam.exe")).unwrap();
        fs::write(external.join("Steam.exe"), b"fake host Steam").unwrap();
        symlink(external.join("Steam.exe"), fixture.steam.join("Steam.exe")).unwrap();
        assert!(discover(&fixture.prefix).unwrap().games.is_empty());
        let other_prefix = fixture.root.join("other-prefix");
        fs::create_dir_all(&other_prefix).unwrap();
        symlink(fixture.prefix.join("drive_c"), other_prefix.join("drive_c")).unwrap();
        assert!(discover(&other_prefix).is_err());
    }

    #[test]
    fn identity_survives_library_move_and_duplicate_manifest_is_one_game() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let first = discover(&fixture.prefix).unwrap().games.pop().unwrap();
        let library = fixture.prefix.join("drive_c/SteamLibrary");
        fs::create_dir_all(library.join("steamapps/common/Spacewar")).unwrap();
        fixture.manifest(&library, 480, 480, "Spacewar", "Spacewar", 4);
        fs::write(
            fixture.steam.join("steamapps/libraryfolders.vdf"),
            r#""libraryfolders" { "1" { "path" "C:\\SteamLibrary" } }"#,
        )
        .unwrap();
        let duplicate = discover(&fixture.prefix).unwrap();
        assert_eq!(duplicate.games.len(), 1);
        assert_eq!(duplicate.games[0].install_path, first.install_path);
        fs::remove_file(fixture.steam.join("steamapps/appmanifest_480.acf")).unwrap();
        let moved = discover(&fixture.prefix).unwrap().games.pop().unwrap();
        assert_eq!(moved.id, first.id);
        assert_ne!(moved.install_path, first.install_path);
    }

    #[test]
    fn aggregate_manifest_budget_marks_discovery_truncated() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        fixture.game(620, "Portal 2", "Portal 2");
        let first_size = fs::metadata(fixture.steam.join("steamapps/appmanifest_480.acf"))
            .unwrap()
            .len();
        let second_size = fs::metadata(fixture.steam.join("steamapps/appmanifest_620.acf"))
            .unwrap()
            .len();
        let result = discover_with_budget(&fixture.prefix, first_size + second_size - 1).unwrap();
        assert!(result.truncated);
        assert_eq!(result.games.len(), 1);
        assert_eq!(result.games[0].app_id, 480);
    }

    #[test]
    fn cancellation_refuses_discovery_and_client_lookup_without_storage() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let (request, _guard) = cancellation::test_request();
        request.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(discover(&fixture.prefix).unwrap_err(), cancellation::ERROR);
        assert_eq!(
            client_paths(&fixture.prefix).unwrap_err(),
            cancellation::ERROR
        );
        assert_eq!(
            discover(&fixture.root.join("missing")).unwrap_err(),
            cancellation::ERROR
        );
        assert!(!fixture.root.join("missing").exists());
        assert!(!fixture.prefix.join("launchers.json").exists());
    }

    #[test]
    fn cancellation_aborts_manifest_reader_without_spending_its_budget() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let drive = CheckedDrive::open(&fixture.prefix.join("drive_c")).unwrap();
        let mut budget = ReadBudget {
            remaining: 4096,
            exhausted: false,
        };
        let (request, _guard) = cancellation::test_request();
        request.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            read_checked_text(
                &drive,
                &fixture.steam.join("steamapps/appmanifest_480.acf"),
                &mut budget
            )
            .is_none()
        );
        assert_eq!(budget.remaining, 4096);
        assert!(!budget.exhausted);
    }

    #[test]
    fn a_regular_file_replaced_with_a_fifo_never_blocks_the_checked_open() {
        let fixture = Fixture::new();
        let path = fixture.steam.join("steamapps/appmanifest_480.acf");
        fs::write(&path, b"original manifest").unwrap();
        let before = fs::symlink_metadata(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            std::process::Command::new("/usr/bin/mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
        // Deterministically places the FIFO in the metadata-to-open race window.
        // A worker lets a regression fail within a bound instead of hanging CI.
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            send.send(open_regular_library_file(&path, &before).is_none())
                .unwrap();
        });
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("checked open blocked on a FIFO without a writer")
        );
        worker.join().unwrap();
    }

    #[test]
    fn opened_drive_rejects_replacement_directory_and_outside_symlink() {
        let fixture = Fixture::new();
        let path = fixture.prefix.join("drive_c");
        let drive = CheckedDrive::open(&path).unwrap();
        fs::rename(&path, fixture.prefix.join("original_drive_c")).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(!drive.is_current());
        assert!(checked_directory(&drive, &path).is_none());
        fs::remove_dir(&path).unwrap();
        symlink(fixture.root.join("outside"), &path).unwrap();
        assert!(!drive.is_current());
        assert!(checked_directory(&drive, &path).is_none());
    }

    #[test]
    fn opened_drive_rejects_symlinked_prefix_ancestor() {
        let fixture = Fixture::new();
        let path = fixture.prefix.join("drive_c");
        let drive = CheckedDrive::open(&path).unwrap();
        let moved = fixture.root.join("moved-prefix");
        fs::rename(&fixture.prefix, &moved).unwrap();
        symlink(&moved, &fixture.prefix).unwrap();
        assert!(!drive.is_current());
        assert!(checked_directory(&drive, &path).is_none());
    }

    #[test]
    fn uses_only_exact_local_cached_icon_and_rejects_symlink_or_oversized_artwork() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        assert!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .is_none()
        );
        let cache = fixture.steam.join("appcache/librarycache");
        fs::create_dir_all(&cache).unwrap();
        let outside = fixture.root.join("outside-icon.png");
        fs::write(&outside, b"synthetic icon fixture").unwrap();
        fs::write(cache.join("620_icon.png"), b"another game's icon").unwrap();
        symlink(&outside, cache.join("480_icon.jpg")).unwrap();
        let large = File::create(cache.join("480_icon.png")).unwrap();
        large.set_len(MAX_ICON_BYTES + 1).unwrap();
        let modern = cache.join("480");
        fs::create_dir(&modern).unwrap();
        let icon = modern.join("icon.ico");
        fs::write(&icon, b"synthetic local icon fixture").unwrap();
        assert_eq!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .as_deref(),
            Some(icon.to_str().unwrap())
        );
        fs::remove_file(&icon).unwrap();
        assert!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .is_none()
        );
    }

    #[test]
    fn prefers_real_modern_and_legacy_icons_without_using_covers_or_outside_links() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let cache = fixture.steam.join("appcache/librarycache");
        let modern = cache.join("480");
        fs::create_dir_all(&modern).unwrap();
        let icon = cache.join("480_icon.png");
        fs::write(&icon, b"synthetic icon fixture").unwrap();
        let legacy = cache.join("480_header.jpg");
        fs::write(&legacy, b"synthetic legacy header fixture").unwrap();
        fs::write(
            modern.join("header.jpg"),
            b"synthetic modern header fixture",
        )
        .unwrap();
        let modern_icon = modern.join("icon.ico");
        fs::write(&modern_icon, b"synthetic modern icon fixture").unwrap();
        assert_eq!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .as_deref(),
            Some(modern_icon.to_str().unwrap())
        );
        fs::remove_file(&modern_icon).unwrap();
        assert_eq!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .as_deref(),
            Some(icon.to_str().unwrap())
        );
        let outside = fixture.root.join("outside-artwork");
        fs::create_dir(&outside).unwrap();
        fs::write(
            outside.join("header.jpg"),
            b"outside artwork must not be used",
        )
        .unwrap();
        fs::remove_dir_all(&modern).unwrap();
        symlink(&outside, &modern).unwrap();
        fs::remove_file(&legacy).unwrap();
        assert_eq!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .as_deref(),
            Some(icon.to_str().unwrap())
        );
        fs::remove_file(&icon).unwrap();
        fs::write(cache.join("480_logo.png"), b"wide library wordmark").unwrap();
        assert!(
            discover(&fixture.prefix).unwrap().games[0]
                .icon_path
                .is_none()
        );
    }

    fn shortcut_fixture(fixture: &Fixture, account: &str, app_id: u32, hash: &str) -> PathBuf {
        let desktop = fixture
            .prefix
            .join("drive_c/users")
            .join(account)
            .join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        fs::create_dir_all(fixture.steam.join("steam/games")).unwrap();
        let shortcut = desktop.join(format!("{app_id}.url"));
        fs::write(&shortcut, format!("[InternetShortcut]\r\nURL=steam://rungameid/{app_id}\r\n IconFile = C:\\Program Files (x86)\\Steam\\steam\\games\\{hash}.ico\r\nIconIndex=0\r\n")).unwrap();
        shortcut
    }

    #[test]
    fn resolves_exact_steam_shortcut_icons_and_preserves_public_hash_when_file_missing() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        fixture.game(620, "Portal 2", "Portal 2");
        let hash = "a".repeat(40);
        shortcut_fixture(&fixture, "example", 480, &hash);
        let icon = fixture
            .steam
            .join("steam/games")
            .join(format!("{hash}.ico"));
        fs::write(&icon, b"synthetic publisher ICO").unwrap();
        let games = discover(&fixture.prefix).unwrap().games;
        let game = games.iter().find(|game| game.app_id == 480).unwrap();
        assert_eq!(game.icon_path.as_deref(), Some(icon.to_str().unwrap()));
        assert_eq!(game.icon_hash.as_deref(), Some(hash.as_str()));
        assert!(
            games
                .iter()
                .find(|game| game.app_id == 620)
                .unwrap()
                .icon_path
                .is_none()
        );
        fs::remove_file(&icon).unwrap();
        let game = launch_target(&fixture.prefix, 480).unwrap();
        assert!(game.icon_path.is_none());
        assert_eq!(game.icon_hash.as_deref(), Some(hash.as_str()));
    }

    #[test]
    fn validates_steam_shortcut_identity_paths_duplicates_and_character_encodings() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let hash = "a".repeat(40);
        let shortcut = shortcut_fixture(&fixture, "example", 480, &hash);
        let drive = CheckedDrive::open(&fixture.prefix.join("drive_c")).unwrap();
        let text = fs::read_to_string(&shortcut).unwrap();
        assert_eq!(shortcut_icon_association(&drive, &text).unwrap().1, 480);
        for wrong in [
            text.replace("steam://rungameid/480", "steam://rungameid/0480"),
            text.replace("steam://rungameid/480", "steam://rungameid/480?x=1"),
            text.replace("C:\\Program Files", "Z:\\Program Files"),
            text.replace("Steam\\steam\\games", "Other\\steam\\games"),
            text.replace("Steam\\steam\\games", "Steam\\..\\games"),
            text.replace(".ico", ".exe"),
            text.replace("IconIndex=0", "IconIndex=1"),
            format!("{text}\nURL=steam://rungameid/480"),
            format!("{text}\nIconFile=bad"),
            text.replace(&hash, &"g".repeat(40)),
        ] {
            assert!(shortcut_icon_association(&drive, &wrong).is_none());
        }
        let utf16 = [
            vec![0xff, 0xfe],
            text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        ]
        .concat();
        fs::write(&shortcut, utf16).unwrap();
        assert_eq!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .as_deref(),
            Some(hash.as_str())
        );
        assert!(shortcut_text(vec![0xff, 0xfe, 0]).is_none());
        assert!(shortcut_text(vec![0xff, 0xfe, 0x00, 0xd8]).is_none());
    }

    #[test]
    fn conflicting_shortcuts_do_not_select_an_arbitrary_publisher_icon() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let hash = "a".repeat(40);
        shortcut_fixture(&fixture, "example", 480, &hash);
        shortcut_fixture(&fixture, "Public", 480, &hash);
        assert_eq!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .as_deref(),
            Some(hash.as_str())
        );
        shortcut_fixture(&fixture, "Public", 480, &"b".repeat(40));
        assert!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .is_none()
        );
    }

    #[test]
    fn never_follows_host_profiles_shortcuts_or_icon_files() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let hash = "a".repeat(40);
        let shortcut = shortcut_fixture(&fixture, "example", 480, &hash);
        let outside = fixture.root.join("host");
        fs::create_dir_all(&outside).unwrap();
        let external = outside.join("external.url");
        fs::rename(&shortcut, &external).unwrap();
        symlink(&external, &shortcut).unwrap();
        assert!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .is_none()
        );
        fs::remove_file(&shortcut).unwrap();
        fs::rename(&external, &shortcut).unwrap();
        fs::write(outside.join("icon.ico"), b"outside publisher icon").unwrap();
        let icon = fixture
            .steam
            .join("steam/games")
            .join(format!("{hash}.ico"));
        symlink(outside.join("icon.ico"), &icon).unwrap();
        assert!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_path
                .is_none()
        );
        let desktop = shortcut.parent().unwrap();
        fs::rename(desktop, outside.join("Desktop")).unwrap();
        symlink(outside.join("Desktop"), desktop).unwrap();
        assert!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .is_none()
        );
    }

    #[test]
    fn finds_start_menu_icons_and_refuses_oversized_shortcuts() {
        let fixture = Fixture::new();
        fixture.game(480, "Spacewar", "Spacewar");
        let hash = "a".repeat(40);
        let shortcut = shortcut_fixture(&fixture, "example", 480, &hash);
        let programs = fixture.prefix.join(
            "drive_c/users/example/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Steam",
        );
        fs::create_dir_all(&programs).unwrap();
        let nested = programs.join("Spacewar.url");
        fs::rename(&shortcut, &nested).unwrap();
        assert_eq!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .as_deref(),
            Some(hash.as_str())
        );
        let oversized = File::options().write(true).open(&nested).unwrap();
        oversized.set_len(MAX_SHORTCUT_BYTES + 1).unwrap();
        assert!(
            launch_target(&fixture.prefix, 480)
                .unwrap()
                .icon_hash
                .is_none()
        );
    }

    #[test]
    fn bounded_parser_handles_comments_utf8_and_escaped_quotes() {
        let fields = parse_key_values("\u{feff}\"AppState\" { // comment\n\"name\" \"Café \\\"Game\\\"\" \"nested\" { \"a\" \"b\" } }").unwrap();
        let state = unique_object(&fields, "appstate").unwrap();
        assert_eq!(unique_text(state, "NAME"), Some("Café \"Game\""));
        assert!(parse_key_values("\"AppState\" { \"name\" \"unfinished").is_none());
        assert!(parse_key_values("\"AppState\" { } }").is_none());
        assert!(
            parse_key_values(&format!(
                "{}{}",
                "x { ".repeat(MAX_DEPTH + 1),
                "}".repeat(MAX_DEPTH + 1)
            ))
            .is_none()
        );
        assert!(parse_key_values(&"a b ".repeat(MAX_TOKENS / 2 + 1)).is_none());
    }
}
