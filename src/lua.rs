/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr};
use crate::mythroad::{
    mr_exit, mr_free, mr_malloc, mr_timer_start, mr_timer_stop, MrResult, MrRunState, MrUserInfo,
    BITMAPMAX,
};
use crate::{dsm, encoding, font, mrp, Environment};

use lunify::lua50::Dialect;
use mlua::{
    ffi, FromLuaMulti, Function, IntoLuaMulti, Lua, LuaOptions, MultiValue, StdLib, Table, Value,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::CStr;
use std::os::raw::c_int;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice;

mod file;
mod string_pack;
mod sys;

pub(crate) struct LuaContext {
    environment: Cell<Option<NonNull<Environment>>>,
    state: Cell<Option<NonNull<ffi::lua_State>>>,
    guest_handle: u32,
    // Native extensions may retain pointers after a Lua API call returns.
    // Keep Guest copies of Host Lua strings alive for the lifetime of the VM.
    retained_strings: RefCell<Vec<MutPtr<u8>>>,
    open_files: RefCell<HashSet<u32>>,
    shutdown_requested: Cell<bool>,
}

pub(crate) type SharedLuaContext = Rc<LuaContext>;

pub(crate) struct LuaRuntime {
    vm: Option<Lua>,
    context: SharedLuaContext,
}

impl LuaRuntime {
    pub(crate) fn new(guest_handle: u32) -> Self {
        Self {
            vm: None,
            context: Rc::new(LuaContext::new(guest_handle)),
        }
    }
}

struct ActiveEnvironmentGuard<'a> {
    context: &'a LuaContext,
}

impl<'a> ActiveEnvironmentGuard<'a> {
    fn install(context: &'a LuaContext, env: &mut Environment) -> Self {
        assert!(
            context.environment.get().is_none(),
            "Lua runtime already has an active Environment"
        );
        context.environment.set(Some(NonNull::from(&mut *env)));

        Self { context }
    }
}

impl Drop for ActiveEnvironmentGuard<'_> {
    fn drop(&mut self) {
        let removed = self.context.environment.take();
        debug_assert!(removed.is_some());
    }
}

struct ActiveLuaStateGuard<'a> {
    context: &'a LuaContext,
    previous: Option<NonNull<ffi::lua_State>>,
}

impl<'a> ActiveLuaStateGuard<'a> {
    unsafe fn install(context: &'a LuaContext, state: *mut ffi::lua_State) -> Self {
        let state = NonNull::new(state).expect("active Lua state must not be null");
        let previous = context.state.replace(Some(state));
        Self { context, previous }
    }
}

impl Drop for ActiveLuaStateGuard<'_> {
    fn drop(&mut self) {
        self.context.state.set(self.previous);
    }
}

impl LuaContext {
    pub(crate) fn new(guest_handle: u32) -> Self {
        Self {
            environment: Cell::new(None),
            state: Cell::new(None),
            guest_handle,
            retained_strings: RefCell::new(Vec::new()),
            open_files: RefCell::new(HashSet::new()),
            shutdown_requested: Cell::new(false),
        }
    }

    fn active_environment(&self) -> Option<NonNull<Environment>> {
        self.environment.get()
    }

    pub(crate) fn active_state(&self, guest_handle: u32) -> Option<NonNull<ffi::lua_State>> {
        (guest_handle == self.guest_handle)
            .then(|| self.state.get())
            .flatten()
    }

    pub(crate) fn guest_handle(&self) -> u32 {
        self.guest_handle
    }

    fn retain_string(&self, env: &mut Environment, bytes: &[u8]) -> MutPtr<u8> {
        let ptr = env.mem.alloc_and_write_cstr(bytes);
        self.retained_strings.borrow_mut().push(ptr);
        ptr
    }

    fn clear_retained_strings(&self, env: &mut Environment) {
        for ptr in self.retained_strings.borrow_mut().drain(..) {
            env.mem.free(ptr.cast_void());
        }
    }

    fn register_file(&self, handle: u32) {
        self.open_files.borrow_mut().insert(handle);
    }

    fn unregister_file(&self, handle: u32) {
        self.open_files.borrow_mut().remove(&handle);
    }

    fn close_open_files(&self, env: &mut Environment) {
        let handles = self.open_files.borrow_mut().drain().collect::<Vec<_>>();
        for handle in handles {
            if dsm::mr_close(env, handle) != MrResult::Success as i32 {
                log_dbg!("Mythroad: failed to close Lua file handle {handle}");
            }
        }
    }
}

