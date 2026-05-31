use crate::gzip;
use crate::mem::{Memory, Ptr};
use std::path::Path;

pub const START_FILE_NAME: &str = "cfunction.ext";

pub const CODE_BASE_ADDR: u32 = 0x0008_0000;
pub const ENTRY_OFFSET: u32 = 8;

const MRP_MAGIC: &[u8; 4] = b"MRPG";
const MRP_HEADER_SIZE: usize = 16;

#[derive(Debug)]
pub struct Mrp {
    pub data: Vec<u8>,
    pub entry_point_pc: Option<u32>,
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
    pub fn load_from_file<P: AsRef<Path>>(path: P, into_mem: &mut Memory) -> Result<Mrp, String> {
        let bytes = std::fs::read(path).map_err(|_| "Could not read MRP file")?;
        Self::load_from_bytes(&bytes, into_mem)
    }

    pub fn load_from_bytes(bytes: &[u8], into_mem: &mut Memory) -> Result<Mrp, String> {
        let data = bytes.to_vec();

        let cfunction_ext = read_file(&data, START_FILE_NAME)?;

        let file_size = u32::try_from(cfunction_ext.len())
            .map_err(|_| format!("{START_FILE_NAME} is too large"))?;

        let entry_point_pc = CODE_BASE_ADDR
            .checked_add(ENTRY_OFFSET)
            .ok_or_else(|| "MRP entry point PC overflow".to_string())?;

        let code_end = CODE_BASE_ADDR
            .checked_add(file_size)
            .ok_or_else(|| "MRP code range overflow".to_string())?;

        if entry_point_pc >= code_end {
            return Err(format!(
                "MRP entry point PC is outside loaded code: pc=0x{entry_point_pc:08x}"
            ));
        }

        {
            let dst = into_mem.bytes_at_mut(Ptr::<u8, true>::from_bits(CODE_BASE_ADDR), file_size);
            dst.copy_from_slice(&cfunction_ext);
        }

        Ok(Mrp {
            data,
            entry_point_pc: Some(entry_point_pc),
        })
    }
}

pub fn read_file(data: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let header = MrpHeader::parse(data)?;

    if header.info_size <= 232 {
        return Err(format!("Invalid MRP info size: {}", header.info_size));
    }

    let mrp_file_size = usize::try_from(header.mrp_file_size)
        .map_err(|_| "MRP file size does not fit in usize".to_string())?;

    if mrp_file_size > data.len() {
        return Err(format!(
            "MRP header file size {mrp_file_size} exceeds actual size {}",
            data.len()
        ));
    }

    let entry = find_entry(data, &header, mrp_file_size, name)?
        .ok_or_else(|| format!("MRP entry not found: {name}"))?;

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

fn find_entry(
    data: &[u8],
    header: &MrpHeader,
    mrp_file_size: usize,
    target_name: &str,
) -> Result<Option<MrpEntry>, String> {
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

        let entry_name = std::ffi::CStr::from_bytes_until_nul(raw_name)
            .map(|cstr| cstr.to_string_lossy().into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(raw_name).into_owned());
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

        if entry_name == target_name {
            return Ok(Some(MrpEntry {
                name: entry_name,
                offset,
                size,
            }));
        }
    }

    Ok(None)
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
