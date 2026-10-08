use super::*;

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("bottle-archive-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        Self { home }
    }
    fn stage(&self) -> PathBuf {
        let path = self.home.join(Uuid::new_v4().to_string());
        fs::create_dir(&path).unwrap();
        path
    }
    fn snapshot(&self) -> PathBuf {
        let path = self.stage();
        fs::create_dir_all(path.join("payload/prefix/drive_c")).unwrap();
        let manifest = BackupManifest {
            format: BACKUP_FORMAT.into(),
            version: BACKUP_VERSION,
            bottle: Bottle {
                id: Uuid::new_v4().to_string(),
                name: "Example".into(),
                created_at: 1,
            },
            launchers: Vec::new(),
            host_folders: Vec::new(),
            profile_account: None,
        };
        save_json(&path.join("manifest.json"), &manifest).unwrap();
        fs::write(
            path.join("payload/prefix/drive_c/example.exe"),
            b"original example app",
        )
        .unwrap();
        path
    }
    fn integrity(&self, root: &Path) -> Integrity {
        Integrity {
            format: "caskrun-compressed-bottle".into(),
            version: 1,
            entries: scan_snapshot(root, LIMITS).unwrap(),
        }
    }
    fn unpack(&self, bytes: &[u8], limits: Limits) -> Result<(), String> {
        read_archive(bytes, &self.stage(), limits)
    }
    fn import(&self, bytes: &[u8]) -> Result<ImportResult, String> {
        let archive = self.home.join(format!("{}.caskrun-backup", Uuid::new_v4()));
        fs::write(&archive, bytes).unwrap();
        import_archive(&self.home.join("imported"), &archive, None)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn pack(root: &Path, integrity: &Integrity) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_archive(root, integrity, &mut bytes).unwrap();
    bytes
}

fn raw_archive(
    integrity: &Integrity,
    mut alter: impl FnMut(&Item, &mut tar::Header, &mut Vec<u8>),
) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let json = serde_json::to_vec(integrity).unwrap();
    tar.append_data(
        &mut header(tar::EntryType::Regular, json.len() as u64, 0o600),
        INTEGRITY,
        json.as_slice(),
    )
    .unwrap();
    for item in &integrity.entries {
        let (kind, data) = match &item.kind {
            Kind::Directory => (tar::EntryType::Directory, Vec::new()),
            Kind::File { .. } => (tar::EntryType::Regular, Vec::new()),
            Kind::Symlink { .. } => (tar::EntryType::Symlink, Vec::new()),
        };
        let mut header = header(kind, data.len() as u64, item.mode);
        let mut data = data;
        header.set_path(&item.path).unwrap();
        if let Kind::Symlink { target } = &item.kind {
            header.set_link_name(target).unwrap();
        }
        alter(item, &mut header, &mut data);
        header.set_cksum();
        tar.append(&header, data.as_slice()).unwrap();
    }
    let raw = tar.into_inner().unwrap();
    compress(&raw)
}
fn compress(raw: &[u8]) -> Vec<u8> {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(raw).unwrap();
    gzip.finish().unwrap()
}

fn raw_with_source(
    root: &Path,
    integrity: &Integrity,
    mut alter: impl FnMut(&Item, &mut tar::Header, &mut Vec<u8>),
) -> Vec<u8> {
    raw_archive(integrity, |item, header, bytes| {
        if matches!(item.kind, Kind::File { .. }) {
            *bytes = fs::read(root.join(&item.path)).unwrap();
            header.set_size(bytes.len() as u64);
        }
        alter(item, header, bytes);
    })
}