pub(crate) fn start(env: &mut Environment, filename: &str, entry: &str) -> Result<(), String> {
    shutdown(env);
    let context = Rc::clone(&env.mythroad.lua.context);

    let lua = create_lua(Rc::clone(&context))?;
    env.mythroad
        .state
        .vm_state
        .set(&mut env.mem, context.guest_handle());

    let entry_function = match (|| {
        register_apis(&lua, &context).map_err(|err| err.to_string())?;
        register_globals(env, &lua, entry)?;
        load_chunk(&lua, env, filename)
    })() {
        Ok(function) => function,
        Err(err) => {
            env.mythroad.state.vm_state.set(&mut env.mem, 0);
            return Err(err);
        }
    };
    env.mythroad.lua.vm = Some(lua);
    let result = with_lua(env, |_| {
        entry_function
            .call::<()>(())
            .map_err(|err| format!("failed to execute {filename}: {err}"))
    });
    if result.is_err() {
        if env.mythroad.state.mr_state.get(&env.mem) == MrRunState::Stop as u32 {
            return Ok(());
        }
        shutdown(env);
    }
    result
}

pub(crate) fn shutdown(env: &mut Environment) {
    let context = Rc::clone(&env.mythroad.lua.context);
    if context.active_environment().is_some() {
        context.shutdown_requested.set(true);
        return;
    }
    context.shutdown_requested.set(false);
    context.close_open_files(env);
    env.mythroad.lua.vm.take();
    context.clear_retained_strings(env);
    env.mythroad.state.vm_state.set(&mut env.mem, 0);
}

pub(crate) fn with_lua<R>(
    env: &mut Environment,
    f: impl FnOnce(&Lua) -> Result<R, String>,
) -> Result<R, String> {
    let context = Rc::clone(&env.mythroad.lua.context);
    let lua = env
        .mythroad
        .lua
        .vm
        .take()
        .ok_or_else(|| "Lua runtime is not initialized".to_owned())?;
    let result = {
        let _guard = ActiveEnvironmentGuard::install(&context, env);
        f(&lua)
    };
    env.mythroad.lua.vm = Some(lua);
    if context.shutdown_requested.replace(false) {
        shutdown(env);
    }
    result
}

pub(crate) fn call_global<A>(env: &mut Environment, name: &[u8], arguments: A) -> i32
where
    A: IntoLuaMulti,
{
    if env.mythroad.lua.vm.is_none() {
        return MrResult::Ignored as i32;
    }

    let result = with_lua(env, |lua| {
        let name = lua.create_string(name).map_err(|err| err.to_string())?;
        let value = lua
            .globals()
            .get::<Value>(name)
            .map_err(|err| err.to_string())?;
        let Value::Function(function) = value else {
            return Ok(false);
        };
        function
            .call::<()>(arguments)
            .map_err(|err| err.to_string())?;
        Ok(true)
    });

    match result {
        Ok(true) => MrResult::Success as i32,
        Ok(false) => MrResult::Success as i32,
        Err(err) => {
            let mr_state = env.mythroad.state.mr_state.get(&env.mem);
            if mr_state == MrRunState::Stop as u32 || mr_state == MrRunState::Restart as u32 {
                MrResult::Success as i32
            } else {
                log!("Mythroad: Lua callback failed: {err}");
                env.mythroad
                    .state
                    .mr_state
                    .set(&mut env.mem, MrRunState::Error as u32);
                MrResult::Failed as i32
            }
        }
    }
}

fn with_env<R>(
    context: &LuaContext,
    f: impl FnOnce(&mut Environment) -> mlua::Result<R>,
) -> mlua::Result<R> {
    let mut env = context
        .active_environment()
        .ok_or_else(|| mlua::Error::runtime("Lua API called outside the active runtime"))?;

    // SAFETY: with_lua installs this pointer while it exclusively borrows the
    // Environment, and no Environment reference can escape this callback.
    unsafe { f(env.as_mut()) }
}

fn create_lua(context: SharedLuaContext) -> Result<Lua, String> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|err| format!("failed to create Lua VM: {err}"))?;
    lua.set_app_data(Rc::clone(&context));

    Ok(lua)
}

