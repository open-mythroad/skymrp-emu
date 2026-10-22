/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use mlua::{Lua, LuaString, MultiValue, Value, Variadic};

use crate::mem::{MutPtr, LINEAR_MEMORY_SIZE};
use crate::mythroad::mr_free;
use crate::{encoding, Environment};

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

struct PackField {
    option: u8,
    size: usize,
    alignment: usize,
}

struct PackFormat<'a> {
    format: &'a [u8],
    index: usize,
    endian: Endian,
    alignment: usize,
}

impl<'a> PackFormat<'a> {
    fn new(format: &'a [u8]) -> mlua::Result<Self> {
        let mut index = 0;
        let endian = match format.first() {
            Some(b'>') => {
                index += 1;
                Endian::Big
            }
            Some(b'<') => {
                index += 1;
                Endian::Little
            }
            _ if cfg!(target_endian = "big") => Endian::Big,
            _ => Endian::Little,
        };
        let alignment = if format.get(index) == Some(&b'@') {
            index += 1;
            Self::read_number(format, &mut index, 4)?
        } else {
            1
        };

        Ok(Self {
            format,
            index,
            endian,
            alignment,
        })
    }

    fn read_number(format: &[u8], index: &mut usize, default: usize) -> mlua::Result<usize> {
        if !format.get(*index).is_some_and(u8::is_ascii_digit) {
            return Ok(default);
        }

        let mut value = 0usize;
        while let Some(digit) = format.get(*index).filter(|byte| byte.is_ascii_digit()) {
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add((*digit - b'0') as usize))
                .ok_or_else(|| mlua::Error::runtime("format size is too large"))?;
            *index += 1;
        }
        Ok(value)
    }

    fn next_field(&mut self) -> mlua::Result<Option<PackField>> {
        let Some(&option) = self.format.get(self.index) else {
            return Ok(None);
        };
        self.index += 1;

        let size = match option {
            b'b' | b'B' | b'x' | b' ' => 1,
            b'h' | b'H' => 2,
            b'i' | b'I' => 4,
            b'l' | b'L' => Self::read_number(self.format, &mut self.index, 4)?.min(4),
            b'c' => Self::read_number(self.format, &mut self.index, 1)?,
            b's' | b'p' => 0,
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "invalid format '{}'",
                    char::from(option)
                )));
            }
        };
        let alignment = if matches!(option, b'c' | b's' | b'p') {
            1
        } else {
            size.min(self.alignment)
        };
        if alignment == 0 || !alignment.is_power_of_two() {
            return Err(mlua::Error::runtime("alignment must be 2^n"));
        }

        Ok(Some(PackField {
            option,
            size,
            alignment,
        }))
    }
}

fn align_position(position: usize, alignment: usize) -> mlua::Result<usize> {
    position
        .checked_add(alignment - 1)
        .map(|position| position & !(alignment - 1))
        .ok_or_else(|| mlua::Error::runtime("packed data is too large"))
}

fn c_string_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len())
}

fn wide_string_len(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(2)
        .position(|unit| unit == [0, 0])
        .map(|units| units * 2)
        .unwrap_or(bytes.len())
}

pub(super) fn clen(_: &Lua, value: LuaString) -> mlua::Result<usize> {
    Ok(c_string_len(&value.as_bytes()))
}

pub(super) fn wlen(_: &Lua, value: LuaString) -> mlua::Result<usize> {
    Ok(wide_string_len(&value.as_bytes()))
}

pub(super) fn cstr(lua: &Lua, value: LuaString) -> mlua::Result<LuaString> {
    let bytes = value.as_bytes();
    lua.create_string(&bytes[..c_string_len(&bytes)])
}

pub(super) fn wstr(lua: &Lua, value: LuaString) -> mlua::Result<LuaString> {
    let bytes = value.as_bytes();
    lua.create_string(&bytes[..wide_string_len(&bytes)])
}

pub(super) fn pack_len(_: &Lua, format: LuaString) -> mlua::Result<usize> {
    let format_bytes = format.as_bytes();
    let mut format = PackFormat::new(format_bytes.as_ref())?;
    let mut total_size = 0usize;

    while let Some(field) = format.next_field()? {
        if field.size == 0 {
            return Err(mlua::Error::runtime("meet size 0, check 'c' , 's' or 'p'"));
        }
        total_size = align_position(total_size, field.alignment)?
            .checked_add(field.size)
            .ok_or_else(|| mlua::Error::runtime("packed data is too large"))?;
    }

    Ok(total_size)
}

