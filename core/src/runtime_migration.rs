//! Reviewed exact-package transitions for Caskrun's installed Steam bottles. Other Wine
//! changes continue through the exact-match check in `verify_runtime_record`.

use super::*;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

// Complete package-tree SHA-256 values produced by runtime_identity.rs. The
// TLS and Steam values preserve the original reviewed transition. The native
// successor was measured by this module's complete package attestation after
// an independently packaged runtime initialized a fresh disposable prefix.
const TLS_WINE_11_IDENTITY: &str =
    "sha256:6df2a9f5dfaa119ba3d6c84214d79f885c70e887ef7f3bc9d2255aeb8b2f88e6";
const STEAM_WINE_11_IDENTITY: &str =
    "sha256:07c5721ad17efa9d4ea89ff61522133407756c4aea8a47866625dcf118e7347c";
const NATIVE_WINE_11_IDENTITY: &str =
    "sha256:9dcab17f21490998ad03f6ee4f553da0992a1d9234dfe40efd82956b63fd47ce";
const RECORDED_DISPATCH: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch";
const SELECTED_WINE: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/wine";

struct MigrationPolicy<'a> {
    previous: &'a str,
    approved: &'a str,
    backup_suffix: &'a str,
    verify_launcher_pe: bool,
}

const TLS_TO_STEAM_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: STEAM_WINE_11_IDENTITY,
    backup_suffix: "tls-to-steam",
    verify_launcher_pe: false,
};

const STEAM_TO_NATIVE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: NATIVE_WINE_11_IDENTITY,
    backup_suffix: "steam-to-native",
    verify_launcher_pe: true,
};

// Skipping the intervening app release needs its own reviewed exact edge.
// It never adopts an arbitrary successor or rewrites the TLS record twice.
const TLS_TO_NATIVE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: NATIVE_WINE_11_IDENTITY,
    backup_suffix: "tls-to-native",
    verify_launcher_pe: true,
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
    migrate_with_policies(
        home,
        bottle_id,
        wine,
        launcher_path,
        &[
            &TLS_TO_STEAM_POLICY,
            &STEAM_TO_NATIVE_POLICY,
            &TLS_TO_NATIVE_POLICY,
        ],
        WINE_SERVER_CHECK_TIMEOUT,
    )
}