fn create_api<A, R, F>(lua: &Lua, context: &SharedLuaContext, callback: F) -> mlua::Result<Function>
where
    A: FromLuaMulti,
    R: IntoLuaMulti,
    F: Fn(&Lua, &mut Environment, A) -> mlua::Result<R> + 'static,
{
    let context = Rc::clone(context);
    let function = lua.create_function(move |lua, arguments| {
        with_env(&context, |env| callback(lua, env, arguments))
    })?;
    Ok(function)
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

fn load_chunk(lua: &Lua, env: &mut Environment, filename: &str) -> Result<Function, String> {
    let data = dsm::read_current_package_file(env, filename)?;
    let data = convert_chunk(&data)?;
    lua.load(data.as_slice())
        .set_name(filename)
        .into_function()
        .map_err(|err| err.to_string())
}

fn lua_loadfile(
    lua: &Lua,
    env: &mut Environment,
    filename: mlua::LuaString,
) -> mlua::Result<MultiValue> {
    let filename = encoding::gb_to_utf8_string(&filename.as_bytes()).into_owned();
    match load_chunk(lua, env, &filename) {
        Ok(function) => Ok(MultiValue::from_vec(vec![Value::Function(function)])),
        Err(err) => Ok(MultiValue::from_vec(vec![
            Value::Nil,
            Value::String(lua.create_string(err)?),
        ])),
    }
}

fn register_apis(lua: &Lua, context: &SharedLuaContext) -> mlua::Result<()> {
    let globals = lua.globals();
    let loadfile = create_api(lua, context, lua_loadfile)?;
    globals.set("loadfile", loadfile.clone())?;
    globals.set("_loadFile", loadfile.clone())?;
    let dofile = lua.create_function(move |_, filename: mlua::LuaString| {
        let mut result = loadfile.call::<MultiValue>(filename)?.into_iter();
        match result.next() {
            Some(Value::Function(function)) => function.call::<MultiValue>(()),
            _ => {
                let message = match result.next() {
                    Some(Value::String(message)) => message.to_string_lossy(),
                    _ => "failed to load Lua file".to_owned(),
                };
                Err(mlua::Error::runtime(message))
            }
        }
    })?;
    globals.set("dofile", dofile.clone())?;
    globals.set("_execFile", dofile)?;
    for (alias, name) in [
        ("_error", "error"),
        ("_getTab", "getmetatable"),
        ("_setTab", "setmetatable"),
        ("_getEnv", "getfenv"),
        ("_setEnv", "setfenv"),
        ("_iPairs", "ipairs"),
        ("_num", "tonumber"),
        ("_str", "tostring"),
        ("_next", "next"),
        ("_loads", "loadstring"),
        ("_assert", "assert"),
        ("_rawEq", "rawequal"),
        ("_pCall", "pcall"),
        ("_pCallEx", "xpcall"),
        ("_gc", "collectgarbage"),
        ("_loadBuf", "loadstring"),
    ] {
        let function: Function = globals.get(name)?;
        globals.set(alias, function)?;
    }

    let file_table = lua.create_table()?;
    file_table.set("close", lua.create_function(file::close)?)?;
    file_table.set(
        "open",
        create_api(lua, context, {
            let context = Rc::clone(context);
            move |lua, env, arguments| file::open(lua, env, arguments, &context)
        })?,
    )?;
    file_table.set("state", lua.create_function(file::state)?)?;
    file_table.set("readAll", create_api(lua, context, file::read_all)?)?;
    globals.set("file", file_table)?;
    let string_table: Table = globals.get("string")?;
    string_table.set("pack", lua.create_function(string_pack::pack)?)?;
    string_table.set("unpack", lua.create_function(string_pack::unpack)?)?;
    string_table.set(
        "subV",
        create_api(lua, context, {
            let context = Rc::clone(context);
            move |_, env, value: mlua::LuaString| {
                let bytes = value.as_bytes();
                let len = u32::try_from(bytes.len()).map_err(mlua::Error::external)?;
                let ptr = context.retain_string(env, &bytes);
                Ok((ptr.to_bits() as f64, len as f64))
            }
        })?,
    )?;
    let test_com = create_api(lua, context, |_, env, (input0, input1): (u32, u32)| {
        Ok(dsm::test_com(env, 0, input0, input1))
    })?;
    globals.set("_com", test_com.clone())?;
    globals.set("TestCom", test_com)?;
    globals.set("_platEx", create_api(lua, context, lua_plat_ex)?)?;
    globals.set("_plat", create_api(lua, context, lua_plat)?)?;
    let get_sys_info = create_api(lua, context, lua_get_sys_info)?;
    globals.set("GetSysInfo", get_sys_info.clone())?;

    let sys_table = lua.create_table()?;
    let get_uptime = create_api(lua, context, sys::get_uptime)?;
    sys_table.set("getUptime", get_uptime.clone())?;
    sys_table.set("getuptime", get_uptime)?;
    let remove = create_api(lua, context, sys::remove)?;
    sys_table.set("rm", remove.clone())?;
    sys_table.set("remove", remove)?;
    sys_table.set("rename", create_api(lua, context, sys::rename)?)?;
    let mkdir = create_api(lua, context, sys::mkdir)?;
    sys_table.set("mkDir", mkdir.clone())?;
    sys_table.set("mkdir", mkdir)?;
    let rmdir = create_api(lua, context, sys::rmdir)?;
    sys_table.set("rmDir", rmdir.clone())?;
    sys_table.set("rmdir", rmdir)?;
    let get_file_info = create_api(lua, context, sys::get_file_info)?;
    sys_table.set("getFileInfo", get_file_info.clone())?;
    sys_table.set("getfileinfo", get_file_info)?;
    let get_file_len = create_api(lua, context, sys::get_file_len)?;
    sys_table.set("getFileLen", get_file_len.clone())?;
    sys_table.set("getfilelen", get_file_len)?;
    let find_start = create_api(lua, context, sys::find_start)?;
    sys_table.set("findStart", find_start.clone())?;
    sys_table.set("findstart", find_start)?;
    let find_next = create_api(lua, context, sys::find_next)?;
    sys_table.set("findNext", find_next.clone())?;
    sys_table.set("findnext", find_next)?;
    let find_stop = create_api(lua, context, sys::find_stop)?;
    sys_table.set("findStop", find_stop.clone())?;
    sys_table.set("findstop", find_stop)?;
    sys_table.set("getInfo", get_sys_info)?;
    let datetime = lua.create_function(sys::datetime)?;
    sys_table.set("datetime", datetime.clone())?;
    globals.set("sys", sys_table)?;
    globals.set("GetDatetime", datetime)?;

    let bitmap_load = create_api(lua, context, lua_bitmap_load)?;
    globals.set("_bmpLoad", bitmap_load.clone())?;
    globals.set("BitmapLoad", bitmap_load)?;
    globals.set("_bmpInfo", create_api(lua, context, lua_bitmap_info)?)?;
    let bitmap_new = create_api(lua, context, lua_bitmap_new)?;
    globals.set("_bmpNew", bitmap_new.clone())?;
    globals.set("BitmapNew", bitmap_new)?;
    // SAFETY: lua_test_com1 follows the Lua C function calling convention and
    // accesses the current Environment only while with_lua keeps it active.
    let str_com = unsafe { lua.create_c_function(lua_test_com1) }?;
    globals.set("_strCom", str_com.clone())?;
    globals.set("TestCom1", str_com)?;

    globals.set(
        "_t",
        lua.create_function(|_, value: Value| {
            Ok(match value {
                Value::Nil => "nil",
                Value::Boolean(_) => "bool",
                Value::LightUserData(_) | Value::UserData(_) | Value::Error(_) => "obj",
                Value::Integer(_) | Value::Number(_) => "num",
                Value::String(_) => "str",
                Value::Table(_) => "tab",
                Value::Function(_) => "func",
                Value::Thread(_) => "co",
                _ => "nil",
            })
        })?,
    )?;
    let exit = create_api(lua, context, |_, env, (): ()| {
        mr_exit(env);
        if env.mythroad.state.mr_state.get(&env.mem) == MrRunState::Stop as u32 {
            return Err(mlua::Error::runtime("Exiting..."));
        }
        Ok(())
    })?;
    globals.set("_exit", exit.clone())?;
    globals.set("Exit", exit)?;
    let timer_start = create_api(lua, context, {
        let context = Rc::clone(context);
        move |_,
              env,
              (_id, interval, callback): (
            Option<Value>,
            Option<u16>,
            Option<mlua::LuaString>,
        )| {
            let mr_state = env.mythroad.state.mr_state.get(&env.mem);
            let timer_runs_while_paused =
                env.mythroad.state.mr_timer_run_without_pause.get(&env.mem) != 0;
            if mr_state != MrRunState::Run as u32
                && !(timer_runs_while_paused && mr_state == MrRunState::Pause as u32)
            {
                return Ok(());
            }

            let callback = callback
                .map(|callback| {
                    context
                        .retain_string(env, &callback.as_bytes())
                        .cast_const()
                })
                .unwrap_or_else(ConstPtr::null);
            env.mythroad.state.mr_timer_p.set(&mut env.mem, callback);
            mr_timer_start(env, interval.unwrap_or(0));
            Ok(())
        }
    })?;
    globals.set("_timerStart", timer_start.clone())?;
    globals.set("TimerStart", timer_start)?;
    let timer_stop = create_api(lua, context, |_, env, _id: Option<Value>| {
        mr_timer_stop(env);
        Ok(())
    })?;
    globals.set("_timerStop", timer_stop.clone())?;
    globals.set("TimerStop", timer_stop)?;

    globals.set(
        "_gc",
        lua.create_function(|lua, _threshold: Option<i32>| lua.gc_collect())?,
    )?;
    let get_rand = create_api(lua, context, |_, env, limit: i32| {
        Ok(crate::mythroad::mr_rand(env) % limit)
    })?;
    globals.set("_rand", get_rand.clone())?;
    globals.set("GetRand", get_rand)?;
    globals.set(
        "_add",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left.wrapping_add(right)))?,
    )?;
    globals.set(
        "_sub",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left.wrapping_sub(right)))?,
    )?;
    globals.set(
        "_mul",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left.wrapping_mul(right)))?,
    )?;
    globals.set(
        "_div",
        lua.create_function(|_, (left, right): (i32, i32)| {
            if right == 0 {
                return Err(mlua::Error::runtime("attempt to divide by zero"));
            }
            Ok(left.wrapping_div(right))
        })?,
    )?;
    globals.set(
        "_neg",
        lua.create_function(|_, value: i32| Ok(value.wrapping_neg()))?,
    )?;
    let modulo = lua.create_function(|_, (left, right): (i32, i32)| Ok(left % right))?;
    globals.set("_mod", modulo.clone())?;
    globals.set("mod", modulo)?;
    globals.set(
        "_and",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left & right))?,
    )?;
    globals.set(
        "_or",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left | right))?,
    )?;
    globals.set(
        "_not",
        lua.create_function(|_, value: i32| Ok(i32::from(value == 0)))?,
    )?;
    globals.set(
        "_xor",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left ^ right))?,
    )?;

    Ok(())
}

