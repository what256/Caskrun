//! Independent, experimental GOG GALAXY Windows installer recipe.
//! GOG publishes the current full Windows installer in its remote configuration.
//! This flow verifies that publisher source and the downloaded file; it does not
//! claim that GALAXY sign-in, updates, or any game works under this Wine build.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

pub(super) const CATALOG_ID: &str = "gog-galaxy";
const MANIFEST_URL: &str =
    "https://remote-config.gog.com/components/webinstaller?component_version=2.0.0";
const DOWNLOAD_PAGE: &str = "https://www.gog.com/galaxy";
const CONTENT_SYSTEM_PREFIX: &str =
    "https://content-system.gog.com/open_link/download?path=/open/galaxy/client/";
// Both destinations are reviewed exact HTTPS hosts and installer paths. GOG's
// content-system endpoint redirected to gcdn.co on 2026-10-07. Retain the
// separately reviewed Fastly destination as well.
const CDN_PREFIXES: [&str; 2] = [
    "https://gog-cdn-fastly.gog.com/open/galaxy/client/",
    "https://gog-cdn.gcdn.co/open/galaxy/client/",
];
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const MAX_HEADER_BYTES: u64 = 64 * 1024;
const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(40);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(12 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct Manifest {
    version: String,
    content: ManifestContent,
}

#[derive(Deserialize)]
struct ManifestContent {
    windows: WindowsInstaller,
}

#[derive(Deserialize)]
struct WindowsInstaller {
    size: u64,
    version: String,
    deprecated: bool,
    #[serde(rename = "downloadLink")]
    download_link: String,
    #[serde(rename = "installerMd5")]
    installer_md5: String,
}

#[derive(Serialize)]
pub(super) struct GogCatalogInstallResult {
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

fn checked_version(version: &str) -> Result<(), String> {
    let numbers: Vec<_> = version.split('.').collect();
    if version.len() > 32
        || numbers.len() != 4
        || numbers
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("GOG installer version changed format".into());
    }
    Ok(())
}

fn checked_installer_name(path: &str) -> Result<&str, String> {
    if path
        .bytes()
        .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b'#' | b'?' | b'%' | b'@'))
    {
        return Err("GOG installer URL contains an invalid character".into());
    }
    let (directory, filename) = path.rsplit_once('/').unwrap_or(("", path));
    let version = filename
        .strip_prefix("setup_galaxy_")
        .and_then(|value| value.strip_suffix(".exe"))
        .ok_or("GOG installer filename changed")?;
    checked_version(version)?;
    if !directory.is_empty() && directory != version {
        return Err("GOG installer directory does not match its version".into());
    }
    Ok(version)
}

fn checked_content_url(url: &str) -> Result<&str, String> {
    let path = url
        .strip_prefix(CONTENT_SYSTEM_PREFIX)
        .ok_or("GOG manifest points outside its reviewed download service")?;
    checked_installer_name(path)
}

fn checked_cdn_version(url: &str) -> Result<&str, String> {
    let path = CDN_PREFIXES
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))
        .ok_or("GOG installer redirected to an unreviewed host or path")?;
    checked_installer_name(path)
}

pub(super) fn checked_gog_installer_url(url: &str) -> Result<(), String> {
    checked_cdn_version(url)?;
    Ok(())
}

fn parse_manifest(bytes: &[u8]) -> Result<WindowsInstaller, String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("GOG installer manifest has an unexpected size".into());
    }
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| format!("Could not read GOG installer manifest: {error}"))?;
    if manifest.version != "2.0.0" {
        return Err("GOG installer manifest version changed".into());
    }
    let installer = manifest.content.windows;
    checked_version(&installer.version)?;
    if installer.deprecated {
        return Err("GOG marked this Windows installer as deprecated".into());
    }
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&installer.size) {
        return Err("GOG installer size is outside the reviewed bounds".into());
    }
    let url_version = checked_content_url(&installer.download_link)?;
    if url_version != installer.version {
        return Err("GOG installer version and download URL disagree".into());
    }
    if installer.installer_md5.len() != 32
        || !installer
            .installer_md5
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("GOG installer checksum is malformed".into());
    }
    Ok(installer)
}

