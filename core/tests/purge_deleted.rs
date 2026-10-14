//! Exercise the permanent-deletion CLI boundary with disposable storage only.
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
            .join(format!("purge-cli-{}", Uuid::new_v4()));
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
fn permanent_deletion_requires_recently_deleted_and_returns_removed_identity() {
    let fixture = Fixture::new();
    let first = fixture.json(&["create", "Disposable Steam fixture"]);
    let neighbor = fixture.json(&["create", "Neighbor fixture"]);
    let id = first["id"].as_str().unwrap();
    let other_id = neighbor["id"].as_str().unwrap();
    let active = fixture.0.join("bottles").join(id);
    fs::write(active.join("prefix/saved-file"), b"fixture contents").unwrap();
    assert!(!fixture.call(&["purge-deleted", id]).status.success());
    assert!(active.join("prefix/saved-file").is_file());
    assert_eq!(fixture.json(&["delete", id]), first);
    let deleted = fixture.0.join("deleted-bottles").join(id);
    assert!(deleted.join("prefix/saved-file").is_file());
    assert_eq!(fixture.json(&["purge-deleted", id]), first);
    assert!(!deleted.exists());
    assert!(fixture.json(&["deleted"]).as_array().unwrap().is_empty());
    assert!(
        fixture
            .0
            .join("bottles")
            .join(other_id)
            .join("bottle.json")
            .is_file()
    );
    assert!(!fixture.call(&["purge-deleted", id]).status.success());
    assert!(
        !fixture
            .call(&["purge-deleted", "../outside"])
            .status
            .success()
    );
}
