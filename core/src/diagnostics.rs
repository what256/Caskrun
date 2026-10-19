//! A deliberately small, shareable diagnostic summary. This is a projection
//! onto fixed fields, not a redacted copy of user records or Wine logs.
//!
//! Reporting never starts Wine, migrates a runtime, creates storage, or reads
//! log contents. Arbitrary source text, including errors, is never serialized.

use super::*;

const REPORT_FORMAT: &str = "caskrun-diagnostic-report";
const PROGRAM_BYTES: u64 = 1024 * 1024;
const SMALL_RECORD_BYTES: u64 = 64 * 1024;
const RECEIPT_BYTES: u64 = 1024 * 1024;
const MAX_PROGRAMS: usize = 10_000;
const MAX_LOG_ENTRIES: usize = 1024;
const REPORT_ERROR: &str = "Choose an existing bottle with readable, regular Caskrun metadata.";

// Only hashes already published as reviewed Caskrun packages may leave the
// device. A random digest in a writable record can itself carry user data.
const REVIEWED_IDENTITIES: &[&str] = &[
    "sha256:6df2a9f5dfaa119ba3d6c84214d79f885c70e887ef7f3bc9d2255aeb8b2f88e6",
    "sha256:07c5721ad17efa9d4ea89ff61522133407756c4aea8a47866625dcf118e7347c",
    "sha256:9dcab17f21490998ad03f6ee4f553da0992a1d9234dfe40efd82956b63fd47ce",
    runtime_migration::SWITCHABLE_WINE_11_IDENTITY,
];

#[derive(Debug, Serialize)]
pub(super) struct DiagnosticReport {
    format: &'static str,
    version: u32,
    core_version: &'static str,
    phase: &'static str,
    bottle: BottleSummary,
    runtime: RuntimeSummary,
    graphics: GraphicsSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    installer: Option<InstallerSummary>,
    logs: LogsSummary,
    issues: Vec<Issue>,
}

