/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::dsm;
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr};
use crate::Environment;

pub(super) fn mr_open(env: &mut Environment, filename: ConstPtr<u8>, mode: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_open(filename={filename:?}, mode={mode:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_open(env, filename, mode)
}

pub(super) fn mr_close(env: &mut Environment, handle: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_close(handle={handle:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_close(env, handle)
}

pub(super) fn mr_info(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_info(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_info(env, filename)
}

pub(super) fn mr_write(env: &mut Environment, handle: u32, buffer: ConstVoidPtr, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_write(handle={handle:#x}, buffer={buffer:?}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_write(env, handle, buffer, len)
}

pub(super) fn mr_read(env: &mut Environment, handle: u32, buffer: MutVoidPtr, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_read(handle={handle:#x}, buffer={buffer:?}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_read(env, handle, buffer, len)
}

pub(super) fn mr_seek(env: &mut Environment, handle: u32, pos: i32, method: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_seek(handle={handle:#x}, pos={pos:#x}, method={method}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_seek(env, handle, pos, method)
}

pub(super) fn mr_get_len(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getLen(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_get_len(env, filename)
}

pub(super) fn mr_remove(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_remove(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_remove(env, filename)
}

pub(super) fn mr_rename(
    env: &mut Environment,
    oldname: ConstPtr<u8>,
    newname: ConstPtr<u8>,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_rename(oldname={oldname:?}, newname={newname:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_rename(env, oldname, newname)
}

pub(super) fn mr_mkdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_mkDir(name={name:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_mkdir(env, name)
}

pub(super) fn mr_rmdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_rmDir(name={name:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_rmdir(env, name)
}

pub(super) fn mr_find_start(
    env: &mut Environment,
    name: ConstPtr<u8>,
    buffer: MutPtr<u8>,
    len: u32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_findStart(name={name:?}, buffer={buffer:?}, len={len}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_find_start(env, name, buffer, len)
}

pub(super) fn mr_find_get_next(
    env: &mut Environment,
    search_handle: i32,
    buffer: MutPtr<u8>,
    len: u32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_findGetNext(search_handle={search_handle}, buffer={buffer:?}, len={len}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_find_get_next(env, search_handle, buffer, len)
}

pub(super) fn mr_find_stop(env: &mut Environment, search_handle: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_findStop(search_handle={search_handle}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_find_stop(env, search_handle)
}

pub(super) fn mr_read_file(
    env: &mut Environment,
    filename: ConstPtr<u8>,
    filelen: MutPtr<i32>,
    lookfor: i32,
) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: _mr_readFile(filename={:#x}, filelen={:#x}, lookfor={lookfor}) called from {:#x}",
        filename.to_bits(),
        filelen.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    dsm::mr_read_file(env, filename, filelen, lookfor)
}
