//! Typed, bottle-local requests to publisher clients. The store remains
//! responsible for sign-in, ownership, installation and game launch.
//!
//! EA and Ubisoft URI formats adapted from MacadeKit's LauncherStore.swift,
//! Copyright 2026 Steale LLC, Apache-2.0; see third-party/mapc-engine/.
//! Caskrun changes: strict identifiers, current manifest revalidation and
//! checked saved clients; no shell, arbitrary URL or host store dispatch.

use crate::*;

pub(super) fn run(
    home: &Path,
    bottle_id: &str,
    store: &str,
    game_id: &str,
) -> Result<LaunchResult, String> {
    let (launcher, arguments) = request(home, bottle_id, store, game_id)?;
    run_launcher(home, bottle_id, &launcher.id, &arguments)
}

fn game_arguments(store: &str, id: &str) -> Result<Vec<String>, String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err("The store game identifier is invalid".into());
    }
    match store {
        // Existing clients handle navigation themselves. A correct Epic game
        // URI requires catalog identifiers we do not currently collect.
        publisher_library::STORE_EPIC | publisher_library::STORE_GOG => Ok(Vec::new()),
        publisher_library::STORE_EA => {
            Ok(vec![format!("link2ea://launchgame/{id}?platform=PCWIN")])
        }
        publisher_library::STORE_UBISOFT
            if id.len() <= 20
                && id.bytes().all(|b| b.is_ascii_digit())
                && id.bytes().any(|b| b != b'0') =>
        {
            Ok(vec![format!("uplay://launch/{id}/0")])
        }
        _ => Err("This store has no reviewed game-launch request".into()),
    }
}

fn checked_client(prefix: &Path, program: &Path, store: &str) -> bool {
    let Ok((_guard, _)) = open_stable_directory(prefix) else {
        return false;
    };
    let Ok(prefix) = prefix.canonicalize() else {
        return false;
    };
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(program)
    else {
        return false;
    };
    let Ok(snapshot) = file.metadata() else {
        return false;
    };
    if !snapshot.is_file() {
        return false;
    }
    let Ok(program) = program.canonicalize() else {
        return false;
    };
    if !fs::symlink_metadata(&program).is_ok_and(|m| same_snapshot(&snapshot, &m)) {
        return false;
    }
    let catalog = match store {
        publisher_library::STORE_EA => Some(publisher_install::EA_ID),
        publisher_library::STORE_UBISOFT => Some(publisher_install::UBISOFT_ID),
        _ => None,
    };
    if let Some(catalog) = catalog {
        return publisher_install::is_reviewed_client(&prefix, &program, catalog);
    }
    let drive = prefix.join("drive_c");
    let Ok(relative) = program.strip_prefix(&drive) else {
        return false;
    };
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return false;
    }
    let relative = relative.to_string_lossy().to_ascii_lowercase();
    match store {
        publisher_library::STORE_GOG => matches!(
            relative.as_str(),
            "program files/gog galaxy/galaxyclient.exe"
                | "program files (x86)/gog galaxy/galaxyclient.exe"
        ),
        publisher_library::STORE_EPIC => [
            "program files/epic games/launcher/portal/binaries/",
            "program files (x86)/epic games/launcher/portal/binaries/",
        ]
        .iter()
        .any(|root| {
            relative.strip_prefix(root).is_some_and(|tail| {
                matches!(
                    tail,
                    "win32/epicgameslauncher.exe" | "win64/epicgameslauncher.exe"
                )
            })
        }),
        _ => false,
    }
}