fn lua_plat_ex(
    lua: &Lua,
    env: &mut Environment,
    (code, input): (Value, mlua::LuaString),
) -> mlua::Result<(mlua::LuaString, i32)> {
    let code = lua.coerce_integer(code)?.unwrap_or(0) as u32;
    let input = input.as_bytes();
    let input_len = u32::try_from(input.len()).map_err(mlua::Error::external)?;
    let input_ptr = env.mem.alloc_and_write_cstr(&input);
    let output_ptr: MutPtr<MutPtr<u8>> = env.mem.alloc_and_write(MutPtr::<u8>::null());
    let output_len_ptr: MutPtr<i32> = env.mem.alloc_and_write(0);

    let ret = dsm::mr_plat_ex(
        env,
        code,
        input_ptr.cast_const(),
        input_len,
        output_ptr,
        output_len_ptr,
        MutVoidPtr::null(),
    );

    let output = env.mem.read(output_ptr);
    let output_len = env.mem.read(output_len_ptr);
    let output = if !output.is_null() && output_len > 0 {
        env.mem
            .bytes_at(output.cast_const(), output_len as u32)
            .to_vec()
    } else {
        Vec::new()
    };

    env.mem.free(input_ptr.cast_void());
    env.mem.free(output_ptr.cast_void());
    env.mem.free(output_len_ptr.cast_void());

    Ok((lua.create_string(output)?, ret))
}

