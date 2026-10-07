//! One reviewed transition for Caskrun's existing Steam bottles. Other Wine
//! changes continue through the exact-match check in `verify_runtime_record`.

use super::*;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

// Complete package-tree SHA-256 values produced by runtime_identity.rs. The
// predecessor was verified against .build/tls-wine-runtime-dev; the successor
// was verified against both .build/steam-wine-runtime-dev and the packaged app.
const TLS_WINE_11_IDENTITY: &str =
    "sha256:6df2a9f5dfaa119ba3d6c84214d79f885c70e887ef7f3bc9d2255aeb8b2f88e6";
const STEAM_WINE_11_IDENTITY: &str =
    "sha256:07c5721ad17efa9d4ea89ff61522133407756c4aea8a47866625dcf118e7347c";
const RECORDED_DISPATCH: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch";
const SELECTED_WINE: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/wine";

struct MigrationPolicy<'a> {
    previous: &'a str,
    approved: &'a str,
}

const POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: STEAM_WINE_11_IDENTITY,
};

/// Returned only after the old record has been backed up and replaced. The
/// caller restores it if the ordinary prefix verification then fails.
pub(super) struct RecordRollback {
    record_path: PathBuf,
    backup_path: PathBuf,
    original_bytes: Vec<u8>,
    migrated_bytes: Vec<u8>,
}

impl RecordRollback {
    pub(super) fn restore(self) -> Result<(), String> {
        if read_record_bytes(&self.backup_path)? != self.original_bytes {
            return Err(format!(
                "Steam runtime backup changed; restore the bottle from {}",
                self.backup_path.display()
            ));
        }
        if read_record_bytes(&self.record_path)? != self.migrated_bytes {
            return Err(format!(
                "Steam runtime record changed; review its backup at {}",
                self.backup_path.display()
            ));
        }
        replace_record_bytes(&self.record_path, &self.original_bytes)
    }
}

/// Called while the launcher's per-bottle lock is held, before Wine starts.
/// This hook is intentionally absent from generic initialize, tools, and
/// custom launch paths.
pub(super) fn maybe_migrate_steam_runtime(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
) -> Result<Option<RecordRollback>, String> {
    migrate_with_policy(
        home,
        bottle_id,
        wine,
        launcher_path,
        &POLICY,
        WINE_SERVER_CHECK_TIMEOUT,
    )
}

