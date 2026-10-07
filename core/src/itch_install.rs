//! Independent, experimental recipe for itch.io's Windows desktop app.
//! Publisher-controlled redirects are checked before fetching the installer.
//! Sign-in, app updates, game downloads, and gameplay remain unverified.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, catalog_attempt,
    cleanup_timed_out_catalog_install, lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

pub(super) const CATALOG_ID: &str = "itch-io-app";
const INSTALLER_ENDPOINT: &str = "https://itch.io/app/download?platform=windows";
const BROTH_URL: &str =
    "https://broth.itch.zone/install-itch/windows-amd64/LATEST/unpacked/default";
const DOWNLOAD_PAGE: &str = "https://itch.io/app";
const MAX_HEADER_BYTES: u64 = 16 * 1024;
const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 128 * 1024 * 1024;
const REDIRECT_TIMEOUT: Duration = Duration::from_secs(35);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(12 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(2 * 60);
// itch-setup is a stable proxy: its own flags precede `--`, and everything
// after it belongs to the current itch app, not to the setup/update helper.
// https://github.com/itchio/itch-setup#launching-the-app
pub(super) const ITCH_LAUNCH_ARGS: [&str; 4] = ["--prefer-launch", "--appname", "itch", "--"];

#[derive(Serialize)]
pub(super) struct ItchCatalogInstallResult {
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
}

pub(super) fn checked_itch_installer_url(url: &str) -> Result<(), String> {
    if url != INSTALLER_ENDPOINT {
        return Err("itch.io installer receipt has an unreviewed download URL".into());
    }
    Ok(())
}

fn checked_broth_url(url: &str) -> Result<(), String> {
    if url != BROTH_URL {
        return Err("itch.io redirected to an unreviewed installer service".into());
    }
    Ok(())
}

fn decimal_digits(value: &str, max_len: usize) -> bool {
    !value.is_empty() && value.len() <= max_len && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

// Broth uses a short-lived, signed R2 URL. Keep this allowlist deliberately
// narrow; an unfamiliar mirror/query needs review rather than silent trust.
fn checked_mirror_url(url: &str) -> Result<(), String> {
    if url.len() > 2048
        || !url.is_ascii()
        || url
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b'#' | b'@'))
    {
        return Err("itch.io mirror URL contains an invalid character".into());
    }
    let rest = url
        .strip_prefix("https://itchio-mirror.")
        .ok_or("itch.io installer moved to an unreviewed mirror")?;
    let (account, rest) = rest
        .split_once(".r2.cloudflarestorage.com/build/")
        .ok_or("itch.io installer moved to an unreviewed mirror path")?;
    if !lower_hex(account, 32) {
        return Err("itch.io mirror account changed format".into());
    }
    let (path, query) = rest
        .split_once('?')
        .ok_or("itch.io mirror URL is missing its signed query")?;
    let (build, file) = path
        .split_once('/')
        .ok_or("itch.io mirror build path changed")?;
    let id = file
        .strip_suffix("-unpacked-default")
        .ok_or("itch.io mirror installer name changed")?;
    if !decimal_digits(build, 15) || !decimal_digits(id, 15) {
        return Err("itch.io mirror build identifiers changed format".into());
    }
    let parts: Vec<&str> = query.split('&').collect();
    if parts.len() != 6 {
        return Err("itch.io mirror signed query changed".into());
    }
    let expected_keys = [
        "X-Amz-Algorithm",
        "X-Amz-Credential",
        "X-Amz-Date",
        "X-Amz-Expires",
        "X-Amz-SignedHeaders",
        "X-Amz-Signature",
    ];
    let mut values = Vec::with_capacity(expected_keys.len());
    for (part, key) in parts.into_iter().zip(expected_keys) {
        let (actual, value) = part
            .split_once('=')
            .ok_or("itch.io mirror signed query is malformed")?;
        if actual != key || value.is_empty() {
            return Err("itch.io mirror signed query changed".into());
        }
        values.push(value);
    }
    if values[0] != "AWS4-HMAC-SHA256" || values[4] != "host" || !lower_hex(values[5], 64) {
        return Err("itch.io mirror signature format changed".into());
    }
    let date = values[2];
    if date.len() != 16
        || &date[8..9] != "T"
        || &date[15..16] != "Z"
        || !date[..8].bytes().all(|b| b.is_ascii_digit())
        || !date[9..15].bytes().all(|b| b.is_ascii_digit())
    {
        return Err("itch.io mirror signature date changed format".into());
    }
    let credential = values[1]
        .split_once("%2F")
        .ok_or("itch.io mirror credential changed format")?;
    if !lower_hex(credential.0, 32)
        || credential.1 != format!("{}%2Fauto%2Fs3%2Faws4_request", &date[..8])
    {
        return Err("itch.io mirror credential changed format".into());
    }
    let expires: u64 = values[3]
        .parse()
        .map_err(|_| "itch.io mirror expiry changed format")?;
    if !(1..=600).contains(&expires) {
        return Err("itch.io mirror expiry is out of bounds".into());
    }
    Ok(())
}

