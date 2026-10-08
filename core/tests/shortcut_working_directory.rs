//! Complete .lnk command discovery and save across the real CLI boundary.
//! Original inert PE/link fixtures; no Windows engine is started.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uuid::Uuid;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("shortcut-command-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn call(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_caskrun-core"))
            .args(arguments)
            .env("CASKRUN_HOME", &self.0)
            .env("CASKRUN_WINE", self.0.join("absent-engine/wine"))
            .env_remove("CASKRUN_CANCEL_STDIN")
            .output()
            .unwrap()
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.call(arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn pe(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = vec![0u8; 0x90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

fn shortcut(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut link = vec![0u8; 76];
    link[..4].copy_from_slice(&76u32.to_le_bytes());
    link[4..20].copy_from_slice(&[1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]);
    link[20..24].copy_from_slice(&(2u32 | 16 | 32 | 128).to_le_bytes());
    let mut info = vec![0u8; 28];
    info[4..8].copy_from_slice(&28u32.to_le_bytes());
    info[8..12].copy_from_slice(&1u32.to_le_bytes());
    info[12..16].copy_from_slice(&28u32.to_le_bytes());
    let mut volume = vec![0u8; 17];
    volume[..4].copy_from_slice(&17u32.to_le_bytes());
    volume[4..8].copy_from_slice(&3u32.to_le_bytes());
    volume[12..16].copy_from_slice(&16u32.to_le_bytes());
    info.extend(volume);
    info[16..20].copy_from_slice(&45u32.to_le_bytes());
    info.extend_from_slice(b"C:\\Program Files\\Atlas\\Atlas.exe\0");
    let suffix = info.len() as u32;
    info[24..28].copy_from_slice(&suffix.to_le_bytes());
    info.push(0);
    let size = info.len() as u32;
    info[..4].copy_from_slice(&size.to_le_bytes());
    link.extend(info);
    for value in ["C:\\Games\\Profile One", "--profile \"two words\""] {
        let units: Vec<_> = value.encode_utf16().collect();
        link.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            link.extend_from_slice(&unit.to_le_bytes());
        }
    }
    fs::write(path, link).unwrap();
}

#[test]
fn cli_saves_the_whole_shortcut_command_and_legacy_forms_still_work() {
    let fixture = Fixture::new();
    let bottle = fixture.json(&["create", "Shortcut fixture"]);
    let id = bottle["id"].as_str().unwrap();
    let dir = fixture.0.join("bottles").join(id);
    let exe = dir.join("prefix/drive_c/Program Files/Atlas/Atlas.exe");
    pe(&exe);
    fs::create_dir_all(dir.join("prefix/drive_c/Games/Profile One")).unwrap();
    shortcut(&dir.join("prefix/drive_c/users/Public/Desktop/Atlas.lnk"));
    let discovered = fixture.json(&["discover-apps", id]);
    let candidates = discovered["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate["working_directory"], "drive_c/Games/Profile One");
    assert_eq!(
        candidate["arguments"],
        serde_json::json!(["--profile", "two words"])
    );
    let saved = fixture.json(&[
        "add-app",
        id,
        "Atlas",
        exe.to_str().unwrap(),
        "--working-directory",
        candidate["working_directory"].as_str().unwrap(),
        "--",
        "--profile",
        "two words",
    ]);
    assert_eq!(saved["working_directory"], candidate["working_directory"]);
    assert_eq!(saved["saved_arguments"], candidate["arguments"]);
    let before = fs::read(dir.join("launchers.json")).unwrap();
    let refused = fixture.call(&[
        "add-app",
        id,
        "Unsafe",
        exe.to_str().unwrap(),
        "--working-directory",
        "drive_c/../outside",
    ]);
    assert!(!refused.status.success());
    assert_eq!(fs::read(dir.join("launchers.json")).unwrap(), before);
    assert!(!dir.join("prefix-record.json").exists());
    assert!(!dir.join("prefix/system.reg").exists());
    let ordinary = fixture.json(&["add-app", id, "Ordinary", exe.to_str().unwrap()]);
    assert!(ordinary.get("working_directory").is_none());
    let old_arguments = fixture.json(&[
        "add-app",
        id,
        "Old options",
        exe.to_str().unwrap(),
        "--",
        "-safe",
    ]);
    assert!(old_arguments.get("working_directory").is_none());
    assert_eq!(
        old_arguments["saved_arguments"],
        serde_json::json!(["-safe"])
    );
}
