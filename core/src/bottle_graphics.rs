//! Bottle-scoped renderer selection. Switching changes one small Caskrun
//! preference while idle; it never replaces an application's DLLs or registry.

use super::*;

const SETTINGS: &str = "graphics.json";
const SETTINGS_LIMIT: u64 = 4096;
const AUTO_SETTINGS: &str = "auto-graphics.json";
const AUTO_TARGET_LIMIT: usize = 32;
const AUTO_IDLE_TIMEOUT: Duration = Duration::from_millis(250);
const IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const DXMT_ROOT: &str = "components/dxmt/runtime";
// Original x64/x86 lifecycle and all three spatial-scale gates pass with the
// exact reviewed package. This does not establish game or image-quality support.
// See docs/DXMT_OPTIONS.md for the reproducible evidence boundary.
const SPATIAL_VALIDATED: bool = true;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Backend {
    #[default]
    Auto,
    Wined3d,
    Dxmt,
}

impl Backend {
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

/// A local, derived decision, not a game-compatibility or performance claim.
/// The portable preference remains `auto`; only an idle launch may refresh
/// this decision. Every DXMT use still requires the reviewed package proof.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AutoReason {
    D3d11Detected,
    NoSupportedEvidence,
    MixedOrIncompleteEvidence,
    EngineUnavailable,
    NotYetScanned,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AutoSettings {
    version: u32,
    effective_backend: Backend,
    reason: AutoReason,
}

impl AutoSettings {
    fn baseline(reason: AutoReason) -> Self {
        Self {
            version: 1,
            effective_backend: Backend::Wined3d,
            reason,
        }
    }
}

/// Bounded, nonblocking, no-follow reads also reject a concurrent rename or
/// modification. Neither a restored cache nor an arbitrary path can select a
/// runtime: this file carries only two known backends and closed reason codes.
fn read_auto_settings(dir: &Path) -> Result<(Option<AutoSettings>, Option<Vec<u8>>), String> {
    let bytes = read_small_file(&dir.join(AUTO_SETTINGS), SETTINGS_LIMIT)?;
    let Some(bytes) = bytes else {
        return Ok((None, None));
    };
    let settings: AutoSettings = serde_json::from_slice(&bytes).map_err(|_| {
        "Invalid automatic graphics decision; review the bottle's graphics setting".to_owned()
    })?;
    if settings.version != 1
        || settings.effective_backend == Backend::Auto
        || (settings.effective_backend == Backend::Dxmt)
            != (settings.reason == AutoReason::D3d11Detected)
    {
        return Err("Unsupported automatic graphics decision".into());
    }
    Ok((Some(settings), Some(bytes)))
}

fn read_small_file(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let before = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not inspect automatic graphics metadata".into()),
    };
    if !before.is_file() || before.len() > limit {
        return Err("Automatic graphics metadata must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(path)
        .map_err(|_| "Could not safely open automatic graphics metadata".to_owned())?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Automatic graphics metadata changed while opening".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(path).map_err(|e| e.to_string())?,
        )
    {
        return Err("Automatic graphics metadata changed while reading".into());
    }
    Ok(Some(bytes))
}

fn effective_backend(dir: &Path, selected: Backend) -> Result<Backend, String> {
    if selected != Backend::Auto {
        return Ok(selected);
    }
    Ok(read_auto_settings(dir)?
        .0
        .map_or(Backend::Wined3d, |saved| saved.effective_backend))
}

#[derive(Default)]
struct AutoEvidence {
    supported: bool,
    incomplete: bool,
}

impl AutoEvidence {
    fn include(&mut self, probe: &graphics_probe::Probe) {
        if probe.supports_dxmt() {
            self.supported = true;
        } else {
            self.incomplete = true;
        }
    }

