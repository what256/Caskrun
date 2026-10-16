//! Original, deliberately narrow recipe for Valve's Windows Steam installer.
//! Epic, GOG, and Battle.net use separate recipes; other entries remain manual
//! until each has its own reviewed installation flow.

use super::*;
use sha2::{Digest, Sha256};
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;

// This is the Windows installer URL currently linked by Valve's public
// https://store.steampowered.com/about/ page. It is not a compatibility claim.
const STEAM_INSTALLER_URL: &str =
    "https://cdn.fastly.steamstatic.com/client/installer/SteamSetup.exe";
const MAX_INSTALLER_BYTES: u64 = 50 * 1024 * 1024;
const MIN_INSTALLER_BYTES: u64 = 100 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(130);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_ATTEMPT_RECORD_BYTES: u64 = 8 * 1024;
const MAX_ATTEMPT_ERROR_CHARS: usize = 500;
const CATALOG_ATTEMPT_VERSION: u32 = 1;
const CATALOG_ATTEMPT_FILE: &str = "catalog-attempt.json";
const STEAM_RECOVERY_NOTE: &str = "Steam's installer stalled after creating Steam.exe. Caskrun stopped the installer and started Steam's own updater. Sign-in and games remain untested.";
// Steam's embedded Chromium UI failed its D3D11/EGL initialization on the
// bundled Wine runtime. This is a best-effort software UI fallback, not proof
// that the sign-in window renders or that a game will run.
pub(super) const STEAM_CEF_SOFTWARE_FLAG: &str = "-cef-disable-gpu";

/// A durable, deliberately narrow receipt for an installer Caskrun downloaded.
/// It is not a publisher signature. Retry always rechecks the retained bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct CatalogAttempt {
    version: u32,
    pub(super) bottle_id: String,
    pub(super) catalog_id: String,
    installer_path: String,
    installer_sha256: String,
    download_url: String,
    pub(super) stage: CatalogStage,
    last_log_name: Option<String>,
    last_error: Option<String>,
    updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CatalogStage {
    Ready,
    Running,
    Failed,
    InstallerComplete,
    PreparingClient,
    Installed,
}

impl CatalogAttempt {
    pub(super) fn new(
        bottle_id: &str,
        catalog_id: &str,
        installer_sha256: String,
        download_url: String,
    ) -> Result<Self, String> {
        let installer_path = installer_relative_path(catalog_id)?;
        let attempt = Self {
            version: CATALOG_ATTEMPT_VERSION,
            bottle_id: bottle_id.to_owned(),
            catalog_id: catalog_id.to_owned(),
            installer_path: installer_path.to_owned(),
            installer_sha256,
            download_url,
            stage: CatalogStage::Ready,
            last_log_name: None,
            last_error: None,
            updated_at: now_seconds()?,
        };
        validate_attempt(&attempt, bottle_id)?;
        Ok(attempt)
    }

    pub(super) fn installer_sha256(&self) -> &str {
        &self.installer_sha256
    }

    pub(super) fn download_url(&self) -> &str {
        &self.download_url
    }

    pub(super) fn set_stage(&mut self, stage: CatalogStage) {
        self.stage = stage;
        self.updated_at = now_seconds().unwrap_or(self.updated_at);
        if stage != CatalogStage::Failed {
            self.last_error = None;
        }
    }

    pub(super) fn set_failure(&mut self, error: &str) {
        self.set_stage(CatalogStage::Failed);
        self.set_issue(error);
    }

    pub(super) fn set_issue(&mut self, error: &str) {
        self.updated_at = now_seconds().unwrap_or(self.updated_at);
        self.last_error = Some(
            error
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .take(MAX_ATTEMPT_ERROR_CHARS)
                .collect(),
        );
    }

    pub(super) fn set_log_path(&mut self, path: &str) -> Result<(), String> {
        let name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid catalog install log path")?;
        if !is_log_name(name) {
            return Err("Invalid catalog install log name".into());
        }
        self.last_log_name = Some(name.to_owned());
        Ok(())
    }

    pub(super) fn install_log_path(&self, dir: &Path) -> String {
        self.last_log_name
            .as_deref()
            .map(|name| dir.join("logs").join(name).display().to_string())
            .unwrap_or_default()
    }
}

fn installer_relative_path(catalog_id: &str) -> Result<&'static str, String> {
    match catalog_id {
        "steam" => Ok("installers/SteamSetup.exe"),
        "epic-games-store" => Ok("installers/EpicInstaller.msi"),
        "gog-galaxy" => Ok("installers/GOGGalaxySetup.exe"),
        "battle-net" => Ok("installers/Battle.net-Setup.exe"),
        "ea-app" => Ok("installers/EAappInstaller.exe"),
        "ubisoft-connect" => Ok("installers/UbisoftConnectInstaller.exe"),
        "itch-io-app" => Ok("installers/itch-setup.exe"),
        "rockstar-games-launcher" => Ok("installers/Rockstar-Games-Launcher.exe"),
        _ => Err("Unknown automatic catalog installer".into()),
    }
}

fn validate_attempt(attempt: &CatalogAttempt, bottle_id: &str) -> Result<(), String> {
    if Uuid::parse_str(bottle_id).is_err() || attempt.bottle_id != bottle_id {
        return Err("Catalog attempt does not belong to this bottle".into());
    }
    if attempt.version != CATALOG_ATTEMPT_VERSION
        || attempt.installer_path != installer_relative_path(&attempt.catalog_id)?
        || attempt.installer_sha256.len() != 64
        || !attempt
            .installer_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Invalid catalog attempt receipt".into());
    }
    match attempt.catalog_id.as_str() {
        "steam" if attempt.download_url == STEAM_INSTALLER_URL => {}
        "epic-games-store" => {
            super::epic_install::checked_epic_installer_url(&attempt.download_url)?;
        }
        "gog-galaxy" => {
            super::gog_install::checked_gog_installer_url(&attempt.download_url)?;
        }
        "battle-net" => {
            super::battle_net_install::checked_battle_net_installer_url(&attempt.download_url)?;
        }
        "ea-app" | "ubisoft-connect" => {
            super::publisher_install::checked_installer_url(
                &attempt.catalog_id,
                &attempt.download_url,
            )?;
        }
        "itch-io-app" => {
            super::itch_install::checked_itch_installer_url(&attempt.download_url)?;
        }
        "rockstar-games-launcher" => {
            super::rockstar_install::checked_rockstar_installer_url(&attempt.download_url)?;
        }
        _ => return Err("Catalog attempt has an unreviewed download URL".into()),
    }
    if attempt
        .last_log_name
        .as_deref()
        .is_some_and(|name| !is_log_name(name))
        || attempt.last_error.as_ref().is_some_and(|error| {
            error.chars().count() > MAX_ATTEMPT_ERROR_CHARS || error.chars().any(char::is_control)
        })
    {
        return Err("Invalid catalog attempt status".into());
    }
    Ok(())
}

pub(super) fn save_catalog_attempt(home: &Path, attempt: &CatalogAttempt) -> Result<(), String> {
    validate_attempt(attempt, &attempt.bottle_id)?;
    let dir = bottle_dir(home, &attempt.bottle_id)?;
    save_json(&dir.join(CATALOG_ATTEMPT_FILE), attempt)
}

pub(super) fn catalog_attempt(
    home: &Path,
    bottle_id: &str,
) -> Result<Option<CatalogAttempt>, String> {
    let dir = bottle_dir(home, bottle_id)?;
    let path = dir.join(CATALOG_ATTEMPT_FILE);
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect catalog attempt: {error}")),
    };
    if !before.file_type().is_file() || before.len() > MAX_ATTEMPT_RECORD_BYTES {
        return Err("Catalog attempt must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| format!("Could not safely open catalog attempt: {error}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Catalog attempt changed while opening it".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_ATTEMPT_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ATTEMPT_RECORD_BYTES
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Catalog attempt changed while reading it".into());
    }
    let attempt: CatalogAttempt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid catalog attempt: {error}"))?;
    // Duplicate/import creates a new bottle ID. The copied receipt belongs to
    // the source bottle and must never authorize automatic execution here.
    if attempt.bottle_id != bottle_id {
        return Ok(None);
    }
    validate_attempt(&attempt, bottle_id)?;
    Ok(Some(attempt))
}

pub(super) fn catalog_attempts(home: &Path) -> Result<Vec<CatalogAttempt>, String> {
    let mut attempts = Vec::new();
    for bottle in list(home)? {
        if let Some(attempt) = catalog_attempt(home, &bottle.id)? {
            attempts.push(attempt);
        }
    }
    Ok(attempts)
}

pub(super) fn lock_catalog_attempt(dir: &Path) -> Result<File, String> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(dir.join(".catalog-attempt.lock"))
        .map_err(|error| format!("Could not open catalog installer lock: {error}"))?;
    if cancellation::is_active() {
        loop {
            cancellation::check()?;
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("Could not lock catalog installer: {error}")),
            }
        }
    } else {
        lock.lock()
            .map_err(|error| format!("Could not lock catalog installer: {error}"))?;
    }
    Ok(lock)
}

/// Keeps the exact checked file descriptor alive until Wine has been spawned.
/// The final check happens under the ordinary bottle launch lock. A separate
/// same-user process can still mutate a file after that check; this is not an
/// adversarial same-user file-system boundary.
pub(super) struct VerifiedInstaller {
    path: PathBuf,
    canonical_path: PathBuf,
    file: File,
    snapshot: fs::Metadata,
    expected_sha256: String,
    inspect: fn(&mut File) -> Result<String, String>,
}

