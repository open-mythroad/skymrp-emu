/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::{with_env, SharedLuaContext};
use crate::dsm;
use crate::mythroad::MrResult;
use crate::Environment;

use mlua::{
    AnyUserData, Lua, LuaString, MetaMethod, MultiValue, UserData, UserDataMethods, Value, Variadic,
};
use std::cell::Cell;
use std::rc::Rc;

const MR_FILE_RDONLY: u32 = 1;
const MR_IS_FILE: i32 = 1;
const MR_FILE_STATE_OPEN: i32 = 0;
const MR_FILE_STATE_CLOSED: i32 = 1;
const MR_FILE_STATE_NIL: i32 = 2;
const READ_BUFFER_SIZE: u32 = 256;

struct LuaFile {
    handle: Cell<u32>,
    context: SharedLuaContext,
}

impl LuaFile {
    fn closed(context: SharedLuaContext) -> Self {
        Self {
            handle: Cell::new(0),
            context,
        }
    }

    fn handle(&self) -> mlua::Result<u32> {
        match self.handle.get() {
            0 => Err(mlua::Error::runtime("attempt to use a closed file")),
            handle => Ok(handle),
        }
    }

    fn close(&self, env: &mut Environment) -> bool {
        let handle = self.handle.get();
        if handle == 0 {
            return false;
        }
        if dsm::mr_close(env, handle) != MrResult::Success as i32 {
            return false;
        }
        self.context.unregister_file(handle);
        self.handle.set(0);
        true
    }
}

impl Drop for LuaFile {
    fn drop(&mut self) {
        if self.handle.get() == 0 {
            return;
        }
        let _ = with_env(&self.context, |env| {
            self.close(env);
            Ok(())
        });
    }
}

impl UserData for LuaFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |lua, this, lengths: Variadic<u32>| {
            let handle = this.handle()?;
            let values = with_env(&this.context, |env| {
                let mut values = Vec::with_capacity(lengths.len());
                for &length in lengths.iter() {
                    let Some(bytes) = read_bytes(env, handle, length) else {
                        values.push(None);
                        break;
                    };
                    values.push(Some(bytes));
                }
                Ok(values)
            })?;

            let mut results = Vec::with_capacity(values.len());
            for value in values {
                results.push(match value {
                    Some(bytes) => Value::String(lua.create_string(bytes)?),
                    None => Value::Nil,
                });
            }
            Ok(MultiValue::from_vec(results))
        });
        methods.add_method(
            "seek",
            |lua, this, (method, offset): (Option<i32>, Option<i32>)| {
                let handle = this.handle()?;
                let offset = offset.unwrap_or(0);
                let succeeded = with_env(&this.context, |env| {
                    Ok(dsm::mr_seek(env, handle, offset, method.unwrap_or(0))
                        == MrResult::Success as i32)
                })?;
                if succeeded {
                    Ok(MultiValue::from_vec(vec![Value::Number(offset as f64)]))
                } else {
                    push_result(lua, false, None)
                }
            },
        );
        methods.add_method("write", |lua, this, values: Variadic<LuaString>| {
            let handle = this.handle()?;
            let succeeded = with_env(&this.context, |env| {
                for value in values.iter() {
                    if !write_bytes(env, handle, &value.as_bytes()) {
                        return Ok(false);
                    }
                }
                Ok(true)
            })?;
            push_result(lua, succeeded, None)
        });
        methods.add_method("close", |lua, this, ()| close_file(lua, this));
        methods.add_method("__str", |_, this, ()| Ok(file_description(this)));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(file_description(this))
        });
    }
}

pub(super) fn close(lua: &Lua, userdata: AnyUserData) -> mlua::Result<MultiValue> {
    let file = userdata.borrow::<LuaFile>()?;
    close_file(lua, &file)
}

pub(super) fn open(
    lua: &Lua,
    env: &mut Environment,
    (filename, mode): (LuaString, Option<u32>),
    context: &SharedLuaContext,
) -> mlua::Result<MultiValue> {
    let filename_bytes = filename.as_bytes();
    let userdata = lua.create_userdata(LuaFile::closed(Rc::clone(context)))?;
    let filename_ptr = env.mem.alloc_and_write_cstr(&filename_bytes);
    let handle = dsm::mr_open(
        env,
        filename_ptr.cast_const(),
        mode.unwrap_or(MR_FILE_RDONLY),
    );
    env.mem.free(filename_ptr.cast_void());

    if handle == 0 {
        return push_result(lua, false, Some(&filename_bytes));
    }

    userdata.borrow::<LuaFile>()?.handle.set(handle as u32);
    context.register_file(handle as u32);
    Ok(MultiValue::from_vec(vec![Value::UserData(userdata)]))
}

pub(super) fn state(_: &Lua, value: Value) -> mlua::Result<i32> {
    Ok(match value {
        Value::UserData(userdata) => file_state(&userdata),
        _ => MR_FILE_STATE_NIL,
    })
}

fn close_file(lua: &Lua, file: &LuaFile) -> mlua::Result<MultiValue> {
    file.handle()?;
    let succeeded = with_env(&file.context, |env| Ok(file.close(env)))?;
    push_result(lua, succeeded, None)
}

