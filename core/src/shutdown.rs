//! Shut down Windows apps in Caskrun-owned bottles before quitting the host UI.
//!
//! Every stop request goes through the existing per-bottle controls. A recorded
//! predecessor is usable only after a separate full-package allowlist proof;
//! bottle metadata alone never authorizes execution or signalling saved PIDs.

use super::*;
use std::io::{Seek, SeekFrom};
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
// Exact reviewed predecessor identities also used by runtime_migration.rs.
// This shutdown-only fallback does not adopt or migrate any runtime record.
const SHUTDOWN_PREDECESSORS: [&str; 3] = [
    "sha256:6df2a9f5dfaa119ba3d6c84214d79f885c70e887ef7f3bc9d2255aeb8b2f88e6",
    "sha256:07c5721ad17efa9d4ea89ff61522133407756c4aea8a47866625dcf118e7347c",
    "sha256:9dcab17f21490998ad03f6ee4f553da0992a1d9234dfe40efd82956b63fd47ce",
];
const RECORDED_PREDECESSOR_DISPATCH: &str =
    "Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch";

struct RuntimeRecordSnapshot {
    path: PathBuf,
    file: File,
    metadata: fs::Metadata,
    bytes: Vec<u8>,
}

impl RuntimeRecordSnapshot {
    fn read(dir: &Path) -> Result<Option<Self>, String> {
        let path = dir.join("prefix-record.json");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Could not inspect the bottle runtime record for shutdown".into()),
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_PREFIX_RECORD_BYTES {
            return Err("Bottle runtime record must be a bounded regular file".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | 0x4) // Darwin O_NONBLOCK: a raced FIFO must not stall Quit.
            .open(&path)
            .map_err(|_| "Could not safely open the bottle runtime record for shutdown")?;
        let opened = file.metadata().map_err(|e| e.to_string())?;
        if !opened.file_type().is_file() || !same_snapshot(&metadata, &opened) {
            return Err("Bottle runtime record changed while opening it for shutdown".into());
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_PREFIX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PREFIX_RECORD_BYTES {
            return Err("Bottle runtime record is too large for shutdown".into());
        }
        let snapshot = Self {
            path,
            file,
            metadata,
            bytes,
        };
        snapshot.recheck()?;
        Ok(Some(snapshot))
    }

    fn recheck(&self) -> Result<(), String> {
        let unchanged = || -> Result<bool, String> {
            Ok(same_snapshot(
                &self.metadata,
                &self.file.metadata().map_err(|e| e.to_string())?,
            ) && same_snapshot(
                &self.metadata,
                &fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?,
            ))
        };
        if !unchanged()? {
            return Err("Bottle runtime record changed during shutdown".into());
        }
        let mut file = self.file.try_clone().map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_PREFIX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes != self.bytes || !unchanged()? {
            return Err("Bottle runtime record changed during shutdown".into());
        }
        Ok(())
    }

    fn predecessor_record(&self) -> Result<PrefixRecord, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            wine_path: String,
            #[serde(default)]
            runtime_identity: Option<String>,
            #[serde(default)]
            runtime_fingerprint: Option<String>,
        }
        let record: Record = serde_json::from_slice(&self.bytes)
            .map_err(|_| "Invalid bottle runtime record for predecessor shutdown")?;
        if !record
            .runtime_identity
            .as_deref()
            .is_some_and(|identity| SHUTDOWN_PREDECESSORS.contains(&identity))
        {
            return Err("The recorded engine is not a reviewed Caskrun predecessor".into());
        }
        Ok(PrefixRecord {
            wine_path: record.wine_path,
            runtime_identity: record.runtime_identity,
            runtime_fingerprint: record.runtime_fingerprint,
        })
    }
}