impl VerifiedInstaller {
    pub(super) fn open(
        path: PathBuf,
        expected_sha256: &str,
        inspect: fn(&mut File) -> Result<String, String>,
    ) -> Result<Self, String> {
        let before = fs::symlink_metadata(&path)
            .map_err(|error| format!("Catalog installer is missing: {error}"))?;
        if !before.file_type().is_file() {
            return Err("Catalog installer must be a regular file".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&path)
            .map_err(|error| format!("Could not safely open catalog installer: {error}"))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("Catalog installer changed while opening it".into());
        }
        let digest =
            inspect(&mut file).map_err(|error| format!("Catalog installer is invalid: {error}"))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("Catalog installer changed while inspecting it".into());
        }
        if digest != expected_sha256 {
            return Err("Catalog installer changed; installation stopped before execution".into());
        }
        let canonical_path = path.canonicalize().map_err(|e| e.to_string())?;
        if !same_snapshot(
            &before,
            &fs::symlink_metadata(&path).map_err(|e| e.to_string())?,
        ) {
            return Err("Catalog installer changed while resolving its path".into());
        }
        Ok(Self {
            path,
            canonical_path,
            file,
            snapshot: before,
            expected_sha256: expected_sha256.to_owned(),
            inspect,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn verify_before_spawn(
        &self,
        home: &Path,
        bottle_id: &str,
        wine: &Path,
        resolved_path: &Path,
        check_idle_server: bool,
    ) -> Result<(), String> {
        let dir = bottle_dir(home, bottle_id)?;
        refuse_running_bottle(&dir)?;
        if check_idle_server {
            refuse_live_wine_server(&dir, WINE_SERVER_CHECK_TIMEOUT, Some(wine))?;
        }
        let current = fs::symlink_metadata(&self.path)
            .map_err(|error| format!("Catalog installer changed before launch: {error}"))?;
        let opened = self.file.metadata().map_err(|e| e.to_string())?;
        if !current.file_type().is_file()
            || !same_snapshot(&self.snapshot, &current)
            || !same_snapshot(&self.snapshot, &opened)
            || resolved_path != self.canonical_path
        {
            return Err("Catalog installer changed after verification; launch stopped".into());
        }
        let mut file = self.file.try_clone().map_err(|e| e.to_string())?;
        let digest = (self.inspect)(&mut file)
            .map_err(|error| format!("Catalog installer changed before launch: {error}"))?;
        if digest != self.expected_sha256
            || !same_snapshot(
                &self.snapshot,
                &self.file.metadata().map_err(|e| e.to_string())?,
            )
            || !same_snapshot(
                &self.snapshot,
                &fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?,
            )
        {
            return Err("Catalog installer changed after verification; launch stopped".into());
        }
        Ok(())
    }
}

pub(super) fn prepare_retry(
    home: &Path,
    bottle_id: &str,
    expected_catalog_id: &str,
    wine: &Path,
    inspect: fn(&mut File) -> Result<String, String>,
) -> Result<(Bottle, VerifiedInstaller, CatalogAttempt, File), String> {
    let dir = bottle_dir(home, bottle_id)?;
    let guard = lock_catalog_attempt(&dir)?;
    let bottle = read_bottle_record(&dir, bottle_id)?;
    let attempt = catalog_attempt(home, bottle_id)?.ok_or(
        "This bottle has no verified catalog installer receipt; use its saved installer manually",
    )?;
    if attempt.catalog_id != expected_catalog_id {
        return Err("Catalog installer does not match this bottle's receipt".into());
    }
    if attempt.stage == CatalogStage::Installed {
        return Err(
            "This catalog installer already completed; open its saved launcher instead".into(),
        );
    }
    refuse_running_bottle(&dir)?;
    // The direct Wine child may have exited while its Windows installer and
    // wineserver are still running. A retry must never overlap that work.
    refuse_live_wine_server(&dir, Duration::from_secs(8), Some(wine)).map_err(|error| {
        if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Cannot retry while this bottle's Windows apps may still be running. Use Quit Windows apps in the bottle, then retry. Details: {error}"
            )
        }
    })?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = dir.join(&attempt.installer_path);
    if !installer.starts_with(&installers) {
        return Err("Catalog installer path is outside the bottle".into());
    }
    let verified = VerifiedInstaller::open(installer, &attempt.installer_sha256, inspect)?;
    Ok((bottle, verified, attempt, guard))
}

/// An installer can leave Windows child processes behind after Caskrun's
/// direct Wine process times out. Stop only this bottle's Wine server before
/// offering retry; if that fails, the retry idle check will still refuse it.
/// A cancel request that wins before the bottle-specific timeout cleanup
/// leaves the bottle alone.
pub(super) fn cleanup_timed_out_catalog_install(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<String, String> {
    let note = match bottle_control::quit_windows_apps_with_wine(
        home,
        bottle_id,
        wine,
        Duration::from_secs(8),
    ) {
        Ok(result) if result.server_stopped => {
            "Windows installer processes in this bottle were stopped.".into()
        }
        Ok(_) => {
            "Windows installer processes may still be running. Stop them before retrying.".into()
        }
        Err(error) if cancellation::is_error(&error) => return Err(error),
        Err(error) => format!(
            "Windows installer processes may still be running. Stop them before retrying: {error}"
        ),
    };
    Ok(note)
}

#[derive(Serialize)]
pub(super) struct CatalogInstallResult {
    bottle: Bottle,
    launcher: Launcher,
    installer_sha256: String,
    download_url: &'static str,
    install_log_path: String,
    launch: LaunchResult,
    setup_note: Option<&'static str>,
}

struct DownloadedInstaller {
    path: PathBuf,
    sha256: String,
}

pub(super) fn install_catalog(home: &Path, id: &str) -> Result<CatalogInstallResult, String> {
    if id != "steam" {
        return Err(format!(
            "No automatic installer is available for {id}. Use its manual catalog guide."
        ));
    }
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let installer = download_steam_installer(home)?;
    install_steam_from_download(home, &wine, installer)
}

fn download_steam_installer(home: &Path) -> Result<DownloadedInstaller, String> {
    download_steam_installer_with_curl(home, Path::new("/usr/bin/curl"))
}

fn download_steam_installer_with_curl(
    home: &Path,
    curl: &Path,
) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let id = Uuid::new_v4();
    let path = downloads.join(format!("SteamSetup-{id}.exe"));
    let log_path = downloads.join(format!("SteamSetup-{id}.download.log"));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| format!("Could not prepare Steam download: {error}"))?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare Steam download log: {error}"))?;

    // -q must be the first option: it disables ~/.curlrc so local curl settings
    // cannot silently change the source, TLS checks, or output. No redirects
    // are followed; a changed Valve link requires a reviewed source update.
    let mut child = Command::new(curl)
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
            "120",
            "--max-filesize",
            "52428800",
            STEAM_INSTALLER_URL,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("Could not start official Steam download: {error}"))?;
    let status = match wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT) {
        Ok(status) => status,
        Err(error) => {
            let _ = fs::remove_file(&path);
            if cancellation::is_error(&error) {
                return Err(error);
            }
            return Err(format!(
                "Steam download did not finish: {error}. Check {}",
                log_path.display()
            ));
        }
    };
    if !status.is_some_and(|status| status.success()) {
        let _ = fs::remove_file(&path);
        return Err(format!(
            "Could not download Steam from Valve's HTTPS installer URL. Check {} and retry.",
            log_path.display()
        ));
    }
    let sha256 = match inspect_pe_and_sha256(&mut file) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(format!(
                "Valve's Steam download was not a usable Windows installer: {error}. Check {}",
                log_path.display()
            ));
        }
    };
    Ok(DownloadedInstaller { path, sha256 })
}