fn redirect_location(raw: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "Invalid GOG response headers")?;
    let mut status = None;
    let mut location = None;
    for line in text.lines().map(|line| line.trim_end_matches('\r')) {
        if line.starts_with("HTTP/") {
            status = line.split_ascii_whitespace().nth(1).map(str::to_owned);
            location = None;
        } else if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("location")
        {
            if location.is_some() {
                return Err("GOG returned multiple installer destinations".into());
            }
            location = Some(value.trim().to_owned());
        }
    }
    if status.as_deref() != Some("302") {
        return Err("GOG changed its installer redirect response".into());
    }
    location.ok_or_else(|| "GOG did not provide an installer destination".into())
}

fn run_curl_to_file(
    url: &str,
    destination: &mut File,
    log: &File,
    head: bool,
    seconds: &str,
    max_size: &str,
    timeout: Duration,
) -> Result<(), String> {
    cancellation::check()?;
    let mut command = Command::new("/usr/bin/curl");
    command.args([
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
        seconds,
        "--max-filesize",
        max_size,
    ]);
    if head {
        command.arg("--head");
    }
    let mut child = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            destination.try_clone().map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .spawn()
        .map_err(|error| format!("Could not contact GOG: {error}"))?;
    let status = wait_child_bounded(&mut child, timeout)?;
    if !status.is_some_and(|status| status.success()) {
        return Err("GOG download request failed".into());
    }
    Ok(())
}

fn read_bounded(file: &mut File, limit: u64) -> Result<Vec<u8>, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length == 0 || length > limit {
        return Err("GOG response exceeded its size limit".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("GOG response exceeded its size limit".into());
    }
    Ok(bytes)
}

fn inspect_pe_and_sha256(file: &mut File) -> Result<String, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&length) {
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
        return Err("Windows program is too small".into());
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
    if &pe[..4] != b"PE\0\0" {
        return Err("missing Windows PE signature".into());
    }
    let machine = u16::from_le_bytes([pe[4], pe[5]]);
    if !matches!(machine, 0x014c | 0x8664) {
        return Err("Windows program is not x86 or x64".into());
    }
    Ok(machine)
}

fn matches_manifest_md5(path: &Path, expected: &str) -> Result<bool, String> {
    // GOG's own manifest supplies MD5 for release identity. SHA-256 in the
    // durable receipt independently pins bytes for every later retry.
    let result = Command::new("/sbin/md5")
        .arg("-q")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not verify GOG's published checksum: {error}"))?;
    if !result.status.success() || result.stdout.len() > 128 {
        return Err("Could not verify GOG's published checksum".into());
    }
    let actual = std::str::from_utf8(&result.stdout)
        .map_err(|_| "GOG checksum output was invalid")?
        .trim();
    Ok(actual == expected)
}

pub(super) fn install_catalog(home: &Path, id: &str) -> Result<GogCatalogInstallResult, String> {
    if id != CATALOG_ID {
        return Err(format!("No GOG installer is available for {id}"));
    }
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let download = download_gog_installer(home)?;
    install_gog_from_download(home, &wine, download)
}

fn download_gog_installer(home: &Path) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let id = Uuid::new_v4();
    let manifest_path = downloads.join(format!("GOGGalaxy-{id}.manifest.tmp"));
    let headers_path = downloads.join(format!("GOGGalaxy-{id}.headers.tmp"));
    let path = downloads.join(format!("GOGGalaxy-{id}.exe"));
    let log_path = downloads.join(format!("GOGGalaxy-{id}.download.log"));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare GOG download log: {error}"))?;
    let result = (|| -> Result<DownloadedInstaller, String> {
        let mut manifest_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&manifest_path)
            .map_err(|error| format!("Could not prepare GOG manifest response: {error}"))?;
        run_curl_to_file(
            MANIFEST_URL,
            &mut manifest_file,
            &log,
            false,
            "30",
            "16384",
            FETCH_TIMEOUT,
        )?;
        let installer = parse_manifest(&read_bounded(&mut manifest_file, MAX_MANIFEST_BYTES)?)?;
        cancellation::check()?;

        let mut headers = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&headers_path)
            .map_err(|error| format!("Could not prepare GOG redirect response: {error}"))?;
        run_curl_to_file(
            &installer.download_link,
            &mut headers,
            &log,
            true,
            "30",
            "65536",
            FETCH_TIMEOUT,
        )?;
        let download_url = redirect_location(&read_bounded(&mut headers, MAX_HEADER_BYTES)?)?;
        let destination_version = checked_cdn_version(&download_url)?;
        if destination_version != installer.version {
            return Err("GOG installer redirect changed its version".into());
        }
        cancellation::check()?;

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&path)
            .map_err(|error| format!("Could not prepare GOG installer download: {error}"))?;
        run_curl_to_file(
            &download_url,
            &mut file,
            &log,
            false,
            "680",
            "536870912",
            DOWNLOAD_TIMEOUT,
        )?;
        if file.metadata().map_err(|e| e.to_string())?.len() != installer.size {
            return Err("GOG installer size differs from its published manifest".into());
        }
        let sha256 = inspect_pe_and_sha256(&mut file)?;
        if !matches_manifest_md5(&path, &installer.installer_md5)? {
            return Err("GOG installer checksum differs from its published manifest".into());
        }
        Ok(DownloadedInstaller {
            path: path.clone(),
            sha256,
            download_url,
        })
    })();
    // The manifest and redirect can contain transient service data; retain
    // only the verified installer, its digest receipt, and the download log.
    let _ = fs::remove_file(manifest_path);
    let _ = fs::remove_file(headers_path);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result.map_err(|error| {
        if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Could not download GOG's Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                DOWNLOAD_PAGE
            )
        }
    })
}

