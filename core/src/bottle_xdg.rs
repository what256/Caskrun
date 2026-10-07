//! Keep Wine's desktop, menu entries, and MIME integration inside a bottle.
//!
//! Wine reads XDG_CONFIG_HOME/user-dirs.dirs when it creates shell folders.
//! Its winemenubuilder uses the same Desktop directory for generated .desktop
//! shortcuts and XDG_DATA_HOME for native menu/association files.

use super::*;
use std::io::Write;

const MAPPING_MARKER: &str = "# Caskrun private Wine desktop\n";

pub(super) struct BottleXdg {
    config: PathBuf,
    data: PathBuf,
    desktop: PathBuf,
}

impl BottleXdg {
    pub(super) fn apply(&self, command: &mut Command) {
        command
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_DATA_HOME", &self.data);
    }

    pub(super) fn desktop(&self) -> &Path {
        &self.desktop
    }
}

pub(super) fn for_bottle(dir: &Path) -> Result<BottleXdg, String> {
    let xdg = real_subdirectory(dir, "xdg")?;
    let config = real_subdirectory(&xdg, "config")?;
    let data = real_subdirectory(&xdg, "data")?;
    let desktop = real_subdirectory(&xdg, "desktop")?;

    let destination = desktop
        .to_str()
        .ok_or("Bottle path is not valid UTF-8 for Wine's Desktop mapping")?;
    if destination.contains('\n') || destination.contains('\r') {
        return Err("Bottle path cannot contain a line break for Wine's Desktop mapping".into());
    }
    let escaped = destination.replace('\\', "\\\\").replace('"', "\\\"");
    let expected = format!("{MAPPING_MARKER}XDG_DESKTOP_DIR=\"{escaped}\"\n");
    let config_file = config.join("user-dirs.dirs");
    match fs::symlink_metadata(&config_file) {
        Ok(metadata) if metadata.file_type().is_file() => {
            let saved = fs::read(&config_file).map_err(|e| e.to_string())?;
            if saved != expected.as_bytes() {
                if !saved.starts_with(MAPPING_MARKER.as_bytes()) {
                    return Err("Bottle Desktop mapping changed unexpectedly".into());
                }
                write_mapping(&config_file, &expected)?;
            }
        }
        Ok(_) => return Err("Bottle Desktop mapping must be a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_mapping(&config_file, &expected)?;
        }
        Err(error) => return Err(error.to_string()),
    }

    Ok(BottleXdg {
        config,
        data,
        desktop,
    })
}