pub(super) fn pack(
    lua: &Lua,
    (format, arguments): (LuaString, Variadic<Value>),
) -> mlua::Result<LuaString> {
    let format_bytes = format.as_bytes();
    let mut format = PackFormat::new(format_bytes.as_ref())?;
    let mut output = Vec::new();
    let mut argument_index = 0usize;
    let mut total_size = 0usize;

    while let Some(field) = format.next_field()? {
        let aligned = align_position(total_size, field.alignment)?;
        output.resize(output.len() + aligned - total_size, 0);
        total_size = aligned;
        let mut size = field.size;

        match field.option {
            b'b' | b'B' | b'h' | b'H' | b'l' | b'L' | b'i' | b'I' => {
                let value = arguments
                    .get(argument_index)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("missing value for integer field"))?;
                let number = lua
                    .coerce_number(value)?
                    .ok_or_else(|| mlua::Error::runtime("number expected"))?;
                let value = if number < 0.0 {
                    (-(number.trunc()) as u64).wrapping_neg()
                } else {
                    number.trunc() as u64
                };
                for index in 0..field.size {
                    let shift = match format.endian {
                        Endian::Little => index * 8,
                        Endian::Big => (field.size - index - 1) * 8,
                    };
                    output.push((value >> shift) as u8);
                }
                argument_index += 1;
            }
            b'x' => output.push(0),
            b'c' | b's' | b'p' => {
                let value = arguments
                    .get(argument_index)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("missing value for string field"))?;
                let value = lua
                    .coerce_string(value)?
                    .ok_or_else(|| mlua::Error::runtime("string expected"))?;
                let bytes = value.as_bytes();
                size = if field.size == 0 {
                    bytes.len()
                } else {
                    field.size
                };
                let copied = bytes.len().min(size);
                output.extend_from_slice(&bytes[..copied]);
                output.resize(output.len() + size - copied, 0);
                if field.option == b's' {
                    output.push(0);
                    size += 1;
                }
                argument_index += 1;
            }
            b' ' => argument_index += 1,
            _ => unreachable!(),
        }
        total_size = total_size
            .checked_add(size)
            .ok_or_else(|| mlua::Error::runtime("packed data is too large"))?;
    }

    lua.create_string(output)
}

pub(super) fn unpack(
    lua: &Lua,
    (format, data, position): (LuaString, LuaString, Option<i32>),
) -> mlua::Result<MultiValue> {
    let format_bytes = format.as_bytes();
    let mut format = PackFormat::new(format_bytes.as_ref())?;
    let data = data.as_bytes();
    let position = position.unwrap_or(1);
    if position < 1 {
        return Err(mlua::Error::runtime("unpack position must be at least 1"));
    }
    let mut position = (position - 1) as usize;
    let mut values = Vec::new();

    while let Some(field) = format.next_field()? {
        position = align_position(position, field.alignment)?;
        let mut size = field.size;
        if field.option == b'p' {
            size = match values.last() {
                Some(Value::Number(value)) if *value >= 0.0 => value.trunc() as usize,
                Some(Value::Integer(value)) if *value >= 0 => *value as usize,
                _ => return Err(mlua::Error::runtime("previous size for `p' missing")),
            };
        }

        let end = position
            .checked_add(size)
            .ok_or_else(|| mlua::Error::runtime("unpack:input too short"))?;
        if end > data.len() {
            return Err(mlua::Error::runtime("unpack:input too short"));
        }

        match field.option {
            b'b' | b'B' | b'h' | b'H' | b'l' | b'L' | b'i' | b'I' => {
                let mut value = 0u64;
                for index in 0..size {
                    let source = match format.endian {
                        Endian::Little => position + size - index - 1,
                        Endian::Big => position + index,
                    };
                    value = (value << 8) | u64::from(data[source]);
                }
                let number =
                    if field.option.is_ascii_lowercase() && value & (1u64 << (size * 8 - 1)) != 0 {
                        (value as i64 - (1i64 << (size * 8))) as f64
                    } else {
                        value as f64
                    };
                values.push(Value::Number(number));
            }
            b'c' | b'p' => {
                values.push(Value::String(lua.create_string(&data[position..end])?));
            }
            b's' => {
                let relative_end = data[position..]
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| mlua::Error::runtime("unfinished string in input"))?;
                size = relative_end + 1;
                values.push(Value::String(
                    lua.create_string(&data[position..position + relative_end])?,
                ));
            }
            b'x' | b' ' => {}
            _ => unreachable!(),
        }
        position = position
            .checked_add(size)
            .ok_or_else(|| mlua::Error::runtime("unpack position overflow"))?;
    }

    values.push(Value::Number((position + 1) as f64));
    Ok(MultiValue::from_vec(values))
}

