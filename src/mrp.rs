/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::encoding;
use crate::fs::{Fs, GuestPath};
use crate::gzip;
use crate::mem::{Memory, MutPtr};
use std::collections::HashMap;

pub const MR_START_FILE_NAME: &str = "start.mr";
pub const START_FILE_NAME: &str = "cfunction.ext";
pub const LOGO_EXT_FILE_NAME: &str = "logo.ext";
const MRP_MAGIC: &[u8; 4] = b"MRPG";
const MRP_HEADER_SIZE: usize = 16;
const LEGACY_MRP_HEADER_SIZE: usize = 8;
const MRP_APP_ID_OFFSET: usize = 192;
const MRP_APP_VERSION_OFFSET: usize = 196;
const MRP_RAM_OFFSET: usize = 228;
const MRP_RAM_CHECK_OFFSET: usize = 230;

#[derive(Debug)]
pub struct PackageCache {
    filename: String,
    data: Vec<u8>,
    entries: HashMap<String, MrpEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct MrpHeader {
    pub info_size: u32,
    pub mrp_file_size: u32,
    pub list_offset: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct MrpEntry {
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MrpAppInfo {
    pub id: u32,
    pub version: u32,
    pub ram: u32,
}

impl PackageCache {
    pub fn new(filename: String, data: Vec<u8>) -> Result<Self, String> {
        let entries = parse_entries(&data)?;
        Ok(Self {
            filename,
            data,
            entries,
        })
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>, String> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| format!("MRP entry not found: {name}"))?;
        read_entry_from_bytes(&self.data, name, *entry)
    }

    pub fn app_info(&self) -> Result<MrpAppInfo, String> {
        read_app_info(&self.data)
    }
}

impl MrpHeader {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < MRP_HEADER_SIZE {
            return Err("MRP file is too small".to_string());
        }

        if &data[0..4] != MRP_MAGIC {
            return Err("Invalid MRP magic".to_string());
        }

        let info_size = read_u32_le(data, 4)?;
        let mrp_file_size = read_u32_le(data, 8)?;
        let list_offset = read_u32_le(data, 12)?;

        Ok(Self {
            info_size,
            mrp_file_size,
            list_offset,
        })
    }
}

pub fn load_from_file<P: AsRef<GuestPath>>(
    path: P,
    fs: &Fs,
    into_mem: &mut Memory,
) -> Result<MutPtr<u8>, String> {
    load_from_bytes(
        &fs.read(path.as_ref())
            .map_err(|_| "Could not read MRP file")?,
        into_mem,
    )
}

pub fn load_from_bytes(bytes: &[u8], into_mem: &mut Memory) -> Result<MutPtr<u8>, String> {
    parse_entries(bytes)?;

    let guest_len = u32::try_from(bytes.len())
        .map_err(|_| "MRP file size does not fit in guest memory size".to_string())?;
    let guest_base: MutPtr<u8> = into_mem.alloc(guest_len).cast();
    copy_bytes_to_guest(into_mem, guest_base, bytes)?;

    Ok(guest_base)
}

pub fn find_entry(data: &[u8], name: &str) -> Result<Option<MrpEntry>, String> {
    Ok(parse_entries(data)?.get(name).copied())
}

pub fn read_app_info(data: &[u8]) -> Result<MrpAppInfo, String> {
    MrpHeader::parse(data)?;

    let id = read_u32_be(data, MRP_APP_ID_OFFSET).unwrap_or_default();
    let version = read_u32_be(data, MRP_APP_VERSION_OFFSET).unwrap_or_default();
    let ram = match (
        data.get(MRP_RAM_OFFSET..MRP_RAM_OFFSET + 2),
        data.get(MRP_RAM_CHECK_OFFSET..MRP_RAM_CHECK_OFFSET + 2),
    ) {
        (Some(ram_bytes), Some(ram_check_bytes)) => {
            let ram = u16::from_be_bytes([ram_bytes[0], ram_bytes[1]]);
            let ram_check = u16::from_be_bytes([ram_check_bytes[0], ram_check_bytes[1]]);
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&ram.to_le_bytes());
            hasher.update(&ram.to_le_bytes());
            if ram_check == (hasher.finalize() & 0xff) as u16 {
                u32::from(ram)
            } else {
                0
            }
        }
        _ => 0,
    };

    Ok(MrpAppInfo { id, version, ram })
}

pub fn parse_entries(data: &[u8]) -> Result<HashMap<String, MrpEntry>, String> {
    let header = MrpHeader::parse(data)?;

    if header.info_size <= 232 {
        return parse_legacy_entries(data, &header);
    }

    let mrp_file_size = usize::try_from(header.mrp_file_size)
        .map_err(|_| "MRP file size does not fit in usize".to_string())?;

    if mrp_file_size > data.len() {
        return Err(format!(
            "MRP header file size {mrp_file_size} exceeds actual size {}",
            data.len()
        ));
    }

    parse_entries_in_list(data, &header, mrp_file_size)
}

pub fn read_file_from_bytes(data: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let entry = find_entry(data, name)?.ok_or_else(|| format!("MRP entry not found: {name}"))?;
    read_entry_from_bytes(data, name, entry)
}

fn read_entry_from_bytes(data: &[u8], name: &str, entry: MrpEntry) -> Result<Vec<u8>, String> {
    let start = usize::try_from(entry.offset)
        .map_err(|_| format!("MRP entry offset does not fit in usize: {name}"))?;

    let size = usize::try_from(entry.size)
        .map_err(|_| format!("MRP entry size does not fit in usize: {name}"))?;

    let end = start
        .checked_add(size)
        .ok_or_else(|| format!("MRP entry range overflow: {name}"))?;

    let raw = data
        .get(start..end)
        .ok_or_else(|| format!("MRP entry data is out of bounds: {name}"))?;
    gzip::decompress_if_needed(raw).map_err(|err| format!("{name}: {err}"))
}

