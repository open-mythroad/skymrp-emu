use crate::gzip::{self};
use std::path::Path;

const MRP_MAGIC: &[u8; 4] = b"MRPG";
const MRP_HEADER_SIZE: usize = 16;

#[derive(Debug)]
pub struct Mrp {
    data: Vec<u8>,
    header: MrpHeader,
    entries: Vec<MrpEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct MrpHeader {
    pub info_size: u32,
    pub mrp_file_size: u32,
    pub list_offset: u32,
}

#[derive(Debug, Clone)]
pub struct MrpEntry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
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

impl Mrp {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Mrp, String> {
        let bytes = std::fs::read(path).map_err(|_| "Could not read MRP file")?;
        Self::load_from_bytes(&bytes)
    }

    pub fn load_from_bytes(bytes: &[u8]) -> Result<Mrp, String> {
        let data = bytes.to_vec();
        let header = MrpHeader::parse(&data)?;

        if header.info_size <= 232 {
            return Err(format!("Invalid MRP info size: {}", header.info_size));
        }

        let actual_size = data.len();
        let mrp_file_size = usize::try_from(header.mrp_file_size)
            .map_err(|_| "MRP file size does not fit in usize".to_string())?;

        if mrp_file_size > actual_size {
            return Err(format!(
                "MRP header file size {mrp_file_size} exceeds actual size {actual_size}"
            ));
        }

        let entries = parse_entries(&data, &header, mrp_file_size)?;

        Ok(Mrp {
            data,
            header,
            entries,
        })
    }

    pub fn header(&self) -> &MrpHeader {
        &self.header
    }

    pub fn entries(&self) -> &[MrpEntry] {
        &self.entries
    }

    pub fn find_entry(&self, name: &str) -> Option<&MrpEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn read_file_raw(&self, name: &str) -> Result<&[u8], String> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| format!("MRP entry not found: {name}"))?;

        let start = usize::try_from(entry.offset)
            .map_err(|_| format!("MRP entry offset does not fit in usize: {name}"))?;
        let size = usize::try_from(entry.size)
            .map_err(|_| format!("MRP entry size does not fit in usize: {name}"))?;

        let end = start
            .checked_add(size)
            .ok_or_else(|| format!("MRP entry range overflow: {name}"))?;

        self.data
            .get(start..end)
            .ok_or_else(|| format!("MRP entry data is out of bounds: {name}"))
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>, String> {
        let raw = self.read_file_raw(name)?;

        gzip::decompress_if_needed(raw).map_err(|err| format!("{name}: {err}"))
    }
}

fn parse_entries(
    data: &[u8],
    header: &MrpHeader,
    mrp_file_size: usize,
) -> Result<Vec<MrpEntry>, String> {
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

    let mut entries = Vec::new();
    let mut pos = list_offset;

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

        let name = std::ffi::CStr::from_bytes_until_nul(raw_name)
            .map(|cstr| cstr.to_string_lossy().into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(raw_name).into_owned());
        pos = name_end;

        let meta_end = pos
            .checked_add(12)
            .ok_or_else(|| "MRP entry metadata range overflow".to_string())?;

        if meta_end > list_end {
            return Err(format!("MRP entry metadata is truncated: {name}"));
        }

        let offset = read_u32_le(data, pos)?;
        pos += 4;

        let size = read_u32_le(data, pos)?;
        pos += 4;

        pos += 4; // Unknown/reserved field used by the original format.

        let file_end = offset
            .checked_add(size)
            .ok_or_else(|| format!("MRP entry range overflow: {name}"))?;

        if usize::try_from(file_end).map_or(true, |end| end > mrp_file_size) {
            return Err(format!("MRP entry is out of package bounds: {name}"));
        }

        entries.push(MrpEntry { name, offset, size });
    }

    Ok(entries)
}

fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| format!("Unexpected end of MRP data at offset {offset}"))?;

    Ok(u32::from_le_bytes(bytes.try_into().expect(
        "slice length was checked before converting to u32",
    )))
}
