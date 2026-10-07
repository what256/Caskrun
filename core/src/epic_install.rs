//! Original, experimental installer flow for Epic's Windows launcher.
//! The launcher, account sign-in, game downloads, and gameplay still require
//! separate compatibility validation. No Epic installer is bundled here.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

// Epic's public download service currently redirects to an MSI on its CDN.
// Resolve the redirect on each install; a changed host or path must be reviewed.
const EPIC_INSTALLER_ENDPOINT: &str = "https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/installer/download/EpicGamesLauncherInstaller.msi";
const EPIC_DOWNLOAD_PAGE: &str = "https://store.epicgames.com/en-US/download";
const EPIC_CDN_HOST: &str = "epicgames-download1.akamaized.net";
const EPIC_CDN_PATH: &str = "/Builds/UnrealEngineLauncher/Installers/Windows/";
const MAX_HEADER_BYTES: u64 = 64 * 1024;
const MIN_INSTALLER_BYTES: u64 = 100 * 1024;
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
const REDIRECT_TIMEOUT: Duration = Duration::from_secs(35);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60 + 10);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Serialize)]
pub(super) struct EpicCatalogInstallResult {
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

pub(super) fn install_catalog(home: &Path, id: &str) -> Result<EpicCatalogInstallResult, String> {
    if id != "epic-games-store" {
        return Err(format!("No Epic installer is available for {id}"));
    }
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let installer = download_epic_installer(home)?;
    install_epic_from_download(home, &wine, installer)
}

fn download_epic_installer(home: &Path) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let id = Uuid::new_v4();
    let header_path = downloads.join(format!("EpicInstaller-{id}.headers.tmp"));
    let path = downloads.join(format!("EpicInstaller-{id}.msi"));
    let log_path = downloads.join(format!("EpicInstaller-{id}.download.log"));
    let mut headers = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&header_path)
        .map_err(|error| format!("Could not prepare Epic download headers: {error}"))?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare Epic download log: {error}"))?;

    // -q must be first, disabling ~/.curlrc. The first request is HEAD without
    // redirect following; the returned destination is checked before any GET.
    let redirect_result = (|| -> Result<String, String> {
        cancellation::check()?;
        let mut child = Command::new("/usr/bin/curl")
            .args([
                "-q",
                "--head",
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
                "30",
                EPIC_INSTALLER_ENDPOINT,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(headers.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .spawn()
            .map_err(|error| format!("Could not contact Epic's installer service: {error}"))?;
        let status = wait_child_bounded(&mut child, REDIRECT_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("Epic's installer service did not return a usable redirect".into());
        }
        let length = headers.metadata().map_err(|e| e.to_string())?.len();
        if length == 0 || length > MAX_HEADER_BYTES {
            return Err("Epic's installer response headers had an unexpected size".into());
        }
        headers
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::with_capacity(length as usize);
        headers
            .by_ref()
            .take(MAX_HEADER_BYTES)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        let location = redirect_location(&raw)?;
        checked_epic_installer_url(&location)?;
        Ok(location)
    })();
    // Response headers can contain transient service cookies; never retain them.
    let _ = fs::remove_file(&header_path);
    let download_url = redirect_result.map_err(|error| {
        if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Could not resolve Epic's Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                EPIC_DOWNLOAD_PAGE
            )
        }
    })?;
    cancellation::check()?;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| format!("Could not prepare Epic installer download: {error}"))?;
    // The final GET also refuses redirects. A new CDN destination needs review.
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
            "600",
            "--max-filesize",
            "536870912",
            &download_url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("Could not start Epic installer download: {error}"))?;
    let status = match wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT) {
        Ok(status) => status,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(error);
        }
    };
    if !status.is_some_and(|status| status.success()) {
        let _ = fs::remove_file(&path);
        return Err(format!(
            "Could not download Epic's Windows installer. Check {} or use {}",
            log_path.display(),
            EPIC_DOWNLOAD_PAGE
        ));
    }
    let sha256 = match inspect_msi_and_sha256(&mut file) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(format!(
                "Epic's download was not a usable MSI: {error}. Check {}",
                log_path.display()
            ));
        }
    };
    Ok(DownloadedInstaller {
        path,
        sha256,
        download_url,
    })
}

