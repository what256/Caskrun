//! Read-only uninstall-registration inventory. A registration is evidence that
//! an installer registered a product, not that its files or dependencies work.
//! Wine's on-disk registry format is decoded without starting Wine, flushing a
//! live server, editing a registry, or following a bottle's host-folder links.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::{O_NOFOLLOW_ANY, bottle_dir, open_stable_directory, same_identity, same_snapshot};

const MAX_REGISTRY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_FIELD_BYTES: usize = 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_WARNINGS: usize = 8;
// Darwin: avoid blocking if an inspected regular file is exchanged for a FIFO
// before open. The descriptor's metadata must still match the regular file.
const O_NONBLOCK: i32 = 0x0004;
const UNINSTALL: &str = "software\\microsoft\\windows\\currentversion\\uninstall\\";
const UNINSTALL_X86: &str =
    "software\\wow6432node\\microsoft\\windows\\currentversion\\uninstall\\";

#[derive(Debug, Serialize)]
pub(crate) struct ProgramInventory {
    pub(crate) bottle_id: String,
    /// Whether a machine registry exists. This does not attest engine health.
    pub(crate) initialized: bool,
    pub(crate) complete: bool,
    pub(crate) count: usize,
    pub(crate) programs: Vec<RegisteredProgram>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RegisteredProgram {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) publisher: Option<String>,
    /// Registry view/scope, not an inspection of a program's PE architecture.
    pub(crate) sources: Vec<&'static str>,
}

#[derive(Default)]
struct Evidence {
    programs: Vec<RegisteredProgram>,
    warnings: Vec<String>,
    incomplete: bool,
}

impl Evidence {
    fn warn(&mut self, warning: String) {
        self.incomplete = true;
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(warning);
        } else if self.warnings.len() == MAX_WARNINGS {
            self.warnings
                .push("Additional registry problems were omitted.".into());
        }
    }

    fn malformed(&mut self, file: &str, line: usize, description: &str) {
        self.warn(format!("{file}, line {line}: {description}."));
    }
}

enum RegistryRead {
    Missing,
    Text(String, fs::Metadata),
    Unavailable(String),
}

/// Inspect only the two registry files at the root of an independently owned
/// bottle. Changed files produce a warning rather than a mixed snapshot.
pub(crate) fn inventory(home: &Path, id: &str) -> Result<ProgramInventory, String> {
    let dir = bottle_dir(home, id)?;
    let (_bottle_handle, bottle_snapshot) = open_stable_directory(&dir)?;
    let prefix = dir.join("prefix");
    let mut evidence = Evidence::default();
    let prefix_snapshot = match fs::symlink_metadata(&prefix) {
        Ok(snapshot) if snapshot.file_type().is_dir() => snapshot,
        Ok(_) => return Err("Bottle prefix must be a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(finish(id, false, evidence));
        }
        Err(error) => return Err(format!("Could not inspect bottle prefix: {error}")),
    };
    let (_prefix_handle, opened_snapshot) = open_stable_directory(&prefix)?;
    if !same_snapshot(&prefix_snapshot, &opened_snapshot) {
        return Err("Bottle prefix changed while opening it".into());
    }
    let system = read_registry(&prefix, "system.reg");
    let user = read_registry(&prefix, "user.reg");
    let initialized = !matches!(&system, RegistryRead::Missing);
    let has_registry = initialized || !matches!(&user, RegistryRead::Missing);
    let mut snapshots = Vec::new();
    for (name, machine, read) in [("system.reg", true, system), ("user.reg", false, user)] {
        match read {
            RegistryRead::Text(contents, snapshot) => {
                parse_registry(&contents, name, machine, &mut evidence);
                snapshots.push((name, machine, snapshot));
            }
            RegistryRead::Unavailable(message) => evidence.warn(message),
            RegistryRead::Missing if has_registry => evidence.warn(format!(
                "{name} is missing; registered programs may be incomplete."
            )),
            RegistryRead::Missing => {}
        }
    }
    for (name, machine, snapshot) in snapshots {
        if !fs::symlink_metadata(prefix.join(name))
            .is_ok_and(|current| same_snapshot(&snapshot, &current))
        {
            evidence.warn(format!(
                "{name} changed during the inspection; its entries were omitted."
            ));
            evidence.programs.retain(|program| {
                !program.sources.iter().any(|source| {
                    if machine {
                        source.starts_with("machine")
                    } else {
                        source.starts_with("current_user")
                    }
                })
            });
        }
    }
    // Registry writers can legitimately replace these files while an app is
    // running. Do not assert a complete snapshot if the enclosing directories
    // were exchanged; O_NOFOLLOW_ANY already prevents following their links.
    if !fs::symlink_metadata(&dir).is_ok_and(|current| same_identity(&bottle_snapshot, &current))
        || !fs::symlink_metadata(&prefix)
            .is_ok_and(|current| same_identity(&prefix_snapshot, &current))
    {
        return Err("Bottle storage changed while reading registered programs".into());
    }
    Ok(finish(id, initialized, evidence))
}

