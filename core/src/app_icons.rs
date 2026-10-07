//! Read an installed Windows program's own PE icon. No publisher artwork is
//! bundled with Caskrun; the cached ICO is derived from the user's EXE.

use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::Path;
use uuid::Uuid;

use crate::{
    O_NOFOLLOW_ANY, bottle_dir, is_independent_runtime, list_launchers, native_app_identity,
    same_snapshot,
};

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const MAX_SECTIONS: usize = 96;
const MAX_DIRECTORY_ENTRIES: usize = 512;
const MAX_GROUP_ENTRIES: usize = 64;
const MAX_ICON_RESOURCE: usize = 8 * 1024 * 1024;
const MAX_ICO: usize = 16 * 1024 * 1024;

/// Bounded metadata from a program already opened by the native identity
/// preparer. No code is executed and no host-file cache is read here.
pub(crate) struct ExecutableArtwork {
    pub(crate) display_name: Option<String>,
    pub(crate) generic_installer: bool,
    pub(crate) ico: Option<Vec<u8>>,
}

pub(crate) fn executable_artwork(file: &File, length: u64) -> ExecutableArtwork {
    let Ok(mut reader) = PeIconReader::new(file, length) else {
        return ExecutableArtwork {
            display_name: None,
            generic_installer: false,
            ico: None,
        };
    };
    let names = reader
        .leaf(16, None)
        .ok()
        .flatten()
        .and_then(|bytes| version_names(&bytes))
        .unwrap_or([None, None]);
    let generic_installer = names[0].as_deref().is_some_and(generic_installer_name);
    let display_name = preferred_version_name(names);
    let ico = reader.first_icon().ok().flatten();
    ExecutableArtwork {
        display_name,
        generic_installer,
        ico,
    }
}

// Parse VERSIONINFO's bounded, aligned tree. Reading arbitrary UTF-16 strings
// from a program could mistake unrelated resource data for its display name.
fn version_names(bytes: &[u8]) -> Option<[Option<String>; 2]> {
    fn align4(value: usize) -> Option<usize> {
        value.checked_add(3).map(|v| v & !3)
    }
    fn visit(
        bytes: &[u8],
        offset: usize,
        limit: usize,
        depth: usize,
        budget: &mut usize,
        names: &mut [Option<String>; 2],
    ) -> Option<usize> {
        if depth > 8 || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let length = u16_le(bytes, offset).ok()? as usize;
        let value_length = u16_le(bytes, offset.checked_add(2)?).ok()? as usize;
        let kind = u16_le(bytes, offset.checked_add(4)?).ok()?;
        let end = offset.checked_add(length)?;
        if length < 8 || end > limit || end > bytes.len() {
            return None;
        }
        let mut cursor = offset.checked_add(6)?;
        let mut key = Vec::new();
        loop {
            if cursor.checked_add(2)? > end || key.len() > 128 {
                return None;
            }
            let character = u16_le(bytes, cursor).ok()?;
            cursor += 2;
            if character == 0 {
                break;
            }
            key.push(character);
        }
        let key = String::from_utf16(&key).ok()?;
        let value_start = align4(cursor)?;
        let value_bytes = value_length.checked_mul(if kind == 1 { 2 } else { 1 })?;
        let value_end = value_start.checked_add(value_bytes)?;
        if value_end > end {
            return None;
        }
        let name_index = match key.as_str() {
            "FileDescription" => Some(0),
            "ProductName" => Some(1),
            _ => None,
        };
        if let Some(index) = name_index.filter(|_| kind == 1 && value_length <= 256) {
            let characters: Vec<u16> = bytes[value_start..value_end]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|character| *character != 0)
                .collect();
            names[index] = String::from_utf16(&characters)
                .ok()
                .filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control));
        }
        cursor = align4(value_end)?;
        while cursor.checked_add(8)? <= end {
            let child_end = visit(bytes, cursor, end, depth + 1, budget, names)?;
            let next = align4(child_end)?;
            if next <= cursor {
                return None;
            }
            cursor = next;
        }
        Some(end)
    }
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let mut names = [None, None];
    visit(bytes, 0, bytes.len(), 0, &mut 512, &mut names)?;
    Some(names)
}

fn generic_installer_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '/')
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "setupuninstall" | "setup" | "installer"
    )
}

