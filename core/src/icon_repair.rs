//! Explicit repair of disposable, Caskrun-generated icon caches. Publisher
//! executables, installed apps, preferences and the selected engine are never
//! changed. A later ordinary lookup/launch regenerates artwork and aliases.

use crate::*;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;

const MAX_ENTRIES: usize = 8192;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const AT_REMOVEDIR: i32 = 0x80;
const QUARANTINE: &str = "icon-repair-cache";
const RECEIPT: &str = "claims.json";
const RECEIPT_LIMIT: u64 = 4 * 1024 * 1024;

unsafe extern "C" {
    fn geteuid() -> u32;
    fn unlinkat(fd: i32, name: *const c_char, flags: i32) -> i32;
}

#[derive(Debug, Serialize)]
pub(crate) struct IconRepairResult {
    bottle_id: String,
    removed_native_apps: usize,
    removed_artwork_entries: usize,
    message: String,
}

struct Node {
    path: PathBuf,
    guard: File,
    snapshot: fs::Metadata,
    children: Vec<Node>,
}

struct Cache {
    name: &'static str,
    node: Node,
    native_apps: usize,
    artwork_entries: usize,
}

struct Budget {
    entries: usize,
    bytes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaimReceipt {
    version: u32,
    bottle_id: String,
    entries: Vec<ClaimEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaimEntry {
    path: String,
    directory: bool,
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

impl ClaimEntry {
    fn record(node: &Node, path: &Path, entries: &mut Vec<Self>) -> Result<(), String> {
        let metadata = &node.snapshot;
        entries.push(Self {
            path: path
                .to_str()
                .ok_or("Generated icon path is not UTF-8")?
                .to_owned(),
            directory: metadata.file_type().is_dir(),
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            length: metadata.len(),
            mtime: metadata.mtime(),
            mtime_ns: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_ns: metadata.ctime_nsec(),
        });
        for child in &node.children {
            Self::record(child, &path.join(child.path.file_name().unwrap()), entries)?;
        }
        Ok(())
    }

    fn matches(&self, metadata: &fs::Metadata) -> bool {
        self.directory == metadata.file_type().is_dir()
            && self.device == metadata.dev() && self.inode == metadata.ino() && self.mode == metadata.mode()
            // Claiming/deleting a directory changes its own timestamps and
            // size. Surviving files must retain the complete saved snapshot.
            && (self.directory || (self.length == metadata.len() && self.mtime == metadata.mtime()
                && self.mtime_ns == metadata.mtime_nsec() && self.ctime == metadata.ctime()
                && self.ctime_ns == metadata.ctime_nsec()))
    }
}

pub(crate) fn repair_bottle_icons(home: &Path, id: &str) -> Result<IconRepairResult, String> {
    repair_with_idle_check(home, id, |dir| {
        prefix_processes::wait_until_idle(dir, IDLE_TIMEOUT)
            .map_err(|_| "Close this bottle's Windows apps before repairing its icons. Caskrun could not prove that its generated files are idle.".into())
    })
}

fn repair_with_idle_check(
    home: &Path,
    id: &str,
    mut independent_idle: impl FnMut(&Path) -> Result<(), String>,
) -> Result<IconRepairResult, String> {
    let _lock = lock_bottle(home, id)?;
    let dir = bottle_dir(home, id)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let (bottle_guard, bottle_snapshot) = open_stable_directory(&dir)?;
    check_owned(&bottle_snapshot, true)?;
    if has_tracked_apps(&dir)? {
        return Err("Close this bottle's Windows apps before repairing its icons.".into());
    }
    independent_idle(&dir)?;
    let recovered = recover_previous_claims(&dir, id, &bottle_guard, &mut independent_idle)?;
    let mut budget = Budget {
        entries: 0,
        bytes: 0,
    };
    let mut caches = Vec::new();
    for name in ["native-apps", "artwork"] {
        let path = dir.join(name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        let node = scan(&path, 0, &mut budget)?;
        if !node.snapshot.file_type().is_dir() {
            return Err(
                "Generated icon cache root must be a real directory; it was preserved.".into(),
            );
        }
        let (native_apps, artwork_entries) = if name == "native-apps" {
            (validate_native_cache(&node, id)?, 0)
        } else {
            (0, validate_artwork_cache(&node)?)
        };
        caches.push(Cache {
            name,
            node,
            native_apps,
            artwork_entries,
        });
    }
    // The complete plan is checked before the first mutation. This second
    // independent inspection also catches helpers that started while scanning.
    independent_idle(&dir)?;
    if has_tracked_apps(&dir)? {
        return Err(
            "A Windows app opened while preparing icon repair. Close it and try again.".into(),
        );
    }
    bottle_control::assert_same_prefix(&dir, &bottle_guard, &bottle_snapshot)?;
    for cache in &caches {
        assert_tree(&cache.node, &cache.node.path)?;
    }

    // Claims stay in one separately excluded cache directory. An interruption
    // cannot make generated native executables part of a portable backup.
    // Record exact inode snapshots before the first claim, so a later retry
    // can remove surviving entries without accepting added or replaced files.
    let quarantine = dir.join(QUARANTINE);
    let targets: Vec<OsString> = caches
        .iter()
        .map(|cache| OsString::from(format!("{}-{}", Uuid::new_v4(), cache.name)))
        .collect();
    let mut receipt = ClaimReceipt {
        version: 1,
        bottle_id: id.to_owned(),
        entries: Vec::new(),
    };
    for (cache, target) in caches.iter().zip(&targets) {
        ClaimEntry::record(&cache.node, Path::new(target), &mut receipt.entries)?;
    }
    let bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > RECEIPT_LIMIT {
        return Err("Icon repair recovery receipt exceeds the safety limit.".into());
    }
    let quarantine_guard = if caches.is_empty() {
        None
    } else {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&quarantine)
            .map_err(|e| e.to_string())?;
        Some(open_stable_directory(&quarantine)?.0)
    };
    if let Some(guard) = &quarantine_guard {
        let setup = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(O_NOFOLLOW_ANY)
                .open(quarantine.join(RECEIPT))
                .map_err(|e| e.to_string())?;
            use std::io::Write;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            guard.sync_all().map_err(|e| e.to_string())
        })();
        if let Err(error) = setup {
            clear_empty_quarantine(&dir, &bottle_guard, guard)
                .map_err(|cleanup| format!("Could not prepare icon repair: {error}. Its empty recovery cache could not be removed: {cleanup}"))?;
            return Err(error);
        }
    }
    // Roll back earlier claims if a later root cannot be claimed.
    let mut claimed: Vec<(&Cache, OsString)> = Vec::new();
    for (cache, target) in caches.iter().zip(targets) {
        let guard = quarantine_guard.as_ref().unwrap();
        if let Err(error) = rename_exclusive(&bottle_guard, OsStr::new(cache.name), guard, &target)
        {
            for (previous, name) in claimed.iter().rev() {
                rename_exclusive(guard, name, &bottle_guard, OsStr::new(previous.name))
                    .map_err(|rollback| format!("Icon repair could not claim a cache: {error}. Its earlier cache could not be restored: {rollback}"))?;
            }
            clear_empty_quarantine(&dir, &bottle_guard, guard)?;
            return Err(error);
        }
        claimed.push((cache, target));
    }
    let validation = (|| {
        bottle_control::assert_same_prefix(&dir, &bottle_guard, &bottle_snapshot)?;
        for (cache, name) in &claimed {
            assert_tree(&cache.node, &quarantine.join(name))?;
        }
        independent_idle(&dir)?;
        Ok::<(), String>(())
    })();
    if let Err(error) = validation {
        for (cache, name) in claimed.iter().rev() {
            rename_exclusive(
                quarantine_guard.as_ref().unwrap(),
                name,
                &bottle_guard,
                OsStr::new(cache.name),
            )
            .map_err(|rollback| {
                format!("Icon repair stopped: {error}. Its cache could not be restored: {rollback}")
            })?;
        }
        if let Some(guard) = &quarantine_guard {
            clear_empty_quarantine(&dir, &bottle_guard, guard)?;
        }
        return Err(error);
    }
    for (index, (cache, name)) in claimed.iter().enumerate() {
        let guard = quarantine_guard.as_ref().unwrap();
        if let Err(error) = remove_claimed(&cache.node, guard, name, &quarantine.join(name)) {
            let mut restoration_errors = Vec::new();
            for (later, later_name) in claimed[index + 1..].iter().rev() {
                if let Err(restore) =
                    rename_exclusive(guard, later_name, &bottle_guard, OsStr::new(later.name))
                {
                    restoration_errors.push(restore);
                }
            }
            return Err(format!(
                "Icon repair stopped: {error}. Its remaining generated files are preserved in {} and excluded from portable backups. Retry Rebuild Icons to clean only the recorded files. {}",
                quarantine.display(),
                restoration_errors.join(" ")
            ));
        }
    }
    if let Some(guard) = &quarantine_guard {
        clear_empty_quarantine(&dir, &bottle_guard, guard)?;
    }
    bottle_control::assert_same_prefix(&dir, &bottle_guard, &bottle_snapshot)?;
    let removed_native_apps = caches.iter().map(|cache| cache.native_apps).sum();
    let removed_artwork_entries = caches.iter().map(|cache| cache.artwork_entries).sum();
    Ok(IconRepairResult {
        bottle_id: id.to_owned(),
        removed_native_apps,
        removed_artwork_entries,
        message: if caches.is_empty() && !recovered {
            "No generated icons needed repair.".into()
        } else {
            "Generated icons cleared. App artwork refreshes automatically; Dock icons rebuild the next time an app opens.".into()
        },
    })
}

fn check_owned(metadata: &fs::Metadata, directory: bool) -> Result<(), String> {
    if metadata.uid() != unsafe { geteuid() }
        || metadata.mode() & 0o022 != 0
        || (directory && (!metadata.file_type().is_dir() || metadata.mode() & 0o700 != 0o700))
        || (!directory && (!metadata.file_type().is_file() || metadata.nlink() != 1))
    {
        return Err("Icon repair requires exclusively owned real cache files and readable, writable, searchable directories; links, shared files and read-only caches are preserved.".into());
    }
    Ok(())
}

fn has_tracked_apps(dir: &Path) -> Result<bool, String> {
    // This maintenance action reads process records without pruning them.
    // Even stale tracking metadata remains outside the disposable cache plan.
    for pid in read_active_processes(dir)? {
        if process_alive(pid)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn scan(path: &Path, depth: usize, budget: &mut Budget) -> Result<Node, String> {
    budget.entries += 1;
    if depth > 6 || budget.entries > MAX_ENTRIES {
        return Err("Icon cache exceeds the repair safety limit.".into());
    }
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let directory = before.file_type().is_dir();
    check_owned(&before, directory)?;
    let (guard, snapshot) = if directory {
        open_stable_directory(path)?
    } else {
        budget.bytes = budget
            .bytes
            .checked_add(before.len())
            .ok_or("Icon cache size overflow")?;
        if before.len() > 16 * 1024 * 1024 || budget.bytes > MAX_BYTES {
            return Err("Icon cache exceeds the repair safety limit.".into());
        }
        let guard = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | 0x4)
            .open(path)
            .map_err(|e| e.to_string())?;
        if !same_snapshot(&before, &guard.metadata().map_err(|e| e.to_string())?) {
            return Err("Icon cache changed while opening it.".into());
        }
        (guard, before)
    };
    let mut children = Vec::new();
    if directory {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            children.push(scan(&entry.path(), depth + 1, budget)?);
        }
        children.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    }
    let node = Node {
        path: path.to_owned(),
        guard,
        snapshot,
        children,
    };
    assert_tree(&node, path)?;
    Ok(node)
}

fn names(node: &Node) -> Result<Vec<&str>, String> {
    node.children
        .iter()
        .map(|child| {
            child
                .path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| "Icon cache contains an unrecognized filename.".into())
        })
        .collect()
}