#[derive(Debug, Serialize)]
struct BottleSummary {
    initialization: Initialization,
    #[serde(skip_serializing_if = "Option::is_none")]
    saved_program_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible_program_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Initialization {
    Uninitialized,
    Initialized,
    Incomplete,
    Missing,
    Unavailable,
}

#[derive(Debug, Serialize)]
struct RuntimeSummary {
    availability: RuntimeAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recorded_identity: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_matches: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeAvailability {
    Missing,
    VerifiedBundle,
    Custom,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Renderer {
    Auto,
    Wined3d,
    Dxmt,
}

#[derive(Debug, Serialize)]
struct GraphicsSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_backend: Option<Renderer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_backend: Option<Renderer>,
    dxmt_available: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Catalog {
    Steam,
    EpicGamesStore,
    GogGalaxy,
    BattleNet,
    EaApp,
    UbisoftConnect,
    ItchIoApp,
    RockstarGamesLauncher,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstallerStage {
    Ready,
    Running,
    Failed,
    InstallerComplete,
    PreparingClient,
    Installed,
}

#[derive(Debug, Serialize)]
struct InstallerSummary {
    catalog_id: Catalog,
    stage: InstallerStage,
    has_issue: bool,
}

#[derive(Debug, Default, Serialize)]
struct LogsSummary {
    count: usize,
    scan_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<LogSummary>,
}

#[derive(Debug, Serialize)]
struct LogSummary {
    kind: LogKind,
    size_bytes: u64,
    age: LogAge,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum LogKind {
    Launch,
    Initialization,
    Resolution,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum LogAge {
    LessThanOneHour,
    LessThanOneDay,
    LessThanOneWeek,
    Older,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Issue {
    BottleChangedDuringReport,
    PrefixUnavailable,
    PrefixMissing,
    RegistryMissing,
    InitializationIncomplete,
    ProgramsUnreadable,
    ProgramLimitReached,
    RuntimeMissing,
    RuntimeUnavailable,
    RuntimeUnreviewed,
    RuntimeRecordUnreadable,
    RuntimeRecordMismatch,
    GraphicsUnreadable,
    DxmtUnavailable,
    ExperimentalGraphics,
    InstallerStateUnreadable,
    InstallerFailed,
    LogsUnreadable,
    LogsPartiallyScanned,
}

fn issue(issues: &mut Vec<Issue>, value: Issue) {
    if !issues.contains(&value) {
        issues.push(value);
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ReadFailure {
    UnsafeOrUnavailable,
    TooLarge,
    Changed,
    Invalid,
}

/// Every ancestor and the file itself must be real, and neither its handle nor
/// its path may change during the bounded read. Missing files are distinct
/// from corrupt files so defaults apply only when no preference exists.
fn json_file(path: &Path, limit: u64) -> Result<Option<serde_json::Value>, ReadFailure> {
    let before = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ReadFailure::UnsafeOrUnavailable),
    };
    if !before.file_type().is_file() {
        return Err(ReadFailure::UnsafeOrUnavailable);
    }
    if before.len() > limit {
        return Err(ReadFailure::TooLarge);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|_| ReadFailure::UnsafeOrUnavailable)?;
    if !same_snapshot(&before, &file.metadata().map_err(|_| ReadFailure::Changed)?) {
        return Err(ReadFailure::Changed);
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadFailure::UnsafeOrUnavailable)?;
    if bytes.len() as u64 > limit {
        return Err(ReadFailure::TooLarge);
    }
    if !same_snapshot(&before, &file.metadata().map_err(|_| ReadFailure::Changed)?)
        || !same_snapshot(
            &before,
            &fs::symlink_metadata(path).map_err(|_| ReadFailure::Changed)?,
        )
    {
        return Err(ReadFailure::Changed);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| ReadFailure::Invalid)
}

fn known_identity(value: Option<&str>) -> Option<&'static str> {
    value.and_then(|value| {
        REVIEWED_IDENTITIES
            .iter()
            .copied()
            .find(|known| *known == value)
    })
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RecordState {
    Absent,
    Valid,
    Unreadable,
}

fn record_state(dir: &Path) -> RecordState {
    match json_file(&dir.join("prefix-record.json"), SMALL_RECORD_BYTES) {
        Ok(None) => RecordState::Absent,
        // Match the runtime summary: serde would also accept an array form.
        Ok(Some(value)) if value.is_object() && PrefixRecord::deserialize(&value).is_ok() => {
            RecordState::Valid
        }
        Ok(Some(_)) | Err(_) => RecordState::Unreadable,
    }
}

fn initialization(dir: &Path, issues: &mut Vec<Issue>) -> Initialization {
    // The runtime record is written after Wine creates the registry, so a
    // valid record without Windows files means they were lost, not that the
    // bottle is new. A record that cannot be trusted proves neither.
    let record = record_state(dir);
    if record == RecordState::Unreadable {
        issue(issues, Issue::RuntimeRecordUnreadable);
    }
    let unavailable = |issues: &mut Vec<Issue>| {
        issue(issues, Issue::PrefixUnavailable);
        Initialization::Unavailable
    };
    let prefix = dir.join("prefix");
    match fs::symlink_metadata(&prefix) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match record {
            RecordState::Absent => Initialization::Uninitialized,
            RecordState::Valid => {
                issue(issues, Issue::PrefixMissing);
                Initialization::Missing
            }
            RecordState::Unreadable => Initialization::Unavailable,
        },
        Ok(metadata) if metadata.file_type().is_dir() => {
            let Ok((_guard, snapshot)) = open_stable_directory(&prefix) else {
                return unavailable(issues);
            };
            let mut present = [false; 3];
            for (index, name) in ["system.reg", "user.reg", "userdef.reg"].iter().enumerate() {
                match fs::symlink_metadata(prefix.join(name)) {
                    Ok(metadata) if metadata.file_type().is_file() => present[index] = true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return unavailable(issues),
                }
            }
            if !fs::symlink_metadata(&prefix)
                .is_ok_and(|current| same_snapshot(&snapshot, &current))
            {
                return unavailable(issues);
            }
            let [system, user, defaults] = present;
            match (system, user || defaults, record) {
                (true, _, _) if user && defaults => Initialization::Initialized,
                (true, _, _) | (false, true, RecordState::Absent) => {
                    issue(issues, Issue::InitializationIncomplete);
                    Initialization::Incomplete
                }
                (false, false, RecordState::Absent) => Initialization::Uninitialized,
                // Launch refuses to rebuild these; retrying setup cannot help.
                (false, false, RecordState::Valid) => {
                    issue(issues, Issue::PrefixMissing);
                    Initialization::Missing
                }
                (false, true, RecordState::Valid) => {
                    issue(issues, Issue::RegistryMissing);
                    Initialization::Missing
                }
                (false, _, RecordState::Unreadable) => Initialization::Unavailable,
            }
        }
        _ => unavailable(issues),
    }
}

fn program_counts(dir: &Path, issues: &mut Vec<Issue>) -> (Option<usize>, Option<usize>) {
    match json_file(&dir.join("launchers.json"), PROGRAM_BYTES) {
        Ok(None) => (Some(0), Some(0)),
        Ok(Some(value)) => {
            let Some(programs) = value.as_array() else {
                issue(issues, Issue::ProgramsUnreadable);
                return (None, None);
            };
            if programs.len() > MAX_PROGRAMS {
                issue(issues, Issue::ProgramLimitReached);
                return (None, None);
            }
            let mut visible = 0;
            for program in programs {
                // Use the same complete shape the launcher reads, including
                // saved arguments. Only its visibility enters the summary;
                // names, identity, paths and arguments are dropped here.
                let launcher = program
                    .is_object()
                    .then(|| serde_json::from_value::<Launcher>(program.clone()))
                    .and_then(Result::ok);
                let Some(launcher) = launcher else {
                    issue(issues, Issue::ProgramsUnreadable);
                    return (None, None);
                };
                if !launcher.hidden_from_home {
                    visible += 1;
                }
            }
            (Some(programs.len()), Some(visible))
        }
        Err(_) => {
            issue(issues, Issue::ProgramsUnreadable);
            (None, None)
        }
    }
}

fn runtime_summary(dir: &Path, wine: Option<&Path>, issues: &mut Vec<Issue>) -> RuntimeSummary {
    runtime_summary_with_attestor(dir, wine, issues, |wine| {
        runtime_identity::bundled_runtime_attestation(wine)
    })
}

fn runtime_summary_with_attestor(
    dir: &Path,
    wine: Option<&Path>,
    issues: &mut Vec<Issue>,
    attest: impl FnOnce(&Path) -> Result<Option<runtime_identity::BundleAttestation>, String>,
) -> RuntimeSummary {
    let mut result = RuntimeSummary {
        availability: RuntimeAvailability::Missing,
        identity: None,
        recorded_identity: None,
        record_matches: None,
    };
    let record = match json_file(&dir.join("prefix-record.json"), SMALL_RECORD_BYTES) {
        Ok(Some(value)) if value.is_object() => match serde_json::from_value::<PrefixRecord>(value)
        {
            Ok(record) => {
                result.recorded_identity = known_identity(record.runtime_identity.as_deref());
                Some(record)
            }
            Err(_) => {
                issue(issues, Issue::RuntimeRecordUnreadable);
                None
            }
        },
        Ok(None) => None,
        Ok(Some(_)) | Err(_) => {
            issue(issues, Issue::RuntimeRecordUnreadable);
            None
        }
    };
    let Some(wine) = wine else {
        issue(issues, Issue::RuntimeMissing);
        return result;
    };
    // Classify the selected engine independently of any old bottle pin. A
    // path-only pin can refer to the reviewed bundle; an obsolete recorded
    // path cannot make a healthy selected engine unavailable. Full attestation
    // is read-only and bounded, and is done once rather than per record branch.
    let selected = match attest(wine) {
        Ok(Some(bundle)) => {
            result.availability = RuntimeAvailability::VerifiedBundle;
            result.identity = known_identity(Some(&bundle.identity));
            Some(bundle)
        }
        Ok(None) if is_usable_wine(wine) => {
            result.availability = RuntimeAvailability::Custom;
            None
        }
        _ => {
            result.availability = RuntimeAvailability::Unavailable;
            issue(issues, Issue::RuntimeUnavailable);
            return result;
        }
    };
    if let Some(record) = &record {
        let matches = if let Some(expected) = &record.runtime_identity {
            // The freshly computed complete identity also permits an exact
            // bundled engine to be relocated, matching ordinary launch rules.
            selected
                .as_ref()
                .is_some_and(|bundle| bundle.identity == *expected)
        } else {
            // Custom/legacy path-only pins retain exact canonical-path rules.
            // Failure to resolve the old path is a pin mismatch, not a failure
            // of the independently attested selected engine.
            match (
                Path::new(&record.wine_path).canonicalize(),
                wine.canonicalize(),
            ) {
                (Ok(recorded), Ok(current)) => recorded == current,
                _ => false,
            }
        };
        result.record_matches = Some(matches);
        if !matches {
            issue(issues, Issue::RuntimeRecordMismatch);
        }
    }
    if result.identity.is_none()
        && matches!(
            result.availability,
            RuntimeAvailability::VerifiedBundle | RuntimeAvailability::Custom
        )
    {
        issue(issues, Issue::RuntimeUnreviewed);
    }
    result
}

fn dxmt_available(wine: Option<&Path>, runtime: &RuntimeSummary) -> bool {
    if runtime.identity != Some(runtime_migration::SWITCHABLE_WINE_11_IDENTITY)
        || runtime.record_matches == Some(false)
    {
        return false;
    }
    let Some(root) = wine.and_then(Path::parent).and_then(Path::parent) else {
        return false;
    };
    let Ok(Some(receipt)) = json_file(&root.join("components.json"), RECEIPT_BYTES) else {
        return false;
    };
    if receipt["format"] != "caskrun-runtime-components"
        || receipt["version"] != 1
        || receipt["layout"] != "switchable-v1"
        || receipt["default_backend"] != "wined3d"
        || receipt["backend_roots"]["wined3d"] != "runtime"
        || receipt["backend_roots"]["dxmt"] != "components/dxmt/runtime"
        || receipt["game_validated"] != false
    {
        return false;
    }
    // The exact attestation covers the bytes. These checks also ensure the
    // selected layout has not disappeared since it was attested.
    ["x86_64", "i386"].into_iter().all(|architecture| {
        ["d3d11.dll", "dxgi.dll", "d3d10core.dll", "winemetal.dll"]
            .into_iter()
            .all(|dll| {
                let path = root
                    .join("components/dxmt/runtime/lib/wine")
                    .join(format!("{architecture}-windows"))
                    .join(dll);
                OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NOFOLLOW_ANY)
                    .open(path)
                    .is_ok_and(|file| file.metadata().is_ok_and(|metadata| metadata.is_file()))
            })
    })
}

fn graphics_summary(
    dir: &Path,
    wine: Option<&Path>,
    runtime: &RuntimeSummary,
    issues: &mut Vec<Issue>,
) -> GraphicsSummary {
    let selected = bottle_graphics::selected_backend(dir)
        .ok()
        .map(|backend| match backend {
            bottle_graphics::Backend::Auto => Renderer::Auto,
            bottle_graphics::Backend::Wined3d => Renderer::Wined3d,
            bottle_graphics::Backend::Dxmt => Renderer::Dxmt,
        });
    if selected.is_none() {
        issue(issues, Issue::GraphicsUnreadable);
    }
    let effective = bottle_graphics::resolved_backend(dir)
        .ok()
        .map(|backend| match backend {
            bottle_graphics::Backend::Auto => Renderer::Auto,
            bottle_graphics::Backend::Wined3d => Renderer::Wined3d,
            bottle_graphics::Backend::Dxmt => Renderer::Dxmt,
        });
    if effective.is_none() {
        issue(issues, Issue::GraphicsUnreadable);
    }
    let available = dxmt_available(wine, runtime);
    if matches!(effective, Some(Renderer::Dxmt)) {
        issue(
            issues,
            if available {
                Issue::ExperimentalGraphics
            } else {
                Issue::DxmtUnavailable
            },
        );
    }
    GraphicsSummary {
        selected_backend: selected,
        effective_backend: effective,
        dxmt_available: available,
    }
}

fn installer_summary(dir: &Path, id: &str, issues: &mut Vec<Issue>) -> Option<InstallerSummary> {
    #[derive(Deserialize)]
    struct Attempt {
        version: u32,
        bottle_id: String,
        catalog_id: Catalog,
        stage: InstallerStage,
        #[serde(default)]
        last_error: Option<String>,
    }
    match json_file(&dir.join("catalog-attempt.json"), 8 * 1024) {
        Ok(None) => None,
        Ok(Some(value)) if value.is_object() => match serde_json::from_value::<Attempt>(value) {
            Ok(attempt) if attempt.version == 1 && attempt.bottle_id == id => {
                if matches!(attempt.stage, InstallerStage::Failed) {
                    issue(issues, Issue::InstallerFailed);
                }
                Some(InstallerSummary {
                    catalog_id: attempt.catalog_id,
                    stage: attempt.stage,
                    has_issue: attempt.last_error.is_some(),
                })
            }
            _ => {
                issue(issues, Issue::InstallerStateUnreadable);
                None
            }
        },
        Ok(Some(_)) | Err(_) => {
            issue(issues, Issue::InstallerStateUnreadable);
            None
        }
    }
}

fn log_kind(name: &str) -> Option<LogKind> {
    if !is_log_name(name) {
        return None;
    }
    if name.starts_with("launch-") {
        Some(LogKind::Launch)
    } else if name.starts_with("wineboot-") {
        Some(LogKind::Initialization)
    } else {
        Some(LogKind::Resolution)
    }
}

fn log_age(modified: Option<SystemTime>) -> LogAge {
    let Some(age) = modified.and_then(|modified| SystemTime::now().duration_since(modified).ok())
    else {
        return LogAge::Unknown;
    };
    match age.as_secs() {
        0..3600 => LogAge::LessThanOneHour,
        3600..86400 => LogAge::LessThanOneDay,
        86400..604800 => LogAge::LessThanOneWeek,
        _ => LogAge::Older,
    }
}

fn logs_summary(dir: &Path, issues: &mut Vec<Issue>) -> LogsSummary {
    let mut summary = LogsSummary::default();
    let path = dir.join("logs");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return summary,
        Ok(metadata) if metadata.file_type().is_dir() => {}
        _ => {
            issue(issues, Issue::LogsUnreadable);
            return summary;
        }
    }
    let Ok((_guard, snapshot)) = open_stable_directory(&path) else {
        issue(issues, Issue::LogsUnreadable);
        return summary;
    };
    let Ok(entries) = fs::read_dir(&path) else {
        issue(issues, Issue::LogsUnreadable);
        return summary;
    };
    let mut latest_modified = None;
    for (index, entry) in entries.enumerate() {
        if index == MAX_LOG_ENTRIES {
            summary.scan_truncated = true;
            issue(issues, Issue::LogsPartiallyScanned);
            break;
        }
        let Ok(entry) = entry else {
            issue(issues, Issue::LogsUnreadable);
            continue;
        };
        let Some(kind) = entry.file_name().to_str().and_then(log_kind) else {
            continue;
        };
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            issue(issues, Issue::LogsUnreadable);
            continue;
        };
        if !metadata.file_type().is_file() {
            issue(issues, Issue::LogsUnreadable);
            continue;
        }
        summary.count += 1;
        let modified = metadata.modified().ok();
        if summary.latest.is_none() || modified > latest_modified {
            latest_modified = modified;
            summary.latest = Some(LogSummary {
                kind,
                size_bytes: metadata.len(),
                age: log_age(modified),
            });
        }
    }
    if !fs::symlink_metadata(path).is_ok_and(|current| same_snapshot(&snapshot, &current)) {
        // A changed or replaced directory cannot supply a trustworthy count.
        // Discard its observations rather than retaining outside metadata if
        // another process substituted a directory while it was being scanned.
        summary.count = 0;
        summary.latest = None;
        summary.scan_truncated = true;
        issue(issues, Issue::LogsPartiallyScanned);
    }
    summary
}

pub(super) fn bottle_diagnostics(home: &Path, id: &str) -> Result<DiagnosticReport, String> {
    report_with_wine(home, id, wine_path().as_deref())
}

fn report_with_wine(
    home: &Path,
    id: &str,
    wine: Option<&Path>,
) -> Result<DiagnosticReport, String> {
    // Hold all storage directory handles. O_NOFOLLOW_ANY refuses symlinks in
    // any ancestor, including a substituted data-home or bottle-storage path.
    let (_home_guard, _) = open_stable_directory(home).map_err(|_| REPORT_ERROR.to_owned())?;
    let root = storage_root(home, false).map_err(|_| REPORT_ERROR.to_owned())?;
    let (_root_guard, _) = open_stable_directory(&root).map_err(|_| REPORT_ERROR.to_owned())?;
    let dir = bottle_dir(home, id).map_err(|_| REPORT_ERROR.to_owned())?;
    let (_bottle_guard, snapshot) =
        open_stable_directory(&dir).map_err(|_| REPORT_ERROR.to_owned())?;
    let mut issues = Vec::new();
    let initialization = initialization(&dir, &mut issues);
    let (saved_program_count, visible_program_count) = program_counts(&dir, &mut issues);
    let runtime = runtime_summary(&dir, wine, &mut issues);
    let graphics = graphics_summary(&dir, wine, &runtime, &mut issues);
    let installer = installer_summary(&dir, id, &mut issues);
    let logs = logs_summary(&dir, &mut issues);
    if !fs::symlink_metadata(&dir).is_ok_and(|current| same_snapshot(&snapshot, &current)) {
        issue(&mut issues, Issue::BottleChangedDuringReport);
    }
    Ok(DiagnosticReport {
        format: REPORT_FORMAT,
        version: 1,
        core_version: env!("CARGO_PKG_VERSION"),
        phase: "development",
        bottle: BottleSummary {
            initialization,
            saved_program_count,
            visible_program_count,
        },
        runtime,
        graphics,
        installer,
        logs,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        home: PathBuf,
        bottle: Bottle,
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".build")
                .join(format!("diagnostic-test-{}", Uuid::new_v4()));
            let bottle = create(&home, "Private Fixture Account").unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            Self { home, bottle, dir }
        }

        fn report(&self) -> serde_json::Value {
            serde_json::to_value(report_with_wine(&self.home, &self.bottle.id, None).unwrap())
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.home).unwrap();
        }
    }