fn preferred_version_name(mut names: [Option<String>; 2]) -> Option<String> {
    names[0]
        .take()
        .filter(|name| !generic_installer_name(name))
        .or_else(|| names[1].take().filter(|name| !generic_installer_name(name)))
}

#[cfg(test)]
fn version_display_name(bytes: &[u8]) -> Option<String> {
    preferred_version_name(version_names(bytes)?)
}

#[derive(Serialize)]
pub(crate) struct IconResult {
    pub(crate) path: Option<String>,
}

pub(crate) fn app_icon(
    home: &Path,
    bottle_id: &str,
    launcher_id: &str,
) -> Result<IconResult, String> {
    let bottle = bottle_dir(home, bottle_id)?;
    let launcher = list_launchers(home, bottle_id)?
        .into_iter()
        .find(|item| item.id == launcher_id)
        .ok_or("Launcher not found in this bottle")?;
    if let Some(path) = native_app_identity::cached_icon(home, bottle_id, Path::new(&launcher.path))
    {
        return Ok(IconResult {
            path: Some(path.display().to_string()),
        });
    }
    extract_for_launcher(&bottle, launcher_id, Path::new(&launcher.path))
}

pub(crate) fn bottle_icon(home: &Path, bottle_id: &str) -> Result<IconResult, String> {
    let bottle = bottle_dir(home, bottle_id)?;
    // A bottle may contain a removed app or an EXE without icon resources.
    // Prefer the first installed program that can provide its own artwork.
    for launcher in list_launchers(home, bottle_id)?.into_iter().take(64) {
        if let Some(path) =
            native_app_identity::cached_icon(home, bottle_id, Path::new(&launcher.path))
        {
            return Ok(IconResult {
                path: Some(path.display().to_string()),
            });
        }
        let icon = extract_for_launcher(&bottle, &launcher.id, Path::new(&launcher.path))?;
        if icon.path.is_some() {
            return Ok(icon);
        }
    }
    Ok(IconResult { path: None })
}

fn extract_for_launcher(
    bottle: &Path,
    launcher_id: &str,
    exe: &Path,
) -> Result<IconResult, String> {
    if Uuid::parse_str(launcher_id).is_err() || !exe.is_absolute() || !is_independent_runtime(exe) {
        return Ok(IconResult { path: None });
    }
    // A saved launcher may have been removed, replaced, or imported from
    // another Mac. Such an app simply has no icon until its EXE is available.
    let before = match fs::symlink_metadata(exe) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        _ => return Ok(IconResult { path: None }),
    };
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(exe)
    {
        Ok(file) => file,
        Err(_) => return Ok(IconResult { path: None }),
    };
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Ok(IconResult { path: None });
    }
    let ico = PeIconReader::new(&file, before.len())
        .and_then(|mut reader| reader.first_icon())
        .ok()
        .flatten();
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Ok(IconResult { path: None });
    }
    let Some(ico) = ico else {
        return Ok(IconResult { path: None });
    };
    // This converts bounded data only, without creating native .app aliases
    // or starting Wine. Older engines and safe renderer failures retain the
    // publisher ICO fallback below.
    let rendered = native_app_identity::rendered_icon(bottle, &ico);
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !fs::symlink_metadata(exe).is_ok_and(|after| same_snapshot(&before, &after))
    {
        return Ok(IconResult { path: None });
    }
    if let Some(path) = rendered {
        return Ok(IconResult {
            path: Some(path.display().to_string()),
        });
    }
    let artwork = bottle.join("artwork");
    match fs::create_dir(&artwork) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("Could not create artwork cache: {error}")),
    }
    if !fs::symlink_metadata(&artwork).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err("Artwork cache must be a real directory".into());
    }
    let output = artwork.join(format!("{launcher_id}.ico"));
    if cached_icon_matches(&output, &ico) {
        return Ok(IconResult {
            path: Some(output.display().to_string()),
        });
    }
    let temp = artwork.join(format!(".{}.tmp", Uuid::new_v4()));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&ico).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, &output).map_err(|e| e.to_string())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result?;
    Ok(IconResult {
        path: Some(output.display().to_string()),
    })
}

