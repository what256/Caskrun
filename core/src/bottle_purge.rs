//! Permanent removal is restricted to an already deleted, idle bottle.
//! Every recursive operation is relative to a retained directory descriptor;
//! payload links are removed as links and can never redirect the traversal.

use super::*;
use std::ffi::OsString;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

const IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_DEPTH: usize = 128;
const MAX_ENTRIES: usize = 1_000_000;
const PRESERVED: &str =
    "Deleted bottle changed while preparing permanent deletion; it was preserved";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    kind: libc::mode_t,
}

impl Identity {
    fn metadata(value: &fs::Metadata) -> Self {
        Self {
            device: value.dev(),
            inode: value.ino(),
            kind: value.mode() as libc::mode_t & libc::S_IFMT,
        }
    }

    fn stat(value: &libc::stat) -> Self {
        Self {
            device: value.st_dev as u64,
            inode: value.st_ino,
            kind: value.st_mode & libc::S_IFMT,
        }
    }
}

struct Node {
    name: OsString,
    identity: Identity,
    children: Vec<Node>,
}

pub(super) fn purge_deleted_bottle(home: &Path, id: &str) -> Result<Bottle, String> {
    let selected = wine_path();
    purge_deleted_with_timeout(home, id, IDLE_TIMEOUT, selected.as_deref())
}

fn purge_deleted_with_timeout(
    home: &Path,
    id: &str,
    timeout: Duration,
    selected_wine: Option<&Path>,
) -> Result<Bottle, String> {
    cancellation::check()?;
    if Uuid::parse_str(id).is_err() {
        return Err("Invalid bottle ID".into());
    }
    ensure_independent_storage(home)?;
    let (home_guard, home_snapshot) = open_stable_directory(home)?;
    refuse_active_entry(&home_guard, id)?;
    let root = deleted_root(home, false)?.ok_or("Deleted bottle not found")?;
    let (root_guard, root_snapshot) = open_stable_directory(&root)?;
    let source = root.join(id);
    let (source_guard, source_snapshot) = open_stable_directory(&source)
        .map_err(|_| "Deleted bottle not found or is not a real directory")?;
    require_owned_directory(&source_guard, source_snapshot.dev())?;
    let _lock = lock_at(&source_guard, timeout)?;
    let bottle = read_bottle_record(&source, id)?;
    assert_locations(
        home,
        &home_guard,
        &home_snapshot,
        &root,
        &root_guard,
        &root_snapshot,
        id,
        &source_guard,
        &source_snapshot,
    )?;
    // Saved PIDs alone do not cover orphaned clients or inherited file handles.
    // These checks never signal apps or execute a runtime selected by the record.
    refuse_running_bottle(&source)?;
    refuse_unverified_runtime(&source, timeout, selected_wine)?;
    prefix_processes::wait_until_idle(&source, timeout)?;
    assert_locations(
        home,
        &home_guard,
        &home_snapshot,
        &root,
        &root_guard,
        &root_snapshot,
        id,
        &source_guard,
        &source_snapshot,
    )?;

    // Scan the whole tree before removal: unsupported mounts, inaccessible
    // directories, or excessive trees fail without deleting any payload file.
    let mut entries = 0;
    let tree = inspect_tree(
        &source_guard,
        OsString::from(id),
        source_snapshot.dev(),
        0,
        &mut entries,
    )?;
    prefix_processes::wait_until_idle(&source, timeout)?;
    assert_locations(
        home,
        &home_guard,
        &home_snapshot,
        &root,
        &root_guard,
        &root_snapshot,
        id,
        &source_guard,
        &source_snapshot,
    )?;
    assert_tree(&source_guard, &tree)?;
    cancellation::check()?;

    // Cancellation is checked before this irreversible phase. Once removal
    // starts, finish it rather than intentionally leave a half-deleted bottle.
    let removal = remove_contents(&source_guard, &tree)
        .and_then(|()| finish_root_removal(&root_guard, &tree));
    preserve_identity_after_failure(&root_guard, &source_guard, &tree, &bottle, removal)?;
    Ok(bottle)
}

