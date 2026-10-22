/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use mlua::{Lua, Value};

use crate::{dsm, Environment};

pub(super) fn init(lua: &Lua) -> mlua::Result<Value> {
    log_dbg!("Mythroad: Lua network is not implemented");
    let socket = lua.create_table()?;
    socket.set("state", 5)?;
    lua.globals().set("socket", socket.clone())?;
    Ok(Value::Table(socket))
}

pub(super) fn close(lua: &Lua, env: &mut Environment, _: ()) -> mlua::Result<i32> {
    let result = dsm::test_com(env, 0, 405, 0);
    if let Value::Table(socket) = lua.globals().get::<Value>("socket")? {
        socket.set("state", 3)?;
    }
    Ok(result)
}
