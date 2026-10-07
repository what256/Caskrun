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
