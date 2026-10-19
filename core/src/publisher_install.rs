//! EA/Ubisoft adaptation: MacadeKit LauncherStore.swift, revision
//! 37bb046423dbb85c5cb98fee742868dccb96002d, Copyright 2026 Steale LLC,
//! Apache-2.0. Modified for Caskrun: bounded no-follow reads, strict metadata,
//! same-bottle identity and cancellation. See third-party/mapc-engine/.
//!
//! Independent, experimental Windows installer recipes for EA app and Ubisoft Connect.
//! Only setup and launcher registration are automated. Account sign-in, updates,
//! game downloads, and gameplay have not been validated in this Wine build.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

// EA version-folder lookup and Chromium client switches are adapted from
// MacadeKit LauncherStore.swift (MaPC), Apache-2.0, commit
// 37bb046423dbb85c5cb98fee742868dccb96002d. See THIRD_PARTY_NOTICES.md.
// Caskrun adds bounded numeric selection, checked PE files and no-follow opens.
const MAX_EA_VERSION_ENTRIES: usize = 256;
const EA_ROOTS: &[&str] = &[
    "Program Files/Electronic Arts/EA Desktop",
    "Program Files (x86)/Electronic Arts/EA Desktop",
];
const EA_CLIENT_SWITCHES: &[&str] = &["--disable-gpu", "--disable-gpu-compositing"];

pub(super) const EA_ID: &str = "ea-app";
pub(super) const UBISOFT_ID: &str = "ubisoft-connect";

// The EA URL is the Windows href currently published on https://www.ea.com/ea-app.
// Ubisoft's publisher CDN serves its Windows Connect installer at this stable
// path. Any host/path change must be reviewed before downloads resume.
const EA_INSTALLER_URL: &str = "https://origin-a.akamaihd.net/EA-Desktop-Client-Download/installer-releases/EAappInstaller.exe";
const UBISOFT_INSTALLER_URL: &str =
    "https://static3.cdn.ubi.com/orbit/launcher_installer/UbisoftConnectInstaller.exe";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(12 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Copy)]
struct Profile {
    id: &'static str,
    name: &'static str,
    publisher: &'static str,
    url: &'static str,
    download_page: &'static str,
    installer_name: &'static str,
    min_bytes: u64,
    max_bytes: u64,
    launcher_candidates: &'static [&'static str],
    inspect: fn(&mut File) -> Result<String, String>,
}

const EA_CANDIDATES: &[&str] = &[
    "Program Files/Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe",
    "Program Files (x86)/Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe",
];
const UBISOFT_CANDIDATES: &[&str] = &[
    "Program Files (x86)/Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe",
    "Program Files/Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe",
    "Program Files (x86)/Ubisoft/Ubisoft Game Launcher/Uplay.exe",
    "Program Files/Ubisoft/Ubisoft Game Launcher/Uplay.exe",
];

const EA: Profile = Profile {
    id: EA_ID,
    name: "EA app",
    publisher: "EA",
    url: EA_INSTALLER_URL,
    download_page: "https://www.ea.com/ea-app",
    installer_name: "EAappInstaller.exe",
    min_bytes: 100 * 1024,
    max_bytes: 50 * 1024 * 1024,
    launcher_candidates: EA_CANDIDATES,
    inspect: inspect_ea_installer,
};
const UBISOFT: Profile = Profile {
    id: UBISOFT_ID,
    name: "Ubisoft Connect",
    publisher: "Ubisoft",
    url: UBISOFT_INSTALLER_URL,
    download_page: "https://www.ubisoft.com/en-us/ubisoft-connect/download",
    installer_name: "UbisoftConnectInstaller.exe",
    min_bytes: 1024 * 1024,
    max_bytes: 400 * 1024 * 1024,
    launcher_candidates: UBISOFT_CANDIDATES,
    inspect: inspect_ubisoft_installer,
};

fn profile(id: &str) -> Result<Profile, String> {
    match id {
        EA_ID => Ok(EA),
        UBISOFT_ID => Ok(UBISOFT),
        _ => Err(format!("No EA or Ubisoft installer is available for {id}")),
    }
}

pub(super) fn checked_installer_url(id: &str, url: &str) -> Result<(), String> {
    let expected = profile(id)?.url;
    if url != expected {
        return Err(
            "Publisher installer URL changed or points outside its reviewed host and path".into(),
        );
    }
    Ok(())
}

#[derive(Serialize)]
pub(super) struct PublisherCatalogInstallResult {
    bottle: Bottle,
    launcher: Launcher,
    installer_sha256: String,
    download_url: String,
    install_log_path: String,
    launch: LaunchResult,
}