fn gog_launcher_in_prefix(prefix: &Path) -> Option<PathBuf> {
    const CANDIDATES: [&str; 2] = [
        "Program Files (x86)/GOG Galaxy/GalaxyClient.exe",
        "Program Files/GOG Galaxy/GalaxyClient.exe",
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
        if !real.starts_with(&real_prefix) {
            return None;
        }
        let length = file.metadata().ok()?.len();
        pe_machine(&mut file, length).ok()?;
        if !same_snapshot(&before, &file.metadata().ok()?)
            || !same_snapshot(&before, &fs::symlink_metadata(&path).ok()?)
        {
            return None;
        }
        Some(real)
    })
}

fn wait_for_gog_launcher(prefix: &Path) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = gog_launcher_in_prefix(prefix) {
            return Ok(Some(path));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn install_gog_from_download(
    home: &Path,
    wine: &Path,
    download: DownloadedInstaller,
) -> Result<GogCatalogInstallResult, String> {
    checked_gog_installer_url(&download.download_url)?;
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "GOG Galaxy")?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join("GOGGalaxySetup.exe");
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "GOG bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_pe_and_sha256)
        .map_err(|error| format!("GOG bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        CATALOG_ID,
        download.sha256,
        download.download_url,
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_gog_installer(home, wine, bottle, verified, attempt, false)
}

pub(super) fn retry_catalog(
    home: &Path,
    bottle_id: &str,
) -> Result<GogCatalogInstallResult, String> {
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
) -> Result<GogCatalogInstallResult, String> {
    let (bottle, installer, attempt, _guard) =
        prepare_retry(home, bottle_id, CATALOG_ID, wine, inspect_pe_and_sha256)?;
    run_gog_installer(home, wine, bottle, installer, attempt, true)
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

fn saved_gog_launcher(home: &Path, bottle_id: &str, path: &Path) -> Result<Launcher, String> {
    let expected = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(existing) = list_launchers(home, bottle_id)?.into_iter().find(|item| {
        Path::new(&item.path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == expected)
    }) {
        return Ok(existing);
    }
    add_launcher(home, bottle_id, "GOG Galaxy", path)
}

fn run_gog_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<GogCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        // The published full installer is Inno Setup, but do not force quiet
        // flags: GOG may require an EULA or install-location interaction.
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
                format!("GOG bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("GOG bottle {}: {error}", bottle.id),
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
                    "GOG bottle {} remains available, but its installer did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
                    bottle.id,
                    result.exit_code,
                    result.signal,
                    result.timed_out,
                    cleanup.as_deref().unwrap_or(""),
                    result.log_path
                ),
            ));
        }
        if wait_for_gog_launcher(&dir.join("prefix"))
            .map_err(|error| failed_attempt(home, &mut attempt, error))?
            .is_none()
        {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "GOG bottle {} remains available, but the installer did not create a Galaxy launcher at the reviewed path. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let gog = gog_launcher_in_prefix(&dir.join("prefix")).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "GOG bottle {}: previously installed Galaxy launcher is missing",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_gog_launcher(home, &bottle.id, &gog).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "GOG bottle {}: installer completed, but launcher registration failed: {error}",
                bottle.id
            ),
        )
    })?;
    attempt.set_stage(CatalogStage::Installed);
    save_catalog_attempt(home, &attempt)?;
    cancellation::check()?;
    let launch = launch_with_wine(home, &bottle.id, &gog, wine).map_err(|error| {
        let error = if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "GOG bottle {} has a saved launcher, but Galaxy could not start: {error}",
                bottle.id
            )
        };
        followup_issue(home, &mut attempt, error)
    })?;
    Ok(GogCatalogInstallResult {
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

    const REVIEWED_URL: &str =
        "https://gog-cdn-fastly.gog.com/open/galaxy/client/setup_galaxy_2.1.10.56.exe";
    const CONTENT_URL: &str = "https://content-system.gog.com/open_link/download?path=/open/galaxy/client/setup_galaxy_2.1.10.56.exe";

    fn test_home() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("gog-install-{}", Uuid::new_v4()))
    }

    fn fake_pe(path: &Path, size: usize) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; size];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x8664_u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
        inspect_pe_and_sha256(&mut File::open(path).unwrap()).unwrap()
    }

    fn fake_wine(home: &Path, exit_code: i32) -> PathBuf {
        let wine = home.join("wine");
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  *GOGGalaxySetup.exe)
    echo installing >> "$WINEPREFIX/installer-invocations.txt"
    if test {exit_code} -eq 0; then
      mkdir -p "$WINEPREFIX/drive_c/Program Files (x86)/GOG Galaxy"
      printf 'MZ' > "$WINEPREFIX/drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe"
      dd if=/dev/zero bs=1 count=58 2>/dev/null >> "$WINEPREFIX/drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe"
      printf '\100\000\000\000PE\000\000\144\206' >> "$WINEPREFIX/drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe"
    fi
    exit {exit_code};;
  *GalaxyClient.exe) echo launched-GOG; exit 0;;
  *) exit 93;;
