//! Local bookmarks for browser games. No requests, account access, or Wine
//! processes are needed to save these links. The native app opens a checked
//! HTTPS URL in the user's ordinary browser only after an explicit Play action.

use super::*;
use std::io::Write;
use std::os::fd::FromRawFd;

const FILE_NAME: &str = "browser-games.json";
const LOCK_NAME: &str = ".browser-games.lock";
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_GAMES: usize = 512;
const MAX_URL_BYTES: usize = 2048;
const O_NONBLOCK: i32 = 0x4;
const O_NOFOLLOW: i32 = 0x100;
const O_CREAT: i32 = 0x200;
const O_EXCL: i32 = 0x800;
const O_CLOEXEC: i32 = 0x0100_0000;
const O_RDWR: i32 = 0x2;
const O_WRONLY: i32 = 0x1;
const LOCK_EX: i32 = 2;

unsafe extern "C" {
    fn openat(fd: i32, path: *const c_char, flags: i32, ...) -> i32;
    fn renameat(from_fd: i32, from: *const c_char, to_fd: i32, to: *const c_char) -> i32;
    fn unlinkat(fd: i32, path: *const c_char, flags: i32) -> i32;
    fn flock(fd: i32, operation: i32) -> i32;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrowserGame {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) url: String,
    pub(super) created_at: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct BrowserGames {
    pub(super) games: Vec<BrowserGame>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    games: Vec<BrowserGame>,
}

/// Accept a conservative browser URL, never a file, executable, deep link,
/// credential-bearing authority or local-network literal. DNS is not resolved:
/// Caskrun does not contact a game while saving its bookmark.
pub(super) fn validate_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_URL_BYTES {
        return Err("Use an HTTPS game link no longer than 2048 bytes".into());
    }
    if !value.is_ascii()
        || value.bytes().any(|b| {
            b <= 0x20
                || b == 0x7f
                || matches!(
                    b,
                    b'\\' | b'"' | b'<' | b'>' | b'`' | b'^' | b'{' | b'}' | b'|'
                )
        })
    {
        return Err("Game links cannot contain spaces, hidden characters or backslashes; use an encoded HTTPS link".into());
    }
    let rest = value
        .strip_prefix("https://")
        .ok_or("Game links must start with https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    if host.is_empty() || host.len() > 253 || host.contains(['@', ':', '%', '[', ']']) {
        return Err("Use a public website address without credentials or a custom port".into());
    }
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        || !labels
            .last()
            .is_some_and(|last| last.len() >= 2 && last.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return Err("Use a public website name, not an IP address or a local hostname".into());
    }
    if [
        "localhost",
        "local",
        "internal",
        "lan",
        "home",
        "invalid",
        "test",
        "example",
        "onion",
    ]
    .contains(labels.last().unwrap())
    {
        return Err("Local and reserved website names cannot be saved as browser games".into());
    }
    let suffix = &rest[end..];
    let bytes = suffix.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err("The game link contains an invalid percent escape".into());
            }
            let digit = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b.to_ascii_lowercase() - b'a' + 10
                }
            };
            let decoded = digit(bytes[index + 1]) * 16 + digit(bytes[index + 2]);
            if decoded < 0x20 || decoded == 0x7f || decoded == b'\\' {
                return Err(
                    "Game links cannot contain encoded hidden characters or backslashes".into(),
                );
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    // Bookmarks are local, but account links must not become durable game
    // shortcuts. Inspect both query and fragment keys; OAuth callbacks often
    // put credentials in a fragment. Percent-escaped/case variants count too.
    for parameters in suffix.split(['?', '#']).skip(1) {
        if parameters.split(['&', ';']).any(sensitive_parameter) {
            return Err("Use the game's public page, not a sign-in link containing account or access details".into());
        }
    }
    let suffix = if suffix.is_empty() { "/" } else { suffix };
    Ok(format!("https://{host}{suffix}"))
}