fn digest(name: &str, length: usize) -> bool {
    name.len() == length
        && name
            .bytes()
            .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f'))
}

fn uuid_suffix(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(suffix))
        .is_some_and(|s| Uuid::parse_str(s).is_ok_and(|id| id.hyphenated().to_string() == s))
}

fn is_friendly_app(name: &str) -> bool {
    name.strip_suffix(".app").is_some_and(|stem| {
        !stem.is_empty()
            && stem.chars().count() <= 120
            && !stem.starts_with('.')
            && !stem.ends_with(['.', ' '])
            && !stem
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
    })
}

fn validate_native_cache(root: &Node, id: &str) -> Result<usize, String> {
    let mut count = 0;
    for child in &root.children {
        let name = child
            .path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("Unrecognized native cache entry")?;
        if digest(name, 64) && child.snapshot.file_type().is_dir() {
            for app in &child.children {
                let app_name = app
                    .path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .ok_or("Unrecognized generated app name")?;
                let temporary = uuid_suffix(app_name, ".native-app-", "");
                if !temporary && !is_friendly_app(app_name) {
                    return Err(
                        "Native icon cache contains an unrecognized entry; it was preserved."
                            .into(),
                    );
                }
                validate_bundle(app, id, Some(&name[..32]), temporary)?;
                if !temporary {
                    count += 1;
                }
            }
        } else if is_friendly_app(name)
            && name
                .strip_suffix(".app")
                .and_then(|stem| stem.rsplit_once('-'))
                .is_some_and(|(_, key)| digest(key, 16))
        {
            validate_bundle(child, id, None, false)?;
            count += 1;
        } else {
            return Err(
                "Native icon cache contains an unrecognized entry; it was preserved.".into(),
            );
        }
    }
    Ok(count)
}