fn refuse_unverified_runtime(
    source: &Path,
    timeout: Duration,
    selected_wine: Option<&Path>,
) -> Result<(), String> {
    match fs::symlink_metadata(source.join("prefix")) {
        Ok(value) if value.is_dir() => refuse_live_wine_server(source, timeout, selected_wine),
        Ok(_) => Err("Deleted bottle prefix must be a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A late finalization failure can leave only restored bottle.json
            // and a remaining entry. There is then no Wine prefix to probe.
            // A surviving runtime claim must still fail closed; never silently
            // discard an initialized/partially initialized bottle's ownership.
            if recorded_runtime_for_prefix(source, selected_wine)?.is_some() {
                return Err("Cannot verify the runtime of a deleted bottle whose recorded prefix is missing; its remaining files were preserved".into());
            }
            Ok(())
        }
        Err(error) => Err(format!("Could not inspect deleted bottle prefix: {error}")),
    }
}

fn finish_root_removal(root: &File, tree: &Node) -> Result<(), String> {
    assert_at(root, &tree.name, tree.identity)?;
    unlink_at(root, &tree.name, true)
}

fn preserve_identity_after_failure(
    root: &File,
    source: &File,
    tree: &Node,
    bottle: &Bottle,
    result: Result<(), String>,
) -> Result<(), String> {
    let Err(error) = result else {
        return Ok(());
    };
    let recover = || -> Result<(), String> {
        assert_at(root, &tree.name, tree.identity)?;
        if Identity::metadata(&source.metadata().map_err(|_| PRESERVED)?) != tree.identity {
            return Err(PRESERVED.into());
        }
        let name = c_name(OsStr::new("bottle.json"))?;
        match stat_at(source, &name) {
            // It was not yet removed: do not overwrite even a changed entry.
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Could not inspect remaining bottle identity: {error}"
                ));
            }
        }
        let fd = unsafe {
            libc::openat(
                source.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_int,
            )
        };
        if fd < 0 {
            return Err("Could not restore remaining bottle identity safely".into());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let bytes = serde_json::to_vec_pretty(bottle).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(())
    };
    match recover() {
        Ok(()) => Err(error),
        Err(recovery) => Err(format!(
            "{error}. Remaining bottle identity recovery failed: {recovery}"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_locations(
    home: &Path,
    home_guard: &File,
    home_snapshot: &fs::Metadata,
    root: &Path,
    root_guard: &File,
    root_snapshot: &fs::Metadata,
    id: &str,
    source_guard: &File,
    source_snapshot: &fs::Metadata,
) -> Result<(), String> {
    // O_NOFOLLOW_ANY revalidates every ancestor, not just the final entry.
    for (path, guard, snapshot) in [
        (home, home_guard, home_snapshot),
        (root, root_guard, root_snapshot),
    ] {
        let (current, _) = open_stable_directory(path).map_err(|_| PRESERVED)?;
        if !same_identity(snapshot, &guard.metadata().map_err(|_| PRESERVED)?)
            || !same_identity(snapshot, &current.metadata().map_err(|_| PRESERVED)?)
        {
            return Err(PRESERVED.into());
        }
    }
    assert_at(
        home_guard,
        OsStr::new("deleted-bottles"),
        Identity::metadata(root_snapshot),
    )?;
    assert_at(
        root_guard,
        OsStr::new(id),
        Identity::metadata(source_snapshot),
    )?;
    if !same_identity(
        source_snapshot,
        &source_guard.metadata().map_err(|_| PRESERVED)?,
    ) {
        return Err(PRESERVED.into());
    }
    refuse_active_entry(home_guard, id)
}

fn refuse_active_entry(home: &File, id: &str) -> Result<(), String> {
    let name = c_name(OsStr::new("bottles"))?;
    match stat_at(home, &name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not check active-bottle storage: {error}")),
        Ok(metadata) if Identity::stat(&metadata).kind == libc::S_IFDIR => {
            let active = open_directory_at(home, &name)?;
            let id = c_name(OsStr::new(id))?;
            match stat_at(&active, &id) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("Could not check active bottle: {error}")),
                Ok(_) => Err("An active bottle has this ID. Move it to Recently Deleted before permanent deletion".into()),
            }
        }
        Ok(_) => Err("Active-bottle storage must be a real directory".into()),
    }
}

fn c_name(name: &OsStr) -> Result<CString, String> {
    if name.as_bytes().is_empty() || name == "." || name == ".." || name.as_bytes().contains(&b'/')
    {
        return Err("Invalid deleted-bottle entry name".into());
    }
    CString::new(name.as_bytes()).map_err(|_| "Invalid deleted-bottle entry name".into())
}

fn stat_at(parent: &File, name: &CString) -> Result<libc::stat, std::io::Error> {
    let mut result = MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            result.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { result.assume_init() })
}

