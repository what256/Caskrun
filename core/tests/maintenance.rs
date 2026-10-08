//! Exercise the maintenance command boundary without starting an engine.
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use uuid::Uuid;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("maintenance-{}", Uuid::new_v4()));
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

#[test]
fn maintenance_commands_preserve_programs_and_do_not_initialize_windows() {
    let fixture = Fixture::new();
    let first = fixture.json(&["create", "Private fixture name"]);
    let other = fixture.json(&["create", "Other fixture"]);
    let id = first["id"].as_str().unwrap();
    let dir = fixture.0.join("bottles").join(id);
    let exe = fixture.0.join("Example.exe");
    fs::write(&exe, b"private-program-sentinel").unwrap();
    let app = fixture.json(&[
        "add-app",
        id,
        "Private program name",
        exe.to_str().unwrap(),
        "--",
        " literal spaces ",
        "--example",
    ]);
    let app_id = app["id"].as_str().unwrap();
    fixture.json(&["set-app-visibility", id, app_id, "hide"]);
    let before: Value = fixture.json(&["apps", id]);
    assert!(
        !fixture
            .call(&[
                "rename-app",
                other["id"].as_str().unwrap(),
                app_id,
                "Wrong bottle"
            ])
            .status
            .success()
    );
    assert!(
        !fixture
            .call(&["rename-app", id, app_id, "\n"])
            .status
            .success()
    );
    assert_eq!(fixture.json(&["apps", id]), before);
    let renamed = fixture.json(&["rename-app", id, app_id, "  Friendly name  "]);
    assert_eq!(renamed["name"], "Friendly name");
    for field in [
        "id",
        "path",
        "created_at",
        "saved_arguments",
        "hidden_from_home",
    ] {
        assert_eq!(renamed[field], before[0][field]);
    }
    let record = fs::read(dir.join("bottle.json")).unwrap();
    let launchers = fs::read(dir.join("launchers.json")).unwrap();
    let report = fixture.json(&["diagnostics", id]);
    assert_eq!(report["format"], "caskrun-diagnostic-report");
    assert_eq!(report["bottle"]["saved_program_count"], 1);
    assert_eq!(report["bottle"]["visible_program_count"], 0);
    let text = serde_json::to_string(&report).unwrap();
    for forbidden in [
        id,
        "Private fixture name",
        "Friendly name",
        "literal spaces",
        fixture.0.to_str().unwrap(),
    ] {
        assert!(!text.contains(forbidden));
    }
    let inventory = fixture.json(&["installed-programs", id]);
    assert_eq!(inventory["initialized"], false);
    assert_eq!(inventory["count"], 0);
    fixture.json(&["rebuild-icons", id]);
    assert_eq!(fs::read(dir.join("bottle.json")).unwrap(), record);
    assert_eq!(fs::read(dir.join("launchers.json")).unwrap(), launchers);
    assert_eq!(fs::read(&exe).unwrap(), b"private-program-sentinel");
    assert!(!dir.join("prefix-record.json").exists());
    assert_eq!(fs::read_dir(dir.join("prefix")).unwrap().count(), 0);
}

#[test]
fn one_time_launch_validates_literal_arguments_before_initialization() {
    let fixture = Fixture::new();
    let bottle = fixture.json(&["create", "Run once fixture"]);
    let id = bottle["id"].as_str().unwrap();
    let exe = fixture.0.join("Example.exe");
    fs::write(&exe, b"inert").unwrap();
    let path = exe.to_str().unwrap();
    let without_separator = fixture.call(&["launch", id, path, "--example"]);
    assert!(!without_separator.status.success());
    assert!(String::from_utf8_lossy(&without_separator.stderr).contains("after --"));
    let oversized = fixture.call(&["launch", id, path, "--", &"x".repeat(4097)]);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("4096"));
    let valid = fixture.call(&["launch", id, path, "--", " literal spaces ", "$(literal)"]);
    assert!(!valid.status.success());
    assert!(String::from_utf8_lossy(&valid.stderr).contains("included Wine runtime"));
    let dir = fixture.0.join("bottles").join(id);
    assert!(!dir.join("prefix-record.json").exists());
    assert_eq!(fs::read_dir(dir.join("prefix")).unwrap().count(), 0);
    assert_eq!(fixture.json(&["apps", id]), serde_json::json!([]));
}
