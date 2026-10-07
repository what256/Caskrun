//! Independent, experimental Windows installer recipes for EA app and Ubisoft Connect.
//! Only setup and launcher registration are automated. Account sign-in, updates,
//! game downloads, and gameplay have not been validated in this Wine build.

use super::steam_install::{
    CatalogAttempt, CatalogStage, VerifiedInstaller, cleanup_timed_out_catalog_install,
    lock_catalog_attempt, prepare_retry, save_catalog_attempt,
};
use super::*;
use sha2::{Digest, Sha256};

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

fn launcher_in_prefix(prefix: &Path, profile: Profile) -> Option<PathBuf> {
    let real_prefix = prefix.canonicalize().ok()?;
    profile.launcher_candidates.iter().find_map(|candidate| {
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

fn wait_for_launcher(prefix: &Path, profile: Profile) -> Result<Option<PathBuf>, String> {
    let deadline = Instant::now() + LAUNCHER_APPEAR_TIMEOUT;
    loop {
        cancellation::check()?;
        if let Some(path) = launcher_in_prefix(prefix, profile) {
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