fn migrate_with_policy(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
    policy: &MigrationPolicy<'_>,
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    let dir = bottle_dir(home, bottle_id)?;
    let prefix = dir.join("prefix");
    let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
    if !path_is_real_file(&prefix.join("system.reg")) {
        return Ok(None);
    }
    let record_path = dir.join("prefix-record.json");
    let original_bytes = read_record_bytes(&record_path)?;
    let record: PrefixRecord = serde_json::from_slice(&original_bytes)
        .map_err(|e| format!("Invalid bottle runtime record: {e}"))?;
    if record.runtime_identity.as_deref() != Some(policy.previous) {
        return Ok(None);
    }
    // A custom Wine path, a legacy path-only record, and every other package
    // mismatch keep the existing strict behavior.
    if !Path::new(&record.wine_path).is_absolute()
        || !Path::new(&record.wine_path).ends_with(RECORDED_DISPATCH)
        || !wine.is_absolute()
        || !wine.ends_with(SELECTED_WINE)
        || !record
            .runtime_fingerprint
            .as_deref()
            .is_some_and(bound_fingerprint_shape)
    {
        return Ok(None);
    }
    let Some(attempt) = steam_install::catalog_attempt(home, bottle_id)? else {
        return Ok(None);
    };
    if attempt.catalog_id != "steam" || attempt.stage != steam_install::CatalogStage::Installed {
        return Ok(None);
    }
    let launchers = list_launchers(home, bottle_id)?;
    if !launchers.iter().any(|launcher| {
        Path::new(&launcher.path)
            .canonicalize()
            .is_ok_and(|path| path == launcher_path)
            && steam_install::is_recipe_steam_launcher(home, bottle_id, launcher)
    }) {
        return Ok(None);
    }
    if !is_usable_wine(wine) {
        return Err("The approved Caskrun Steam engine is unavailable".into());
    }
    // Always hash all new package bytes. A saved bottle identity or same-path
    // metadata fingerprint cannot authorize a change of engine content.
    let attestation = runtime_identity::bundled_runtime_attestation(wine)?
        .ok_or("The selected Windows engine is not a complete Caskrun package")?;
    if attestation.identity != policy.approved {
        return Err("The selected Windows engine is not the approved Steam package; this bottle was unchanged".into());
    }
    refuse_running_bottle(&dir)?;
    wait_for_idle_wineserver(&dir, wine, idle_timeout)?;
    refuse_running_bottle(&dir)?;
    if !same_identity(
        &prefix_snapshot,
        &prefix_guard.metadata().map_err(|e| e.to_string())?,
    ) || !same_identity(
        &prefix_snapshot,
        &fs::symlink_metadata(&prefix).map_err(|e| e.to_string())?,
    ) {
        return Err("Steam bottle prefix changed during engine migration".into());
    }
    if read_record_bytes(&record_path)? != original_bytes {
        return Err("Steam bottle runtime record changed during migration".into());
    }

    let backups = real_subdirectory(home, "runtime-migration-backups")?;
    let backup_path = backups.join(format!("{bottle_id}-tls-to-steam-prefix-record.json"));
    retain_original_record(&backup_path, &original_bytes)?;
    let migrated = PrefixRecord {
        wine_path: wine
            .canonicalize()
            .map_err(|e| e.to_string())?
            .display()
            .to_string(),
        runtime_identity: Some(attestation.identity.clone()),
        runtime_fingerprint: Some(runtime_identity::bind_record_fingerprint(
            &attestation.identity,
            &attestation.metadata_fingerprint,
        )),
    };
    let migrated_bytes = serde_json::to_vec_pretty(&migrated).map_err(|e| e.to_string())?;
    replace_record_bytes(&record_path, &migrated_bytes)?;
    Ok(Some(RecordRollback {
        record_path,
        backup_path,
        original_bytes,
        migrated_bytes,
    }))
}

fn bound_fingerprint_shape(value: &str) -> bool {
    value
        .strip_prefix("sha256-bound-v2:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn read_record_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let before = fs::symlink_metadata(path)
        .map_err(|e| format!("Could not inspect bottle runtime record: {e}"))?;
    if !before.file_type().is_file() || before.len() > MAX_PREFIX_RECORD_BYTES {
        return Err("Bottle runtime record must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| format!("Could not safely open bottle runtime record: {e}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle runtime record changed while opening it".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_PREFIX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PREFIX_RECORD_BYTES
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Bottle runtime record changed while reading it".into());
    }
    Ok(bytes)
}

fn retain_original_record(path: &Path, original: &[u8]) -> Result<(), String> {
    retain_original_record_with_writer(path, original, |file, bytes| {
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    })
}

fn retain_original_record_with_writer(
    path: &Path,
    original: &[u8],
    write_backup: impl FnOnce(&mut File, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            if read_record_bytes(path)? != original {
                return Err(format!(
                    "Existing Steam runtime backup differs from this bottle's record: {}",
                    path.display()
                ));
            }
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Could not inspect Steam runtime backup: {e}")),
    }
    let parent = path
        .parent()
        .ok_or("Steam runtime backup has no parent directory")?;
    let (parent_guard, parent_snapshot) = open_stable_directory(parent)?;
    let backup_name = path
        .file_name()
        .ok_or("Steam runtime backup has no file name")?;
    let temporary_name = format!(".steam-runtime-backup-{}.tmp", Uuid::new_v4());
    let temporary_path = parent.join(&temporary_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temporary_path)
            .map_err(|e| format!("Could not create Steam runtime backup: {e}"))?;
        write_backup(&mut file, original)
            .map_err(|e| format!("Could not finish Steam runtime backup: {e}"))?;
        if !same_identity(
            &parent_snapshot,
            &fs::symlink_metadata(parent).map_err(|e| e.to_string())?,
        ) {
            return Err("Steam runtime backup directory changed while writing".into());
        }
        let from = CString::new(temporary_name).map_err(|e| e.to_string())?;
        let to = CString::new(backup_name.as_bytes()).map_err(|e| e.to_string())?;
        let moved = unsafe {
            renameatx_np(
                parent_guard.as_raw_fd(),
                from.as_ptr(),
                parent_guard.as_raw_fd(),
                to.as_ptr(),
                RENAME_EXCL | RENAME_NOFOLLOW_ANY,
            )
        };
        if moved != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists
                && read_record_bytes(path)? == original
            {
                return Ok(());
            }
            return Err(format!("Could not publish Steam runtime backup: {error}"));
        }
        parent_guard
            .sync_all()
            .map_err(|e| format!("Could not sync Steam runtime backup directory: {e}"))
    })();
    // An interrupted write never becomes the durable backup that authorizes
    // replacing the prefix record. The unique staging file is disposable.
    let _ = fs::remove_file(&temporary_path);
    result
}