fn redirect_location(raw: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "Invalid Epic response headers")?;
    let mut status = None;
    let mut location = None;
    for line in text.lines().map(|line| line.trim_end_matches('\r')) {
        if line.starts_with("HTTP/") {
            status = line.split_ascii_whitespace().nth(1).map(str::to_owned);
            location = None;
        } else if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("location")
        {
            if location.is_some() {
                return Err("Epic returned multiple installer destinations".into());
            }
            location = Some(value.trim().to_owned());
        }
    }
    if status.as_deref() != Some("303") {
        return Err("Epic changed its installer redirect response".into());
    }
    location.ok_or_else(|| "Epic did not provide an installer destination".into())
}

pub(super) fn checked_epic_installer_url(url: &str) -> Result<(), String> {
    if url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\' || byte == b'#')
    {
        return Err("Epic installer URL contains an invalid character".into());
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or("Epic installer URL is not HTTPS")?;
    let (host, path) = rest
        .split_once('/')
        .ok_or("Epic installer URL has no path")?;
    if host != EPIC_CDN_HOST {
        return Err("Epic installer redirected to an unreviewed host".into());
    }
    let path = format!("/{path}");
    let filename_and_query = path
        .strip_prefix(EPIC_CDN_PATH)
        .ok_or("Epic installer redirected to an unreviewed path")?;
    let (filename, query) = filename_and_query
        .split_once('?')
        .unwrap_or((filename_and_query, ""));
    let version = filename
        .strip_prefix("EpicInstaller-")
        .and_then(|name| name.strip_suffix(".msi"))
        .ok_or("Epic installer filename changed")?;
    if version.is_empty()
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("Epic installer version is malformed".into());
    }
    if !query.is_empty() && query != format!("launcherfilename={filename}") {
        return Err("Epic installer URL query changed".into());
    }
    Ok(())
}

