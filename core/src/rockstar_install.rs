//! Original, experimental recipe for Rockstar's Windows launcher.
//! Caskrun opens Rockstar's setup and saves the resulting launcher. Account
//! sign-in, updates, game installation, and gameplay are not yet validated.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

pub(super) const CATALOG_ID: &str = "rockstar-games-launcher";
// Rockstar's own download host and public installer path. Any change needs
// review before downloads or saved receipts can authorize execution.
const INSTALLER_URL: &str =
    "https://gamedownloads.rockstargames.com/public/installer/Rockstar-Games-Launcher.exe";
const DOWNLOAD_PAGE: &str = "https://support.rockstargames.com/articles/4extB4aITvMKdDEZzsFAwE/rockstar-games-launcher-download";
const INSTALLER_NAME: &str = "Rockstar-Games-Launcher.exe";
const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 250 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const LAUNCHER_APPEAR_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const LAUNCHER_CANDIDATES: [&str; 2] = [
    "Program Files/Rockstar Games/Launcher/Launcher.exe",
    "Program Files (x86)/Rockstar Games/Launcher/Launcher.exe",
];

#[derive(Serialize)]
pub(super) struct RockstarCatalogInstallResult {
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

pub(super) fn checked_rockstar_installer_url(url: &str) -> Result<(), String> {
    if url != INSTALLER_URL {
        return Err(
            "Rockstar installer URL changed or points outside its reviewed host and path".into(),
        );
    }
    Ok(())
}

pub(super) fn install_catalog(
    home: &Path,
    id: &str,
) -> Result<RockstarCatalogInstallResult, String> {
    if id != CATALOG_ID {
        return Err(format!(
            "No Rockstar Games Launcher installer is available for {id}"
        ));
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
    checked_rockstar_installer_url(INSTALLER_URL)?;
    storage_root(home, true)?;
    let downloads = real_subdirectory(home, "downloads")?;
    let id = Uuid::new_v4();
    let path = downloads.join(format!("RockstarLauncher-{id}.exe"));
    let log_path = downloads.join(format!("RockstarLauncher-{id}.download.log"));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&path)
        .map_err(|error| format!("Could not prepare Rockstar installer download: {error}"))?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(&log_path)
        .map_err(|error| format!("Could not prepare Rockstar download log: {error}"))?;

    // -q ignores ~/.curlrc; refusing redirects keeps the GET on the exact
    // publisher URL that the receipt accepts.
    let downloaded = (|| -> Result<String, String> {
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
                "20",
                "--max-time",
                "840",
                "--max-filesize",
                "262144000",
                INSTALLER_URL,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|error| format!("Could not start Rockstar installer download: {error}"))?;
        let status = wait_child_bounded(&mut child, DOWNLOAD_TIMEOUT)?;
        if !status.is_some_and(|status| status.success()) {
            return Err("Rockstar installer download did not complete".into());
        }
        inspect_rockstar_installer(&mut file)
    })();
    let sha256 = match downloaded {
        Ok(digest) => digest,
        Err(error) => {
            let _ = fs::remove_file(&path);
            if cancellation::is_error(&error) {
                return Err(error);
            }
            return Err(format!(
                "Could not download a checked Rockstar Windows installer: {error}. Check {} or use {}",
                log_path.display(),
                DOWNLOAD_PAGE
            ));
        }
    };
    Ok(DownloadedInstaller { path, sha256 })
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

fn inspect_rockstar_installer(file: &mut File) -> Result<String, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&length) {
        return Err("unexpected Rockstar installer size".into());
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

fn launcher_in_prefix(prefix: &Path) -> Option<PathBuf> {
    let real_prefix = prefix.canonicalize().ok()?;
    LAUNCHER_CANDIDATES.iter().find_map(|candidate| {
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
        if !real.starts_with(&real_prefix) || pe_machine(&mut file, before.len()).is_err() {
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
        if let Some(path) = launcher_in_prefix(prefix) {
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
) -> Result<RockstarCatalogInstallResult, String> {
    checked_rockstar_installer_url(INSTALLER_URL)?;
    if let Err(error) = cancellation::check() {
        let _ = fs::remove_file(&download.path);
        return Err(error);
    }
    let bottle = create(home, "Rockstar Games Launcher")?;
    let dir = bottle_dir(home, &bottle.id)?;
    let installers = real_subdirectory(&dir, "installers")?;
    let installer = installers.join(INSTALLER_NAME);
    fs::rename(&download.path, &installer).map_err(|error| {
        format!(
            "Rockstar bottle {} was created, but its installer could not be saved: {error}. Download: {}",
            bottle.id,
            download.path.display()
        )
    })?;
    let verified = VerifiedInstaller::open(installer, &download.sha256, inspect_rockstar_installer)
        .map_err(|error| format!("Rockstar bottle {}: {error}", bottle.id))?;
    let attempt = CatalogAttempt::new(
        &bottle.id,
        CATALOG_ID,
        download.sha256,
        INSTALLER_URL.to_owned(),
    )?;
    save_catalog_attempt(home, &attempt)?;
    let _guard = lock_catalog_attempt(&dir)?;
    run_installer(home, wine, bottle, verified, attempt, false)
}

pub(super) fn retry_catalog(
    home: &Path,
    bottle_id: &str,
) -> Result<RockstarCatalogInstallResult, String> {
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
) -> Result<RockstarCatalogInstallResult, String> {
    let (bottle, installer, attempt, _guard) = prepare_retry(
        home,
        bottle_id,
        CATALOG_ID,
        wine,
        inspect_rockstar_installer,
    )?;
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
    add_launcher(home, bottle_id, "Rockstar Games Launcher", path)
}

fn run_installer(
    home: &Path,
    wine: &Path,
    bottle: Bottle,
    installer: VerifiedInstaller,
    mut attempt: CatalogAttempt,
    is_retry: bool,
) -> Result<RockstarCatalogInstallResult, String> {
    let dir = bottle_dir(home, &bottle.id)?;
    cancellation::check()?;
    if attempt.stage != CatalogStage::InstallerComplete {
        attempt.set_stage(CatalogStage::Running);
        save_catalog_attempt(home, &attempt)?;
        cancellation::check().map_err(|error| failed_attempt(home, &mut attempt, error))?;
        // Show Rockstar's installer without accepting its prompts or terms.
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
                format!("Rockstar bottle {}: {error}", bottle.id)
            };
            failed_attempt(home, &mut attempt, error)
        })?;
        attempt.set_log_path(&result.log_path).map_err(|error| {
            failed_attempt(
                home,
                &mut attempt,
                format!("Rockstar bottle {}: {error}", bottle.id),
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
                    "Rockstar bottle {} remains available, but setup did not finish successfully (exit {:?}, signal {:?}, timeout {}). {} Check {}",
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
                    "Rockstar bottle {} remains available, but setup did not create a checked Launcher.exe. Check {}",
                    bottle.id, result.log_path
                ),
            ));
        }
        attempt.set_stage(CatalogStage::InstallerComplete);
        save_catalog_attempt(home, &attempt)?;
    }
    cancellation::check()?;
    let program = launcher_in_prefix(&dir.join("prefix")).ok_or_else(|| {
        failed_attempt(
            home,
            &mut attempt,
            format!(
                "Rockstar bottle {}: previously installed launcher is missing",
                bottle.id
            ),
        )
    })?;
    let launcher = saved_launcher(home, &bottle.id, &program).map_err(|error| {
        followup_issue(
            home,
            &mut attempt,
            format!(
                "Rockstar bottle {}: installer completed, but launcher registration failed: {error}",
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
                "Rockstar bottle {} has a saved launcher, but it could not start: {error}",
                bottle.id
            )
        };
        followup_issue(home, &mut attempt, error)
    })?;
    Ok(RockstarCatalogInstallResult {
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
        root.join(format!("rockstar-install-{}", Uuid::new_v4()))
    }

    fn fake_pe(path: &Path) -> String {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; MIN_INSTALLER_BYTES as usize];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x014c_u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
        inspect_rockstar_installer(&mut File::open(path).unwrap()).unwrap()
    }

    fn fake_wine(home: &Path, exit_code: i32) -> PathBuf {
        let wine = home.join("wine");
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  *Rockstar-Games-Launcher.exe)
    test "$#" -eq 1 || exit 92
    echo installing >> "$WINEPREFIX/installer-invocations.txt"
    if test {exit_code} -eq 0; then
      mkdir -p "$WINEPREFIX/drive_c/Program Files/Rockstar Games/Launcher"
      printf 'MZ' > "$WINEPREFIX/drive_c/Program Files/Rockstar Games/Launcher/Launcher.exe"
      dd if=/dev/zero bs=1 count=58 2>/dev/null >> "$WINEPREFIX/drive_c/Program Files/Rockstar Games/Launcher/Launcher.exe"
      printf '\100\000\000\000PE\000\000\114\001' >> "$WINEPREFIX/drive_c/Program Files/Rockstar Games/Launcher/Launcher.exe"
    fi
    exit {exit_code};;
  *Launcher.exe) echo launched-Rockstar; exit 0;;
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
    fn only_exact_rockstar_download_url_is_accepted() {
        assert!(checked_rockstar_installer_url(INSTALLER_URL).is_ok());
        for wrong in [
            "http://gamedownloads.rockstargames.com/public/installer/Rockstar-Games-Launcher.exe",
            "https://gamedownloads.rockstargames.com.evil.test/public/installer/Rockstar-Games-Launcher.exe",
            "https://gamedownloads.rockstargames.com@evil.test/public/installer/Rockstar-Games-Launcher.exe",
            "https://gamedownloads.rockstargames.com:443/public/installer/Rockstar-Games-Launcher.exe",
            "https://gamedownloads.rockstargames.com/public/installer/Other.exe",
            "https://gamedownloads.rockstargames.com/public/installer/Rockstar-Games-Launcher.exe?next=evil",
        ] {
            assert!(
                checked_rockstar_installer_url(wrong).is_err(),
                "accepted {wrong}"
            );
        }
    }

    #[test]
    fn installer_and_launcher_must_be_checked_windows_files() {
        let home = test_home();
        let installer = home.join(INSTALLER_NAME);
        let digest = fake_pe(&installer);
        assert_eq!(digest.len(), 64);
        let mut bytes = fs::read(&installer).unwrap();
        bytes[..2].fill(0);
        fs::write(&installer, bytes).unwrap();
        assert!(inspect_rockstar_installer(&mut File::open(&installer).unwrap()).is_err());

        let prefix = home.join("prefix");
        let launcher = prefix.join("drive_c").join(LAUNCHER_CANDIDATES[0]);
        let _ = fake_pe(&launcher);
        assert!(launcher_in_prefix(&prefix).is_some());
        fs::remove_file(&launcher).unwrap();
        symlink(&installer, &launcher).unwrap();
        assert!(launcher_in_prefix(&prefix).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn successful_setup_saves_launcher_and_receipt() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 0);
        let path = home.join(INSTALLER_NAME);
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
        assert_eq!(installed.bottle.name, "Rockstar Games Launcher");
        assert_eq!(installed.launcher.name, "Rockstar Games Launcher");
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
    fn retry_rechecks_retained_installer_and_uses_same_bottle() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        let wine = fake_wine(&home, 37);
        let path = home.join(INSTALLER_NAME);
        let digest = fake_pe(&path);
        let _ = install_from_download(
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
        let installer = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("installers/Rockstar-Games-Launcher.exe");
        let before_logs = list_logs(&home, &bottle.id).unwrap().len();
        let mut bytes = fs::read(&installer).unwrap();
        *bytes.last_mut().unwrap() = 1;
        fs::write(&installer, bytes).unwrap();
        assert!(retry_with_wine(&home, &bottle.id, &wine).is_err());
        assert_eq!(list_logs(&home, &bottle.id).unwrap().len(), before_logs);

        // Restore the exact reviewed bytes, then retry without redownloading.
        let _ = fake_pe(&installer);
        fake_wine(&home, 0);
        let installed = retry_with_wine(&home, &bottle.id, &wine).unwrap();
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