fn write_mapping(path: &Path, contents: &str) -> Result<(), String> {
    let temp = path.with_file_name(format!(".user-dirs-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temp)
            .map_err(|e| format!("Could not create bottle Desktop mapping: {e}"))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("Could not save bottle Desktop mapping: {e}"))?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| format!("Could not update bottle Desktop mapping: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Find only Wine's standard, exact macOS Desktop mapping. Never follow it.
pub(super) fn host_desktop_links(prefix: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(home) = env::var_os("HOME") else {
        return Ok(Vec::new());
    };
    let host_desktop = PathBuf::from(home).join("Desktop");
    let users = prefix.join("drive_c/users");
    if !path_is_real_directory(&users) {
        return Ok(Vec::new());
    }
    let mut links = Vec::new();
    for entry in fs::read_dir(&users).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let link = entry.path().join("Desktop");
        if fs::symlink_metadata(&link).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && fs::read_link(&link).map_err(|e| e.to_string())? == host_desktop
        {
            links.push(link);
        }
    }
    Ok(links)
}

/// Caller must establish that Wine's server is idle and hold the bottle lock.
/// Replacing an exact symlink does not move or remove anything on the host Desktop.
pub(super) fn migrate_host_desktop_links(prefix: &Path, xdg: &BottleXdg) -> Result<(), String> {
    for link in host_desktop_links(prefix)? {
        let old_target = fs::read_link(&link).map_err(|e| e.to_string())?;
        let temporary = link.with_file_name(format!(".Caskrun-Desktop-{}", Uuid::new_v4()));
        symlink(xdg.desktop(), &temporary)
            .map_err(|e| format!("Could not prepare private Desktop link: {e}"))?;
        let result = (|| {
            if fs::read_link(&link).map_err(|e| e.to_string())? != old_target {
                return Err("Bottle Desktop changed during migration".into());
            }
            fs::rename(&temporary, &link)
                .map_err(|e| format!("Could not isolate the bottle Desktop: {e}"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
            return result;
        }
    }
    Ok(())
}

/// This macOS account's own home folder, when the environment names a real
/// absolute one. Nothing inside it is read, created or copied.
pub(super) fn account_home() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
}

/// The one policy that decides which of Wine's host-folder links a bottle copy
/// may carry. Each entry is an exact path inside a bottle account folder and the
/// home folders that path is allowed to name. The packaged runtime was observed
/// creating every entry below; `Videos` also accepts `Videos` because that is
/// the XDG default name for the same folder.
///
/// `Desktop` is absent on purpose: Caskrun replaces it with the bottle's own
/// private Desktop so Windows programs cannot write to the real one. Any other
/// path or target, including a home folder such as `Library` or `.ssh`, is
/// refused in both directions and stops the copy instead of travelling into a
/// backup or being recreated from one.
const STANDARD_USER_FOLDERS: &[(&str, &[&str])] = &[
    ("Documents", &["Documents"]),
    ("Downloads", &["Downloads"]),
    ("Music", &["Music"]),
    ("Pictures", &["Pictures"]),
    ("Videos", &["Movies", "Videos"]),
    (
        "AppData/Roaming/Microsoft/Windows/Templates",
        &["Templates"],
    ),
];

/// The home folders this exact path inside a bottle account may be mapped to.
fn allowed_host_folders(inside: &Path) -> Option<&'static [&'static str]> {
    STANDARD_USER_FOLDERS
        .iter()
        .find(|(path, _)| inside == Path::new(path))
        .map(|(_, folders)| *folders)
}

/// Wine maps several Windows user folders onto this account's macOS home folder
/// with an absolute symlink, so a copied bottle cannot keep the link verbatim.
/// Recognise one only where the standard-folder policy allows both the path
/// inside the bottle account and the home folder it names.
pub(super) fn host_folder_link(home: &Path, relative: &Path, target: &Path) -> Option<String> {
    let (_, inside) = account_folder_parts(relative)?;
    let allowed = allowed_host_folders(&inside)?;
    let mut named = target.strip_prefix(home).ok()?.components();
    let Some(Component::Normal(folder)) = named.next() else {
        return None;
    };
    if named.next().is_some() {
        return None;
    }
    let folder = folder.to_str()?;
    allowed.contains(&folder).then(|| folder.to_owned())
}

/// Split prefix/drive_c/users/<account>/<path inside the account>.
fn account_folder_parts(relative: &Path) -> Option<(&OsStr, PathBuf)> {
    let mut components = relative.components();
    for expected in ["prefix", "drive_c", "users"] {
        if components.next()?.as_os_str() != expected {
            return None;
        }
    }
    let Component::Normal(account) = components.next()? else {
        return None;
    };
    let mut inside = PathBuf::new();
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        inside.push(name);
    }
    (!inside.as_os_str().is_empty()).then_some((account, inside))
}

/// The same policy that decides what a copy may record decides what a backup may
/// restore, so a hand-edited manifest cannot introduce a mapping export refuses.
pub(super) fn check_host_folder_mapping(relative: &Path, folder: &str) -> Result<(), String> {
    let refused = || {
        Err(format!(
            "Backup user folder mapping is not a standard Windows user folder: {}",
            relative.display()
        ))
    };
    let Some((_, inside)) = account_folder_parts(relative) else {
        return refused();
    };
    match allowed_host_folders(&inside) {
        Some(allowed) if allowed.contains(&folder) => Ok(()),
        _ => refused(),
    }
}

/// Wine names each bottle profile folder after the account that created it, so
/// a bottle restored under another account would otherwise get a fresh, empty
/// profile and leave its installed programs' settings and data unreachable.
const MAX_ACCOUNT_NAME: usize = 64;

/// The macOS account name Wine will use for this bottle's Windows profile.
pub(super) fn account_name() -> Option<String> {
    let name = env::var("USER").ok()?;
    check_account_name(&name).ok()?;
    Some(name)
}

fn check_account_name(name: &str) -> Result<(), String> {
    let plain = Path::new(name)
        .components()
        .next()
        .is_some_and(|only| matches!(only, Component::Normal(part) if part == name));
    if !plain || name.len() > MAX_ACCOUNT_NAME || name == "." || name == ".." {
        return Err("Backup names an unusable Windows profile folder".into());
    }
    Ok(())
}

/// The bottle account folder this account's Windows profile uses, following only
/// a link to a sibling profile inside the same bottle. Recording it lets a
/// restore under a different account reach the same profile.
pub(super) fn profile_account(prefix: &Path, account: Option<&str>) -> Option<String> {
    let name = account?.to_owned();
    check_account_name(&name).ok()?;
    let accounts = prefix.join("drive_c/users");
    let profile = accounts.join(&name);
    if path_is_real_directory(&profile) {
        return Some(name);
    }
    // A bottle restored under this account already points at another profile.
    let target = fs::read_link(&profile).ok()?;
    let sibling = match target.components().next()? {
        Component::Normal(part) if target.components().count() == 1 => part.to_str()?.to_owned(),
        _ => return None,
    };
    check_account_name(&sibling).ok()?;
    path_is_real_directory(&accounts.join(&sibling)).then_some(sibling)
}

/// Let this account's Windows profile reach the restored one. Wine creates
/// `drive_c/users/<account>` from the account that runs it, so a bottle restored
/// under a different account needs that name to resolve to the profile the
/// backup actually holds. This adds one bottle-relative link beside it; it never
/// renames, moves, merges or removes a profile, and it leaves an existing
/// profile folder of that name untouched.
pub(super) fn link_restored_profile(
    destination: &Path,
    account: Option<&str>,
    recorded: Option<&str>,
) -> Result<bool, String> {
    let Some(recorded) = recorded else {
        return Ok(false);
    };
    check_account_name(recorded)?;
    let Some(name) = account.filter(|name| check_account_name(name).is_ok()) else {
        return Ok(false);
    };
    if name == recorded {
        return Ok(false);
    }
    let accounts = destination.join("prefix/drive_c/users");
    if !path_is_real_directory(&accounts) || !path_is_real_directory(&accounts.join(recorded)) {
        return Ok(false);
    }
    let profile = accounts.join(name);
    if fs::symlink_metadata(&profile).is_ok() {
        return Ok(false);
    }
    symlink(recorded, &profile)
        .map_err(|e| format!("Could not connect the restored Windows profile: {e}"))?;
    Ok(true)
}

#[derive(Default)]
pub(super) struct HostFolderOutcome {
    pub(super) linked: usize,
    pub(super) private: usize,
}

/// Recreate Wine's user-folder mappings in a bottle Caskrun has just copied.
/// Each link points at this account's own home folder; no host file is read,
/// created, copied or followed. A home folder this account does not have becomes
/// a real folder inside the bottle instead, so the bottle still works.
pub(super) fn restore_host_folder_links(
    home: Option<&Path>,
    destination: &Path,
    mappings: &[HostFolderMapping],
) -> Result<HostFolderOutcome, String> {
    if mappings.len() > MAX_HOST_FOLDER_LINKS {
        return Err("Backup lists too many user folder mappings".into());
    }
    let home = home.filter(|home| path_is_real_directory(home));
    // Check every mapping before creating anything, so a refused backup leaves
    // no half-restored user folders behind.
    let mut planned: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut seen: Vec<&Path> = Vec::new();
    for mapping in mappings {
        let relative = Path::new(&mapping.path);
        validate_relative_path(relative)?;
        check_host_folder_mapping(relative, &mapping.folder)?;
        if seen.contains(&relative) {
            return Err(format!(
                "Backup repeats the user folder mapping {}",
                relative.display()
            ));
        }
        seen.push(relative);
        let link = absent_bottle_path(destination, relative)?;
        let target = home
            .map(|home| home.join(&mapping.folder))
            .filter(|target| path_is_real_directory(target));
        planned.push((link, target));
    }
    let mut outcome = HostFolderOutcome::default();
    for (link, target) in planned {
        match target {
            Some(target) => {
                symlink(&target, &link)
                    .map_err(|e| format!("Could not restore a bottle user folder: {e}"))?;
                outcome.linked += 1;
            }
            None => {
                fs::create_dir(&link)
                    .map_err(|e| format!("Could not create a bottle user folder: {e}"))?;
                outcome.private += 1;
            }
        }
    }
    Ok(outcome)
}

/// Resolve a mapping inside the new bottle without crossing a symlink, and
/// refuse a path the copied payload already occupies.
fn absent_bottle_path(destination: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut path = destination.to_path_buf();
    if !path_is_real_directory(&path) {
        return Err("Restored bottle must be a real directory".into());
    }
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err("Backup user folder mapping is not a plain bottle path".into());
        };
        path.push(name);
        if components.peek().is_some() {
            if !path_is_real_directory(&path) {
                return Err(format!(
                    "Backup has no bottle folder for the user folder mapping {}",
                    relative.display()
                ));
            }
        } else if fs::symlink_metadata(&path).is_ok() {
            return Err(format!(
                "Backup already contains the user folder {}",
                relative.display()
            ));
        }
    }
    Ok(path)
}