pub(super) fn c2u(lua: &Lua, env: &mut Environment, value: LuaString) -> mlua::Result<LuaString> {
    let input = env.mem.alloc_and_write_cstr(&value.as_bytes());
    let converted = encoding::c2u(env, input.cast_const(), false);
    env.mem.free(input.cast_void());
    if converted.ptr.is_null() {
        return Err(mlua::Error::runtime("Gb2312toUnicode failed"));
    }

    let output = lua.create_string(env.mem.bytes_at(converted.ptr.cast(), converted.size))?;
    mr_free(env, converted.ptr.cast_void(), converted.size);
    Ok(output)
}

pub(super) fn u2c(lua: &Lua, value: LuaString) -> mlua::Result<LuaString> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        if pair == [0, 0] || pair[0] != 0 {
            break;
        }
        output.push(pair[1]);
    }
    lua.create_string(output)
}

fn relative_index(index: i32, len: usize) -> Option<usize> {
    let index = if index < 0 {
        i64::try_from(len).ok()?.checked_add(i64::from(index))?
    } else {
        i64::from(index).checked_sub(1)?
    };
    usize::try_from(index).ok()
}

fn update_range(
    destination_len: usize,
    source_len: usize,
    offset: Option<i32>,
    start: Option<i32>,
    end: Option<i32>,
) -> mlua::Result<(usize, usize, usize)> {
    let offset = relative_index(offset.unwrap_or(1), destination_len)
        .filter(|offset| *offset <= destination_len)
        .ok_or_else(|| mlua::Error::runtime("update overflow"))?;
    let start = relative_index(start.unwrap_or(1), source_len)
        .filter(|start| *start <= source_len)
        .ok_or_else(|| mlua::Error::runtime("update overflow"))?;
    let end = relative_index(
        end.unwrap_or_else(|| i32::try_from(source_len).unwrap_or(i32::MAX)),
        source_len,
    )
    .filter(|end| *end < source_len)
    .ok_or_else(|| mlua::Error::runtime("update overflow"))?;
    if start > end {
        return Err(mlua::Error::runtime("update overflow"));
    }

    let count = (end - start + 1).min(destination_len - offset);
    Ok((offset, start, count))
}

pub(super) fn update(
    _: &Lua,
    (destination, source, offset, start, end): (
        LuaString,
        LuaString,
        Option<i32>,
        Option<i32>,
        Option<i32>,
    ),
) -> mlua::Result<()> {
    let destination = destination.as_bytes();
    let source = source.as_bytes();
    let (offset, start, count) = update_range(destination.len(), source.len(), offset, start, end)?;

    // Mythroad intentionally exposes mutable Lua strings through this API.
    unsafe {
        std::ptr::copy(
            source.as_ptr().add(start),
            destination.as_ptr().cast_mut().add(offset),
            count,
        );
    }
    Ok(())
}

pub(super) fn pupdate(
    _: &Lua,
    env: &mut Environment,
    (destination, destination_len, source, source_len, offset, start, end): (
        u32,
        u32,
        u32,
        u32,
        Option<i32>,
        Option<i32>,
        Option<i32>,
    ),
) -> mlua::Result<()> {
    let destination_len = usize::try_from(destination_len).map_err(mlua::Error::external)?;
    let source_len = usize::try_from(source_len).map_err(mlua::Error::external)?;
    let (offset, start, count) = update_range(destination_len, source_len, offset, start, end)?;
    let count_u32 = u32::try_from(count).map_err(mlua::Error::external)?;
    let destination = destination
        .checked_add(u32::try_from(offset).map_err(mlua::Error::external)?)
        .filter(|address| {
            address
                .checked_add(count_u32)
                .is_some_and(|end| end <= LINEAR_MEMORY_SIZE)
        })
        .ok_or_else(|| mlua::Error::runtime("update overflow"))?;
    let source = source
        .checked_add(u32::try_from(start).map_err(mlua::Error::external)?)
        .filter(|address| {
            address
                .checked_add(count_u32)
                .is_some_and(|end| end <= LINEAR_MEMORY_SIZE)
        })
        .ok_or_else(|| mlua::Error::runtime("update overflow"))?;

    let bytes = env
        .mem
        .bytes_at(MutPtr::<u8>::from_bits(source).cast_const(), count_u32)
        .to_vec();
    env.mem
        .bytes_at_mut(MutPtr::from_bits(destination), count_u32)
        .copy_from_slice(&bytes);
    Ok(())
}

pub(super) fn new(lua: &Lua, len: u32) -> mlua::Result<LuaString> {
    let len = usize::try_from(len).map_err(mlua::Error::external)?;
    lua.create_string(vec![0; len])
}