struct DownloadedInstaller {
    path: PathBuf,
    sha256: String,
    download_url: String,
}

pub(super) fn install_catalog(
    home: &Path,
    id: &str,
) -> Result<PublisherCatalogInstallResult, String> {
    let profile = profile(id)?;
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let installer = download_installer(home, profile)?;
    install_from_download(home, &wine, profile, installer)
}

pub(super) fn retry_catalog(
    home: &Path,
    id: &str,
    bottle_id: &str,
) -> Result<PublisherCatalogInstallResult, String> {
    let profile = profile(id)?;
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    retry_with_wine(home, bottle_id, &wine, profile)
}

fn download_installer(home: &Path, profile: Profile) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    checked_installer_url(profile.id, profile.url)?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let token = Uuid::new_v4();
    let path = downloads.join(format!("{}-{token}.exe", profile.id));
    let log_path = downloads.join(format!("{}-{token}.download.log", profile.id));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| {
            format!(
                "Could not prepare {} installer download: {error}",
                profile.name
            )
        })?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare {} download log: {error}", profile.name))?;

    // -q first ignores ~/.curlrc. A redirect never becomes an arbitrary GET.
    let result = (|| -> Result<String, String> {
        let mut child = Command::new("/usr/bin/curl")
            .args([
                "-q",
                "--fail",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "15",
                "--max-time",
                "700",
                "--max-filesize",
                &profile.max_bytes.to_string(),
                profile.url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|error| {
                format!(
                    "Could not start {} installer download: {error}",
                    profile.name
                )
            })?;
        let status = wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err(format!(
                "{} installer download did not complete",
                profile.name
            ));
        }
        (profile.inspect)(&mut file)
    })();
    let sha256 = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&path);
            if cancellation::is_error(&error) {
                return Err(error);
            }
            return Err(format!(
                "Could not download a checked {} Windows installer: {error}. Check {} or use {}",
                profile.publisher,
                log_path.display(),
                profile.download_page
            ));
        }
    };
    Ok(DownloadedInstaller {
        path,
        sha256,
        download_url: profile.url.to_owned(),
    })
}

fn inspect_ea_installer(file: &mut File) -> Result<String, String> {
    inspect_installer(file, EA.min_bytes, EA.max_bytes)
}

fn inspect_ubisoft_installer(file: &mut File) -> Result<String, String> {
    inspect_installer(file, UBISOFT.min_bytes, UBISOFT.max_bytes)
}

fn inspect_installer(file: &mut File, min_bytes: u64, max_bytes: u64) -> Result<String, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(min_bytes..=max_bytes).contains(&length) {
        return Err("unexpected installer size".into());
    }
    pe_machine(file, length)?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn pe_machine(file: &mut File, length: u64) -> Result<u16, String> {
    if length < 70 {
        return Err("Windows executable is too short".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut dos = [0_u8; 64];
    file.read_exact(&mut dos).map_err(|e| e.to_string())?;
    if &dos[..2] != b"MZ" {
        return Err("missing Windows MZ header".into());
    }
    let offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    if offset < 64 || offset > length.saturating_sub(6) {
        return Err("invalid Windows PE header offset".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut pe = [0_u8; 6];
    file.read_exact(&mut pe).map_err(|e| e.to_string())?;
    let machine = u16::from_le_bytes([pe[4], pe[5]]);
    if &pe[..4] != b"PE\0\0" || !matches!(machine, 0x014c | 0x8664) {
        return Err("not an x86 or x64 Windows PE program".into());
    }
    Ok(machine)
}

/// Locate only a real, checked publisher client inside this bottle. EA's
/// installer may leave its current-version junction absent under Wine; search
/// its numeric version directories instead of treating setup as failed.
pub(super) fn installed_launcher(prefix: &Path, catalog_id: &str) -> Option<PathBuf> {
    launcher_in_prefix(prefix, profile(catalog_id).ok()?)
}

/// Recover only a missing, previously reviewed EA client pathname after an EA
/// self-update removed its version directory. Existing files, invalid clients,
/// symlinks and paths belonging to another bottle are never redirected.
/// The caller must retain the bottle lock and compare the saved record again
/// before publishing the returned path; this function changes no files.
pub(super) fn resolve_saved_ea_client(prefix: &Path, saved_path: &Path) -> Option<PathBuf> {
    let (_prefix_guard, prefix_snapshot) = open_stable_directory(prefix).ok()?;
    let real_prefix = prefix.canonicalize().ok()?;
    if !saved_path.is_absolute() {
        return None;
    }
    let relative = reviewed_client_relative(&real_prefix, saved_path, EA)?;
    if !relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    if !reviewed_path_is_missing(&real_prefix, &relative) {
        return None;
    }
    let recovered = installed_launcher(&real_prefix, EA_ID)?;
    if !is_reviewed_client(&real_prefix, &recovered, EA_ID)
        || !reviewed_path_is_missing(&real_prefix, &relative)
        || !fs::symlink_metadata(prefix)
            .is_ok_and(|current| same_identity(&prefix_snapshot, &current))
    {
        return None;
    }
    let (current_guard, current_snapshot) = open_stable_directory(prefix).ok()?;
    if !same_identity(&prefix_snapshot, &current_snapshot) {
        return None;
    }
    // Retain both anchored handles through the final validation.
    drop(current_guard);
    Some(recovered)
}

fn reviewed_path_is_missing(prefix: &Path, relative: &Path) -> bool {
    let mut current = prefix.join("drive_c");
    let Ok((drive_guard, drive_snapshot)) = open_stable_directory(&current) else {
        return false;
    };
    let mut guards = vec![(current.clone(), drive_guard, drive_snapshot)];
    let mut missing = false;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                break;
            }
            Err(_) => return false,
            Ok(metadata) => {
                // A final path of any existing kind must be preserved. Every
                // existing ancestor must be a real directory, never a link.
                if index + 1 == components.len() || !metadata.file_type().is_dir() {
                    return false;
                }
                let Ok((guard, snapshot)) = open_stable_directory(&current) else {
                    return false;
                };
                if !same_identity(&metadata, &snapshot) {
                    return false;
                }
                guards.push((current.clone(), guard, snapshot));
            }
        }
    }
    missing
        && guards.iter().all(|(path, guard, snapshot)| {
            guard
                .metadata()
                .is_ok_and(|current| same_identity(snapshot, &current))
                && OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NOFOLLOW_ANY | O_DIRECTORY)
                    .open(path)
                    .and_then(|file| file.metadata())
                    .is_ok_and(|current| same_identity(snapshot, &current))
        })
}