esac
"#
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
    fn accepts_only_reviewed_manifest_shape_and_publisher_destination() {
        let manifest = format!(
            "{{\"version\":\"2.0.0\",\"content\":{{\"windows\":{{\"size\":342291968,\"version\":\"2.1.10.56\",\"deprecated\":false,\"downloadLink\":\"{CONTENT_URL}\",\"installerMd5\":\"1395712e9c7e6ba31447a5d948e90dbc\"}}}}}}"
        );
        let installer = parse_manifest(manifest.as_bytes()).unwrap();
        assert_eq!(installer.size, 342_291_968);
        assert_eq!(installer.version, "2.1.10.56");
        assert!(checked_gog_installer_url(REVIEWED_URL).is_ok());
        let versioned = "https://gog-cdn-fastly.gog.com/open/galaxy/client/2.1.10.56/setup_galaxy_2.1.10.56.exe";
        assert!(checked_gog_installer_url(versioned).is_ok());
        for wrong in [
            "http://gog-cdn-fastly.gog.com/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com.evil.test/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com@evil.test/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com:443/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com/other/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com/open/galaxy/client/2.1.10.57/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn-fastly.gog.com/open/galaxy/client/setup_galaxy_2.1.10.56.exe?next=evil",
            "https://gog-cdn-fastly.gog.com/open/galaxy/client/setup_galaxy_2.1.10.56.exe#fragment",
        ] {
            assert!(
                checked_gog_installer_url(wrong).is_err(),
                "accepted {wrong}"
            );
        }
        let wrong_version =
            manifest.replace("2.1.10.56\",\"deprecated", "2.1.10.57\",\"deprecated");
        assert!(parse_manifest(wrong_version.as_bytes()).is_err());
        let deprecated = manifest.replace("\"deprecated\":false", "\"deprecated\":true");
        assert!(parse_manifest(deprecated.as_bytes()).is_err());
        let wrong_size = manifest.replace("342291968", "9");
        assert!(parse_manifest(wrong_size.as_bytes()).is_err());
    }

    #[test]
    fn accepts_current_publisher_cdn_without_broadening_host_or_path_rules() {
        let current = "https://gog-cdn.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe";
        let versioned =
            "https://gog-cdn.gcdn.co/open/galaxy/client/2.1.10.56/setup_galaxy_2.1.10.56.exe";
        assert!(checked_gog_installer_url(current).is_ok());
        assert!(checked_gog_installer_url(versioned).is_ok());
        assert_eq!(checked_cdn_version(current).unwrap(), "2.1.10.56");
        assert_eq!(checked_cdn_version(versioned).unwrap(), "2.1.10.56");
        for wrong in [
            "http://gog-cdn.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co.evil.test/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co@evil.test/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            concat!(
                "https://",
                "user@",
                "gog-cdn.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe"
            ),
            "https://gog-cdn.gcdn.co:443/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://other.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co/other/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co/open/galaxy/client/2.1.10.57/setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe?next=evil",
            "https://gog-cdn.gcdn.co/open/galaxy/client/setup_galaxy_2.1.10.56.exe#fragment",
            "https://gog-cdn.gcdn.co/open/galaxy/client/../setup_galaxy_2.1.10.56.exe",
            "https://gog-cdn.gcdn.co/open/galaxy/client/%2e%2e/setup_galaxy_2.1.10.56.exe",
        ] {
            assert!(
                checked_gog_installer_url(wrong).is_err(),
                "accepted {wrong}"
            );
        }
        let headers = format!("HTTP/2 302\r\nLocation: {current}\r\n\r\n");
        assert_eq!(redirect_location(headers.as_bytes()).unwrap(), current);
    }

    #[test]
    fn accepts_only_one_gog_302_destination() {
        let headers = format!(
            "HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 302\r\nLocation: {REVIEWED_URL}\r\n\r\n"
        );
        assert_eq!(redirect_location(headers.as_bytes()).unwrap(), REVIEWED_URL);
        assert!(redirect_location(b"HTTP/2 200\r\n\r\n").is_err());
        assert!(redirect_location(b"HTTP/2 302\r\nLocation: a\r\nLocation: b\r\n\r\n").is_err());
    }

    #[test]
    fn rejects_wrong_or_corrupt_windows_installer() {
        let home = test_home();
        let path = home.join("GOGGalaxySetup.exe");
        let digest = fake_pe(&path, MIN_INSTALLER_BYTES as usize);
        assert_eq!(digest.len(), 64);
        let mut bytes = fs::read(&path).unwrap();
        bytes[..2].fill(0);
        fs::write(&path, bytes).unwrap();
        assert!(inspect_pe_and_sha256(&mut File::open(&path).unwrap()).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn successful_setup_creates_launcher_and_receipt() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join("GOGGalaxySetup.exe");
        let digest = fake_pe(&path, MIN_INSTALLER_BYTES as usize);
        let installed = install_gog_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest.clone(),
                download_url: REVIEWED_URL.into(),
            },
        )
        .unwrap();
        assert_eq!(installed.bottle.name, "GOG Galaxy");
        assert_eq!(installed.launcher.name, "GOG Galaxy");
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

    #[test]
    fn retry_reuses_checked_installer_and_same_bottle() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("GOGGalaxySetup.exe");
        let digest = fake_pe(&path, MIN_INSTALLER_BYTES as usize);
        let error = install_gog_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest,
                download_url: REVIEWED_URL.into(),
            },
        )
        .err()
        .unwrap();
        let bottle = list(&home).unwrap().pop().unwrap();
        assert!(error.contains(&bottle.id));
        assert!(list_launchers(&home, &bottle.id).unwrap().is_empty());
        assert_eq!(
            catalog_attempt(&home, &bottle.id).unwrap().unwrap().stage,
            CatalogStage::Failed
        );
        let first_logs = list_logs(&home, &bottle.id).unwrap().len();
        fake_wine(&home, 0);
        let installed = retry_catalog_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(installed.bottle.id, bottle.id);
        assert_eq!(list(&home).unwrap().len(), 1);
        assert!(list_logs(&home, &bottle.id).unwrap().len() > first_logs);
        let invocations = fs::read_to_string(
            bottle_dir(&home, &bottle.id)
                .unwrap()
                .join("prefix/installer-invocations.txt"),
        )
        .unwrap();
        assert_eq!(invocations.lines().count(), 2);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn retry_refuses_tampered_or_replaced_installer_before_wine() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("GOGGalaxySetup.exe");
        let digest = fake_pe(&path, MIN_INSTALLER_BYTES as usize);
        let _ = install_gog_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest,
                download_url: REVIEWED_URL.into(),
            },
        );
        let bottle = list(&home).unwrap().pop().unwrap();
        let installer = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("installers/GOGGalaxySetup.exe");
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        let mut bytes = fs::read(&installer).unwrap();
        *bytes.last_mut().unwrap() = 1;
        fs::write(&installer, bytes).unwrap();
        assert!(retry_catalog_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_file(&installer).unwrap();
        symlink(home.join("wine"), &installer).unwrap();
        assert!(retry_catalog_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_dir_all(home).unwrap();
    }
}
