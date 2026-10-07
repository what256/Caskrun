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

/// Wine maps several Windows user folders onto this account's macOS home folder
/// with an absolute symlink, so a copied bottle cannot keep the link verbatim.
/// Recognise only a link placed directly in a bottle account folder whose target
/// is exactly one folder inside this account's home. Caskrun keeps its own
/// private Desktop mapping instead, so Desktop is deliberately not included.
pub(super) fn host_folder_link(home: &Path, relative: &Path, target: &Path) -> Option<String> {
    let (_, windows_name) = account_folder_parts(relative)?;
    if windows_name == "Desktop" {
        return None;
    }
    let mut inside = target.strip_prefix(home).ok()?.components();
    let Some(Component::Normal(folder)) = inside.next() else {
        return None;
    };
    if inside.next().is_some() {
        return None;
    }
    let folder = folder.to_str()?;
    check_host_folder_name(folder).ok()?;
    Some(folder.to_owned())
}

/// A user-folder mapping lives at prefix/drive_c/users/<account>/<folder>.
fn account_folder_parts(relative: &Path) -> Option<(&OsStr, &OsStr)> {
    let components: Vec<_> = relative.components().collect();
    let [first, second, third, account, name] = components.as_slice() else {
        return None;
    };
    if first.as_os_str() != "prefix"
        || second.as_os_str() != "drive_c"
        || third.as_os_str() != "users"
    {
        return None;
    }
    match (account, name) {
        (Component::Normal(account), Component::Normal(name)) => Some((account, name)),
        _ => None,
    }
}

/// A restored mapping names a home folder, never a path. Reject anything else.
pub(super) fn check_host_folder_name(folder: &str) -> Result<(), String> {
    if folder.is_empty()
        || folder.len() > 64
        || folder == "."
        || folder == ".."
        || folder.contains('/')
        || folder.contains('\0')
    {
        return Err("Backup has an unusable user folder name".into());
    }
    Ok(())
}

pub(super) fn check_host_folder_path(relative: &Path) -> Result<(), String> {
    if account_folder_parts(relative).is_none() {
        return Err(format!(
            "Backup user folder mapping is not inside a bottle account: {}",
            relative.display()
        ));
    }
    Ok(())
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
        check_host_folder_path(relative)?;
        check_host_folder_name(&mapping.folder)?;
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
    fn recognises_only_wine_user_folder_links_in_a_bottle_account() {
        let home = Path::new("/example/home");
        let account = Path::new("prefix/drive_c/users/example");
        assert_eq!(
            host_folder_link(home, &account.join("Documents"), &home.join("Documents")),
            Some("Documents".into())
        );
        // macOS names some folders differently from Windows.
        assert_eq!(
            host_folder_link(home, &account.join("Videos"), &home.join("Movies")),
            Some("Movies".into())
        );
        // Caskrun keeps its own private Desktop mapping instead.
        assert_eq!(
            host_folder_link(home, &account.join("Desktop"), &home.join("Desktop")),
            None
        );
        // The home folder itself, a deeper folder, a sibling of the home folder,
        // a relative target and a path outside an account folder stay refused.
        assert_eq!(host_folder_link(home, &account.join("Home"), home), None);
        assert_eq!(
            host_folder_link(home, &account.join("Documents"), &home.join("Library/Mail")),
            None
        );
        assert_eq!(
            host_folder_link(
                home,
                &account.join("Documents"),
                Path::new("/example/home-elsewhere/Documents")
            ),
            None
        );
        assert_eq!(
            host_folder_link(home, &account.join("Documents"), Path::new("Documents")),
            None
        );
        assert_eq!(
            host_folder_link(
                home,
                Path::new("prefix/drive_c/Documents"),
                &home.join("Documents")
            ),
            None
        );
        assert_eq!(
            host_folder_link(
                home,
                &account.join("AppData/Roaming"),
                &home.join("Documents")
            ),
            None
        );
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