fn read_registry(prefix: &Path, name: &str) -> RegistryRead {
    let path = prefix.join(name);
    let read = || -> Result<Option<(String, fs::Metadata)>, String> {
        let before = match fs::symlink_metadata(&path) {
            Ok(snapshot) => snapshot,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{name} could not be inspected: {error}")),
        };
        if !before.file_type().is_file() || before.len() > MAX_REGISTRY_BYTES {
            return Err(format!(
                "{name} must be a bounded regular file; it was not read."
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY | O_NONBLOCK)
            .open(&path)
            .map_err(|_| format!("{name} could not be safely opened; it was not read."))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err(format!("{name} changed while opening it; it was not read."));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_REGISTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| format!("{name} could not be read completely."))?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES
            || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
            || !fs::symlink_metadata(&path).is_ok_and(|now| same_snapshot(&before, &now))
        {
            return Err(format!(
                "{name} changed while reading it; its entries were omitted."
            ));
        }
        String::from_utf8(bytes)
            .map(|text| Some((text, before)))
            .map_err(|_| format!("{name} is not valid UTF-8; its entries were omitted."))
    };
    match read() {
        Ok(Some((text, snapshot))) => RegistryRead::Text(text, snapshot),
        Ok(None) => RegistryRead::Missing,
        Err(error) => RegistryRead::Unavailable(error),
    }
}

fn finish(id: &str, initialized: bool, evidence: Evidence) -> ProgramInventory {
    // Merge only entries with coherent display metadata. Different versions or
    // publishers stay separate; no guess is made from an application's name.
    let mut unique: BTreeMap<(String, String, String), RegisteredProgram> = BTreeMap::new();
    for program in evidence.programs {
        let key = (
            program.name.to_lowercase(),
            program.version.as_deref().unwrap_or("").to_lowercase(),
            program.publisher.as_deref().unwrap_or("").to_lowercase(),
        );
        if let Some(existing) = unique.get_mut(&key) {
            for source in program.sources {
                if !existing.sources.contains(&source) {
                    existing.sources.push(source);
                    existing.sources.sort_unstable();
                }
            }
        } else {
            unique.insert(key, program);
        }
    }
    let programs: Vec<_> = unique.into_values().collect();
    ProgramInventory {
        bottle_id: id.into(),
        initialized,
        complete: !evidence.incomplete,
        count: programs.len(),
        programs,
        warnings: evidence.warnings,
    }
}

#[derive(Default)]
struct Entry {
    source: &'static str,
    name: Option<String>,
    version: Option<String>,
    publisher: Option<String>,
    system_component: bool,
    child: bool,
    update: bool,
    invalid: bool,
    seen_fields: BTreeSet<String>,
}

fn parse_registry(contents: &str, file: &str, machine: bool, evidence: &mut Evidence) {
    let mut lines = contents.lines().enumerate();
    if !matches!(lines.next(), Some((_, "WINE REGISTRY Version 2"))) {
        evidence.warn(format!(
            "{file} has an unsupported registry header; its entries were omitted."
        ));
        return;
    }
    if !contents.ends_with('\n') {
        evidence.warn(format!(
            "{file} has an unfinished final line; its last entry was omitted."
        ));
    }
    let mut arch = None;
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut section_seen = false;
    let mut skipping_continuation = false;
    while let Some((line_index, raw)) = lines.next() {
        let number = line_index + 1;
        let line = raw.trim();
        if skipping_continuation {
            if valid_hex_row(line) {
                skipping_continuation = line.ends_with('\\');
                continue;
            }
            // A truncated binary must not swallow the following section or
            // display value and make an incomplete scan look complete.
            evidence.malformed(file, number, "malformed registry continuation");
            if let Some(entry) = current.as_ref().and_then(|key| entries.get_mut(key)) {
                entry.invalid = true;
            }
            skipping_continuation = false;
        }
        if line.len() > MAX_LINE_BYTES {
            evidence.malformed(file, number, "registry line exceeds the inspection limit");
            if let Some(entry) = current.as_ref().and_then(|key| entries.get_mut(key)) {
                entry.invalid = true;
            }
            skipping_continuation = line.ends_with('\\');
            continue;
        }
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(value) = line.strip_prefix("#arch=") {
            if !entries.is_empty() || arch.is_some() || !matches!(value, "win32" | "win64") {
                evidence.malformed(file, number, "ambiguous registry architecture metadata");
            } else {
                arch = Some(value);
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            current = None;
            let Some((key, _)) = decode_delimited(section, ']') else {
                evidence.malformed(file, number, "malformed registry section");
                continue;
            };
            section_seen = true;
            let lower = key.to_lowercase();
            let Some(source) = inventory_source(&lower, machine, arch) else {
                continue;
            };
            if entries.len() >= MAX_ENTRIES && !entries.contains_key(&lower) {
                evidence.warn(format!(
                    "{file} exceeds the registered-program inspection limit."
                ));
                break;
            }
            if let Some(entry) = entries.get_mut(&lower) {
                entry.invalid = true;
                evidence.malformed(file, number, "duplicate uninstall registry section");
            } else {
                entries.insert(
                    lower.clone(),
                    Entry {
                        source,
                        ..Entry::default()
                    },
                );
            }
            current = Some(lower);
            continue;
        }
        // Hex values can continue on following physical lines, including keys
        // outside the uninstall branches. Consume them without treating their
        // byte rows as registry sections or values.
        let Some(key) = current.as_ref() else {
            if !matches!(line.as_bytes().first(), Some(b'"' | b'@')) || !section_seen {
                evidence.malformed(file, number, "unrecognized registry input");
            }
            skipping_continuation = line.ends_with('\\');
            continue;
        };
        let entry = entries.get_mut(key).expect("current registry entry exists");
        let Some(rest) = line.strip_prefix('"') else {
            if !line.starts_with('@') {
                entry.invalid = true;
                evidence.malformed(file, number, "malformed uninstall registry value");
            }
            skipping_continuation = line.ends_with('\\');
            continue;
        };
        let Some((name, consumed)) = decode_delimited(rest, '"') else {
            entry.invalid = true;
            evidence.malformed(file, number, "malformed uninstall registry value name");
            continue;
        };
        let field = name.to_ascii_lowercase();
        let Some(value) = rest[consumed..]
            .trim_start()
            .strip_prefix('=')
            .map(str::trim_start)
        else {
            entry.invalid = true;
            evidence.malformed(file, number, "missing uninstall registry value assignment");
            continue;
        };
        if !matches!(
            field.as_str(),
            "displayname"
                | "displayversion"
                | "publisher"
                | "systemcomponent"
                | "parentkeyname"
                | "releasetype"
        ) {
            skipping_continuation = value.ends_with('\\');
            continue;
        }
        if !entry.seen_fields.insert(field.clone()) {
            entry.invalid = true;
            evidence.malformed(file, number, "duplicate uninstall registry display value");
            continue;
        }
        let mut value = value.to_owned();
        while value.ends_with('\\') && value.starts_with("hex(") {
            value.pop();
            let Some((_, next)) = lines.next() else {
                entry.invalid = true;
                evidence.malformed(file, number, "unfinished registry continuation");
                break;
            };
            if value.len() + next.len() > MAX_LINE_BYTES {
                entry.invalid = true;
                evidence.malformed(file, number, "registry value exceeds the inspection limit");
                skipping_continuation = next.trim_end().ends_with('\\');
                break;
            }
            value.push_str(next.trim());
        }
        if entry.invalid {
            continue;
        }
        let valid = if field == "systemcomponent" {
            parse_dword(&value)
                .map(|number| entry.system_component = number != 0)
                .is_some()
        } else if let Some(text) = parse_text_value(&value) {
            match field.as_str() {
                "displayname" => entry.name = nonempty(text),
                "displayversion" => entry.version = nonempty(text),
                "publisher" => entry.publisher = nonempty(text),
                "parentkeyname" => entry.child = !text.is_empty(),
                "releasetype" => {
                    entry.update = matches!(
                        text.to_ascii_lowercase().as_str(),
                        "update" | "hotfix" | "security update"
                    )
                }
                _ => unreachable!(),
            }
            true
        } else {
            false
        };
        if !valid {
            entry.invalid = true;
            evidence.malformed(
                file,
                number,
                "unsupported or malformed uninstall display metadata",
            );
        }
    }
    if skipping_continuation {
        evidence.warn(format!("{file} has an unfinished registry continuation."));
        if let Some(entry) = current.as_ref().and_then(|key| entries.get_mut(key)) {
            entry.invalid = true;
        }
    }
    if !contents.ends_with('\n')
        && let Some(entry) = current.as_ref().and_then(|key| entries.get_mut(key))
    {
        entry.invalid = true;
    }
    for entry in entries.into_values() {
        if !entry.invalid
            && !entry.system_component
            && !entry.child
            && !entry.update
            && let Some(name) = entry.name
        {
            evidence.programs.push(RegisteredProgram {
                name,
                version: entry.version,
                publisher: entry.publisher,
                sources: vec![entry.source],
            });
        }
    }
}

fn inventory_source(key: &str, machine: bool, arch: Option<&str>) -> Option<&'static str> {
    let (suffix, wow) = if let Some(suffix) = key.strip_prefix(UNINSTALL) {
        (suffix, false)
    } else if let Some(suffix) = key.strip_prefix(UNINSTALL_X86) {
        (suffix, true)
    } else {
        return None;
    };
    if suffix.is_empty() || suffix.contains('\\') {
        return None;
    }
    Some(match (machine, wow, arch) {
        (false, false, _) => "current_user",
        (false, true, _) => "current_user_x86",
        (true, true, _) | (true, false, Some("win32")) => "machine_x86",
        (true, false, Some("win64")) => "machine_x64",
        (true, false, _) => "machine",
    })
}

fn nonempty(text: String) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn valid_hex_row(line: &str) -> bool {
    let row = line.strip_suffix('\\').unwrap_or(line).trim_end();
    let row = row.strip_suffix(',').unwrap_or(row);
    !row.is_empty()
        && row.split(',').all(|part| {
            let part = part.trim();
            part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn parse_dword(value: &str) -> Option<u32> {
    let hex = value.strip_prefix("dword:")?;
    if hex.is_empty() || hex.len() > 8 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn parse_text_value(value: &str) -> Option<String> {
    let decoded = if let Some(quoted) = value
        .strip_prefix('"')
        .or_else(|| value.strip_prefix("str:\""))
        .or_else(|| value.strip_prefix("str(2):\""))
    {
        let (text, consumed) = decode_delimited(quoted, '"')?;
        if !quoted[consumed..].trim().is_empty() {
            return None;
        }
        text
    } else {
        let bytes = value
            .strip_prefix("hex(1):")
            .or_else(|| value.strip_prefix("hex(2):"))?;
        let bytes: Option<Vec<u8>> = bytes
            .split(',')
            .map(|byte| {
                let byte = byte.trim();
                (byte.len() == 2)
                    .then(|| u8::from_str_radix(byte, 16).ok())
                    .flatten()
            })
            .collect();
        let bytes = bytes?;
        if bytes.len() < 2 || bytes.len() % 2 != 0 || !bytes.ends_with(&[0, 0]) {
            return None;
        }
        let units: Vec<u16> = bytes[..bytes.len() - 2]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).ok()?
    };
    if decoded.len() > MAX_FIELD_BYTES || decoded.chars().any(char::is_control) {
        return None;
    }
    Some(decoded)
}

/// Decode Wine's C/octal/1–4-digit UTF-16 hex escapes, including surrogate
/// pairs. `consumed` includes the unescaped delimiter, even for escaped `]`.
fn decode_delimited(input: &str, delimiter: char) -> Option<(String, usize)> {
    let mut units = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((offset, character)) = chars.next() {
        if character == delimiter {
            return String::from_utf16(&units)
                .ok()
                .map(|text| (text, offset + character.len_utf8()));
        }
        if character == '\\' {
            let (_, escaped) = chars.next()?;
            let simple = match escaped {
                'a' => Some(7),
                'b' => Some(8),
                'e' => Some(27),
                'f' => Some(12),
                'n' => Some(10),
                'r' => Some(13),
                't' => Some(9),
                'v' => Some(11),
                _ => None,
            };
            if let Some(unit) = simple {
                units.push(unit);
            } else if escaped == 'x' {
                let mut number = 0u16;
                let mut count = 0;
                while count < 4 {
                    let Some((_, next)) = chars.peek() else {
                        break;
                    };
                    let Some(digit) = next.to_digit(16) else {
                        break;
                    };
                    number = number * 16 + digit as u16;
                    chars.next();
                    count += 1;
                }
                units.push(if count == 0 { b'x' as u16 } else { number });
            } else if matches!(escaped, '0'..='7') {
                let mut number = escaped as u16 - b'0' as u16;
                for _ in 0..2 {
                    let Some((_, next)) = chars.peek() else {
                        break;
                    };
                    let Some(digit) = next.to_digit(8) else {
                        break;
                    };
                    number = number * 8 + digit as u16;
                    chars.next();
                }
                units.push(number);
            } else {
                units.extend(escaped.encode_utf16(&mut [0; 2]).iter().copied());
            }
        } else {
            units.extend(character.encode_utf16(&mut [0; 2]).iter().copied());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn registry(arch: &str, sections: &str) -> String {
        format!("WINE REGISTRY Version 2\n#arch={arch}\n\n{sections}")
    }

    fn section(key: &str, values: &str) -> String {
        format!(
            "[Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\{key}] 1700000000\n{values}\n"
        )
    }

    fn parsed(contents: &str, machine: bool) -> ProgramInventory {
        let mut evidence = Evidence::default();
        parse_registry(
            contents,
            if machine { "system.reg" } else { "user.reg" },
            machine,
            &mut evidence,
        );
        finish("example", true, evidence)
    }

    struct Fixture {
        home: PathBuf,
        id: String,
    }
    impl Fixture {
        fn new() -> Self {
            let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-data")
                .join(format!("program-inventory-{}", Uuid::new_v4()));
            fs::create_dir_all(&home).unwrap();
            let id = crate::create(&home, "Inventory fixture").unwrap().id;
            Self { home, id }
        }
        fn prefix(&self) -> PathBuf {
            self.home.join("bottles").join(&self.id).join("prefix")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.home).unwrap();
        }
    }

    #[test]
    fn parses_machine_and_user_views_without_confusing_nested_subkeys() {
        let normal = section(
            "Example",
            "\"DisplayName\"=\"Example App\"\n\"DisplayVersion\"=\"1.2\"\n\"Publisher\"=\"Example Publisher\"",
        );
        let wow = "[software\\\\wow6432node\\\\microsoft\\\\windows\\\\currentversion\\\\uninstall\\\\Another]\n\"DisplayName\"=\"32-bit app\"\n";
        let nested = section("Example\\\\Child", "\"DisplayName\"=\"Not a product\"");
        let machine = parsed(&registry("win64", &(normal.clone() + wow + &nested)), true);
        assert!(machine.complete, "{:?}", machine.warnings);
        assert_eq!(machine.count, 2);
        assert_eq!(machine.programs[0].sources, ["machine_x86"]);
        assert_eq!(machine.programs[1].sources, ["machine_x64"]);
        assert_eq!(machine.programs[1].version.as_deref(), Some("1.2"));
        let user = parsed(&registry("win64", &(normal + wow)), false);
        assert_eq!(user.programs[0].sources, ["current_user_x86"]);
        assert_eq!(user.programs[1].sources, ["current_user"]);
        let win32 = parsed(
            &registry("win32", &section("Native", "\"DisplayName\"=\"Native\"")),
            true,
        );
        assert_eq!(win32.programs[0].sources, ["machine_x86"]);
    }

    #[test]
    fn decodes_real_wine_escaping_unicode_surrogates_and_section_delimiters() {
        let text = registry(
            "win64",
            &section(
                "Escaped\\]Key",
                r#""DisplayName"="Caf\xe9 \xd83d\xde80 \"Edition\""
"Publisher"=str(2):"A\\B\040C"
"DisplayVersion"=hex(1):31,00,2e,00,\
  30,00,00,00
"#,
            ),
        );
        let result = parsed(&text, true);
        assert!(result.complete, "{:?}", result.warnings);
        assert_eq!(result.programs[0].name, "Café 🚀 \"Edition\"");
        assert_eq!(result.programs[0].publisher.as_deref(), Some("A\\B C"));
        assert_eq!(result.programs[0].version.as_deref(), Some("1.0"));
    }

    #[test]
    fn omits_hidden_children_updates_and_empty_names_but_keeps_no_remove_products() {
        let mut sections = section(
            "Visible",
            "\"DisplayName\"=\"Visible\"\n\"NoRemove\"=dword:00000001",
        );
        sections += &section(
            "Hidden",
            "\"DisplayName\"=\"Hidden\"\n\"SystemComponent\"=dword:00000001",
        );
        sections += &section(
            "Child",
            "\"DisplayName\"=\"Child\"\n\"ParentKeyName\"=\"Parent\"",
        );
        sections += &section(
            "Update",
            "\"DisplayName\"=\"Update\"\n\"ReleaseType\"=\"Security Update\"",
        );
        sections += &section("Empty", "\"DisplayName\"=\"   \"");
        sections += &section("NoName", "\"Publisher\"=\"Unknown\"");
        let result = parsed(&registry("win64", &sections), true);
        assert!(result.complete);
        assert_eq!(result.count, 1);
        assert_eq!(result.programs[0].name, "Visible");
    }

    #[test]
    fn coherently_deduplicates_metadata_across_sources_but_keeps_distinct_versions() {
        let mut evidence = Evidence::default();
        let text = section(
            "One",
            "\"DisplayName\"=\"Example\"\n\"DisplayVersion\"=\"1\"",
        );
        parse_registry(&registry("win64", &text), "system.reg", true, &mut evidence);
        parse_registry(
            &registry(
                "win64",
                &(text
                    + &section(
                        "Two",
                        "\"DisplayName\"=\"Example\"\n\"DisplayVersion\"=\"2\"",
                    )),
            ),
            "user.reg",
            false,
            &mut evidence,
        );
        let result = finish("example", true, evidence);
        assert_eq!(result.count, 2);
        assert_eq!(result.programs[0].sources, ["current_user", "machine_x64"]);
        assert_eq!(result.programs[1].sources, ["current_user"]);
    }

    #[test]
    fn malformed_or_ambiguous_evidence_warns_and_omits_the_affected_entry() {
        for values in [
            "\"DisplayName\"=\"Example\"\n\"SystemComponent\"=\"not a dword\"",
            "\"DisplayName\"=\"Example\" trailing",
            "\"DisplayName\"=\"Example\"\n\"DisplayName\"=\"Other\"",
            "\"DisplayName\"=\"Example\\nInjected\"",
            "\"DisplayName\"=\"Example\\xd800\"",
            "\"DisplayName\"=hex(1):41,00",
            "\"DisplayName\"=\"Example\"\n\"Publisher\" broken",
        ] {
            let result = parsed(&registry("win64", &section("Bad", values)), true);
            assert!(!result.complete, "{values}");
            assert_eq!(result.count, 0, "{values}");
            assert!(!result.warnings.is_empty());
        }
        let duplicate = section("Repeated", "\"DisplayName\"=\"First\"")
            + &section("repeated", "\"DisplayName\"=\"Second\"");
        assert_eq!(parsed(&registry("win64", &duplicate), true).count, 0);
        let unfinished = registry("win64", &section("Last", "\"DisplayName\"=\"Last\""));
        let result = parsed(unfinished.trim_end_matches('\n'), true);
        assert!(!result.complete);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn missing_architecture_is_not_claimed_to_be_x64_and_unknown_headers_fail_closed() {
        let text = format!(
            "WINE REGISTRY Version 2\n{}",
            section("Example", "\"DisplayName\"=\"Example\"")
        );
        assert_eq!(parsed(&text, true).programs[0].sources, ["machine"]);
        let result = parsed("Windows Registry Editor Version 5.00\n", true);
        assert!(!result.complete);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn unknown_multiline_binary_values_do_not_leak_into_other_sections() {
        let text = format!(
            "WINE REGISTRY Version 2\n#arch=win64\n[Other]\n\"Binary\"=hex:11,22,\\\n  33,44\n{}",
            section(
                "Good",
                "\"DisplayName\"=\"Good\"\n\"OtherBinary\"=hex:11,\\\n  22,33"
            )
        );
        let result = parsed(&text, true);
        assert!(result.complete, "{:?}", result.warnings);
        assert_eq!(result.count, 1);
        let truncated = format!(
            "WINE REGISTRY Version 2\n[Other]\n\"Binary\"=hex:11,\\\n{}",
            section("Good", "\"DisplayName\"=\"Good\"")
        );
        let result = parsed(&truncated, true);
        assert!(!result.complete);
        assert_eq!(result.count, 1);
        let unfinished = registry(
            "win64",
            &section(
                "Bad",
                "\"DisplayName\"=\"Bad\"\n\"Publisher\"=hex(1):41,00,00,00\\",
            ),
        );
        let result = parsed(&unfinished, true);
        assert!(!result.complete);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn inspection_and_warning_bounds_report_partial_results_instead_of_claiming_success() {
        let mut text = registry("win64", "");
        for index in 0..=MAX_ENTRIES {
            text += &section(
                &format!("Product{index}"),
                &format!("\"DisplayName\"=\"Product {index}\""),
            );
        }
        let result = parsed(&text, true);
        assert_eq!(result.count, MAX_ENTRIES);
        assert!(!result.complete);
        assert!(result.warnings[0].contains("inspection limit"));
        let oversized_name = "a".repeat(MAX_FIELD_BYTES + 1);
        let result = parsed(
            &registry(
                "win64",
                &section("LongName", &format!("\"DisplayName\"=\"{oversized_name}\"")),
            ),
            true,
        );
        assert!(!result.complete);
        assert_eq!(result.count, 0);
        let malformed = format!("WINE REGISTRY Version 2\n{}", "invalid input\n".repeat(100));
        let result = parsed(&malformed, true);
        assert!(!result.complete);
        assert_eq!(result.warnings.len(), MAX_WARNINGS + 1);
        assert!(result.warnings.last().unwrap().contains("omitted"));
    }

    #[test]
    fn new_bottle_inventory_does_not_initialize_or_create_any_files() {
        let fixture = Fixture::new();
        let prefix = fixture.prefix();
        let before = fs::metadata(&prefix).unwrap();
        let result = inventory(&fixture.home, &fixture.id).unwrap();
        assert!(!result.initialized);
        assert!(result.complete);
        assert_eq!(result.count, 0);
        assert_eq!(fs::read_dir(&prefix).unwrap().count(), 0);
        assert!(same_snapshot(&before, &fs::metadata(&prefix).unwrap()));
    }

    #[test]
    fn file_fixture_is_read_only_and_missing_user_registry_is_explicit() {
        let fixture = Fixture::new();
        let path = fixture.prefix().join("system.reg");
        let bytes = registry("win64", &section("Example", "\"DisplayName\"=\"Example\""));
        fs::write(&path, &bytes).unwrap();
        let before = fs::metadata(&path).unwrap();
        let result = inventory(&fixture.home, &fixture.id).unwrap();
        assert!(result.initialized);
        assert!(!result.complete);
        assert_eq!(result.count, 1);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("user.reg is missing"))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        assert!(same_snapshot(&before, &fs::metadata(&path).unwrap()));
        assert_eq!(before.ino(), fs::metadata(&path).unwrap().ino());
        fs::write(fixture.prefix().join("user.reg"), registry("win64", "")).unwrap();
        assert!(inventory(&fixture.home, &fixture.id).unwrap().complete);
    }

    #[test]
    fn external_symlinks_nonregular_files_oversize_and_bad_utf8_are_never_read() {
        let fixture = Fixture::new();
        let prefix = fixture.prefix();
        let outside = fixture.home.join("external-registry");
        fs::write(
            &outside,
            registry(
                "win64",
                &section("External", "\"DisplayName\"=\"External\""),
            ),
        )
        .unwrap();
        let system = prefix.join("system.reg");
        symlink(&outside, &system).unwrap();
        let result = inventory(&fixture.home, &fixture.id).unwrap();
        assert!(!result.complete);
        assert_eq!(result.count, 0);
        fs::remove_file(&system).unwrap();
        fs::create_dir(&system).unwrap();
        assert!(!inventory(&fixture.home, &fixture.id).unwrap().complete);
        fs::remove_dir(&system).unwrap();
        let file = fs::File::create(&system).unwrap();
        file.set_len(MAX_REGISTRY_BYTES + 1).unwrap();
        assert!(!inventory(&fixture.home, &fixture.id).unwrap().complete);
        drop(file);
        fs::write(&system, [0xff, 0xfe]).unwrap();
        assert!(!inventory(&fixture.home, &fixture.id).unwrap().complete);
        fs::remove_file(&system).unwrap();
        fs::remove_dir(&prefix).unwrap();
        symlink(&fixture.home, &prefix).unwrap();
        assert!(inventory(&fixture.home, &fixture.id).is_err());
    }
}