pub(super) fn inspect_pe_and_sha256(file: &mut File) -> Result<String, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&length) {
        return Err("unexpected installer size".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..2] != b"MZ" {
        return Err("missing Windows MZ header".into());
    }
    let pe_offset = u32::from_le_bytes(header[0x3c..0x40].try_into().unwrap()) as u64;
    if pe_offset < 64 || pe_offset > length.saturating_sub(6) {
        return Err("invalid Windows PE header offset".into());
    }
    file.seek(SeekFrom::Start(pe_offset))
        .map_err(|e| e.to_string())?;
    let mut pe_header = [0_u8; 6];
    file.read_exact(&mut pe_header).map_err(|e| e.to_string())?;
    if &pe_header[..4] != b"PE\0\0" {
        return Err("missing Windows PE signature".into());
    }
    let machine = u16::from_le_bytes([pe_header[4], pe_header[5]]);
    if machine != 0x014c && machine != 0x8664 {
        return Err("installer is not an x86 or x64 Windows program".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn install_steam_from_download(
    home: &Path,
    wine: &Path,
    download: DownloadedInstaller,
) -> Result<CatalogInstallResult, String> {
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "Steam")?;
    let dir = bottle_dir(home, &bottle.id)
        .map_err(|error| format!("Steam bottle {}: {error}", bottle.id))?;
    let installers = real_subdirectory(&dir, "installers")
        .map_err(|error| format!("Steam bottle {}: {error}", bottle.id))?;
    let installer = installers.join("SteamSetup.exe");
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "Steam bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_pe_and_sha256)
        .map_err(|error| format!("Steam bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        "steam",
        download.sha256,
        STEAM_INSTALLER_URL.into(),
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_steam_installer(home, wine, bottle, verified, attempt, false)
}

pub(super) fn retry_catalog(home: &Path, bottle_id: &str) -> Result<CatalogInstallResult, String> {
    cancellation::check()?;
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    retry_catalog_with_wine(home, bottle_id, &wine)
}

fn retry_catalog_with_wine(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<CatalogInstallResult, String> {
    let (bottle, installer, mut attempt, _guard) =
        prepare_retry(home, bottle_id, "steam", wine, inspect_pe_and_sha256)?;
    // Older builds saved a failed timeout even when Steam's bootstrapper had
    // already been copied. Recover that exact case without rerunning the
    // service registration step that stalled, after proving the prefix idle.
    if attempt.stage == CatalogStage::Failed
        && attempt
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("timeout true"))
        && checked_steam_bootstrap(&bottle_dir(home, bottle_id)?).is_some()
    {
        attempt.set_stage(CatalogStage::InstallerComplete);
        attempt.set_issue(STEAM_RECOVERY_NOTE);
        save_catalog_attempt(home, &attempt)?;
    }
    run_steam_installer(home, wine, bottle, installer, attempt, true)
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

fn steam_launcher_path(dir: &Path) -> Option<PathBuf> {
    // Valve's installer has produced both `steam.exe` and `Steam.exe` across
    // Windows prefixes. Search only these names in the two standard locations.
    const CANDIDATES: [&str; 4] = [
        "Program Files (x86)/Steam/steam.exe",
        "Program Files (x86)/Steam/Steam.exe",
        "Program Files/Steam/steam.exe",
        "Program Files/Steam/Steam.exe",
    ];
    let prefix = dir.join("prefix");
    let real_prefix = prefix.canonicalize().ok()?;
    CANDIDATES.iter().find_map(|candidate| {
        let path = prefix.join("drive_c").join(candidate);
        if !path_is_real_file(&path) {
            return None;
        }
        let real = path.canonicalize().ok()?;
        real.starts_with(&real_prefix).then_some(real)
    })
}

fn checked_steam_bootstrap(dir: &Path) -> Option<PathBuf> {
    let path = steam_launcher_path(dir)?;
    let mut file = File::open(&path).ok()?;
    inspect_pe_and_sha256(&mut file).ok()?;
    Some(path)
}

fn saved_steam_launcher(home: &Path, bottle_id: &str, path: &Path) -> Result<Launcher, String> {
    let expected = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(existing) = list_launchers(home, bottle_id)?.into_iter().find(|item| {
        Path::new(&item.path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == expected)
    }) {
        return Ok(existing);
    }
    add_launcher(home, bottle_id, "Steam", path)
}

pub(super) fn is_recipe_steam_launcher(home: &Path, bottle_id: &str, launcher: &Launcher) -> bool {
    let Ok(Some(attempt)) = catalog_attempt(home, bottle_id) else {
        return false;
    };
    if attempt.catalog_id != "steam" || attempt.stage != CatalogStage::Installed {
        return false;
    }
    let Ok(dir) = bottle_dir(home, bottle_id) else {
        return false;
    };
    let Some(expected) = steam_launcher_path(&dir) else {
        return false;
    };
    Path::new(&launcher.path)
        .canonicalize()
        .is_ok_and(|actual| actual == expected)
}

/// Steam UI and native focus handling also apply to a saved, installed client
/// imported from another prefix or added manually. This does not create an
/// installer receipt or make the client eligible for recipe-only migrations.
pub(super) fn is_steam_client_launcher(home: &Path, bottle_id: &str, launcher: &Launcher) -> bool {
    if is_recipe_steam_launcher(home, bottle_id, launcher) {
        return true;
    }
    let Ok(dir) = bottle_dir(home, bottle_id) else {
        return false;
    };
    let path = Path::new(&launcher.path);
    let Ok(clients) = steam_library::client_paths(&dir.join("prefix")) else {
        return false;
    };
    if !path
        .canonicalize()
        .is_ok_and(|actual| clients.contains(&actual))
    {
        return false;
    }
    // Only a conventional client inside this bottle is checked. A label or a
    // filename elsewhere is not enough. Read the headers rather than hashing
    // Steam's multi-megabyte executable every time the user opens it.
    let Ok(before) = fs::symlink_metadata(path) else {
        return false;
    };
    if !before.file_type().is_file() || before.len() < 70 {
        return false;
    }
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4) // Darwin O_NONBLOCK: reject an exchanged FIFO.
        .open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|opened| same_snapshot(&before, &opened))
    {
        return false;
    }
    let mut dos = [0_u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    let offset = u64::from(u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()));
    if !(64..=64 * 1024).contains(&offset)
        || offset.saturating_add(6) > before.len()
        || file.seek(SeekFrom::Start(offset)).is_err()
    {
        return false;
    }
    let mut pe = [0_u8; 6];
    file.read_exact(&mut pe).is_ok()
        && &pe[..4] == b"PE\0\0"
        && matches!(u16::from_le_bytes([pe[4], pe[5]]), 0x014c | 0x8664)
        && file
            .metadata()
            .is_ok_and(|after| same_snapshot(&before, &after))
        && fs::symlink_metadata(path).is_ok_and(|after| same_snapshot(&before, &after))
}

/// A recipe-only client target. Retained directory handles and the exact file
/// snapshot prevent a concurrent rename from redirecting the bootstrap copy.
/// As with VerifiedInstaller, this is not a hostile same-user FS boundary.
struct RecipeSteamTarget {
    path: PathBuf,
    name: CString,
    file: File,
    snapshot: fs::Metadata,
    digest: String,
    machine: u16,
    directories: Vec<(PathBuf, File, fs::Metadata)>,
}

impl RecipeSteamTarget {
    fn open(dir: &Path) -> Result<Self, String> {
        let prefix = dir.join("prefix");
        for candidate in [
            "Program Files (x86)/Steam/steam.exe",
            "Program Files (x86)/Steam/Steam.exe",
            "Program Files/Steam/steam.exe",
            "Program Files/Steam/Steam.exe",
        ] {
            let path = prefix.join("drive_c").join(candidate);
            let before = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("Could not inspect Steam's client: {error}")),
            };
            if !before.file_type().is_file() {
                return Err("Steam's client must be a regular file, not a link".into());
            }
            let parent = path.parent().ok_or("Steam's client has no folder")?;
            let mut directories = Vec::new();
            for directory in [dir, prefix.as_path(), parent] {
                let (guard, snapshot) = open_stable_directory(directory)?;
                directories.push((directory.to_path_buf(), guard, snapshot));
            }
            let mut file = OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW_ANY | libc::O_NONBLOCK)
                .open(&path)
                .map_err(|error| format!("Could not safely open Steam's client: {error}"))?;
            if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
                return Err("Steam's client changed while opening it".into());
            }
            let (machine, digest) = inspect_client_machine_and_digest(&mut file)?;
            let name = CString::new(
                path.file_name()
                    .ok_or("Steam's client has no name")?
                    .as_bytes(),
            )
            .map_err(|_| "Invalid Steam client name")?;
            let result = Self {
                path,
                name,
                file,
                snapshot: before,
                digest,
                machine,
                directories,
            };
            result.recheck()?;
            return Ok(result);
        }
        Err("Steam's installer did not leave a usable client executable".into())
    }

    fn check_directories(&self) -> Result<(), String> {
        for (path, guard, snapshot) in &self.directories {
            bottle_control::assert_same_prefix(path, guard, snapshot)?;
            // The leaf inode alone does not detect an ancestor replaced by a
            // symlink back to a moved original tree. Reopen the complete path
            // without following any component before a file can be replaced.
            let (reopened, _) = open_stable_directory(path)?;
            bottle_control::assert_same_prefix(path, &reopened, snapshot)?;
        }
        Ok(())
    }

    fn parent(&self) -> &File {
        &self.directories.last().expect("Steam parent is retained").1
    }

    fn recheck(&self) -> Result<(), String> {
        self.check_directories()?;
        let current = fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?;
        if !current.file_type().is_file()
            || !same_snapshot(&self.snapshot, &current)
            || !same_snapshot(
                &self.snapshot,
                &self.file.metadata().map_err(|e| e.to_string())?,
            )
        {
            return Err(
                "Steam's client changed during preparation; retry after it finishes updating"
                    .into(),
            );
        }
        let (_, digest) = inspect_client_machine_and_digest(
            &mut self.file.try_clone().map_err(|e| e.to_string())?,
        )?;
        if digest != self.digest
            || !same_snapshot(
                &self.snapshot,
                &self.file.metadata().map_err(|e| e.to_string())?,
            )
            || !same_snapshot(
                &self.snapshot,
                &fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?,
            )
        {
            return Err(
                "Steam's client changed during preparation; retry after it finishes updating"
                    .into(),
            );
        }
        Ok(())
    }
}

fn inspect_client_machine_and_digest(file: &mut File) -> Result<(u16, String), String> {
    let digest = inspect_pe_and_sha256(file)?;
    file.seek(SeekFrom::Start(0x3c))
        .map_err(|e| e.to_string())?;
    let mut offset = [0_u8; 4];
    file.read_exact(&mut offset).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(u64::from(u32::from_le_bytes(offset)) + 4))
        .map_err(|e| e.to_string())?;
    let mut machine = [0_u8; 2];
    file.read_exact(&mut machine).map_err(|e| e.to_string())?;
    Ok((u16::from_le_bytes(machine), digest))
}

struct StagedSteamClient<'a> {
    parent: &'a File,
    name: CString,
    file: File,
    renamed: bool,
}