fn parse_redirect(raw: &[u8], expected_status: &str) -> Result<String, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "Invalid itch.io response headers")?;
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
                return Err("itch.io returned multiple installer destinations".into());
            }
            location = Some(value.trim().to_owned());
        }
    }
    if status.as_deref() != Some(expected_status) {
        return Err("itch.io changed its installer redirect response".into());
    }
    location.ok_or_else(|| "itch.io did not provide an installer destination".into())
}

fn resolve_redirect(
    downloads: &Path,
    url: &str,
    expected_status: &str,
    log: &File,
) -> Result<String, String> {
    let headers_path = downloads.join(format!("itch-{}.headers.tmp", Uuid::new_v4()));
    let mut headers = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&headers_path)
        .map_err(|error| format!("Could not prepare itch.io redirect response: {error}"))?;
    let result = (|| -> Result<String, String> {
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
                url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(headers.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .spawn()
            .map_err(|error| format!("Could not contact itch.io's installer service: {error}"))?;
        let status = wait_child_bounded(&mut child, REDIRECT_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("itch.io did not return a usable installer redirect".into());
        }
        let length = headers.metadata().map_err(|e| e.to_string())?.len();
        if length == 0 || length > MAX_HEADER_BYTES {
            return Err("itch.io redirect headers had an unexpected size".into());
        }
        headers
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::with_capacity(length as usize);
        headers
            .by_ref()
            .take(MAX_HEADER_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_HEADER_BYTES {
            return Err("itch.io redirect headers exceeded their limit".into());
        }
        parse_redirect(&bytes, expected_status)
    })();
    // The second response contains a short-lived signed URL. Do not retain it.
    let _ = fs::remove_file(headers_path);
    result
}

pub(super) fn install_catalog(home: &Path, id: &str) -> Result<ItchCatalogInstallResult, String> {
    if id != CATALOG_ID {
        return Err(format!("No itch.io installer is available for {id}"));
    }
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let installer = download_installer(home)?;
    install_from_download(home, &wine, installer)
}

pub(super) fn retry_catalog(
    home: &Path,
    bottle_id: &str,
) -> Result<ItchCatalogInstallResult, String> {
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    retry_with_wine(home, bottle_id, &wine)
}

fn download_installer(home: &Path) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let token = Uuid::new_v4();
    let path = downloads.join(format!("itch-setup-{token}.exe"));
    let log_path = downloads.join(format!("itch-setup-{token}.download.log"));
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare itch.io download log: {error}"))?;
    let result = (|| -> Result<String, String> {
        let broth = resolve_redirect(&downloads, INSTALLER_ENDPOINT, "302", &log)?;
        checked_broth_url(&broth)?;
        cancellation::check()?;
        let mirror = resolve_redirect(&downloads, &broth, "307", &log)?;
        checked_mirror_url(&mirror)?;
        cancellation::check()?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&path)
            .map_err(|error| format!("Could not prepare itch.io installer download: {error}"))?;
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
                "134217728",
                &mirror,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .spawn()
            .map_err(|error| format!("Could not start itch.io installer download: {error}"))?;
        let status = wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("itch.io installer download did not complete".into());
        }
        inspect_pe_and_sha256(&mut file)
    })();
    let sha256 = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&path);
            if cancellation::is_error(&error) {
                return Err(error);
            }
            return Err(format!(
                "Could not download a checked itch.io Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                DOWNLOAD_PAGE
            ));
        }
    };
    Ok(DownloadedInstaller { path, sha256 })
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