pub fn copy_bytes_to_guest(mem: &mut Memory, ptr: MutPtr<u8>, bytes: &[u8]) -> Result<u32, String> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| "Data size does not fit in guest memory size".to_string())?;
    mem.bytes_at_mut(ptr, len).copy_from_slice(bytes);
    Ok(len)
}

fn parse_entries_in_list(
    data: &[u8],
    header: &MrpHeader,
    mrp_file_size: usize,
) -> Result<HashMap<String, MrpEntry>, String> {
    let info_size = usize::try_from(header.info_size)
        .map_err(|_| "MRP info size does not fit in usize".to_string())?;
    let list_offset = usize::try_from(header.list_offset)
        .map_err(|_| "MRP list offset does not fit in usize".to_string())?;

    let list_size = info_size
        .checked_add(8)
        .and_then(|value| value.checked_sub(list_offset))
        .ok_or_else(|| "Invalid MRP list size".to_string())?;

    let list_end = list_offset
        .checked_add(list_size)
        .ok_or_else(|| "MRP list range overflow".to_string())?;

    if list_offset < MRP_HEADER_SIZE || list_end > data.len() {
        return Err(format!(
            "MRP file list range is out of bounds: {list_offset}..{list_end}"
        ));
    }

    let mut pos = list_offset;
    let mut entries = HashMap::new();

    while pos < list_end {
        let name_len = read_u32_le(data, pos)? as usize;
        pos += 4;

        if name_len == 0 || name_len >= 0x80 {
            return Err(format!("Invalid MRP entry name length: {name_len}"));
        }

        let name_end = pos
            .checked_add(name_len)
            .ok_or_else(|| "MRP entry name range overflow".to_string())?;

        if name_end > list_end {
            return Err("MRP entry name is out of bounds".to_string());
        }

        let raw_name = &data[pos..name_end];

        let raw_name = raw_name.split(|byte| *byte == 0).next().unwrap_or(raw_name);
        let entry_name = encoding::gb_to_utf8_string(raw_name).into_owned();
        pos = name_end;

        let meta_end = pos
            .checked_add(12)
            .ok_or_else(|| "MRP entry metadata range overflow".to_string())?;

        if meta_end > list_end {
            return Err(format!("MRP entry metadata is truncated: {entry_name}"));
        }

        let offset = read_u32_le(data, pos)?;
        pos += 4;

        let size = read_u32_le(data, pos)?;
        pos += 4;

        pos += 4; // Unknown/reserved field used by the original format.

        let file_end = offset
            .checked_add(size)
            .ok_or_else(|| format!("MRP entry range overflow: {entry_name}"))?;

        if usize::try_from(file_end).map_or(true, |end| end > mrp_file_size) {
            return Err(format!("MRP entry is out of package bounds: {entry_name}"));
        }

        entries
            .entry(entry_name)
            .or_insert(MrpEntry { offset, size });
    }

    Ok(entries)
}

fn parse_legacy_entries(
    data: &[u8],
    header: &MrpHeader,
) -> Result<HashMap<String, MrpEntry>, String> {
    let info_size = usize::try_from(header.info_size)
        .map_err(|_| "MRP info size does not fit in usize".to_string())?;
    let mut pos = LEGACY_MRP_HEADER_SIZE
        .checked_add(info_size)
        .ok_or_else(|| "Legacy MRP entry offset overflow".to_string())?;
    let mut entries = HashMap::new();

    while pos < data.len() {
        let name_len = read_u32_le(data, pos)? as usize;
        pos = pos
            .checked_add(4)
            .ok_or_else(|| "Legacy MRP entry name offset overflow".to_string())?;
        if name_len == 0 || name_len >= 0x80 {
            return Err(format!("Invalid legacy MRP entry name length: {name_len}"));
        }

        let name_end = pos
            .checked_add(name_len)
            .ok_or_else(|| "Legacy MRP entry name range overflow".to_string())?;
        let raw_name = data
            .get(pos..name_end)
            .ok_or_else(|| "Legacy MRP entry name is out of bounds".to_string())?;
        let raw_name = raw_name.split(|byte| *byte == 0).next().unwrap_or(raw_name);
        let entry_name = encoding::gb_to_utf8_string(raw_name).into_owned();
        pos = name_end;

        let size = read_u32_le(data, pos)?;
        pos = pos
            .checked_add(4)
            .ok_or_else(|| "Legacy MRP entry data offset overflow".to_string())?;
        let data_end = pos
            .checked_add(size as usize)
            .ok_or_else(|| "Legacy MRP entry range overflow".to_string())?;
        if data_end > data.len() {
            return Err("Legacy MRP entry data is out of bounds".to_string());
        }

        entries.entry(entry_name).or_insert(MrpEntry {
            offset: u32::try_from(pos)
                .map_err(|_| "Legacy MRP entry offset does not fit in u32".to_string())?,
            size,
        });
        pos = data_end;
    }

    Ok(entries)
}

fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("MRP u32 read offset overflow: {offset}"))?;

    let bytes = data
        .get(offset..end)
        .ok_or_else(|| format!("Unexpected end of MRP data at offset {offset}"))?;

    Ok(u32::from_le_bytes(bytes.try_into().expect(
        "slice length was checked before converting to u32",
    )))
}

fn read_u32_be(data: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("MRP u32 read offset overflow: {offset}"))?;

    let bytes = data
        .get(offset..end)
        .ok_or_else(|| format!("Unexpected end of MRP data at offset {offset}"))?;

    Ok(u32::from_be_bytes(bytes.try_into().expect(
        "slice length was checked before converting to u32",
    )))
}