fn assert_at(parent: &File, name: &OsStr, expected: Identity) -> Result<(), String> {
    let current = stat_at(parent, &c_name(name)?).map_err(|_| PRESERVED)?;
    if Identity::stat(&current) != expected {
        return Err(PRESERVED.into());
    }
    Ok(())
}

fn open_directory_at(parent: &File, name: &CString) -> Result<File, String> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "Could not safely inspect deleted-bottle directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn lock_at(dir: &File, timeout: Duration) -> Result<File, String> {
    let name = c_name(OsStr::new(".caskrun.lock"))?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600 as libc::c_int,
        )
    };
    if fd < 0 {
        return Err("Could not safely lock deleted bottle".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let held = file
        .metadata()
        .map_err(|_| "Could not inspect deleted-bottle lock")?;
    if !held.is_file() || held.uid() != unsafe { libc::geteuid() } || held.nlink() != 1 {
        return Err("Deleted-bottle lock must be an owned regular file".into());
    }
    let deadline = Instant::now() + timeout;
    loop {
        cancellation::check()?;
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err("Deleted bottle is busy; try again when its operation finishes".into());
            }
            Err(error) => return Err(format!("Could not lock deleted bottle: {error}")),
        }
    }
    assert_at(dir, OsStr::new(".caskrun.lock"), Identity::metadata(&held))?;
    Ok(file)
}

fn require_owned_directory(dir: &File, device: u64) -> Result<(), String> {
    let value = dir
        .metadata()
        .map_err(|_| "Could not inspect deleted-bottle directory")?;
    if !value.is_dir() || value.uid() != unsafe { libc::geteuid() } || value.dev() != device {
        return Err(
            "Deleted-bottle directory must be owned by this account and on the same filesystem"
                .into(),
        );
    }
    Ok(())
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn names(dir: &File) -> Result<Vec<OsString>, String> {
    // dup shares the directory offset. Opening '.' relative to the pinned
    // descriptor gives this scan its own offset and no pathname traversal.
    let dot = CString::new(".").unwrap();
    let scan = open_directory_at(dir, &dot)?;
    let fd = std::os::fd::IntoRawFd::into_raw_fd(scan);
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err("Could not enumerate deleted-bottle directory".into());
    }
    let stream = DirectoryStream(raw);
    let mut result = Vec::new();
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if std::io::Error::last_os_error().raw_os_error() != Some(0) {
                return Err("Could not finish enumerating deleted-bottle directory".into());
            }
            break;
        }
        let entry = unsafe { &*entry };
        let len = usize::from(entry.d_namlen);
        if len == 0 || len >= entry.d_name.len() || entry.d_name[len] != 0 {
            return Err("Invalid deleted-bottle directory entry".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(entry.d_name.as_ptr().cast::<u8>(), len) };
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let name = OsString::from_vec(bytes.to_vec());
        c_name(&name)?;
        result.push(name);
        if result.len() > MAX_ENTRIES {
            return Err("Deleted bottle exceeds the permanent-deletion entry limit".into());
        }
    }
    result.sort();
    if result.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("Deleted-bottle entries changed while enumerating".into());
    }
    Ok(result)
}