pub(super) fn set(_: &Lua, (value, offset, byte): (LuaString, i32, i32)) -> mlua::Result<()> {
    let bytes = value.as_bytes();
    let offset = relative_index(offset, bytes.len())
        .filter(|offset| *offset < bytes.len())
        .ok_or_else(|| mlua::Error::runtime("set overflow"))?;

    // This mirrors Mythroad's mutable-string extension.
    unsafe {
        bytes.as_ptr().cast_mut().add(offset).write(byte as u8);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{LuaOptions, StdLib, Table};

    fn string_lua() -> Lua {
        let lua = Lua::new_with(StdLib::STRING, LuaOptions::default()).unwrap();
        let string: Table = lua.globals().get("string").unwrap();
        string
            .set("pack", lua.create_function(pack).unwrap())
            .unwrap();
        string
            .set("unpack", lua.create_function(unpack).unwrap())
            .unwrap();
        string
            .set("packLen", lua.create_function(pack_len).unwrap())
            .unwrap();
        string
            .set("clen", lua.create_function(clen).unwrap())
            .unwrap();
        string
            .set("wlen", lua.create_function(wlen).unwrap())
            .unwrap();
        string
            .set("cstr", lua.create_function(cstr).unwrap())
            .unwrap();
        string
            .set("wstr", lua.create_function(wstr).unwrap())
            .unwrap();
        string
            .set("u2c", lua.create_function(u2c).unwrap())
            .unwrap();
        string
            .set("update", lua.create_function(update).unwrap())
            .unwrap();
        string
            .set("new", lua.create_function(new).unwrap())
            .unwrap();
        string
            .set("set", lua.create_function(set).unwrap())
            .unwrap();
        lua
    }

    #[test]
    fn pack_unpack_integers() {
        string_lua()
            .load(
                r#"
                local packed = string.pack("<bBhHiI", -1, 255, -2, 65534, -3, 4294967293)
                assert(packed == "\255\255\254\255\254\255\253\255\255\255\253\255\255\255")
                local a, b, c, d, e, f, next = string.unpack("<bBhHiI", packed)
                assert(a == -1 and b == 255 and c == -2 and d == 65534)
                assert(e == -3 and f == 4294967293 and next == 15)
                "#,
            )
            .exec()
            .unwrap();
    }

    #[test]
    fn pack_unpack_strings_alignment_and_position() {
        string_lua()
            .load(
                r#"
                assert(string.pack("c4s", "ab", "z") == "ab\0\0z\0")
                assert(string.pack(">@4bI", -1, 0x11223344) == "\255\0\0\0\17\34\51\68")
                assert(string.pack("@4B I", 1, nil, 2) == "\1\0\0\2\0\0\0")
                assert(string.pack("@4sI", "a", 0x11223344) == "a\0\0\0\68\51\34\17")

                local size, value, next = string.unpack("Bp", "\3abc")
                assert(size == 3 and value == "abc" and next == 5)

                local text, after_text = string.unpack("s", "skip\0tail")
                assert(text == "skip" and after_text == 6)

                local byte, after_byte = string.unpack("B", "\0\5", 2)
                assert(byte == 5 and after_byte == 3)
                "#,
            )
            .exec()
            .unwrap();
    }

    #[test]
    fn unpack_rejects_invalid_input() {
        string_lua()
            .load(
                r#"
                assert(not pcall(string.unpack, "I", "\1\2"))
                assert(not pcall(string.unpack, "s", "unfinished"))
                assert(not pcall(string.unpack, "p", "abc"))
                assert(not pcall(string.unpack, "@3I", "\0\0\0\0"))
                assert(not pcall(string.pack, "q", 1))
                "#,
            )
            .exec()
            .unwrap();
    }

    #[test]
    fn pack_len_matches_mythroad_layout() {
        string_lua()
            .load(
                r#"
                assert(string.packLen("<bBhHiI") == 14)
                assert(string.packLen(">@4bI") == 8)
                assert(string.packLen("@4B I") == 8)
                assert(not pcall(string.packLen, "s"))
                "#,
            )
            .exec()
            .unwrap();
    }

    #[test]
    fn mutable_string_extensions_match_mythroad_indices() {
        string_lua()
            .load(
                r#"
                local value = string.new(5)
                string.set(value, 1, 65)
                string.set(value, -1, 69)
                string.update(value, "BCD", 2)
                assert(string.byte(value, 1) == 65)
                assert(string.byte(value, 2) == 66)
                assert(string.byte(value, 3) == 67)
                assert(string.byte(value, 4) == 68)
                assert(string.byte(value, 5) == 69)
                assert(string.clen("a\0b") == 1)
                assert(string.wlen("\0A\0B\0\0") == 4)
                assert(string.cstr("a\0b") == "a")
                assert(string.wstr("\0A\0B\0\0") == "\0A\0B")
                assert(string.u2c("\0A\0B\0\0") == "AB")
                "#,
            )
            .exec()
            .unwrap();
    }
}