fn cached_icon_matches(path: &Path, expected: &[u8]) -> bool {
    let Ok(before) = fs::symlink_metadata(path) else {
        return false;
    };
    if !before.file_type().is_file() || before.len() != expected.len() as u64 {
        return false;
    }
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY | 0x4)
        .open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|opened| same_snapshot(&before, &opened))
    {
        return false;
    }
    let mut existing = Vec::with_capacity(expected.len());
    if Read::by_ref(&mut file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut existing)
        .is_err()
    {
        return false;
    }
    existing == expected
        && file
            .metadata()
            .is_ok_and(|after| same_snapshot(&before, &after))
}

#[derive(Clone, Copy)]
struct Section {
    rva: u32,
    raw_offset: u32,
    raw_size: u32,
}

struct PeIconReader<'a> {
    file: &'a File,
    length: u64,
    sections: Vec<Section>,
    resource_rva: u32,
    resource_size: u32,
}

impl<'a> PeIconReader<'a> {
    fn new(file: &'a File, length: u64) -> Result<Self, String> {
        let dos = read_at(file, length, 0, 64)?;
        if &dos[..2] != b"MZ" {
            return Err("Not a PE executable".into());
        }
        let pe_offset = u32_le(&dos, 60)? as u64;
        let coff = read_at(file, length, pe_offset, 24)?;
        if &coff[..4] != b"PE\0\0" {
            return Err("Invalid PE header".into());
        }
        let section_count = u16_le(&coff, 6)? as usize;
        let optional_size = u16_le(&coff, 20)? as usize;
        if section_count == 0 || section_count > MAX_SECTIONS || optional_size > 4096 {
            return Err("PE headers exceed icon scan limits".into());
        }
        let optional_offset = pe_offset.checked_add(24).ok_or("PE offset overflow")?;
        let optional = read_at(file, length, optional_offset, optional_size)?;
        let resource_entry = match u16_le(&optional, 0)? {
            0x10b => 112,
            0x20b => 128,
            _ => return Err("Unsupported PE optional header".into()),
        };
        let resource_rva = u32_le(&optional, resource_entry)?;
        let resource_size = u32_le(&optional, resource_entry + 4)?;
        if resource_rva == 0 || resource_size == 0 || resource_size as usize > MAX_ICO * 8 {
            return Err("PE has no bounded resources".into());
        }
        let section_offset = optional_offset
            .checked_add(optional_size as u64)
            .ok_or("PE section offset overflow")?;
        let raw_sections = read_at(file, length, section_offset, section_count * 40)?;
        let mut sections = Vec::with_capacity(section_count);
        for entry in raw_sections.chunks_exact(40) {
            sections.push(Section {
                rva: u32_le(entry, 12)?,
                raw_offset: u32_le(entry, 20)?,
                raw_size: u32_le(entry, 16)?,
            });
        }
        Ok(Self {
            file,
            length,
            sections,
            resource_rva,
            resource_size,
        })
    }

    fn file_offset(&self, rva: u32, length: usize) -> Result<u64, String> {
        for section in &self.sections {
            let Some(relative) = rva.checked_sub(section.rva) else {
                continue;
            };
            let Some(end) = relative.checked_add(length as u32) else {
                continue;
            };
            if end <= section.raw_size {
                return Ok(section.raw_offset as u64 + relative as u64);
            }
        }
        Err("Resource lies outside PE sections".into())
    }

    fn read_resource(&self, relative: u32, length: usize) -> Result<Vec<u8>, String> {
        let end = (relative as u64)
            .checked_add(length as u64)
            .ok_or("Resource offset overflow")?;
        if end > self.resource_size as u64 || length > MAX_ICON_RESOURCE {
            return Err("Resource exceeds icon scan limits".into());
        }
        let rva = self
            .resource_rva
            .checked_add(relative)
            .ok_or("Resource RVA overflow")?;
        read_at(
            self.file,
            self.length,
            self.file_offset(rva, length)?,
            length,
        )
    }

    fn entries(&self, relative: u32) -> Result<Vec<(u32, u32)>, String> {
        let dir = self.read_resource(relative, 16)?;
        let count = u16_le(&dir, 12)? as usize + u16_le(&dir, 14)? as usize;
        if count > MAX_DIRECTORY_ENTRIES {
            return Err("PE icon directory has too many entries".into());
        }
        let entries = self.read_resource(
            relative.checked_add(16).ok_or("Directory overflow")?,
            count * 8,
        )?;
        entries
            .chunks_exact(8)
            .map(|entry| Ok((u32_le(entry, 0)?, u32_le(entry, 4)?)))
            .collect()
    }