fn request(
    home: &Path,
    bottle_id: &str,
    store: &str,
    game_id: &str,
) -> Result<(Launcher, Vec<String>), String> {
    let arguments = game_arguments(store, game_id)?;
    let prefix = bottle_dir(home, bottle_id)?.join("prefix");
    // Treat the UI as a hint. A removed or now-uninstalled manifest cannot
    // issue a request using an old Collection item.
    if !publisher_library::discover(&prefix)?
        .games
        .iter()
        .any(|game| game.store == store && game.store_id == game_id)
    {
        return Err("This game is no longer installed in the selected bottle".into());
    }
    for candidate in list_launchers(home, bottle_id)? {
        let launcher = if store == publisher_library::STORE_EA {
            resolve_saved_launcher(home, bottle_id, &candidate.id)?
        } else {
            candidate
        };
        if launcher.saved_arguments.is_empty()
            && checked_client(&prefix, Path::new(&launcher.path), store)
        {
            return Ok((launcher, arguments));
        }
    }
    Err("Save this store's Windows client without custom arguments in this bottle before opening its games".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_requests_only_produce_fixed_operations() {
        assert_eq!(
            game_arguments("ea", "Origin.OFR.50.0002694").unwrap(),
            ["link2ea://launchgame/Origin.OFR.50.0002694?platform=PCWIN"]
        );
        assert_eq!(
            game_arguments("ubisoft", "480").unwrap(),
            ["uplay://launch/480/0"]
        );
        assert!(game_arguments("gog", "12345").unwrap().is_empty());
        assert!(game_arguments("epic", "ExampleGame").unwrap().is_empty());
        for store in ["steam", "unknown", ""] {
            assert!(game_arguments(store, "480").is_err());
        }
        for id in [
            "",
            "480?quit=1",
            "480/../quit",
            "480#",
            "-shutdown;quit",
            "a\n",
            "é",
            "abc:def",
        ] {
            assert!(game_arguments("ea", id).is_err(), "{id:?}");
        }
        for id in ["0", "000", "app", "123456789012345678901"] {
            assert!(game_arguments("ubisoft", id).is_err());
        }
    }

    #[test]
    fn current_game_and_same_bottle_client_are_required() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build/tests")
            .join(format!("publisher-route-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let bottle = create(&home, "Store fixture").unwrap();
        let sibling = create(&home, "Sibling fixture").unwrap();
        let prefix = bottle_dir(&home, &bottle.id).unwrap().join("prefix");
        let game = prefix.join("drive_c/GOG Games/Example");
        fs::create_dir_all(&game).unwrap();
        let manifest = game.join("goggame-123.info");
        fs::write(&manifest, br#"{"gameId":"123","name":"Example"}"#).unwrap();
        assert!(request(&home, &bottle.id, "gog", "123").is_err());
        let client = prefix.join("drive_c/Program Files/GOG Galaxy/GalaxyClient.exe");
        fs::create_dir_all(client.parent().unwrap()).unwrap();
        fs::write(&client, b"fixture only").unwrap();
        let saved = add_launcher(&home, &bottle.id, "GOG Galaxy", &client).unwrap();
        assert_eq!(
            request(&home, &bottle.id, "gog", "123").unwrap().0.id,
            saved.id
        );
        assert!(request(&home, &sibling.id, "gog", "123").is_err());
        set_launcher_arguments(&home, &bottle.id, &saved.id, &["/uninstall".into()]).unwrap();
        assert!(request(&home, &bottle.id, "gog", "123").is_err());
        set_launcher_arguments(&home, &bottle.id, &saved.id, &[]).unwrap();
        fs::remove_file(&manifest).unwrap();
        assert!(request(&home, &bottle.id, "gog", "123").is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn symlink_and_similar_named_clients_are_not_store_clients() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build/tests")
            .join(format!("publisher-client-{}", Uuid::new_v4()));
        let real = root.join("drive_c/Program Files/GOG Galaxy/GalaxyClient.exe");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, b"fixture").unwrap();
        assert!(checked_client(&root, &real, "gog"));
        assert!(!checked_client(&root, &real, "epic"));
        let outside = root.join("external.exe");
        fs::rename(&real, &outside).unwrap();
        symlink(&outside, &real).unwrap();
        assert!(!checked_client(&root, &real, "gog"));
        fs::remove_dir_all(root).unwrap();
    }
}