fn recheck_runtime_record(
    dir: &Path,
    record: Option<&RuntimeRecordSnapshot>,
) -> Result<(), String> {
    if let Some(record) = record {
        record.recheck()
    } else {
        match fs::symlink_metadata(dir.join("prefix-record.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err("Bottle runtime record changed during shutdown".into()),
        }
    }
}

struct PredecessorRuntime {
    wine: PathBuf,
    root: PathBuf,
    guard: File,
    snapshot: fs::Metadata,
    fingerprint: String,
}

impl PredecessorRuntime {
    fn recheck(&self) -> Result<(), String> {
        bottle_control::assert_same_prefix(&self.root, &self.guard, &self.snapshot)?;
        if runtime_identity::bundled_runtime_metadata_fingerprint(&self.wine)?.as_deref()
            != Some(self.fingerprint.as_str())
        {
            return Err("The reviewed predecessor package changed during shutdown".into());
        }
        Ok(())
    }
}

fn verified_predecessor_runtime(
    record: &RuntimeRecordSnapshot,
    attest: &mut impl FnMut(&Path) -> Result<Option<runtime_identity::BundleAttestation>, String>,
) -> Result<PredecessorRuntime, String> {
    let parsed = record.predecessor_record()?;
    let dispatch = Path::new(&parsed.wine_path);
    if !dispatch.is_absolute()
        || !dispatch.ends_with(RECORDED_PREDECESSOR_DISPATCH)
        || dispatch.canonicalize().ok().as_deref() != Some(dispatch)
    {
        return Err(
            "The recorded predecessor must be a canonical independent Caskrun package".into(),
        );
    }
    let bin = dispatch
        .parent()
        .ok_or("The recorded predecessor has no bin directory")?;
    let wine = bin.join("wine");
    let root = bin
        .parent()
        .ok_or("The recorded predecessor has no package directory")?
        .to_path_buf();
    let (guard, snapshot) = open_stable_directory(&root)?;
    let wineserver = bin.join("wineserver");
    if !is_usable_wine(&wine)
        || !is_executable(&wineserver)
        || !is_independent_runtime(&wineserver)
        || wine.canonicalize().ok().as_deref() != Some(dispatch)
    {
        return Err("The recorded independent predecessor package is unavailable".into());
    }
    // Mandatory content proof: the ordinary verifier's record-bound metadata
    // fast path is insufficient when metadata supplies the engine location.
    cancellation::check()?;
    let attestation =
        attest(&wine)?.ok_or("The recorded engine is not a complete Caskrun package")?;
    cancellation::check()?;
    if !SHUTDOWN_PREDECESSORS.contains(&attestation.identity.as_str())
        || parsed.runtime_identity.as_deref() != Some(attestation.identity.as_str())
    {
        return Err("The recorded predecessor package does not match its reviewed bytes".into());
    }
    record.recheck()?;
    if !runtime_record_matches(&parsed, &wine)? {
        return Err("The recorded predecessor failed ordinary runtime verification".into());
    }
    let verified = PredecessorRuntime {
        wine,
        root,
        guard,
        snapshot,
        fingerprint: attestation.metadata_fingerprint,
    };
    verified.recheck()?;
    record.recheck()?;
    Ok(verified)
}

#[derive(Debug, Serialize)]
pub(crate) struct ShutdownFailure {
    pub(crate) bottle_id: String,
    pub(crate) name: String,
    pub(crate) error: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShutdownResult {
    pub(crate) complete: bool,
    pub(crate) failures: Vec<ShutdownFailure>,
    /// Bottles that received a successful stop request. Already-idle bottles
    /// are verified or skipped, but do not inflate this count.
    pub(crate) stopped_bottles: usize,
}

pub(crate) fn quit_all(home: &Path) -> Result<ShutdownResult, String> {
    let wine = wine_path();
    quit_all_with_wine(home, wine.as_deref(), SHUTDOWN_TIMEOUT)
}

fn quit_all_with_wine(
    home: &Path,
    wine: Option<&Path>,
    timeout: Duration,
) -> Result<ShutdownResult, String> {
    let root = storage_root(home, false)?;
    // Do not let a dangling storage symlink masquerade as an empty library.
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShutdownResult {
                complete: true,
                failures: Vec::new(),
                stopped_bottles: 0,
            });
        }
        Err(error) => return Err(format!("Could not inspect Caskrun bottle storage: {error}")),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err("Caskrun bottle storage must be a real directory".into());
        }
        Ok(_) => {}
    }
    let (root_guard, root_snapshot) = open_stable_directory(&root)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("Could not list Caskrun bottles: {e}"))? {
        let entry = entry.map_err(|e| format!("Could not read Caskrun bottle entry: {e}"))?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if Uuid::parse_str(&id).is_ok() {
            candidates.push(id);
        }
    }
    candidates.sort();
    let mut result = ShutdownResult {
        complete: true,
        failures: Vec::new(),
        stopped_bottles: 0,
    };
    for id in candidates {
        // A malformed/symlinked UUID directory is reported rather than being
        // silently omitted by the normal library listing. Other valid bottles
        // must still receive their own independent shutdown attempt.
        let candidate = bottle_control::assert_same_prefix(&root, &root_guard, &root_snapshot)
            .and_then(|_| bottle_dir(home, &id))
            .and_then(|dir| read_bottle_record(&dir, &id));
        let bottle = match candidate {
            Ok(bottle) => bottle,
            Err(error) => {
                result.failures.push(ShutdownFailure {
                    bottle_id: id.clone(),
                    name: id,
                    error,
                });
                continue;
            }
        };
        match quit_one(home, &bottle.id, wine, timeout) {
            Ok(stopped) => result.stopped_bottles += usize::from(stopped),
            Err(error) => result.failures.push(ShutdownFailure {
                bottle_id: bottle.id,
                name: bottle.name,
                error,
            }),
        }
    }
    result.complete = result.failures.is_empty();
    Ok(result)
}