fn replace_record_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("migration-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn wait_for_idle_wineserver(dir: &Path, wine: &Path, timeout: Duration) -> Result<(), String> {
    let wineserver = wine
        .parent()
        .ok_or("Wine path has no parent")?
        .join("wineserver");
    if !is_executable(&wineserver) || !is_independent_runtime(&wineserver) {
        return Err("Cannot check whether the Steam bottle's Wine server is idle".into());
    }
    let prefix = dir.join("prefix");
    let xdg = bottle_xdg::for_bottle(dir)?;
    let mut probe = Command::new(wineserver);
    xdg.apply(&mut probe);
    let mut probe = probe
        .arg("-w")
        .env("WINEPREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not check this Steam bottle's Wine server: {e}"))?;
    let (status, timed_out) = wait_child_bounded_status(&mut probe, timeout)?;
    if timed_out {
        return Err(
            "Close Steam and other Windows apps in this bottle before upgrading its engine".into(),
        );
    }
    if !status.success() {
        return Err("Could not verify that this Steam bottle's Wine server is idle".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        home: PathBuf,
        bottle_id: String,
        wine: PathBuf,
        steam: PathBuf,
        record_path: PathBuf,
        backup_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
            fs::create_dir_all(&parent).unwrap();
            let home = parent.join(format!("runtime-migration-{}", Uuid::new_v4()));
            let bottle = create(&home, "Steam").unwrap();
            let root = home.join("Caskrun.app/Contents/Resources/wine-runtime");
            fs::create_dir_all(root.join("bin")).unwrap();
            fs::create_dir_all(root.join("runtime/bin")).unwrap();
            fs::create_dir_all(root.join("lib")).unwrap();
            fs::write(root.join("SOURCES.txt"), b"test package\n").unwrap();
            fs::write(root.join("lib/libfreetype.6.dylib"), b"test freetype").unwrap();
            for name in ["wine", "wineboot", "wineserver"] {
                fs::write(root.join("runtime/bin").join(name), b"test engine").unwrap();
                symlink("_dispatch", root.join("bin").join(name)).unwrap();
            }
            let dispatch = root.join("bin/_dispatch");
            fs::write(
                &dispatch,
                b"#!/bin/sh\ncase \"${0##*/}\" in wineboot) touch \"$WINEPREFIX/system.reg\";; wineserver) [ \"$1\" = -w ] || exit 1;; esac\nexit 0\n",
            )
            .unwrap();
            let mut mode = fs::metadata(&dispatch).unwrap().permissions();
            mode.set_mode(0o755);
            fs::set_permissions(&dispatch, mode).unwrap();
            let wine = root.join("bin/wine");
            ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            let prefix = dir.join("prefix");
            let steam = prefix.join("drive_c/Program Files (x86)/Steam/steam.exe");
            fs::create_dir_all(steam.parent().unwrap()).unwrap();
            fs::write(&steam, b"Steam test executable").unwrap();
            fs::write(prefix.join("keep.txt"), b"prefix data to preserve").unwrap();
            add_launcher(&home, &bottle.id, "Steam", &steam).unwrap();
            let mut attempt = steam_install::CatalogAttempt::new(
                &bottle.id,
                "steam",
                "a".repeat(64),
                "https://cdn.fastly.steamstatic.com/client/installer/SteamSetup.exe".into(),
            )
            .unwrap();
            attempt.set_stage(steam_install::CatalogStage::Installed);
            steam_install::save_catalog_attempt(&home, &attempt).unwrap();
            let record_path = dir.join("prefix-record.json");
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
            record.runtime_identity = Some(TLS_WINE_11_IDENTITY.into());
            record.runtime_fingerprint = Some(format!("sha256-bound-v2:{}", "1".repeat(64)));
            save_json(&record_path, &record).unwrap();
            let backup_path = home.join(format!(
                "runtime-migration-backups/{}-tls-to-steam-prefix-record.json",
                bottle.id
            ));
            Self {
                home,
                bottle_id: bottle.id,
                wine,
                steam,
                record_path,
                backup_path,
            }
        }

        fn approved_identity(&self) -> String {
            runtime_identity::bundled_runtime_identity(&self.wine)
                .unwrap()
                .unwrap()
        }

        fn policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: TLS_WINE_11_IDENTITY,
                approved,
            }
        }

        fn migrate(&self, policy: &MigrationPolicy<'_>) -> Result<Option<RecordRollback>, String> {
            let _lock = lock_bottle(&self.home, &self.bottle_id)?;
            migrate_with_policy(
                &self.home,
                &self.bottle_id,
                &self.wine,
                &self.steam.canonicalize().unwrap(),
                policy,
                Duration::from_millis(200),
            )
        }
    }

    #[test]
    fn failed_backup_write_cleans_staging_file_and_can_retry() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let error =
            retain_original_record_with_writer(&fixture.backup_path, &original, |file, bytes| {
                file.write_all(&bytes[..1]).map_err(|e| e.to_string())?;
                Err("simulated interrupted write".into())
            })
            .unwrap_err();
        assert!(error.contains("simulated interrupted write"));
        assert!(!fixture.backup_path.exists());
        assert_eq!(
            fs::read_dir(fixture.backup_path.parent().unwrap())
                .unwrap()
                .count(),
            0
        );
        retain_original_record(&fixture.backup_path, &original).unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn known_steam_transition_keeps_prefix_and_original_record() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let policy = fixture.policy(&approved);
        let _rollback = fixture.migrate(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        let prefix = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix");
        assert_eq!(
            fs::read(prefix.join("keep.txt")).unwrap(),
            b"prefix data to preserve"
        );
        let migrated: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert_eq!(migrated.runtime_identity.as_deref(), Some(policy.approved));
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        assert!(fixture.migrate(&policy).unwrap().is_none());
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn failed_verification_can_restore_exact_original_record() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.policy(&approved))
            .unwrap()
            .unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn unrelated_runtime_and_unapproved_package_are_rejected() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let wrong = MigrationPolicy {
            previous: TLS_WINE_11_IDENTITY,
            approved: "sha256:wrong",
        };
        assert!(fixture.migrate(&wrong).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());

        let mut record: PrefixRecord = serde_json::from_slice(&original).unwrap();
        record.runtime_identity = Some(format!("sha256:{}", "0".repeat(64)));
        save_json(&fixture.record_path, &record).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn busy_bottle_and_unexpected_backup_leave_old_record_in_place() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(fixture.migrate(&fixture.policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());

        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, b"unexpected previous backup").unwrap();
        assert!(fixture.migrate(&fixture.policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(
            fs::read(&fixture.backup_path).unwrap(),
            b"unexpected previous backup"
        );
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn only_an_installed_recipe_steam_launcher_can_trigger_migration() {
        let fixture = Fixture::new();
        let mut attempt = steam_install::catalog_attempt(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .unwrap();
        attempt.set_stage(steam_install::CatalogStage::Failed);
        steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn custom_recorded_path_stays_on_strict_runtime_check() {
        let fixture = Fixture::new();
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        record.wine_path = fixture.home.join("custom-wine").display().to_string();
        save_json(&fixture.record_path, &record).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(
            verify_runtime_record(&record, &fixture.wine)
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(fixture.home).unwrap();
    }

    #[test]
    fn busy_wineserver_prevents_record_change() {
        let fixture = Fixture::new();
        let dispatch = fixture.wine.parent().unwrap().join("_dispatch");
        fs::write(
            dispatch,
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) while :; do :; done;; esac\nexit 0\n",
        )
        .unwrap();
        let approved = fixture.approved_identity();
        let original = fs::read(&fixture.record_path).unwrap();
        let error = fixture.migrate(&fixture.policy(&approved)).err().unwrap();
        assert!(error.contains("Close Steam"), "{error}");
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(fixture.home).unwrap();
    }
}