impl<'a> StagedSteamClient<'a> {
    fn create(parent: &'a File) -> Result<Self, String> {
        let name =
            CString::new(format!(".caskrun-steam-bootstrap-{}.tmp", Uuid::new_v4())).unwrap();
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | O_NOFOLLOW_ANY,
                0o600,
            )
        };
        if fd < 0 {
            return Err(format!(
                "Could not stage Steam's current client: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            parent,
            name,
            file: unsafe { File::from_raw_fd(fd) },
            renamed: false,
        })
    }

    fn recheck(&mut self, expected_digest: &str) -> Result<fs::Metadata, String> {
        let before = self.file.metadata().map_err(|e| e.to_string())?;
        let fd = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK | O_NOFOLLOW_ANY,
            )
        };
        if fd < 0 {
            return Err("Steam's staged client changed before installation".into());
        }
        let mut named = unsafe { File::from_raw_fd(fd) };
        if !same_snapshot(&before, &named.metadata().map_err(|e| e.to_string())?) {
            return Err("Steam's staged client changed before installation".into());
        }
        let (machine, digest) = inspect_client_machine_and_digest(&mut named)?;
        if machine != 0x8664
            || digest != expected_digest
            || !same_snapshot(&before, &named.metadata().map_err(|e| e.to_string())?)
            || !same_snapshot(&before, &self.file.metadata().map_err(|e| e.to_string())?)
        {
            return Err(
                "Steam's staged current client failed verification; the original was preserved"
                    .into(),
            );
        }
        Ok(before)
    }
}