fn file_description(file: &LuaFile) -> String {
    if file.handle.get() == 0 {
        "file (closed)".to_owned()
    } else {
        format!("file ({file:p})")
    }
}

fn file_state(userdata: &AnyUserData) -> i32 {
    match userdata.borrow::<LuaFile>() {
        Ok(file) if file.handle.get() != 0 => MR_FILE_STATE_OPEN,
        Ok(_) => MR_FILE_STATE_CLOSED,
        Err(_) => MR_FILE_STATE_NIL,
    }
}

pub(super) fn read_all(
    lua: &Lua,
    env: &mut Environment,
    filename: LuaString,
) -> mlua::Result<MultiValue> {
    let filename_bytes = filename.as_bytes();
    let filename_ptr = env.mem.alloc_and_write_cstr(&filename_bytes);
    let is_file = dsm::mr_info(env, filename_ptr.cast_const()) == MR_IS_FILE;
    let length = if is_file {
        dsm::mr_get_len(env, filename_ptr.cast_const())
    } else {
        MrResult::Failed as i32
    };
    if length <= 0 {
        env.mem.free(filename_ptr.cast_void());
        return Ok(MultiValue::new());
    }

    let handle = dsm::mr_open(env, filename_ptr.cast_const(), MR_FILE_RDONLY);
    env.mem.free(filename_ptr.cast_void());
    if handle == 0 {
        return Ok(MultiValue::new());
    }

    let bytes = read_bytes(env, handle as u32, length as u32);
    dsm::mr_close(env, handle as u32);
    match bytes {
        Some(bytes) if bytes.len() == length as usize => {
            Ok(MultiValue::from_vec(vec![Value::String(
                lua.create_string(bytes)?,
            )]))
        }
        _ => Ok(MultiValue::new()),
    }
}

fn read_bytes(env: &mut Environment, handle: u32, length: u32) -> Option<Vec<u8>> {
    if length == 0 {
        return Some(Vec::new());
    }

    let buffer_length = length.min(READ_BUFFER_SIZE);
    let buffer = env.mem.alloc(buffer_length).cast::<u8>();
    let mut bytes = Vec::with_capacity(length as usize);
    while bytes.len() < length as usize {
        let remaining = length - bytes.len() as u32;
        let result = dsm::mr_read(
            env,
            handle,
            buffer.cast_void(),
            remaining.min(buffer_length),
        );
        if result <= 0 {
            break;
        }
        bytes.extend_from_slice(env.mem.bytes_at(buffer, result as u32));
    }
    env.mem.free(buffer.cast_void());

    (!bytes.is_empty()).then_some(bytes)
}

fn write_bytes(env: &mut Environment, handle: u32, bytes: &[u8]) -> bool {
    let Ok(length) = u32::try_from(bytes.len()) else {
        return false;
    };

    let buffer = env.mem.alloc(length.max(1)).cast::<u8>();
    env.mem.bytes_at_mut(buffer, length).copy_from_slice(bytes);
    let result = dsm::mr_write(env, handle, buffer.cast_const().cast_void(), length);
    env.mem.free(buffer.cast_void());
    result == length as i32
}

fn push_result(lua: &Lua, succeeded: bool, filename: Option<&[u8]>) -> mlua::Result<MultiValue> {
    if succeeded {
        return Ok(MultiValue::from_vec(vec![Value::Boolean(true)]));
    }

    let error_number = MrResult::Failed as i32;
    let error = match filename {
        Some(filename) => {
            let mut error = b"file err: ".to_vec();
            error.extend_from_slice(filename.split(|&byte| byte == 0).next().unwrap_or_default());
            error.extend_from_slice(format!(": {error_number}").as_bytes());
            error
        }
        None => format!("file err:{error_number}").into_bytes(),
    };
    Ok(MultiValue::from_vec(vec![
        Value::Nil,
        Value::String(lua.create_string(error)?),
        Value::Number(error_number as f64),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::create_lua;

    #[test]
    fn file_state_matches_mythroad_constants() {
        let context = Rc::new(super::super::LuaContext::new(1));
        let lua = create_lua(Rc::clone(&context)).unwrap();

        assert_eq!(state(&lua, Value::Nil).unwrap(), MR_FILE_STATE_NIL);

        let userdata = lua
            .create_userdata(LuaFile::closed(Rc::clone(&context)))
            .unwrap();
        userdata.borrow::<LuaFile>().unwrap().handle.set(7);
        assert_eq!(file_state(&userdata), MR_FILE_STATE_OPEN);

        userdata.borrow::<LuaFile>().unwrap().handle.set(0);
        assert_eq!(file_state(&userdata), MR_FILE_STATE_CLOSED);
    }

    #[test]
    fn failed_result_matches_mythroad_shape() {
        let lua = Lua::new();
        let result = push_result(&lua, false, Some(b"missing.dat")).unwrap();

        assert!(matches!(result.get(0), Some(Value::Nil)));
        assert_eq!(
            result
                .get(1)
                .and_then(Value::as_string)
                .unwrap()
                .to_str()
                .unwrap(),
            "file err: missing.dat: -1"
        );
        assert!(matches!(result.get(2), Some(Value::Number(-1.0))));
    }
}
