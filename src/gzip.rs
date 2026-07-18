/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::io::Read;

const MAX_GZIP_OUTPUT_SIZE: usize = 1024 * 1024;
pub const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

pub fn is_gzip(data: &[u8]) -> bool {
    data.starts_with(&GZIP_MAGIC)
}

pub fn decompress_if_needed(data: &[u8]) -> Result<Vec<u8>, String> {
    if is_gzip(data) {
        ungzip(data)
    } else {
        Ok(data.to_vec())
    }
}

pub fn ungzip(data: &[u8]) -> Result<Vec<u8>, String> {
    if !is_gzip(data) {
        return Err("not gzip data".to_string());
    }

    let expected_size = gzip_original_size(data)?;

    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut output = Vec::with_capacity(expected_size);

    decoder
        .read_to_end(&mut output)
        .map_err(|err| format!("gzip decode failed: {err}"))?;

    if output.len() != expected_size {
        return Err(format!(
            "gzip output size mismatch: expected {expected_size}, got {}",
            output.len()
        ));
    }

    Ok(output)
}

fn gzip_original_size(data: &[u8]) -> Result<usize, String> {
    if data.len() < 4 {
        return Err("gzip data is too small".to_string());
    }

    let footer = data
        .get(data.len() - 4..)
        .ok_or_else(|| "gzip footer is missing".to_string())?;

    let size =
        u32::from_le_bytes(footer.try_into().expect("gzip footer length was checked")) as usize;

    if size == 0 {
        return Err("gzip original size is zero".to_string());
    }

    if size > MAX_GZIP_OUTPUT_SIZE {
        return Err(format!(
            "gzip original size {size} exceeds limit {MAX_GZIP_OUTPUT_SIZE}"
        ));
    }

    Ok(size)
}