/// Backups must not retain an absolute path to their source bottle. Convert
/// only the Desktop link produced by this exact Caskrun XDG configuration.
pub(super) fn portable_desktop_link(
    source_root: &Path,
    relative: &Path,
    target: &Path,
) -> Option<PathBuf> {
    let components: Vec<_> = relative.components().collect();
    if components.len() != 5
        || components[0].as_os_str() != "prefix"
        || components[1].as_os_str() != "drive_c"
        || components[2].as_os_str() != "users"
        || components[4].as_os_str() != "Desktop"
        || target != source_root.join("xdg/desktop")
    {
        return None;
    }
    Some(PathBuf::from("../../../../xdg/desktop"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_private_xdg_paths_without_changing_host_environment() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("caskrun-xdg-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let configured = for_bottle(&root).unwrap();
        assert_eq!(configured.desktop(), root.join("xdg/desktop"));
        assert_eq!(
            fs::read_to_string(root.join("xdg/config/user-dirs.dirs")).unwrap(),
            format!(
                "{MAPPING_MARKER}XDG_DESKTOP_DIR=\"{}\"\n",
                configured.desktop().display()
            )
        );
        let mut command = Command::new("/usr/bin/env");
        configured.apply(&mut command);
        let output = command.output().unwrap();
        let vars = String::from_utf8(output.stdout).unwrap();
        assert!(vars.contains(&format!(
            "XDG_CONFIG_HOME={}",
            root.join("xdg/config").display()
        )));
        assert!(vars.contains(&format!(
            "XDG_DATA_HOME={}",
            root.join("xdg/data").display()
        )));
        assert_eq!(for_bottle(&root).unwrap().desktop(), configured.desktop());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_replaced_mapping_or_linked_directory() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("caskrun-xdg-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for_bottle(&root).unwrap();
        let mapping = root.join("xdg/config/user-dirs.dirs");
        fs::write(&mapping, b"XDG_DESKTOP_DIR=\"/outside-bottle/Desktop\"\n").unwrap();
        assert!(for_bottle(&root).is_err());
        fs::remove_dir_all(root.join("xdg")).unwrap();
        symlink("/tmp", root.join("xdg")).unwrap();
        assert!(for_bottle(&root).is_err());
        fs::remove_file(root.join("xdg")).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moves_only_exact_wine_desktop_links_without_touching_host_files() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("caskrun-xdg-{}", Uuid::new_v4()));
        let users = root.join("prefix/drive_c/users");
        let account = users.join("example");
        let custom = users.join("custom");
        fs::create_dir_all(&account).unwrap();
        fs::create_dir(&custom).unwrap();
        let xdg = for_bottle(&root).unwrap();
        let host_desktop = PathBuf::from(env::var_os("HOME").unwrap()).join("Desktop");
        symlink(&host_desktop, account.join("Desktop")).unwrap();
        symlink("/tmp", custom.join("Desktop")).unwrap();
        assert_eq!(
            host_desktop_links(&root.join("prefix")).unwrap(),
            vec![account.join("Desktop")]
        );
        migrate_host_desktop_links(&root.join("prefix"), &xdg).unwrap();
        assert_eq!(
            fs::read_link(account.join("Desktop")).unwrap(),
            xdg.desktop()
        );
        assert_eq!(
            fs::read_link(custom.join("Desktop")).unwrap(),
            Path::new("/tmp")
        );
        assert!(host_desktop.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebases_own_mapping_after_bottle_copy() {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("caskrun-xdg-{}", Uuid::new_v4()));
        let original = parent.join("first");
        let moved = parent.join("second");
        fs::create_dir_all(&original).unwrap();
        let first = for_bottle(&original).unwrap();
        fs::create_dir_all(moved.join("xdg/config")).unwrap();
        fs::copy(
            original.join("xdg/config/user-dirs.dirs"),
            moved.join("xdg/config/user-dirs.dirs"),
        )
        .unwrap();
        let second = for_bottle(&moved).unwrap();
        assert_ne!(first.desktop(), second.desktop());
        assert!(
            fs::read_to_string(moved.join("xdg/config/user-dirs.dirs"))
                .unwrap()
                .contains(&second.desktop().display().to_string())
        );
        fs::remove_dir_all(parent).unwrap();
    }

    fn xdg_test_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("caskrun-xdg-{}", Uuid::new_v4()))
    }

    #[test]
    fn one_standard_folder_policy_governs_recording_and_restoring() {
        let home = Path::new("/example/home");
        let account = Path::new("prefix/drive_c/users/example");
        // Every mapping the packaged runtime creates is carried, including the
        // deeper Templates path and the macOS Movies name for Videos.
        for (inside, folder) in [
            ("Documents", "Documents"),
            ("Downloads", "Downloads"),
            ("Music", "Music"),
            ("Pictures", "Pictures"),
            ("Videos", "Movies"),
            ("Videos", "Videos"),
            ("AppData/Roaming/Microsoft/Windows/Templates", "Templates"),
        ] {
            let relative = account.join(inside);
            assert_eq!(
                host_folder_link(home, &relative, &home.join(folder)),
                Some(folder.to_owned()),
                "{inside} should map to {folder}"
            );
            assert!(check_host_folder_mapping(&relative, folder).is_ok());
        }
        // Export and restore refuse the same things. Desktop stays private to the
        // bottle, and no mapping may name an unrelated or sensitive home folder.
        for (inside, folder) in [
            ("Desktop", "Desktop"),
            ("Documents", "Library"),
            ("Documents", ".ssh"),
            ("Documents", "Movies"),
            ("Documents", "Documents2"),
            ("Videos", "Pictures"),
            ("AppData/Roaming", "Documents"),
            ("AppData/Roaming/Microsoft/Windows/Templates", "Documents"),
            ("Saved Games", "Documents"),
            ("Documents/Nested", "Documents"),
        ] {
            let relative = account.join(inside);
            assert_eq!(
                host_folder_link(home, &relative, &home.join(folder)),
                None,
                "{inside} must not map to {folder}"
            );
            assert!(
                check_host_folder_mapping(&relative, folder).is_err(),
                "restoring {inside} from {folder} must be refused"
            );
        }
        // The home folder itself, a deeper target, a neighbouring home folder, a
        // relative target and a path outside a bottle account stay refused.
        let documents = account.join("Documents");
        assert_eq!(host_folder_link(home, &documents, home), None);
        assert_eq!(
            host_folder_link(home, &documents, &home.join("Library/Documents")),
            None
        );
        assert_eq!(
            host_folder_link(
                home,
                &documents,
                Path::new("/example/home-elsewhere/Documents")
            ),
            None
        );
        assert_eq!(
            host_folder_link(home, &documents, Path::new("Documents")),
            None
        );
        for outside in [
            "prefix/drive_c/Documents",
            "prefix/dosdevices/z:",
            "prefix/drive_c/users/example",
            "Documents",
        ] {
            assert_eq!(
                host_folder_link(home, Path::new(outside), &home.join("Documents")),
                None
            );
            assert!(check_host_folder_mapping(Path::new(outside), "Documents").is_err());
        }
    }

    #[test]
    fn connects_a_restored_profile_only_beside_an_existing_one() {
        let root = xdg_test_root();
        let bottle = root.join("bottle");
        let accounts = bottle.join("prefix/drive_c/users");
        fs::create_dir_all(accounts.join("first/AppData/Roaming/Example")).unwrap();
        fs::write(
            accounts.join("first/AppData/Roaming/Example/settings.ini"),
            b"installed program settings",
        )
        .unwrap();

        // A bottle restored under another account reaches the saved profile.
        assert!(link_restored_profile(&bottle, Some("second"), Some("first")).unwrap());
        assert_eq!(
            fs::read_link(accounts.join("second")).unwrap(),
            Path::new("first")
        );
        assert_eq!(
            fs::read(accounts.join("second/AppData/Roaming/Example/settings.ini")).unwrap(),
            b"installed program settings"
        );
        // The saved profile keeps its own name and contents.
        assert!(path_is_real_directory(&accounts.join("first")));

        // Nothing to do for the same account, a missing record, an unknown saved
        // profile, or an account name that is not a plain folder name.
        assert!(!link_restored_profile(&bottle, Some("first"), Some("first")).unwrap());
        assert!(!link_restored_profile(&bottle, Some("third"), None).unwrap());
        assert!(!link_restored_profile(&bottle, Some("third"), Some("absent")).unwrap());
        assert!(!link_restored_profile(&bottle, None, Some("first")).unwrap());
        assert!(!link_restored_profile(&bottle, Some("../escape"), Some("first")).unwrap());
        assert!(link_restored_profile(&bottle, Some("third"), Some("../first")).is_err());
        assert!(link_restored_profile(&bottle, Some("third"), Some("a/b")).is_err());
        assert!(!accounts.join("third").exists());

        // An account that already has a profile folder is never replaced.
        fs::create_dir(accounts.join("fourth")).unwrap();
        fs::write(accounts.join("fourth/own.txt"), b"existing profile").unwrap();
        assert!(!link_restored_profile(&bottle, Some("fourth"), Some("first")).unwrap());
        assert_eq!(
            fs::read(accounts.join("fourth/own.txt")).unwrap(),
            b"existing profile"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_the_profile_folder_this_account_actually_uses() {
        let root = xdg_test_root();
        let prefix = root.join("prefix");
        let accounts = prefix.join("drive_c/users");
        fs::create_dir_all(accounts.join("first")).unwrap();
        assert_eq!(
            profile_account(&prefix, Some("first")),
            Some("first".into())
        );
        // An account whose profile already points at a sibling records that one.
        symlink("first", accounts.join("second")).unwrap();
        assert_eq!(
            profile_account(&prefix, Some("second")),
            Some("first".into())
        );
        // A profile that is absent, left outside the bottle, or named unusably
        // records nothing rather than guessing.
        assert_eq!(profile_account(&prefix, Some("third")), None);
        symlink("/tmp", accounts.join("fourth")).unwrap();
        assert_eq!(profile_account(&prefix, Some("fourth")), None);
        symlink("../../../elsewhere", accounts.join("fifth")).unwrap();
        assert_eq!(profile_account(&prefix, Some("fifth")), None);
        assert_eq!(profile_account(&prefix, None), None);
        assert_eq!(profile_account(&prefix, Some("../first")), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restores_user_folders_against_this_account_without_creating_host_folders() {
        let root = xdg_test_root();
        let home = root.join("home");
        let bottle = root.join("bottle");
        let account = bottle.join("prefix/drive_c/users/example");
        fs::create_dir_all(&account).unwrap();
        fs::create_dir_all(home.join("Documents")).unwrap();
        let mapping = |name: &str, folder: &str| HostFolderMapping {
            path: format!("prefix/drive_c/users/example/{name}"),
            folder: folder.into(),
        };
        let outcome = restore_host_folder_links(
            Some(&home),
            &bottle,
            &[
                mapping("Documents", "Documents"),
                mapping("Videos", "Movies"),
            ],
        )
        .unwrap();
        assert_eq!((outcome.linked, outcome.private), (1, 1));
        assert_eq!(
            fs::read_link(account.join("Documents")).unwrap(),
            home.join("Documents")
        );
        // A folder this account does not have becomes a real bottle folder, and
        // nothing new appears in the home folder.
        assert!(path_is_real_directory(&account.join("Videos")));
        assert!(!home.join("Movies").exists());

        // Without a usable home folder every mapping stays inside the bottle.
        let second = bottle.join("second");
        fs::create_dir_all(second.join("prefix/drive_c/users/example")).unwrap();
        let outcome =
            restore_host_folder_links(None, &second, &[mapping("Documents", "Documents")]).unwrap();
        assert_eq!((outcome.linked, outcome.private), (0, 1));
        assert!(path_is_real_directory(
            &second.join("prefix/drive_c/users/example/Documents")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_unsafe_repeated_or_occupied_user_folder_mappings() {
        let root = xdg_test_root();
        let home = root.join("home");
        let bottle = root.join("bottle");
        let account = bottle.join("prefix/drive_c/users/example");
        fs::create_dir_all(&account).unwrap();
        fs::create_dir_all(home.join("Documents")).unwrap();
        let restore = |mappings: &[HostFolderMapping]| {
            restore_host_folder_links(Some(&home), &bottle, mappings)
        };
        let mapping = |path: &str, folder: &str| HostFolderMapping {
            path: path.into(),
            folder: folder.into(),
        };
        let inside = "prefix/drive_c/users/example/Documents";
        // A folder name must stay a name, and the mapping must sit in an account.
        for refused in [
            mapping(inside, "../Documents"),
            mapping(inside, ".."),
            mapping(inside, ""),
            mapping(inside, &"n".repeat(65)),
            mapping("../escape", "Documents"),
            mapping("prefix/drive_c/escape", "Documents"),
            mapping("prefix/drive_c/users/example/AppData/Roaming", "Documents"),
            mapping("prefix/dosdevices/z:", "Documents"),
        ] {
            assert!(restore(&[refused]).is_err());
        }
        // A mapping needs its bottle folder, is never listed twice, and never
        // replaces something the copied payload already put there.
        assert!(
            restore(&[mapping(
                "prefix/drive_c/users/missing/Documents",
                "Documents"
            )])
            .is_err()
        );
        assert!(restore(&[mapping(inside, "Documents"), mapping(inside, "Documents")]).is_err());
        assert!(restore(&[mapping(inside, "Documents")]).is_ok());
        assert!(restore(&[mapping(inside, "Documents")]).is_err());
        let many: Vec<_> = (0..MAX_HOST_FOLDER_LINKS + 1)
            .map(|index| {
                mapping(
                    &format!("prefix/drive_c/users/example/F{index}"),
                    "Documents",
                )
            })
            .collect();
        assert!(restore(&many).is_err());
        // An account folder reached through a symlink is refused as well.
        let linked = bottle.join("linked");
        fs::create_dir_all(linked.join("prefix/drive_c/real")).unwrap();
        symlink("real", linked.join("prefix/drive_c/users")).unwrap();
        assert!(
            restore_host_folder_links(Some(&home), &linked, &[mapping(inside, "Documents")])
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_own_desktop_link_becomes_portable() {
        let root = Path::new("/example/bottle");
        let relative = Path::new("prefix/drive_c/users/example/Desktop");
        assert_eq!(
            portable_desktop_link(root, relative, &root.join("xdg/desktop")),
            Some(PathBuf::from("../../../../xdg/desktop"))
        );
        assert!(
            portable_desktop_link(root, relative, Path::new("/outside-bottle/Desktop")).is_none()
        );
        assert!(
            portable_desktop_link(
                root,
                Path::new("prefix/dosdevices/z:"),
                &root.join("xdg/desktop")
            )
            .is_none()
        );
    }
}