    fn has_issue(report: &serde_json::Value, value: &str) -> bool {
        report["issues"].as_array().unwrap().contains(&value.into())
    }

    #[test]
    fn automatic_diagnostics_share_the_recorded_view_without_scanning() {
        let fixture = Fixture::new();
        let cache = fixture.dir.join("auto-graphics.json");
        fs::write(
            &cache,
            br#"{"version":1,"effective_backend":"dxmt","reason":"d3d11-detected"}"#,
        )
        .unwrap();
        let before = fs::read(&cache).unwrap();
        let report = fixture.report();
        assert_eq!(report["graphics"]["selected_backend"], "auto");
        assert_eq!(report["graphics"]["effective_backend"], "dxmt");
        assert!(has_issue(&report, "dxmt_unavailable"));
        assert_eq!(fs::read(&cache).unwrap(), before);
        assert!(!report.to_string().contains("auto-graphics"));
        fs::write(&cache, b"invalid").unwrap();
        let report = fixture.report();
        assert!(report["graphics"]["effective_backend"].is_null());
        assert!(has_issue(&report, "graphics_unreadable"));
    }

    #[test]
    fn graphics_diagnostics_share_versioned_validation_without_exporting_options() {
        let fixture = Fixture::new();
        for (bytes, expected) in [
            (r#"{"version":1,"backend":"auto"}"#, Some("auto")),
            (
                r#"{"version":2,"backend":"dxmt","dxmt_frame_rate":40,"dxmt_spatial_scale":100}"#,
                Some("dxmt"),
            ),
            (
                r#"{"version":1,"backend":"dxmt","dxmt_frame_rate":30}"#,
                None,
            ),
            (
                r#"{"version":2,"backend":"auto","dxmt_frame_rate":241,"dxmt_spatial_scale":100}"#,
                None,
            ),
            (
                r#"{"version":2,"backend":"auto","dxmt_frame_rate":30,"dxmt_spatial_scale":100,"private_token":"fixture-secret"}"#,
                None,
            ),
            (
                r#"{"version":3,"backend":"auto","dxmt_frame_rate":0,"dxmt_spatial_scale":100}"#,
                None,
            ),
        ] {
            fs::write(fixture.dir.join("graphics.json"), bytes).unwrap();
            let report = fixture.report();
            assert_eq!(report["graphics"]["selected_backend"].as_str(), expected);
            assert_eq!(
                has_issue(&report, "graphics_unreadable"),
                expected.is_none()
            );
            let encoded = serde_json::to_string(&report).unwrap();
            for private in [
                "dxmt_frame_rate",
                "dxmt_spatial_scale",
                "fixture-secret",
                "private_token",
            ] {
                assert!(!encoded.contains(private));
            }
        }
    }

    #[test]
    fn recorded_bottle_without_windows_files_is_missing_not_uninitialized() {
        let fixture = Fixture::new();
        fs::write(
            fixture.dir.join("prefix-record.json"),
            br#"{"wine_path":"/fixture/wine"}"#,
        )
        .unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "missing");
        assert!(has_issue(&report, "prefix_missing"));
        assert_eq!(fs::read_dir(fixture.dir.join("prefix")).unwrap().count(), 0);

        fs::remove_dir(fixture.dir.join("prefix")).unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "missing");
        assert!(has_issue(&report, "prefix_missing"));
        assert!(!fixture.dir.join("prefix").exists());