/// Shared by regular app launches and store URI routing. A copied executable
/// merely named EADesktop.exe or UbisoftConnect.exe is not a publisher client.
pub(super) fn is_reviewed_client(prefix: &Path, program: &Path, catalog_id: &str) -> bool {
    let Ok(real_prefix) = prefix.canonicalize() else {
        return false;
    };
    let Ok(program_guard) = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | libc::O_NONBLOCK)
        .open(program)
    else {
        return false;
    };
    let Ok(program_snapshot) = program_guard.metadata() else {
        return false;
    };
    if !program_snapshot.is_file() {
        return false;
    }
    let Ok(real_program) = program.canonicalize() else {
        return false;
    };
    let Ok(profile) = profile(catalog_id) else {
        return false;
    };
    let Some(relative) = reviewed_client_relative(&real_prefix, &real_program, profile) else {
        return false;
    };
    checked_launcher(prefix, &relative).is_some_and(|checked| checked == real_program)
        && program_guard
            .metadata()
            .is_ok_and(|current| same_snapshot(&program_snapshot, &current))
        && fs::symlink_metadata(program)
            .is_ok_and(|current| same_snapshot(&program_snapshot, &current))
}

/// These switches affect the EA launcher's embedded CEF only; they are never
/// added to installers, game executables, Steam, or generic Chromium programs.
/// The translator selected for games in the bottle is left unchanged.
pub(super) fn launcher_arguments(prefix: &Path, program: &Path) -> Vec<String> {
    if is_reviewed_client(prefix, program, EA_ID) {
        EA_CLIENT_SWITCHES
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    } else {
        Vec::new()
    }
}

fn reviewed_client_relative(prefix: &Path, program: &Path, profile: Profile) -> Option<PathBuf> {
    let drive = prefix.join("drive_c");
    let relative = program.strip_prefix(&drive).ok()?;
    if profile
        .launcher_candidates
        .iter()
        .any(|candidate| relative == Path::new(candidate))
    {
        return Some(relative.to_owned());
    }
    if profile.id == EA_ID {
        for root in EA_ROOTS {
            let Ok(tail) = relative.strip_prefix(root) else {
                continue;
            };
            let parts: Vec<_> = tail.components().collect();
            if parts.len() == 3
                && parts[0]
                    .as_os_str()
                    .to_str()
                    .and_then(numeric_ea_version)
                    .is_some()
                && parts[1].as_os_str() == "EA Desktop"
                && parts[2].as_os_str() == "EADesktop.exe"
            {
                return Some(relative.to_owned());
            }
        }
    }
    None
}

