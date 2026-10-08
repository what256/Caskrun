//! Read-only Epic and GOG installed-game discovery within a bottle's real C: drive.
//!
//! Identity comes from each store's own manifest, never from a guessed
//! executable, so this module reports what a store says is installed and asserts
//! nothing about whether it runs. It reads only manifest files under the
//! bottle's C: drive; it opens no account, credential or token file, starts no
//! Wine process, and performs no network access.
//!
//! Epic manifest location and fields follow the launcher's own `Data/Manifests`
//! directory. GOG game metadata follows its `goggame-<id>.info` files, which the
//! installer writes into the game's own install directory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::{
    O_DIRECTORY, O_NOFOLLOW_ANY, cancellation, open_stable_directory, same_identity, same_snapshot,
};

// macOS sys/fcntl.h. Opening a path that was swapped to a FIFO after its
// metadata was read would otherwise wait for a writer that never arrives.
const O_NONBLOCK: i32 = 0x0000_0004;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_READ_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SCAN_ENTRIES: usize = 8_192;
const MAX_GAMES: usize = 1_024;
const MAX_NAME_CHARS: usize = 200;
const MAX_STORE_ID_CHARS: usize = 128;
const MAX_WINDOWS_PATH_CHARS: usize = 4_096;
const MAX_WINDOWS_PATH_DEPTH: usize = 64;

const EPIC_MANIFESTS: &str = "ProgramData/Epic/EpicGamesLauncher/Data/Manifests";
/// Contained directories a GOG installer uses by default. Nothing outside these
/// is scanned, so an arbitrary directory name cannot pull the scan across the
/// drive.
const GOG_ROOTS: &[&str] = &[
    "Program Files (x86)/GOG Galaxy/Games",
    "Program Files/GOG Galaxy/Games",
    "GOG Games",
];

pub(crate) const STORE_EPIC: &str = "epic";
pub(crate) const STORE_GOG: &str = "gog";

#[derive(Debug, Serialize)]
pub(crate) struct PublisherLibrary {
    pub(crate) games: Vec<PublisherGame>,
    /// A bound was reached, so the list may be incomplete.
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PublisherGame {
    /// Stable within this bottle: the store and that store's own identifier.
    pub(crate) id: String,
    pub(crate) store: String,
    pub(crate) store_id: String,
    pub(crate) name: String,
    pub(crate) install_path: String,
}

/// Epic writes one `.item` file per installed application. Unknown fields are
/// ignored, and every field this reads is optional so a partial manifest is
/// skipped rather than trusted.
#[derive(Deserialize)]
struct EpicManifest {
    #[serde(rename = "AppName")]
    app_name: Option<String>,
    #[serde(rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(rename = "InstallLocation")]
    install_location: Option<String>,
    /// Set while a download is unfinished; such an entry is not installed.
    #[serde(rename = "bIsIncompleteInstall")]
    incomplete: Option<bool>,
}

#[derive(Deserialize)]
struct GogManifest {
    #[serde(rename = "gameId")]
    game_id: Option<String>,
    #[serde(rename = "rootGameId")]
    root_game_id: Option<String>,
    name: Option<String>,
}

/// The bottle's C: drive, held open so a later rename cannot move the scan
/// somewhere else part way through.
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
            return Err("Bottle C: drive changed while reading its game library".into());
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
            // symlink_metadata does not reject a symlink in a parent component.
            OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW_ANY | O_DIRECTORY)
                .open(path)
                .and_then(|file| file.metadata())
                .is_ok_and(|metadata| same_identity(&self.identity, &metadata))
        })
    }
}

struct ReadBudget {
    remaining: u64,
    exhausted: bool,
}

impl ReadBudget {
    fn new(total: u64) -> Self {
        Self {
            remaining: total,
            exhausted: false,
        }
    }
}

pub(crate) fn discover(prefix: &Path) -> Result<PublisherLibrary, String> {
    discover_with_budget(prefix, MAX_TOTAL_READ_BYTES)
}

fn discover_with_budget(prefix: &Path, max_read_bytes: u64) -> Result<PublisherLibrary, String> {
    cancellation::check()?;
    let mut result = PublisherLibrary {
        games: Vec::new(),
        truncated: false,
    };
    let drive_path = prefix.join("drive_c");
    match fs::symlink_metadata(&drive_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Cannot inspect the bottle's C: drive: {error}")),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err("Bottle C: drive must be a real directory".into());
        }
        Ok(_) => {}
    }
    let drive = CheckedDrive::open(&drive_path)?;
    let mut budget = ReadBudget::new(max_read_bytes);
    let mut examined = 0usize;
    let mut found = Vec::new();

    scan_epic(&drive, &mut budget, &mut examined, &mut found, &mut result)?;
    scan_gog(&drive, &mut budget, &mut examined, &mut found, &mut result)?;

    // Nothing is published for a scan the caller has already walked away from.
    cancellation::check()?;
    // The whole scan is only meaningful if it read the drive it started on.
    if !drive.is_current() {
        return Err("Bottle C: drive changed while reading its game library".into());
    }
    if budget.exhausted {
        result.truncated = true;
    }
    // One store identifier is one game, however many manifests mention it.
    let mut seen = BTreeSet::new();
    found.retain(|game: &PublisherGame| seen.insert(game.id.clone()));
    found.sort_by(|a, b| {
        a.store
            .cmp(&b.store)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.store_id.cmp(&b.store_id))
    });
    if found.len() > MAX_GAMES {
        found.truncate(MAX_GAMES);
        result.truncated = true;
    }
    result.games = found;
    Ok(result)
}