#[test]
fn archive_roundtrip_preserves_original_program_links_and_normal_modes() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    fs::create_dir(snapshot.join("payload/prefix/dosdevices")).unwrap();
    symlink("../drive_c", snapshot.join("payload/prefix/dosdevices/c:")).unwrap();
    symlink("/", snapshot.join("payload/prefix/dosdevices/z:")).unwrap();
    fs::set_permissions(
        snapshot.join("payload/prefix/drive_c/example.exe"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let integrity = fixture.integrity(&snapshot);
    let bytes = pack(&snapshot, &integrity);
    let output = fixture.stage();
    read_archive(bytes.as_slice(), &output, LIMITS).unwrap();
    assert_eq!(scan_snapshot(&output, LIMITS).unwrap(), integrity.entries);
    let imported = fixture.import(&bytes).unwrap();
    let bottle = bottle_dir(&fixture.home.join("imported"), &imported.bottle.id).unwrap();
    assert_eq!(
        fs::read(bottle.join("prefix/drive_c/example.exe")).unwrap(),
        b"original example app"
    );
    assert_eq!(
        fs::read_link(bottle.join("prefix/dosdevices/z:")).unwrap(),
        Path::new("/")
    );
    assert!(
        fs::read_dir(fixture.home.join("imported").join(STAGING))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn compressed_export_uses_existing_policy_and_publishes_without_overwriting() {
    let fixture = Fixture::new();
    let bottle = create(&fixture.home, "Example").unwrap();
    let directory = bottle_dir(&fixture.home, &bottle.id).unwrap();
    fs::write(directory.join("prefix/example.exe"), b"example program").unwrap();
    fs::create_dir(directory.join("native-apps")).unwrap();
    fs::write(
        directory.join("native-apps/foreign"),
        b"excluded generated alias",
    )
    .unwrap();
    let output = fixture.home.join("example.caskrun-backup");
    let result = export_archive(&fixture.home, &bottle.id, &output).unwrap();
    assert_eq!(result.path, output.display().to_string());
    let original = fs::read(&output).unwrap();
    assert!(export_archive(&fixture.home, &bottle.id, &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), original);
    let imported =
        import_archive(&fixture.home.join("restored"), &output, Some("Restored")).unwrap();
    assert_eq!(imported.bottle.name, "Restored");
    let restored = bottle_dir(&fixture.home.join("restored"), &imported.bottle.id).unwrap();
    assert!(!restored.join("native-apps").exists());
    assert_eq!(
        fs::read(restored.join("prefix/example.exe")).unwrap(),
        b"example program"
    );
    assert!(
        fs::read_dir(fixture.home.join(STAGING))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(
        export_archive(
            &fixture.home,
            &bottle.id,
            &directory.join("recursive.caskrun-backup")
        )
        .is_err()
    );
}

#[test]
fn file_tampering_and_truncation_refuse_before_bottle_storage_exists() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let tampered = raw_with_source(&snapshot, &integrity, |item, _, bytes| {
        if item.path.ends_with("example.exe") {
            bytes[0] ^= 1;
        }
    });
    assert!(
        fixture
            .import(&tampered)
            .err()
            .unwrap()
            .contains("checksum")
    );
    assert!(!fixture.home.join("imported/bottles").exists());
    let mut truncated = pack(&snapshot, &integrity);
    truncated.truncate(truncated.len() / 2);
    assert!(fixture.import(&truncated).is_err());
    assert!(!fixture.home.join("imported/bottles").exists());
    assert!(
        fs::read_dir(fixture.home.join("imported").join(STAGING))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn integrity_manifest_refuses_duplicate_traversal_absolute_and_missing_parent_paths() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let original = fixture.integrity(&snapshot);
    for invalid in [
        "../escape",
        "/absolute",
        "payload/../escape",
        "payload//prefix",
        "payload/./prefix",
        "payload/unknown/file",
    ] {
        let mut integrity = original.clone();
        integrity.entries.last_mut().unwrap().path = invalid.into();
        assert!(validate_integrity(&integrity, LIMITS).is_err(), "{invalid}");
    }
    let mut integrity = original.clone();
    integrity
        .entries
        .push(integrity.entries.last().unwrap().clone());
    assert!(validate_integrity(&integrity, LIMITS).is_err());
    let mut integrity = original;
    integrity
        .entries
        .retain(|item| item.path != "payload/prefix");
    assert!(validate_integrity(&integrity, LIMITS).is_err());
}

#[test]
fn raw_headers_cannot_override_paths_or_repeat_a_manifest_entry() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    for invalid in ["../escape", "/absolute", "manifest.json"] {
        let bytes = raw_with_source(&snapshot, &integrity, |item, header, _| {
            if item.path.ends_with("example.exe") {
                header.as_mut_bytes()[..100].fill(0);
                header.as_mut_bytes()[..invalid.len()].copy_from_slice(invalid.as_bytes());
            }
        });
        assert!(fixture.unpack(&bytes, LIMITS).is_err());
        assert!(!fixture.home.join("escape").exists());
    }
}

#[test]
fn all_hardlink_sparse_device_pax_and_unknown_types_are_refused() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    for kind in [
        tar::EntryType::Link,
        tar::EntryType::GNUSparse,
        tar::EntryType::Char,
        tar::EntryType::Block,
        tar::EntryType::Fifo,
        tar::EntryType::XHeader,
        tar::EntryType::XGlobalHeader,
        tar::EntryType::Continuous,
    ] {
        let bytes = raw_with_source(&snapshot, &integrity, |item, header, _| {
            if item.path.ends_with("example.exe") {
                header.set_entry_type(kind);
            }
        });
        assert!(fixture.unpack(&bytes, LIMITS).is_err(), "{kind:?}");
    }
}

#[test]
fn unsafe_links_and_link_as_output_parent_are_refused_before_extraction() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let original = fixture.integrity(&snapshot);
    for target in [
        "/private",
        "../../../../outside",
        "../dosdevices/z:/private",
    ] {
        let mut integrity = original.clone();
        integrity.entries.last_mut().unwrap().kind = Kind::Symlink {
            target: target.into(),
        };
        assert!(validate_integrity(&integrity, LIMITS).is_err());
    }
    let mut integrity = original;
    integrity
        .entries
        .iter_mut()
        .find(|item| item.path == "payload/prefix/drive_c")
        .unwrap()
        .kind = Kind::Symlink {
        target: "prefix".into(),
    };
    assert!(validate_integrity(&integrity, LIMITS).is_err());
}

#[test]
fn gzip_crc_trailing_bytes_and_additional_members_are_checked() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let original = pack(&snapshot, &integrity);
    let mut crc = original.clone();
    let index = crc.len() - 8;
    crc[index] ^= 0x80;
    assert!(fixture.unpack(&crc, LIMITS).is_err());
    let mut trailing = original.clone();
    trailing.extend_from_slice(b"unexpected trailing bytes");
    assert!(fixture.unpack(&trailing, LIMITS).is_err());
    let mut members = original;
    members.extend(compress(b""));
    assert!(fixture.unpack(&members, LIMITS).is_err());
}