fn numeric_ea_version(value: &str) -> Option<Vec<u32>> {
    if value.len() > 64 {
        return None;
    }
    let mut numbers = Vec::new();
    for part in value.split('.') {
        if numbers.len() == 6 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        numbers.push(part.parse::<u32>().ok()?);
    }
    (numbers.len() >= 2).then_some(numbers)
}

fn newest_ea_launcher(prefix: &Path, root: &str) -> Option<PathBuf> {
    let path = prefix.join("drive_c").join(root);
    let (_guard, snapshot) = open_stable_directory(&path).ok()?;
    let entries = fs::read_dir(&path).ok()?;
    let mut candidates = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_EA_VERSION_ENTRIES || cancellation::is_requested() {
            return None; // A partial scan cannot prove which version is newest.
        }
        let entry = entry.ok()?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(version) = numeric_ea_version(name) else {
            continue;
        };
        if !fs::symlink_metadata(entry.path())
            .ok()?
            .file_type()
            .is_dir()
        {
            continue;
        }
        let relative = Path::new(root).join(name).join("EA Desktop/EADesktop.exe");
        if let Some(client) = checked_launcher(prefix, &relative) {
            candidates.push((version, name.to_owned(), client));
        }
    }
    if !fs::symlink_metadata(&path).is_ok_and(|current| same_snapshot(&snapshot, &current)) {
        return None;
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    candidates.pop().map(|(_, _, client)| client)
}

fn launcher_in_prefix(prefix: &Path, profile: Profile) -> Option<PathBuf> {
    if profile.id == EA_ID {
        for root in EA_ROOTS {
            if let Some(client) = newest_ea_launcher(prefix, root) {
                return Some(client);
            }
        }
    }
    profile
        .launcher_candidates
        .iter()
        .find_map(|candidate| checked_launcher(prefix, Path::new(candidate)))
}

fn checked_launcher(prefix: &Path, candidate: &Path) -> Option<PathBuf> {
    let real_prefix = prefix.canonicalize().ok()?;
    let path = prefix.join("drive_c").join(candidate);
    let before = fs::symlink_metadata(&path).ok()?;
    if !before.file_type().is_file() {
        return None;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | libc::O_NONBLOCK)
        .open(&path)
        .ok()?;
    if !same_snapshot(&before, &file.metadata().ok()?) {
        return None;
    }
    let real = path.canonicalize().ok()?;
    if !real.starts_with(&real_prefix) || pe_machine(&mut file, before.len()).is_err() {
        return None;
    }
    if !same_snapshot(&before, &file.metadata().ok()?)
        || !same_snapshot(&before, &fs::symlink_metadata(&path).ok()?)
    {
        return None;
    }
    Some(real)
}