fn lua_plat(lua: &Lua, env: &mut Environment, arguments: MultiValue) -> mlua::Result<i32> {
    Ok(dsm::mr_plat(
        env,
        mr_to_number(lua, &arguments, 1, 0)? as u32,
        mr_to_number(lua, &arguments, 2, 0)? as u32,
    ))
}

fn lua_bitmap_load(
    _: &Lua,
    env: &mut Environment,
    (index, filename, x, y, width, height, source_width): (
        u16,
        mlua::LuaString,
        i16,
        i16,
        u16,
        u16,
        u16,
    ),
) -> mlua::Result<()> {
    let filename = encoding::gb_to_utf8_string(&filename.as_bytes()).into_owned();
    if env.mythroad.state.bi.get(&env.mem) & 1 == 0 {
        return Err(mlua::Error::runtime(format!(
            "BitmapLoad:cannot read File \"{filename}\"!"
        )));
    }
    if u32::from(index) > BITMAPMAX {
        return Err(mlua::Error::runtime(format!(
            "BitmapLoad:index {index} invalid!"
        )));
    }

    let bitmap_ptr = env.mythroad.state.mr_bitmap + u32::from(index);
    let mut bitmap = env.mem.read(bitmap_ptr);
    if !bitmap.p.is_null() {
        mr_free(env, bitmap.p.cast_void(), bitmap.buflen);
        bitmap.p = MutPtr::null();
        bitmap.buflen = 0;
        env.mem.write(bitmap_ptr, bitmap);
    }
    if filename.starts_with('*') {
        return Ok(());
    }

    let file_data = dsm::read_current_package_file(env, &filename).map_err(|_| {
        mlua::Error::runtime(format!("BitmapLoad {index}:cannot read \"{filename}\"!"))
    })?;
    let bitmap_len = u32::from(width)
        .checked_mul(u32::from(height))
        .and_then(|pixels| pixels.checked_mul(std::mem::size_of::<u16>() as u32))
        .ok_or_else(|| mlua::Error::runtime("BitmapLoad: bitmap size overflow"))?;

    let pixels = if x == 0 && y == 0 && width == source_width {
        file_data
    } else {
        let x =
            u32::try_from(x).map_err(|_| mlua::Error::runtime("BitmapLoad: negative source x"))?;
        let y =
            u32::try_from(y).map_err(|_| mlua::Error::runtime("BitmapLoad: negative source y"))?;
        let source_width = u32::from(source_width);
        let right = x
            .checked_add(u32::from(width))
            .ok_or_else(|| mlua::Error::runtime("BitmapLoad: source x overflow"))?;
        if right > source_width {
            return Err(mlua::Error::runtime("BitmapLoad: source row overflow"));
        }
        let bottom = y
            .checked_add(u32::from(height))
            .ok_or_else(|| mlua::Error::runtime("BitmapLoad: source y overflow"))?;
        let required_len = bottom
            .checked_mul(source_width)
            .and_then(|pixels| pixels.checked_mul(std::mem::size_of::<u16>() as u32))
            .ok_or_else(|| mlua::Error::runtime("BitmapLoad: source size overflow"))?;
        if required_len as usize > file_data.len() {
            return Err(mlua::Error::runtime(format!(
                "BitmapLoad {index} \"{filename}\":len err!"
            )));
        }

        let row_len = usize::from(width) * std::mem::size_of::<u16>();
        let source_stride = source_width as usize * std::mem::size_of::<u16>();
        let source_x = x as usize * std::mem::size_of::<u16>();
        let mut pixels = Vec::with_capacity(bitmap_len as usize);
        for row in y..bottom {
            let start = row as usize * source_stride + source_x;
            pixels.extend_from_slice(&file_data[start..start + row_len]);
        }
        pixels
    };

    let pixel_len = u32::try_from(pixels.len())
        .map_err(|_| mlua::Error::runtime("BitmapLoad: file too large"))?;
    let pixel_ptr = mr_malloc(env, pixel_len).cast::<u8>();
    if pixel_ptr.is_null() {
        return Err(mlua::Error::runtime(format!(
            "BitmapLoad {index} \"{filename}\":No memory!"
        )));
    }
    env.mem
        .bytes_at_mut(pixel_ptr, pixel_len)
        .copy_from_slice(&pixels);

    bitmap.w = width;
    bitmap.h = height;
    bitmap.buflen = pixel_len;
    bitmap.p = pixel_ptr.cast();
    env.mem.write(bitmap_ptr, bitmap);
    Ok(())
}