fn sensitive_parameter(parameter: &str) -> bool {
    let key = parameter.split('=').next().unwrap_or("");
    let mut bytes = Vec::with_capacity(key.len());
    let mut input = key.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input.next().and_then(|b| (b as char).to_digit(16));
            let low = input.next().and_then(|b| (b as char).to_digit(16));
            if let (Some(high), Some(low)) = (high, low) {
                bytes.push((high * 16 + low) as u8);
            }
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8_lossy(&bytes);
    let plain = decoded.split('[').next().unwrap_or("");
    let normalized: String = plain
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    // OAuth 2 authorization responses carry `code` (RFC 6749, 4.1.2);
    // OAuth 1 callbacks carry `oauth_token` and `oauth_verifier` (RFC 5849).
    // These and common token aliases are sign-in links, not public game pages.
    [
        "token",
        "accesstoken",
        "idtoken",
        "refreshtoken",
        "auth",
        "authtoken",
        "bearer",
        "bearertoken",
        "authorization",
        "authorizationcode",
        "code",
        "clientsecret",
        "jwt",
        "oauthtoken",
        "oauthtokensecret",
        "oauthverifier",
        "session",
        "sessionid",
        "sid",
        "apikey",
        "apitoken",
        "key",
        "password",
        "passwd",
        "pwd",
        "email",
    ]
    .contains(&normalized.as_str())
}

fn validate_document(document: &Document) -> Result<(), String> {
    if document.version != 1 || document.games.len() > MAX_GAMES {
        return Err("Browser games use an unsupported format or exceed the 512-game limit".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut urls = std::collections::BTreeSet::new();
    for game in &document.games {
        if Uuid::parse_str(&game.id).is_err()
            || !ids.insert(&game.id)
            || check_name(&game.name)? != game.name
            || validate_url(&game.url)? != game.url
            || !urls.insert(&game.url)
        {
            return Err("Browser games contain invalid or duplicate entries; the original file was preserved".into());
        }
    }
    Ok(())
}

struct Storage {
    path: PathBuf,
    directory: File,
    identity: fs::Metadata,
}

impl Storage {
    fn open(home: &Path, create: bool) -> Result<Option<Self>, String> {
        ensure_independent_storage(home)?;
        match fs::symlink_metadata(home) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
                return Ok(None);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(home).map_err(|e| e.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        let (directory, identity) = open_stable_directory(home)?;
        let storage = Self {
            path: home.into(),
            directory,
            identity,
        };
        storage.current()?;
        Ok(Some(storage))
    }

    fn current(&self) -> Result<(), String> {
        let (_, current) = open_stable_directory(&self.path)?;
        if !same_identity(&self.identity, &current) {
            return Err("Caskrun storage changed while saving browser games".into());
        }
        Ok(())
    }

    fn open_file(&self, name: &str, flags: i32, mode: u32) -> Result<Option<File>, String> {
        let name = CString::new(name).map_err(|_| "Invalid browser game file name")?;
        let fd = unsafe {
            openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                flags | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                mode,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(format!("Could not safely open browser games: {error}"))
            };
        }
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }

    fn read(&self) -> Result<(Document, Option<Vec<u8>>), String> {
        self.current()?;
        let Some(mut file) = self.open_file(FILE_NAME, 0, 0)? else {
            return Ok((
                Document {
                    version: 1,
                    games: Vec::new(),
                },
                None,
            ));
        };
        let before = file.metadata().map_err(|e| e.to_string())?;
        if !before.is_file() || before.nlink() != 1 || before.len() > MAX_FILE_BYTES {
            return Err("Browser games must be a small, private regular file".into());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let after = file.metadata().map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE_BYTES || !same_snapshot(&before, &after) {
            return Err("Browser games changed while being read".into());
        }
        let reopened = self
            .open_file(FILE_NAME, 0, 0)?
            .ok_or("Browser games changed while being read")?;
        if !same_snapshot(&before, &reopened.metadata().map_err(|e| e.to_string())?) {
            return Err("Browser games changed while being read".into());
        }
        self.current()?;
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| "Browser games have an invalid format; the original file was preserved")?;
        validate_document(&document)?;
        Ok((document, Some(bytes)))
    }

    fn lock(&self) -> Result<File, String> {
        let file = self
            .open_file(LOCK_NAME, O_RDWR | O_CREAT, 0o600)?
            .ok_or("Browser game lock could not be created")?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err("Browser game lock must be a private regular file".into());
        }
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | O_NONBLOCK) } != 0 {
            return Err("Another browser game edit is finishing. Try again.".into());
        }
        // A replaced lock cannot coordinate two editors. Hold the same inode
        // throughout this short edit, and refuse an unexpected replacement.
        let current = self
            .open_file(LOCK_NAME, O_RDWR, 0)?
            .ok_or("Browser game lock changed")?;
        if !same_snapshot(&metadata, &current.metadata().map_err(|e| e.to_string())?) {
            return Err("Browser game lock changed".into());
        }
        Ok(file)
    }

    fn write(&self, document: &Document, original: Option<Vec<u8>>) -> Result<(), String> {
        validate_document(document)?;
        let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("Browser games exceed the storage limit".into());
        }
        let temporary = format!(".browser-games-{}.tmp", Uuid::new_v4());
        let mut file = self
            .open_file(&temporary, O_WRONLY | O_CREAT | O_EXCL, 0o600)?
            .ok_or("Could not create browser games")?;
        let temporary_name = CString::new(temporary).unwrap();
        let result = (|| {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            cancellation::check()?;
            self.current()?;
            let staged = self
                .open_file(temporary_name.to_str().unwrap(), 0, 0)?
                .ok_or("Browser game save changed before publication")?;
            if !same_snapshot(
                &file.metadata().map_err(|e| e.to_string())?,
                &staged.metadata().map_err(|e| e.to_string())?,
            ) {
                return Err("Browser game save changed before publication".into());
            }
            if self.read()?.1 != original {
                return Err("Browser games changed before saving. Refresh and try again.".into());
            }
            let target = CString::new(FILE_NAME).unwrap();
            if unsafe {
                renameat(
                    self.directory.as_raw_fd(),
                    temporary_name.as_ptr(),
                    self.directory.as_raw_fd(),
                    target.as_ptr(),
                )
            } != 0
            {
                return Err(format!(
                    "Could not save browser games: {}",
                    std::io::Error::last_os_error()
                ));
            }
            self.directory.sync_all().map_err(|e| e.to_string())?;
            self.current()?;
            if self.read()?.1.as_deref() != Some(bytes.as_slice()) {
                return Err("Browser games changed during save verification".into());
            }
            Ok(())
        })();
        if result.is_err() {
            // Never remove an unrelated entry that replaced our temporary file.
            if let Ok(Some(current)) = self.open_file(temporary_name.to_str().unwrap(), 0, 0)
                && let (Ok(ours), Ok(theirs)) = (file.metadata(), current.metadata())
                && ours.dev() == theirs.dev()
                && ours.ino() == theirs.ino()
            {
                unsafe {
                    unlinkat(self.directory.as_raw_fd(), temporary_name.as_ptr(), 0);
                }
            }
        }
        result
    }
}

