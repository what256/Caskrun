//! Independent, experimental recipe for Blizzard's Windows Battle.net app.
//! The bootstrapper is publisher-sourced. Sign-in, app updates, game downloads,
//! and gameplay still need separate, real compatibility testing.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    inspect_pe_and_sha256, lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;

pub(super) const CATALOG_ID: &str = "battle-net";
// The Windows button on https://download.battle.net/en-us/desktop currently
// points here. The extra slash is in Blizzard's published link.
const INSTALLER_ENDPOINT: &str = "https://downloader.battle.net//download/getInstallerForGame?os=win&gameProgram=BATTLENET_APP&version=Live";
const DOWNLOAD_PAGE: &str = "https://download.battle.net/en-us/desktop";
const INSTALLER_PREFIX: &str = "https://downloader.battle.net/download/installer/win/";
const MAX_HEADER_BYTES: u64 = 64 * 1024;
const MIN_INSTALLER_BYTES: u64 = 100 * 1024;
const MAX_INSTALLER_BYTES: u64 = 50 * 1024 * 1024;
const REDIRECT_TIMEOUT: Duration = Duration::from_secs(40);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(2 * 60);

#[derive(Serialize)]
pub(super) struct BattleNetCatalogInstallResult {
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

pub(super) fn checked_battle_net_installer_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix(INSTALLER_PREFIX)
        .ok_or("Battle.net installer redirected to an unreviewed host or path")?;
    if rest
        .bytes()
        .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b'?' | b'#' | b'%' | b'@'))
    {
        return Err("Battle.net installer URL contains an invalid character".into());
    }
    let (version, filename) = rest
        .split_once('/')
        .ok_or("Battle.net installer version path changed")?;
    if filename != "Battle.net-Setup.exe" || version.len() > 32 {
        return Err("Battle.net installer filename changed".into());
    }
    let segments: Vec<_> = version.split('.').collect();
    if !(3..=4).contains(&segments.len())
        || segments
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("Battle.net installer version changed format".into());
    }
    Ok(())
}

fn parse_redirect(raw: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "Invalid Battle.net response headers")?;
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
                return Err("Battle.net returned multiple installer destinations".into());
            }
            location = Some(value.trim().to_owned());
        }
    }
    if status.as_deref() != Some("302") {
        return Err("Battle.net changed its installer redirect response".into());
    }
    let url = location.ok_or("Battle.net did not provide an installer destination")?;
    checked_battle_net_installer_url(&url)?;
    Ok(url)
}

pub(super) fn install_catalog(
    home: &Path,
    id: &str,
) -> Result<BattleNetCatalogInstallResult, String> {
    if id != CATALOG_ID {
        return Err(format!("No Battle.net installer is available for {id}"));
    }
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    let installer = download_installer(home)?;
    install_from_download(home, &wine, installer)
}

fn download_installer(home: &Path) -> Result<DownloadedInstaller, String> {
    cancellation::check()?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let id = Uuid::new_v4();
    let headers_path = downloads.join(format!("BattleNetSetup-{id}.headers.tmp"));
    let path = downloads.join(format!("BattleNetSetup-{id}.exe"));
    let log_path = downloads.join(format!("BattleNetSetup-{id}.download.log"));
    let mut headers = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&headers_path)
        .map_err(|error| format!("Could not prepare Battle.net redirect response: {error}"))?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare Battle.net download log: {error}"))?;

    let destination = (|| -> Result<String, String> {
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
                "--max-filesize",
                "65536",
                INSTALLER_ENDPOINT,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(headers.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .spawn()
            .map_err(|error| format!("Could not contact Blizzard's download service: {error}"))?;
        let status = wait_child_bounded(&mut child, REDIRECT_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("Blizzard did not return a usable installer redirect".into());
        }
        let length = headers.metadata().map_err(|e| e.to_string())?.len();
        if length == 0 || length > MAX_HEADER_BYTES {
            return Err("Battle.net redirect headers had an unexpected size".into());
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
            return Err("Battle.net redirect headers exceeded their limit".into());
        }
        parse_redirect(&bytes)
    })();
    // Headers may contain transient service data. The receipt only needs the
    // checked final URL, not the response body or service headers.
    let _ = fs::remove_file(&headers_path);
    let download_url = destination.map_err(|error| {
        if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Could not resolve Blizzard's Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                DOWNLOAD_PAGE
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
        .map_err(|error| format!("Could not prepare Battle.net installer download: {error}"))?;
    let download_result = (|| -> Result<String, String> {
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
                "280",
                "--max-filesize",
                "52428800",
                &download_url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|error| format!("Could not start Battle.net installer download: {error}"))?;
        let status = wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("Blizzard's installer download did not complete".into());
        }
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&length) {
            return Err("Battle.net installer has an unexpected size".into());
        }
        inspect_pe_and_sha256(&mut file)
    })();
    if download_result.is_err() {
        let _ = fs::remove_file(&path);
    }
    let sha256 = download_result.map_err(|error| {
        if cancellation::is_error(&error) {
            error
        } else {
            format!(
                "Could not download Blizzard's Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                DOWNLOAD_PAGE
            )
        }
    })?;
    Ok(DownloadedInstaller {
        path,
        sha256,
        download_url,
    })
}