fn inspect_msi_and_sha256(file: &mut File) -> Result<String, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&length) {
        return Err("unexpected installer size".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut header = [0_u8; 512];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if header[..8] != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
        return Err("missing MSI compound-file signature".into());
    }
    if u16::from_le_bytes([header[0x1c], header[0x1d]]) != 0xfffe {
        return Err("invalid MSI byte order".into());
    }
    let major = u16::from_le_bytes([header[0x1a], header[0x1b]]);
    let sector_shift = u16::from_le_bytes([header[0x1e], header[0x1f]]);
    if !matches!((major, sector_shift), (3, 9) | (4, 12)) {
        return Err("invalid MSI sector format".into());
    }
    if u16::from_le_bytes([header[0x20], header[0x21]]) != 6 {
        return Err("invalid MSI mini-sector format".into());
    }
    let sectors = length.saturating_sub(512) / (1_u64 << sector_shift);
    let fat_sectors = u32::from_le_bytes(header[0x2c..0x30].try_into().unwrap());
    if fat_sectors == 0 || u64::from(fat_sectors) > sectors {
        return Err("MSI has an invalid allocation table".into());
    }
    let directory_sector = u32::from_le_bytes(header[0x30..0x34].try_into().unwrap());
    if u64::from(directory_sector) >= sectors {
        return Err("MSI has an invalid directory sector".into());
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

fn epic_launcher_in_prefix(prefix: &Path) -> Option<PathBuf> {
    // Epic documents both 64-bit and older 32-bit launcher locations.
    const CANDIDATES: [&str; 6] = [
        "Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe",
        "Program Files/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe",
        "Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win32/EpicGamesLauncher.exe",
        "Program Files/Epic Games/Launcher/Portal/Binaries/Win32/EpicGamesLauncher.exe",
        "Program Files (x86)/Epic Games/Launcher/Engine/Binaries/Win64/EpicGamesLauncher.exe",
        "Program Files/Epic Games/Launcher/Engine/Binaries/Win64/EpicGamesLauncher.exe",
    ];
    let real_prefix = prefix.canonicalize().ok()?;
    CANDIDATES.iter().find_map(|candidate| {
        let path = prefix.join("drive_c").join(candidate);
        let before = fs::symlink_metadata(&path).ok()?;
        if !before.file_type().is_file() {
            return None;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&path)
            .ok()?;
        if !same_snapshot(&before, &file.metadata().ok()?) {
            return None;
        }
        let real = path.canonicalize().ok()?;
        if !real.starts_with(&real_prefix) || !supported_launcher_pe(&mut file, before.len()) {
            return None;
        }
        if !same_snapshot(&before, &file.metadata().ok()?)
            || !same_snapshot(&before, &fs::symlink_metadata(&path).ok()?)
        {
            return None;
        }
        Some(real)
    })
}

// A filename and successful setup exit cannot establish that an executable
// was fully written. Use the same bounded x86/x64 PE check as other recipes.
fn supported_launcher_pe(file: &mut File, length: u64) -> bool {
    if length < 70 || file.seek(SeekFrom::Start(0)).is_err() {
        return false;
    }
    let mut dos = [0_u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    let offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    if offset < 64
        || offset > length.saturating_sub(6)
        || file.seek(SeekFrom::Start(offset)).is_err()
    {
        return false;
    }
    let mut pe = [0_u8; 6];
    file.read_exact(&mut pe).is_ok()
        && &pe[..4] == b"PE\0\0"
        && matches!(u16::from_le_bytes([pe[4], pe[5]]), 0x014c | 0x8664)
}

fn wait_for_epic_launcher(prefix: &Path) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = epic_launcher_in_prefix(prefix) {
            return Ok(Some(path));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn install_epic_from_download(
    home: &Path,
    wine: &Path,
    download: DownloadedInstaller,
) -> Result<EpicCatalogInstallResult, String> {
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "Epic Games")?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join("EpicInstaller.msi");
    fs::rename(&download.path, &installer).map_err(|error| {
        format!("Epic bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id, download.path.display())
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_msi_and_sha256)
        .map_err(|error| format!("Epic bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        "epic-games-store",
        download.sha256,
        download.download_url,
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_epic_installer(home, wine, bottle, verified, attempt, false)
}

pub(super) fn retry_catalog(
    home: &Path,
    bottle_id: &str,
) -> Result<EpicCatalogInstallResult, String> {
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    retry_catalog_with_wine(home, bottle_id, &wine)
}

fn retry_catalog_with_wine(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<EpicCatalogInstallResult, String> {
    let (bottle, installer, attempt, _guard) = prepare_retry(
        home,
        bottle_id,
        "epic-games-store",
        wine,
        inspect_msi_and_sha256,
    )?;
    run_epic_installer(home, wine, bottle, installer, attempt, true)
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

fn saved_epic_launcher(home: &Path, bottle_id: &str, path: &Path) -> Result<Launcher, String> {
    let expected = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(existing) = list_launchers(home, bottle_id)?.into_iter().find(|item| {
        Path::new(&item.path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == expected)
    }) {
        return Ok(existing);
    }
    add_launcher(home, bottle_id, "Epic Games", path)
}

fn run_epic_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<EpicCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        // Keep the official Setup Wizard available. It may ask the user to install
        // prerequisites or Epic Online Services; a quiet install is unvalidated.
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
                format!("Epic bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("Epic bottle {}: {error}", bottle.id),
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
                    "Epic bottle {} remains available, but its installer did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
                    bottle.id,
                    result.exit_code,
                    result.signal,
                    result.timed_out,
                    cleanup.as_deref().unwrap_or(""),
                    result.log_path
                ),
            ));
        }
        let prefix = dir.join("prefix");
        if wait_for_epic_launcher(&prefix)
            .map_err(|error| failed_attempt(home, &mut attempt, error))?
            .is_none()
        {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "Epic bottle {} remains available, but the installer did not create a launcher at a documented path. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let prefix = dir.join("prefix");
    let epic = epic_launcher_in_prefix(&prefix).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "Epic bottle {}: previously installed launcher is missing; check the bottle",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_epic_launcher(home, &bottle.id, &epic).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "Epic bottle {}: installer completed, but launcher registration failed: {error}",
                bottle.id
            ),
        )
    })?;
    attempt.set_stage(CatalogStage::Installed);
    save_catalog_attempt(home, &attempt)?;
    cancellation::check()?;
    let launch = launch_with_wine(home, &bottle.id, &epic, wine).map_err(|error| {
        let error = if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Epic bottle {} has a saved launcher, but Epic Games could not start: {error}",
                bottle.id
            )
        };
        followup_issue(home, &mut attempt, error)
    })?;
    Ok(EpicCatalogInstallResult {
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
        root.join(format!("epic-install-{}", Uuid::new_v4()))
    }

    fn fake_msi(path: &Path) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; MIN_INSTALLER_BYTES as usize];
        bytes[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        bytes[0x1a..0x1c].copy_from_slice(&3_u16.to_le_bytes());
        bytes[0x1c..0x1e].copy_from_slice(&0xfffe_u16.to_le_bytes());
        bytes[0x1e..0x20].copy_from_slice(&9_u16.to_le_bytes());
        bytes[0x20..0x22].copy_from_slice(&6_u16.to_le_bytes());
        bytes[0x2c..0x30].copy_from_slice(&1_u32.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&0_u32.to_le_bytes());
        fs::write(path, bytes).unwrap();
        inspect_msi_and_sha256(&mut File::open(path).unwrap()).unwrap()
    }

    fn fake_wine(home: &Path, installer_exit: i32) -> PathBuf {
        let wine = home.join("wine");
        let script = format!(
            "#!/bin/sh\ncase \"$1\" in\n  msiexec)\n    test \"$2\" = /i || exit 91\n    test \"$3\" != /qn || exit 92\n    echo installing >> \"$WINEPREFIX/installer-invocations.txt\"\n    if test {installer_exit} -eq 0; then\n      mkdir -p \"$WINEPREFIX/drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64\"\n      launcher=\"$WINEPREFIX/drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe\"\n      printf 'MZ' > \"$launcher\"\n      dd if=/dev/zero bs=1 count=58 2>/dev/null >> \"$launcher\"\n      printf '\\100\\000\\000\\000PE\\000\\000\\144\\206' >> \"$launcher\"\n    fi\n    exit {installer_exit};;\n  *EpicGamesLauncher.exe) echo launched-Epic; exit 0;;\n  *) exit 93;;\nesac\n"
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
    fn accepts_only_the_reviewed_epic_cdn_redirect() {
        let allowed = "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi?launcherfilename=EpicInstaller-20.1.4.msi";
        assert!(checked_epic_installer_url(allowed).is_ok());
        for wrong in [
            "http://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi",
            "https://epicgames-download1.akamaized.net.evil.test/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi",
            "https://epicgames-download1.akamaized.net@evil.test/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi",
            "https://epicgames-download1.akamaized.net:443/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi",
            "https://epicgames-download1.akamaized.net/other/EpicInstaller-20.1.4.msi",
            "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi?next=https://evil.test",
            "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi#fragment",
        ] {
            assert!(
                checked_epic_installer_url(wrong).is_err(),
                "accepted {wrong}"
            );
        }
    }

    #[test]
    fn parses_only_one_epic_303_destination() {
        let response = b"HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 303\r\nLocation: https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi\r\n\r\n";
        let url = redirect_location(response).unwrap();
        checked_epic_installer_url(&url).unwrap();
        assert!(redirect_location(b"HTTP/2 200\r\n\r\n").is_err());
        assert!(redirect_location(b"HTTP/2 303\r\nLocation: a\r\nLocation: b\r\n\r\n").is_err());
    }

    #[test]
    fn rejects_wrong_or_malformed_msi_before_installation() {
        let home = test_home();
        let path = home.join("EpicInstaller.msi");
        let digest = fake_msi(&path);
        assert_eq!(digest.len(), 64);
        let mut bytes = fs::read(&path).unwrap();
        bytes[..8].fill(0);
        fs::write(&path, &bytes).unwrap();
        assert!(
            inspect_msi_and_sha256(&mut File::open(&path).unwrap())
                .unwrap_err()
                .contains("signature")
        );
        let _ = fake_msi(&path);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0x1e..0x20].copy_from_slice(&13_u16.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(
            inspect_msi_and_sha256(&mut File::open(&path).unwrap())
                .unwrap_err()
                .contains("sector")
        );
        fs::remove_dir_all(home).unwrap();
    }

    fn launcher_pe(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 70];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn installed_launcher_requires_a_supported_complete_pe() {
        let home = test_home();
        let prefix = home.join("prefix");
        let path = prefix.join("drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for machine in [0x014c, 0x8664] {
            fs::write(&path, launcher_pe(machine)).unwrap();
            assert_eq!(epic_launcher_in_prefix(&prefix), Some(path.clone()));
        }
        let mut bad_mz = launcher_pe(0x8664);
        bad_mz[..2].fill(0);
        let mut bad_pe = launcher_pe(0x8664);
        bad_pe[64..68].fill(0);
        let mut early_header = launcher_pe(0x8664);
        early_header[0x3c..0x40].copy_from_slice(&63_u32.to_le_bytes());
        let mut truncated_header = launcher_pe(0x8664);
        truncated_header.pop();
        let mut outside_header = launcher_pe(0x8664);
        outside_header[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        for invalid in [
            Vec::new(),
            b"MZ".to_vec(),
            bad_mz,
            bad_pe,
            early_header,
            truncated_header,
            outside_header,
            launcher_pe(0xaa64),
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(epic_launcher_in_prefix(&prefix).is_none());
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn installed_launcher_rejects_links_and_skips_corrupt_candidates() {
        let home = test_home();
        let prefix = home.join("prefix");
        let first = prefix.join("drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe");
        let fallback = prefix.join(
            "drive_c/Program Files/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe",
        );
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(fallback.parent().unwrap()).unwrap();
        fs::write(&first, b"unfinished").unwrap();
        fs::write(&fallback, launcher_pe(0x8664)).unwrap();
        assert_eq!(epic_launcher_in_prefix(&prefix), Some(fallback.clone()));
        fs::remove_file(&fallback).unwrap();
        symlink(&first, &fallback).unwrap();
        fs::write(&first, launcher_pe(0x8664)).unwrap();
        fs::remove_file(&first).unwrap();
        let external = home.join("external.exe");
        fs::write(&external, launcher_pe(0x8664)).unwrap();
        symlink(&external, &first).unwrap();
        assert!(epic_launcher_in_prefix(&prefix).is_none());
        fs::remove_file(&first).unwrap();
        fs::remove_file(&fallback).unwrap();
        let external_dir = home.join("external");
        fs::create_dir_all(&external_dir).unwrap();
        fs::write(
            external_dir.join("EpicGamesLauncher.exe"),
            launcher_pe(0x8664),
        )
        .unwrap();
        fs::remove_dir(first.parent().unwrap()).unwrap();
        symlink(&external_dir, first.parent().unwrap()).unwrap();
        assert!(epic_launcher_in_prefix(&prefix).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_does_not_register_a_corrupt_previously_completed_launcher() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        assert!(install_epic_from_download(
            &home, &wine, DownloadedInstaller {
                path, sha256,
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        ).is_err());
        let bottle = list(&home).unwrap().pop().unwrap();
        let mut attempt = catalog_attempt(&home, &bottle.id).unwrap().unwrap();
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(&home, &attempt).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let launcher = dir.join("prefix/drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        fs::write(&launcher, b"unfinished").unwrap();
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        let error = retry_catalog_with_wine(&home, &bottle.id, &wine)
            .err()
            .unwrap();
        assert!(
            error.contains("previously installed launcher is missing"),
            "{error}"
        );
        assert!(list_launchers(&home, &bottle.id).unwrap().is_empty());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        assert_eq!(
            catalog_attempt(&home, &bottle.id).unwrap().unwrap().stage,
            CatalogStage::Failed
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn installer_creates_bottle_launcher_and_starts_epic() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        let result = install_epic_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: sha256.clone(),
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        )
        .unwrap();
        assert_eq!(result.bottle.name, "Epic Games");
        assert_eq!(result.launcher.name, "Epic Games");
        assert_eq!(result.installer_sha256, sha256);
        assert_eq!(list_launchers(&home, &result.bottle.id).unwrap().len(), 1);
        assert!(Path::new(&result.install_log_path).is_file());
        assert!(Path::new(&result.launch.log_path).is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_install_retains_bottle_installer_and_logs() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        let error = install_epic_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256,
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        )
        .err()
        .unwrap();
        let bottles = list(&home).unwrap();
        assert_eq!(bottles.len(), 1);
        assert!(error.contains(&bottles[0].id));
        assert!(error.contains("exit Some(37)"));
        assert!(path_is_real_file(
            &bottle_dir(&home, &bottles[0].id)
                .unwrap()
                .join("installers/EpicInstaller.msi")
        ));
        assert!(!list_logs(&home, &bottles[0].id).unwrap().is_empty());
        assert!(list_launchers(&home, &bottles[0].id).unwrap().is_empty());
        let attempt = catalog_attempt(&home, &bottles[0].id).unwrap().unwrap();
        assert_eq!(attempt.catalog_id, "epic-games-store");
        assert_eq!(attempt.stage, CatalogStage::Failed);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_uses_verified_installer_and_same_failed_epic_bottle() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        let _ = install_epic_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256,
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        );
        let first = list(&home).unwrap().pop().unwrap();
        let first_logs = list_logs(&home, &first.id).unwrap().len();
        fake_wine(&home, 0);
        let retried = retry_catalog_with_wine(&home, &first.id, &wine).unwrap();
        assert_eq!(retried.bottle.id, first.id);
        assert_eq!(list(&home).unwrap().len(), 1);
        assert_eq!(list_launchers(&home, &first.id).unwrap().len(), 1);
        assert!(list_logs(&home, &first.id).unwrap().len() > first_logs);
        let attempt = catalog_attempt(&home, &first.id).unwrap().unwrap();
        assert_eq!(attempt.stage, CatalogStage::Installed);
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
    fn retry_rejects_wrong_recipe_or_tampered_epic_installer() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        let _ = install_epic_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256,
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        );
        let bottle = list(&home).unwrap().pop().unwrap();
        assert!(
            prepare_retry(&home, &bottle.id, "steam", &wine, inspect_msi_and_sha256)
                .err()
                .unwrap()
                .contains("does not match")
        );
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let installer = dir.join("installers/EpicInstaller.msi");
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
        fs::remove_file(&installer).unwrap();
        symlink(home.join("wine"), &installer).unwrap();
        assert!(retry_catalog_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn epic_install_stops_if_wineboot_changes_msi_before_spawn() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        fs::write(
            home.join("wineboot"),
            b"#!/bin/sh\nprintf changed >> \"$WINEPREFIX/../installers/EpicInstaller.msi\"\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        let path = home.join("EpicInstaller.msi");
        let sha256 = fake_msi(&path);
        let error = install_epic_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256,
                download_url: "https://epicgames-download1.akamaized.net/Builds/UnrealEngineLauncher/Installers/Windows/EpicInstaller-20.1.4.msi".into(),
            },
        )
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
}