fn installed_launcher_in_prefix(prefix: &Path) -> Option<PathBuf> {
    let real_prefix = prefix.canonicalize().ok()?;
    let users = prefix.join("drive_c/users");
    let mut entries = fs::read_dir(users).ok()?;
    // Wine prefixes usually have one user and Public. Refuse unexpectedly large
    // or linked user trees instead of walking arbitrary host directories.
    for _ in 0..32 {
        let entry = match entries.next() {
            Some(Ok(entry)) => entry,
            Some(Err(_)) => return None,
            None => return None,
        };
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let path = entry.path().join("AppData/Local/itch/itch-setup.exe");
        let before = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            _ => continue,
        };
        let Ok(mut file) = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&path)
        else {
            continue;
        };
        if !same_snapshot(&before, &file.metadata().ok()?) {
            return None;
        }
        let real = path.canonicalize().ok()?;
        if !real.starts_with(&real_prefix) || pe_machine(&mut file, before.len()).is_err() {
            continue;
        }
        if !same_snapshot(&before, &file.metadata().ok()?)
            || !same_snapshot(&before, &fs::symlink_metadata(&path).ok()?)
        {
            return None;
        }
        return Some(real);
    }
    None
}

fn wait_for_launcher(prefix: &Path) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = installed_launcher_in_prefix(prefix) {
            return Ok(Some(path));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub(super) fn is_recipe_itch_launcher(home: &Path, bottle_id: &str, launcher: &Launcher) -> bool {
    let Ok(Some(attempt)) = catalog_attempt(home, bottle_id) else {
        return false;
    };
    if attempt.catalog_id != CATALOG_ID || attempt.stage != CatalogStage::Installed {
        return false;
    }
    let Ok(dir) = bottle_dir(home, bottle_id) else {
        return false;
    };
    let Some(expected) = installed_launcher_in_prefix(&dir.join("prefix")) else {
        return false;
    };
    Path::new(&launcher.path)
        .canonicalize()
        .is_ok_and(|actual| actual == expected)
}

fn install_from_download(
    home: &Path,
    wine: &Path,
    download: DownloadedInstaller,
) -> Result<ItchCatalogInstallResult, String> {
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "itch.io app")?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join("itch-setup.exe");
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "itch.io bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_pe_and_sha256)
        .map_err(|error| format!("itch.io bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        CATALOG_ID,
        download.sha256,
        INSTALLER_ENDPOINT.to_owned(),
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_installer(home, wine, bottle, verified, attempt, false)
}

fn retry_with_wine(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<ItchCatalogInstallResult, String> {
    let (bottle, installer, attempt, _guard) =
        prepare_retry(home, bottle_id, CATALOG_ID, wine, inspect_pe_and_sha256)?;
    run_installer(home, wine, bottle, installer, attempt, true)
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

fn saved_launcher(home: &Path, bottle_id: &str, path: &Path) -> Result<Launcher, String> {
    let expected = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(existing) = list_launchers(home, bottle_id)?.into_iter().find(|item| {
        Path::new(&item.path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == expected)
    }) {
        return Ok(existing);
    }
    add_launcher(home, bottle_id, "itch.io app", path)
}

fn run_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<ItchCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
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
                arguments: Vec::new(),
            },
            |resolved| installer.verify_before_spawn(home, &bottle.id, wine, resolved, is_retry),
        )
        .map_err(|error| {
            let error = if cancellation::is_error(&error) {
                error
            } else {
                format!("itch.io bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("itch.io bottle {}: {error}", bottle.id),
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
                    "itch.io bottle {} remains available, but setup did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
                    bottle.id,
                    result.exit_code,
                    result.signal,
                    result.timed_out,
                    cleanup.as_deref().unwrap_or(""),
                    result.log_path
                ),
            ));
        }
        if wait_for_launcher(&dir.join("prefix"))
            .map_err(|error| failed_attempt(home, &mut attempt, error))?
            .is_none()
        {
            return Err(failed_attempt(
                home,
                &mut attempt,
                format!(
                    "itch.io bottle {} remains available, but setup did not create a checked launcher. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let program = installed_launcher_in_prefix(&dir.join("prefix")).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "itch.io bottle {}: previously installed launcher is missing",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_launcher(home, &bottle.id, &program).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "itch.io bottle {}: installer completed, but launcher registration failed: {error}",
                bottle.id
            ),
        )
    })?;
    attempt.set_stage(CatalogStage::Installed);
    save_catalog_attempt(home, &attempt)?;
    cancellation::check()?;
    let args: Vec<String> = ITCH_LAUNCH_ARGS
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    let launch =
        launch_with_wine_args(home, &bottle.id, &program, wine, &args, false).map_err(|error| {
            let error = if cancellation::is_error(&error) {
                error
            } else {
                format!(
                    "itch.io bottle {} has a saved launcher, but it could not start: {error}",
                    bottle.id
                )
            };
            followup_issue(home, &mut attempt, error)
        })?;
    Ok(ItchCatalogInstallResult {
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

    const MIRROR: &str = "https://itchio-mirror.cb031a832f44726753d6267436f3b414.r2.cloudflarestorage.com/build/1937562/7443541-unpacked-default?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=3edfcce40115d057d0b5606758e7e9ee%2F20260923%2Fauto%2Fs3%2Faws4_request&X-Amz-Date=20260923T134913Z&X-Amz-Expires=60&X-Amz-SignedHeaders=host&X-Amz-Signature=045f06dff30d280a668bcbd3a3830787c02ced4ce37dcaecb7ace352b7449eee";

    fn test_home() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("itch-install-{}", Uuid::new_v4()))
    }

    fn fake_pe(path: &Path) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; MIN_INSTALLER_BYTES as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x014c_u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
        inspect_pe_and_sha256(&mut File::open(path).unwrap()).unwrap()
    }

    fn fake_wine(home: &Path, exit_code: i32) -> PathBuf {
        let wine = home.join("wine");
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  *"/installers/itch-setup.exe")
    test "$#" -eq 1 || exit 92
    echo installing >> "$WINEPREFIX/installer-invocations.txt"
    if test {exit_code} -eq 0; then
      mkdir -p "$WINEPREFIX/drive_c/users/caskrun/AppData/Local/itch"
      target="$WINEPREFIX/drive_c/users/caskrun/AppData/Local/itch/itch-setup.exe"
      printf 'MZ' > "$target"
      dd if=/dev/zero bs=1 count=58 2>/dev/null >> "$target"
      printf '\100\000\000\000PE\000\000\114\001' >> "$target"
    fi
    exit {exit_code};;
  *"/AppData/Local/itch/itch-setup.exe")
    test "$#" -ge 5 || exit 94
    test "$2" = --prefer-launch || exit 95
    test "$3" = --appname || exit 96
    test "$4" = itch || exit 97
    test "$5" = -- || exit 98
    shift 5
    echo bootstrap-argument-separator
    index=0
    for argument in "$@"; do
      printf 'app-arg%s=%s\n' "$index" "$argument"
      index=$((index + 1))
    done
    echo launched-itch; exit 0;;
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
    fn restricts_both_redirects_and_signed_mirror_shape() {
        assert!(checked_itch_installer_url(INSTALLER_ENDPOINT).is_ok());
        assert!(checked_itch_installer_url("https://itch.io/app").is_err());
        assert!(checked_broth_url(BROTH_URL).is_ok());
        assert!(checked_broth_url("https://broth.itch.zone/other").is_err());
        assert!(checked_mirror_url(MIRROR).is_ok());
        for wrong in [
            MIRROR.replace("itchio-mirror.", "evil-mirror."),
            MIRROR.replace(".r2.cloudflarestorage.com", ".evil.example"),
            MIRROR.replace("/build/1937562/", "/build/../"),
            MIRROR.replace("-unpacked-default", "-other"),
            MIRROR.replace("X-Amz-Expires=60", "X-Amz-Expires=9999"),
            MIRROR.replace(
                "X-Amz-SignedHeaders=host",
                "X-Amz-SignedHeaders=host;cookie",
            ),
            MIRROR.replace("X-Amz-Signature=", "X-Amz-Signature=zz"),
            format!("{MIRROR}&next=evil"),
            format!("{MIRROR}#fragment"),
            format!("{MIRROR}é"),
        ] {
            assert!(checked_mirror_url(&wrong).is_err(), "accepted {wrong}");
        }
    }

    #[test]
    fn accepts_only_expected_redirect_statuses_and_single_location() {
        let first = format!(
            "HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 302\r\nLocation: {BROTH_URL}\r\n\r\n"
        );
        assert_eq!(parse_redirect(first.as_bytes(), "302").unwrap(), BROTH_URL);
        let second = format!("HTTP/2 307\r\nLocation: {MIRROR}\r\n\r\n");
        assert_eq!(parse_redirect(second.as_bytes(), "307").unwrap(), MIRROR);
        assert!(parse_redirect(second.as_bytes(), "302").is_err());
        assert!(parse_redirect(b"HTTP/2 200\r\n\r\n", "302").is_err());
        assert!(
            parse_redirect(b"HTTP/2 302\r\nLocation: a\r\nLocation: b\r\n\r\n", "302").is_err()
        );
    }

    #[test]
    fn rejects_corrupt_download_and_external_launcher() {
        let home = test_home();
        let path = home.join("download.exe");
        let digest = fake_pe(&path);
        assert_eq!(digest.len(), 64);
        let mut bytes = fs::read(&path).unwrap();
        bytes[..2].fill(0);
        fs::write(&path, bytes).unwrap();
        assert!(inspect_pe_and_sha256(&mut File::open(&path).unwrap()).is_err());
        let prefix = home.join("prefix");
        let launcher = prefix.join("drive_c/users/caskrun/AppData/Local/itch/itch-setup.exe");
        fake_pe(&launcher);
        assert!(installed_launcher_in_prefix(&prefix).is_some());
        fs::remove_file(&launcher).unwrap();
        symlink(home.join("outside.exe"), &launcher).unwrap();
        assert!(installed_launcher_in_prefix(&prefix).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn setup_saves_stable_launcher_and_receipt() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join("download.exe");
        let digest = fake_pe(&path);
        let installed = install_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest.clone(),
            },
        )
        .unwrap();
        assert_eq!(installed.bottle.name, "itch.io app");
        assert_eq!(installed.launcher.name, "itch.io app");
        assert_eq!(installed.installer_sha256, digest);
        assert_eq!(installed.download_url, INSTALLER_ENDPOINT);
        assert_eq!(
            list_launchers(&home, &installed.bottle.id).unwrap().len(),
            1
        );
        assert!(is_recipe_itch_launcher(
            &home,
            &installed.bottle.id,
            &installed.launcher
        ));
        assert_eq!(
            catalog_attempt(&home, &installed.bottle.id)
                .unwrap()
                .unwrap()
                .stage,
            CatalogStage::Installed
        );
        assert!(Path::new(&installed.install_log_path).is_file());
        let initial_output = completed_launch_output(&installed.launch);
        assert!(
            initial_output.contains("bootstrap-argument-separator\n"),
            "{initial_output}"
        );
        assert!(!initial_output.contains("app-arg"), "{initial_output}");

        let saved = vec!["--profile".to_owned(), "two words".to_owned()];
        set_launcher_arguments(&home, &installed.bottle.id, &installed.launcher.id, &saved)
            .unwrap();
        let temporary = vec![
            "--disable-gpu".to_owned(),
            "itch://install?game_id=123&launch".to_owned(),
        ];
        let reopened = run_launcher_with_wine(
            &home,
            &installed.bottle.id,
            &installed.launcher.id,
            &wine,
            &temporary,
        )
        .unwrap();
        let output = completed_launch_output(&reopened);
        assert_eq!(
            output.matches("bootstrap-argument-separator\n").count(),
            1,
            "{output}"
        );
        for (index, argument) in saved.iter().chain(&temporary).enumerate() {
            assert_eq!(
                output
                    .matches(&format!("app-arg{index}={argument}\n"))
                    .count(),
                1,
                "{output}"
            );
        }
        assert!(!output.contains("app-arg4="), "{output}");
        fs::remove_dir_all(home).unwrap();
    }

    fn completed_launch_output(launch: &LaunchResult) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        while Instant::now() < deadline {
            output = fs::read_to_string(&launch.log_path).unwrap();
            if output.contains("launched-itch\n") {
                return output;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("itch bootstrap did not finish with its checked arguments: {output}");
    }

    #[test]
    fn retry_reuses_same_bottle_and_rejects_tampering() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("download.exe");
        let digest = fake_pe(&path);
        let error = install_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest,
            },
        )
        .err()
        .unwrap();
        let bottle = list(&home).unwrap().pop().unwrap();
        assert!(error.contains(&bottle.id));
        assert!(list_launchers(&home, &bottle.id).unwrap().is_empty());
        let installer = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("installers/itch-setup.exe");
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        let mut bytes = fs::read(&installer).unwrap();
        *bytes.last_mut().unwrap() = 1;
        fs::write(&installer, bytes).unwrap();
        assert!(retry_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_file(&installer).unwrap();
        symlink(home.join("wine"), &installer).unwrap();
        assert!(retry_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);
        fs::remove_file(&installer).unwrap();
        let restored = home.join("restored.exe");
        fake_pe(&restored);
        fs::rename(restored, &installer).unwrap();
        fake_wine(&home, 0);
        let installed = retry_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(installed.bottle.id, bottle.id);
        assert_eq!(list(&home).unwrap().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    #[ignore = "downloads the current itch.io Windows installer from publisher-controlled hosts"]
    fn real_download_smoke() {
        let build = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.build");
        fs::create_dir_all(&build).unwrap();
        let home = build.join(format!("itch-download-smoke-{}", Uuid::new_v4()));
        let result = download_installer(&home).and_then(|download| {
            let mut file = File::open(&download.path).map_err(|error| error.to_string())?;
            let verified = inspect_pe_and_sha256(&mut file)?;
            if verified != download.sha256 {
                return Err("itch.io smoke download changed during verification".into());
            }
            println!(
                "itch.io Windows PE download: {} bytes, sha256 {}",
                file.metadata().map_err(|e| e.to_string())?.len(),
                verified
            );
            Ok(())
        });
        let cleanup = fs::remove_dir_all(&home);
        assert!(
            cleanup.is_ok(),
            "could not remove transient download: {cleanup:?}"
        );
        assert!(result.is_ok(), "itch.io download smoke failed: {result:?}");
    }
}