    fn decision(self, dxmt_available: bool) -> AutoSettings {
        if !dxmt_available {
            return AutoSettings::baseline(AutoReason::EngineUnavailable);
        }
        if self.incomplete {
            return AutoSettings::baseline(AutoReason::MixedOrIncompleteEvidence);
        }
        if self.supported {
            return AutoSettings {
                version: 1,
                effective_backend: Backend::Dxmt,
                reason: AutoReason::D3d11Detected,
            };
        }
        AutoSettings::baseline(AutoReason::NoSupportedEvidence)
    }
}

/// Inspect only store-declared installed game directories and registered EXEs.
/// No account files, network requests, registry overrides or app DLL writes.
/// A mixed bottle remains on WineD3D: its clients must share one engine view.
fn automatic_evidence(dir: &Path) -> Result<AutoEvidence, String> {
    use std::collections::BTreeSet;
    let prefix = dir.join("prefix");
    let mut targets = BTreeSet::new();
    let mut evidence = AutoEvidence::default();
    match steam_library::discover(&prefix) {
        Ok(library) => {
            evidence.incomplete |= library.truncated;
            for game in library.games {
                // Steam's shared prerequisite package is not a game and has no
                // meaningful renderer. It must not mask actual game imports.
                if game.app_id == 228980 {
                    continue;
                }
                targets.insert(PathBuf::from(game.install_path));
            }
        }
        Err(_) => evidence.incomplete = true,
    }
    match publisher_library::discover(&prefix) {
        Ok(library) => {
            evidence.incomplete |= library.truncated;
            for game in library.games {
                targets.insert(PathBuf::from(game.install_path));
            }
        }
        Err(_) => evidence.incomplete = true,
    }
    match read_small_file(&dir.join("launchers.json"), 1024 * 1024) {
        Ok(Some(bytes)) => match serde_json::from_slice::<Vec<Launcher>>(&bytes) {
            Ok(launchers) if launchers.len() <= 1024 => {
                for launcher in launchers {
                    let path = PathBuf::from(launcher.path);
                    let name = path
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    // Store clients are not evidence about their games. In
                    // particular, Steam's x86 client must not mask an installed
                    // x64 D3D11 game. These names never establish compatibility.
                    if [
                        "steam.exe",
                        "epicgameslauncher.exe",
                        "galaxyclient.exe",
                        "battle.net.exe",
                        "battle.net launcher.exe",
                        "eadesktop.exe",
                        "ubisoftconnect.exe",
                        "uplay.exe",
                        "itch.exe",
                        "rockstargameslauncher.exe",
                    ]
                    .contains(&name.as_str())
                    {
                        continue;
                    }
                    if path.is_absolute()
                        && path
                            .extension()
                            .and_then(OsStr::to_str)
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                        && !targets.iter().any(|root| path.starts_with(root))
                    {
                        targets.insert(path);
                    } else if !path.is_absolute() {
                        evidence.incomplete = true;
                    }
                }
            }
            _ => evidence.incomplete = true,
        },
        Ok(None) => {}
        Err(_) => evidence.incomplete = true,
    }
    if targets.len() > AUTO_TARGET_LIMIT {
        evidence.incomplete = true;
    }
    for target in targets.into_iter().take(AUTO_TARGET_LIMIT) {
        cancellation::check()?;
        match graphics_probe::scan(&target) {
            Ok(probe) => evidence.include(&probe),
            Err(_) => evidence.incomplete = true,
        }
    }
    cancellation::check()?;
    Ok(evidence)
}

fn automatic_idle(dir: &Path, wine: &Path) -> Result<(), String> {
    refuse_running_bottle(dir)?;
    refuse_live_wine_server(dir, AUTO_IDLE_TIMEOUT, Some(wine))?;
    prefix_processes::wait_until_idle(dir, AUTO_IDLE_TIMEOUT)?;
    refuse_running_bottle(dir)
}

/// Caller holds this bottle's lock and has verified/migrated its runtime.
/// Read-only consumers and native aliases never rescan. A running session keeps
/// its exact recorded view, including the legacy WineD3D default when no local
/// decision exists yet. Newly installed games are considered on the next idle
/// startup, never by changing the Wine/DXMT view under an active client.
pub(super) fn prepare_automatic(home: &Path, id: &str, wine: &Path) -> Result<(), String> {
    let dir = bottle_dir(home, id)?;
    prepare_automatic_with(&dir, wine, automatic_idle, automatic_evidence, dxmt_support)
}

fn prepare_automatic_with(
    dir: &Path,
    wine: &Path,
    idle: impl Fn(&Path, &Path) -> Result<(), String>,
    evidence: impl FnOnce(&Path) -> Result<AutoEvidence, String>,
    support: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let (preferences, preference_bytes) = read_settings(dir)?;
    if preferences.backend != Backend::Auto {
        return Ok(());
    }
    let (previous, previous_bytes) = read_auto_settings(dir)?;
    let (directory_guard, directory_snapshot) = open_stable_directory(dir)?;
    let prefix = dir.join("prefix");
    let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
    if idle(dir, wine).is_err() {
        cancellation::check()?;
        return Ok(());
    }
    let available = support(dir, wine).is_ok();
    let updated = if available {
        evidence(dir)?.decision(true)
    } else {
        AutoSettings::baseline(AutoReason::EngineUnavailable)
    };
    cancellation::check()?;
    if previous == Some(updated) {
        return Ok(());
    }
    // The scan can take time. Prove idle and both directory identities again
    // immediately before publishing a decision any later client will consume.
    if idle(dir, wine).is_err() {
        cancellation::check()?;
        return Ok(());
    }
    bottle_control::assert_same_prefix(dir, &directory_guard, &directory_snapshot)?;
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    if read_settings(dir)?.1 != preference_bytes || read_auto_settings(dir)?.1 != previous_bytes {
        return Err("Bottle graphics changed during automatic detection; try again".into());
    }
    let bytes = serde_json::to_vec_pretty(&updated).map_err(|e| e.to_string())?;
    write_preference(&dir.join(AUTO_SETTINGS), &bytes)?;
    if read_auto_settings(dir)?.0 != Some(updated) {
        return Err("Automatic graphics decision changed during verification".into());
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    backend: Backend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dxmt_frame_rate: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dxmt_spatial_scale: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Preferences {
    backend: Backend,
    frame_rate: u16,
    spatial_scale: u16,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            frame_rate: 0,
            spatial_scale: 100,
        }
    }
}

fn validate_options(frame_rate: u16, spatial_scale: u16) -> Result<(), String> {
    if frame_rate > 240 {
        return Err(
            "Choose an application-controlled frame rate (0), or a target from 1 to 240 FPS".into(),
        );
    }
    if ![100, 125, 150, 200].contains(&spatial_scale) {
        return Err("Choose MetalFX off (100), 1.25× (125), 1.5× (150), or 2× (200)".into());
    }
    if spatial_scale != 100 && !SPATIAL_VALIDATED {
        return Err("MetalFX spatial scaling has not passed this engine's validation gates".into());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub(super) struct GraphicsResult {
    bottle_id: String,
    selected_backend: Backend,
    effective_backend: Backend,
    dxmt_available: bool,
    dxmt_frame_rate: u16,
    dxmt_spatial_scale: u16,
    dxmt_spatial_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    automatic_reason: Option<AutoReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn read_settings(dir: &Path) -> Result<(Preferences, Option<Vec<u8>>), String> {
    let path = dir.join(SETTINGS);
    let before = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Preferences::default(), None));
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
    let preferences = match settings.version {
        1 => {
            // A legacy record predates these fields. Even a null claim is not
            // a version-1 setting; reject it before copy/import creates storage.
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value.get("dxmt_frame_rate").is_some() || value.get("dxmt_spatial_scale").is_some() {
                return Err("Version-1 graphics preferences cannot contain DXMT options".into());
            }
            Preferences {
                backend: settings.backend,
                ..Preferences::default()
            }
        }
        2 => {
            let frame_rate = settings
                .dxmt_frame_rate
                .ok_or("Graphics preferences are missing their DXMT frame target")?;
            let spatial_scale = settings
                .dxmt_spatial_scale
                .ok_or("Graphics preferences are missing their MetalFX setting")?;
            validate_options(frame_rate, spatial_scale)?;
            Preferences {
                backend: settings.backend,
                frame_rate,
                spatial_scale,
            }
        }
        _ => return Err("Unsupported bottle graphics preference version".into()),
    };
    Ok((preferences, Some(bytes)))
}

// The complete reviewed package identity covers both renderer views, the
// dispatcher, receipts and source notices. A manifest by itself is not proof
// that a random/custom Wine runtime supports Caskrun's selection protocol.
fn dxmt_support(dir: &Path, wine: &Path) -> Result<(), String> {
    let record = recorded_runtime_for_prefix(dir, Some(wine))?;
    let identity = match record {
        Some(record)
            if record
                .runtime_identity
                .as_deref()
                .is_some_and(runtime_migration::is_reviewed_switchable_runtime) =>
        {
            if !runtime_record_matches(&record, wine)? {
                return Err("This bottle uses a different Windows engine".into());
            }
            record.runtime_identity.unwrap()
        }
        Some(record) if record.runtime_identity.is_some() => {
            // A verified but non-switchable package cannot acquire DXMT by
            // rehashing the same tree. This is a conservative negative result,
            // not an authorization based on a writable record. Normal launch
            // has already verified/upgraded the record before Auto runs; a
            // tampered identity still fails that ordinary runtime verification.
            // Legacy path-pinned records without an identity retain the full
            // attestation path below.
            return Err("DXMT is not included in this Windows engine package".into());
        }
        _ => {
            runtime_identity::bundled_runtime_attestation(wine)?
                .ok_or("DXMT requires Caskrun's verified graphics engine package")?
                .identity
        }
    };
    if !runtime_migration::is_reviewed_switchable_runtime(&identity) {
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

fn result(dir: &Path, id: &str, preferences: Preferences, wine: Option<&Path>) -> GraphicsResult {
    let selected = preferences.backend;
    let support = wine
        .ok_or_else(|| "The Windows engine is unavailable".to_owned())
        .and_then(|wine| dxmt_support(dir, wine));
    let available = support.is_ok();
    let automatic = if selected == Backend::Auto {
        Some(read_auto_settings(dir))
    } else {
        None
    };
    let automatic_error = automatic.as_ref().and_then(|value| value.as_ref().err());
    let automatic_settings = automatic
        .as_ref()
        .and_then(|value| value.as_ref().ok())
        .and_then(|value| value.0);
    let effective = automatic_settings.map_or_else(
        || {
            if selected == Backend::Auto {
                Backend::Wined3d
            } else {
                selected
            }
        },
        |value| value.effective_backend,
    );
    GraphicsResult {
        bottle_id: id.to_owned(),
        selected_backend: selected,
        effective_backend: effective,
        dxmt_available: available,
        dxmt_frame_rate: preferences.frame_rate,
        dxmt_spatial_scale: preferences.spatial_scale,
        dxmt_spatial_available: available && SPATIAL_VALIDATED,
        automatic_reason: (selected == Backend::Auto)
            .then(|| automatic_settings.map_or(AutoReason::NotYetScanned, |value| value.reason)),
        unavailable_reason: support.err(),
        warning: if automatic_error.is_some() {
            Some("The automatic graphics decision could not be read. Review this bottle's graphics setting before opening apps.".into())
        } else if effective == Backend::Dxmt {
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
    let (preferences, _) = read_settings(&dir)?;
    Ok(result(&dir, id, preferences, wine_path().as_deref()))
}

/// Pure, bounded preference read shared with privacy-safe diagnostics. It does
/// not attest, initialize or start Wine, and emits only the closed backend enum.
pub(super) fn selected_backend(dir: &Path) -> Result<Backend, String> {
    read_settings(dir).map(|(preferences, _)| preferences.backend)
}

/// The same local decision used by launch and native aliases, with no scan,
/// attestation, process startup or path-bearing output. Diagnostics must not
/// describe Automatic as WineD3D after an idle launch has resolved it to DXMT.
pub(super) fn resolved_backend(dir: &Path) -> Result<Backend, String> {
    effective_backend(dir, selected_backend(dir)?)
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
    let result = (|| {
        let (mut preferences, original) = read_settings(&bottle_dir(home, id)?)?;
        preferences.backend = backend;
        set_locked(home, id, preferences, original, wine)
    })();
    if result.is_err()
        && let Some(backup) = migration
    {
        backup.restore().map_err(|error| format!("Graphics update failed and its previous runtime record could not be restored: {error}"))?;
    }
    result
}

pub(super) fn set_bottle_dxmt_options(
    home: &Path,
    id: &str,
    frame_rate: u16,
    spatial_scale: u16,
) -> Result<GraphicsResult, String> {
    validate_options(frame_rate, spatial_scale)?;
    let wine = wine_path().ok_or("The Windows engine is unavailable")?;
    set_options_with_wine(home, id, frame_rate, spatial_scale, &wine)
}

fn set_options_with_wine(
    home: &Path,
    id: &str,
    frame_rate: u16,
    spatial_scale: u16,
    wine: &Path,
) -> Result<GraphicsResult, String> {
    validate_options(frame_rate, spatial_scale)?;
    let _lock = lock_bottle(home, id)?;
    let dir = bottle_dir(home, id)?;
    let (mut preferences, original) = read_settings(&dir)?;
    if preferences.backend != Backend::Dxmt {
        return Err("Choose DXMT before changing its frame pacing or MetalFX options".into());
    }
    let migration = runtime_migration::maybe_migrate_bundled_runtime(home, id, wine)?;
    let result = (|| {
        dxmt_support(&dir, wine)?;
        preferences.frame_rate = frame_rate;
        preferences.spatial_scale = spatial_scale;
        set_locked(home, id, preferences, original, wine)
    })();
    if result.is_err()
        && let Some(backup) = migration
    {
        backup.restore().map_err(|error| format!("Graphics options update failed and its previous runtime record could not be restored: {error}"))?;
    }
    result
}

fn set_locked(
    home: &Path,
    id: &str,
    preferences: Preferences,
    expected_original: Option<Vec<u8>>,
    wine: &Path,
) -> Result<GraphicsResult, String> {
    let dir = bottle_dir(home, id)?;
    let prefix = dir.join("prefix");
    let (guard, snapshot) = open_stable_directory(&prefix)?;
    let (_, original) = read_settings(&dir)?;
    if original != expected_original {
        return Err("Bottle graphics changed before this update; refresh and try again".into());
    }
    if preferences.backend == Backend::Dxmt {
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
        version: 2,
        backend: preferences.backend,
        dxmt_frame_rate: Some(preferences.frame_rate),
        dxmt_spatial_scale: Some(preferences.spatial_scale),
    })
    .map_err(|e| e.to_string())?;
    write_preference(&dir.join(SETTINGS), &updated)?;
    if !read_settings(&dir).is_ok_and(|value| value.0 == preferences) {
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
    Ok(result(&dir, id, preferences, Some(wine)))
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
    match effective_backend(dir, read_settings(dir)?.0.backend)? {
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
    let (preferences, _) = read_settings(dir)?;
    let backend = effective_backend(dir, preferences.backend)?;
    command.env("CASKRUN_GRAPHICS_BACKEND", backend.value());
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
        configure_dxmt_options(command, preferences);
    }
    Ok(())
}

// NUL is Wine's empty device, so a trailing app-local config section cannot
// accidentally disable managed options. Existing config files stay untouched.
// Caskrun supplies bounded, typed values; inherited DXMT settings cannot turn a
// saved Off setting back on or inject extra translation options into this run.
fn configure_dxmt_options(command: &mut Command, preferences: Preferences) {
    // With both controls off preserve the reviewed baseline, including any
    // explicit user/environment or app-local DXMT configuration.
    if preferences.frame_rate == 0 && preferences.spatial_scale == 100 {
        return;
    }
    command.env("DXMT_CONFIG_FILE", "NUL");
    command.env(
        "DXMT_CONFIG",
        format!(
            "d3d11.preferredMaxFrameRate={};d3d11.metalSpatialUpscaleFactor={}.{:02};",
            preferences.frame_rate,
            preferences.spatial_scale / 100,
            preferences.spatial_scale % 100
        ),
    );
    command.env(
        "DXMT_METALFX_SPATIAL_SWAPCHAIN",
        if preferences.spatial_scale > 100 {
            "1"
        } else {
            "0"
        },
    );
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
        assert_eq!(read_settings(&dir).unwrap(), (Preferences::default(), None));
        assert_eq!(
            effective_backend(&dir, Backend::Auto).unwrap(),
            Backend::Wined3d
        );
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
        let status = result(
            &dir,
            "example",
            Preferences {
                backend: Backend::Dxmt,
                ..Preferences::default()
            },
            None,
        );
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
    fn graphics_capabilities_require_an_explicitly_reviewed_complete_package() {
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::SWITCHABLE_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::NATIVE_TEXT_EDITING_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::BOUNDED_NATIVE_UI_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::BUNDLED_STEAM_UI_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::SINGLE_STEAM_UI_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::STEAM_CLIENT_QUIT_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::STEAM_DOCK_REOPEN_WINE_11_IDENTITY
        ));
        assert!(runtime_migration::is_reviewed_switchable_runtime(
            runtime_migration::GUARDED_STEAM_DOCK_REOPEN_WINE_11_IDENTITY
        ));
        for identity in [
            "wine-11.0",
            "caskrun-switchable-graphics-v1",
            "sha256:unknown",
            "sha256:4d82b50b96eb5995a78d848fb67166979577cd98e36d7db72fbb08636f8f44c0",
            // Rebuilt prototypes were rejected before installation. Their
            // component receipts cannot promote them into approved engines.
            "sha256:0bb25bb7ae0bcc82bd2db993845e82d8d3cc047422f6acc6040e12587a775f35",
            "sha256:5be503a788c8c00e376904c77f4de0012c8faf508338a9a8d9720bd16f5375aa",
        ] {
            assert!(!runtime_migration::is_reviewed_switchable_runtime(identity));
        }
        for approved in [
            runtime_migration::REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            runtime_migration::BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            runtime_migration::BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            runtime_migration::SINGLE_STEAM_UI_WINE_11_IDENTITY,
            runtime_migration::CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
            runtime_migration::RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
            runtime_migration::STEAM_CLIENT_QUIT_WINE_11_IDENTITY,
            runtime_migration::STEAM_DOCK_REOPEN_WINE_11_IDENTITY,
            runtime_migration::GUARDED_STEAM_DOCK_REOPEN_WINE_11_IDENTITY,
        ] {
            let mut altered = approved.to_owned();
            let original = altered.pop().unwrap();
            altered.push(if original == '0' { '1' } else { '0' });
            assert!(!runtime_migration::is_reviewed_switchable_runtime(&altered));
        }
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
                .0
                .backend,
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
                dxmt_frame_rate: None,
                dxmt_spatial_scale: None,
            },
        )
        .unwrap();
        let duplicated = duplicate_bottle_with_runtime(&home, &bottle.id, None, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &duplicated.id).unwrap())
                .unwrap()
                .0
                .backend,
            Backend::Dxmt
        );
        let backup = home.join("graphics.caskrun-bottle");
        export_bottle_with_runtime(&home, &bottle.id, &backup, None).unwrap();
        let imported = import_bottle(&home, &backup, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &imported.bottle.id).unwrap())
                .unwrap()
                .0
                .backend,
            Backend::Dxmt
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn legacy_preferences_default_options_and_reject_claimed_new_fields() {
        let dir = fixture();
        fs::write(dir.join(SETTINGS), r#"{"version":1,"backend":"dxmt"}"#).unwrap();
        assert_eq!(
            read_settings(&dir).unwrap().0,
            Preferences {
                backend: Backend::Dxmt,
                ..Preferences::default()
            }
        );
        for field in ["dxmt_frame_rate", "dxmt_spatial_scale"] {
            for value in ["null", "0", "100"] {
                fs::write(
                    dir.join(SETTINGS),
                    format!(r#"{{"version":1,"backend":"dxmt","{field}":{value}}}"#),
                )
                .unwrap();
                assert!(validate_portable_settings(&dir).is_err());
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_two_options_are_typed_complete_bounded_and_closed() {
        let dir = fixture();
        for value in [
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":30,"dxmt_spatial_scale":100}),
            serde_json::json!({"version":2,"backend":"auto","dxmt_frame_rate":240,"dxmt_spatial_scale":100}),
        ] {
            fs::write(dir.join(SETTINGS), serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(validate_portable_settings(&dir).is_ok());
        }
        for value in [
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":241,"dxmt_spatial_scale":100}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":-1,"dxmt_spatial_scale":100}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":"30","dxmt_spatial_scale":100}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":30,"dxmt_spatial_scale":101}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":30,"dxmt_spatial_scale":null}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":30}),
            serde_json::json!({"version":2,"backend":"dxmt","dxmt_frame_rate":30,"dxmt_spatial_scale":100,"extra":true}),
            serde_json::json!({"version":3,"backend":"auto","dxmt_frame_rate":0,"dxmt_spatial_scale":100}),
        ] {
            fs::write(dir.join(SETTINGS), serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(validate_portable_settings(&dir).is_err());
        }
        fs::write(dir.join(SETTINGS), br#"{"version":2,"backend":"dxmt","dxmt_frame_rate":30,"dxmt_frame_rate":60,"dxmt_spatial_scale":100}"#).unwrap();
        assert!(validate_portable_settings(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn launch_options_replace_inherited_managed_values_without_a_shell_or_file_write() {
        let mut command = Command::new("unused-fixture");
        command.env(
            "DXMT_CONFIG",
            "d3d11.preferredMaxFrameRate=999;untrusted.key=True;",
        );
        command.env("DXMT_CONFIG_FILE", "untrusted-file");
        command.env("DXMT_METALFX_SPATIAL_SWAPCHAIN", "1");
        configure_dxmt_options(
            &mut command,
            Preferences {
                backend: Backend::Dxmt,
                frame_rate: 30,
                spatial_scale: 100,
            },
        );
        let values: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert_eq!(
            values["DXMT_CONFIG"],
            "d3d11.preferredMaxFrameRate=30;d3d11.metalSpatialUpscaleFactor=1.00;"
        );
        assert_eq!(values["DXMT_CONFIG_FILE"], "NUL");
        assert_eq!(values["DXMT_METALFX_SPATIAL_SWAPCHAIN"], "0");
        configure_dxmt_options(
            &mut command,
            Preferences {
                backend: Backend::Dxmt,
                frame_rate: 0,
                spatial_scale: 150,
            },
        );
        let values: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert_eq!(
            values["DXMT_CONFIG"],
            "d3d11.preferredMaxFrameRate=0;d3d11.metalSpatialUpscaleFactor=1.50;"
        );
        assert_eq!(values["DXMT_METALFX_SPATIAL_SWAPCHAIN"], "1");
    }

    #[test]
    fn default_options_preserve_existing_dxmt_configuration_exactly() {
        let mut command = Command::new("unused-fixture");
        command.env("DXMT_CONFIG_FILE", "existing-local-config");
        command.env("DXMT_CONFIG", "dxgi.forceSDR=True;");
        command.env("DXMT_METALFX_SPATIAL_SWAPCHAIN", "custom-value");
        let before: Vec<_> = command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(|s| s.to_owned())))
            .collect();
        configure_dxmt_options(
            &mut command,
            Preferences {
                backend: Backend::Dxmt,
                ..Preferences::default()
            },
        );
        let after: Vec<_> = command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(|s| s.to_owned())))
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn changing_renderer_and_portable_copies_preserve_dxmt_options() {
        let home = fixture();
        let bottle = create(&home, "Portable graphics options").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(
            dir.join(SETTINGS),
            br#"{"version":2,"backend":"auto","dxmt_frame_rate":40,"dxmt_spatial_scale":100}"#,
        )
        .unwrap();
        let wine = home.join("wine");
        fs::write(&wine, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&wine, fs::Permissions::from_mode(0o755)).unwrap();
        let status = set_with_wine(&home, &bottle.id, Backend::Wined3d, &wine).unwrap();
        assert_eq!(
            (status.dxmt_frame_rate, status.dxmt_spatial_scale),
            (40, 100)
        );
        let duplicated = duplicate_bottle_with_runtime(&home, &bottle.id, None, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &duplicated.id).unwrap())
                .unwrap()
                .0,
            Preferences {
                backend: Backend::Wined3d,
                frame_rate: 40,
                spatial_scale: 100
            }
        );
        let backup = home.join("options.caskrun-bottle");
        export_bottle_with_runtime(&home, &bottle.id, &backup, None).unwrap();
        let imported = import_bottle(&home, &backup, None).unwrap();
        assert_eq!(
            read_settings(&bottle_dir(&home, &imported.bottle.id).unwrap())
                .unwrap()
                .0,
            Preferences {
                backend: Backend::Wined3d,
                frame_rate: 40,
                spatial_scale: 100
            }
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn invalid_options_or_non_dxmt_engine_preserve_exact_preference() {
        let home = fixture();
        let bottle = create(&home, "Refused graphics options").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let original = br#"{"version":1,"backend":"auto"}"#;
        fs::write(dir.join(SETTINGS), original).unwrap();
        assert!(set_options_with_wine(&home, &bottle.id, 241, 100, &home.join("wine")).is_err());
        assert!(set_options_with_wine(&home, &bottle.id, 30, 101, &home.join("wine")).is_err());
        assert!(set_options_with_wine(&home, &bottle.id, 30, 100, &home.join("wine")).is_err());
        assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), original);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn stale_options_do_not_replace_an_independent_preference_edit() {
        let home = fixture();
        let bottle = create(&home, "Concurrent graphics preference").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let current =
            br#"{"version":2,"backend":"auto","dxmt_frame_rate":40,"dxmt_spatial_scale":100}"#;
        fs::write(dir.join(SETTINGS), current).unwrap();
        let stale = Some(br#"{"version":1,"backend":"auto"}"#.to_vec());
        let error = set_locked(
            &home,
            &bottle.id,
            Preferences::default(),
            stale,
            &home.join("wine"),
        )
        .unwrap_err();
        assert!(error.contains("changed before this update"));
        assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), current);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn tampered_portable_options_are_refused_before_creating_bottle_storage() {
        let home = fixture();
        let bottle = create(&home, "Graphics import boundary").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        fs::write(
            dir.join(SETTINGS),
            br#"{"version":2,"backend":"auto","dxmt_frame_rate":40,"dxmt_spatial_scale":100}"#,
        )
        .unwrap();
        let backup = home.join("invalid-options.caskrun-bottle");
        export_bottle_with_runtime(&home, &bottle.id, &backup, None).unwrap();
        let count_before = fs::read_dir(home.join("bottles")).unwrap().count();
        fs::write(
            backup.join("payload").join(SETTINGS),
            br#"{"version":1,"backend":"dxmt","dxmt_frame_rate":30}"#,
        )
        .unwrap();
        assert!(import_bottle(&home, &backup, None).is_err());
        assert_eq!(
            fs::read_dir(home.join("bottles")).unwrap().count(),
            count_before
        );
        assert_eq!(read_settings(&dir).unwrap().0.frame_rate, 40);
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

    fn automatic_fixture() -> PathBuf {
        let dir = fixture();
        fs::create_dir_all(dir.join("prefix/drive_c")).unwrap();
        dir
    }

    fn save_automatic(dir: &Path, settings: AutoSettings) -> Vec<u8> {
        let bytes = serde_json::to_vec(&settings).unwrap();
        fs::write(dir.join(AUTO_SETTINGS), &bytes).unwrap();
        bytes
    }

    fn positive_evidence(_: &Path) -> Result<AutoEvidence, String> {
        Ok(AutoEvidence {
            supported: true,
            incomplete: false,
        })
    }

    fn d3d11_fixture_image() -> Vec<u8> {
        // A section-backed x64 PE with one D3D11 import. This is original
        // structural test data, never an executable downloaded from a game.
        let mut bytes = vec![0; 4096];
        let put16 = |bytes: &mut [u8], offset: usize, value: u16| {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        };
        let put32 = |bytes: &mut [u8], offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 0x84, 0x8664);
        put16(&mut bytes, 0x86, 1);
        put16(&mut bytes, 0x94, 240);
        put16(&mut bytes, 0x96, 2);
        put16(&mut bytes, 0x98, 0x20b);
        put32(&mut bytes, 0x98 + 60, 512);
        put32(&mut bytes, 0x98 + 108, 16);
        let section = 0x98 + 240;
        put32(&mut bytes, section + 8, 3584);
        put32(&mut bytes, section + 12, 0x1000);
        put32(&mut bytes, section + 16, 3584);
        put32(&mut bytes, section + 20, 512);
        let imports = 0x98 + 112 + 8;
        put32(&mut bytes, imports, 0x1000);
        put32(&mut bytes, imports + 4, 40);
        put32(&mut bytes, 512 + 12, 0x1600);
        bytes[2048..2058].copy_from_slice(b"d3d11.dll\0");
        bytes
    }

    #[test]
    fn automatic_reads_store_game_imports_and_registered_apps_without_client_guessing() {
        for source in ["steam", "epic", "registered"] {
            let dir = automatic_fixture();
            let drive = dir.join("prefix/drive_c");
            let game = if source == "steam" {
                let steam = drive.join("Program Files (x86)/Steam");
                let game = steam.join("steamapps/common/FixtureGame");
                fs::create_dir_all(&game).unwrap();
                fs::write(
                    steam.join("Steam.exe"),
                    b"store client is not graphics evidence",
                )
                .unwrap();
                fs::write(steam.join("steamapps/appmanifest_480.acf"), br#""AppState" { "appid" "480" "name" "Fixture Game" "installdir" "FixtureGame" "StateFlags" "4" }"#).unwrap();
                let prerequisites = steam.join("steamapps/common/Steamworks Shared");
                fs::create_dir_all(&prerequisites).unwrap();
                fs::write(
                    prerequisites.join("Prerequisite.exe"),
                    b"not a game or renderer",
                )
                .unwrap();
                fs::write(steam.join("steamapps/appmanifest_228980.acf"), br#""AppState" { "appid" "228980" "name" "Steamworks Common Redistributables" "installdir" "Steamworks Shared" "StateFlags" "4" }"#).unwrap();
                game
            } else {
                let game = drive.join("Games/FixtureGame");
                fs::create_dir_all(&game).unwrap();
                game
            };
            let exe = game.join("FixtureGame.exe");
            fs::write(&exe, d3d11_fixture_image()).unwrap();
            if source == "epic" {
                let manifests = drive.join("ProgramData/Epic/EpicGamesLauncher/Data/Manifests");
                fs::create_dir_all(&manifests).unwrap();
                fs::write(manifests.join("Fixture.item"), br#"{"AppName":"fixture-game","DisplayName":"Fixture Game","InstallLocation":"C:\\Games\\FixtureGame","bIsIncompleteInstall":false}"#).unwrap();
            }
            let mut launchers = vec![Launcher {
                id: "fixture".into(),
                name: "Fixture".into(),
                path: if source == "registered" {
                    exe
                } else {
                    drive.join("Program Files (x86)/Steam/Steam.exe")
                }
                .display()
                .to_string(),
                created_at: 0,
                hidden_from_home: false,
                saved_arguments: Vec::new(),
                working_directory: None,
            }];
            for (index, client) in ["uplay.exe", "Battle.net Launcher.exe"]
                .into_iter()
                .enumerate()
            {
                let mut launcher = launchers[0].clone();
                launcher.id = format!("store-fixture-{index}");
                launcher.path = drive
                    .join("StoreClients")
                    .join(client)
                    .display()
                    .to_string();
                launchers.push(launcher);
            }
            fs::write(
                dir.join("launchers.json"),
                serde_json::to_vec(&launchers).unwrap(),
            )
            .unwrap();
            let evidence = automatic_evidence(&dir).unwrap();
            assert!(
                evidence.supported,
                "Missing positive import evidence for {source}"
            );
            assert!(!evidence.incomplete, "Incomplete fixture for {source}");
            assert_eq!(evidence.decision(true).effective_backend, Backend::Dxmt);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn automatic_real_tracked_owner_keeps_session_without_rescan() {
        let dir = automatic_fixture();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let original = save_automatic(
            &dir,
            AutoSettings::baseline(AutoReason::NoSupportedEvidence),
        );
        prepare_automatic_with(
            &dir,
            &dir.join("wine"),
            automatic_idle,
            |_| panic!("Tracked live process must keep the session"),
            |_, _| panic!("Tracked live process must not select an engine"),
        )
        .unwrap();
        assert_eq!(fs::read(dir.join(AUTO_SETTINGS)).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automatic_selects_only_available_positive_unmixed_evidence() {
        assert_eq!(
            AutoEvidence {
                supported: true,
                incomplete: false
            }
            .decision(true)
            .effective_backend,
            Backend::Dxmt
        );
        for (supported, incomplete, available) in [
            (false, false, true),
            (true, true, true),
            (false, true, true),
            (true, false, false),
            (true, true, false),
            (false, false, false),
        ] {
            assert_eq!(
                AutoEvidence {
                    supported,
                    incomplete
                }
                .decision(available)
                .effective_backend,
                Backend::Wined3d
            );
        }
    }

    #[test]
    fn automatic_idle_decision_is_shared_by_reads_aliases_and_client_environment() {
        let dir = automatic_fixture();
        let wine = dir.join("engine/bin/wine");
        prepare_automatic_with(&dir, &wine, |_, _| Ok(()), positive_evidence, |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            effective_backend(&dir, Backend::Auto).unwrap(),
            Backend::Dxmt
        );
        let status = result(&dir, "fixture", Preferences::default(), None);
        assert_eq!(status.selected_backend, Backend::Auto);
        assert_eq!(status.effective_backend, Backend::Dxmt);
        assert_eq!(status.automatic_reason, Some(AutoReason::D3d11Detected));
        // A cached choice never grants access to an unreviewed engine view.
        assert!(selected_runtime_root(&dir, &wine).is_err());
        assert!(configure_launch(&mut Command::new("unused"), &dir, &wine).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automatic_active_session_keeps_recorded_view_without_scanning() {
        for saved in [
            None,
            Some(AutoSettings {
                version: 1,
                effective_backend: Backend::Dxmt,
                reason: AutoReason::D3d11Detected,
            }),
        ] {
            let dir = automatic_fixture();
            let original = saved.map(|value| save_automatic(&dir, value));
            prepare_automatic_with(
                &dir,
                &dir.join("wine"),
                |_, _| Err("busy".into()),
                |_| panic!("A live bottle must not be rescanned"),
                |_, _| panic!("A live bottle must not be reselected"),
            )
            .unwrap();
            assert_eq!(read_auto_settings(&dir).unwrap().1, original);
            assert_eq!(
                effective_backend(&dir, Backend::Auto).unwrap(),
                saved.map_or(Backend::Wined3d, |value| value.effective_backend)
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn automatic_does_not_publish_when_a_session_begins_during_detection() {
        use std::cell::Cell;
        let dir = automatic_fixture();
        let original = save_automatic(
            &dir,
            AutoSettings::baseline(AutoReason::NoSupportedEvidence),
        );
        let calls = Cell::new(0);
        prepare_automatic_with(
            &dir,
            &dir.join("wine"),
            |_, _| {
                let previous = calls.get();
                calls.set(previous + 1);
                if previous == 0 {
                    Ok(())
                } else {
                    Err("new live client".into())
                }
            },
            positive_evidence,
            |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read(dir.join(AUTO_SETTINGS)).unwrap(), original);
        assert_eq!(
            effective_backend(&dir, Backend::Auto).unwrap(),
            Backend::Wined3d
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manual_choice_ignores_cache_and_bypasses_automatic_detection() {
        let dir = automatic_fixture();
        fs::write(dir.join(AUTO_SETTINGS), b"invalid local cache").unwrap();
        for backend in [Backend::Wined3d, Backend::Dxmt] {
            fs::write(
                dir.join(SETTINGS),
                serde_json::to_vec(&Settings {
                    version: 1,
                    backend,
                    dxmt_frame_rate: None,
                    dxmt_spatial_scale: None,
                })
                .unwrap(),
            )
            .unwrap();
            let original = fs::read(dir.join(SETTINGS)).unwrap();
            prepare_automatic_with(
                &dir,
                &dir.join("wine"),
                |_, _| panic!("manual settings require no Auto idle probe"),
                |_| panic!("manual settings must not be scanned"),
                |_, _| panic!("manual settings must not be reselected"),
            )
            .unwrap();
            assert_eq!(effective_backend(&dir, backend).unwrap(), backend);
            assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), original);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automatic_cache_is_closed_bounded_and_never_follows_a_link() {
        let dir = automatic_fixture();
        for bytes in [
            br#"{"version":2,"effective_backend":"dxmt","reason":"d3d11-detected"}"#.as_slice(),
            br#"{"version":1,"effective_backend":"auto","reason":"not-yet-scanned"}"#.as_slice(),
            br#"{"version":1,"effective_backend":"dxmt","reason":"engine-unavailable"}"#.as_slice(),
            br#"{"version":1,"effective_backend":"dxvk","reason":"d3d11-detected"}"#.as_slice(),
            br#"{"version":1,"effective_backend":"dxmt","reason":"d3d11-detected","path":"elsewhere"}"#.as_slice(),
        ] {
            fs::write(dir.join(AUTO_SETTINGS), bytes).unwrap();
            assert!(read_auto_settings(&dir).is_err());
            assert!(effective_backend(&dir, Backend::Auto).is_err());
        }
        fs::write(dir.join(AUTO_SETTINGS), vec![b' '; 4097]).unwrap();
        assert!(read_auto_settings(&dir).is_err());
        fs::remove_file(dir.join(AUTO_SETTINGS)).unwrap();
        symlink("missing", dir.join(AUTO_SETTINGS)).unwrap();
        assert!(read_auto_settings(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unavailable_engine_never_scans_or_promotes_automatic_to_dxmt() {
        let dir = automatic_fixture();
        prepare_automatic_with(
            &dir,
            &dir.join("wine"),
            |_, _| Ok(()),
            |_| panic!("No reason to scan with an unavailable backend"),
            |_, _| Err("unreviewed engine".into()),
        )
        .unwrap();
        assert_eq!(
            read_auto_settings(&dir).unwrap().0,
            Some(AutoSettings::baseline(AutoReason::EngineUnavailable))
        );
        let mut command = Command::new("unused");
        configure_launch(&mut command, &dir, &dir.join("engine/bin/wine")).unwrap();
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == "CASKRUN_GRAPHICS_BACKEND"
                    && value == Some(OsStr::new("wined3d")))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_manual_setting_is_not_replaced_by_auto_detection() {
        let dir = automatic_fixture();
        let changed = br#"{"version":1,"backend":"wined3d"}"#;
        let error = prepare_automatic_with(
            &dir,
            &dir.join("wine"),
            |_, _| Ok(()),
            |_| {
                fs::write(dir.join(SETTINGS), changed).unwrap();
                positive_evidence(&dir)
            },
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("changed during automatic detection"));
        assert_eq!(fs::read(dir.join(SETTINGS)).unwrap(), changed);
        assert!(!dir.join(AUTO_SETTINGS).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_detection_keeps_the_previous_confirmed_decision() {
        let dir = automatic_fixture();
        let original = save_automatic(
            &dir,
            AutoSettings::baseline(AutoReason::NoSupportedEvidence),
        );
        assert!(
            prepare_automatic_with(
                &dir,
                &dir.join("wine"),
                |_, _| Ok(()),
                |_| Err("canceled fixture".into()),
                |_, _| Ok(())
            )
            .is_err()
        );
        assert_eq!(fs::read(dir.join(AUTO_SETTINGS)).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reading_automatic_graphics_never_scans_or_writes() {
        let dir = automatic_fixture();
        let before = fs::read_dir(&dir).unwrap().count();
        let status = result(&dir, "fixture", Preferences::default(), None);
        assert_eq!(status.automatic_reason, Some(AutoReason::NotYetScanned));
        assert_eq!(status.effective_backend, Backend::Wined3d);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), before);
        fs::remove_dir_all(dir).unwrap();
    }
}