fn scan_epic(
    drive: &CheckedDrive,
    budget: &mut ReadBudget,
    examined: &mut usize,
    found: &mut Vec<PublisherGame>,
    result: &mut PublisherLibrary,
) -> Result<(), String> {
    cancellation::check()?;
    if budget.exhausted {
        return Ok(());
    }
    let Some(manifests) = contained_directory(drive, EPIC_MANIFESTS) else {
        return Ok(());
    };
    let Ok((_guard, snapshot)) = open_stable_directory(&manifests) else {
        result.truncated = true;
        return Ok(());
    };
    let Ok(entries) = fs::read_dir(&manifests) else {
        result.truncated = true;
        return Ok(());
    };
    for entry in entries {
        cancellation::check()?;
        if budget.exhausted {
            return Ok(());
        }
        if *examined >= MAX_SCAN_ENTRIES {
            result.truncated = true;
            return Ok(());
        }
        *examined += 1;
        let Ok(entry) = entry else {
            result.truncated = true;
            continue;
        };
        // The directory must still be the one that was opened.
        if !fs::symlink_metadata(&manifests).is_ok_and(|current| same_snapshot(&snapshot, &current))
        {
            result.truncated = true;
            return Ok(());
        }
        let path = entry.path();
        if !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        cancellation::check()?;
        let bytes = read_manifest(&path, budget, result);
        // read_manifest reports every refusal as None, so check again here: a
        // cancellation during the read must not be mistaken for a bad manifest.
        cancellation::check()?;
        let Some(bytes) = bytes else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<EpicManifest>(&bytes) else {
            continue;
        };
        if manifest.incomplete.unwrap_or(false) {
            continue;
        }
        let (Some(app_name), Some(display_name), Some(location)) = (
            manifest.app_name.as_deref(),
            manifest.display_name.as_deref(),
            manifest.install_location.as_deref(),
        ) else {
            continue;
        };
        let (Some(store_id), Some(name)) =
            (checked_identifier(app_name), checked_name(display_name))
        else {
            continue;
        };
        // An install directory the store records but the bottle does not have is
        // not an installed game.
        let Some(install_path) = windows_directory(drive, location) else {
            continue;
        };
        found.push(PublisherGame {
            id: format!("{STORE_EPIC}:{store_id}"),
            store: STORE_EPIC.to_owned(),
            store_id,
            name,
            install_path: install_path.display().to_string(),
        });
    }
    Ok(())
}

fn scan_gog(
    drive: &CheckedDrive,
    budget: &mut ReadBudget,
    examined: &mut usize,
    found: &mut Vec<PublisherGame>,
    result: &mut PublisherLibrary,
) -> Result<(), String> {
    for root in GOG_ROOTS {
        cancellation::check()?;
        if budget.exhausted {
            return Ok(());
        }
        let Some(root_path) = contained_directory(drive, root) else {
            continue;
        };
        let Ok((_guard, snapshot)) = open_stable_directory(&root_path) else {
            result.truncated = true;
            continue;
        };
        let Ok(entries) = fs::read_dir(&root_path) else {
            result.truncated = true;
            continue;
        };
        for entry in entries {
            cancellation::check()?;
            if budget.exhausted {
                return Ok(());
            }
            if *examined >= MAX_SCAN_ENTRIES {
                result.truncated = true;
                return Ok(());
            }
            *examined += 1;
            let Ok(entry) = entry else {
                result.truncated = true;
                continue;
            };
            if !fs::symlink_metadata(&root_path)
                .is_ok_and(|current| same_snapshot(&snapshot, &current))
            {
                result.truncated = true;
                return Ok(());
            }
            // Only a real directory directly inside a known root is a game
            // folder; a symlink here is refused rather than followed.
            if !fs::symlink_metadata(entry.path())
                .is_ok_and(|metadata| metadata.file_type().is_dir())
            {
                continue;
            }
            let Some(game_dir) = contained_path(drive, &entry.path()) else {
                continue;
            };
            scan_gog_game_directory(drive, &game_dir, budget, examined, found, result)?;
        }
    }
    Ok(())
}

