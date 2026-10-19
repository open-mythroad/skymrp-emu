/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::push_result;
use crate::dsm;
use crate::mem::ConstPtr;
use crate::mythroad::{MrResult, MR_FILE_MAX_LEN};
use crate::Environment;

use chrono::{Datelike, Local, Timelike};
use mlua::{Lua, LuaString, MultiValue, Table};

pub(super) fn get_uptime(_: &Lua, env: &mut Environment, (): ()) -> mlua::Result<u32> {
    Ok(dsm::mr_get_time(env))
}

pub(super) fn remove(
    lua: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<MultiValue> {
    let filename = filename.as_bytes();
    let result = with_guest_path(env, &filename, dsm::mr_remove);
    push_result(lua, result == MrResult::Success as i32, Some(&filename))
}

pub(super) fn rename(
    lua: &Lua,
    env: &mut Environment,
    (oldname, newname): (LuaString, LuaString),
) -> mlua::Result<MultiValue> {
    let oldname_bytes = oldname.as_bytes();
    let result = with_guest_path(env, &oldname_bytes, |env, oldname_ptr| {
        with_guest_path(env, &newname.as_bytes(), |env, newname_ptr| {
            dsm::mr_rename(env, oldname_ptr, newname_ptr)
        })
    });
    let succeeded = result == MrResult::Success as i32;
    push_result(lua, succeeded, Some(&oldname_bytes))
}

pub(super) fn mkdir(
    lua: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<MultiValue> {
    let filename = filename.as_bytes();
    let result = with_guest_path(env, &filename, dsm::mr_mkdir);
    push_result(lua, result == MrResult::Success as i32, Some(&filename))
}

pub(super) fn rmdir(
    lua: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<MultiValue> {
    let filename = filename.as_bytes();
    let result = with_guest_path(env, &filename, dsm::mr_rmdir);
    push_result(lua, result == MrResult::Success as i32, Some(&filename))
}

pub(super) fn get_file_info(
    _: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<i32> {
    Ok(with_guest_path(env, &filename.as_bytes(), dsm::mr_info))
}

pub(super) fn get_file_len(
    _: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<i32> {
    Ok(with_guest_path(env, &filename.as_bytes(), dsm::mr_get_len))
}

pub(super) fn find_start(
    lua: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<(i32, LuaString)> {
    let buffer = env.mem.alloc(MR_FILE_MAX_LEN).cast::<u8>();
    env.mem.bytes_at_mut(buffer, MR_FILE_MAX_LEN).fill(0);
    let handle = with_guest_path(env, &filename.as_bytes(), |env, filename_ptr| {
        dsm::mr_find_start(env, filename_ptr, buffer, MR_FILE_MAX_LEN)
    });
    let first = env.mem.cstr_at(buffer.cast_const()).to_vec();
    env.mem.free(buffer.cast_void());
    Ok((handle, lua.create_string(first)?))
}

pub(super) fn find_next(
    lua: &Lua,
    env: &mut Environment,
    handle: Option<i32>,
) -> mlua::Result<Option<LuaString>> {
    let buffer = env.mem.alloc(MR_FILE_MAX_LEN).cast::<u8>();
    let result = dsm::mr_find_get_next(env, handle.unwrap_or(0), buffer, MR_FILE_MAX_LEN);
    let filename = if result == MrResult::Success as i32 {
        Some(env.mem.cstr_at(buffer.cast_const()).to_vec())
    } else {
        None
    };
    env.mem.free(buffer.cast_void());
    filename
        .map(|filename| lua.create_string(filename))
        .transpose()
}

pub(super) fn find_stop(_: &Lua, env: &mut Environment, handle: Option<i32>) -> mlua::Result<i32> {
    Ok(dsm::mr_find_stop(env, handle.unwrap_or(0)))
}

pub(super) fn datetime(lua: &Lua, (): ()) -> mlua::Result<Table> {
    let now = Local::now();
    let table = lua.create_table()?;
    table.set("year", now.year())?;
    table.set("mon", now.month())?;
    table.set("day", now.day())?;
    table.set("hour", now.hour())?;
    table.set("min", now.minute())?;
    table.set("sec", now.second())?;
    Ok(table)
}

fn with_guest_path<R>(
    env: &mut Environment,
    filename: &[u8],
    operation: impl FnOnce(&mut Environment, ConstPtr<u8>) -> R,
) -> R {
    let filename_ptr = env.mem.alloc_and_write_cstr(filename);
    let result = operation(env, filename_ptr.cast_const());
    env.mem.free(filename_ptr.cast_void());
    result
}