pub(super) fn list(home: &Path) -> Result<BrowserGames, String> {
    let Some(storage) = Storage::open(home, false)? else {
        return Ok(BrowserGames { games: Vec::new() });
    };
    let (mut document, _) = storage.read()?;
    document.games.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(BrowserGames {
        games: document.games,
    })
}

pub(super) fn add(home: &Path, name: &str, url: &str) -> Result<BrowserGame, String> {
    let name = check_name(name)?.to_owned();
    let url = validate_url(url)?;
    let storage = Storage::open(home, true)?.unwrap();
    let _lock = storage.lock()?;
    let (mut document, original) = storage.read()?;
    if let Some(existing) = document.games.iter().find(|game| game.url == url) {
        return Ok(existing.clone());
    }
    if document.games.len() >= MAX_GAMES {
        return Err("You can save up to 512 browser games".into());
    }
    let game = BrowserGame {
        id: Uuid::new_v4().to_string(),
        name,
        url,
        created_at: now_seconds()?,
    };
    document.games.push(game.clone());
    storage.write(&document, original)?;
    Ok(game)
}

pub(super) fn remove(home: &Path, id: &str) -> Result<BrowserGames, String> {
    Uuid::parse_str(id).map_err(|_| "Invalid browser game ID")?;
    let storage = Storage::open(home, false)?.ok_or("Browser game not found")?;
    let _lock = storage.lock()?;
    let (mut document, original) = storage.read()?;
    let before = document.games.len();
    document.games.retain(|game| game.id != id);
    if document.games.len() == before {
        return Err("Browser game not found".into());
    }
    storage.write(&document, original)?;
    list(home)
}