fn inspect_tree(
    dir: &File,
    name: OsString,
    device: u64,
    depth: usize,
    entries: &mut usize,
) -> Result<Node, String> {
    cancellation::check()?;
    if depth > MAX_DEPTH {
        return Err("Deleted bottle exceeds the permanent-deletion depth limit".into());
    }
    require_owned_directory(dir, device)?;
    let identity = Identity::metadata(&dir.metadata().map_err(|_| PRESERVED)?);
    let mut children = Vec::new();
    for name in names(dir)? {
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err("Deleted bottle exceeds the permanent-deletion entry limit".into());
        }
        let value = stat_at(dir, &c_name(&name)?).map_err(|_| PRESERVED)?;
        let child_identity = Identity::stat(&value);
        if child_identity.device != device {
            return Err(
                "A mounted filesystem is inside this deleted bottle; it was preserved".into(),
            );
        }
        let child = if child_identity.kind == libc::S_IFDIR {
            let held = open_directory_at(dir, &c_name(&name)?)?;
            if Identity::metadata(&held.metadata().map_err(|_| PRESERVED)?) != child_identity {
                return Err(PRESERVED.into());
            }
            inspect_tree(&held, name, device, depth + 1, entries)?
        } else {
            Node {
                name,
                identity: child_identity,
                children: Vec::new(),
            }
        };
        assert_at(dir, &child.name, child.identity)?;
        children.push(child);
    }
    let node = Node {
        name,
        identity,
        children,
    };
    assert_names(dir, &node)?;
    Ok(node)
}

fn assert_names(dir: &File, node: &Node) -> Result<(), String> {
    if Identity::metadata(&dir.metadata().map_err(|_| PRESERVED)?) != node.identity
        || names(dir)?
            != node
                .children
                .iter()
                .map(|child| child.name.clone())
                .collect::<Vec<_>>()
    {
        return Err(PRESERVED.into());
    }
    Ok(())
}

fn assert_tree(dir: &File, node: &Node) -> Result<(), String> {
    cancellation::check()?;
    assert_names(dir, node)?;
    for child in &node.children {
        assert_at(dir, &child.name, child.identity)?;
        if child.identity.kind == libc::S_IFDIR {
            let held = open_directory_at(dir, &c_name(&child.name)?)?;
            assert_tree(&held, child)?;
        }
    }
    Ok(())
}

fn remove_contents(dir: &File, node: &Node) -> Result<(), String> {
    assert_names(dir, node)?;
    // Keep identity metadata and the coordination lock until all payload files
    // are gone. An ordinary I/O failure therefore remains visible for retry.
    let ordered = node
        .children
        .iter()
        .filter(|child| child.name != "bottle.json" && child.name != ".caskrun.lock")
        .chain(
            node.children
                .iter()
                .filter(|child| child.name == "bottle.json"),
        )
        .chain(
            node.children
                .iter()
                .filter(|child| child.name == ".caskrun.lock"),
        );
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|error| {
            format!("Could not prepare deleted-bottle directory for removal: {error}")
        })?;
    for child in ordered {
        assert_at(dir, &child.name, child.identity)?;
        let is_directory = child.identity.kind == libc::S_IFDIR;
        if is_directory {
            let held = open_directory_at(dir, &c_name(&child.name)?)?;
            remove_contents(&held, child)?;
            assert_at(dir, &child.name, child.identity)?;
        }
        unlink_at(dir, &child.name, is_directory)?;
    }
    Ok(())
}