fn wait_for_launcher(prefix: &Path, profile: Profile) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = installed_launcher(prefix, profile.id) {
            return Ok(Some(path));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn install_from_download(
    home: &Path,
    wine: &Path,
    profile: Profile,
    download: DownloadedInstaller,
) -> Result<PublisherCatalogInstallResult, String> {
    checked_installer_url(profile.id, &download.download_url)?;
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, profile.name)?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join(profile.installer_name);
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "{} bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            profile.name,
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, profile.inspect)
        .map_err(|error| format!("{} bottle {}: {error}", profile.name, bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        profile.id,
        download.sha256,
        download.download_url,
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_installer(home, wine, profile, bottle, verified, attempt, false)
}

fn retry_with_wine(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    profile: Profile,
) -> Result<PublisherCatalogInstallResult, String> {
    let (bottle, installer, attempt, _guard) =
        prepare_retry(home, bottle_id, profile.id, wine, profile.inspect)?;
    run_installer(home, wine, profile, bottle, installer, attempt, true)
}

fn failed_attempt(home: &Path, attempt: &mut CatalogAttempt, error: String) -> String {
    attempt.set_failure(&error);
    match save_catalog_attempt(home, attempt) {
        Ok(()) => error,
        Err(save_error) if cancellation::is_error(&error) => {
            format!("Could not save recovery status after cancellation: {save_error}")
        }
        Err(save_error) => format!("{error}. Could not save recovery status: {save_error}"),
    }
}

fn followup_issue(home: &Path, attempt: &mut CatalogAttempt, error: String) -> String {
    attempt.set_issue(&error);
    match save_catalog_attempt(home, attempt) {
        Ok(()) => error,
        Err(save_error) if cancellation::is_error(&error) => {
            format!("Could not save recovery status after cancellation: {save_error}")
        }
        Err(save_error) => format!("{error}. Could not save recovery status: {save_error}"),
    }
}

fn saved_launcher(
    home: &Path,
    bottle_id: &str,
    profile: Profile,
    path: &Path,
) -> Result<Launcher, String> {
    let expected = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(existing) = list_launchers(home, bottle_id)?.into_iter().find(|item| {
        Path::new(&item.path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == expected)
    }) {
        return Ok(existing);
    }
    add_launcher(home, bottle_id, profile.name, path)
}

fn run_installer(
    home: &Path,
    wine: &Path,
    profile: Profile,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<PublisherCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        // Show the publisher's installer. We do not silently accept its terms.
        let result = launch_wait_with_wine_checked(
            home,
            &bottle.id,
            installer.path(),
            wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: INSTALL_TIMEOUT,
                arguments: Vec::new(),
            },
            |resolved| installer.verify_before_spawn(home, &bottle.id, wine, resolved, is_retry),
        )
        .map_err(|error| {
            let error = if cancellation::is_error(&error) {
                error
            } else {
                format!("{} bottle {}: {error}", profile.name, bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("{} bottle {}: {error}", profile.name, bottle.id),
            )
        })?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        if result.timed_out || result.exit_code != Some(0) {
            let cleanup = if result.timed_out {
                Some(
                    cleanup_timed_out_catalog_install(home, &bottle.id, wine)
                        .map_err(|error| failed_attempt(home, &mut attempt, error))?,
                )
            } else {
                None
            };
            cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "{} bottle {} remains available, but setup did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
                    profile.name,
                    bottle.id,
                    result.exit_code,
                    result.signal,
                    result.timed_out,
                    cleanup.as_deref().unwrap_or(""),
                    result.log_path
                ),
            ));
        }
        if wait_for_launcher(&dir.join("prefix"), profile)
            .map_err(|error| failed_attempt(home, &mut attempt, error))?
            .is_none()
        {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "{} bottle {} remains available, but setup did not create a checked launcher at a reviewed path. Check {}",
                    profile.name, bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let program = launcher_in_prefix(&dir.join("prefix"), profile).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "{} bottle {}: previously installed launcher is missing",
                profile.name, bottle.id
            ),
        )
    })?;
    let launcher = saved_launcher(home, &bottle.id, profile, &program).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "{} bottle {}: installer completed, but launcher registration failed: {error}",
                profile.name, bottle.id
            ),
        )
    })?;
    attempt.set_stage(CatalogStage::Installed);
    save_catalog_attempt(home, &attempt)?;
    cancellation::check()?;
    let launch = launch_with_wine(home, &bottle.id, &program, wine).map_err(|error| {
        let error = if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "{} bottle {} has a saved launcher, but it could not start: {error}",
                profile.name, bottle.id
            )
        };
        followup_issue(home, &mut attempt, error)
    })?;
    Ok(PublisherCatalogInstallResult {
        bottle,
        launcher,
        installer_sha256: attempt.installer_sha256().to_owned(),
        download_url: attempt.download_url().to_owned(),
        install_log_path: attempt.install_log_path(&dir),
        launch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam_install::catalog_attempt;

    fn test_home() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("publisher-install-{}", Uuid::new_v4()))
    }

    fn fake_pe(path: &Path, profile: Profile) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; profile.min_bytes as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x014c_u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
        (profile.inspect)(&mut File::open(path).unwrap()).unwrap()
    }

    fn fake_wine(home: &Path, profile: Profile, exit_code: i32) -> PathBuf {
        let wine = home.join("wine");
        let launcher = profile.launcher_candidates[0];
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  *{installer})
    test "$#" -eq 1 || exit 92
    echo installing >> "$WINEPREFIX/installer-invocations.txt"
    if test {exit_code} -eq 0; then
      mkdir -p "$WINEPREFIX/drive_c/$(dirname '{launcher}')"
      printf 'MZ' > "$WINEPREFIX/drive_c/{launcher}"
      dd if=/dev/zero bs=1 count=58 2>/dev/null >> "$WINEPREFIX/drive_c/{launcher}"
      printf '\100\000\000\000PE\000\000\114\001' >> "$WINEPREFIX/drive_c/{launcher}"
    fi
    exit {exit_code};;
  *"{launcher_name}") echo launched-{id}; exit 0;;
  *) exit 93;;