        // A record that cannot be trusted is not reported as a valid,
        // initialized bottle whose files went missing.
        for record in [&b"not json"[..], br#"["wine_path"]"#] {
            fs::write(fixture.dir.join("prefix-record.json"), record).unwrap();
            let report = fixture.report();
            assert_eq!(report["bottle"]["initialization"], "unavailable");
            assert!(has_issue(&report, "runtime_record_unreadable"));
            assert!(!has_issue(&report, "prefix_missing"));
        }
        fs::remove_file(fixture.dir.join("prefix-record.json")).unwrap();
        fs::create_dir(fixture.dir.join("prefix-record.json")).unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "unavailable");
        assert!(has_issue(&report, "runtime_record_unreadable"));
        assert!(!has_issue(&report, "prefix_missing"));
        fs::remove_dir(fixture.dir.join("prefix-record.json")).unwrap();

        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "uninitialized");
        assert!(!has_issue(&report, "prefix_missing"));
        assert!(!fixture.dir.join("prefix").exists());
    }

    #[test]
    fn recorded_bottle_with_partial_registry_reports_missing_registry_not_retry() {
        let fixture = Fixture::new();
        let prefix = fixture.dir.join("prefix");
        fs::write(prefix.join("user.reg"), b"user").unwrap();
        fs::write(prefix.join("userdef.reg"), b"defaults").unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "incomplete");
        assert!(has_issue(&report, "initialization_incomplete"));

        fs::write(
            fixture.dir.join("prefix-record.json"),
            br#"{"wine_path":"/fixture/wine"}"#,
        )
        .unwrap();
        for remaining in [&["user.reg", "userdef.reg"][..], &["user.reg"][..]] {
            for name in ["user.reg", "userdef.reg"] {
                let path = prefix.join(name);
                if remaining.contains(&name) {
                    fs::write(&path, name).unwrap();
                } else if path.exists() {
                    fs::remove_file(&path).unwrap();
                }
            }
            let report = fixture.report();
            assert_eq!(report["bottle"]["initialization"], "missing");
            assert!(has_issue(&report, "registry_missing"));
            assert!(!has_issue(&report, "initialization_incomplete"));
            assert!(!prefix.join("system.reg").exists());
        }

        fs::write(prefix.join("system.reg"), b"system").unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "incomplete");
        assert!(!has_issue(&report, "registry_missing"));
        fs::write(prefix.join("userdef.reg"), b"defaults").unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "initialized");
        assert!(report["issues"].as_array().unwrap().iter().all(|value| {
            !value.as_str().unwrap().contains("missing") || value == "runtime_missing"
        }));
    }

    #[test]
    fn uninitialized_missing_runtime_report_is_small_and_does_not_create_storage() {
        let fixture = Fixture::new();
        let before = fs::read_dir(&fixture.dir).unwrap().count();
        let report = fixture.report();
        assert_eq!(report["format"], REPORT_FORMAT);
        assert_eq!(report["version"], 1);
        assert_eq!(report["phase"], "development");
        assert_eq!(report["bottle"]["initialization"], "uninitialized");
        assert_eq!(report["bottle"]["saved_program_count"], 0);
        assert_eq!(report["runtime"]["availability"], "missing");
        assert_eq!(report["graphics"]["selected_backend"], "auto");
        assert_eq!(report["graphics"]["effective_backend"], "wined3d");
        assert_eq!(report["graphics"]["dxmt_available"], false);
        assert!(has_issue(&report, "runtime_missing"));
        assert!(serde_json::to_vec(&report).unwrap().len() < 1024);
        assert_eq!(fs::read_dir(&fixture.dir).unwrap().count(), before);
        assert!(fixture.dir.join("prefix").is_dir());
        assert_eq!(fs::read_dir(fixture.dir.join("prefix")).unwrap().count(), 0);
        assert!(!fixture.dir.join("logs").exists());
    }

    #[test]
    fn preparing_steam_client_is_a_known_installer_stage() {
        let fixture = Fixture::new();
        fs::write(
            fixture.dir.join("catalog-attempt.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "bottle_id": fixture.bottle.id,
                "catalog_id": "steam", "stage": "preparing_client"
            }))
            .unwrap(),
        )
        .unwrap();
        let report = fixture.report();
        assert_eq!(report["installer"]["stage"], "preparing_client");
        assert_eq!(report["installer"]["has_issue"], false);
        assert!(!has_issue(&report, "installer_record_unreadable"));
    }

    #[test]
    fn report_projects_only_allowlisted_values_from_personal_records_and_never_reads_logs() {
        let fixture = Fixture::new();
        let secret = "PRIVATE_ACCOUNT_FIXTURE_TOKEN";
        let personal_path = format!("/Users/{secret}/Documents/Confidential.exe");
        fs::write(
            fixture.dir.join("launchers.json"),
            serde_json::to_vec(&serde_json::json!([
                {"id": secret, "name": secret, "path": personal_path,
                 "created_at": 123, "saved_arguments": ["--token", secret]},
                {"id": secret, "name": secret, "path": secret,
                 "created_at": 124, "hidden_from_home": true}
            ]))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.dir.join("catalog-attempt.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "bottle_id": fixture.bottle.id,
                "catalog_id": "gog-galaxy", "stage": "failed",
                "last_error": personal_path, "download_url": secret,
                "installer_path": secret, "installer_sha256": secret
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.dir.join("prefix-record.json"),
            serde_json::to_vec(&serde_json::json!({
                "wine_path": personal_path,
                "runtime_identity": format!("sha256:{}", "a".repeat(64)),
                "runtime_fingerprint": secret
            }))
            .unwrap(),
        )
        .unwrap();
        fs::create_dir(fixture.dir.join("logs")).unwrap();
        let log_id = Uuid::new_v4();
        let name = format!("launch-123-{log_id}.log");
        fs::write(fixture.dir.join("logs").join(&name), secret).unwrap();
        fs::write(fixture.dir.join("logs").join(secret), secret).unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["saved_program_count"], 2);
        assert_eq!(report["bottle"]["visible_program_count"], 1);
        assert_eq!(report["installer"]["catalog_id"], "gog-galaxy");
        assert_eq!(report["installer"]["stage"], "failed");
        assert_eq!(report["installer"]["has_issue"], true);
        assert_eq!(report["logs"]["count"], 1);
        assert_eq!(report["logs"]["latest"]["kind"], "launch");
        assert!(report["runtime"].get("recorded_identity").is_none());
        let text = serde_json::to_string(&report).unwrap();
        for forbidden in [
            secret,
            &personal_path,
            &fixture.bottle.id,
            &fixture.bottle.name,
            &fixture.home.display().to_string(),
            &name,
            &log_id.to_string(),
            &"a".repeat(64),
        ] {
            assert!(!text.contains(forbidden));
        }
        assert!(has_issue(&report, "installer_failed"));
    }

    #[test]
    fn corrupt_unknown_and_oversize_records_produce_fixed_issue_codes() {
        let fixture = Fixture::new();
        fs::write(fixture.dir.join("launchers.json"), b"{secret-corruption").unwrap();
        fs::write(fixture.dir.join("prefix-record.json"), b"[\"secret\"]").unwrap();
        fs::write(
            fixture.dir.join("graphics.json"),
            serde_json::to_vec(&serde_json::json!({"version": 1, "backend": "private-token"}))
                .unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.dir.join("catalog-attempt.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "bottle_id": fixture.bottle.id,
                "catalog_id": "private-token", "stage": "private-token"
            }))
            .unwrap(),
        )
        .unwrap();
        let report = fixture.report();
        for code in [
            "programs_unreadable",
            "runtime_record_unreadable",
            "graphics_unreadable",
            "installer_state_unreadable",
        ] {
            assert!(has_issue(&report, code));
        }
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-token")
        );
        let file = File::create(fixture.dir.join("launchers.json")).unwrap();
        file.set_len(PROGRAM_BYTES + 1).unwrap();
        assert!(has_issue(&fixture.report(), "programs_unreadable"));
        assert_eq!(
            json_file(&fixture.dir.join("launchers.json"), PROGRAM_BYTES).unwrap_err(),
            ReadFailure::TooLarge
        );
    }

    #[test]
    fn symlinked_records_logs_and_prefix_are_not_followed() {
        let fixture = Fixture::new();
        let outside = fixture.home.join("private-source");
        fs::write(&outside, b"PRIVATE_CONTENT_THAT_MUST_NOT_APPEAR").unwrap();
        for name in [
            "launchers.json",
            "prefix-record.json",
            "graphics.json",
            "catalog-attempt.json",
        ] {
            symlink(&outside, fixture.dir.join(name)).unwrap();
        }
        let private_dir = fixture.home.join("private-folder");
        fs::create_dir(&private_dir).unwrap();
        symlink(&private_dir, fixture.dir.join("logs")).unwrap();
        fs::remove_dir(fixture.dir.join("prefix")).unwrap();
        symlink(&private_dir, fixture.dir.join("prefix")).unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "unavailable");
        assert_eq!(report["logs"]["count"], 0);
        assert!(has_issue(&report, "prefix_unavailable"));
        assert!(has_issue(&report, "logs_unreadable"));
        assert!(has_issue(&report, "runtime_record_unreadable"));
        assert!(has_issue(&report, "graphics_unreadable"));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("PRIVATE_CONTENT")
        );
        assert_eq!(
            fs::read(&outside).unwrap(),
            b"PRIVATE_CONTENT_THAT_MUST_NOT_APPEAR"
        );
    }

    #[test]
    fn wrong_account_unknown_stage_or_version_never_claim_installation() {
        let fixture = Fixture::new();
        for (id, version, stage) in [
            (Uuid::new_v4().to_string(), 1, "installed"),
            (fixture.bottle.id.clone(), 2, "installed"),
            (fixture.bottle.id.clone(), 1, "private-stage-token"),
        ] {
            fs::write(
                fixture.dir.join("catalog-attempt.json"),
                serde_json::to_vec(&serde_json::json!({
                    "version": version, "bottle_id": id,
                    "catalog_id": "steam", "stage": stage
                }))
                .unwrap(),
            )
            .unwrap();
            let report = fixture.report();
            assert!(report.get("installer").is_none());
            assert!(has_issue(&report, "installer_state_unreadable"));
        }
    }

    #[test]
    fn requested_unavailable_dxmt_is_preserved_and_flagged_without_mutation() {
        let fixture = Fixture::new();
        let path = fixture.dir.join("graphics.json");
        let original = b"{\"version\":1,\"backend\":\"dxmt\"}";
        fs::write(&path, original).unwrap();
        let report = fixture.report();
        assert_eq!(report["graphics"]["selected_backend"], "dxmt");
        assert_eq!(report["graphics"]["effective_backend"], "dxmt");
        assert_eq!(report["graphics"]["dxmt_available"], false);
        assert!(has_issue(&report, "dxmt_unavailable"));
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn logs_are_metadata_only_and_directory_scan_is_bounded() {
        let fixture = Fixture::new();
        let logs = fixture.dir.join("logs");
        fs::create_dir(&logs).unwrap();
        let file = File::create(logs.join(format!("wineboot-{}.log", Uuid::new_v4()))).unwrap();
        file.set_len(8 * 1024 * 1024 * 1024).unwrap();
        for index in 0..MAX_LOG_ENTRIES + 1 {
            fs::write(logs.join(format!("ignored-{index}")), b"").unwrap();
        }
        let report = fixture.report();
        assert_eq!(report["logs"]["scan_truncated"], true);
        assert!(has_issue(&report, "logs_partially_scanned"));
        // The file is sparse and is never read; its size is useful metadata.
        if report["logs"]["count"] == 1 {
            assert_eq!(
                report["logs"]["latest"]["size_bytes"],
                8_u64 * 1024 * 1024 * 1024
            );
        }
    }

    #[test]
    fn too_many_programs_does_not_emit_a_misleading_partial_count() {
        let fixture = Fixture::new();
        let program = serde_json::json!({
            "id": "fixture", "name": "fixture", "path": "fixture", "created_at": 0
        });
        let bytes = serde_json::to_vec(&vec![program; MAX_PROGRAMS + 1]).unwrap();
        assert!((bytes.len() as u64) < PROGRAM_BYTES);
        fs::write(fixture.dir.join("launchers.json"), bytes).unwrap();
        let report = fixture.report();
        assert!(has_issue(&report, "program_limit_reached"));
        assert!(report["bottle"].get("saved_program_count").is_none());
    }

    #[test]
    fn malformed_saved_arguments_make_the_saved_app_record_unreadable() {
        let fixture = Fixture::new();
        for arguments in [
            serde_json::json!("private-invalid-arguments"),
            serde_json::json!([42]),
            serde_json::Value::Null,
        ] {
            fs::write(
                fixture.dir.join("launchers.json"),
                serde_json::to_vec(&serde_json::json!([{
                    "id": "fixture", "name": "fixture", "path": "fixture",
                    "created_at": 0, "saved_arguments": arguments
                }]))
                .unwrap(),
            )
            .unwrap();
            let report = fixture.report();
            assert!(has_issue(&report, "programs_unreadable"));
            assert!(report["bottle"].get("saved_program_count").is_none());
            assert!(
                !serde_json::to_string(&report)
                    .unwrap()
                    .contains("private-invalid")
            );
        }
    }

    fn selected_engine_fixture(fixture: &Fixture) -> PathBuf {
        // Controlled attestor tests exercise report classification independently
        // of the already-tested tree hasher. These fixture bytes are never
        // represented as a genuinely reviewed runtime or executed.
        let engine = fixture.home.join("selected-engine-fixture");
        fs::create_dir_all(engine.join("bin")).unwrap();
        for name in ["wine", "wineboot"] {
            let path = engine.join("bin").join(name);
            fs::write(&path, b"fixture, not an executable runtime").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        engine.join("bin/wine")
    }

    fn attested_v4() -> runtime_identity::BundleAttestation {
        runtime_identity::BundleAttestation {
            identity: runtime_migration::SWITCHABLE_WINE_11_IDENTITY.to_owned(),
            metadata_fingerprint: "fixture-only".to_owned(),
        }
    }

    #[test]
    fn path_only_pin_does_not_misclassify_a_verified_bundle_or_hide_dxmt() {
        let fixture = Fixture::new();
        let wine = selected_engine_fixture(&fixture);
        let engine = wine.parent().unwrap().parent().unwrap();
        fs::write(
            fixture.dir.join("prefix-record.json"),
            serde_json::to_vec(&serde_json::json!({"wine_path": wine})).unwrap(),
        )
        .unwrap();
        fs::write(
            engine.join("components.json"),
            serde_json::to_vec(&serde_json::json!({
                "format": "caskrun-runtime-components", "version": 1,
                "layout": "switchable-v1", "default_backend": "wined3d",
                "backend_roots": {"wined3d": "runtime", "dxmt": "components/dxmt/runtime"},
                "game_validated": false
            }))
            .unwrap(),
        )
        .unwrap();
        for architecture in ["x86_64", "i386"] {
            let dlls = engine
                .join("components/dxmt/runtime/lib/wine")
                .join(format!("{architecture}-windows"));
            fs::create_dir_all(&dlls).unwrap();
            for dll in ["d3d11.dll", "dxgi.dll", "d3d10core.dll", "winemetal.dll"] {
                fs::write(dlls.join(dll), b"fixture").unwrap();
            }
        }
        let mut issues = Vec::new();
        let mut calls = 0;
        let runtime = runtime_summary_with_attestor(&fixture.dir, Some(&wine), &mut issues, |_| {
            calls += 1;
            Ok(Some(attested_v4()))
        });
        assert_eq!(calls, 1);
        assert!(matches!(
            runtime.availability,
            RuntimeAvailability::VerifiedBundle
        ));
        assert_eq!(
            runtime.identity,
            Some(runtime_migration::SWITCHABLE_WINE_11_IDENTITY)
        );
        assert_eq!(runtime.record_matches, Some(true));
        assert!(issues.is_empty());
        assert!(dxmt_available(Some(&wine), &runtime));
    }

    #[test]
    fn broken_old_pin_does_not_report_a_healthy_selected_engine_as_unavailable() {
        let fixture = Fixture::new();
        let wine = selected_engine_fixture(&fixture);
        let old = fixture.home.join("removed-private-old-engine/wine");
        fs::write(
            fixture.dir.join("prefix-record.json"),
            serde_json::to_vec(&serde_json::json!({"wine_path": old})).unwrap(),
        )
        .unwrap();
        let mut issues = Vec::new();
        let runtime = runtime_summary_with_attestor(&fixture.dir, Some(&wine), &mut issues, |_| {
            Ok(Some(attested_v4()))
        });
        assert!(matches!(
            runtime.availability,
            RuntimeAvailability::VerifiedBundle
        ));
        assert_eq!(
            runtime.identity,
            Some(runtime_migration::SWITCHABLE_WINE_11_IDENTITY)
        );
        assert_eq!(runtime.record_matches, Some(false));
        assert_eq!(issues, [Issue::RuntimeRecordMismatch]);
        assert!(
            !serde_json::to_string(&runtime)
                .unwrap()
                .contains("removed-private")
        );
    }

    #[test]
    fn custom_engine_is_observed_without_executing_it_or_exporting_its_path() {
        let fixture = Fixture::new();
        let engine = fixture.home.join("private-custom-engine");
        fs::create_dir(&engine).unwrap();
        let sentinel = fixture.home.join("must-not-be-created");
        for name in ["wine", "wineboot"] {
            let path = engine.join(name);
            fs::write(
                &path,
                format!("#!/bin/sh\ntouch '{}'\nexit 1\n", sentinel.display()),
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let wine = engine.join("wine");
        fs::write(
            fixture.dir.join("prefix-record.json"),
            serde_json::to_vec(&serde_json::json!({"wine_path": wine})).unwrap(),
        )
        .unwrap();
        let report = serde_json::to_value(
            report_with_wine(&fixture.home, &fixture.bottle.id, Some(&wine)).unwrap(),
        )
        .unwrap();
        assert_eq!(report["runtime"]["availability"], "custom");
        assert_eq!(report["runtime"]["record_matches"], true);
        assert!(report["runtime"].get("identity").is_none());
        assert!(has_issue(&report, "runtime_unreviewed"));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-custom-engine")
        );
        assert!(!sentinel.exists());
        assert!(!fixture.dir.join("graphics.json").exists());
    }

    #[test]
    fn log_symlinks_never_contribute_metadata_or_names() {
        let fixture = Fixture::new();
        let outside = fixture.home.join("private-log-source");
        fs::write(&outside, b"PRIVATE_LOG_CONTENT").unwrap();
        let logs = fixture.dir.join("logs");
        fs::create_dir(&logs).unwrap();
        symlink(
            &outside,
            logs.join(format!("launch-123-{}.log", Uuid::new_v4())),
        )
        .unwrap();
        let report = fixture.report();
        assert_eq!(report["logs"]["count"], 0);
        assert!(report["logs"].get("latest").is_none());
        assert!(has_issue(&report, "logs_unreadable"));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-log-source")
        );
    }

    #[test]
    fn missing_bottle_or_symlinked_storage_returns_only_a_fixed_safe_error() {
        let fixture = Fixture::new();
        assert_eq!(
            report_with_wine(&fixture.home, "PRIVATE_INVALID_ID", None).unwrap_err(),
            REPORT_ERROR
        );
        let alias = fixture.home.join("alias");
        symlink(&fixture.home, &alias).unwrap();
        assert_eq!(
            report_with_wine(&alias, &fixture.bottle.id, None).unwrap_err(),
            REPORT_ERROR
        );
    }

    #[test]
    fn prefix_summary_distinguishes_partial_and_initialized_without_reading_registry_text() {
        let fixture = Fixture::new();
        let prefix = fixture.dir.join("prefix");
        fs::write(prefix.join("system.reg"), b"PRIVATE_REGISTRY_CONTENT").unwrap();
        assert_eq!(fixture.report()["bottle"]["initialization"], "incomplete");
        fs::write(prefix.join("user.reg"), b"PRIVATE_REGISTRY_CONTENT").unwrap();
        fs::write(prefix.join("userdef.reg"), b"PRIVATE_REGISTRY_CONTENT").unwrap();
        let report = fixture.report();
        assert_eq!(report["bottle"]["initialization"], "initialized");
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("PRIVATE_REGISTRY_CONTENT")
        );
    }
}
