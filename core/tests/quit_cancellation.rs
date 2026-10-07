//! CLI regressions for the host's Quit cancellation channel. These fixtures
//! have no initialized Wine prefix and never launch a Windows runtime.

use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use uuid::Uuid;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("quit-cancellation-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_caskrun-core"));
        command
            .env("CASKRUN_HOME", &self.0)
            // An explicit absent engine prevents fallback to an installed Wine.
            .env("CASKRUN_WINE", self.0.join("absent-runtime/wine"))
            .env_remove("CASKRUN_CANCEL_STDIN");
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct RunningCore(Option<Child>);

impl Drop for RunningCore {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[test]
fn quit_eof_cancels_a_held_bottle_lock_without_touching_the_prefix() {
    let fixture = Fixture::new();
    let created = fixture
        .command()
        .args(["create", "Quit lock test"])
        .output()
        .unwrap();
    assert!(created.status.success());
    let bottle: Value = serde_json::from_slice(&created.stdout).unwrap();
    let dir = fixture
        .0
        .join("bottles")
        .join(bottle["id"].as_str().unwrap());
    let bottle_before = fs::read(dir.join("bottle.json")).unwrap();
    let lock_path = dir.join(".caskrun.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    let mut command = fixture.command();
    command
        .arg("quit-all-bottles")
        .env("CASKRUN_CANCEL_STDIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut running = RunningCore(Some(command.spawn().unwrap()));
    std::thread::sleep(Duration::from_millis(300));
    assert!(running.0.as_mut().unwrap().try_wait().unwrap().is_none());

    // The host closes its own pipe rather than killing a saved process ID.
    drop(running.0.as_mut().unwrap().stdin.take());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if running.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Quit did not observe cancellation while waiting for the bottle lock"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = running.0.take().unwrap().wait_with_output().unwrap();
    if output.status.success() {
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["stopped_bottles"], 0);
        assert_eq!(result["failures"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"][0]["error"], "Setup canceled");
    } else {
        // A slow process may observe EOF at run()'s initial cancellation check.
        assert_eq!(output.status.code(), Some(130));
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("Setup canceled"));
    }
    assert_eq!(fs::read(dir.join("bottle.json")).unwrap(), bottle_before);
    assert_eq!(fs::read_dir(dir.join("prefix")).unwrap().count(), 0);
    assert!(!dir.join("prefix-record.json").exists());
    let second_lock = File::options()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    assert!(matches!(
        second_lock.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(second_lock);
    drop(lock);
}

#[test]
fn ordinary_quit_with_closed_stdin_does_not_opt_into_cancellation() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .arg("quit-all-bottles")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["complete"], true);
    assert_eq!(result["stopped_bottles"], 0);
    assert!(result["failures"].as_array().unwrap().is_empty());
}