fn quit_one(home: &Path, id: &str, wine: Option<&Path>, timeout: Duration) -> Result<bool, String> {
    quit_one_with_attester(
        home,
        id,
        wine,
        timeout,
        runtime_identity::bundled_runtime_attestation,
    )
}

fn quit_one_with_attester(
    home: &Path,
    id: &str,
    wine: Option<&Path>,
    timeout: Duration,
    mut attest: impl FnMut(&Path) -> Result<Option<runtime_identity::BundleAttestation>, String>,
) -> Result<bool, String> {
    let _lock = lock_bottle(home, id)?;
    let dir = bottle_dir(home, id)?;
    let (dir_guard, dir_snapshot) = open_stable_directory(&dir)?;
    let prefix = dir.join("prefix");
    let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
    let record_snapshot = RuntimeRecordSnapshot::read(&dir)?;
    let record = recorded_runtime_for_prefix(&dir, wine)?;
    recheck_runtime_record(&dir, record_snapshot.as_ref())?;
    let has_record = record_snapshot.is_some();

    if record.is_none() {
        // Fresh bottles cannot have a Wine server without initialization.
        // Live tracking means this is inconsistent, so do not claim idle.
        if !live_processes(&dir)?.is_empty() {
            return Err("This uninitialized bottle still has tracked processes. Caskrun cannot safely verify that its Windows apps stopped.".into());
        }
        prefix_processes::wait_until_idle(&dir, timeout)?;
        bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
        return Ok(false);
    }

    if !has_record {
        // A copied/imported registry is not proof of a running Caskrun
        // engine. A read-only, prefix-scoped wait can establish idle using
        // the selected runtime without adopting or executing source code.
        refuse_live_wine_server(&dir, timeout, wine)?;
        if !live_processes(&dir)?.is_empty() {
            return Err("Wine is idle, but this bottle still has tracked processes. Caskrun cannot safely verify that they stopped.".into());
        }
        prefix_processes::wait_until_idle(&dir, timeout)?;
        bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
        return Ok(false);
    }

    let record = record
        .as_ref()
        .ok_or("This initialized bottle has no runtime record")?;
    let selected_matches = wine.is_some_and(|wine| {
        is_usable_wine(wine) && runtime_record_matches(record, wine).unwrap_or(false)
    });
    let predecessor = if selected_matches {
        None
    } else {
        let snapshot = record_snapshot
            .as_ref()
            .ok_or("This initialized bottle has no runtime record")?;
        Some(verified_predecessor_runtime(snapshot, &mut attest).map_err(|error| {
            if wine.is_none() {
                format!("Caskrun's included Wine runtime is unavailable, and its recorded predecessor could not be verified: {error}")
            } else {
                format!("This bottle uses a different Windows engine, and its recorded predecessor could not be verified: {error}")
            }
        })?)
    };
    let wine = predecessor
        .as_ref()
        .map(|runtime| runtime.wine.as_path())
        .or(wine)
        .ok_or("The verified shutdown engine is unavailable")?;
    // Both quit commands share server shutdown, verified native-owner fallback,
    // and final idle checks. Keep this lock continuously from preflight through
    // completion; only host shutdown permits a recorded prefix's registry loss.
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    bottle_control::assert_same_prefix(&dir, &dir_guard, &dir_snapshot)?;
    recheck_runtime_record(&dir, record_snapshot.as_ref())?;
    if let Some(predecessor) = &predecessor {
        predecessor.recheck()?;
    }
    let stopped = bottle_control::quit_recorded_windows_apps_locked(home, id, wine, timeout)?;
    bottle_control::assert_same_prefix(&prefix, &prefix_guard, &prefix_snapshot)?;
    bottle_control::assert_same_prefix(&dir, &dir_guard, &dir_snapshot)?;
    recheck_runtime_record(&dir, record_snapshot.as_ref())?;
    if let Some(predecessor) = &predecessor {
        predecessor.recheck()?;
    }
    if !stopped.server_stopped || stopped.tracked_after > 0 {
        return Err("Caskrun could not confirm that this bottle's Windows apps stopped".into());
    }
    Ok(stopped.forced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
        fs::create_dir_all(&parent).unwrap();
        parent.join(format!("caskrun-shutdown-test-{}", Uuid::new_v4()))
    }

    fn fake_runtime(home: &Path) -> PathBuf {
        let runtime = home.join("test-runtime");
        fs::create_dir_all(&runtime).unwrap();
        let wine = runtime.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::write(
            runtime.join("wineboot"),
            b"#!/bin/sh\ntouch \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        fs::write(
            runtime.join("wineserver"),
            b"#!/bin/sh\ncase \"$1\" in\n-k) printf '%s\\n' \"$WINEPREFIX\" >> \"$WINEPREFIX/quit-request\"; if test -f \"$WINEPREFIX/fail-quit\"; then exit 1; elif test -f \"$WINEPREFIX/mock.pid\"; then kill \"$(cat \"$WINEPREFIX/mock.pid\")\"; rm \"$WINEPREFIX/mock.pid\"; else exit 1; fi;;\n-w) test ! -f \"$WINEPREFIX/mock.pid\" && test ! -f \"$WINEPREFIX/fail-quit\";;\n*) exit 64;;\nesac\n",
        )
        .unwrap();
        for name in ["wine", "wineboot", "wineserver"] {
            let path = runtime.join(name);
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
        wine
    }

    fn initialized(home: &Path, name: &str, wine: &Path) -> (Bottle, PathBuf) {
        let bottle = create(home, name).unwrap();
        ensure_prefix_with_wine(home, &bottle.id, wine).unwrap();
        let prefix = bottle_dir(home, &bottle.id).unwrap().join("prefix");
        (bottle, prefix)
    }

    fn fake_app(prefix: &Path) -> Child {
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        fs::write(prefix.join("mock.pid"), child.id().to_string()).unwrap();
        child
    }

    struct PredecessorFixture {
        home: PathBuf,
        bottle: Bottle,
        dir: PathBuf,
        wine: PathBuf,
        identity: &'static str,
    }

    impl Drop for PredecessorFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    impl PredecessorFixture {
        fn new(identity: &'static str) -> Self {
            let home = test_home();
            let bottle = create(&home, "Reviewed predecessor fixture").unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            fs::write(dir.join("prefix/system.reg"), b"synthetic registry").unwrap();
            let root = home.join("Caskrun.app/Contents/Resources/wine-runtime");
            for folder in ["bin", "runtime/bin", "lib"] {
                fs::create_dir_all(root.join(folder)).unwrap();
            }
            fs::write(
                root.join("SOURCES.txt"),
                b"Original mock package; never a real Wine engine",
            )
            .unwrap();
            fs::write(root.join("lib/libfreetype.6.dylib"), b"original fixture").unwrap();
            for name in ["wine", "wineboot", "wineserver"] {
                symlink("_dispatch", root.join("bin").join(name)).unwrap();
                fs::write(root.join("runtime/bin").join(name), b"#!/bin/sh\nexit 9\n").unwrap();
                fs::set_permissions(
                    root.join("runtime/bin").join(name),
                    fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            fs::write(root.join("bin/_dispatch"), b"#!/bin/sh\ncase \"${0##*/}\" in\nwineserver) printf '%s\\n' \"$1\" >> \"$WINEPREFIX/server-calls\"; case \"$1\" in\n-k) if test -f \"$WINEPREFIX/mock.pid\"; then kill \"$(cat \"$WINEPREFIX/mock.pid\")\"; rm \"$WINEPREFIX/mock.pid\"; else exit 1; fi;;\n-w) test ! -f \"$WINEPREFIX/mock.pid\";;\n*) exit 64;;\nesac;;\n*) printf unexpected > \"$WINEPREFIX/unexpected-engine-call\"; exit 9;;\nesac\n").unwrap();
            fs::set_permissions(
                root.join("bin/_dispatch"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            let fixture = Self {
                home,
                bottle,
                dir,
                wine: root.join("bin/wine"),
                identity,
            };
            fixture.bind_record();
            fixture
        }

        fn bind_record(&self) {
            let metadata = runtime_identity::bundled_runtime_metadata_fingerprint(&self.wine)
                .unwrap()
                .unwrap();
            save_json(
                &self.dir.join("prefix-record.json"),
                &PrefixRecord {
                    wine_path: self.wine.canonicalize().unwrap().display().to_string(),
                    runtime_identity: Some(self.identity.into()),
                    runtime_fingerprint: Some(runtime_identity::bind_record_fingerprint(
                        self.identity,
                        &metadata,
                    )),
                },
            )
            .unwrap();
        }

        // The real hash of this deliberately tiny original shell package is
        // unknown to production. Only these positive branch tests substitute
        // a reviewed digest; real layout, metadata, ordinary verification,
        // prefix controls and owner inspections still execute normally.
        fn mock_reviewed_attestation(
            &self,
            wine: &Path,
        ) -> Result<Option<runtime_identity::BundleAttestation>, String> {
            assert_eq!(wine, self.wine);
            let mut proof = runtime_identity::bundled_runtime_attestation(wine)?
                .ok_or("Missing fixture package")?;
            proof.identity = self.identity.into();
            Ok(Some(proof))
        }

        fn run_mocked(&self, selected: Option<&Path>) -> Result<bool, String> {
            quit_one_with_attester(
                &self.home,
                &self.bottle.id,
                selected,
                Duration::from_millis(700),
                |wine| self.mock_reviewed_attestation(wine),
            )
        }
    }

    struct OwnedTestChild(Child);
    impl Drop for OwnedTestChild {
        fn drop(&mut self) {
            drop(self.0.stdin.take());
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    #[test]
    fn every_reviewed_predecessor_can_verify_idle_without_record_migration() {
        for identity in SHUTDOWN_PREDECESSORS {
            let fixture = PredecessorFixture::new(identity);
            let record = fixture.dir.join("prefix-record.json");
            let original = fs::read(&record).unwrap();
            assert!(!fixture.run_mocked(None).unwrap());
            assert_eq!(fs::read(&record).unwrap(), original);
            assert!(!fixture.home.join("runtime-migration-backups").exists());
            assert!(!fixture.dir.join("prefix/unexpected-engine-call").exists());
            let calls = fs::read_to_string(fixture.dir.join("prefix/server-calls")).unwrap();
            assert!(calls.starts_with("-k\n-w\n"));
        }
    }

    #[test]
    fn reviewed_predecessor_stops_its_running_fixture_without_using_selected_engine() {
        let fixture = PredecessorFixture::new(SHUTDOWN_PREDECESSORS[1]);
        let selected = fake_runtime(&fixture.home.join("selected"));
        fs::write(
            selected.parent().unwrap().join("wineserver"),
            b"#!/bin/sh\nprintf wrong > \"$WINEPREFIX/wrong-selected-engine\"; exit 9\n",
        )
        .unwrap();
        let mut child = OwnedTestChild(fake_app(&fixture.dir.join("prefix")));
        assert!(fixture.run_mocked(Some(&selected)).unwrap());
        assert!(child.0.wait().unwrap().signal().is_some());
        assert!(!fixture.dir.join("prefix/wrong-selected-engine").exists());
    }

    #[test]
    fn forged_metadata_cannot_authorize_an_unreviewed_package_for_shutdown() {
        for identity in SHUTDOWN_PREDECESSORS {
            let fixture = PredecessorFixture::new(identity);
            let record_path = fixture.dir.join("prefix-record.json");
            let bytes = fs::read(&record_path).unwrap();
            let record: PrefixRecord = serde_json::from_slice(&bytes).unwrap();
            assert!(
                runtime_record_matches(&record, &fixture.wine).unwrap(),
                "the fixture deliberately passes the metadata fast path"
            );
            let before = runtime_identity::full_content_hash_count();
            assert!(
                quit_one(
                    &fixture.home,
                    &fixture.bottle.id,
                    None,
                    Duration::from_secs(1)
                )
                .unwrap_err()
                .contains("reviewed bytes")
            );
            assert!(runtime_identity::full_content_hash_count() > before);
            assert_eq!(fs::read(record_path).unwrap(), bytes);
            assert!(!fixture.dir.join("prefix/server-calls").exists());
        }
    }

    #[test]
    fn predecessor_shutdown_refuses_unrecognized_records_paths_and_incomplete_packages() {
        for case in [
            "unknown",
            "current",
            "path-only",
            "malformed",
            "extra",
            "other-app",
            "missing-server",
            "external-wine",
            "incomplete",
        ] {
            let fixture = PredecessorFixture::new(SHUTDOWN_PREDECESSORS[0]);
            let path = fixture.dir.join("prefix-record.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match case {
                "unknown" => value["runtime_identity"] = format!("{}0", fixture.identity).into(),
                "current" => {
                    value["runtime_identity"] =
                        runtime_migration::SWITCHABLE_WINE_11_IDENTITY.into()
                }
                "path-only" => {
                    value.as_object_mut().unwrap().remove("runtime_identity");
                }
                "malformed" => {}
                "extra" => value["unrecognized"] = true.into(),
                "other-app" => {
                    fs::rename(
                        fixture.home.join("Caskrun.app"),
                        fixture.home.join("Other.app"),
                    )
                    .unwrap();
                    value["wine_path"] = fixture
                        .home
                        .join("Other.app/Contents/Resources/wine-runtime/bin/_dispatch")
                        .display()
                        .to_string()
                        .into();
                }
                "missing-server" => {
                    fs::remove_file(fixture.wine.parent().unwrap().join("wineserver")).unwrap()
                }
                "external-wine" => {
                    fs::remove_file(&fixture.wine).unwrap();
                    symlink(fake_runtime(&fixture.home.join("external")), &fixture.wine).unwrap();
                }
                "incomplete" => fs::remove_file(
                    fixture
                        .wine
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("lib/libfreetype.6.dylib"),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            fs::write(
                &path,
                if case == "malformed" {
                    b"malformed".to_vec()
                } else {
                    serde_json::to_vec(&value).unwrap()
                },
            )
            .unwrap();
            let bytes = fs::read(&path).unwrap();
            assert!(fixture.run_mocked(None).is_err(), "{case}");
            assert_eq!(fs::read(&path).unwrap(), bytes, "{case}");
            assert!(!fixture.dir.join("prefix/server-calls").exists(), "{case}");
        }
    }

    #[test]
    fn predecessor_shutdown_refuses_record_or_prefix_replacement_before_stopping() {
        for case in ["record-bytes", "record-inode", "prefix"] {
            let fixture = PredecessorFixture::new(SHUTDOWN_PREDECESSORS[2]);
            let path = fixture.dir.join("prefix-record.json");
            let original = fs::read(&path).unwrap();
            let result = quit_one_with_attester(
                &fixture.home,
                &fixture.bottle.id,
                None,
                Duration::from_secs(1),
                |wine| {
                    let proof = fixture.mock_reviewed_attestation(wine)?;
                    match case {
                        "record-bytes" => {
                            let mut changed = original.clone();
                            changed.push(b' ');
                            fs::write(&path, changed).unwrap();
                        }
                        "record-inode" => {
                            fs::rename(&path, path.with_extension("previous")).unwrap();
                            fs::write(&path, &original).unwrap();
                        }
                        "prefix" => {
                            fs::rename(
                                fixture.dir.join("prefix"),
                                fixture.dir.join("prefix-original"),
                            )
                            .unwrap();
                            fs::create_dir(fixture.dir.join("prefix")).unwrap();
                        }
                        _ => unreachable!(),
                    }
                    Ok(proof)
                },
            );
            assert!(result.unwrap_err().contains("changed"), "{case}");
            assert!(!fixture.dir.join("prefix/server-calls").exists());
            assert!(!fixture.dir.join("prefix-original/server-calls").exists());
            if case == "record-bytes" {
                assert_eq!(fs::read(&path).unwrap().last(), Some(&b' '));
            }
        }
    }

    #[test]
    fn predecessor_shutdown_still_refuses_live_tracked_and_unverified_native_owners() {
        use std::io::BufRead;
        for native in [false, true] {
            let fixture = PredecessorFixture::new(SHUTDOWN_PREDECESSORS[1]);
            let mut observer = if native {
                let held = fixture.dir.join("held-fixture.log");
                fs::write(&held, b"original owner fixture").unwrap();
                let mut owner = OwnedTestChild(
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
                Some(owner)
            } else {
                save_json(
                    &fixture.dir.join("active-processes.json"),
                    &vec![std::process::id()],
                )
                .unwrap();
                None
            };
            let result = fixture.run_mocked(None);
            if let Some(owner) = &mut observer {
                assert!(owner.0.try_wait().unwrap().is_none());
            }
            drop(observer);
            assert!(result.is_err());
            assert!(process_alive(std::process::id()).unwrap());
        }
    }

    #[test]
    fn predecessor_shutdown_reports_a_record_changed_during_the_stop_without_repairing_it() {
        let fixture = PredecessorFixture::new(SHUTDOWN_PREDECESSORS[2]);
        let dispatch = fixture.wine.parent().unwrap().join("_dispatch");
        let script = fs::read_to_string(&dispatch).unwrap().replace(
            "-w) test ! -f",
            "-w) printf ' ' >> \"$WINEPREFIX/../prefix-record.json\"; test ! -f",
        );
        fs::write(&dispatch, script).unwrap();
        fixture.bind_record();
        let path = fixture.dir.join("prefix-record.json");
        let original = fs::read(&path).unwrap();
        assert!(
            fixture
                .run_mocked(None)
                .unwrap_err()
                .contains("record changed")
        );
        let changed = fs::read(&path).unwrap();
        assert!(changed.starts_with(&original) && changed.len() > original.len());
        assert!(changed[original.len()..].iter().all(|byte| *byte == b' '));
        assert!(!fixture.home.join("runtime-migration-backups").exists());
    }

    #[test]
    fn stops_all_own_bottles_without_touching_other_home() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, first_prefix) = initialized(&home, "Steam", &wine);
        let (_, second_prefix) = initialized(&home, "Editor", &wine);
        let other_home = home.join("other-owner");
        let (_, other_prefix) = initialized(&other_home, "Other Wine user", &wine);
        let mut first = fake_app(&first_prefix);
        let mut second = fake_app(&second_prefix);
        let mut other = fake_app(&other_prefix);

        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 2);
        assert!(first.wait().unwrap().signal().is_some());
        assert!(second.wait().unwrap().signal().is_some());
        assert!(other.try_wait().unwrap().is_none());
        assert!(!other_prefix.join("quit-request").exists());
        for prefix in [&first_prefix, &second_prefix] {
            assert_eq!(
                fs::read_to_string(prefix.join("quit-request"))
                    .unwrap()
                    .trim(),
                prefix.display().to_string()
            );
        }
        other.kill().unwrap();
        other.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failure_does_not_skip_other_bottles_and_identifies_failed_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (failed, failed_prefix) = initialized(&home, "Will not stop", &wine);
        let (_, healthy_prefix) = initialized(&home, "Stops normally", &wine);
        fs::write(failed_prefix.join("fail-quit"), b"mock failure").unwrap();
        let mut healthy = fake_app(&healthy_prefix);

        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.stopped_bottles, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].bottle_id, failed.id);
        assert_eq!(result.failures[0].name, "Will not stop");
        assert!(result.failures[0].error.contains("server did not stop"));
        assert!(healthy.wait().unwrap().signal().is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn empty_and_uninitialized_bottles_need_no_runtime() {
        let home = test_home();
        assert!(
            quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT)
                .unwrap()
                .complete
        );
        create(&home, "Not installed yet").unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 0);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn verifies_idle_import_without_record_and_does_not_initialize_it() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Imported data", &wine);
        let record = bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix-record.json");
        fs::remove_file(&record).unwrap();
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 0);
        assert!(!record.exists());
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_runtime_does_not_execute_the_recorded_engine() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Running or unverifiable", &wine);
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        assert!(result.failures[0].error.contains("runtime is unavailable"));
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_registry_does_not_prevent_stopping_a_verified_running_bottle() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, prefix) = initialized(&home, "Damaged registry", &wine);
        let mut child = fake_app(&prefix);
        fs::remove_file(prefix.join("system.reg")).unwrap();
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(result.complete && result.failures.is_empty());
        assert_eq!(result.stopped_bottles, 1);
        assert!(child.wait().unwrap().signal().is_some());
        assert!(!prefix.join("system.reg").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_registry_fallback_keeps_the_selected_runtime_guard() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (_, prefix) = initialized(&home, "Damaged registry", &wine);
        fs::remove_file(prefix.join("system.reg")).unwrap();
        let other_wine = fake_runtime(&home.join("other-engine"));
        let result = quit_all_with_wine(&home, Some(&other_wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert!(
            result.failures[0]
                .error
                .contains("different Windows engine")
        );
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_wrong_engine_without_stopping_its_bottle() {
        let home = test_home();
        let first_wine = fake_runtime(&home);
        let (bottle, prefix) = initialized(&home, "Pinned engine", &first_wine);
        let second_wine = fake_runtime(&home.join("other-engine"));
        let result = quit_all_with_wine(&home, Some(&second_wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        assert!(
            result.failures[0]
                .error
                .contains("different Windows engine")
        );
        assert!(!prefix.join("quit-request").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn symlinked_prefix_is_a_failure_but_valid_bottles_are_attempted() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let (invalid, invalid_prefix) = initialized(&home, "Unsafe path", &wine);
        let (_, valid_prefix) = initialized(&home, "Valid", &wine);
        let outside = home.join("unowned-prefix");
        fs::rename(&invalid_prefix, &outside).unwrap();
        symlink(&outside, &invalid_prefix).unwrap();
        let mut valid = fake_app(&valid_prefix);
        let result = quit_all_with_wine(&home, Some(&wine), SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures[0].bottle_id, invalid.id);
        assert_eq!(result.stopped_bottles, 1);
        assert!(!outside.join("quit-request").exists());
        assert!(valid.wait().unwrap().signal().is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn symlinked_or_malformed_bottle_record_is_not_silently_skipped() {
        let home = test_home();
        let bottle = create(&home, "Unsafe metadata").unwrap();
        let metadata = bottle_dir(&home, &bottle.id).unwrap().join("bottle.json");
        let target = home.join("unowned.json");
        fs::rename(&metadata, &target).unwrap();
        symlink(&target, &metadata).unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].bottle_id, bottle.id);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn live_tracking_in_uninitialized_prefix_is_not_killed_or_reported_idle() {
        let home = test_home();
        let bottle = create(&home, "Inconsistent tracking").unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        save_json(&dir.join("active-processes.json"), &vec![child.id()]).unwrap();
        let result = quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).unwrap();
        assert!(!result.complete);
        assert!(result.failures[0].error.contains("tracked processes"));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refuses_dangling_storage_symlink() {
        let home = test_home();
        fs::create_dir_all(&home).unwrap();
        symlink(home.join("nonexistent"), home.join("bottles")).unwrap();
        assert!(quit_all_with_wine(&home, None, SHUTDOWN_TIMEOUT).is_err());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn idle_wineserver_cannot_hide_an_untracked_owner_of_bottle_files() {
        use std::io::BufRead;
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Untracked client").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let logs = real_subdirectory(&dir, "logs").unwrap();
        let log = logs.join("orphan-fixture.log");
        fs::write(&log, b"fixture").unwrap();
        let mut observer = Command::new("/bin/sh")
            .args([
                "-c",
                "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                "fixture",
            ])
            .arg(&log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        std::io::BufReader::new(observer.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let result = quit_all_with_wine(&home, Some(&wine), Duration::from_millis(500)).unwrap();
        // Close the fixture's own pipe before any assertion; it exits naturally.
        drop(observer.stdin.take());
        observer.wait().unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures.len(), 1);
        assert!(
            result.failures[0].error.contains("still use this bottle"),
            "{}",
            result.failures[0].error
        );
        assert!(dir.is_dir());
        assert!(read_active_processes(&dir).unwrap().is_empty());
        assert!(
            quit_all_with_wine(&home, Some(&wine), Duration::from_secs(2))
                .unwrap()
                .complete
        );
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn shutdown_retains_the_original_prefix_identity_through_the_server_wait() {
        let home = test_home();
        let wine = fake_runtime(&home);
        let bottle = create(&home, "Replaced prefix").unwrap();
        ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
        let dir = bottle_dir(&home, &bottle.id).unwrap();
        let server = wine.parent().unwrap().join("wineserver");
        fs::write(&server, b"#!/bin/sh\ncase \"$1\" in\n-k) exit 1;;\n-w) mv \"$WINEPREFIX\" \"$WINEPREFIX-original\"; mkdir \"$WINEPREFIX\"; touch \"$WINEPREFIX/system.reg\"; exit 0;;\nesac\n").unwrap();
        let result = quit_all_with_wine(&home, Some(&wine), Duration::from_secs(2)).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0].error.contains("prefix changed"));
        assert!(dir.join("prefix-original").is_dir());
        fs::remove_dir_all(home).unwrap();
    }
}