fn validate_bundle(app: &Node, id: &str, key: Option<&str>, temporary: bool) -> Result<(), String> {
    if !app.snapshot.file_type().is_dir() || names(app)?.iter().any(|name| *name != "Contents") {
        return Err("Generated app has an unrecognized layout; it was preserved.".into());
    }
    let Some(contents) = app.children.first() else {
        return if temporary {
            Ok(())
        } else {
            Err("Generated app is missing its identity; it was preserved.".into())
        };
    };
    if !contents.snapshot.file_type().is_dir()
        || names(contents)?
            .iter()
            .any(|name| !["Info.plist", "MacOS", "Resources"].contains(name))
    {
        return Err("Generated app has an unrecognized contents layout; it was preserved.".into());
    }
    let mut identified = false;
    for child in &contents.children {
        match child.path.file_name().and_then(OsStr::to_str) {
            Some("Info.plist") if child.snapshot.file_type().is_file() => {
                if child.snapshot.len() > 16 * 1024 {
                    return Err("Generated app identity exceeds the repair safety limit.".into());
                }
                let mut bytes = Vec::new();
                (&child.guard)
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                let plist = std::str::from_utf8(&bytes)
                    .map_err(|_| "Generated app identity is invalid; it was preserved.")?;
                let marker = format!(
                    "<key>CFBundleIdentifier</key><string>dev.caskrun.windows.{}.",
                    id.replace('-', "")
                );
                let saved_key = plist
                    .split_once(&marker)
                    .and_then(|(_, rest)| rest.split_once("</string>"))
                    .map(|(key, _)| key);
                identified = saved_key.is_some_and(|saved| {
                    key.map_or_else(
                        || digest(saved, 16) || digest(saved, 32),
                        |expected| saved == expected,
                    )
                }) && plist
                    .contains("<key>CFBundleExecutable</key><string>WindowsApp</string>")
                    && plist
                        .contains("<key>NSPrincipalClass</key><string>WineApplication</string>");
                if !identified {
                    return Err(
                        "Generated app identity does not belong to this bottle; it was preserved."
                            .into(),
                    );
                }
            }
            Some("MacOS") if child.snapshot.file_type().is_dir() => {
                if names(child)?.iter().any(|name| *name != "WindowsApp")
                    || child.children.iter().any(|file| {
                        !file.snapshot.file_type().is_file() || file.snapshot.len() > 1024 * 1024
                    })
                {
                    return Err(
                        "Generated app loader layout is unrecognized; it was preserved.".into(),
                    );
                }
            }
            Some("Resources") if child.snapshot.file_type().is_dir() => {
                if names(child)?.iter().any(|name| {
                    ![
                        "app.ico",
                        "app.icns",
                        "app.png",
                        "app-source.png",
                        "native-icon-mode",
                        "native-icon.sha256",
                    ]
                    .contains(name)
                }) || child
                    .children
                    .iter()
                    .any(|file| !file.snapshot.file_type().is_file())
                {
                    return Err(
                        "Generated app artwork contains an unrecognized entry; it was preserved."
                            .into(),
                    );
                }
            }
            _ => {
                return Err(
                    "Generated app contains an unrecognized entry; it was preserved.".into(),
                );
            }
        }
    }
    if !temporary && !identified {
        return Err("Generated app is missing its identity; it was preserved.".into());
    }
    Ok(())
}