#[test]
fn omitted_extra_and_reordered_entries_fail_complete_set_validation() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let original = pack(&snapshot, &integrity);
    let mut raw = Vec::new();
    GzDecoder::new(original.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let mut tar = tar::Archive::new(raw.as_slice());
    let ranges = tar
        .entries()
        .unwrap()
        .raw(true)
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.raw_header_position() as usize,
                (entry.raw_file_position() + entry.size().div_ceil(512) * 512) as usize,
            )
        })
        .collect::<Vec<_>>();
    let (start, end) = *ranges.last().unwrap();
    let mut omitted = raw.clone();
    omitted.drain(start..end);
    assert!(
        fixture
            .unpack(&compress(&omitted), LIMITS)
            .unwrap_err()
            .contains("missing entries")
    );
    let mut extra = raw.clone();
    extra.splice(end..end, raw[start..end].iter().copied());
    assert!(fixture.unpack(&compress(&extra), LIMITS).is_err());
    let (first, first_end) = ranges[2];
    let (second, second_end) = ranges[3];
    assert_eq!(first_end - first, 512);
    assert_eq!(second_end - second, 512);
    let mut reordered = raw.clone();
    reordered[first..first_end].copy_from_slice(&raw[second..second_end]);
    reordered[second..second_end].copy_from_slice(&raw[first..first_end]);
    assert!(fixture.unpack(&compress(&reordered), LIMITS).is_err());
    let tiny = Limits {
        entries: 2,
        ..LIMITS
    };
    assert!(fixture.unpack(&original, tiny).is_err());
    let mut integrity = fixture.integrity(&snapshot);
    integrity.entries.swap(0, 1);
    assert!(validate_integrity(&integrity, LIMITS).is_err());
}

#[test]
fn optional_unbounded_gzip_header_metadata_is_refused_before_decoding() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let original = pack(&snapshot, &integrity);
    let mut raw = Vec::new();
    GzDecoder::new(original.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let mut gzip = flate2::GzBuilder::new()
        .filename("unexpected-source-name")
        .write(Vec::new(), Compression::default());
    gzip.write_all(&raw).unwrap();
    let bytes = gzip.finish().unwrap();
    assert!(
        fixture
            .unpack(&bytes, LIMITS)
            .unwrap_err()
            .contains("Unsupported gzip header")
    );
}

#[test]
fn extraction_never_follows_a_preexisting_output_link() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let bytes = pack(&snapshot, &integrity);
    let output = fixture.stage();
    let outside = fixture.stage();
    fs::write(outside.join("keep"), b"untouched").unwrap();
    symlink(&outside, output.join("payload")).unwrap();
    assert!(read_archive(bytes.as_slice(), &output, LIMITS).is_err());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched");
    assert!(!outside.join("prefix").exists());
}