fn unlink_at(parent: &File, name: &OsStr, directory: bool) -> Result<(), String> {
    let name = c_name(name)?;
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } != 0
    {
        return Err(format!(
            "Permanent deletion did not finish; remaining files were preserved: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("deleted-purge-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn deleted(&self, name: &str) -> (Bottle, PathBuf) {
            let bottle = create(&self.0, name).unwrap();
            delete_bottle(&self.0, &bottle.id).unwrap();
            let dir = self.0.join("deleted-bottles").join(&bottle.id);
            (bottle, dir)
        }

        fn purge(&self, id: &str) -> Result<Bottle, String> {
            purge_deleted_with_timeout(&self.0, id, Duration::from_secs(2), None)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fn writable_directories(path: &Path) {
                if fs::symlink_metadata(path).is_ok_and(|value| value.is_dir()) {
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
                    for entry in fs::read_dir(path).unwrap() {
                        writable_directories(&entry.unwrap().path());
                    }
                }
            }
            writable_directories(&self.0);
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    #[test]
    fn purge_removes_only_requested_deleted_bottle_and_unlinks_host_mappings() {
        let fixture = Fixture::new();
        let active = create(&fixture.0, "Active neighbor").unwrap();
        let active_dir = bottle_dir(&fixture.0, &active.id).unwrap();
        fs::write(active_dir.join("keep"), b"active contents").unwrap();
        let (removed, dir) = fixture.deleted("Remove this fixture");
        let (neighbor, neighbor_dir) = fixture.deleted("Deleted neighbor");
        fs::write(neighbor_dir.join("keep"), b"deleted contents").unwrap();
        let host = fixture.0.join("host-folder");
        fs::create_dir(&host).unwrap();
        fs::write(host.join("keep"), b"host contents").unwrap();
        fs::set_permissions(&host, fs::Permissions::from_mode(0o500)).unwrap();
        symlink(&host, dir.join("host-mapping")).unwrap();
        symlink(host.join("keep"), dir.join("host-file")).unwrap();
        symlink("missing", dir.join("dangling-link")).unwrap();
        let readonly = dir.join("prefix/readonly");
        fs::create_dir(&readonly).unwrap();
        fs::write(readonly.join("remove"), b"fixture payload").unwrap();
        fs::set_permissions(&readonly, fs::Permissions::from_mode(0o500)).unwrap();

        assert_eq!(fixture.purge(&removed.id).unwrap().id, removed.id);
        assert!(!dir.exists());
        assert_eq!(list_deleted(&fixture.0).unwrap()[0].id, neighbor.id);
        assert_eq!(
            fs::read(active_dir.join("keep")).unwrap(),
            b"active contents"
        );
        assert_eq!(
            fs::read(neighbor_dir.join("keep")).unwrap(),
            b"deleted contents"
        );
        assert_eq!(fs::read(host.join("keep")).unwrap(), b"host contents");
        assert_eq!(fs::metadata(&host).unwrap().mode() & 0o777, 0o500);
        fs::set_permissions(&host, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn purge_refuses_active_unknown_and_invalid_ids_without_creating_storage() {
        let fixture = Fixture::new();
        let active = create(&fixture.0, "Active").unwrap();
        assert!(
            fixture
                .purge(&active.id)
                .unwrap_err()
                .contains("active bottle")
        );
        assert!(fixture.purge(&Uuid::new_v4().to_string()).is_err());
        assert!(fixture.purge("../outside").is_err());
        assert!(fixture.purge("not-an-id").is_err());
        assert!(!fixture.0.join("deleted-bottles").exists());
        assert!(bottle_dir(&fixture.0, &active.id).is_ok());
    }

    #[test]
    fn purge_refuses_active_id_even_if_a_deleted_entry_also_claims_it() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Conflicting fixture");
        let active = fixture.0.join("bottles").join(&bottle.id);
        fs::create_dir(&active).unwrap();
        fs::write(active.join("keep"), b"active conflict").unwrap();
        assert!(
            fixture
                .purge(&bottle.id)
                .unwrap_err()
                .contains("active bottle")
        );
        assert!(dir.join("bottle.json").is_file());
        assert_eq!(fs::read(active.join("keep")).unwrap(), b"active conflict");
    }

    #[test]
    fn purge_refuses_symlinked_storage_source_and_metadata() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Protected fixture");
        let root = fixture.0.join("deleted-bottles");
        let saved_root = fixture.0.join("saved-deleted");
        fs::rename(&root, &saved_root).unwrap();
        symlink(&saved_root, &root).unwrap();
        assert!(fixture.purge(&bottle.id).is_err());
        fs::remove_file(&root).unwrap();
        fs::rename(&saved_root, &root).unwrap();
        let saved_dir = root.join("saved-fixture");
        fs::rename(&dir, &saved_dir).unwrap();
        symlink(&saved_dir, &dir).unwrap();
        assert!(fixture.purge(&bottle.id).is_err());
        fs::remove_file(&dir).unwrap();
        fs::rename(&saved_dir, &dir).unwrap();
        let record = dir.join("bottle.json");
        let saved_record = dir.join("saved-bottle.json");
        fs::rename(&record, &saved_record).unwrap();
        symlink(&saved_record, &record).unwrap();
        assert!(fixture.purge(&bottle.id).is_err());
        assert!(saved_record.is_file());
    }

    #[test]
    fn purge_refuses_a_symlinked_storage_ancestor() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Ancestor fixture");
        let alias = fixture.0.join("home-alias");
        symlink(&fixture.0, &alias).unwrap();
        assert!(
            purge_deleted_with_timeout(&alias, &bottle.id, Duration::from_secs(1), None).is_err()
        );
        assert!(dir.join("bottle.json").is_file());
    }

    #[test]
    fn purge_refuses_live_tracked_process_and_preserves_all_files() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Tracked owner");
        fs::write(dir.join("keep"), b"payload").unwrap();
        let child = OwnedChild(Command::new("/bin/sleep").arg("10").spawn().unwrap());
        save_json(&dir.join("active-processes.json"), &vec![child.0.id()]).unwrap();
        assert!(
            fixture
                .purge(&bottle.id)
                .unwrap_err()
                .contains("Close Windows apps")
        );
        assert_eq!(fs::read(dir.join("keep")).unwrap(), b"payload");
        assert!(process_alive(child.0.id()).unwrap());
    }

    #[test]
    fn purge_refuses_real_untracked_open_handle_without_signalling_it() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Untracked owner");
        let held = dir.join("held.log");
        fs::write(&held, b"payload").unwrap();
        let mut observer = OwnedChild(
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
        std::io::BufReader::new(observer.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        assert!(
            purge_deleted_with_timeout(&fixture.0, &bottle.id, Duration::from_millis(300), None)
                .is_err()
        );
        assert!(observer.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&held).unwrap(), b"payload");
        drop(observer.0.stdin.take());
        observer.0.wait().unwrap();
        assert!(fixture.purge(&bottle.id).is_ok());
    }

    #[test]
    fn purge_refuses_unverifiable_recorded_runtime() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Unknown runtime");
        save_json(
            &dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: fixture.0.join("missing/wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        assert!(fixture.purge(&bottle.id).is_err());
        assert!(dir.join("bottle.json").is_file());
        assert!(dir.join("prefix-record.json").is_file());
    }

    #[test]
    fn purge_refuses_a_held_or_symlinked_coordination_lock() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Busy lock");
        let held = lock_bottle_directory(&dir).unwrap();
        assert!(
            purge_deleted_with_timeout(&fixture.0, &bottle.id, Duration::from_millis(50), None)
                .unwrap_err()
                .contains("busy")
        );
        drop(held);
        let lock = dir.join(".caskrun.lock");
        fs::remove_file(&lock).unwrap();
        let outside = fixture.0.join("outside-lock");
        fs::write(&outside, b"keep").unwrap();
        symlink(&outside, &lock).unwrap();
        assert!(fixture.purge(&bottle.id).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"keep");
        assert!(dir.join("bottle.json").is_file());
    }

    #[test]
    fn descriptor_tree_refuses_replaced_child_before_removal() {
        let fixture = Fixture::new();
        let (_, dir) = fixture.deleted("Child race");
        let child = dir.join("prefix/child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("keep"), b"owned payload").unwrap();
        let (guard, snapshot) = open_stable_directory(&dir).unwrap();
        let tree = inspect_tree(
            &guard,
            dir.file_name().unwrap().to_owned(),
            snapshot.dev(),
            0,
            &mut 0,
        )
        .unwrap();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"host payload").unwrap();
        let moved = dir.join("prefix/child.saved");
        fs::rename(&child, &moved).unwrap();
        symlink(&outside, &child).unwrap();
        assert!(assert_tree(&guard, &tree).is_err());
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"host payload");
        assert_eq!(fs::read(moved.join("keep")).unwrap(), b"owned payload");
    }

    #[test]
    fn descriptor_tree_refuses_replaced_root_and_added_entry() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Root race");
        let (home_guard, home_snapshot) = open_stable_directory(&fixture.0).unwrap();
        let root = fixture.0.join("deleted-bottles");
        let (root_guard, root_snapshot) = open_stable_directory(&root).unwrap();
        let (guard, snapshot) = open_stable_directory(&dir).unwrap();
        let tree = inspect_tree(
            &guard,
            dir.file_name().unwrap().to_owned(),
            snapshot.dev(),
            0,
            &mut 0,
        )
        .unwrap();
        fs::write(dir.join("added"), b"preserve").unwrap();
        assert!(assert_tree(&guard, &tree).is_err());
        let moved = root.join("saved-root");
        fs::rename(&dir, &moved).unwrap();
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("keep"), b"replacement").unwrap();
        assert!(
            assert_locations(
                &fixture.0,
                &home_guard,
                &home_snapshot,
                &root,
                &root_guard,
                &root_snapshot,
                &bottle.id,
                &guard,
                &snapshot
            )
            .is_err()
        );
        assert_eq!(fs::read(dir.join("keep")).unwrap(), b"replacement");
        assert!(moved.join("bottle.json").is_file());
    }

    #[test]
    fn late_root_removal_failure_restores_identity_and_allows_a_visible_retry() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Retry after finalization failure");
        fs::write(dir.join("payload"), b"requested removal").unwrap();
        let root = fixture.0.join("deleted-bottles");
        let (root_guard, _) = open_stable_directory(&root).unwrap();
        let (guard, snapshot) = open_stable_directory(&dir).unwrap();
        let _lock = lock_at(&guard, Duration::from_secs(1)).unwrap();
        let tree = inspect_tree(
            &guard,
            dir.file_name().unwrap().to_owned(),
            snapshot.dev(),
            0,
            &mut 0,
        )
        .unwrap();
        remove_contents(&guard, &tree).unwrap();
        assert!(!dir.join("bottle.json").exists());
        fs::write(dir.join("late-entry"), b"preserve remaining entry").unwrap();
        let error = finish_root_removal(&root_guard, &tree).unwrap_err();
        assert!(
            preserve_identity_after_failure(&root_guard, &guard, &tree, &bottle, Err(error))
                .is_err()
        );
        assert_eq!(list_deleted(&fixture.0).unwrap()[0].id, bottle.id);
        assert_eq!(
            fs::read(dir.join("late-entry")).unwrap(),
            b"preserve remaining entry"
        );
        drop(_lock);
        assert!(fixture.purge(&bottle.id).is_ok());
        assert!(!dir.exists());
    }

    #[test]
    fn missing_prefix_keeps_a_surviving_runtime_claim_fail_closed() {
        let fixture = Fixture::new();
        let (bottle, dir) = fixture.deleted("Partial removal with runtime claim");
        fs::remove_dir(dir.join("prefix")).unwrap();
        save_json(
            &dir.join("prefix-record.json"),
            &PrefixRecord {
                wine_path: fixture.0.join("missing/wine").display().to_string(),
                runtime_identity: None,
                runtime_fingerprint: None,
            },
        )
        .unwrap();
        assert!(
            fixture
                .purge(&bottle.id)
                .unwrap_err()
                .contains("recorded prefix is missing")
        );
        assert!(dir.join("bottle.json").is_file());
        assert!(dir.join("prefix-record.json").is_file());
    }
}
