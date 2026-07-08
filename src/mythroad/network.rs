use crate::abi::GuestFunction;
use crate::libc;
use crate::mem::ConstPtr;
use crate::Environment;

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
}

impl Default for SocketSlot {
    fn default() -> Self {
        Self {
            socket_id: -1,
            status: SocketStatus::Close,
            read_status: SocketReadStatus::NoRead,
            write_status: SocketWriteStatus::NoWrite,
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
        set_socket_connected(env, index);
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

    if libc::sys::socket::connect_sockaddr(env, fd, sockaddr) == 0 {
        set_socket_connected(env, index);
        MrResult::Success as i32
    } else {
        env.mythroad.state.network.sockets[index].status = SocketStatus::Err;
        MrResult::Failed as i32
    }
}

fn set_socket_connected(env: &mut Environment, index: usize) {
    let slot = &mut env.mythroad.state.network.sockets[index];
    slot.status = SocketStatus::Connected;
    slot.read_status = SocketReadStatus::Readable;
    slot.write_status = SocketWriteStatus::Writeable;
}