fn validate_artwork_cache(root: &Node) -> Result<usize, String> {
    let mut count = 0;
    for child in &root.children {
        let name = child
            .path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("Unrecognized artwork entry")?;
        if name == "native" && child.snapshot.file_type().is_dir() {
            for artwork in &child.children {
                let name = artwork
                    .path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .ok_or("Unrecognized artwork key")?;
                if (!digest(name, 64) && !uuid_suffix(name, ".artwork-", ""))
                    || !artwork.snapshot.file_type().is_dir()
                    || names(artwork)?.iter().any(|name| {
                        !["app.ico", "app.icns", "app.png", "artwork.sha256"].contains(name)
                    })
                    || artwork
                        .children
                        .iter()
                        .any(|file| !file.snapshot.file_type().is_file())
                {
                    return Err(
                        "Artwork cache contains an unrecognized entry; it was preserved.".into(),
                    );
                }
                count += 1;
            }
        } else if child.snapshot.file_type().is_file()
            && (uuid_suffix(name, "", ".ico") || uuid_suffix(name, ".", ".tmp"))
        {
            count += 1;
        } else {
            return Err("Artwork cache contains an unrecognized entry; it was preserved.".into());
        }
    }
    Ok(count)
}

fn assert_tree(node: &Node, path: &Path) -> Result<(), String> {
    let now = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let held = node.guard.metadata().map_err(|e| e.to_string())?;
    check_owned(&now, node.snapshot.file_type().is_dir())?;
    // A root claim changes ctime; use inode/mode/mtime/size for directories,
    // while retained file snapshots must remain exact.
    let stable = if node.snapshot.file_type().is_dir() {
        [now.dev(), held.dev()]
            .iter()
            .all(|dev| *dev == node.snapshot.dev())
            && [now.ino(), held.ino()]
                .iter()
                .all(|ino| *ino == node.snapshot.ino())
            && now.mode() == node.snapshot.mode()
            && now.mtime() == node.snapshot.mtime()
            && now.mtime_nsec() == node.snapshot.mtime_nsec()
    } else {
        same_snapshot(&node.snapshot, &now) && same_snapshot(&node.snapshot, &held)
    };
    if !stable {
        return Err("Icon cache changed while preparing repair; it was preserved.".into());
    }
    if node.snapshot.file_type().is_dir() {
        let mut actual = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        actual.sort();
        let expected = node
            .children
            .iter()
            .map(|child| child.path.file_name().unwrap().to_owned())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(
                "Icon cache entries changed while preparing repair; they were preserved.".into(),
            );
        }
        for child in &node.children {
            assert_tree(child, &path.join(child.path.file_name().unwrap()))?;
        }
    }
    Ok(())
}