pub(super) fn rename(home: &Path, id: &str, name: &str) -> Result<BrowserGame, String> {
    Uuid::parse_str(id).map_err(|_| "Invalid browser game ID")?;
    let name = check_name(name)?.to_owned();
    let storage = Storage::open(home, false)?.ok_or("Browser game not found")?;
    let _lock = storage.lock()?;
    let (mut document, original) = storage.read()?;
    let game = document
        .games
        .iter_mut()
        .find(|game| game.id == id)
        .ok_or("Browser game not found")?;
    game.name = name;
    let result = game.clone();
    storage.write(&document, original)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("browser-games-{}", Uuid::new_v4()));
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn url_policy_checks_authority_and_escapes_without_contacting_hosts() {
        assert_eq!(
            validate_url(" https://Games.EXAMPLE.org ").unwrap(),
            "https://games.example.org/"
        );
        assert_eq!(
            validate_url("https://games.example.org/play?a=one%20two#level").unwrap(),
            "https://games.example.org/play?a=one%20two#level"
        );
        for bad in [
            "http://games.example.org/",
            "file:///game",
            "javascript:alert(1)",
            concat!("https://", "name:secret@games.example.org/"),
            "https://games.example.org:443/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://localhost/",
            "https://game.local/",
            "https://game.test/",
            "https://game.example/",
            "https://games.example.org./",
            concat!("https://", "games.example.org\\@evil.org/"),
            "https://games.example.org/%00",
            "https://games.example.org/%0a",
            "https://games.example.org/%5c",
            "https://games.example.org/%",
            "https://games.example.org/a b",
            "https://gáme.example.org/",
        ] {
            assert!(validate_url(bad).is_err(), "accepted {bad}");
        }
        assert!(validate_url(&format!("https://games.example.org/{}", "x".repeat(2048))).is_err());
    }

    #[test]
    fn missing_list_is_read_only_and_invalid_add_creates_no_storage() {
        let fixture = Fixture::new();
        assert!(list(&fixture.0).unwrap().games.is_empty());
        assert!(!fixture.0.exists());
        assert!(add(&fixture.0, "Game", "http://games.example.org/").is_err());
        assert!(add(&fixture.0, "\n", "https://games.example.org/").is_err());
        assert!(!fixture.0.exists());
    }

    #[test]
    fn account_query_and_fragment_keys_are_never_saved() {
        let fixture = Fixture::new();
        for suffix in [
            "?token=private",
            "?ACCESS_TOKEN=private",
            "?%74%6f%6b%65%6e=private",
            "?access%5ftoken=private",
            "?session[id]=private",
            "?email=person",
            "?api-key=private",
            "?auth_token=private",
            "?AUTH%5fTOKEN=private",
            "?bearer_token=private",
            "?authorization_code=private",
            "?code=private",
            "?client_secret=private",
            "?jwt=private",
            "?oauth_token=private",
            "?oauth_token_secret=private",
            "?oauth_verifier=private",
            "#id_token=private",
            "#auth[token]=private",
            "#client-secret=private",
            "#/play?authorization=private",
            "?game=1;Password=private",
        ] {
            assert!(
                validate_url(&format!("https://games.example.org/{suffix}")).is_err(),
                "accepted {suffix}"
            );
            assert!(
                add(
                    &fixture.0,
                    "Game",
                    &format!("https://games.example.org/{suffix}")
                )
                .is_err()
            );
        }
        assert!(!fixture.0.exists());
        assert!(validate_url("https://games.example.org/?game=1&level=2#play").is_ok());
    }

    #[test]
    fn add_deduplicates_and_rename_remove_preserve_other_games() {
        let fixture = Fixture::new();
        let first = add(&fixture.0, " First ", "https://Games.EXAMPLE.org").unwrap();
        let second = add(&fixture.0, "Second", "https://games.example.org/two").unwrap();
        assert_eq!(
            add(&fixture.0, "Duplicate", "https://games.example.org/")
                .unwrap()
                .id,
            first.id
        );
        assert_eq!(list(&fixture.0).unwrap().games.len(), 2);
        let renamed = rename(&fixture.0, &first.id, "Changed").unwrap();
        assert_eq!(renamed.name, "Changed");
        assert_eq!(renamed.created_at, first.created_at);
        assert_eq!(renamed.url, first.url);
        let remaining = remove(&fixture.0, &first.id).unwrap();
        assert_eq!(remaining.games.len(), 1);
        assert_eq!(remaining.games[0].id, second.id);
        assert!(remove(&fixture.0, &first.id).is_err());
        assert!(remove(&fixture.0, "invalid").is_err());
        assert_eq!(
            fs::metadata(fixture.0.join(FILE_NAME)).unwrap().mode() & 0o777,
            0o600
        );
        assert!(!fixture.0.join("bottles").exists());
        assert!(
            !fs::read_dir(&fixture.0).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
    }

    #[test]
    fn malformed_unknown_duplicate_and_oversized_documents_are_preserved() {
        let fixture = Fixture::new();
        let first = add(&fixture.0, "Game", "https://games.example.org/").unwrap();
        let original = fs::read(fixture.0.join(FILE_NAME)).unwrap();
        let documents = [
            b"broken".to_vec(),
            br#"{"version":2,"games":[]}"#.to_vec(),
            br#"{"version":1,"games":[],"secret":true}"#.to_vec(),
            serde_json::to_vec(&Document {
                version: 1,
                games: vec![first.clone(), first],
            })
            .unwrap(),
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        ];
        for bytes in documents {
            fs::write(fixture.0.join(FILE_NAME), &bytes).unwrap();
            assert!(list(&fixture.0).is_err());
            assert!(add(&fixture.0, "Other", "https://games.example.org/other").is_err());
            assert_eq!(fs::read(fixture.0.join(FILE_NAME)).unwrap(), bytes);
        }
        fs::write(fixture.0.join(FILE_NAME), original).unwrap();
    }

    #[test]
    fn symlinks_and_hardlinks_are_refused_and_targets_survive() {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0).unwrap();
        let target = fixture.0.join("target.json");
        fs::write(&target, br#"{"version":1,"games":[]}"#).unwrap();
        let original = fs::read(&target).unwrap();
        symlink(&target, fixture.0.join(FILE_NAME)).unwrap();
        assert!(list(&fixture.0).is_err());
        assert!(add(&fixture.0, "Game", "https://games.example.org/").is_err());
        fs::remove_file(fixture.0.join(FILE_NAME)).unwrap();
        fs::hard_link(&target, fixture.0.join(FILE_NAME)).unwrap();
        assert!(list(&fixture.0).is_err());
        assert!(add(&fixture.0, "Game", "https://games.example.org/").is_err());
        assert_eq!(fs::read(&target).unwrap(), original);
        fs::remove_file(fixture.0.join(FILE_NAME)).unwrap();
        symlink(&target, fixture.0.join(LOCK_NAME)).unwrap_err(); // real lock is kept for coordination
        fs::remove_file(fixture.0.join(LOCK_NAME)).unwrap();
        symlink(&target, fixture.0.join(LOCK_NAME)).unwrap();
        assert!(add(&fixture.0, "Game", "https://games.example.org/").is_err());
        assert_eq!(fs::read(target).unwrap(), original);
    }

    #[test]
    fn replaced_storage_and_concurrent_edits_refuse_without_overwrite() {
        let fixture = Fixture::new();
        add(&fixture.0, "Game", "https://games.example.org/").unwrap();
        let storage = Storage::open(&fixture.0, false).unwrap().unwrap();
        let lock = storage.lock().unwrap();
        assert!(add(&fixture.0, "Other", "https://games.example.org/two").is_err());
        drop(lock);
        let (document, original) = storage.read().unwrap();
        add(&fixture.0, "Other", "https://games.example.org/two").unwrap();
        assert!(storage.write(&document, original).is_err());
        assert_eq!(list(&fixture.0).unwrap().games.len(), 2);
        let moved = fixture.0.with_extension("moved");
        fs::rename(&fixture.0, &moved).unwrap();
        fs::create_dir(&fixture.0).unwrap();
        assert!(storage.read().is_err());
        fs::remove_dir(&fixture.0).unwrap();
        fs::rename(moved, &fixture.0).unwrap();
    }
}