    fn leaf(&self, type_id: u32, resource_id: Option<u32>) -> Result<Option<Vec<u8>>, String> {
        let Some((_, type_pointer)) = self.entries(0)?.into_iter().find(|(id, _)| *id == type_id)
        else {
            return Ok(None);
        };
        if type_pointer & 0x8000_0000 == 0 {
            return Ok(None);
        }
        let names = self.entries(type_pointer & 0x7fff_ffff)?;
        for (id, name_pointer) in names {
            if resource_id.is_some_and(|wanted| id != wanted) || name_pointer & 0x8000_0000 == 0 {
                continue;
            }
            for (_, language_pointer) in self.entries(name_pointer & 0x7fff_ffff)? {
                if language_pointer & 0x8000_0000 != 0 {
                    continue;
                }
                let entry = self.read_resource(language_pointer, 16)?;
                let data_rva = u32_le(&entry, 0)?;
                let data_size = u32_le(&entry, 4)? as usize;
                if data_size == 0 || data_size > MAX_ICON_RESOURCE {
                    continue;
                }
                return read_at(
                    self.file,
                    self.length,
                    self.file_offset(data_rva, data_size)?,
                    data_size,
                )
                .map(Some);
            }
        }
        Ok(None)
    }

    fn first_icon(&mut self) -> Result<Option<Vec<u8>>, String> {
        let Some(group) = self.leaf(RT_GROUP_ICON, None)? else {
            return Ok(None);
        };
        if group.len() < 6 || u16_le(&group, 0)? != 0 || u16_le(&group, 2)? != 1 {
            return Ok(None);
        }
        let count = u16_le(&group, 4)? as usize;
        if count == 0 || count > MAX_GROUP_ENTRIES || group.len() < 6 + count * 14 {
            return Ok(None);
        }
        let mut selected = Vec::new();
        for entry in group[6..6 + count * 14].chunks_exact(14) {
            let id = u16_le(entry, 12)? as u32;
            let Some(data) = self.leaf(RT_ICON, Some(id))? else {
                continue;
            };
            if data.len() > MAX_ICON_RESOURCE {
                continue;
            }
            selected.push((entry, data));
        }
        if selected.is_empty() {
            return Ok(None);
        }
        let total = selected
            .iter()
            .try_fold(6 + selected.len() * 16, |sum, (_, data)| {
                sum.checked_add(data.len())
                    .filter(|end| *end <= MAX_ICO)
                    .ok_or("Icon exceeds cache limit")
            })?;
        let mut ico = Vec::with_capacity(total);
        ico.extend_from_slice(&[0, 0, 1, 0]);
        ico.extend_from_slice(&(selected.len() as u16).to_le_bytes());
        let mut offset = 6 + selected.len() * 16;
        for (entry, data) in &selected {
            ico.extend_from_slice(&entry[..8]); // dimensions, colors, planes, bit depth
            ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
            ico.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += data.len();
        }
        for (_, data) in selected {
            ico.extend_from_slice(&data);
        }
        Ok(Some(ico))
    }
}

fn read_at(file: &File, file_length: u64, offset: u64, size: usize) -> Result<Vec<u8>, String> {
    let end = offset
        .checked_add(size as u64)
        .ok_or("PE offset overflow")?;
    if end > file_length || size > MAX_ICON_RESOURCE {
        return Err("PE icon data is outside the executable".into());
    }
    let mut data = vec![0; size];
    let mut used = 0;
    while used < size {
        let n = file
            .read_at(&mut data[used..], offset + used as u64)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Truncated PE data".into());
        }
        used += n;
    }
    Ok(data)
}