fn scan_gog_game_directory(
    drive: &CheckedDrive,
    game_dir: &Path,
    budget: &mut ReadBudget,
    examined: &mut usize,
    found: &mut Vec<PublisherGame>,
    result: &mut PublisherLibrary,
) -> Result<(), String> {
    cancellation::check()?;
    if budget.exhausted {
        return Ok(());
    }
    let Ok((_guard, snapshot)) = open_stable_directory(game_dir) else {
        result.truncated = true;
        return Ok(());
    };
    let Ok(entries) = fs::read_dir(game_dir) else {
        result.truncated = true;
        return Ok(());
    };
    for entry in entries {
        cancellation::check()?;
        if budget.exhausted {
            return Ok(());
        }
        if *examined >= MAX_SCAN_ENTRIES {
            result.truncated = true;
            return Ok(());
        }
        *examined += 1;
        let Ok(entry) = entry else {
            result.truncated = true;
            continue;
        };
        if !fs::symlink_metadata(game_dir).is_ok_and(|current| same_snapshot(&snapshot, &current)) {
            result.truncated = true;
            return Ok(());
        }
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with("goggame-") || !file_name.ends_with(".info") {
            continue;
        }
        cancellation::check()?;
        let bytes = read_manifest(&path, budget, result);
        // As above: a refusal and a cancellation both arrive as None here.
        cancellation::check()?;
        let Some(bytes) = bytes else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<GogManifest>(&bytes) else {
            continue;
        };
        let (Some(game_id), Some(name)) = (manifest.game_id.as_deref(), manifest.name.as_deref())
        else {
            continue;
        };
        // A DLC names the base game as its root; only the base game is listed.
        if manifest
            .root_game_id
            .as_deref()
            .is_some_and(|root| root != game_id)
        {
            continue;
        }
        let (Some(store_id), Some(name)) = (checked_identifier(game_id), checked_name(name)) else {
            continue;
        };
        if !drive.is_current() {
            result.truncated = true;
            return Ok(());
        }
        found.push(PublisherGame {
            id: format!("{STORE_GOG}:{store_id}"),
            store: STORE_GOG.to_owned(),
            store_id,
            name,
            install_path: game_dir.display().to_string(),
        });
    }
    Ok(())
}

/// Read a manifest without following a symlink and without exceeding either the
/// per-file or the whole-scan budget.
fn read_manifest(
    path: &Path,
    budget: &mut ReadBudget,
    result: &mut PublisherLibrary,
) -> Option<Vec<u8>> {
    if budget.exhausted || cancellation::is_requested() {
        return None;
    }
    let before = fs::symlink_metadata(path).ok()?;
    if !before.file_type().is_file() {
        return None;
    }
    // O_NONBLOCK keeps this open from waiting even if the path became a FIFO
    // after the check above; the opened handle is then checked again below.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | O_NONBLOCK)
        .open(path)
        .ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() || !same_snapshot(&before, &opened) {
        return None;
    }
    let bytes = read_manifest_contents(&mut file, before.len(), budget, result)?;
    // Publish bytes only when the handle that produced them and the path it was
    // opened from are both still exactly what was measured, so a file replaced
    // or rewritten while it was being read is discarded instead of parsed.
    let after = file.metadata().ok()?;
    if !after.is_file() || !same_snapshot(&before, &after) {
        return None;
    }
    if !fs::symlink_metadata(path).is_ok_and(|current| same_snapshot(&before, &current)) {
        return None;
    }
    Some(bytes)
}

/// Account for bytes actually returned, even when a file grew or a later read
/// failed. At most one overflow byte is read for the entire exhausted scan.
fn read_manifest_contents(
    reader: &mut impl Read,
    measured_len: u64,
    budget: &mut ReadBudget,
    result: &mut PublisherLibrary,
) -> Option<Vec<u8>> {
    if budget.exhausted || cancellation::is_requested() {
        return None;
    }
    if measured_len > MAX_MANIFEST_BYTES {
        result.truncated = true;
        return None;
    }
    if measured_len > budget.remaining {
        budget.exhausted = true;
        result.truncated = true;
        return None;
    }
    let limit = MAX_MANIFEST_BYTES.min(budget.remaining);
    let mut bytes = Vec::with_capacity(measured_len as usize);
    let read_result = crate::copy_cancellable(&mut reader.take(limit + 1), &mut bytes);
    let consumed = bytes.len() as u64;
    if consumed > budget.remaining {
        budget.exhausted = true;
    }
    budget.remaining = budget.remaining.saturating_sub(consumed);
    if consumed > limit {
        result.truncated = true;
        return None;
    }
    if read_result.is_err() || consumed != measured_len || cancellation::is_requested() {
        return None;
    }
    Some(bytes)
}

/// Resolve a path written with forward slashes relative to the drive, requiring
/// every component to be a real directory inside it.
fn contained_directory(drive: &CheckedDrive, relative: &str) -> Option<PathBuf> {
    let mut path = drive.path.clone();
    for component in relative.split('/').filter(|part| !part.is_empty()) {
        if !safe_windows_component(component) {
            return None;
        }
        path.push(component);
    }
    contained_path(drive, &path)
}

/// Map a Windows path from a manifest onto the bottle's C: drive. Any other
/// drive letter, including Wine's Z: host mapping, and any UNC path is refused.
fn windows_directory(drive: &CheckedDrive, windows_path: &str) -> Option<PathBuf> {
    let bytes = windows_path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].eq_ignore_ascii_case(&b'c')
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || windows_path.len() > MAX_WINDOWS_PATH_CHARS
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
        if depth > MAX_WINDOWS_PATH_DEPTH || !safe_windows_component(component) {
            return None;
        }
        path.push(component);
    }
    // A manifest naming the drive root itself is not a game directory.
    if depth == 0 {
        return None;
    }
    contained_path(drive, &path)
}