fn battle_net_launcher_in_prefix(prefix: &Path) -> Option<PathBuf> {
    const CANDIDATES: [&str; 4] = [
        "Program Files (x86)/Battle.net/Battle.net Launcher.exe",
        "Program Files/Battle.net/Battle.net Launcher.exe",
        "Program Files (x86)/Battle.net/Battle.net.exe",
        "Program Files/Battle.net/Battle.net.exe",
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
        if length < 70 {
            return None;
        }
        let mut dos = [0_u8; 64];
        file.read_exact(&mut dos).ok()?;
        let offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().ok()?) as u64;
        if &dos[..2] != b"MZ" || offset < 64 || offset > length.saturating_sub(6) {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut pe = [0_u8; 6];
        file.read_exact(&mut pe).ok()?;
        let machine = u16::from_le_bytes([pe[4], pe[5]]);
        if &pe[..4] != b"PE\0\0" || !matches!(machine, 0x014c | 0x8664) {
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

fn wait_for_launcher(prefix: &Path) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = battle_net_launcher_in_prefix(prefix) {
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
    download: DownloadedInstaller,
) -> Result<BattleNetCatalogInstallResult, String> {
    checked_battle_net_installer_url(&download.download_url)?;
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "Battle.net")?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join("Battle.net-Setup.exe");
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "Battle.net bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_pe_and_sha256)
        .map_err(|error| format!("Battle.net bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        CATALOG_ID,
        download.sha256,
        download.download_url,
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_installer(home, wine, bottle, verified, attempt, false)
}

pub(super) fn retry_catalog(
    home: &Path,
    bottle_id: &str,
) -> Result<BattleNetCatalogInstallResult, String> {
    let wine = wine_path().ok_or(
        "This build has no usable Windows engine. Use a Caskrun build with the included engine.",
    )?;
    cancellation::check()?;
    retry_with_wine(home, bottle_id, &wine)
}

fn retry_with_wine(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<BattleNetCatalogInstallResult, String> {
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
    add_launcher(home, bottle_id, "Battle.net", path)
}

fn run_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<BattleNetCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        // Show Blizzard's installer as published. No quiet flags or automatic
        // acceptance of publisher prompts are applied here.
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
                format!("Battle.net bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("Battle.net bottle {}: {error}", bottle.id),
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
                    "Battle.net bottle {} remains available, but its installer did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
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
                    "Battle.net bottle {} remains available, but setup did not create a checked Battle.net launcher. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let program = battle_net_launcher_in_prefix(&dir.join("prefix")).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "Battle.net bottle {}: previously installed launcher is missing",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_launcher(home, &bottle.id, &program).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "Battle.net bottle {}: installer completed, but launcher registration failed: {error}",
                bottle.id
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
                "Battle.net bottle {} has a saved launcher, but Battle.net could not start: {error}",
                bottle.id
            )
        };
        followup_issue(
            home,
            &mut attempt,
            error,
        )
    })?;
    Ok(BattleNetCatalogInstallResult {
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
        "https://downloader.battle.net/download/installer/win/1.0.66/Battle.net-Setup.exe";

    fn test_home() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("battle-net-install-{}", Uuid::new_v4()))
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
  *Battle.net-Setup.exe)
    test "$#" -eq 1 || exit 92
    echo installing >> "$WINEPREFIX/installer-invocations.txt"
    if test {exit_code} -eq 0; then
      mkdir -p "$WINEPREFIX/drive_c/Program Files (x86)/Battle.net"
      printf 'MZ' > "$WINEPREFIX/drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe"
      dd if=/dev/zero bs=1 count=58 2>/dev/null >> "$WINEPREFIX/drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe"
      printf '\100\000\000\000PE\000\000\114\001' >> "$WINEPREFIX/drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe"
    fi
    exit {exit_code};;
  *"Battle.net Launcher.exe") echo launched-Battle.net; exit 0;;
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
    fn accepts_only_versioned_blizzard_installer_url() {
        assert!(checked_battle_net_installer_url(REVIEWED_URL).is_ok());
        for wrong in [
            "http://downloader.battle.net/download/installer/win/1.0.66/Battle.net-Setup.exe",
            "https://downloader.battle.net.evil.test/download/installer/win/1.0.66/Battle.net-Setup.exe",
            "https://downloader.battle.net@evil.test/download/installer/win/1.0.66/Battle.net-Setup.exe",
            "https://downloader.battle.net:443/download/installer/win/1.0.66/Battle.net-Setup.exe",
            "https://downloader.battle.net/download/installer/mac/1.0.66/Battle.net-Setup.exe",
            "https://downloader.battle.net/download/installer/win/1.0.66/Other.exe",
            "https://downloader.battle.net/download/installer/win/1.0/Battle.net-Setup.exe",
            "https://downloader.battle.net/download/installer/win/1.0.66/Battle.net-Setup.exe?next=evil",
            "https://downloader.battle.net/download/installer/win/1.0.66/Battle.net-Setup.exe#fragment",
        ] {
            assert!(
                checked_battle_net_installer_url(wrong).is_err(),
                "accepted {wrong}"
            );
        }
    }

    #[test]
    fn parses_only_single_302_to_checked_blizzard_path() {
        let response = format!(
            "HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 302\r\nLocation: {REVIEWED_URL}\r\n\r\n"
        );
        assert_eq!(parse_redirect(response.as_bytes()).unwrap(), REVIEWED_URL);
        assert!(parse_redirect(b"HTTP/2 200\r\n\r\n").is_err());
        assert!(parse_redirect(b"HTTP/2 302\r\nLocation: a\r\nLocation: b\r\n\r\n").is_err());
        let hostile = b"HTTP/2 302\r\nLocation: https://evil.test/installer.exe\r\n\r\n";
        assert!(parse_redirect(hostile).is_err());
    }

    #[test]
    fn rejects_corrupt_or_non_windows_setup() {
        let home = test_home();
        let path = home.join("Battle.net-Setup.exe");
        let digest = fake_pe(&path);
        assert_eq!(digest.len(), 64);
        let mut bytes = fs::read(&path).unwrap();
        bytes[..2].fill(0);
        fs::write(&path, bytes).unwrap();
        assert!(inspect_pe_and_sha256(&mut File::open(&path).unwrap()).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn checked_launcher_must_be_pe_inside_bottle() {
        let home = test_home();
        let prefix = home.join("prefix");
        let path = prefix.join("drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe");
        let _ = fake_pe(&path);
        assert!(battle_net_launcher_in_prefix(&prefix).is_some());
        fs::remove_file(&path).unwrap();
        symlink(home.join("outside.exe"), &path).unwrap();
        assert!(battle_net_launcher_in_prefix(&prefix).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn successful_setup_saves_launcher_and_receipt() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join("Battle.net-Setup.exe");
        let digest = fake_pe(&path);
        let installed = install_from_download(
            &home,
            &wine,
            DownloadedInstaller {
                path,
                sha256: digest.clone(),
                download_url: REVIEWED_URL.into(),
            },
        )
        .unwrap();
        assert_eq!(installed.bottle.name, "Battle.net");
        assert_eq!(installed.launcher.name, "Battle.net");
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
    fn retry_reuses_checked_installer_in_same_bottle() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("Battle.net-Setup.exe");
        let digest = fake_pe(&path);
        let error = install_from_download(
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
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        fake_wine(&home, 0);
        let installed = retry_with_wine(&home, &bottle.id, &wine).unwrap();
        assert_eq!(installed.bottle.id, bottle.id);
        assert_eq!(list(&home).unwrap().len(), 1);
        assert!(list_logs(&home, &bottle.id).unwrap().len() > before_logs);
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
    fn retry_refuses_tampered_or_symlink_installer_before_wine() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join("Battle.net-Setup.exe");
        let digest = fake_pe(&path);
        let _ = install_from_download(
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
            .join("installers/Battle.net-Setup.exe");
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
        fs::remove_dir_all(home).unwrap();
    }
}
