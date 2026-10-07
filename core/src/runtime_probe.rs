use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(test)]
use std::path::PathBuf;

#[derive(Serialize)]
pub struct RuntimeProbe {
    pub wine_path: String,
    pub version: String,
    pub file_description: String,
    pub wineboot_path: String,
    pub wineserver_present: bool,
    pub log_path: String,
}

pub fn probe(home: &Path, wine: &Path) -> Result<RuntimeProbe, String> {
    probe_with_timeout(home, wine, Duration::from_secs(10))
}

fn probe_with_timeout(home: &Path, wine: &Path, timeout: Duration) -> Result<RuntimeProbe, String> {
    let resolved_wine = wine.canonicalize().map_err(|e| e.to_string())?;
    let parent = wine.parent().ok_or("Wine path has no parent directory")?;
    let wineboot = parent.join("wineboot");
    let wineserver = parent.join("wineserver");
    ensure_real_directory(home)?;
    let logs = home.join("runtime-probes");
    ensure_real_directory(&logs)?;
    let log_path = logs.join(format!("wine-version-{}.log", Uuid::new_v4()));
    let output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let errors = output.try_clone().map_err(|e| e.to_string())?;
    let mut capture = output.try_clone().map_err(|e| e.to_string())?;

    let mut child = Command::new(wine)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|e| format!("Could not start Wine: {e}"))?;
    let status = wait_with_timeout(&mut child, timeout)?;
    const MAX_VERSION_OUTPUT: u64 = 64 * 1024;
    if capture.metadata().map_err(|e| e.to_string())?.len() > MAX_VERSION_OUTPUT {
        return Err(format!(
            "Wine version check produced too much output. Check {}",
            log_path.display()
        ));
    }
    let mut version_bytes = Vec::new();
    capture
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    capture
        .take(MAX_VERSION_OUTPUT)
        .read_to_end(&mut version_bytes)
        .map_err(|e| e.to_string())?;
    let version_output = String::from_utf8_lossy(&version_bytes);
    if !status.success() {
        return Err(format!(
            "Wine version check failed (status {status}). Check {}",
            log_path.display()
        ));
    }
    let version = version_output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("wine-"))
        .ok_or_else(|| {
            format!(
                "Wine did not report a version. Check {}",
                log_path.display()
            )
        })?
        .chars()
        .take(120)
        .collect();
    let description = Command::new("/usr/bin/file")
        .arg("-b")
        .arg(wine)
        .output()
        .map_err(|e| format!("Could not inspect Wine architecture: {e}"))?;
    if !description.status.success() {
        return Err("Could not inspect Wine architecture".into());
    }
    let file_description = String::from_utf8_lossy(&description.stdout)
        .trim()
        .chars()
        .take(240)
        .collect();
    Ok(RuntimeProbe {
        wine_path: resolved_wine.display().to_string(),
        version,
        file_description,
        wineboot_path: wineboot.display().to_string(),
        wineserver_present: wineserver.is_file(),
        log_path: log_path.display().to_string(),
    })
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_dir() {
        return Err(format!(
            "Runtime probe storage must be a real directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Wine version check timed out after {} seconds",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn probe_records_version_and_companion_paths() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build")
            .join(format!("runtime-probe-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let wine = home.join("wine");
        fs::write(&wine, b"#!/bin/sh\nprintf 'wine-11.0-test\\n'\n").unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        fs::write(home.join("wineboot"), b"#!/bin/sh\n").unwrap();
        let report = probe(&home, &wine).unwrap();
        assert_eq!(report.version, "wine-11.0-test");
        assert!(report.wineboot_path.ends_with("/wineboot"));
        assert!(Path::new(&report.log_path).is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn probe_times_out_stalled_runtime() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build")
            .join(format!("runtime-probe-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let wine = home.join("wine");
        fs::write(&wine, b"#!/bin/sh\nsleep 2\n").unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        assert!(probe_with_timeout(&home, &wine, Duration::from_millis(100)).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn probe_rejects_symlinked_log_directory() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build")
            .join(format!("runtime-probe-test-{}", Uuid::new_v4()));
        let outside = home.join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, home.join("runtime-probes")).unwrap();
        let wine = home.join("wine");
        fs::write(&wine, b"#!/bin/sh\nprintf 'wine-11.0-test\\n'\n").unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        assert!(probe(&home, &wine).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn probe_rejects_excessive_version_output() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.build")
            .join(format!("runtime-probe-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let wine = home.join("wine");
        fs::write(
            &wine,
            b"#!/bin/sh\n/usr/bin/awk 'BEGIN { for (i = 0; i < 70000; i++) printf \"x\"; print \"wine-11.0\"; }'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
        let error = probe(&home, &wine).err().unwrap();
        assert!(error.contains("too much output"), "{error}");
        fs::remove_dir_all(home).unwrap();
    }
}