esac
"#,
            installer = profile.installer_name,
            launcher = launcher,
            launcher_name = Path::new(launcher).file_name().unwrap().to_str().unwrap(),
            id = profile.id,
        );
        fs::write(&wine, script).unwrap();
        let boot = home.join("wineboot");
        fs::write(&boot, b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n").unwrap();
        let server = home.join("wineserver");
        fs::write(&server, b"#!/bin/sh\nexit 0\n").unwrap();
        for path in [&wine, &boot, &server] {
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
        wine
    }

    #[test]
    fn exact_publisher_urls_only() {
        for profile in [EA, UBISOFT] {
            assert!(checked_installer_url(profile.id, profile.url).is_ok());
            for wrong in [
                format!("http://{}", profile.url.trim_start_matches("https://")),
                format!("{}?next=evil", profile.url),
                format!("{}#fragment", profile.url),
                profile.url.replace(".exe", "-old.exe"),
                profile.url.replace("https://", "https://evil.example@"),
            ] {
                assert!(
                    checked_installer_url(profile.id, &wrong).is_err(),
                    "accepted {wrong}"
                );
            }
        }
        assert!(profile("unknown").is_err());
    }

    #[test]
    fn rejects_corrupt_installer_and_out_of_bounds_size() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        for profile in [EA, UBISOFT] {
            let path = home.join(profile.installer_name);
            assert_eq!(fake_pe(&path, profile).len(), 64);
            let mut bytes = fs::read(&path).unwrap();
            bytes[..2].fill(0);
            fs::write(&path, bytes).unwrap();
            assert!((profile.inspect)(&mut File::open(&path).unwrap()).is_err());
            fs::write(&path, b"MZ").unwrap();
            assert!((profile.inspect)(&mut File::open(&path).unwrap()).is_err());
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn checked_launcher_must_be_pe_inside_bottle() {
        let home = test_home();
        let prefix = home.join("prefix");
        let path = prefix.join("drive_c").join(EA.launcher_candidates[0]);
        let _ = fake_pe(&path, EA);
        assert!(launcher_in_prefix(&prefix, EA).is_some());
        fs::remove_file(&path).unwrap();
        symlink(home.join("outside.exe"), &path).unwrap();
        assert!(launcher_in_prefix(&prefix, EA).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ea_versioned_client_lookup_is_numeric_and_preserves_real_latest_fallback() {
        let home = test_home();
        let prefix = home.join("prefix");
        let ea_root = prefix.join("drive_c").join(EA_ROOTS[0]);
        fs::create_dir_all(&ea_root).unwrap();
        for version in ["13.9.0.1", "13.1000.0.1", "13.99.0.1"] {
            fake_pe(&ea_root.join(version).join("EA Desktop/EADesktop.exe"), EA);
        }
        // A populated direct directory may be an old install rather than the
        // missing latest junction; use the checked newest numeric version.
        let direct = prefix.join("drive_c").join(EA_CANDIDATES[0]);
        fake_pe(&direct, EA);
        assert!(
            installed_launcher(&prefix, EA_ID)
                .unwrap()
                .ends_with("13.1000.0.1/EA Desktop/EADesktop.exe")
        );
        fs::remove_dir_all(ea_root.join("13.1000.0.1")).unwrap();
        assert!(
            installed_launcher(&prefix, EA_ID)
                .unwrap()
                .ends_with("13.99.0.1/EA Desktop/EADesktop.exe")
        );
        for version in ["13.9.0.1", "13.99.0.1"] {
            fs::remove_dir_all(ea_root.join(version)).unwrap();
        }
        assert_eq!(
            installed_launcher(&prefix, EA_ID),
            Some(direct.canonicalize().unwrap())
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ea_version_lookup_refuses_aliases_bad_versions_and_non_pe_files() {
        let home = test_home();
        let prefix = home.join("prefix");
        let root = prefix.join("drive_c").join(EA_ROOTS[0]);
        fs::create_dir_all(&root).unwrap();
        let outside = home.join("outside/EA Desktop/EADesktop.exe");
        fake_pe(&outside, EA);
        symlink(home.join("outside"), root.join("99.0.0.0")).unwrap();
        for version in [
            "latest",
            "13..4",
            "13.4-beta",
            "13.4294967296",
            "13",
            "13.1.2.3.4.5.6",
        ] {
            fake_pe(&root.join(version).join("EA Desktop/EADesktop.exe"), EA);
        }
        let corrupt = root.join("99.99/EA Desktop/EADesktop.exe");
        fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
        fs::write(corrupt, b"MZ fake").unwrap();
        assert!(installed_launcher(&prefix, EA_ID).is_none());
        assert!(!is_reviewed_client(&prefix, &outside, EA_ID));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ea_version_lookup_refuses_a_partial_scan_and_symlinked_parent() {
        let home = test_home();
        let prefix = home.join("prefix");
        let root = prefix.join("drive_c").join(EA_ROOTS[0]);
        fake_pe(&root.join("13.1/EA Desktop/EADesktop.exe"), EA);
        for index in 0..MAX_EA_VERSION_ENTRIES {
            fs::create_dir(root.join(format!("ignored-{index}"))).unwrap();
        }
        assert!(installed_launcher(&prefix, EA_ID).is_none());
        fs::remove_dir_all(&root).unwrap();
        let outside = home.join("outside");
        fake_pe(&outside.join("13.2/EA Desktop/EADesktop.exe"), EA);
        symlink(&outside, &root).unwrap();
        assert!(installed_launcher(&prefix, EA_ID).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn saved_ea_version_recovers_only_after_its_original_path_disappears() {
        let home = test_home();
        let prefix = home.join("prefix");
        fs::create_dir_all(prefix.join("drive_c")).unwrap();
        let prefix = prefix.canonicalize().unwrap();
        let old = prefix
            .join("drive_c")
            .join(EA_ROOTS[0])
            .join("13.9.0.1/EA Desktop/EADesktop.exe");
        let current = prefix
            .join("drive_c")
            .join(EA_ROOTS[0])
            .join("13.1000.0.1/EA Desktop/EADesktop.exe");
        fake_pe(&old, EA);
        fake_pe(&current, EA);
        assert!(resolve_saved_ea_client(&prefix, &old).is_none());
        fs::remove_dir_all(old.parent().unwrap().parent().unwrap()).unwrap();
        assert_eq!(
            resolve_saved_ea_client(&prefix, &old),
            Some(current.clone())
        );
        let missing_fixed = prefix.join("drive_c").join(EA_CANDIDATES[0]);
        assert_eq!(
            resolve_saved_ea_client(&prefix, &missing_fixed),
            Some(current)
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn saved_ea_recovery_preserves_existing_bad_files_and_symlinks() {
        let home = test_home();
        let prefix = home.join("prefix");
        fs::create_dir_all(prefix.join("drive_c")).unwrap();
        let prefix = prefix.canonicalize().unwrap();
        let old = prefix.join("drive_c").join(EA_CANDIDATES[0]);
        let current = prefix
            .join("drive_c")
            .join(EA_ROOTS[0])
            .join("13.1000.0.1/EA Desktop/EADesktop.exe");
        fake_pe(&current, EA);
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, b"corrupt client must be preserved").unwrap();
        assert!(resolve_saved_ea_client(&prefix, &old).is_none());
        fs::remove_file(&old).unwrap();
        symlink(&current, &old).unwrap();
        assert!(resolve_saved_ea_client(&prefix, &old).is_none());
        fs::remove_file(&old).unwrap();
        symlink(home.join("missing-target"), &old).unwrap();
        assert!(resolve_saved_ea_client(&prefix, &old).is_none());
        fs::remove_file(&old).unwrap();
        fs::remove_dir(old.parent().unwrap()).unwrap();
        symlink(home.join("missing-parent-target"), old.parent().unwrap()).unwrap();
        assert!(resolve_saved_ea_client(&prefix, &old).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn saved_ea_recovery_refuses_other_bottles_generic_names_and_linked_prefixes() {
        let home = test_home();
        let prefix = home.join("prefix");
        fs::create_dir_all(prefix.join("drive_c")).unwrap();
        let prefix = prefix.canonicalize().unwrap();
        let current = prefix
            .join("drive_c")
            .join(EA_ROOTS[0])
            .join("13.1000.0.1/EA Desktop/EADesktop.exe");
        fake_pe(&current, EA);
        for path in [
            home.join("another-prefix/drive_c").join(EA_CANDIDATES[0]),
            prefix.join("drive_c/Games/EADesktop.exe"),
            prefix
                .join("drive_c")
                .join(EA_ROOTS[0])
                .join("latest/EA Desktop/EADesktop.exe"),
            prefix
                .join("drive_c")
                .join(EA_ROOTS[0])
                .join("13.bad/EA Desktop/EADesktop.exe"),
            PathBuf::from(EA_CANDIDATES[0]),
        ] {
            assert!(
                resolve_saved_ea_client(&prefix, &path).is_none(),
                "accepted an unreviewed saved path"
            );
        }
        let alias = home.join("linked-prefix");
        symlink(&prefix, &alias).unwrap();
        assert!(
            resolve_saved_ea_client(&alias, &prefix.join("drive_c").join(EA_CANDIDATES[0]))
                .is_none()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ea_cef_switches_are_restricted_to_checked_store_client_layouts() {
        let home = test_home();
        let prefix = home.join("prefix");
        for relative in [
            EA_CANDIDATES[0],
            EA_CANDIDATES[1],
            "Program Files/Electronic Arts/EA Desktop/13.1000.0.1/EA Desktop/EADesktop.exe",
        ] {
            let path = prefix.join("drive_c").join(relative);
            fake_pe(&path, EA);
            assert!(is_reviewed_client(&prefix, &path, EA_ID));
            assert_eq!(launcher_arguments(&prefix, &path), EA_CLIENT_SWITCHES);
        }
        for relative in [
            "Games/Example/EADesktop.exe",
            "Games/Example/Game.exe",
            "Program Files/Electronic Arts/EA Desktop/EA Desktop/EAappInstaller.exe",
            UBISOFT_CANDIDATES[0],
        ] {
            let path = prefix.join("drive_c").join(relative);
            fake_pe(&path, EA);
            assert!(launcher_arguments(&prefix, &path).is_empty());
        }
        let alias = home.join("linked-EADesktop.exe");
        symlink(prefix.join("drive_c").join(EA_CANDIDATES[0]), &alias).unwrap();
        assert!(launcher_arguments(&prefix, &alias).is_empty());
        let ubisoft = prefix.join("drive_c").join(UBISOFT_CANDIDATES[0]);
        assert!(is_reviewed_client(&prefix, &ubisoft, UBISOFT_ID));
        assert!(!is_reviewed_client(&prefix, &ubisoft, EA_ID));
        assert!(!is_reviewed_client(&prefix, &ubisoft, "unknown"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn both_setups_save_launcher_and_receipt() {
        for profile in [EA, UBISOFT] {
            let home = test_home();
            fs::create_dir_all(&home).unwrap();
            let wine = fake_wine(&home, profile, 0);
            let path = home.join(profile.installer_name);
            let digest = fake_pe(&path, profile);
            let installed = install_from_download(
                &home,
                &wine,
                profile,
                DownloadedInstaller {
                    path,
                    sha256: digest.clone(),
                    download_url: profile.url.into(),
                },
            )
            .unwrap();
            assert_eq!(installed.bottle.name, profile.name);
            assert_eq!(installed.launcher.name, profile.name);
            assert_eq!(installed.installer_sha256, digest);
            assert_eq!(
                list_launchers(&home, &installed.bottle.id).unwrap().len(),
                1
            );
            assert_eq!(
                catalog_attempt(&home, &installed.bottle.id)
                    .unwrap()
                    .unwrap()
                    .stage,
                CatalogStage::Installed
            );
            assert!(Path::new(&installed.install_log_path).is_file());
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn failed_setup_retries_same_checked_bottle() {
        for profile in [EA, UBISOFT] {
            let home = test_home();
            fs::create_dir_all(&home).unwrap();
            let wine = fake_wine(&home, profile, 37);
            let path = home.join(profile.installer_name);
            let digest = fake_pe(&path, profile);
            let error = install_from_download(
                &home,
                &wine,
                profile,
                DownloadedInstaller {
                    path,
                    sha256: digest,
                    download_url: profile.url.into(),
                },
            )
            .err()
            .unwrap();
            let bottle = list(&home).unwrap().pop().unwrap();
            assert!(error.contains(&bottle.id));
            assert!(list_launchers(&home, &bottle.id).unwrap().is_empty());
            fake_wine(&home, profile, 0);
            let installed = retry_with_wine(&home, &bottle.id, &wine, profile).unwrap();
            assert_eq!(installed.bottle.id, bottle.id);
            assert_eq!(list(&home).unwrap().len(), 1);
            let invocations = fs::read_to_string(
                bottle_dir(&home, &bottle.id)
                    .unwrap()
                    .join("prefix/installer-invocations.txt"),
            )
            .unwrap();
            assert_eq!(invocations.lines().count(), 2);
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn retry_refuses_tampered_or_symlink_installer() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, EA, 37);
        let path = home.join(EA.installer_name);
        let digest = fake_pe(&path, EA);
        let _ = install_from_download(
            &home,
            &wine,
            EA,
            DownloadedInstaller {
                path,
                sha256: digest,
                download_url: EA.url.into(),
            },
        );
        let bottle = list(&home).unwrap().pop().unwrap();
        let installer = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("installers/EAappInstaller.exe");
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        let mut bytes = fs::read(&installer).unwrap();
        *bytes.last_mut().unwrap() = 1;
        fs::write(&installer, bytes).unwrap();
        assert!(retry_with_wine(&home, &bottle.id, &wine, EA).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_file(&installer).unwrap();
        symlink(home.join("wine"), &installer).unwrap();
        assert!(retry_with_wine(&home, &bottle.id, &wine, EA).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_dir_all(home).unwrap();
    }
}