#[test]
fn decompression_metadata_file_and_total_limits_are_enforced() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let integrity = fixture.integrity(&snapshot);
    let bytes = pack(&snapshot, &integrity);
    for limits in [
        Limits { file: 4, ..LIMITS },
        Limits { total: 4, ..LIMITS },
        Limits {
            metadata: 4,
            ..LIMITS
        },
        Limits {
            entries: 1,
            ..LIMITS
        },
    ] {
        assert!(fixture.unpack(&bytes, limits).is_err());
    }
    let mut raw = Vec::new();
    let gzip = GzDecoder::new(bytes.as_slice());
    let mut gzip = gzip;
    gzip.read_to_end(&mut raw).unwrap();
    raw.extend(vec![0; MAX_TRAILER as usize + 1024]);
    assert!(fixture.unpack(&compress(&raw), LIMITS).is_err());
}

#[test]
fn unsafe_permission_bits_and_unknown_schema_fields_are_refused() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let original = fixture.integrity(&snapshot);
    for mode in [0o4755, 0o2755, 0o1755, 0o000] {
        let mut integrity = original.clone();
        integrity.entries[0].mode = mode;
        assert!(validate_integrity(&integrity, LIMITS).is_err());
    }
    let mut value = serde_json::to_value(original).unwrap();
    value["unexpected"] = true.into();
    assert!(serde_json::from_value::<Integrity>(value).is_err());
}

#[test]
fn safe_long_unicode_paths_and_relative_symlinks_roundtrip() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let long = "a".repeat(160);
    let directory = snapshot.join("payload/prefix/drive_c").join(&long);
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("application-λ.exe"), b"example").unwrap();
    symlink(
        format!("{long}/application-λ.exe"),
        snapshot.join("payload/prefix/drive_c/λ-link"),
    )
    .unwrap();
    let integrity = fixture.integrity(&snapshot);
    let bytes = pack(&snapshot, &integrity);
    let output = fixture.stage();
    read_archive(bytes.as_slice(), &output, LIMITS).unwrap();
    assert_eq!(scan_snapshot(&output, LIMITS).unwrap(), integrity.entries);
}

#[test]
fn failed_import_keeps_folder_backup_validation_and_cleans_readonly_staging() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(snapshot.join("manifest.json")).unwrap()).unwrap();
    manifest["version"] = 999.into();
    fs::write(
        snapshot.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        snapshot.join("payload/prefix/drive_c"),
        fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    let integrity = fixture.integrity(&snapshot);
    let bytes = pack(&snapshot, &integrity);
    assert!(
        fixture
            .import(&bytes)
            .err()
            .unwrap()
            .contains("Unsupported Caskrun backup")
    );
    assert!(!fixture.home.join("imported/bottles").exists());
    assert!(
        fs::read_dir(fixture.home.join("imported").join(STAGING))
            .unwrap()
            .next()
            .is_none()
    );
    fs::set_permissions(
        snapshot.join("payload/prefix/drive_c"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
}

#[test]
fn export_refuses_a_tracked_running_bottle_before_creating_staging_or_output() {
    let fixture = Fixture::new();
    let bottle = create(&fixture.home, "Example").unwrap();
    let dir = bottle_dir(&fixture.home, &bottle.id).unwrap();
    save_json(
        &dir.join("active-processes.json"),
        &vec![std::process::id()],
    )
    .unwrap();
    let output = fixture.home.join("running.caskrun-backup");
    assert!(
        export_archive(&fixture.home, &bottle.id, &output)
            .err()
            .unwrap()
            .contains("Close Windows apps")
    );
    assert!(!output.exists());
    assert!(!fixture.home.join(STAGING).exists());
}

#[test]
fn archive_does_not_expand_an_excessive_folder_manifest_or_follow_input_links() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let mut integrity = fixture.integrity(&snapshot);
    let Kind::File { size, .. } = &mut integrity.entries[0].kind else {
        panic!("fixture manifest missing")
    };
    *size = 4 * 1024 * 1024 + 1;
    assert!(validate_integrity(&integrity, LIMITS).is_err());
    let input = fixture.home.join("input.caskrun-backup");
    symlink(snapshot.join("manifest.json"), &input).unwrap();
    assert!(import_archive(&fixture.home.join("imported"), &input, None).is_err());
    assert!(!fixture.home.join("imported").exists());
}