fn lua_bitmap_info(
    lua: &Lua,
    env: &mut Environment,
    arguments: MultiValue,
) -> mlua::Result<(u32, u32, u16, u16, u32)> {
    let index = mr_to_number(lua, &arguments, 1, 0)? as u16;
    if u32::from(index) > BITMAPMAX {
        return Err(mlua::Error::runtime(format!(
            "MRF_BitmapInfo:index {index} invalid!"
        )));
    }

    let bitmap = env
        .mem
        .read(env.mythroad.state.mr_bitmap + u32::from(index));
    Ok((
        bitmap.p.to_bits(),
        bitmap.buflen,
        bitmap.w,
        bitmap.h,
        bitmap.type_,
    ))
}

fn lua_bitmap_new(lua: &Lua, env: &mut Environment, arguments: MultiValue) -> mlua::Result<()> {
    let index = mr_to_number(lua, &arguments, 1, 0)? as u16;
    let width = mr_to_number(lua, &arguments, 2, 0)? as u16;
    let height = mr_to_number(lua, &arguments, 3, 0)? as u16;
    if u32::from(index) > BITMAPMAX {
        return Err(mlua::Error::runtime(format!(
            "BitmapNew:index {index} invalid!"
        )));
    }

    let bitmap_ptr = env.mythroad.state.mr_bitmap + u32::from(index);
    let mut bitmap = env.mem.read(bitmap_ptr);
    let bitmap_len = u32::from(width)
        .checked_mul(u32::from(height))
        .and_then(|pixels| pixels.checked_mul(std::mem::size_of::<u16>() as u32))
        .ok_or_else(|| mlua::Error::runtime("BitmapNew: bitmap size overflow"))?;
    if bitmap.buflen != bitmap_len {
        if !bitmap.p.is_null() {
            mr_free(env, bitmap.p.cast_void(), bitmap.buflen);
            bitmap.p = MutPtr::null();
        }
        bitmap.p = mr_malloc(env, bitmap_len).cast();
        if bitmap.p.is_null() {
            return Err(mlua::Error::runtime(format!(
                "BitmapNew {index} :No memory!"
            )));
        }
        libc::string::memset(env, bitmap.p.cast_void(), 0, bitmap_len);
    }
    bitmap.buflen = bitmap_len;
    bitmap.w = width;
    bitmap.h = height;
    env.mem.write(bitmap_ptr, bitmap);
    Ok(())
}

