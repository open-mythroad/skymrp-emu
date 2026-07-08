use crate::abi::GuestFunction;
use crate::libc;
use crate::mem::ConstPtr;
use crate::Environment;

use super::MrResult;

const DSM_SUPPORT_SOCK_NUM: usize = 5;
const MR_SOCK_STREAM: i32 = 0;
const MR_SOCK_DGRAM: i32 = 1;

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