fn u16_le(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data.get(offset..offset + 2).ok_or("Truncated PE field")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_le(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data.get(offset..offset + 4).ok_or("Truncated PE field")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
pub(crate) fn fixture_pe_with_version(description: &str, product: &str) -> Vec<u8> {
    tests::pe_with_version(description, product)
}

#[cfg(test)]
pub(crate) fn fixture_pe_with_icon() -> Vec<u8> {
    let mut bytes = tests::pe_with_icon();
    bytes[0x84..0x86].copy_from_slice(&0x014c_u16.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_node(key: &str, value: &[u16], children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 6];
        bytes[2..4].copy_from_slice(&(value.len() as u16).to_le_bytes());
        bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
        for unit in key.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        for unit in value {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        for child in children {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
            bytes.extend_from_slice(child);
        }
        let size = bytes.len() as u16;
        bytes[..2].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    #[test]
    fn version_names_prefer_file_description_and_reject_invalid_text() {
        let text = |value: &str| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        };
        let product = version_node("ProductName", &text("Game product"), &[]);
        let description = version_node("FileDescription", &text("Game window"), &[]);
        let root = version_node("VS_VERSION_INFO", &[], &[product.clone(), description]);
        assert_eq!(version_display_name(&root).as_deref(), Some("Game window"));
        for value in [vec![0xd800, 0], text("Bad\nname"), vec![b'A' as u16; 257]] {
            let bad = version_node("FileDescription", &value, &[]);
            let root = version_node("VS_VERSION_INFO", &[], &[product.clone(), bad]);
            assert_eq!(version_display_name(&root).as_deref(), Some("Game product"));
        }
        let mut invalid_key = version_node("A", &[], &[]);
        invalid_key[6..8].copy_from_slice(&0xd800_u16.to_le_bytes());
        assert!(version_display_name(&invalid_key).is_none());
    }

    #[test]
    fn generic_setup_description_uses_specific_product_or_filename_fallback() {
        let text = |value: &str| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        };
        for description in [
            "Setup/Uninstall",
            "Setup / Uninstall",
            "setup uninstall",
            "SETUP",
            "Installer",
        ] {
            let root = version_node(
                "VS_VERSION_INFO",
                &[],
                &[
                    version_node("FileDescription", &text(description), &[]),
                    version_node("ProductName", &text("Original Example Game"), &[]),
                ],
            );
            assert_eq!(
                version_display_name(&root).as_deref(),
                Some("Original Example Game")
            );
            let root = version_node(
                "VS_VERSION_INFO",
                &[],
                &[
                    version_node("FileDescription", &text(description), &[]),
                    version_node("ProductName", &text("Setup / Uninstall"), &[]),
                ],
            );
            assert!(version_display_name(&root).is_none());
        }
    }

    pub(super) fn pe_with_version(description: &str, product: &str) -> Vec<u8> {
        let text = |value: &str| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        };
        let root = version_node(
            "VS_VERSION_INFO",
            &[],
            &[
                version_node("FileDescription", &text(description), &[]),
                version_node("ProductName", &text(product), &[]),
            ],
        );
        let resource_size = 0x80 + root.len();
        let mut pe = vec![0u8; 0x200 + resource_size];
        pe[..2].copy_from_slice(b"MZ");
        put_u32(&mut pe, 60, 0x80);
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        put_u16(&mut pe, 0x84, 0x14c);
        put_u16(&mut pe, 0x86, 1);
        put_u16(&mut pe, 0x94, 0xe0);
        put_u16(&mut pe, 0x98, 0x10b);
        put_u32(&mut pe, 0x98 + 112, 0x1000);
        put_u32(&mut pe, 0x98 + 116, resource_size as u32);
        let section = 0x80 + 24 + 0xe0;
        put_u32(&mut pe, section + 12, 0x1000);
        put_u32(&mut pe, section + 16, resource_size as u32);
        put_u32(&mut pe, section + 20, 0x200);
        for (offset, id, next) in [(0, 16, 0x20), (0x20, 1, 0x40)] {
            put_u16(&mut pe, 0x200 + offset + 14, 1);
            put_u32(&mut pe, 0x200 + offset + 16, id);
            put_u32(&mut pe, 0x200 + offset + 20, 0x8000_0000 | next);
        }
        put_u16(&mut pe, 0x240 + 14, 1);
        put_u32(&mut pe, 0x240 + 16, 1033);
        put_u32(&mut pe, 0x240 + 20, 0x60);
        put_u32(&mut pe, 0x260, 0x1080);
        put_u32(&mut pe, 0x264, root.len() as u32);
        pe[0x280..].copy_from_slice(&root);
        pe
    }

    #[test]
    fn version_tree_limits_reject_deep_and_truncated_metadata() {
        let mut nested = version_node("FileDescription", &[b'G' as u16, 0], &[]);
        for _ in 0..10 {
            nested = version_node("Container", &[], &[nested]);
        }
        assert!(version_display_name(&nested).is_none());
        let valid = version_node("FileDescription", &[b'G' as u16, 0], &[]);
        for end in 0..valid.len() {
            assert!(version_display_name(&valid[..end]).is_none());
        }
        let mut invalid_size = valid;
        invalid_size[2..4].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(version_display_name(&invalid_size).is_none());
        assert!(version_display_name(&vec![0; 256 * 1024 + 1]).is_none());
    }

    fn test_path(label: &str) -> std::path::PathBuf {
        let build = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".build");
        fs::create_dir_all(&build).unwrap();
        build.join(format!("caskrun-icon-{label}-{}", Uuid::new_v4()))
    }

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    // Minimal PE32 image with an RT_GROUP_ICON pointing to one RT_ICON. The
    // payload is deliberately tiny; the test checks container extraction,
    // while the live Steam smoke covers macOS image decoding.
    pub(super) fn pe_with_icon() -> Vec<u8> {
        let mut pe = vec![0u8; 0x400];
        pe[..2].copy_from_slice(b"MZ");
        put_u32(&mut pe, 60, 0x80);
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        put_u16(&mut pe, 0x86, 1); // one section
        put_u16(&mut pe, 0x94, 0xe0); // PE32 optional header
        put_u16(&mut pe, 0x98, 0x10b);
        put_u32(&mut pe, 0x98 + 112, 0x1000); // resource RVA
        put_u32(&mut pe, 0x98 + 116, 0x200); // resource size
        let section = 0x80 + 24 + 0xe0;
        put_u32(&mut pe, section + 12, 0x1000);
        put_u32(&mut pe, section + 16, 0x200);
        put_u32(&mut pe, section + 20, 0x200);

        // Type → resource ID → language → data entry. All directory offsets
        // are relative to the .rsrc root at raw file offset 0x200.
        put_u16(&mut pe, 0x200 + 14, 2);
        put_u32(&mut pe, 0x200 + 16, RT_GROUP_ICON);
        put_u32(&mut pe, 0x200 + 20, 0x8000_0020);
        put_u32(&mut pe, 0x200 + 24, RT_ICON);
        put_u32(&mut pe, 0x200 + 28, 0x8000_0050);
        for (offset, id, next) in [(0x20, 1, 0x70), (0x50, 5, 0x90)] {
            put_u16(&mut pe, 0x200 + offset + 14, 1);
            put_u32(&mut pe, 0x200 + offset + 16, id);
            put_u32(&mut pe, 0x200 + offset + 20, 0x8000_0000 | next);
        }
        for (offset, next) in [(0x70, 0xb0), (0x90, 0xc0)] {
            put_u16(&mut pe, 0x200 + offset + 14, 1);
            put_u32(&mut pe, 0x200 + offset + 16, 1033);
            put_u32(&mut pe, 0x200 + offset + 20, next);
        }
        put_u32(&mut pe, 0x200 + 0xb0, 0x1100);
        put_u32(&mut pe, 0x200 + 0xb4, 20);
        put_u32(&mut pe, 0x200 + 0xc0, 0x1120);
        put_u32(&mut pe, 0x200 + 0xc4, 8);
        put_u16(&mut pe, 0x300 + 2, 1); // GRPICONDIR type
        put_u16(&mut pe, 0x300 + 4, 1); // one image
        pe[0x306] = 16;
        pe[0x307] = 16;
        put_u16(&mut pe, 0x30a, 1);
        put_u16(&mut pe, 0x30c, 32);
        put_u32(&mut pe, 0x30e, 8);
        put_u16(&mut pe, 0x312, 5);
        pe[0x320..0x328].copy_from_slice(b"ICONDATA");
        pe
    }

    #[test]
    fn rejects_short_and_non_pe_data() {
        let dir = test_path("invalid-pe");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("sample.exe");
        fs::write(&path, b"not a PE").unwrap();
        let file = File::open(&path).unwrap();
        assert!(PeIconReader::new(&file, 8).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn little_endian_fields_are_bounded() {
        assert_eq!(u16_le(&[0x34, 0x12], 0).unwrap(), 0x1234);
        assert_eq!(u32_le(&[0x78, 0x56, 0x34, 0x12], 0).unwrap(), 0x12345678);
        assert!(u16_le(&[], 0).is_err());
        assert!(u32_le(&[1, 2, 3], 0).is_err());
    }

    #[test]
    fn extracts_group_icon_and_rejects_out_of_bounds_resource() {
        let dir = test_path("resource-tree");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("sample.exe");
        let mut pe = pe_with_icon();
        fs::write(&path, &pe).unwrap();
        let file = File::open(&path).unwrap();
        let ico = PeIconReader::new(&file, pe.len() as u64)
            .unwrap()
            .first_icon()
            .unwrap()
            .unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(u32_le(&ico, 18).unwrap(), 22); // first image follows ICO header
        assert_eq!(&ico[22..], b"ICONDATA");
        drop(file);

        put_u32(&mut pe, 0x200 + 0xc0, 0xffff_fff0); // icon data outside section
        fs::write(&path, &pe).unwrap();
        let file = File::open(&path).unwrap();
        assert!(
            PeIconReader::new(&file, pe.len() as u64)
                .unwrap()
                .first_icon()
                .is_err()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_updates_with_exe_and_bottle_falls_back_to_next_launcher() {
        let home = test_path("cache");
        let bottle = crate::create(&home, "Icon tests").unwrap();
        let drive = crate::bottle_dir(&home, &bottle.id)
            .unwrap()
            .join("prefix/drive_c/Games");
        fs::create_dir_all(&drive).unwrap();
        let first_exe = drive.join("First.exe");
        let mut first_pe = pe_with_icon();
        fs::write(&first_exe, &first_pe).unwrap();
        let first = crate::add_launcher(&home, &bottle.id, "First", &first_exe).unwrap();
        let first_path = app_icon(&home, &bottle.id, &first.id)
            .unwrap()
            .path
            .unwrap();
        assert_eq!(&fs::read(&first_path).unwrap()[22..], b"ICONDATA");

        first_pe[0x320..0x328].copy_from_slice(b"NEWICON!");
        fs::write(&first_exe, &first_pe).unwrap();
        let updated = app_icon(&home, &bottle.id, &first.id)
            .unwrap()
            .path
            .unwrap();
        assert_eq!(updated, first_path);
        assert_eq!(&fs::read(&first_path).unwrap()[22..], b"NEWICON!");

        let second_exe = drive.join("Second.exe");
        fs::write(&second_exe, pe_with_icon()).unwrap();
        let second = crate::add_launcher(&home, &bottle.id, "Second", &second_exe).unwrap();
        fs::remove_file(&first_exe).unwrap();
        assert!(
            app_icon(&home, &bottle.id, &first.id)
                .unwrap()
                .path
                .is_none()
        );
        let second_path = app_icon(&home, &bottle.id, &second.id)
            .unwrap()
            .path
            .unwrap();
        assert_eq!(
            bottle_icon(&home, &bottle.id).unwrap().path.as_deref(),
            Some(second_path.as_str())
        );
        fs::remove_file(&second_exe).unwrap();
        assert!(bottle_icon(&home, &bottle.id).unwrap().path.is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn saved_manual_host_exe_artwork_does_not_require_native_alias_containment() {
        let home = test_path("manual-host");
        let bottle = crate::create(&home, "Manual app artwork").unwrap();
        let host = home.join("host-installers");
        fs::create_dir(&host).unwrap();
        let exe = host.join("OriginalManualApp.exe");
        let bytes = pe_with_icon();
        fs::write(&exe, &bytes).unwrap();
        let launcher = crate::add_launcher(&home, &bottle.id, "Manual app", &exe).unwrap();
        let result = app_icon(&home, &bottle.id, &launcher.id).unwrap();
        let path = result
            .path
            .expect("Saved host EXEs retain their publisher artwork");
        assert!(Path::new(&path).exists());
        assert_eq!(fs::read(&exe).unwrap(), bytes);
        let dir = crate::bottle_dir(&home, &bottle.id).unwrap();
        assert!(!dir.join("native-apps").exists());
        fs::remove_dir_all(home).unwrap();
    }
}