fn lua_get_sys_info(
    lua: &Lua,
    env: &mut Environment,
    font_size: Option<u16>,
) -> mlua::Result<Table> {
    let font_size = font_size.unwrap_or(font::FontSize::Medium as u16);
    let (chinese_width, chinese_height) = font::measure_char(env, 0x70b9, font_size);
    let (english_width, english_height) = font::measure_char(env, 0x0032, font_size);
    let screen_width = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_height = env.mythroad.state.mr_screen_h.get(&env.mem);
    let pack_name = lua.create_string(
        env.mem
            .cstr_at(env.mythroad.state.pack_filename.cast_const()),
    )?;
    let user_info = MrUserInfo::default();

    let table = lua.create_table()?;
    table.set("vmver", mrp::MR_VERSION)?;
    table.set("ScreenW", screen_width)?;
    table.set("ScreenH", screen_height)?;
    table.set("scrw", screen_width)?;
    table.set("scrh", screen_height)?;
    table.set("ChineseWidth", chinese_width)?;
    table.set("ChineseHigh", chinese_height)?;
    table.set("chw", chinese_width)?;
    table.set("chh", chinese_height)?;
    table.set("EnglishWidth", english_width)?;
    table.set("EnglishHigh", english_height)?;
    table.set("ascw", english_width)?;
    table.set("asch", english_height)?;
    table.set("PackName", pack_name.clone())?;
    table.set("packname", pack_name)?;
    table.set(
        "hsman",
        lua.create_string(
            CStr::from_bytes_until_nul(&user_info.manufactory)
                .unwrap()
                .to_bytes(),
        )?,
    )?;
    table.set(
        "hstype",
        lua.create_string(
            CStr::from_bytes_until_nul(&user_info.r#type)
                .unwrap()
                .to_bytes(),
        )?,
    )?;
    table.set(
        "IMEI",
        lua.create_string(
            CStr::from_bytes_until_nul(&user_info.imei)
                .unwrap()
                .to_bytes(),
        )?,
    )?;
    table.set(
        "IMSI",
        lua.create_string(
            CStr::from_bytes_until_nul(&user_info.imsi)
                .unwrap()
                .to_bytes(),
        )?,
    )?;
    table.set("hsver", user_info.ver)?;

    Ok(table)
}

unsafe extern "C-unwind" fn lua_test_com1(state: *mut ffi::lua_State) -> c_int {
    catch_unwind(AssertUnwindSafe(|| {
        let lua = unsafe { Lua::get_or_init_from_ptr(state) };
        let context = lua
            .app_data_ref::<SharedLuaContext>()
            .expect("Lua context is not installed");
        let context = Rc::clone(&context);

        let mut env = context
            .active_environment()
            .expect("Environment is not active");
        let _state_guard = unsafe { ActiveLuaStateGuard::install(&context, state) };
        let env = unsafe { env.as_mut() };

        let input0 = unsafe { ffi::lua_tointeger(state, 1) } as u32;
        let (input_ptr, input_len) = if unsafe { ffi::lua_type(state, 2) } == ffi::LUA_TTABLE {
            unsafe { ffi::lua_rawgeti(state, 2, 1) };
            let ptr = MutPtr::from_bits(unsafe { ffi::lua_tointeger(state, -1) } as u32);
            unsafe { ffi::lua_pop(state, 1) };

            unsafe { ffi::lua_rawgeti(state, 2, 2) };
            let len = unsafe { ffi::lua_tointeger(state, -1) } as u32;
            unsafe { ffi::lua_pop(state, 1) };
            (ptr, len)
        } else {
            let mut len = 0usize;
            let bytes = unsafe { ffi::lua_tolstring(state, 2, &mut len) };
            if bytes.is_null() {
                return 0;
            }
            let Ok(len_u32) = u32::try_from(len) else {
                return 0;
            };
            let bytes = unsafe { slice::from_raw_parts(bytes.cast::<u8>(), len) };

            if matches!(input0, 2 | 800) {
                let ptr = mr_malloc(env, len_u32).cast::<u8>();
                if ptr.is_null() {
                    unsafe { ffi::lua_pushnumber(state, MrResult::Failed as i32 as f64) };
                    return 1;
                }
                env.mem.bytes_at_mut(ptr, len_u32).copy_from_slice(bytes);
                (ptr, len_u32)
            } else {
                (context.retain_string(env, bytes), len_u32)
            }
        };

        dsm::test_com1(env, context.guest_handle(), input0, input_ptr, input_len)
    }))
    .unwrap_or_else(|_| {
        log!("Mythroad: panic while handling _strCom");
        0
    })
}

pub(crate) fn to_mr_tonumber(
    env: &Environment,
    guest_state: u32,
    index: c_int,
    default: i32,
) -> i32 {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    if unsafe { ffi::lua_gettop(state) } < index.abs() {
        default
    } else {
        unsafe { ffi::lua_tointeger(state, index) as i32 }
    }
}

pub(crate) fn mr_l_optint(env: &Environment, guest_state: u32, index: c_int, default: i32) -> i32 {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    let value_type = unsafe { ffi::lua_type(state, index) };
    if value_type == ffi::LUA_TNONE || value_type == ffi::LUA_TNIL {
        default
    } else if unsafe { ffi::lua_isnumber(state, index) } != 0 {
        unsafe { ffi::lua_tointeger(state, index) as i32 }
    } else {
        default
    }
}

pub(crate) fn mr_l_optstring(
    env: &Environment,
    guest_state: u32,
    index: c_int,
    default: &[u8],
) -> Vec<u8> {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    let value_type = unsafe { ffi::lua_type(state, index) };
    if value_type == ffi::LUA_TNONE || value_type == ffi::LUA_TNIL {
        return default.to_vec();
    }

    let mut len = 0;
    let value = unsafe { ffi::lua_tolstring(state, index, &mut len) };
    if value.is_null() {
        default.to_vec()
    } else {
        unsafe { slice::from_raw_parts(value.cast(), len) }.to_vec()
    }
}

pub(crate) fn mrp_pushnumber(env: &Environment, guest_state: u32, value: i32) {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    assert_ne!(unsafe { ffi::lua_checkstack(state, 1) }, 0);
    unsafe { ffi::lua_pushnumber(state, value as f64) };
}

pub(crate) fn mrp_pushlstring(env: &Environment, guest_state: u32, bytes: &[u8]) {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    assert_ne!(unsafe { ffi::lua_checkstack(state, 1) }, 0);
    unsafe { ffi::lua_pushlstring(state, bytes.as_ptr().cast(), bytes.len()) };
}

pub(crate) fn mrp_pushnil(env: &Environment, guest_state: u32) {
    let state = env
        .mythroad
        .lua
        .context
        .active_state(guest_state)
        .map(NonNull::as_ptr)
        .expect("Lua state is not active");
    assert_ne!(unsafe { ffi::lua_checkstack(state, 1) }, 0);
    unsafe { ffi::lua_pushnil(state) };
}

fn register_globals(env: &Environment, lua: &Lua, entry: &str) -> Result<(), String> {
    let globals = lua.globals();
    let entry = lua
        .create_string(entry.as_bytes())
        .map_err(|err| err.to_string())?;
    let parameter = lua
        .create_string(
            env.mem
                .cstr_at(env.mythroad.state.start_file_parameter.cast_const()),
        )
        .map_err(|err| err.to_string())?;

    globals
        .set("_mr_entry", entry)
        .map_err(|err| err.to_string())?;
    globals
        .set("_mr_param", parameter)
        .map_err(|err| err.to_string())
}

fn convert_chunk(mr_data: &[u8]) -> Result<Vec<u8>, String> {
    if !mr_data.starts_with(mrp::MRP_SIGNATURE.as_bytes()) {
        return Ok(mr_data.to_vec());
    }

    let settings = lunify::Settings {
        lua50: lunify::lua50::Settings {
            binary_signature: mrp::MRP_SIGNATURE,
            dialect: Dialect::Mythroad,
            ..Default::default()
        },
        ..Default::default()
    };

    lunify::unify(mr_data, &lunify::Format::default(), &settings)
        .map_err(|err| format!("failed to convert Mythroad bytecode: {err:?}"))
}
