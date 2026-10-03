/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::GuestFunction;
use crate::libc;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::Environment;
use std::net::ToSocketAddrs;
use std::time::Instant;

use super::MrResult;

const DSM_SUPPORT_SOCK_NUM: usize = 5;
const MR_SOCK_STREAM: i32 = 0;
const MR_SOCK_DGRAM: i32 = 1;
const MR_SOCKET_NONBLOCK: i32 = 1;
const CMWAP_PROXY_IP: u32 = 0x0a0000ac;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum SocketStatus {
    Close,
    Open,
    Connecting,
    Connected,
    Err,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum SocketReadStatus {
    NoRead,
    Readable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum SocketWriteStatus {
    NoWrite,
    Writeable,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct SocketSlot {
    socket_id: i32,
    status: SocketStatus,
    read_status: SocketReadStatus,
    write_status: SocketWriteStatus,
    is_proxy: bool,
    real_socket_id: i32,
    real_connected: bool,
}

impl Default for SocketSlot {
    fn default() -> Self {
        Self {
            socket_id: -1,
            status: SocketStatus::Close,
            read_status: SocketReadStatus::NoRead,
            write_status: SocketWriteStatus::NoWrite,
            is_proxy: false,
            real_socket_id: -1,
            real_connected: false,
        }
    }
}

pub struct State {
    pub initialized: bool,
    pub init_callback: GuestFunction,
    pub mode: Option<String>,
    sockets: [SocketSlot; DSM_SUPPORT_SOCK_NUM],
}

impl State {
    pub fn new() -> Self {
        Self {
            initialized: false,
            init_callback: GuestFunction::from_addr_with_thumb_bit(0),
            mode: None,
            sockets: [SocketSlot::default(); DSM_SUPPORT_SOCK_NUM],
        }
    }

    fn reset_sockets(&mut self) {
        self.sockets = [SocketSlot::default(); DSM_SUPPORT_SOCK_NUM];
    }

    fn free_socket_index(&self) -> Option<usize> {
        self.sockets
            .iter()
            .position(|socket| socket.socket_id == -1)
    }

    fn socket_index(&self, socket: i32) -> Option<usize> {
        let index: usize = socket.try_into().ok()?;
        (index < self.sockets.len() && self.sockets[index].socket_id != -1).then_some(index)
    }
}

pub(crate) fn mr_init_network(
    env: &mut Environment,
    callback: GuestFunction,
    mode: ConstPtr<u8>,
) -> i32 {
    let mode_string = if mode.is_null() {
        String::new()
    } else {
        String::from_utf8_lossy(env.mem.cstr_at(mode)).into_owned()
    };

    log_dbg!(
        "Mythroad: mr_initNetwork(callback={callback:?}, mode={mode_string:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    mr_close_network(env);

    let network = &mut env.mythroad.state.network;
    network.reset_sockets();
    network.init_callback = callback;
    network.mode = Some(mode_string.clone());

    if mode_string == "cmwap" {
        network.initialized = false;
        MrResult::Failed as i32
    } else {
        network.initialized = true;
        MrResult::Success as i32
    }
}

pub(crate) fn mr_socket(env: &mut Environment, type_: i32, protocol: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_socket(type={type_}, protocol={protocol}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(index) = env.mythroad.state.network.free_socket_index() else {
        return MrResult::Failed as i32;
    };

    let socket_type = match type_ {
        MR_SOCK_STREAM => libc::sys::socket::SOCK_STREAM,
        MR_SOCK_DGRAM => libc::sys::socket::SOCK_DGRAM,
        _ => {
            log!("Warning: mr_socket unsupported type {type_}, returning MR_FAILED");
            return MrResult::Failed as i32;
        }
    };

    let fd = libc::sys::socket::socket(env, libc::sys::socket::AF_INET, socket_type, 0);
    if fd < 0 {
        log!("Warning: mr_socket failed to create host socket, returning MR_FAILED");
        return MrResult::Failed as i32;
    }

    let (read_status, write_status) = if type_ == MR_SOCK_STREAM {
        (SocketReadStatus::NoRead, SocketWriteStatus::NoWrite)
    } else {
        (SocketReadStatus::Readable, SocketWriteStatus::Writeable)
    };

    env.mythroad.state.network.sockets[index] = SocketSlot {
        socket_id: fd,
        status: SocketStatus::Open,
        read_status,
        write_status,
        ..SocketSlot::default()
    };

    index.try_into().unwrap()
}

pub(crate) fn mr_connect(
    env: &mut Environment,
    socket: i32,
    ip: u32,
    port: u16,
    type_: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_connect(socket={socket}, ip={ip:#010x}, port={port}, type={type_}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_connect invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };

    match env.mythroad.state.network.sockets[index].status {
        SocketStatus::Connected => return MrResult::Success as i32,
        SocketStatus::Err | SocketStatus::Close => return MrResult::Failed as i32,
        SocketStatus::Connecting => return MrResult::Waiting as i32,
        SocketStatus::Open => {}
    }

    if ip == CMWAP_PROXY_IP {
        log!("Mythroad: mr_connect detected CMWAP proxy socket {socket}");
        set_socket_proxy_connected(env, index);
        return MrResult::Success as i32;
    }

    let fd = env.mythroad.state.network.sockets[index].socket_id;
    let sockaddr = libc::sys::socket::sockaddr::from_ipv4_parts(ip.to_be_bytes(), port);

    if type_ == MR_SOCKET_NONBLOCK {
        // TODO: MR_SOCKET_NONBLOCK currently still uses blocking connect and
        // does not return MR_WAITING. This keeps networking simple until we
        // need full Mythroad asynchronous connect semantics.
        log!("Warning: MR_SOCKET_NONBLOCK connect does not return MR_WAITING yet");
    }

    log!(
        "Mythroad: mr_connect begin backend connect socket={socket}, fd={fd}, ip={ip:#010x}, port={port}"
    );
    let start = Instant::now();
    if libc::sys::socket::connect_sockaddr(env, fd, sockaddr) == 0 {
        log!(
            "Mythroad: mr_connect backend connected socket={socket}, fd={fd}, elapsed={:?}",
            start.elapsed()
        );
        set_socket_connected(env, index);
        MrResult::Success as i32
    } else {
        log!(
            "Mythroad: mr_connect backend failed socket={socket}, fd={fd}, elapsed={:?}",
            start.elapsed()
        );
        env.mythroad.state.network.sockets[index].status = SocketStatus::Err;
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_close_socket(env: &mut Environment, socket: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_closeSocket(socket={socket}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_closeSocket invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };

    if close_socket_index(env, index) {
        MrResult::Success as i32
    } else {
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_close_network(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_closeNetwork() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let mut ok = true;
    for index in 0..DSM_SUPPORT_SOCK_NUM {
        if env.mythroad.state.network.sockets[index].socket_id != -1 {
            ok &= close_socket_index(env, index);
        }
    }
    env.mythroad.state.network.initialized = false;
    env.mythroad.state.network.mode = None;

    if ok {
        MrResult::Success as i32
    } else {
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_get_host_by_name(
    env: &mut Environment,
    name: ConstPtr<u8>,
    callback: GuestFunction,
) -> i32 {
    let host = String::from_utf8_lossy(env.mem.cstr_at(name)).into_owned();
    log_dbg!(
        "Mythroad: mr_getHostByName(name={host:?}, callback={callback:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    // The callback is part of the Mythroad async DNS API, but this simplified
    // backend resolves synchronously and returns the IP address directly.
    let Some(ip) = resolve_host(&host) else {
        log!("Warning: mr_getHostByName failed to resolve {host:?}");
        return MrResult::Failed as i32;
    };

    u32::from_be_bytes(ip) as i32
}

pub(crate) fn mr_recv(env: &mut Environment, socket: i32, buffer: MutVoidPtr, len: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_recv(socket={socket}, buffer={:#x}, len={len}) called from {:#x}",
        buffer.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(length) = len.try_into().ok() else {
        return MrResult::Failed as i32;
    };
    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_recv invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };
    let Some(index) = recv_socket_index(env, index) else {
        return 0;
    };

    let slot = env.mythroad.state.network.sockets[index];
    if slot.status == SocketStatus::Err {
        return MrResult::Failed as i32;
    }
    if slot.read_status != SocketReadStatus::Readable {
        return 0;
    }

    let ret = libc::sys::socket::recv(env, slot.socket_id, buffer, length, 0);
    if ret < 0 {
        set_socket_read_error(env, index);
    }
    ret
}

pub(crate) fn mr_recvfrom(
    env: &mut Environment,
    socket: i32,
    buffer: MutVoidPtr,
    len: i32,
    ip: MutPtr<i32>,
    port: MutPtr<u16>,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_recvfrom(socket={socket}, buffer={:#x}, len={len}, ip={:#x}, port={:#x}) called from {:#x}",
        buffer.to_bits(),
        ip.to_bits(),
        port.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(length) = len.try_into().ok() else {
        return MrResult::Failed as i32;
    };
    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_recvfrom invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };
    if ip.is_null() || port.is_null() {
        return MrResult::Failed as i32;
    }

    let slot = env.mythroad.state.network.sockets[index];
    if slot.status == SocketStatus::Err {
        return MrResult::Failed as i32;
    }
    if slot.read_status != SocketReadStatus::Readable {
        return 0;
    }

    let (ret, address) =
        libc::sys::socket::recvfrom_sockaddr(env, slot.socket_id, buffer, length, 0);
    if ret < 0 {
        set_socket_read_error(env, index);
    } else if let Some(address) = address {
        let (addr_ip, addr_port) = address.to_ipv4_parts();
        env.mem.write(ip, i32::from_be_bytes(addr_ip));
        env.mem.write(port, addr_port);
    }
    ret
}

pub(crate) fn mr_send(env: &mut Environment, socket: i32, buffer: ConstVoidPtr, len: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_send(socket={socket}, buffer={:#x}, len={len}) called from {:#x}",
        buffer.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(length) = len.try_into().ok() else {
        return MrResult::Failed as i32;
    };
    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_send invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };
    let Some(index) = send_socket_index(env, index, buffer, length) else {
        return MrResult::Failed as i32;
    };

    let slot = env.mythroad.state.network.sockets[index];
    if slot.status == SocketStatus::Err {
        return MrResult::Failed as i32;
    }
    if slot.write_status != SocketWriteStatus::Writeable {
        return 0;
    }

    let ret = libc::sys::socket::send(env, slot.socket_id, buffer.cast_mut(), length, 0);
    if ret < 0 {
        set_socket_write_error(env, index);
    }
    ret
}

pub(crate) fn mr_sendto(
    env: &mut Environment,
    socket: i32,
    buffer: ConstVoidPtr,
    len: i32,
    ip: i32,
    port: u16,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_sendto(socket={socket}, buffer={:#x}, len={len}, ip={ip:#010x}, port={port}) called from {:#x}",
        buffer.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Some(length) = len.try_into().ok() else {
        return MrResult::Failed as i32;
    };
    let Some(index) = env.mythroad.state.network.socket_index(socket) else {
        log!("Warning: mr_sendto invalid socket {socket}, returning MR_FAILED");
        return MrResult::Failed as i32;
    };

    let slot = env.mythroad.state.network.sockets[index];
    if slot.status == SocketStatus::Err {
        return MrResult::Failed as i32;
    }
    if slot.write_status != SocketWriteStatus::Writeable {
        return 0;
    }

    let address = libc::sys::socket::sockaddr::from_ipv4_parts(ip.to_be_bytes(), port);
    let ret = libc::sys::socket::sendto_sockaddr(env, slot.socket_id, buffer, length, 0, address);
    if ret < 0 {
        set_socket_write_error(env, index);
    }
    ret
}

fn close_socket_index(env: &mut Environment, index: usize) -> bool {
    let slot = env.mythroad.state.network.sockets[index];
    let mut ok = true;

    if slot.is_proxy && slot.real_socket_id != -1 {
        if let Some(real_index) = env.mythroad.state.network.socket_index(slot.real_socket_id) {
            ok &= close_socket_index(env, real_index);
        }
    }

    if libc::posix_io::close(env, slot.socket_id) == 0 {
        env.mythroad.state.network.sockets[index] = SocketSlot::default();
    } else {
        env.mythroad.state.network.sockets[index].status = SocketStatus::Err;
        ok = false;
    }

    ok
}

fn recv_socket_index(env: &mut Environment, index: usize) -> Option<usize> {
    let slot = env.mythroad.state.network.sockets[index];
    if !slot.is_proxy {
        return Some(index);
    }
    if !slot.real_connected {
        return None;
    }
    env.mythroad.state.network.socket_index(slot.real_socket_id)
}

fn send_socket_index(
    env: &mut Environment,
    index: usize,
    buffer: ConstVoidPtr,
    length: GuestUSize,
) -> Option<usize> {
    let slot = env.mythroad.state.network.sockets[index];
    if !slot.is_proxy {
        return Some(index);
    }
    if slot.real_connected {
        return env.mythroad.state.network.socket_index(slot.real_socket_id);
    }

    let target = {
        let bytes = env.mem.bytes_at(buffer.cast(), length);
        log!(
            "Mythroad: CMWAP proxy first send, parsing Host header from {} bytes",
            bytes.len()
        );
        parse_host_header(bytes)
    };
    let Some((host, port)) = target else {
        log!("Warning: CMWAP proxy send failed to parse Host header");
        set_socket_write_error(env, index);
        return None;
    };
    log!("Mythroad: CMWAP proxy Host parsed as {host}:{port}");

    let resolve_start = Instant::now();
    let Some(ip) = resolve_host(&host) else {
        log!(
            "Warning: CMWAP proxy send failed to resolve {host}:{port} after {:?}",
            resolve_start.elapsed()
        );
        set_socket_write_error(env, index);
        return None;
    };
    log!(
        "Mythroad: CMWAP proxy resolved {host}:{port} to {}.{}.{}.{} after {:?}",
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        resolve_start.elapsed()
    );

    let real_socket = mr_socket(env, MR_SOCK_STREAM, libc::netdb::IPPROTO_TCP);
    if real_socket < 0 {
        set_socket_write_error(env, index);
        return None;
    }
    let real_index = env.mythroad.state.network.socket_index(real_socket)?;
    env.mythroad.state.network.sockets[index].real_socket_id = real_socket;
    let fd = env.mythroad.state.network.sockets[real_index].socket_id;
    let sockaddr = libc::sys::socket::sockaddr::from_ipv4_parts(ip, port);
    log!(
        "Mythroad: CMWAP proxy begin real connect proxy_index={index}, real_socket={real_socket}, fd={fd}, target={host}:{port}"
    );
    let connect_start = Instant::now();
    if libc::sys::socket::connect_sockaddr(env, fd, sockaddr) != 0 {
        log!(
            "Mythroad: CMWAP proxy real connect failed target={host}:{port}, elapsed={:?}",
            connect_start.elapsed()
        );
        env.mythroad.state.network.sockets[real_index].status = SocketStatus::Err;
        if !close_socket_index(env, real_index) {
            log!("Warning: CMWAP proxy failed to close real socket {real_socket}");
        }
        let slot = &mut env.mythroad.state.network.sockets[index];
        slot.real_socket_id = -1;
        slot.real_connected = false;
        set_socket_write_error(env, index);
        return None;
    }
    log!(
        "Mythroad: CMWAP proxy real connect succeeded target={host}:{port}, elapsed={:?}",
        connect_start.elapsed()
    );

    set_socket_connected(env, real_index);
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.real_connected = true;
    Some(real_index)
}

fn parse_host_header(bytes: &[u8]) -> Option<(String, u16)> {
    let pos = find_ascii_case_insensitive(bytes, b"Host:")?;
    let mut value = &bytes[pos + b"Host:".len()..];
    let line_end = value
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(value.len());
    value = &value[..line_end];
    value = trim_ascii(value);
    let value = std::str::from_utf8(value).ok()?;

    let (host, port) = if let Some((host, port)) = value.rsplit_once(':') {
        (host.trim(), port.trim().parse().ok()?)
    } else {
        (value.trim(), 80)
    };
    (!host.is_empty()).then(|| (host.to_owned(), port))
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|pos| pos + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn resolve_host(host: &str) -> Option<[u8; 4]> {
    (host, 80)
        .to_socket_addrs()
        .ok()?
        .find_map(|addr| match addr {
            std::net::SocketAddr::V4(addr) => Some(addr.ip().octets()),
            std::net::SocketAddr::V6(_) => None,
        })
}

fn set_socket_connected(env: &mut Environment, index: usize) {
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.status = SocketStatus::Connected;
    slot.read_status = SocketReadStatus::Readable;
    slot.write_status = SocketWriteStatus::Writeable;
}

fn set_socket_proxy_connected(env: &mut Environment, index: usize) {
    set_socket_connected(env, index);
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.is_proxy = true;
    slot.real_socket_id = -1;
    slot.real_connected = false;
}

fn set_socket_read_error(env: &mut Environment, index: usize) {
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.status = SocketStatus::Err;
    slot.read_status = SocketReadStatus::NoRead;
}

fn set_socket_write_error(env: &mut Environment, index: usize) {
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.status = SocketStatus::Err;
    slot.write_status = SocketWriteStatus::NoWrite;
}
