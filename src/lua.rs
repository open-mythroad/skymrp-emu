/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::mem::MutPtr;
use crate::mythroad::{mr_malloc, MrResult, MrUserInfo};
use crate::{dsm, font, mrp, Environment};

use lunify::lua50::Dialect;
use mlua::{ffi, FromLuaMulti, Function, IntoLuaMulti, Lua, LuaOptions, StdLib, Table};
use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::os::raw::c_int;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice;

mod file;
mod string_pack;

pub(crate) struct LuaContext {
    environment: Cell<Option<NonNull<Environment>>>,
    state: Cell<Option<NonNull<ffi::lua_State>>>,
    guest_handle: u32,
    // Native extensions may retain pointers after a Lua API call returns.
    // Keep Guest copies of Host Lua strings alive for the lifetime of the VM.
    retained_strings: RefCell<Vec<MutPtr<u8>>>,
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
}

pub(crate) fn start(env: &mut Environment, filename: &str, entry: &str) -> Result<(), String> {
    env.mythroad.lua.vm.take();
    let context = Rc::clone(&env.mythroad.lua.context);
    context.clear_retained_strings(env);
    env.mythroad.state.vm_state.set(&mut env.mem, 0);

    let mr_data = dsm::read_current_package_file(env, filename)?;
    let lua = create_lua(Rc::clone(&context))?;
    env.mythroad
        .state
        .vm_state
        .set(&mut env.mem, context.guest_handle());

    let lua_data = match (|| {
        register_apis(&lua, &context).map_err(|err| err.to_string())?;
        register_globals(env, &lua, entry)?;
        convert_chunk(&mr_data)
    })() {
        Ok(lua_data) => lua_data,
        Err(err) => {
            env.mythroad.state.vm_state.set(&mut env.mem, 0);
            return Err(err);
        }
    };
    env.mythroad.lua.vm = Some(lua);
    let result = with_lua(env, |lua| {
        lua.load(lua_data.as_slice())
            .set_name(filename)
            .exec()
            .map_err(|err| format!("failed to execute {filename}: {err}"))
    });
    if result.is_err() {
        env.mythroad.lua.vm.take();
        context.clear_retained_strings(env);
        env.mythroad.state.vm_state.set(&mut env.mem, 0);
    }
    result
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
    result
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

fn register_apis(lua: &Lua, context: &SharedLuaContext) -> mlua::Result<()> {
    let globals = lua.globals();
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
    globals.set("GetSysInfo", create_api(lua, context, lua_get_sys_info)?)?;
    // SAFETY: lua_test_com1 follows the Lua C function calling convention and
    // accesses the current Environment only while with_lua keeps it active.
    let str_com = unsafe { lua.create_c_function(lua_test_com1) }?;
    globals.set("_strCom", str_com.clone())?;
    globals.set("TestCom1", str_com)?;

    globals.set(
        "_gc",
        lua.create_function(|lua, _threshold: Option<i32>| lua.gc_collect())?,
    )?;
    globals.set(
        "_and",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left & right))?,
    )?;
    globals.set(
        "_or",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left | right))?,
    )?;
    globals.set(
        "_xor",
        lua.create_function(|_, (left, right): (i32, i32)| Ok(left ^ right))?,
    )?;

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