fn rename_exclusive(
    from_parent: &File,
    from: &OsStr,
    to_parent: &File,
    to: &OsStr,
) -> Result<(), String> {
    let from = CString::new(from.as_bytes()).map_err(|e| e.to_string())?;
    let to = CString::new(to.as_bytes()).map_err(|e| e.to_string())?;
    if unsafe {
        renameatx_np(
            from_parent.as_raw_fd(),
            from.as_ptr(),
            to_parent.as_raw_fd(),
            to.as_ptr(),
            RENAME_EXCL | RENAME_NOFOLLOW_ANY,
        )
    } != 0
    {
        return Err(format!(
            "Could not exclusively claim generated icon cache: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn clear_empty_quarantine(dir: &Path, bottle: &File, guard: &File) -> Result<(), String> {
    let path = dir.join(QUARANTINE);
    let now = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    let held = guard.metadata().map_err(|e| e.to_string())?;
    check_owned(&now, true)?;
    if !same_identity(&now, &held) {
        return Err("Icon repair recovery directory changed; it was preserved.".into());
    }
    let names = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if names.len() > 1 || names.first().is_some_and(|name| name != RECEIPT) {
        return Err(format!(
            "Icon repair recovery files remain in {}; they were preserved for a safe retry.",
            path.display()
        ));
    }
    if !names.is_empty() {
        let receipt = fs::symlink_metadata(path.join(RECEIPT)).map_err(|e| e.to_string())?;
        check_owned(&receipt, false)?;
        let name = CString::new(RECEIPT).unwrap();
        if unsafe { unlinkat(guard.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    let name = CString::new(QUARANTINE).unwrap();
    if unsafe { unlinkat(bottle.as_raw_fd(), name.as_ptr(), AT_REMOVEDIR) } != 0 {
        return Err(format!(
            "Could not remove empty icon repair recovery directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn recover_previous_claims(
    dir: &Path,
    id: &str,
    bottle: &File,
    independent_idle: &mut impl FnMut(&Path) -> Result<(), String>,
) -> Result<bool, String> {
    let quarantine = dir.join(QUARANTINE);
    match fs::symlink_metadata(&quarantine) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => check_owned(&metadata, true)?,
    }
    let (guard, _) = open_stable_directory(&quarantine)?;
    if fs::read_dir(&quarantine)
        .map_err(|e| e.to_string())?
        .next()
        .is_none()
    {
        clear_empty_quarantine(dir, bottle, &guard)?;
        return Ok(false);
    }
    let receipt_path = quarantine.join(RECEIPT);
    let before = fs::symlink_metadata(&receipt_path)
        .map_err(|_| "Icon repair recovery files have no receipt; they were preserved.")?;
    check_owned(&before, false)?;
    if before.len() > RECEIPT_LIMIT {
        return Err("Icon repair recovery receipt exceeds the safety limit.".into());
    }
    let mut receipt_file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(&receipt_path)
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut receipt_file)
        .take(RECEIPT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > RECEIPT_LIMIT
        || !same_snapshot(
            &before,
            &receipt_file.metadata().map_err(|e| e.to_string())?,
        )
    {
        return Err("Icon repair recovery receipt changed; files were preserved.".into());
    }
    let receipt: ClaimReceipt = serde_json::from_slice(&bytes)
        .map_err(|_| "Icon repair recovery receipt is invalid; files were preserved.")?;
    if receipt.version != 1 || receipt.bottle_id != id || receipt.entries.len() > MAX_ENTRIES {
        return Err(
            "Icon repair recovery receipt does not belong to this bottle; files were preserved."
                .into(),
        );
    }
    let mut expected = BTreeMap::new();
    for entry in &receipt.entries {
        let path = Path::new(&entry.path);
        let components = path.components().collect::<Vec<_>>();
        let root = components.first().and_then(|part| {
            if let Component::Normal(name) = part {
                name.to_str()
            } else {
                None
            }
        });
        if entry.path.len() > 1024
            || components.len() > 7
            || components
                .iter()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !root.is_some_and(|name| {
                uuid_suffix(name, "", "-native-apps") || uuid_suffix(name, "", "-artwork")
            })
            || expected.insert(entry.path.as_str(), entry).is_some()
        {
            return Err(
                "Icon repair recovery receipt has unrecognized paths; files were preserved.".into(),
            );
        }
    }
    let mut nodes = Vec::new();
    let mut budget = Budget {
        entries: 0,
        bytes: 0,
    };
    for entry in fs::read_dir(&quarantine).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == RECEIPT {
            continue;
        }
        let node = scan(&entry.path(), 0, &mut budget)?;
        validate_recovery_node(&node, Path::new(&entry.file_name()), &expected)?;
        nodes.push(node);
    }
    independent_idle(dir)?;
    if !same_snapshot(
        &before,
        &fs::symlink_metadata(&receipt_path).map_err(|e| e.to_string())?,
    ) {
        return Err("Icon repair recovery receipt changed; files were preserved.".into());
    }
    for node in &nodes {
        remove_claimed(node, &guard, node.path.file_name().unwrap(), &node.path)
            .map_err(|error| format!("Previous icon repair files remain in {}: {error}. Retry when the cache is writable; they are excluded from portable backups.", quarantine.display()))?;
    }
    clear_empty_quarantine(dir, bottle, &guard)?;
    Ok(true)
}

fn validate_recovery_node(
    node: &Node,
    path: &Path,
    expected: &BTreeMap<&str, &ClaimEntry>,
) -> Result<(), String> {
    let entry = path.to_str().and_then(|path| expected.get(path));
    if !entry.is_some_and(|entry| entry.matches(&node.snapshot)) {
        return Err("Icon repair recovery contains added or replaced files; all remaining files were preserved.".into());
    }
    for child in &node.children {
        validate_recovery_node(child, &path.join(child.path.file_name().unwrap()), expected)?;
    }
    Ok(())
}

fn remove_claimed(node: &Node, parent: &File, name: &OsStr, path: &Path) -> Result<(), String> {
    assert_tree(node, path)?;
    for child in &node.children {
        remove_claimed(
            child,
            &node.guard,
            child.path.file_name().unwrap(),
            &path.join(child.path.file_name().unwrap()),
        )?;
    }
    let name = CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
    if unsafe {
        unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if node.snapshot.file_type().is_dir() {
                AT_REMOVEDIR
            } else {
                0
            },
        )
    } != 0
    {
        return Err(format!(
            "Could not remove claimed generated icon cache: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        home: PathBuf,
        bottle: Bottle,
        dir: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("icon-repair-{}", Uuid::new_v4()));
            let bottle = create(&home, "Example").unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            Self { home, bottle, dir }
        }
        fn artwork(&self) -> PathBuf {
            let dir = self.dir.join("artwork/native").join("a".repeat(64));
            fs::create_dir_all(&dir).unwrap();
            // Corrupt generated bytes are deliberately repairable. No image
            // parser or executable runs merely to discard a derived cache.
            fs::write(dir.join("app.png"), b"corrupt image").unwrap();
            fs::write(dir.join("artwork.sha256"), b"stale receipt").unwrap();
            dir
        }
        fn alias(&self) -> PathBuf {
            let app = self
                .dir
                .join("native-apps")
                .join("b".repeat(64))
                .join("Example.app");
            fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            fs::create_dir(app.join("Contents/Resources")).unwrap();
            fs::write(app.join("Contents/MacOS/WindowsApp"), b"generated loader").unwrap();
            fs::write(app.join("Contents/Info.plist"), format!("<key>CFBundleIdentifier</key><string>dev.caskrun.windows.{}.{}</string><key>CFBundleExecutable</key><string>WindowsApp</string><key>NSPrincipalClass</key><string>WineApplication</string>", self.bottle.id.replace('-', ""), "b".repeat(32))).unwrap();
            fs::write(app.join("Contents/Resources/app.png"), b"old artwork").unwrap();
            app
        }
        fn repair(&self) -> Result<IconRepairResult, String> {
            repair_with_idle_check(&self.home, &self.bottle.id, |_| Ok(()))
        }
        fn interrupted_claim(&self) -> PathBuf {
            self.alias();
            let source = self.dir.join("native-apps");
            let mut budget = Budget {
                entries: 0,
                bytes: 0,
            };
            let node = scan(&source, 0, &mut budget).unwrap();
            validate_native_cache(&node, &self.bottle.id).unwrap();
            let name = format!("{}-native-apps", Uuid::new_v4());
            let mut receipt = ClaimReceipt {
                version: 1,
                bottle_id: self.bottle.id.clone(),
                entries: Vec::new(),
            };
            ClaimEntry::record(&node, Path::new(&name), &mut receipt.entries).unwrap();
            let quarantine = self.dir.join(QUARANTINE);
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&quarantine)
                .unwrap();
            fs::write(
                quarantine.join(RECEIPT),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            let destination = quarantine.join(name);
            fs::rename(source, &destination).unwrap();
            destination
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    #[test]
    fn repairs_corrupt_generated_artwork_and_aliases_idempotently_without_touching_user_data() {
        let fixture = Fixture::new();
        fixture.artwork();
        fixture.alias();
        let source = fixture.dir.join("prefix/publisher.exe");
        fs::write(&source, b"publisher artwork and program").unwrap();
        for name in [
            "graphics.json",
            "resolution.json",
            "prefix.json",
            "launchers.json",
        ] {
            fs::write(fixture.dir.join(name), name).unwrap();
        }
        let result = fixture.repair().unwrap();
        assert_eq!(result.removed_native_apps, 1);
        assert_eq!(result.removed_artwork_entries, 1);
        assert_eq!(fs::read(source).unwrap(), b"publisher artwork and program");
        for name in [
            "graphics.json",
            "resolution.json",
            "prefix.json",
            "launchers.json",
        ] {
            assert_eq!(fs::read(fixture.dir.join(name)).unwrap(), name.as_bytes());
        }
        let result = fixture.repair().unwrap();
        assert_eq!(result.removed_native_apps, 0);
        assert_eq!(result.removed_artwork_entries, 0);
        assert!(!fs::read_dir(&fixture.dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("icon-repair")
        }));
    }

    #[test]
    fn unknown_contents_abort_the_complete_plan_before_mutation() {
        let fixture = Fixture::new();
        let app = fixture.alias();
        let artwork = fixture.artwork();
        fs::write(artwork.join("personal.txt"), b"preserve").unwrap();
        assert!(fixture.repair().is_err());
        assert!(app.exists());
        assert!(artwork.join("personal.txt").exists());
    }

    #[test]
    fn links_at_roots_or_nested_files_are_preserved_and_refused() {
        for root in [true, false] {
            let fixture = Fixture::new();
            let artwork = fixture.artwork();
            let outside = fixture.home.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("keep"), b"keep").unwrap();
            if root {
                fs::remove_dir_all(fixture.dir.join("artwork")).unwrap();
                symlink(&outside, fixture.dir.join("artwork")).unwrap();
            } else {
                fs::remove_file(artwork.join("app.png")).unwrap();
                symlink(outside.join("keep"), artwork.join("app.png")).unwrap();
            }
            assert!(fixture.repair().is_err());
            assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
        }
    }

    #[test]
    fn shared_hardlinks_are_preserved_and_refused() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        let outside = fixture.home.join("original.png");
        fs::hard_link(artwork.join("app.png"), &outside).unwrap();
        assert!(fixture.repair().is_err());
        assert!(outside.exists());
        assert!(artwork.join("app.png").exists());
    }

    #[test]
    fn wrong_bottle_identity_is_preserved_and_refused() {
        let fixture = Fixture::new();
        let app = fixture.alias();
        fs::write(app.join("Contents/Info.plist"), "a different application").unwrap();
        assert!(fixture.repair().is_err());
        assert!(app.exists());
    }

    #[test]
    fn stale_partial_known_temporary_entries_are_repairable() {
        let fixture = Fixture::new();
        let temp = fixture
            .dir
            .join("native-apps")
            .join("c".repeat(64))
            .join(format!(".native-app-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();
        let temp = fixture
            .dir
            .join("artwork/native")
            .join(format!(".artwork-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("app.ico"), []).unwrap();
        assert!(fixture.repair().is_ok());
        assert!(!fixture.dir.join("native-apps").exists());
        assert!(!fixture.dir.join("artwork").exists());
    }

    #[test]
    fn independent_busy_check_preserves_all_caches() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        let result =
            repair_with_idle_check(&fixture.home, &fixture.bottle.id, |_| Err("busy".into()));
        assert_eq!(result.unwrap_err(), "busy");
        assert!(artwork.exists());
    }

    #[test]
    fn cache_changed_between_scan_and_claim_is_preserved() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        let mut calls = 0;
        let result = repair_with_idle_check(&fixture.home, &fixture.bottle.id, |_| {
            calls += 1;
            if calls == 2 {
                fs::write(artwork.join("app.png"), b"changed").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(artwork.join("app.png")).unwrap(), b"changed");
    }

    #[test]
    fn busy_after_claim_restores_original_cache_names() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        fixture.alias();
        let mut calls = 0;
        let result = repair_with_idle_check(&fixture.home, &fixture.bottle.id, |_| {
            calls += 1;
            if calls == 3 {
                Err("late owner".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "late owner");
        assert!(artwork.exists());
        assert!(fixture.dir.join("native-apps").exists());
    }

    #[test]
    fn bounded_scan_refuses_oversized_generated_files() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        OpenOptions::new()
            .write(true)
            .open(artwork.join("app.png"))
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        assert!(fixture.repair().is_err());
        assert!(artwork.exists());
    }

    #[test]
    fn cache_root_files_and_writable_shared_directories_are_refused() {
        let fixture = Fixture::new();
        fs::write(fixture.dir.join("artwork"), b"unrecognized root").unwrap();
        assert!(fixture.repair().is_err());
        fs::remove_file(fixture.dir.join("artwork")).unwrap();
        let artwork = fixture.artwork();
        fs::set_permissions(&artwork, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(fixture.repair().is_err());
        assert!(artwork.exists());
    }

    #[test]
    fn tracked_live_processes_are_refused_without_changing_tracking_metadata() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        let bytes = format!("[{}]", std::process::id());
        let path = fixture.dir.join("active-processes.json");
        fs::write(&path, &bytes).unwrap();
        assert!(fixture.repair().unwrap_err().contains("Close this bottle"));
        assert_eq!(fs::read(path).unwrap(), bytes.as_bytes());
        assert!(artwork.exists());
    }

    #[test]
    fn readonly_root_or_nested_cache_refuses_before_any_claim() {
        for root in [true, false] {
            let fixture = Fixture::new();
            let artwork = fixture.artwork();
            let app = fixture.alias();
            let readonly = if root {
                fixture.dir.join("artwork")
            } else {
                artwork.clone()
            };
            fs::set_permissions(&readonly, fs::Permissions::from_mode(0o500)).unwrap();
            assert!(fixture.repair().unwrap_err().contains("read-only caches"));
            assert!(artwork.exists());
            assert!(app.exists());
            assert!(!fixture.dir.join(QUARANTINE).exists());
            fs::set_permissions(readonly, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    #[test]
    fn retry_cleans_exact_remaining_claim_files_after_partial_deletion() {
        let fixture = Fixture::new();
        let claim = fixture.interrupted_claim();
        fs::remove_file(
            claim
                .join("b".repeat(64))
                .join("Example.app/Contents/Info.plist"),
        )
        .unwrap();
        let result = fixture.repair().unwrap();
        assert!(result.message.contains("Generated icons cleared"));
        assert!(!fixture.dir.join(QUARANTINE).exists());
        assert!(
            fixture
                .repair()
                .unwrap()
                .message
                .contains("No generated icons")
        );
    }

    #[test]
    fn retry_refuses_foreign_added_or_replaced_files_without_removing_any_claim() {
        for replacement in [true, false] {
            let fixture = Fixture::new();
            let claim = fixture.interrupted_claim();
            let resource = claim
                .join("b".repeat(64))
                .join("Example.app/Contents/Resources");
            let foreign = resource.join(if replacement {
                "app.png"
            } else {
                "personal.txt"
            });
            if replacement {
                fs::remove_file(&foreign).unwrap();
            }
            fs::write(&foreign, b"preserve foreign data").unwrap();
            assert!(fixture.repair().is_err());
            assert_eq!(fs::read(foreign).unwrap(), b"preserve foreign data");
            assert!(
                claim
                    .join("b".repeat(64))
                    .join("Example.app/Contents/Info.plist")
                    .exists()
            );
        }
    }

    #[test]
    fn unrecognized_quarantine_is_preserved_without_clearing_normal_caches() {
        let fixture = Fixture::new();
        let artwork = fixture.artwork();
        fs::create_dir(fixture.dir.join(QUARANTINE)).unwrap();
        fs::write(
            fixture.dir.join(QUARANTINE).join("personal.txt"),
            b"preserve",
        )
        .unwrap();
        assert!(fixture.repair().is_err());
        assert!(artwork.exists());
        assert_eq!(
            fs::read(fixture.dir.join(QUARANTINE).join("personal.txt")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    fn deletion_error_restores_untouched_later_roots_and_retains_retryable_claim() {
        let fixture = Fixture::new();
        fixture.alias();
        let artwork = fixture.artwork();
        let mut calls = 0;
        let mut changed = None;
        let result = repair_with_idle_check(&fixture.home, &fixture.bottle.id, |_| {
            calls += 1;
            if calls == 3 {
                let quarantine = fixture.dir.join(QUARANTINE);
                let root = fs::read_dir(&quarantine)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .ends_with("-native-apps")
                    })
                    .unwrap();
                let resources = root
                    .join("b".repeat(64))
                    .join("Example.app/Contents/Resources");
                let mode = fs::symlink_metadata(&resources).unwrap().mode();
                fs::set_permissions(&resources, fs::Permissions::from_mode(0o500)).unwrap();
                changed = Some((resources, mode));
            }
            Ok(())
        });
        let error = result.unwrap_err();
        assert!(error.contains("Retry Rebuild Icons"));
        assert!(error.contains("excluded from portable backups"));
        assert!(artwork.exists());
        assert!(!fixture.dir.join("native-apps").exists());
        let (path, mode) = changed.unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        fixture.repair().unwrap();
        assert!(!fixture.dir.join(QUARANTINE).exists());
        assert!(!fixture.dir.join("artwork").exists());
    }
}