impl Drop for StagedSteamClient<'_> {
    fn drop(&mut self) {
        if !self.renamed {
            // Remove only our unique name through the retained parent handle.
            unsafe {
                libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}

fn prepare_recipe_bootstrap(
    home: &Path,
    wine: &Path,
    bottle: &Bottle,
    attempt: &mut CatalogAttempt,
) -> Result<(), String> {
    prepare_recipe_bootstrap_with(
        home,
        wine,
        bottle,
        attempt,
        steam_bootstrap::download_verified_bootstrap,
        |home, id, wine| {
            // The caller holds this bottle's lock. Stop only this recipe's
            // installer-started session; never relock through the public API.
            let stopped = bottle_control::quit_recorded_windows_apps_locked(
                home,
                id,
                wine,
                Duration::from_secs(8),
            )?;
            if !stopped.server_stopped {
                return Err("Steam's installer session has not stopped; retry after closing its Windows apps".into());
            }
            Ok(())
        },
    )
}

fn prepare_recipe_bootstrap_with(
    home: &Path,
    wine: &Path,
    bottle: &Bottle,
    attempt: &mut CatalogAttempt,
    download: impl FnOnce(&Path) -> Result<steam_bootstrap::DownloadedBootstrap, String>,
    stop_locked: impl FnOnce(&Path, &str, &Path) -> Result<(), String>,
) -> Result<(), String> {
    if attempt.bottle_id != bottle.id
        || attempt.catalog_id != "steam"
        || !matches!(
            attempt.stage,
            CatalogStage::InstallerComplete | CatalogStage::PreparingClient
        )
    {
        return Err("Only an unfinished Steam recipe can prepare its initial client".into());
    }
    // A crash/cancel during download must resume here, without repeating setup.
    attempt.set_stage(CatalogStage::InstallerComplete);
    let result = (|| {
        cancellation::check()?;
        let dir = bottle_dir(home, &bottle.id)?;
        let original = RecipeSteamTarget::open(&dir)?;
        if original.machine == 0x8664 {
            return Ok(());
        }
        attempt.set_stage(CatalogStage::PreparingClient);
        save_catalog_attempt(home, attempt)?;
        {
            let _lock = lock_bottle(home, &bottle.id)?;
            original.recheck()?;
            // SteamSetup can auto-start its old updater even after the direct
            // installer exits. Stop that initial recipe session before HTTP,
            // so a failed download leaves a genuinely idle, retryable bottle.
            stop_locked(home, &bottle.id, wine)?;
            cancellation::check()?;
            refuse_running_bottle(&dir)?;
            refuse_live_wine_server(&dir, WINE_SERVER_CHECK_TIMEOUT, Some(wine))?;
            prefix_processes::wait_until_idle(&dir, Duration::from_secs(8))?;
            original.recheck()?;
        }
        // Network work does not hold the ordinary bottle launch lock.
        let prepared = download(home)?;
        cancellation::check()?;
        let _lock = lock_bottle(home, &bottle.id)?;
        let saved = catalog_attempt(home, &bottle.id)?
            .ok_or("Steam's recipe receipt disappeared during preparation")?;
        if saved.catalog_id != "steam"
            || saved.stage != CatalogStage::PreparingClient
            || saved.installer_sha256 != attempt.installer_sha256
        {
            return Err("Steam's recipe changed during preparation".into());
        }
        original.check_directories()?;
        // A separately started normal updater may have completed migration.
        // Preserve its new client; do not stop another session after HTTP.
        let current = RecipeSteamTarget::open(&dir)?;
        if current.machine == 0x8664 {
            return Ok(());
        }
        original.recheck()?;
        cancellation::check()?;
        original.recheck()?;
        refuse_running_bottle(&dir)?;
        refuse_live_wine_server(&dir, WINE_SERVER_CHECK_TIMEOUT, Some(wine))?;
        prefix_processes::wait_until_idle(&dir, Duration::from_secs(8))?;
        refuse_running_bottle(&dir)?;
        original.recheck()?;
        let mut source = prepared.open_verified()?;
        let mut staged = StagedSteamClient::create(original.parent())?;
        copy_cancellable(&mut source, &mut staged.file)?;
        std::io::Write::flush(&mut staged.file).map_err(|e| e.to_string())?;
        staged
            .file
            .set_permissions(original.snapshot.permissions())
            .map_err(|e| e.to_string())?;
        staged.file.sync_all().map_err(|e| e.to_string())?;
        let (machine, digest) = inspect_client_machine_and_digest(&mut staged.file)?;
        if machine != 0x8664 || digest != prepared.sha256() {
            return Err(
                "Steam's staged current client failed verification; the original was preserved"
                    .into(),
            );
        }
        // Recheck both inputs and activity immediately before the atomic swap.
        let _ = prepared.open_verified()?;
        original.recheck()?;
        refuse_running_bottle(&dir)?;
        if prefix_processes::owner_count(&dir)? != 0 {
            return Err("Windows apps started while Steam's client was prepared; the original was preserved".into());
        }
        cancellation::check()?;
        let staged_snapshot = staged.recheck(prepared.sha256())?;
        original.recheck()?;
        let renamed = unsafe {
            libc::renameat(
                original.parent().as_raw_fd(),
                staged.name.as_ptr(),
                original.parent().as_raw_fd(),
                original.name.as_ptr(),
            )
        };
        if renamed != 0 {
            return Err(format!(
                "Could not install Steam's current client: {}",
                std::io::Error::last_os_error()
            ));
        }
        staged.renamed = true;
        original.check_directories()?;
        let installed = RecipeSteamTarget::open(&dir)?;
        if installed.machine != 0x8664
            || installed.digest != prepared.sha256()
            || installed.snapshot.dev() != staged_snapshot.dev()
            || installed.snapshot.ino() != staged_snapshot.ino()
        {
            return Err("Steam's current client changed immediately after installation; retry before opening it".into());
        }
        Ok(())
    })();
    // A bootstrap failure is a follow-up to completed setup. Failed would run
    // the old installer again on retry and repeat its 32-bit update crash.
    attempt.set_stage(CatalogStage::InstallerComplete);
    match result {
        Ok(()) => save_catalog_attempt(home, attempt),
        Err(error) => Err(followup_issue(home, attempt, error)),
    }
}

fn run_steam_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<CatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage == CatalogStage::PreparingClient {
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    let mut recovered_from_timeout = attempt.last_error.as_deref() == Some(STEAM_RECOVERY_NOTE);
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        let result = launch_wait_with_wine_checked(
            home,
            &bottle.id,
            installer.path(),
            wine,
            WaitLaunchOptions {
                quiet_msi: false,
                timeout: INSTALL_TIMEOUT,
                arguments: vec!["/S".into()],
            },
            |resolved| installer.verify_before_spawn(home, &bottle.id, wine, resolved, is_retry),
        )
        .map_err(|error| {
            let error = if cancellation::is_error(&error) {
                error
            } else {
                format!("Steam bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("Steam bottle {}: {error}", bottle.id),
            )
        })?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        let mut cleanup = None;
        if result.timed_out {
            match bottle_control::quit_windows_apps_with_wine(
                home,
                &bottle.id,
                wine,
                Duration::from_secs(8),
            ) {
                Ok(stopped) if stopped.server_stopped => {
                    cleanup = Some("Windows installer processes in this bottle were stopped.");
                    recovered_from_timeout = checked_steam_bootstrap(&dir).is_some();
                }
                Err(error) if cancellation::is_error(&error) => {
                    return Err(failed_attempt(home, &mut attempt, error));
                }
                Ok(_) | Err(_) => {
                    cleanup = Some("Windows installer processes may still be running.");
                }
            }
            cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        }
        if !recovered_from_timeout && (result.timed_out || result.exit_code != Some(0)) {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "Steam bottle {} remains available, but its installer did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
                    bottle.id,
                    result.exit_code,
                    result.signal,
                    result.timed_out,
                    cleanup.unwrap_or(""),
                    result.log_path
                ),
            ));
        }
        if steam_launcher_path(&dir).is_none() {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "Steam bottle {} remains available, but the installer did not create a Steam executable. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        if recovered_from_timeout {
            attempt.set_issue(STEAM_RECOVERY_NOTE);
        }
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    prepare_recipe_bootstrap(home, wine, &bottle, &mut attempt)?;
    let steam = steam_launcher_path(&dir).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "Steam bottle {}: previously installed Steam executable is missing; check the bottle",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_steam_launcher(home, &bottle.id, &steam).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "Steam bottle {}: installer completed, but launcher registration failed: {error}",
                bottle.id
            ),
        )
    })?;
    attempt.set_stage(CatalogStage::Installed);
    if recovered_from_timeout {
        attempt.set_issue(STEAM_RECOVERY_NOTE);
    }
    save_catalog_attempt(home, &attempt)?;
    cancellation::check()?;
    let launch = launch_with_wine_args(
        home,
        &bottle.id,
        &steam,
        wine,
        &[STEAM_CEF_SOFTWARE_FLAG.to_owned()],
        true,
    )
    .map_err(|error| {
        let error = if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Steam bottle {} has a saved launcher, but Steam could not start: {error}",
                bottle.id
            )
        };
        followup_issue(home, &mut attempt, error)
    })?;
    let install_log_path = attempt.install_log_path(&dir);
    Ok(CatalogInstallResult {
        bottle,
        launcher,
        installer_sha256: attempt.installer_sha256,
        download_url: STEAM_INSTALLER_URL,
        install_log_path,
        launch,
        setup_note: recovered_from_timeout.then_some(STEAM_RECOVERY_NOTE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("steam-install-{}", Uuid::new_v4()))
    }

    fn fake_installer(path: &Path) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; MIN_INSTALLER_BYTES as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x014c_u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
        let mut file = File::open(path).unwrap();
        inspect_pe_and_sha256(&mut file).unwrap()
    }

    fn fake_steam_client(path: &Path, machine: u16) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; MIN_INSTALLER_BYTES as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1_u16.to_le_bytes());
        bytes[0x94..0x96].copy_from_slice(&240_u16.to_le_bytes());
        bytes[0x96..0x98].copy_from_slice(&2_u16.to_le_bytes());
        let magic: u16 = if machine == 0x8664 { 0x20b } else { 0x10b };
        bytes[0x98..0x9a].copy_from_slice(&magic.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn saved_conventional_steam_clients_do_not_need_a_recipe_receipt() {
        let home = test_home();
        for location in ["Program Files (x86)/Steam", "Program Files/Steam"] {
            for name in ["steam.exe", "Steam.exe"] {
                for machine in [0x014c, 0x8664] {
                    let bottle = create(&home, "Imported client").unwrap();
                    let client = bottle_dir(&home, &bottle.id)
                        .unwrap()
                        .join("prefix/drive_c")
                        .join(location)
                        .join(name);
                    fake_steam_client(&client, machine);
                    let saved = add_launcher(&home, &bottle.id, "My Steam", &client).unwrap();
                    assert!(is_steam_client_launcher(&home, &bottle.id, &saved));
                    assert!(!is_recipe_steam_launcher(&home, &bottle.id, &saved));
                    assert!(catalog_attempt(&home, &bottle.id).unwrap().is_none());
                }
            }
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn steam_client_handling_refuses_arbitrary_paths_and_invalid_headers() {
        let home = test_home();
        let bottle = create(&home, "Client checks").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        for relative in [
            "drive_c/Other/Steam.exe",
            "drive_c/Program Files/Steam/Steam-copy.exe",
            "drive_c/Program Files/Other/Steam.exe",
        ] {
            let client = prefix.join(relative);
            fake_steam_client(&client, 0x014c);
            let saved = add_launcher(&home, &bottle.id, "Steam", &client).unwrap();
            assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        }
        let client = prefix.join("drive_c/Program Files/Steam/Steam.exe");
        fake_steam_client(&client, 0x014c);
        let saved = add_launcher(&home, &bottle.id, "Steam", &client).unwrap();
        for (offset, bytes) in [
            (0usize, b"no".as_slice()),
            (0x80, b"PX\0\0".as_slice()),
            (0x84, 0xaa64_u16.to_le_bytes().as_slice()),
            (0x3c, 0xffff_ffff_u32.to_le_bytes().as_slice()),
            (0x3c, 32_u32.to_le_bytes().as_slice()),
        ] {
            fake_steam_client(&client, 0x014c);
            let mut file = OpenOptions::new().write(true).open(&client).unwrap();
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            use std::io::Write;
            file.write_all(bytes).unwrap();
            assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        }
        // A valid-looking header far into a large/sparse file is outside the
        // bounded scan policy and must not turn a launch into a full-file read.
        fake_steam_client(&client, 0x014c);
        let mut file = OpenOptions::new().write(true).open(&client).unwrap();
        file.set_len(1024 * 1024).unwrap();
        file.seek(SeekFrom::Start(0x3c)).unwrap();
        use std::io::Write;
        file.write_all(&(128 * 1024_u32).to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(128 * 1024)).unwrap();
        file.write_all(b"PE\0\0\x4c\x01").unwrap();
        assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn steam_client_handling_refuses_file_and_parent_symlinks() {
        let home = test_home();
        let bottle = create(&home, "No links").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let root = prefix.join("drive_c/Program Files/Steam");
        let client = root.join("Steam.exe");
        fake_steam_client(&client, 0x014c);
        let mut saved = add_launcher(&home, &bottle.id, "Steam", &client).unwrap();
        let alias = prefix.join("drive_c/Steam.exe");
        symlink(&client, &alias).unwrap();
        saved.path = alias.display().to_string();
        assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        let outside = home.join("external/Steam.exe");
        fake_steam_client(&outside, 0x8664);
        fs::remove_file(&client).unwrap();
        symlink(&outside, &client).unwrap();
        saved.path = client.display().to_string();
        assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        fs::remove_file(&client).unwrap();
        fs::remove_dir(&root).unwrap();
        symlink(outside.parent().unwrap(), &root).unwrap();
        assert!(!is_steam_client_launcher(&home, &bottle.id, &saved));
        assert!(catalog_attempt(&home, &bottle.id).unwrap().is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn imported_steam_client_uses_native_mode_and_one_software_flag() {
        let home = test_home();
        let bottle = create(&home, "Imported Steam").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let client = prefix.join("drive_c/Program Files (x86)/Steam/steam.exe");
        fake_steam_client(&client, 0x014c);
        let saved = add_launcher(&home, &bottle.id, "Steam", &client).unwrap();
        let wine = fake_wine(&home, 0);
        let (chosen, request) = steam_install_request(&home, &bottle.id, 480).unwrap();
        assert_eq!(chosen.id, saved.id);
        assert_eq!(request, ["steam://install/480"]);
        assert_eq!(
            launcher_arguments(true, false, &saved.saved_arguments, &request).unwrap(),
            [STEAM_CEF_SOFTWARE_FLAG, "steam://install/480"]
        );
        run_launcher_with_wine(&home, &bottle.id, &saved.id, &wine, &request).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let arguments =
                fs::read_to_string(prefix.join("launch-arguments.txt")).unwrap_or_default();
            let mode = fs::read_to_string(prefix.join("launch-mode.txt")).unwrap_or_default();
            if arguments.ends_with("steam.exe|-cef-disable-gpu\n")
                && mode.ends_with("steam.exe|1\n")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Steam mode or launch arguments missing"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(catalog_attempt(&home, &bottle.id).unwrap().is_none());
        assert!(!is_recipe_steam_launcher(&home, &bottle.id, &saved));
        fs::remove_dir_all(home).unwrap();
    }

    fn fake_wine(home: &Path, installer_exit: i32) -> PathBuf {
        fake_wine_with_executable(home, installer_exit, "Steam.exe")
    }

    fn fake_wine_with_executable(home: &Path, installer_exit: i32, executable: &str) -> PathBuf {
        assert!(matches!(executable, "Steam.exe" | "steam.exe"));
        let wine = home.join("wine");
        fake_steam_client(&home.join("wine.client"), 0x8664);
        let script = format!(
            "#!/bin/sh\nprintf '%s|%s\\n' \"$1\" \"${{CASKRUN_STEAM_MODE-}}\" >> \"$WINEPREFIX/launch-mode.txt\"\nprintf '%s|%s\\n' \"$1\" \"${{2-}}\" >> \"$WINEPREFIX/launch-arguments.txt\"\ncase \"$1\" in\n  *SteamSetup.exe)\n    test \"$2\" = /S || exit 91\n    echo installing >> \"$WINEPREFIX/installer-invocations.txt\"\n    if test {installer_exit} -eq 0; then\n      mkdir -p \"$WINEPREFIX/drive_c/Program Files (x86)/Steam\"\n      cp \"$0.client\" \"$WINEPREFIX/drive_c/Program Files (x86)/Steam/{executable}\"\n    fi\n    exit {installer_exit};;\n  *Steam.exe|*steam.exe) echo launched-Steam; exit 0;;\n  *) exit 92;;\nesac\n"
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

    fn bootstrap_fixture() -> (PathBuf, PathBuf, Bottle, CatalogAttempt, PathBuf) {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let bottle = create(&home, "Steam").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let digest = fake_installer(&dir.join("installers/SteamSetup.exe"));
        let client = dir.join("prefix/drive_c/Program Files (x86)/Steam/Steam.exe");
        fake_steam_client(&client, 0x014c);
        let mut attempt =
            CatalogAttempt::new(&bottle.id, "steam", digest, STEAM_INSTALLER_URL.into()).unwrap();
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(&home, &attempt).unwrap();
        (home, wine, bottle, attempt, client)
    }

    fn current_test_client(home: &Path) -> Vec<u8> {
        let path = home.join("current-test-client.exe");
        fake_steam_client(&path, 0x8664);
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() = 42;
        fs::remove_file(path).unwrap();
        steam_bootstrap::inspect_x64_pe(&bytes).unwrap();
        bytes
    }

    fn assert_no_staged_client(client: &Path) {
        assert!(
            fs::read_dir(client.parent().unwrap())
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".caskrun-steam-bootstrap-")
                })
        );
    }

    #[test]
    fn recipe_updates_only_its_32_bit_bootstrap_then_retry_skips_setup() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let old = fs::read(&client).unwrap();
        let payload = current_test_client(&home);
        let mut stopped = false;
        prepare_recipe_bootstrap_with(
            &home,
            &wine,
            &bottle,
            &mut attempt,
            |home| steam_bootstrap::from_test_payload(home, &payload),
            |_, _, _| {
                assert_eq!(fs::read(&client).unwrap(), old);
                stopped = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(stopped);
        assert_eq!(fs::read(&client).unwrap(), payload);
        assert_eq!(attempt.stage, CatalogStage::InstallerComplete);
        assert_no_staged_client(&client);
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let result = retry_catalog_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(result.bottle.id, bottle.id);
        assert!(!dir.join("prefix/installer-invocations.txt").exists());
        assert_eq!(
            catalog_attempt(&home, &bottle.id).unwrap().unwrap().stage,
            CatalogStage::Installed
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn recipe_preserves_existing_64_bit_client_without_network_or_stop() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        fake_steam_client(&client, 0x8664);
        let before = fs::read(&client).unwrap();
        let snapshot = fs::metadata(&client).unwrap();
        prepare_recipe_bootstrap_with(
            &home,
            &wine,
            &bottle,
            &mut attempt,
            |_| panic!("a current client must not trigger a download"),
            |_, _, _| panic!("a current client must not be stopped"),
        )
        .unwrap();
        assert!(same_snapshot(&snapshot, &fs::metadata(&client).unwrap()));
        assert_eq!(fs::read(&client).unwrap(), before);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bootstrap_download_failure_and_cancellation_preserve_completed_setup_for_retry() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let before = fs::read(&client).unwrap();
        for message in ["download unavailable", cancellation::ERROR] {
            let error = prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |_| Err(message.into()),
                |_, _, _| Ok(()),
            )
            .unwrap_err();
            assert_eq!(error, message);
            let saved = catalog_attempt(&home, &bottle.id).unwrap().unwrap();
            assert_eq!(saved.stage, CatalogStage::InstallerComplete);
            assert_eq!(saved.last_error.as_deref(), Some(message));
            assert_eq!(fs::read(&client).unwrap(), before);
            assert_no_staged_client(&client);
        }
        // A previous app exit during PreparingClient resumes the same phase.
        attempt.set_stage(CatalogStage::PreparingClient);
        save_catalog_attempt(&home, &attempt).unwrap();
        fake_steam_client(&client, 0x8664);
        retry_catalog_with_wine(&home, &bottle.id, &wine).unwrap();
        assert!(
            !bottle_dir(&home, &bottle.id)
                .unwrap()
                .join("prefix/installer-invocations.txt")
                .exists()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn installed_or_unrelated_receipts_cannot_authorize_bootstrap_replacement() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let before = fs::read(&client).unwrap();
        attempt.set_stage(CatalogStage::Installed);
        assert!(
            prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |_| panic!("installed clients must not download"),
                |_, _, _| panic!("installed clients must not stop")
            )
            .unwrap_err()
            .contains("unfinished Steam recipe")
        );
        attempt.set_stage(CatalogStage::InstallerComplete);
        attempt.catalog_id = "gog-galaxy".into();
        assert!(
            prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |_| panic!("unrelated clients must not download"),
                |_, _, _| panic!("unrelated clients must not stop")
            )
            .is_err()
        );
        assert_eq!(fs::read(&client).unwrap(), before);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bootstrap_replacement_refuses_changed_target_or_parent_link() {
        for swap_parent in [false, true] {
            let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
            let payload = current_test_client(&home);
            let parent = client.parent().unwrap();
            let saved_parent = parent.with_file_name("Steam-retained");
            let error = prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |home| {
                    if swap_parent {
                        fs::rename(parent, &saved_parent).unwrap();
                        symlink(&saved_parent, parent).unwrap();
                    } else {
                        let mut changed = fs::read(&client).unwrap();
                        *changed.last_mut().unwrap() = 7;
                        fs::write(&client, changed).unwrap();
                    }
                    steam_bootstrap::from_test_payload(home, &payload)
                },
                |_, _, _| Ok(()),
            )
            .unwrap_err();
            assert!(error.contains("changed"), "{error}");
            assert_ne!(fs::read(&client).unwrap(), payload);
            assert_eq!(attempt.stage, CatalogStage::InstallerComplete);
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn bootstrap_replacement_refuses_a_live_tracked_pid_after_stop() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let before = fs::read(&client).unwrap();
        let payload = current_test_client(&home);
        let error = prepare_recipe_bootstrap_with(
            &home,
            &wine,
            &bottle,
            &mut attempt,
            |home| steam_bootstrap::from_test_payload(home, &payload),
            |home, id, _| {
                save_json(
                    &bottle_dir(home, id)?.join("active-processes.json"),
                    &vec![std::process::id()],
                )
            },
        )
        .unwrap_err();
        assert!(error.contains("Close Windows apps"), "{error}");
        assert_eq!(fs::read(&client).unwrap(), before);
        assert_no_staged_client(&client);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_new_launch_during_bootstrap_download_is_never_stopped_or_replaced() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let before = fs::read(&client).unwrap();
        let payload = current_test_client(&home);
        let stops = std::cell::Cell::new(0);
        let error = prepare_recipe_bootstrap_with(
            &home,
            &wine,
            &bottle,
            &mut attempt,
            |home| {
                assert_eq!(
                    stops.get(),
                    1,
                    "initial installer session is stopped before HTTP"
                );
                save_json(
                    &bottle_dir(home, &bottle.id)?.join("active-processes.json"),
                    &vec![std::process::id()],
                )?;
                steam_bootstrap::from_test_payload(home, &payload)
            },
            |_, _, _| {
                stops.set(stops.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("Close Windows apps"), "{error}");
        assert_eq!(
            stops.get(),
            1,
            "a new session must not receive another stop"
        );
        assert_eq!(fs::read(&client).unwrap(), before);
        assert_no_staged_client(&client);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn recipe_refuses_a_linked_client_before_network_or_stop() {
        let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
        let outside = home.join("outside-client.exe");
        fake_steam_client(&outside, 0x014c);
        let before = fs::read(&outside).unwrap();
        fs::remove_file(&client).unwrap();
        symlink(&outside, &client).unwrap();
        assert!(
            prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |_| panic!("linked clients must not download"),
                |_, _, _| panic!("linked clients must not stop"),
            )
            .unwrap_err()
            .contains("regular file")
        );
        assert_eq!(fs::read(&outside).unwrap(), before);
        assert!(
            fs::symlink_metadata(&client)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bootstrap_directory_guard_refuses_a_linked_intermediate_ancestor() {
        let (home, _, bottle, _, client) = bootstrap_fixture();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let target = RecipeSteamTarget::open(&dir).unwrap();
        let before = fs::read(&client).unwrap();
        let programs = dir.join("prefix/drive_c/Program Files (x86)");
        let moved = home.join("moved-programs");
        fs::rename(&programs, &moved).unwrap();
        symlink(&moved, &programs).unwrap();
        // Every retained leaf still names the original inode through the new
        // link, including Steam's folder. The full-path check must refuse it.
        for (path, guard, snapshot) in &target.directories {
            bottle_control::assert_same_prefix(path, guard, snapshot).unwrap();
        }
        assert!(target.check_directories().is_err());
        assert_eq!(fs::read(moved.join("Steam/Steam.exe")).unwrap(), before);
        assert_no_staged_client(&moved.join("Steam/Steam.exe"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bootstrap_replacement_refuses_modified_prepared_payload_and_failed_stop() {
        for invalid_payload in [false, true] {
            let (home, wine, bottle, mut attempt, client) = bootstrap_fixture();
            let before = fs::read(&client).unwrap();
            let payload = current_test_client(&home);
            let error = prepare_recipe_bootstrap_with(
                &home,
                &wine,
                &bottle,
                &mut attempt,
                |home| {
                    let prepared = steam_bootstrap::from_test_payload(home, &payload)?;
                    if invalid_payload {
                        let mut changed = payload.clone();
                        *changed.last_mut().unwrap() = 9;
                        fs::write(prepared.path(), changed).unwrap();
                    }
                    Ok(prepared)
                },
                |_, _, _| {
                    if invalid_payload {
                        Ok(())
                    } else {
                        Err("installer session still running".into())
                    }
                },
            )
            .unwrap_err();
            assert!(
                error.contains(if invalid_payload {
                    "changed"
                } else {
                    "still running"
                }),
                "{error}"
            );
            assert_eq!(fs::read(&client).unwrap(), before);
            assert_no_staged_client(&client);
            assert_eq!(attempt.stage, CatalogStage::InstallerComplete);
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn staged_bootstrap_name_cannot_be_exchanged_for_a_link() {
        let (home, _, _, _, client) = bootstrap_fixture();
        let parent = open_stable_directory(client.parent().unwrap()).unwrap().0;
        let mut staged = StagedSteamClient::create(&parent).unwrap();
        let path = client
            .parent()
            .unwrap()
            .join(OsStr::from_bytes(staged.name.to_bytes()));
        fs::remove_file(&path).unwrap();
        symlink(&client, &path).unwrap();
        assert!(staged.recheck(&"0".repeat(64)).is_err());
        drop(staged);
        assert!(!path.exists());
        assert!(client.is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn canceled_catalog_download_removes_partial_installer() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let curl = home.join("fake-curl");
        let marker = home.join("download-started");
        fs::write(
            &curl,
            format!(
                "#!/bin/sh\nprintf 'partial installer'\n: > '{}'\nexec /bin/sleep 30\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&curl).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&curl, permissions).unwrap();
        let (request, cancel_guard) = cancellation::test_request();
        let signal = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let started = marker.exists();
            request.store(true, std::sync::atomic::Ordering::Release);
            started
        });
        let error = download_steam_installer_with_curl(&home, &curl)
            .err()
            .expect("canceled download must fail");
        assert!(signal.join().unwrap(), "{error}");
        assert_eq!(error, cancellation::ERROR);
        assert!(cancellation::is_error(&error));
        assert_eq!(
            fs::read_dir(home.join("downloads"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|value| value == "exe"))
                .count(),
            0
        );
        drop(cancel_guard);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn canceled_catalog_installer_keeps_a_failed_retry_receipt() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        fs::write(
            &wine,
            b"#!/bin/sh\ncase \"$1\" in\n  *SteamSetup.exe) : > \"$WINEPREFIX/installer-started\"; exec /bin/sleep 30;;\n  *) exit 0;;\nesac\n",
        )
        .unwrap();
        let bottle = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let installer_path = real_subdirectory(&dir, "installers")
            .unwrap()
            .join("SteamSetup.exe");
        let sha256 = fake_installer(&installer_path);
        let installer =
            VerifiedInstaller::open(installer_path, &sha256, inspect_pe_and_sha256).unwrap();
        let attempt =
            CatalogAttempt::new(&bottle.id, "steam", sha256, STEAM_INSTALLER_URL.into()).unwrap();
        save_catalog_attempt(&home, &attempt).unwrap();
        let marker = dir.join("prefix/installer-started");
        let (request, cancel_guard) = cancellation::test_request();
        let signal = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let started = marker.exists();
            request.store(true, std::sync::atomic::Ordering::Release);
            started
        });
        let error = run_steam_installer(&home, &wine, bottle.clone(), installer, attempt, false)
            .err()
            .expect("canceled installer must fail");
        assert!(signal.join().unwrap(), "{error}");
        assert_eq!(error, cancellation::ERROR);
        let saved = catalog_attempt(&home, &bottle.id).unwrap().unwrap();
        assert_eq!(saved.stage, CatalogStage::Failed);
        assert!(
            saved
                .last_error
                .as_deref()
                .is_some_and(cancellation::is_error)
        );
        assert!(dir.join("installers/SteamSetup.exe").exists());
        drop(cancel_guard);
        let prepared = prepare_retry(&home, &bottle.id, "steam", &wine, inspect_pe_and_sha256)
            .expect("canceled setup should keep a retryable bottle receipt");
        assert_eq!(prepared.2.stage, CatalogStage::Failed);
        drop(prepared);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cancellation_with_unwritable_receipt_reports_recovery_failure() {
        let home = test_home();
        let bottle = create(&home, "Steam").unwrap();
        let mut attempt = CatalogAttempt::new(
            &bottle.id,
            "steam",
            "0".repeat(64),
            STEAM_INSTALLER_URL.into(),
        )
        .unwrap();
        let receipt = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join(CATALOG_ATTEMPT_FILE);
        fs::create_dir(&receipt).unwrap();

        let error = failed_attempt(&home, &mut attempt, cancellation::ERROR.into());
        assert!(error.starts_with("Could not save recovery status after cancellation:"));
        assert!(!cancellation::is_error(&error));
        assert_eq!(attempt.stage, CatalogStage::Failed);

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rejects_non_pe_and_unexpected_machine_before_execution() {
        let home = test_home();
        let path = home.join("installer.exe");
        let _ = fake_installer(&path);
        let mut file = File::open(&path).unwrap();
        assert_eq!(inspect_pe_and_sha256(&mut file).unwrap().len(), 64);
        fs::write(&path, vec![b'H'; MIN_INSTALLER_BYTES as usize]).unwrap();
        let mut file = File::open(&path).unwrap();
        assert!(inspect_pe_and_sha256(&mut file).unwrap_err().contains("MZ"));
        let _ = fake_installer(&path);
        let mut contents = fs::read(&path).unwrap();
        contents[0x84..0x86].copy_from_slice(&0xaa64_u16.to_le_bytes());
        fs::write(&path, contents).unwrap();
        let mut file = File::open(&path).unwrap();
        assert!(
            inspect_pe_and_sha256(&mut file)
                .unwrap_err()
                .contains("x86 or x64")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn silent_install_creates_bottle_launcher_and_starts_steam() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let result = install_steam_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: sha256.clone(),
            },
        )
        .unwrap();
        assert_eq!(result.bottle.name, "Steam");
        assert_eq!(result.launcher.name, "Steam");
        assert_eq!(result.installer_sha256, sha256);
        assert_eq!(list_launchers(&home, &result.bottle.id).unwrap().len(), 1);
        assert!(Path::new(&result.install_log_path).is_file());
        assert!(Path::new(&result.launch.log_path).is_file());
        let prefix = bottle_dir(&home, &result.bottle.id).unwrap().join("prefix");
        let deadline = Instant::now() + Duration::from_secs(2);
        let first = loop {
            let first = fs::read_to_string(prefix.join("launch-arguments.txt")).unwrap_or_default();
            if first
                .lines()
                .any(|line| line.ends_with("Steam.exe|-cef-disable-gpu"))
                || Instant::now() >= deadline
            {
                break first;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            first
                .lines()
                .any(|line| line.ends_with("Steam.exe|-cef-disable-gpu"))
        );
        assert!(is_recipe_steam_launcher(
            &home,
            &result.bottle.id,
            &result.launcher
        ));
        run_launcher_with_wine(&home, &result.bottle.id, &result.launcher.id, &wine, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let saved = loop {
            let saved = fs::read_to_string(prefix.join("launch-arguments.txt")).unwrap();
            if saved
                .lines()
                .filter(|line| line.ends_with("Steam.exe|-cef-disable-gpu"))
                .count()
                >= 2
                || Instant::now() >= deadline
            {
                break saved;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(
            saved
                .lines()
                .filter(|line| line.ends_with("Steam.exe|-cef-disable-gpu"))
                .count(),
            2
        );
        let modes = fs::read_to_string(prefix.join("launch-mode.txt")).unwrap();
        assert_eq!(
            modes
                .lines()
                .filter(|line| line.ends_with("Steam.exe|1"))
                .count(),
            2
        );
        let other = prefix.join("drive_c/Program Files (x86)/Steam/other.exe");
        fs::write(&other, b"MZfake other app").unwrap();
        let other_launcher = add_launcher(&home, &result.bottle.id, "Other", &other).unwrap();
        assert!(!is_recipe_steam_launcher(
            &home,
            &result.bottle.id,
            &other_launcher
        ));
        run_launcher_with_wine(&home, &result.bottle.id, &other_launcher.id, &wine, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let saved = loop {
            let saved = fs::read_to_string(prefix.join("launch-arguments.txt")).unwrap();
            if saved.lines().any(|line| line.ends_with("other.exe|")) || Instant::now() >= deadline
            {
                break saved;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(saved.lines().any(|line| line.ends_with("other.exe|")));
        let modes = fs::read_to_string(prefix.join("launch-mode.txt")).unwrap();
        assert!(modes.lines().any(|line| line.ends_with("other.exe|")));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn lowercase_steam_executable_is_saved_and_launched() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine_with_executable(&home, 0, "steam.exe");
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let result =
            install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 })
                .unwrap();
        assert!(Path::new(&result.launcher.path).is_file());
        assert!(
            result
                .launcher
                .path
                .to_ascii_lowercase()
                .ends_with("/steam.exe")
        );
        assert_eq!(
            catalog_attempt(&home, &result.bottle.id)
                .unwrap()
                .unwrap()
                .stage,
            CatalogStage::Installed
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn steam_launcher_search_rejects_other_names_and_symlinks() {
        let home = test_home();
        let bottle = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let steam_dir = dir.join("prefix/drive_c/Program Files (x86)/Steam");
        fs::create_dir_all(&steam_dir).unwrap();
        fs::write(steam_dir.join("SteamLauncher.exe"), b"other").unwrap();
        assert!(steam_launcher_path(&dir).is_none());
        symlink(
            steam_dir.join("SteamLauncher.exe"),
            steam_dir.join("steam.exe"),
        )
        .unwrap();
        assert!(steam_launcher_path(&dir).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn steam_install_stops_if_wineboot_changes_installer_before_spawn() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        fs::write(
            home.join("wineboot"),
            b"#!/bin/sh\nprintf changed >> \"$WINEPREFIX/../installers/SteamSetup.exe\"\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let error = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 })
            .err()
            .unwrap();
        assert!(error.contains("changed after verification"), "{error}");
        let bottle = list(&home).unwrap().pop().unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        assert!(!dir.join("prefix/installer-invocations.txt").exists());
        assert!(list_launchers(&home, &bottle.id).unwrap().is_empty());
        assert_eq!(
            catalog_attempt(&home, &bottle.id).unwrap().unwrap().stage,
            CatalogStage::Failed
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn verified_installer_rejects_same_content_replaced_with_new_inode() {
        let home = test_home();
        let bottle = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let path = dir.join("installers/SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let verified =
            VerifiedInstaller::open(path.clone(), &sha256, inspect_pe_and_sha256).unwrap();
        let replacement = path.with_extension("replacement");
        fs::copy(&path, &replacement).unwrap();
        fs::rename(&replacement, &path).unwrap();
        let resolved = path.canonicalize().unwrap();
        let error = verified
            .verify_before_spawn(&home, &bottle.id, Path::new("/unused"), &resolved, false)
            .err()
            .unwrap();
        assert!(error.contains("changed after verification"), "{error}");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_rechecks_bottle_activity_after_initial_preflight() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        fs::write(
            home.join("wineboot"),
            b"#!/bin/sh\nprintf '[%s]\\n' \"$PPID\" > \"$WINEPREFIX/../active-processes.json\"\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        let bottle = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let installer = dir.join("installers/SteamSetup.exe");
        let digest = fake_installer(&installer);
        let mut attempt =
            CatalogAttempt::new(&bottle.id, "steam", digest, STEAM_INSTALLER_URL.into()).unwrap();
        attempt.set_stage(CatalogStage::Failed);
        save_catalog_attempt(&home, &attempt).unwrap();
        let error = retry_catalog_with_wine(&home, &bottle.id, &wine)
            .err()
            .unwrap();
        assert!(error.contains("Close Windows apps"), "{error}");
        assert!(!dir.join("prefix/installer-invocations.txt").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_silent_install_retains_bottle_and_diagnostic_log() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let error = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 })
            .err()
            .unwrap();
        let bottles = list(&home).unwrap();
        assert_eq!(bottles.len(), 1);
        assert!(error.contains(&bottles[0].id));
        assert!(error.contains("exit Some(37)"));
        assert_eq!(list_logs(&home, &bottles[0].id).unwrap().len(), 2);
        assert!(list_launchers(&home, &bottles[0].id).unwrap().is_empty());
        let attempt = catalog_attempt(&home, &bottles[0].id).unwrap().unwrap();
        assert_eq!(attempt.catalog_id, "steam");
        assert_eq!(attempt.stage, CatalogStage::Failed);
        assert!(attempt.last_log_name.is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_uses_verified_installer_and_same_failed_steam_bottle() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let _ = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 });
        let first = list(&home).unwrap().pop().unwrap();
        let first_logs = list_logs(&home, &first.id).unwrap().len();
        assert_eq!(catalog_attempts(&home).unwrap().len(), 1);
        fake_wine(&home, 0);
        let retried = retry_catalog_with_wine(&home, &first.id, &wine).unwrap();
        assert_eq!(retried.bottle.id, first.id);
        assert_eq!(list(&home).unwrap().len(), 1);
        assert_eq!(list_launchers(&home, &first.id).unwrap().len(), 1);
        assert!(list_logs(&home, &first.id).unwrap().len() > first_logs);
        assert_eq!(
            catalog_attempt(&home, &first.id).unwrap().unwrap().stage,
            CatalogStage::Installed
        );
        let invocations = fs::read_to_string(
            bottle_dir(&home, &first.id)
                .unwrap()
                .join("prefix/installer-invocations.txt"),
        )
        .unwrap();
        assert_eq!(invocations.lines().count(), 2);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_rejects_legacy_or_tampered_steam_installer_without_execution() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let legacy = create(&home, "Legacy Steam").unwrap();
        assert!(
            retry_catalog_with_wine(&home, &legacy.id, &wine)
                .err()
                .unwrap()
                .contains("no verified catalog installer receipt")
        );
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let _ = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 });
        let bottle = list(&home)
            .unwrap()
            .into_iter()
            .find(|bottle| bottle.id != legacy.id)
            .unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let installer = dir.join("installers/SteamSetup.exe");
        let mut bytes = fs::read(&installer).unwrap();
        *bytes.last_mut().unwrap() = 1;
        fs::write(&installer, bytes).unwrap();
        let before = list_logs(&home, &bottle.id).unwrap().len();
        assert!(
            retry_catalog_with_wine(&home, &bottle.id, &wine)
                .err()
                .unwrap()
                .contains("installer changed")
        );
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before);
        assert_eq!(list(&home).unwrap().len(), 2);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn completed_steam_installer_receipt_finishes_without_running_setup_again() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let bottle = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let installer = dir.join("installers/SteamSetup.exe");
        let sha256 = fake_installer(&installer);
        let steam = dir.join("prefix/drive_c/Program Files (x86)/Steam/Steam.exe");
        fs::create_dir_all(steam.parent().unwrap()).unwrap();
        fake_steam_client(&steam, 0x8664);
        let mut attempt =
            CatalogAttempt::new(&bottle.id, "steam", sha256, STEAM_INSTALLER_URL.into()).unwrap();
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(&home, &attempt).unwrap();
        retry_catalog_with_wine(&home, &bottle.id, &wine).unwrap();
        assert!(!dir.join("prefix/installer-invocations.txt").exists());
        assert_eq!(list_launchers(&home, &bottle.id).unwrap().len(), 1);
        assert_eq!(
            catalog_attempt(&home, &bottle.id).unwrap().unwrap().stage,
            CatalogStage::Installed
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn copied_catalog_receipt_cannot_authorize_retry_in_another_bottle() {
        let home = test_home();
        let original = create(&home, "Steam").unwrap();
        let dir = bottle_dir(&home, &original.id).unwrap();
        let installer = dir.join("installers/SteamSetup.exe");
        let digest = fake_installer(&installer);
        let attempt =
            CatalogAttempt::new(&original.id, "steam", digest, STEAM_INSTALLER_URL.into()).unwrap();
        save_catalog_attempt(&home, &attempt).unwrap();
        let copied = duplicate_bottle_with_runtime(&home, &original.id, None, None).unwrap();
        assert!(catalog_attempt(&home, &copied.id).unwrap().is_none());
        assert_eq!(catalog_attempts(&home).unwrap().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_refuses_to_run_while_bottle_has_a_live_tracked_process() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let _ = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 });
        let bottle = list(&home).unwrap().pop().unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let before = list_logs(&home, &bottle.id).unwrap().len();
        assert!(
            retry_catalog_with_wine(&home, &bottle.id, &wine)
                .err()
                .unwrap()
                .contains("Close Windows apps")
        );
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_refuses_untracked_wine_activity_after_direct_installer_exits() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let _ = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 });
        let bottle = list(&home).unwrap().pop().unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        assert!(read_active_processes(&dir).unwrap().is_empty());
        let before = fs::read_to_string(dir.join("prefix/installer-invocations.txt")).unwrap();
        // An untracked Wine descendant can keep its server alive after the
        // direct installer process is gone. Mock -w's non-idle result.
        fs::write(home.join("wineserver"), b"#!/bin/sh\nexit 1\n").unwrap();
        let error = retry_catalog_with_wine(&home, &bottle.id, &wine)
            .err()
            .unwrap();
        assert!(error.contains("server is idle"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.join("prefix/installer-invocations.txt")).unwrap(),
            before
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn active_cancel_channel_without_request_preserves_steam_timeout_recovery() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("SteamSetup.exe");
        let sha256 = fake_installer(&path);
        let _ = install_steam_from_download(&home, &wine, DownloadedInstaller { path, sha256 });
        let bottle = list(&home).unwrap().pop().unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let steam = dir.join("prefix/drive_c/Program Files (x86)/Steam/Steam.exe");
        fs::create_dir_all(steam.parent().unwrap()).unwrap();
        fake_steam_client(&steam, 0x8664);
        let mut attempt = catalog_attempt(&home, &bottle.id).unwrap().unwrap();
        attempt.set_failure("Steam installer timeout true");
        save_catalog_attempt(&home, &attempt).unwrap();

        // A GUI setup always has a cancellation channel. Without an actual
        // Cancel request, the timeout path must still stop the bottle's
        // Windows processes so Steam's bootstrap can recover.
        fs::write(
            home.join("wineserver"),
            b"#!/bin/sh\ncase \"$1\" in\n  -k) : > \"$WINEPREFIX/timeout-cleanup-invoked\";;\n  -w) exit 0;;\nesac\n",
        )
        .unwrap();
        let (_request, cancel_guard) = cancellation::test_request();
        let cleanup = cleanup_timed_out_catalog_install(&home, &bottle.id, &wine).unwrap();
        assert!(cleanup.contains("were stopped"), "{cleanup}");
        assert!(dir.join("prefix/timeout-cleanup-invoked").exists());

        let result = retry_catalog_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(result.setup_note, Some(STEAM_RECOVERY_NOTE));
        assert_eq!(list_launchers(&home, &bottle.id).unwrap().len(), 1);
        assert_eq!(
            fs::read_to_string(dir.join("prefix/installer-invocations.txt"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let saved = catalog_attempt(&home, &bottle.id).unwrap().unwrap();
        assert_eq!(saved.stage, CatalogStage::Installed);
        assert_eq!(saved.last_error.as_deref(), Some(STEAM_RECOVERY_NOTE));
        drop(cancel_guard);
        fs::remove_dir_all(home).unwrap();
    }
}