/// Accept a path only when every component below the drive is a real directory
/// and the result is still inside the drive that was opened.
fn contained_path(drive: &CheckedDrive, path: &Path) -> Option<PathBuf> {
    if !drive.is_current() {
        return None;
    }
    let relative = path.strip_prefix(&drive.path).ok()?;
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
    (drive.is_current() && resolved.starts_with(&drive.path)).then_some(resolved)
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

/// A store identifier becomes part of a stable record, so it must be a plain
/// token rather than anything that could be read as a path or a separator.
fn checked_identifier(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()
        && trimmed.chars().count() <= MAX_STORE_ID_CHARS
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        && trimmed != "."
        && trimmed != "..")
        .then(|| trimmed.to_owned())
}

/// A display name comes from a manifest, so it is bounded and stripped of
/// control characters rather than shown as written.
fn checked_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    let bounded: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    Some(bounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, symlink};

    /// Fixtures live in the repository's ignored build directory and are removed
    /// by each test that creates them. A fresh identifier per call means no two
    /// tests, and no two concurrent runs of the suite, can share a directory, so
    /// nothing has to be deleted before a test can use its own fixture.
    fn test_prefix(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build/publisher-library-tests")
            .join(format!("{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("drive_c")).unwrap();
        root
    }

    fn make_fifo(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let status = std::process::Command::new("/usr/bin/mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "could not create the test FIFO");
        assert!(
            fs::symlink_metadata(path).unwrap().file_type().is_fifo(),
            "the fixture must really be a FIFO"
        );
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn epic_manifest(prefix: &Path, file: &str, contents: &str) {
        write(
            &prefix.join("drive_c").join(EPIC_MANIFESTS).join(file),
            contents,
        );
    }

    /// Build a manifest from ordinary Windows values. The helper does the JSON
    /// escaping, so a test writes the path a user would see rather than one
    /// pre-escaped for the document it lands in.
    fn epic_item(app_name: &str, display: &str, location: &str) -> String {
        let json = |value: &str| value.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            r#"{{"AppName":"{}","DisplayName":"{}","InstallLocation":"{}","LaunchExecutable":"ignored.exe"}}"#,
            json(app_name),
            json(display),
            json(location)
        )
    }

    #[test]
    fn missing_drive_or_stores_is_an_empty_library() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build/publisher-library-tests")
            .join(format!("absent-drive-{}", uuid::Uuid::new_v4()));
        // No C: drive at all.
        let library = discover(&root).unwrap();
        assert!(library.games.is_empty() && !library.truncated);
        // A C: drive with neither store installed.
        let prefix = test_prefix("missing_drive_or_stores_is_an_empty_library");
        let library = discover(&prefix).unwrap();
        assert!(library.games.is_empty() && !library.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_c_drive_that_is_not_a_real_directory_is_refused() {
        let prefix = test_prefix("a_c_drive_that_is_not_a_real_directory_is_refused");
        fs::remove_dir_all(prefix.join("drive_c")).unwrap();
        symlink("/tmp", prefix.join("drive_c")).unwrap();
        assert!(discover(&prefix).is_err());
        fs::remove_file(prefix.join("drive_c")).unwrap();
        fs::write(prefix.join("drive_c"), b"not a directory").unwrap();
        assert!(discover(&prefix).is_err());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn reads_an_ordinary_epic_and_gog_library() {
        let prefix = test_prefix("reads_an_ordinary_epic_and_gog_library");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Program Files/Epic Games/Nova")).unwrap();
        epic_manifest(
            &prefix,
            "Nova.item",
            &epic_item(
                "NovaAppId",
                "Nova Adventure",
                "C:\\\\Program Files\\\\Epic Games\\\\Nova",
            ),
        );
        let gog_dir = drive.join("GOG Games/Zenith");
        fs::create_dir_all(&gog_dir).unwrap();
        write(
            &gog_dir.join("goggame-1207658924.info"),
            r#"{"gameId":"1207658924","rootGameId":"1207658924","name":"Zenith"}"#,
        );

        let library = discover(&prefix).unwrap();
        assert!(!library.truncated);
        assert_eq!(library.games.len(), 2);
        let epic = &library.games[0];
        assert_eq!(epic.store, "epic");
        assert_eq!(epic.store_id, "NovaAppId");
        assert_eq!(epic.name, "Nova Adventure");
        assert_eq!(epic.id, "epic:NovaAppId");
        assert!(epic.install_path.ends_with("Program Files/Epic Games/Nova"));
        let gog = &library.games[1];
        assert_eq!(gog.store, "gog");
        assert_eq!(gog.store_id, "1207658924");
        assert_eq!(gog.name, "Zenith");
        assert_eq!(gog.id, "gog:1207658924");
        assert!(gog.install_path.ends_with("GOG Games/Zenith"));
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_recorded_install_directory_must_exist_in_the_bottle() {
        let prefix = test_prefix("a_recorded_install_directory_must_exist_in_the_bottle");
        epic_manifest(
            &prefix,
            "Absent.item",
            &epic_item("AbsentId", "Absent Game", "C:\\Program Files\\Absent"),
        );
        // The directory exists but is a file, not an install directory.
        fs::create_dir_all(prefix.join("drive_c/Program Files")).unwrap();
        fs::write(prefix.join("drive_c/Program Files/Decoy"), b"file").unwrap();
        epic_manifest(
            &prefix,
            "Decoy.item",
            &epic_item("DecoyId", "Decoy Game", "C:\\Program Files\\Decoy"),
        );
        let library = discover(&prefix).unwrap();
        assert!(library.games.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn refuses_host_other_drive_and_escaping_install_paths() {
        let prefix = test_prefix("refuses_host_other_drive_and_escaping_install_paths");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        // An escape outside the drive, the host drive, another drive letter, a
        // UNC path, the drive root itself and a traversal component.
        for (file, location) in [
            ("Outside.item", "C:\\..\\..\\outside"),
            ("Host.item", "Z:\\Users\\someone"),
            ("Other.item", "D:\\Games\\Real"),
            ("Unc.item", "\\\\server\\share"),
            ("Root.item", "C:\\"),
            ("Dots.item", "C:\\Games\\..\\Games\\Real"),
            ("Empty.item", ""),
        ] {
            epic_manifest(&prefix, file, &epic_item("EscapeId", "Escape", location));
            let library = discover(&prefix).unwrap();
            assert!(
                library.games.is_empty(),
                "{location} must not produce a game"
            );
            fs::remove_file(drive.join(EPIC_MANIFESTS).join(file)).unwrap();
        }
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn refuses_symlinked_manifests_directories_and_files() {
        let prefix = test_prefix("refuses_symlinked_manifests_directories_and_files");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Program Files/Epic Games/Nova")).unwrap();
        let outside = prefix.join("outside");
        fs::create_dir_all(&outside).unwrap();
        write(
            &outside.join("Planted.item"),
            &epic_item(
                "PlantedId",
                "Planted",
                "C:\\\\Program Files\\\\Epic Games\\\\Nova",
            ),
        );

        // A symlinked manifests directory is not followed.
        let manifests = drive.join(EPIC_MANIFESTS);
        fs::create_dir_all(manifests.parent().unwrap()).unwrap();
        symlink(&outside, &manifests).unwrap();
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_file(&manifests).unwrap();

        // A symlinked manifest file inside a real directory is not read.
        fs::create_dir_all(&manifests).unwrap();
        symlink(outside.join("Planted.item"), manifests.join("Planted.item")).unwrap();
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_file(manifests.join("Planted.item")).unwrap();

        // An install directory reached through a symlink out of the drive is
        // refused even though the manifest itself is ordinary.
        symlink(&outside, drive.join("Linked")).unwrap();
        epic_manifest(
            &prefix,
            "Linked.item",
            &epic_item("LinkedId", "Linked", "C:\\Linked"),
        );
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_symlinked_component_inside_the_drive_is_still_refused() {
        // Canonicalising alone would accept these, because they resolve back
        // inside the drive. Every component below the drive must be a real
        // directory, so a redirected store root or install path is refused.
        let prefix = test_prefix("a_symlinked_component_inside_the_drive_is_still_refused");
        let drive = prefix.join("drive_c");
        let real = drive.join("Elsewhere/Zenith");
        fs::create_dir_all(&real).unwrap();
        write(
            &real.join("goggame-700.info"),
            r#"{"gameId":"700","rootGameId":"700","name":"Zenith"}"#,
        );
        // A GOG root that is a symlink to another directory in the same drive.
        symlink(drive.join("Elsewhere"), drive.join("GOG Games")).unwrap();
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_file(drive.join("GOG Games")).unwrap();

        // An Epic install path whose parent component is such a symlink.
        fs::create_dir_all(drive.join("Games/Nova")).unwrap();
        symlink(drive.join("Games"), drive.join("Shortcut")).unwrap();
        epic_manifest(
            &prefix,
            "Redirected.item",
            &epic_item("RedirectedId", "Redirected", "C:\\Shortcut\\Nova"),
        );
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn malformed_and_hostile_manifests_are_skipped_not_trusted() {
        let prefix = test_prefix("malformed_and_hostile_manifests_are_skipped_not_trusted");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        let good = "C:\\\\Games\\\\Real";
        for (file, contents) in [
            ("NotJson.item", "this is not json".to_owned()),
            ("NotObject.item", "[1, 2, 3]".to_owned()),
            ("Empty.item", "{}".to_owned()),
            (
                "NoName.item",
                format!(r#"{{"AppName":"NoNameId","InstallLocation":"{good}"}}"#),
            ),
            (
                "NoId.item",
                format!(r#"{{"DisplayName":"No Id","InstallLocation":"{good}"}}"#),
            ),
            (
                "WrongTypes.item",
                format!(r#"{{"AppName":5,"DisplayName":true,"InstallLocation":"{good}"}}"#),
            ),
            (
                "Incomplete.item",
                format!(
                    r#"{{"AppName":"IncompleteId","DisplayName":"Still Downloading","InstallLocation":"{good}","bIsIncompleteInstall":true}}"#
                ),
            ),
            (
                "ControlName.item",
                format!(
                    r#"{{"AppName":"ControlId","DisplayName":"Bad\nName","InstallLocation":"{good}"}}"#
                ),
            ),
            (
                "SlashId.item",
                format!(
                    r#"{{"AppName":"../escape","DisplayName":"Escape","InstallLocation":"{good}"}}"#
                ),
            ),
            (
                "DeepNesting.item",
                format!("{}{}{}", "[".repeat(600), "1", "]".repeat(600)),
            ),
        ] {
            epic_manifest(&prefix, file, &contents);
        }
        // A file that is not a manifest at all is ignored by extension.
        epic_manifest(&prefix, "notes.txt", "ignored");
        let library = discover(&prefix).unwrap();
        assert!(
            library.games.is_empty(),
            "no hostile manifest may produce a game: {:?}",
            library.games
        );
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn one_store_identifier_is_one_game() {
        let prefix = test_prefix("one_store_identifier_is_one_game");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        fs::create_dir_all(drive.join("Games/Second")).unwrap();
        epic_manifest(
            &prefix,
            "First.item",
            &epic_item("SharedId", "Shared Game", "C:\\Games\\Real"),
        );
        epic_manifest(
            &prefix,
            "Second.item",
            &epic_item("SharedId", "Shared Game Again", "C:\\Games\\Second"),
        );
        let library = discover(&prefix).unwrap();
        assert_eq!(library.games.len(), 1);
        assert_eq!(library.games[0].store_id, "SharedId");

        // The same identifier from two GOG folders also collapses to one.
        let first = drive.join("GOG Games/One");
        let second = drive.join("GOG Games/Two");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let info = r#"{"gameId":"42","rootGameId":"42","name":"Twin"}"#;
        write(&first.join("goggame-42.info"), info);
        write(&second.join("goggame-42.info"), info);
        let library = discover(&prefix).unwrap();
        assert_eq!(
            library
                .games
                .iter()
                .filter(|game| game.store == "gog")
                .count(),
            1
        );
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn gog_dlc_is_not_listed_as_its_own_game() {
        let prefix = test_prefix("gog_dlc_is_not_listed_as_its_own_game");
        let game_dir = prefix.join("drive_c/GOG Games/Zenith");
        fs::create_dir_all(&game_dir).unwrap();
        write(
            &game_dir.join("goggame-100.info"),
            r#"{"gameId":"100","rootGameId":"100","name":"Zenith"}"#,
        );
        write(
            &game_dir.join("goggame-101.info"),
            r#"{"gameId":"101","rootGameId":"100","name":"Zenith Expansion"}"#,
        );
        let library = discover(&prefix).unwrap();
        assert_eq!(library.games.len(), 1);
        assert_eq!(library.games[0].store_id, "100");
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn gog_scanning_stays_inside_known_roots() {
        let prefix = test_prefix("gog_scanning_stays_inside_known_roots");
        let drive = prefix.join("drive_c");
        // A game folder outside every known root is not discovered.
        let elsewhere = drive.join("Elsewhere/Hidden");
        fs::create_dir_all(&elsewhere).unwrap();
        write(
            &elsewhere.join("goggame-900.info"),
            r#"{"gameId":"900","rootGameId":"900","name":"Hidden"}"#,
        );
        assert!(discover(&prefix).unwrap().games.is_empty());

        // A symlinked game folder inside a known root is refused.
        fs::create_dir_all(drive.join("GOG Games")).unwrap();
        symlink(&elsewhere, drive.join("GOG Games/Linked")).unwrap();
        assert!(discover(&prefix).unwrap().games.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn oversized_manifests_and_an_exhausted_budget_report_truncation() {
        let prefix = test_prefix("oversized_manifests_and_an_exhausted_budget_report_truncation");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        let padding = "x".repeat((MAX_MANIFEST_BYTES + 1024) as usize);
        epic_manifest(
            &prefix,
            "Huge.item",
            &format!(
                r#"{{"AppName":"HugeId","DisplayName":"Huge","InstallLocation":"C:\\Games\\Real","Pad":"{padding}"}}"#
            ),
        );
        let library = discover(&prefix).unwrap();
        assert!(library.truncated && library.games.is_empty());
        fs::remove_file(drive.join(EPIC_MANIFESTS).join("Huge.item")).unwrap();

        // A whole-scan budget smaller than one ordinary manifest also truncates.
        epic_manifest(
            &prefix,
            "Small.item",
            &epic_item("SmallId", "Small", "C:\\Games\\Real"),
        );
        let library = discover_with_budget(&prefix, 4).unwrap();
        assert!(library.truncated && library.games.is_empty());
        // The same fixture within budget is read normally.
        let library = discover(&prefix).unwrap();
        assert!(!library.truncated && library.games.len() == 1);
        fs::remove_dir_all(prefix).unwrap();
    }

    struct MeteredReader<'a> {
        remaining: &'a [u8],
        fail_at_eof: bool,
        calls: usize,
        consumed: usize,
    }

    impl<'a> MeteredReader<'a> {
        fn new(bytes: &'a [u8], fail_at_eof: bool) -> Self {
            Self {
                remaining: bytes,
                fail_at_eof,
                calls: 0,
                consumed: 0,
            }
        }
    }

    impl Read for MeteredReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.remaining.is_empty() && self.fail_at_eof {
                return Err(std::io::Error::other("injected read failure"));
            }
            let count = output.len().min(self.remaining.len());
            output[..count].copy_from_slice(&self.remaining[..count]);
            self.remaining = &self.remaining[count..];
            self.consumed += count;
            Ok(count)
        }
    }

    fn empty_library() -> PublisherLibrary {
        PublisherLibrary {
            games: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn a_grown_manifest_is_discarded_but_charged_for_every_byte_read() {
        let mut budget = ReadBudget::new(12);
        let mut result = empty_library();
        // Represents a one-byte file that grew after its opened metadata check.
        let mut grown = MeteredReader::new(b"12345678", false);
        assert!(read_manifest_contents(&mut grown, 1, &mut budget, &mut result).is_none());
        assert_eq!(grown.consumed, 8);
        assert_eq!(budget.remaining, 4);
        assert!(!budget.exhausted);
        let mut stable = MeteredReader::new(b"next", false);
        assert_eq!(
            read_manifest_contents(&mut stable, 4, &mut budget, &mut result).unwrap(),
            b"next"
        );
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn a_failed_manifest_read_still_spends_the_returned_prefix() {
        let mut budget = ReadBudget::new(10);
        let mut result = empty_library();
        let mut broken = MeteredReader::new(b"123", true);
        assert!(read_manifest_contents(&mut broken, 5, &mut budget, &mut result).is_none());
        assert_eq!(broken.consumed, 3);
        assert_eq!(budget.remaining, 7);
        assert!(!budget.exhausted);
    }

    #[test]
    fn aggregate_overflow_reads_one_sentinel_then_never_touches_another_reader() {
        let mut budget = ReadBudget::new(4);
        let mut result = empty_library();
        let mut grown = MeteredReader::new(b"far more than four bytes", false);
        assert!(read_manifest_contents(&mut grown, 0, &mut budget, &mut result).is_none());
        assert_eq!(grown.consumed, 5);
        assert!(result.truncated && budget.exhausted);
        assert_eq!(budget.remaining, 0);
        let mut next = MeteredReader::new(b"another growing file", false);
        assert!(read_manifest_contents(&mut next, 0, &mut budget, &mut result).is_none());
        assert_eq!(next.calls, 0);
        assert_eq!(next.consumed, 0);
    }

    #[test]
    fn per_file_growth_is_bounded_and_does_not_erase_the_remaining_scan_budget() {
        let payload = vec![b'x'; MAX_MANIFEST_BYTES as usize + 100];
        let mut budget = ReadBudget::new(MAX_MANIFEST_BYTES * 2);
        let mut result = empty_library();
        let mut grown = MeteredReader::new(&payload, false);
        assert!(read_manifest_contents(&mut grown, 1, &mut budget, &mut result).is_none());
        assert_eq!(grown.consumed as u64, MAX_MANIFEST_BYTES + 1);
        assert_eq!(budget.remaining, MAX_MANIFEST_BYTES - 1);
        assert!(result.truncated);
        assert!(!budget.exhausted);
    }

    #[test]
    fn a_fifo_manifest_is_refused_and_never_waits_for_a_writer() {
        // A FIFO with no writer would block an ordinary open for ever. The scan
        // must finish and must not treat the entry as a manifest. This test
        // hanging is itself the failure signal.
        let prefix = test_prefix("fifo_manifest");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        make_fifo(&drive.join(EPIC_MANIFESTS).join("Blocking.item"));
        // A readable manifest beside it still has to be found, so the FIFO
        // cannot be skipped by abandoning the directory.
        epic_manifest(
            &prefix,
            "Real.item",
            &epic_item("RealId", "Real Game", "C:\\Games\\Real"),
        );
        let library = discover(&prefix).unwrap();
        assert_eq!(library.games.len(), 1);
        assert_eq!(library.games[0].store_id, "RealId");

        // The same holds for a GOG info entry and for the first entry a scan
        // meets, which is where an abandoned directory would show.
        let gog_dir = drive.join("GOG Games/Zenith");
        fs::create_dir_all(&gog_dir).unwrap();
        make_fifo(&gog_dir.join("goggame-1.info"));
        write(
            &gog_dir.join("goggame-2.info"),
            r#"{"gameId":"2","rootGameId":"2","name":"Zenith"}"#,
        );
        let library = discover(&prefix).unwrap();
        let gog: Vec<_> = library
            .games
            .iter()
            .filter(|game| game.store == STORE_GOG)
            .collect();
        assert_eq!(gog.len(), 1);
        assert_eq!(gog[0].store_id, "2");
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_replaced_or_rewritten_file_fails_the_snapshot_guard() {
        // read_manifest publishes bytes only while this predicate still holds
        // for the handle it read and for the path it opened.
        let prefix = test_prefix("snapshot_guard");
        let path = prefix.join("manifest.item");
        fs::write(&path, b"{}").unwrap();
        let before = fs::symlink_metadata(&path).unwrap();
        assert!(same_snapshot(
            &before,
            &fs::symlink_metadata(&path).unwrap()
        ));

        // Replacing the file gives a different inode.
        let replacement = prefix.join("other.item");
        fs::write(&replacement, b"{}").unwrap();
        fs::rename(&replacement, &path).unwrap();
        let replaced = fs::symlink_metadata(&path).unwrap();
        assert!(!same_snapshot(&before, &replaced));

        // Rewriting in place keeps the inode but changes length and times.
        fs::write(&path, b"{\"AppName\":\"grown\"}").unwrap();
        assert!(!same_snapshot(
            &replaced,
            &fs::symlink_metadata(&path).unwrap()
        ));
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_drive_replaced_during_the_scan_is_refused() {
        // A result is only published if it describes the drive the scan opened.
        let prefix = test_prefix("drive_replaced");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        epic_manifest(
            &prefix,
            "Real.item",
            &epic_item("RealId", "Real Game", "C:\\Games\\Real"),
        );
        assert_eq!(discover(&prefix).unwrap().games.len(), 1);

        // Swap C: for a different directory after the scan would have opened it.
        let decoy = prefix.join("decoy_drive");
        fs::create_dir_all(&decoy).unwrap();
        fs::rename(&drive, prefix.join("old_drive")).unwrap();
        fs::rename(&decoy, &drive).unwrap();
        // The replacement is a real directory, so this is not caught by the
        // opening checks alone; it has no manifests and yields nothing.
        let library = discover(&prefix).unwrap();
        assert!(library.games.is_empty());
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_requested_cancellation_stops_the_scan_and_is_never_swallowed() {
        use std::sync::atomic::Ordering;
        let prefix = test_prefix("cancellation");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        epic_manifest(
            &prefix,
            "Real.item",
            &epic_item("RealId", "Real Game", "C:\\Games\\Real"),
        );
        let gog_dir = drive.join("GOG Games/Zenith");
        fs::create_dir_all(&gog_dir).unwrap();
        write(
            &gog_dir.join("goggame-5.info"),
            r#"{"gameId":"5","rootGameId":"5","name":"Zenith"}"#,
        );
        // A manifest that would be skipped on its own merits sits alongside, so
        // the test can tell a refusal apart from a cancellation.
        epic_manifest(&prefix, "Broken.item", "not json at all");
        assert_eq!(discover(&prefix).unwrap().games.len(), 2);

        let (request, _guard) = cancellation::test_request();
        request.store(true, Ordering::Release);
        let error = discover(&prefix).unwrap_err();
        assert!(
            cancellation::is_error(&error),
            "a cancelled scan must report the cancellation, not {error:?}"
        );
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn a_cancelled_scan_of_an_empty_bottle_still_reports_the_cancellation() {
        use std::sync::atomic::Ordering;
        // With no store installed there is no loop to notice the request, so the
        // check at the entry is what makes leaving the Library stop the command.
        let prefix = test_prefix("cancellation_empty");
        assert!(discover(&prefix).unwrap().games.is_empty());
        let (request, _guard) = cancellation::test_request();
        request.store(true, Ordering::Release);
        assert!(cancellation::is_error(&discover(&prefix).unwrap_err()));
        fs::remove_dir_all(prefix).unwrap();
    }

    #[test]
    fn identifiers_and_names_are_bounded_and_plain() {
        assert_eq!(
            checked_identifier(" Nova-1_2.3 ").as_deref(),
            Some("Nova-1_2.3")
        );
        for refused in [
            "",
            "   ",
            ".",
            "..",
            "with space",
            "slash/",
            "back\\slash",
            "colon:",
        ] {
            assert!(checked_identifier(refused).is_none(), "{refused:?}");
        }
        assert!(checked_identifier(&"a".repeat(MAX_STORE_ID_CHARS + 1)).is_none());

        assert_eq!(
            checked_name("  Nova Adventure  ").as_deref(),
            Some("Nova Adventure")
        );
        assert!(checked_name("").is_none());
        assert!(checked_name("line\nbreak").is_none());
        assert!(checked_name("\u{0}").is_none());
        let long = checked_name(&"n".repeat(MAX_NAME_CHARS + 50)).unwrap();
        assert_eq!(long.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_crowded_manifests_directory_stops_at_the_entry_bound() {
        let prefix = test_prefix("a_crowded_manifests_directory_stops_at_the_entry_bound");
        let drive = prefix.join("drive_c");
        fs::create_dir_all(drive.join("Games/Real")).unwrap();
        let manifests = drive.join(EPIC_MANIFESTS);
        fs::create_dir_all(&manifests).unwrap();
        // Entries that are not manifests still consume the scan allowance.
        for index in 0..(MAX_SCAN_ENTRIES + 16) {
            fs::write(manifests.join(format!("filler-{index}.txt")), b"").unwrap();
        }
        epic_manifest(
            &prefix,
            "Real.item",
            &epic_item("RealId", "Real", "C:\\Games\\Real"),
        );
        let library = discover(&prefix).unwrap();
        assert!(library.truncated);
        fs::remove_dir_all(prefix).unwrap();
    }
}