fn migrate_with_policies(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
    policies: &[&MigrationPolicy<'_>],
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
    let matching: Vec<_> = policies
        .iter()
        .copied()
        .filter(|policy| record.runtime_identity.as_deref() == Some(policy.previous))
        .collect();
    if matching.is_empty() {
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
    // Select by both pinned endpoints after hashing the complete destination.
    // Multiple successors for one predecessor must not cause the first edge
    // to reject a different explicitly reviewed edge, or hash the tree twice.
    let attestation = runtime_identity::bundled_runtime_attestation(wine)?
        .ok_or("The selected Windows engine is not a complete Caskrun package")?;
    if record.runtime_identity.as_deref() == Some(attestation.identity.as_str()) {
        return Ok(None);
    }
    let policy = matching
        .into_iter()
        .find(|policy| attestation.identity == policy.approved)
        .ok_or("The selected Windows engine is not the approved Steam migration package; this bottle was unchanged")?;
    // New native transitions additionally validate the installed program's PE
    // content. The established TLS-to-Steam policy retains its original gate.
    let verified_launcher = if policy.verify_launcher_pe {
        let before = fs::symlink_metadata(launcher_path)
            .map_err(|e| format!("Could not inspect the installed Steam launcher: {e}"))?;
        if !before.file_type().is_file() {
            return Err("The installed Steam launcher must be a regular Windows program".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(launcher_path)
            .map_err(|e| format!("Could not safely open the installed Steam launcher: {e}"))?;
        steam_install::inspect_pe_and_sha256(&mut file)
            .map_err(|e| format!("Could not verify the installed Steam launcher: {e}"))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("The installed Steam launcher changed during verification".into());
        }
        Some((file, before))
    } else {
        None
    };
    refuse_running_bottle(&dir)?;
    wait_for_idle_wineserver(&dir, wine, idle_timeout)?;
    // A server can exit while an orphaned client retains this exact bottle's
    // files. The read-only handle guard never signals an inventoried process.
    prefix_processes::wait_until_idle(&dir, idle_timeout).map_err(|error| {
        format!("Could not verify this Steam bottle is idle before changing its engine: {error}")
    })?;
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
    if let Some((file, snapshot)) = verified_launcher {
        if !same_snapshot(&snapshot, &file.metadata().map_err(|e| e.to_string())?)
            || !same_snapshot(
                &snapshot,
                &fs::symlink_metadata(launcher_path).map_err(|e| e.to_string())?,
            )
        {
            return Err("The installed Steam launcher changed during migration".into());
        }
    }
    if read_record_bytes(&record_path)? != original_bytes {
        return Err("Steam bottle runtime record changed during migration".into());
    }

    let backups = real_subdirectory(home, "runtime-migration-backups")?;
    let backup_path = backups.join(format!(
        "{bottle_id}-{}-prefix-record.json",
        policy.backup_suffix
    ));
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

#[cfg(test)]
fn migrate_with_policy(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
    policy: &MigrationPolicy<'_>,
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    migrate_with_policies(
        home,
        bottle_id,
        wine,
        launcher_path,
        &[policy],
        idle_timeout,
    )
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
            let mut steam_bytes = vec![0_u8; 100 * 1024];
            steam_bytes[..2].copy_from_slice(b"MZ");
            steam_bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
            steam_bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
            steam_bytes[0x84..0x86].copy_from_slice(&0x014c_u16.to_le_bytes());
            fs::write(&steam, steam_bytes).unwrap();
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
                backup_suffix: "tls-to-steam",
                verify_launcher_pe: false,
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
                Duration::from_secs(2),
            )
        }

        fn set_predecessor(&self, identity: &str) {
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&self.record_path).unwrap()).unwrap();
            record.runtime_identity = Some(identity.into());
            save_json(&self.record_path, &record).unwrap();
        }

        fn native_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: STEAM_WINE_11_IDENTITY,
                approved,
                backup_suffix: "steam-to-native",
                verify_launcher_pe: true,
            }
        }

        fn native_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-steam-to-native-prefix-record.json",
                self.bottle_id
            ))
        }

        fn direct_native_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: TLS_WINE_11_IDENTITY,
                approved,
                backup_suffix: "tls-to-native",
                verify_launcher_pe: true,
            }
        }

        fn direct_native_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-tls-to-native-prefix-record.json",
                self.bottle_id
            ))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn unrelated_runtime_and_unapproved_package_are_rejected() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let wrong = MigrationPolicy {
            previous: TLS_WINE_11_IDENTITY,
            approved: "sha256:wrong",
            backup_suffix: "tls-to-steam",
            verify_launcher_pe: false,
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
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
        fs::remove_dir_all(&fixture.home).unwrap();
    }
    #[test]
    fn native_transition_selects_record_predecessor_and_keeps_separate_backups() {
        let fixture = Fixture::new();
        let tls_original = fs::read(&fixture.record_path).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, &tls_original).unwrap();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let steam_original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:another-approved-steam-package");
        let native_policy = fixture.native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &native_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), tls_original);
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), steam_original);
        assert_eq!(
            fs::read(
                bottle_dir(&fixture.home, &fixture.bottle_id)
                    .unwrap()
                    .join("prefix/keep.txt")
            )
            .unwrap(),
            b"prefix data to preserve"
        );
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), steam_original);
    }

    #[test]
    fn tls_record_requires_an_explicitly_reviewed_direct_edge() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:required-intermediate-steam-package");
        let native_policy = fixture.native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let result = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &native_policy],
            Duration::from_secs(2),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists() && !fixture.native_backup().exists());
    }

    #[test]
    fn direct_native_update_selects_exact_target_and_preserves_tls_backup() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, &original).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:other-reviewed-steam-package");
        let steam_policy = fixture.native_policy(&approved);
        let direct_policy = fixture.direct_native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let before_hashes = runtime_identity::full_content_hash_count();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &steam_policy, &direct_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            runtime_identity::full_content_hash_count(),
            before_hashes + 1
        );
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        assert_eq!(fs::read(fixture.direct_native_backup()).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        let migrated: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert_eq!(
            migrated.runtime_identity.as_deref(),
            Some(approved.as_str())
        );
        let prefix = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix");
        assert_eq!(
            fs::read(prefix.join("keep.txt")).unwrap(),
            b"prefix data to preserve"
        );
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
    }

    #[test]
    fn direct_edge_does_not_change_original_tls_to_steam_gate() {
        let fixture = Fixture::new();
        // The established transition did not require new PE validation.
        fs::write(&fixture.steam, b"original historical launcher fixture").unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let direct_policy = fixture.direct_native_policy("sha256:other-reviewed-native-package");
        let tls_policy = fixture.policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&direct_policy, &tls_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
    }

    #[test]
    fn direct_native_update_refuses_a_different_existing_backup() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let backup = fixture.direct_native_backup();
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::write(&backup, b"different original record").unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(backup).unwrap(), b"different original record");
    }

    #[test]
    fn direct_native_update_refuses_wrong_or_changed_target_and_corrupt_launcher() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy("sha256:unapproved"))
                .is_err()
        );
        let approved = fixture.approved_identity();
        let component = fixture
            .wine
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("lib/libfreetype.6.dylib");
        fs::write(&component, b"changed package bytes").unwrap();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        let approved = fixture.approved_identity();
        fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap();
        let error = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("verify the installed Steam launcher"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
    }

    #[test]
    fn direct_native_update_requires_original_binding_and_installed_catalog_launcher() {
        for invalid in ["binding", "custom-path", "catalog-stage", "launcher"] {
            let fixture = Fixture::new();
            match invalid {
                "binding" | "custom-path" => {
                    let mut record: PrefixRecord =
                        serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
                    if invalid == "binding" {
                        record.runtime_fingerprint = Some("legacy-unbound-record".into());
                    } else {
                        record.wine_path = fixture.home.join("custom-wine").display().to_string();
                    }
                    save_json(&fixture.record_path, &record).unwrap();
                }
                "catalog-stage" => {
                    let mut attempt =
                        steam_install::catalog_attempt(&fixture.home, &fixture.bottle_id)
                            .unwrap()
                            .unwrap();
                    attempt.set_stage(steam_install::CatalogStage::Failed);
                    steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
                }
                "launcher" => save_json(
                    &bottle_dir(&fixture.home, &fixture.bottle_id)
                        .unwrap()
                        .join("launchers.json"),
                    &Vec::<Launcher>::new(),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let original = fs::read(&fixture.record_path).unwrap();
            let approved = fixture.approved_identity();
            assert!(
                fixture
                    .migrate(&fixture.direct_native_policy(&approved))
                    .unwrap()
                    .is_none(),
                "{invalid}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert!(!fixture.direct_native_backup().exists());
        }
    }

    #[test]
    fn direct_native_update_refuses_tracked_and_server_busy_states() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::write(
            fixture.wine.parent().unwrap().join("_dispatch"),
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) while :; do :; done;; esac\nexit 0\n",
        )
        .unwrap();
        let approved = fixture.approved_identity();
        let error = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .err()
            .unwrap();
        assert!(error.contains("Close Steam"), "{error}");
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
    }

    #[test]
    fn direct_native_update_requires_zero_owners_without_signalling_untracked_owner() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let held = fixture
            .steam
            .parent()
            .unwrap()
            .join("held-direct-update.log");
        fs::write(&held, b"original fixture").unwrap();
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
        drop(owner);
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn direct_native_update_rolls_back_exact_tls_record_after_failed_verification() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .unwrap()
            .unwrap();
        let registry = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix/system.reg");
        fs::remove_file(&registry).unwrap();
        fs::create_dir(&registry).unwrap();
        assert!(ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).is_err());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.direct_native_backup()).unwrap(), original);
    }

    #[test]
    fn selecting_exact_previous_package_does_not_migrate() {
        let fixture = Fixture::new();
        let previous = fixture.approved_identity();
        fixture.set_predecessor(&previous);
        let original = fs::read(&fixture.record_path).unwrap();
        let policy = MigrationPolicy {
            previous: &previous,
            approved: "sha256:different-successor",
            backup_suffix: "steam-to-native",
            verify_launcher_pe: true,
        };
        assert!(fixture.migrate(&policy).unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
    }

    #[test]
    fn native_transition_rejects_unapproved_and_corrupt_installed_launcher() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        assert!(
            fixture
                .migrate(&fixture.native_policy("sha256:unapproved"))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        let approved = fixture.approved_identity();
        fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap();
        let error = fixture
            .migrate(&fixture.native_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("verify the installed Steam launcher"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
    }

    #[test]
    fn native_transition_never_executes_recorded_engine() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let saved_engine = fixture
            .home
            .join("old/Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch");
        fs::create_dir_all(saved_engine.parent().unwrap()).unwrap();
        let marker = saved_engine.with_file_name("recorded-engine-was-executed");
        fs::write(
            &saved_engine,
            b"#!/bin/sh\nprintf executed > \"${0%/*}/recorded-engine-was-executed\"\nexit 1\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&saved_engine).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&saved_engine, permissions).unwrap();
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        record.wine_path = saved_engine.display().to_string();
        save_json(&fixture.record_path, &record).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.native_policy(&approved))
            .unwrap()
            .unwrap();
        assert!(!marker.exists());
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), original);
        rollback.restore().unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn native_transition_preserves_record_when_tracked_process_or_backup_is_busy() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(fixture.migrate(&fixture.native_policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::create_dir_all(fixture.native_backup().parent().unwrap()).unwrap();
        fs::write(fixture.native_backup(), b"unrelated native backup").unwrap();
        assert!(fixture.migrate(&fixture.native_policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(
            fs::read(fixture.native_backup()).unwrap(),
            b"unrelated native backup"
        );
    }

    #[test]
    fn native_transition_waits_for_exact_untracked_open_handle_without_signalling_it() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let held = fixture.steam.parent().unwrap().join("held-fixture.log");
        fs::write(&held, b"self-authored fixture").unwrap();
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let approved = fixture.approved_identity();
        let result = fixture.migrate(&fixture.native_policy(&approved));
        assert!(result.is_err());
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        drop(owner); // EOF lets our fixture exit naturally.
        assert!(
            fixture
                .migrate(&fixture.native_policy(&approved))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn native_transition_rolls_back_when_ordinary_prefix_verification_fails() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.native_policy(&approved))
            .unwrap()
            .unwrap();
        let registry = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix/system.reg");
        fs::remove_file(&registry).unwrap();
        fs::create_dir(&registry).unwrap();
        assert!(ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).is_err());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), original);
    }
}
