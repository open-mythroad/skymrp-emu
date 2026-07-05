use crate::abi::{CallFromHost, DotDotDot, GuestFunction};
use crate::audio;
use crate::cpu::Cpu;
use crate::dsm;
use crate::encoding;
use crate::font;
use crate::gzip;
use crate::haptics;
use crate::libc;
use crate::md5::{self, Md5State};
use crate::mem::{
    guest_size_of, ConstPtr, ConstVoidPtr, GuestUSize, GuestVar, Memory, MutPtr, MutVoidPtr, Ptr,
    SafeRead,
};
use crate::syscall::{export_c_data, export_c_func, Export, FunctionExports};
use crate::Environment;
use chrono::{Datelike, Local, Timelike};

const BITMAPMAX: GuestUSize = 30;
const SPRITEMAX: GuestUSize = 10;
const TILEMAX: GuestUSize = 3;
const SOUNDMAX: GuestUSize = 5;
const MR_EXIT_EVENT: i32 = 8;

pub struct Mythroad {
    pub state: State,
    pub font: font::Font,
}

impl Mythroad {
    pub fn new(mem: &mut Memory) -> Mythroad {
        Self {
            state: State::new(mem),
            font: font::Font::new(mem),
        }
    }

    pub fn register_app(&self, mem: &mut Memory, index: i32, ptr: MutPtr<u8>) -> bool {
        let Ok(index) = u32::try_from(index) else {
            return false;
        };
        if index >= MR_M0_FILES {
            return false;
        }

        mem.write(self.state.mr_m0_files + index, ptr.to_bits());
        true
    }
}

#[repr(i32)]
pub enum MrResult {
    Failed = -1,
    Success = 0,
    Ignored = 1,
    Waiting = 2,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MrRunState {
    Idle = 0,
    Run = 1,
    Pause = 2,
    Restart = 3,
    Stop = 4,
    Error = 5,
}

impl Default for MrRunState {
    fn default() -> Self {
        Self::Idle
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MrTimerState {
    Idle = 0,
    Running = 1,
    Suspended = 2,
    Error = 3,
}

impl Default for MrTimerState {
    fn default() -> Self {
        Self::Idle
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrBitmap {
    pub w: u16,
    pub h: u16,
    pub buflen: u32,
    pub type_: u32,
    pub p: MutPtr<u16>,
}

impl SafeRead for MrBitmap {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrBitmapDraw {
    pub p: MutPtr<u16>,
    pub w: u16,
    pub h: u16,
    pub x: u16,
    pub y: u16,
}

impl SafeRead for MrBitmapDraw {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTransMatrix {
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
    pub rop: u16,
}

impl SafeRead for MrTransMatrix {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTransBitmap {
    pub p_data: MutVoidPtr,
    pub width: i32,
    pub height: i32,
    pub max_width: i32,
    pub trans_color: u16,
    pub p_matrix: MutPtr<i16>,
    pub mark_count: i32,
    pub p_zion: MutPtr<u8>,
}

impl SafeRead for MrTransBitmap {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJGraphicsMutableValues {
    pub clip_x: i32,
    pub clip_y: i32,
    pub clip_width: i32,
    pub clip_height: i32,
    pub clip_x_right: i32,
    pub clip_y_bottom: i32,
    pub translate_x: i32,
    pub translate_y: i32,
    pub font: i32,
    pub color_r: u8,
    pub color_g: u8,
    pub color_b: u8,
    pub color_565: u16,
}

impl SafeRead for MrJGraphicsMutableValues {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJGraphicsContext {
    pub mutable_values: MrJGraphicsMutableValues,
    pub screen_buffer: MutPtr<u16>,
    pub screen_width: i32,
    pub screen_height: i32,
    pub flag: i32,
    pub real_screen_buffer: MutPtr<u16>,
    pub real_screen_width: i32,
    pub real_screen_height: i32,
}

impl SafeRead for MrJGraphicsContext {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJImage {
    pub data: MutPtr<u16>,
    pub width: u16,
    pub height: u16,
    pub trans: u8,
    pub transcolor: u16,
}

impl SafeRead for MrJImage {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTile {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub x1: i16,
    pub y1: i16,
    pub x2: i16,
    pub y2: i16,
    pub tilew: u16,
    pub tileh: u16,
}

impl SafeRead for MrTile {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrSound {
    pub p: MutVoidPtr,
    pub buflen: u32,
    pub type_: i32,
}

impl SafeRead for MrSound {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrSprite {
    pub h: u16,
}

impl SafeRead for MrSprite {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrDatetime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl SafeRead for MrDatetime {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrUserInfo {
    pub imei: [u8; 16],
    pub imsi: [u8; 16],
    pub manufactory: [u8; 8],
    pub r#type: [u8; 8],
    pub ver: u32,
    pub spare: [u8; 12],
}

impl SafeRead for MrUserInfo {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct MrEvent {
    pub code: i32,
    pub param0: i32,
    pub param1: i32,
    pub param2: i32,
    pub param3: i32,
}

impl MrEvent {
    pub fn new(code: i32, param0: i32, param1: i32) -> Self {
        Self {
            code,
            param0,
            param1,
            param2: 0,
            param3: 0,
        }
    }
}

impl SafeRead for MrEvent {}

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitmapRasterOp {
    Or = 0,
    Xor = 1,
    Copy = 2,
    Not = 3,
    MergeNot = 4,
    AndNot = 5,
    Transparent = 6,
    And = 7,
    Gray = 8,
    Reverse = 9,
}

impl TryFrom<u16> for BitmapRasterOp {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Or),
            1 => Ok(Self::Xor),
            2 => Ok(Self::Copy),
            3 => Ok(Self::Not),
            4 => Ok(Self::MergeNot),
            5 => Ok(Self::AndNot),
            6 => Ok(Self::Transparent),
            7 => Ok(Self::And),
            8 => Ok(Self::Gray),
            9 => Ok(Self::Reverse),
            _ => Err(()),
        }
    }
}

pub struct MrHeap {
    pub mem_base: GuestVar<u8>,
    pub mem_len: GuestVar<i32>,
    pub mem_end: GuestVar<u8>,
    pub mem_left: GuestVar<i32>,
    pub mem_min: GuestVar<u32>,
    pub mem_top: GuestVar<u32>,
    pub mem_free: MutPtr<u32>,
}

impl MrHeap {
    const MR_HEAP_BASE: GuestUSize = Memory::NULL_PAGE_SIZE;
    const MR_HEAP_LEN: GuestUSize = 32 * 1024 * 1024;
    const MR_HEAP_END: GuestUSize = Self::MR_HEAP_BASE + Self::MR_HEAP_LEN;
    pub fn new(mem: &mut Memory) -> Self {
        Self {
            mem_base: GuestVar::new(mem, Self::MR_HEAP_BASE as u8),
            mem_len: GuestVar::new(mem, Self::MR_HEAP_LEN as i32),
            mem_end: GuestVar::new(mem, Self::MR_HEAP_END as u8),
            mem_left: GuestVar::new(mem, Self::MR_HEAP_LEN as i32),
            mem_min: GuestVar::new(mem, Self::MR_HEAP_LEN),
            mem_top: GuestVar::new(mem, 0u32),
            mem_free: write_u32_table(mem, &[0, 0]),
        }
    }

    pub fn malloc(&mut self, mem: &mut Memory, len: u32) -> MutVoidPtr {
        if len == 0 {
            return MutVoidPtr::null();
        }

        let ptr = mem.alloc(len);
        let left = self.mem_left.get(mem).saturating_sub(len as i32);
        self.mem_left.set(mem, left);
        self.mem_min.update(mem, |min| min.min(left as u32));
        ptr
    }

    pub fn free(&mut self, mem: &mut Memory, ptr: MutVoidPtr, len: u32) {
        if ptr.is_null() || len == 0 {
            return;
        }

        mem.free(ptr);
        let left = self
            .mem_left
            .get(mem)
            .saturating_add(len as i32)
            .min(Self::MR_HEAP_LEN as i32);
        self.mem_left.set(mem, left);
    }
}

pub struct State {
    pub mr_c_function_p: MutVoidPtr,
    pub mr_c_function_p_len: u32,
    pub mr_c_function: GuestFunction,
    pub mr_state: GuestVar<u32>,
    pub mr_timer_state: GuestVar<u32>,
    pub mr_timer_start_time: u32,
    pub mr_timer_interval: u32,
    pub mr_c_function_load: GuestFunction,
    pub mr_event_function: GuestFunction,
    pub mr_timer_function: GuestFunction,
    pub mr_stop_function: GuestFunction,
    pub mr_pause_app_function: GuestFunction,
    pub mr_resume_app_function: GuestFunction,
    pub mr_sound_on: GuestVar<i8>,
    pub mr_shake_on: GuestVar<i8>,
    pub bi: GuestVar<u32>,
    pub mr_sms_return_flag: GuestVar<u32>,
    pub mr_sms_return_val: GuestVar<u32>,
    pub sysinfo: SysInfo,
    pub pack_filename: MutPtr<u8>,
    pub start_filename: MutPtr<u8>,
    pub old_pack_filename: MutPtr<u8>,
    pub old_start_filename: MutPtr<u8>,
    pub start_file_parameter: MutPtr<u8>,
    pub entry: MutPtr<u8>,
    pub app_info: MutVoidPtr,
    pub heap: MrHeap,
    pub mr_m0_files: MutPtr<u32>,
    pub vm_state: GuestVar<u32>,
    pub mr_timer_p: GuestVar<u32>,
    pub mr_timer_run_without_pause: GuestVar<u32>,
    pub mr_c_internal_table: MutPtr<u32>,
    pub mr_c_port_table: MutPtr<u32>,
    pub mr_screen_buf: GuestVar<MutPtr<u16>>,
    pub mr_screen_w: GuestVar<i32>,
    pub mr_screen_h: GuestVar<i32>,
    pub mr_screen_bit: GuestVar<i32>,
    pub mr_bitmap: MutPtr<MrBitmap>,
    pub mr_tile: MutPtr<MrTile>,
    pub mr_map: MutPtr<MutPtr<i16>>,
    pub mr_sound: MutPtr<MrSound>,
    pub mr_sprite: MutPtr<MrSprite>,
    pub mr_ram_file: GuestVar<MutPtr<u8>>,
    pub mr_ram_file_len: GuestVar<i32>,
    pub mr_sms_cfg_buf: MutPtr<u8>,
    pub mr_exit_cb: GuestVar<u32>,
    pub mr_exit_cb_data: GuestVar<i32>,
    pub mr_updcrc: crc32fast::Hasher,
}

const MR_FILE_MAX_LEN: GuestUSize = 128;
const MR_M0_FILES: GuestUSize = 8;
const MR_SMS_CFG_BUF_LEN: GuestUSize = 120 * 36;

pub struct SysInfo {
    pub screen_width: i32,
    pub screen_height: i32,
    pub screen_bits: i32,
}

impl Default for SysInfo {
    fn default() -> SysInfo {
        Self {
            screen_width: 240,
            screen_height: 320,
            screen_bits: 16,
        }
    }
}

impl State {
    pub fn new(mem: &mut Memory) -> State {
        let heap = MrHeap::new(mem);
        let pack_filename = alloc_array(mem, MR_FILE_MAX_LEN);
        let start_filename = alloc_array(mem, MR_FILE_MAX_LEN);
        let old_pack_filename = alloc_array(mem, MR_FILE_MAX_LEN);
        let old_start_filename = alloc_array(mem, MR_FILE_MAX_LEN);
        let start_file_parameter = alloc_array(mem, MR_FILE_MAX_LEN);
        let entry = alloc_array(mem, MR_FILE_MAX_LEN);

        let mr_m0_files = alloc_array(mem, MR_M0_FILES);
        let vm_state = GuestVar::new(mem, 0u32);
        let mr_state = GuestVar::new(mem, MrRunState::Idle as u32);
        let bi = GuestVar::new(mem, 0u32);
        let mr_timer_p = GuestVar::new(mem, 0u32);
        let mr_timer_state = GuestVar::new(mem, MrTimerState::Idle as u32);
        let mr_timer_start_time = 0;
        let mr_timer_interval = 0;
        let mr_timer_run_without_pause = GuestVar::new(mem, 0u32);
        let mr_sound_on = GuestVar::new(mem, 1i8);
        let mr_shake_on = GuestVar::new(mem, 1i8);
        let mr_sms_return_flag = GuestVar::new(mem, 0u32);
        let mr_sms_return_val = GuestVar::new(mem, 0u32);
        let sysinfo = SysInfo::default();
        let screen_buf = alloc_array(
            mem,
            sysinfo.screen_width as u32 * sysinfo.screen_height as u32,
        );
        let mr_screen_buf = GuestVar::new(mem, screen_buf);
        let mr_screen_w = GuestVar::new(mem, sysinfo.screen_width as i32);
        let mr_screen_h = GuestVar::new(mem, sysinfo.screen_height as i32);
        let mr_screen_bit = GuestVar::new(mem, sysinfo.screen_bits as i32);
        let mr_bitmap = alloc_array(mem, BITMAPMAX + 1);
        let mr_tile = alloc_array(mem, TILEMAX);
        let mr_map = alloc_array(mem, TILEMAX);
        let mr_sound = alloc_array(mem, SOUNDMAX);
        let mr_sprite = alloc_array(mem, SPRITEMAX);
        let mr_ram_file = GuestVar::new(mem, MutPtr::<u8>::null());
        let mr_ram_file_len = GuestVar::new(mem, 0i32);
        let mr_sms_cfg_buf = alloc_array(mem, MR_SMS_CFG_BUF_LEN);
        let mr_exit_cb = GuestVar::new(mem, 0u32);
        let mr_exit_cb_data = GuestVar::new(mem, 0i32);
        let mr_updcrc = crc32fast::Hasher::new();

        let mr_c_function_p = MutVoidPtr::null();
        let mr_c_function_p_len = 0;
        let mr_c_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_c_function_load = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_event_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_timer_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_stop_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_pause_app_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_resume_app_function = GuestFunction::from_addr_with_thumb_bit(0);
        let app_info = MutVoidPtr::null();

        let mr_c_internal_table = write_u32_table(
            mem,
            &[
                mr_m0_files.to_bits(),
                vm_state.to_bits(),
                mr_state.to_bits(),
                bi.to_bits(),
                mr_timer_p.to_bits(),
                mr_timer_state.to_bits(),
                mr_timer_run_without_pause.to_bits(),
            ],
        );
        let mr_c_port_table = write_u32_table(mem, &[0, 0, 0, 0]);

        Self {
            mr_c_function_p,
            mr_c_function_p_len,
            mr_c_function,
            mr_state,
            mr_timer_state,
            mr_timer_start_time,
            mr_timer_interval,
            mr_c_function_load,
            mr_event_function,
            mr_timer_function,
            mr_stop_function,
            mr_pause_app_function,
            mr_resume_app_function,
            mr_sound_on,
            mr_shake_on,
            bi,
            mr_sms_return_flag,
            mr_sms_return_val,
            sysinfo,
            pack_filename,
            start_filename,
            old_pack_filename,
            old_start_filename,
            start_file_parameter,
            entry,
            app_info,
            heap,
            mr_m0_files,
            vm_state,
            mr_timer_p,
            mr_timer_run_without_pause,
            mr_c_internal_table,
            mr_c_port_table,
            mr_screen_buf,
            mr_screen_w,
            mr_screen_h,
            mr_screen_bit,
            mr_bitmap,
            mr_tile,
            mr_map,
            mr_sound,
            mr_sprite,
            mr_ram_file,
            mr_ram_file_len,
            mr_sms_cfg_buf,
            mr_exit_cb,
            mr_exit_cb_data,
            mr_updcrc,
        }
    }
}

fn alloc_array<T>(mem: &mut Memory, len: GuestUSize) -> MutPtr<T> {
    mem.calloc(len * guest_size_of::<T>()).cast()
}

fn write_u32_table(mem: &mut Memory, values: &[u32]) -> MutPtr<u32> {
    let table = alloc_array(mem, values.len().try_into().unwrap());
    for (index, value) in values.iter().copied().enumerate() {
        mem.write(table + index.try_into().unwrap(), value);
    }
    table
}

pub(crate) fn reset_resource_tables(env: &mut Environment) {
    let state = &env.mythroad.state;
    let screen_buf = state.mr_screen_buf.get(&env.mem);
    let screen_w = state.mr_screen_w.get(&env.mem);
    let screen_h = state.mr_screen_h.get(&env.mem);
    let screen_buf_len = (screen_w.max(0) as u32)
        .saturating_mul(screen_h.max(0) as u32)
        .saturating_mul(std::mem::size_of::<u16>() as u32);
    let mr_bitmap = state.mr_bitmap;
    let mr_sound = state.mr_sound;
    let mr_sprite = state.mr_sprite;
    let mr_tile = state.mr_tile;
    let mr_map = state.mr_map;

    libc::string::memset(
        env,
        mr_bitmap.cast(),
        0,
        guest_size_of::<MrBitmap>() * BITMAPMAX,
    );
    libc::string::memset(
        env,
        mr_sound.cast(),
        0,
        guest_size_of::<MrSound>() * SOUNDMAX,
    );
    libc::string::memset(
        env,
        mr_sprite.cast(),
        0,
        guest_size_of::<MrSprite>() * SPRITEMAX,
    );
    libc::string::memset(env, mr_tile.cast(), 0, guest_size_of::<MrTile>() * TILEMAX);
    libc::string::memset(
        env,
        mr_map.cast(),
        0,
        guest_size_of::<MutPtr<i16>>() * TILEMAX,
    );

    for i in 0..TILEMAX {
        env.mem.write(
            mr_tile + i,
            MrTile {
                x1: 0,
                y1: 0,
                x2: screen_w as i16,
                y2: screen_h as i16,
                ..MrTile::default()
            },
        );
    }

    env.mem.write(
        mr_bitmap + BITMAPMAX,
        MrBitmap {
            w: screen_w as u16,
            h: screen_h as u16,
            buflen: screen_buf_len,
            type_: 0,
            p: screen_buf,
        },
    );
}

pub(crate) fn mr_malloc(env: &mut Environment, len: u32) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: mr_malloc(len={len}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mythroad.state.heap.malloc(&mut env.mem, len)
}

pub(crate) fn mr_free(env: &mut Environment, p: MutVoidPtr, len: u32) {
    log_dbg!(
        "Mythroad: mr_free(p={:#x}, len={len}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    env.mythroad.state.heap.free(&mut env.mem, p, len);
}

pub(crate) fn mr_realloc(
    env: &mut Environment,
    p: MutVoidPtr,
    oldlen: u32,
    len: u32,
) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: mr_realloc(p={:#x}, oldlen={oldlen:#x}, len={len:#x}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    let minsize = if oldlen > len { len } else { oldlen };
    if p.is_null() {
        return mr_malloc(env, len);
    }
    if len == 0 {
        mr_free(env, p, oldlen);
        return MutVoidPtr::null();
    }
    let addr = mr_malloc(env, len);
    if addr.is_null() {
        return MutVoidPtr::null();
    }
    libc::string::memmove(env, addr, p.cast_const(), minsize);
    mr_free(env, p, oldlen);
    addr
}

fn mr_memcpy(env: &mut Environment, dst: MutVoidPtr, src: ConstVoidPtr, n: u32) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: mr_memcpy(dst={:#x}, src={:#x}, n={n}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::memcpy(env, dst, src, n)
}

fn mr_memmove(env: &mut Environment, dst: MutVoidPtr, src: ConstVoidPtr, n: u32) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: mr_memmove(dst={:#x}, src={:#x}, n={n}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::memmove(env, dst, src, n)
}

fn mr_strcpy(env: &mut Environment, dst: MutPtr<u8>, src: ConstPtr<u8>) -> MutPtr<u8> {
    log_dbg!(
        "Mythroad: mr_strcpy(dst={:#x}, src={:#x}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strcpy(env, dst, src)
}

fn mr_strncpy(env: &mut Environment, dst: MutPtr<u8>, src: ConstPtr<u8>, n: u32) -> MutPtr<u8> {
    log_dbg!(
        "Mythroad: mr_strncpy(dst={:#x}, src={:#x}, n={n}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strncpy(env, dst, src, n)
}

fn mr_strcat(env: &mut Environment, dst: MutPtr<u8>, src: ConstPtr<u8>) -> MutPtr<u8> {
    log_dbg!(
        "Mythroad: mr_strcat(dst={:#x}, src={:#x}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strcat(env, dst, src)
}

fn mr_strncat(env: &mut Environment, dst: MutPtr<u8>, src: ConstPtr<u8>, n: u32) -> MutPtr<u8> {
    log_dbg!(
        "Mythroad: mr_strncat(dst={:#x}, src={:#x}, n={n}) called from {:#x}",
        dst.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strncat(env, dst, src, n)
}

fn mr_memcmp(env: &mut Environment, s1: ConstVoidPtr, s2: ConstVoidPtr, n: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_memcmp(s1={:#x}, s2={:#x}, n={n}) called from {:#x}",
        s1.to_bits(),
        s2.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::memcmp(env, s1, s2, n)
}

fn mr_strcmp(env: &mut Environment, s1: ConstPtr<u8>, s2: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_strcmp(s1={:#x}, s2={:#x}) called from {:#x}",
        s1.to_bits(),
        s2.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strcmp(env, s1, s2)
}

fn mr_strncmp(env: &mut Environment, s1: ConstPtr<u8>, s2: ConstPtr<u8>, n: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_strncmp(s1={:#x}, s2={:#x}, n={n}) called from {:#x}",
        s1.to_bits(),
        s2.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strncmp(env, s1, s2, n)
}

fn mr_strcoll(env: &mut Environment, s1: ConstPtr<u8>, s2: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_strcoll(s1={:#x}, s2={:#x}) called from {:#x}",
        s1.to_bits(),
        s2.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strcoll(env, s1, s2)
}

fn mr_memchr(env: &mut Environment, s: ConstVoidPtr, ch: u32, n: u32) -> ConstVoidPtr {
    log_dbg!(
        "Mythroad: mr_memchr(s={:#x}, ch={ch:#x}, n={n}) called from {:#x}",
        s.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::memchr(env, s, ch, n)
}

fn mr_memset(env: &mut Environment, dst: MutVoidPtr, ch: u32, n: u32) -> MutVoidPtr {
    log_dbg!(
        "Mythroad: mr_memset(s={:#x}, ch={ch}, n={n}) called from {:#x}",
        dst.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    libc::string::memset(env, dst, ch as i32, n)
}

fn mr_strlen(env: &mut Environment, str: ConstPtr<u8>) -> u32 {
    log_dbg!(
        "Mythroad: mr_strlen(str={:#x}) called from {:#x}",
        str.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strlen(env, str)
}

fn mr_strstr(env: &mut Environment, haystack: ConstPtr<u8>, needle: ConstPtr<u8>) -> ConstPtr<u8> {
    log_dbg!(
        "Mythroad: mr_strstr(haystack={:#x}, needle={:#x}) called from {:#x}",
        haystack.to_bits(),
        needle.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::strstr(env, haystack, needle)
}

fn mr_sprintf(env: &mut Environment, buf: MutPtr<u8>, fmt: ConstPtr<u8>, args: DotDotDot) -> i32 {
    log_dbg!(
        "Mythroad: mr_sprintf(buf={:#x}, fmt={:#x}) called from {:#x}",
        buf.to_bits(),
        fmt.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::stdio::printf::sprintf(env, buf, fmt, args)
}

fn mr_atoi(env: &mut Environment, s: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_atoi(s={:#x}) called from {:#x}",
        s.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::stdlib::atoi(env, s)
}

fn mr_strtoul(
    env: &mut Environment,
    nptr: ConstPtr<u8>,
    endptr: MutPtr<MutPtr<u8>>,
    base: i32,
) -> u32 {
    log_dbg!(
        "Mythroad: mr_strtoul(nptr={:#x}, endptr={:#x}, base={base}) called from {:#x}",
        nptr.to_bits(),
        endptr.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::stdlib::strtoul(env, nptr, endptr, base)
}

fn mr_rand(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_rand() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    libc::stdlib::rand(env)
}

pub(crate) fn mr_stop(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_stop() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let stop_function = env.mythroad.state.mr_stop_function;
    if stop_function.addr_without_thumb_bit() != 0 {
        let status: i32 = stop_function.call_from_host(env, ());
        env.mythroad.state.mr_stop_function = GuestFunction::from_addr_with_thumb_bit(0);
        if status != MrResult::Ignored as i32 {
            return status;
        }
    }

    mr_stop_ex(env, 1)
}

fn mr_stop_ex(env: &mut Environment, freemem: i16) -> i32 {
    log_dbg!(
        "Mythroad: mr_stop_ex(freemem={freemem}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let mr_state = env.mythroad.state.mr_state.get(&env.mem);
    if mr_state == MrRunState::Idle as u32 {
        return MrResult::Ignored as i32;
    }

    if mr_state == MrRunState::Run as u32 || mr_state == MrRunState::Pause as u32 {
        let event_function = env.mythroad.state.mr_event_function;
        let mut event_status = MrResult::Ignored as i32;
        if event_function.addr_without_thumb_bit() != 0 {
            event_status = event_function.call_from_host(env, (MR_EXIT_EVENT, 0i32, 0i32));
        }

        if event_status == MrResult::Ignored as i32
            && env.mythroad.state.mr_c_function.addr_without_thumb_bit() != 0
        {
            let event_ptr: MutPtr<MrEvent> = env.mem.alloc(guest_size_of::<MrEvent>()).cast();
            env.mem.write(event_ptr, MrEvent::new(MR_EXIT_EVENT, 0, 0));
            dsm::mr_test_com_c(
                env,
                801,
                event_ptr.cast_void(),
                guest_size_of::<MrEvent>(),
                1,
            );
            env.mem.free(event_ptr.cast_void());
        }
    }

    env.mythroad
        .state
        .mr_state
        .set(&mut env.mem, MrRunState::Idle as u32);
    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle as u32);
    env.mythroad
        .state
        .mr_timer_run_without_pause
        .set(&mut env.mem, 0);
    env.mythroad.state.mr_timer_interval = 0;
    env.mythroad.state.mr_timer_start_time = dsm::mr_get_time(env);

    if freemem != 0 {
        env.mythroad
            .state
            .mr_screen_buf
            .set(&mut env.mem, MutPtr::<u16>::null());
    }

    MrResult::Success as i32
}

fn mr_c_function_new(env: &mut Environment, func: GuestFunction, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_c_function_new(func={:#010x}, len={len}) called from {:#x}",
        func.addr_with_thumb_bit(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if !env.mythroad.state.mr_c_function_p.is_null() {
        mr_free(
            env,
            env.mythroad.state.mr_c_function_p,
            env.mythroad.state.mr_c_function_p_len,
        );
    }

    let addr = mr_malloc(env, len);

    if addr.is_null() {
        env.mythroad
            .state
            .mr_state
            .set(&mut env.mem, MrRunState::Error as u32);
        return MrResult::Failed as i32;
    }

    env.mem.bytes_at_mut(addr.cast(), len).fill(0);

    env.mythroad.state.mr_c_function_p = addr;
    env.mythroad.state.mr_c_function_p_len = len;
    env.mythroad.state.mr_c_function = func;

    env.mem.write(
        Ptr::from_bits(
            env.mythroad
                .state
                .mr_c_function_load
                .addr_without_thumb_bit()
                - 4,
        ),
        addr.to_bits(),
    );

    MrResult::Success as i32
}

fn mr_printf(env: &mut Environment, format: ConstPtr<u8>, args: DotDotDot) -> i32 {
    log_dbg!(
        "Mythroad: mr_printf(format={:#x}) called from {:#x}",
        format.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::stdio::printf::printf(env, format, args)
}

fn mr_mem_get(env: &mut Environment, mem_base: u32, mem_len: u32) {
    log_dbg!(
        "Mythroad: mr_mem_get(mem_base={mem_base:#x}, mem_len={mem_len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_mem_free(env: &mut Environment, mem: u32, len: u32) {
    log_dbg!(
        "Mythroad: mr_mem_free(mem={mem:#x}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_draw_bitmap(env: &mut Environment, bmp: MutPtr<u16>, x: i16, y: i16, w: u16, h: u16) {
    log_dbg!(
        "Mythroad: mr_drawBitmap(bmp={:#x}, x={x}, y={y}, w={w}, h={h}) called from {:#x}",
        bmp.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if bmp.is_null() || w == 0 || h == 0 {
        return;
    }

    let screen_w_i32 = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h_i32 = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w_i32 <= 0 || screen_h_i32 <= 0 {
        return;
    }

    let screen_w = screen_w_i32 as u32;
    let screen_h = screen_h_i32 as u32;
    let len = screen_w * screen_h * guest_size_of::<u16>();

    let x = i32::from(x);
    let y = i32::from(y);
    let w = i32::from(w);
    let h = i32::from(h);
    if x >= screen_w_i32 || y >= screen_h_i32 || w <= 0 || h <= 0 {
        return;
    }

    let x1 = x + w - 1;
    let y1 = y + h - 1;
    if x1 < 0 || y1 < 0 {
        return;
    }

    let framebuffer = env.mem.bytes_at(bmp.cast::<u8>().cast_const(), len);
    env.window.refresh(framebuffer, screen_w, screen_h);
}

fn mr_get_char_bitmap(
    env: &mut Environment,
    ch: u16,
    font_size: u16,
    width: MutPtr<i32>,
    height: MutPtr<i32>,
) -> ConstPtr<u8> {
    log_dbg!(
        "Mythroad: mr_getCharBitmap(ch={ch}, fontSize={font_size}, width={:#x}, height={:#x}) called from {:#x}",
        width.to_bits(),
        height.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    font::get_char_bitmap(env, ch, font_size, width, height)
}

fn mr_timer_start(env: &mut Environment, interval: u16) -> i32 {
    log_dbg!(
        "Mythroad: mr_timerStart(t={interval}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mythroad.state.mr_timer_start_time = dsm::mr_get_time(env);
    env.mythroad.state.mr_timer_interval = interval.into();
    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Running as u32);

    MrResult::Success as i32
}

fn mr_timer_stop(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_timerStop() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle as u32);
    env.mythroad.state.mr_timer_interval = 0;
    env.mythroad.state.mr_timer_start_time = dsm::mr_get_time(env);

    MrResult::Success as i32
}

fn mr_get_time(env: &mut Environment) -> u32 {
    let time = dsm::mr_get_time(env);
    log_dbg!(
        "Mythroad: mr_getTime() -> {time} called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    time
}

fn mr_get_datetime(env: &mut Environment, datetime: MutPtr<MrDatetime>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getDatetime(datetime={datetime:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if datetime.is_null() {
        return MrResult::Failed as i32;
    }

    let now = Local::now();
    env.mem.write(
        datetime,
        MrDatetime {
            year: now.year() as u16,
            month: now.month() as u8,
            day: now.day() as u8,
            hour: now.hour() as u8,
            minute: now.minute() as u8,
            second: now.second() as u8,
        },
    );

    MrResult::Success as i32
}

fn mr_get_user_info(env: &mut Environment, user_info: MutPtr<MrUserInfo>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getUserInfo(user_info={user_info:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    if user_info.is_null() {
        return MrResult::Failed as i32;
    }

    libc::string::memset(env, user_info.cast_void(), 0, guest_size_of::<MrUserInfo>());

    env.mem.write(
        user_info,
        MrUserInfo {
            imei: [0; 16],
            imsi: [0; 16],
            manufactory: [0; 8],
            r#type: [0; 8],
            ver: make_plat_version(1, 8, 0, 18, 0),
            spare: [0; 12],
        },
    );

    MrResult::Success as i32
}

fn mr_sleep(env: &mut Environment, ms: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_sleep(ms={ms:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_sleep(ms);
    MrResult::Success as i32
}

fn mr_plat(env: &mut Environment, code: u32, param: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_plat(code={code:#x}, param={param:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    dsm::mr_plat(env, code, param)
}

fn mr_plat_ex(
    env: &mut Environment,
    code: u32,
    input: ConstPtr<u8>,
    input_len: u32,
    output: MutPtr<MutPtr<u8>>,
    output_len: MutPtr<i32>,
    cb: MutVoidPtr,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_platEx(code={code}, input={input:?}, input_len={input_len}, output={output:?}, output_len={output_len:?}, cb={cb:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    dsm::mr_plat_ex(env, code, input, input_len, output, output_len, cb)
}

fn mr_ferrno(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_ferrno called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    MrResult::Failed as i32
}

fn mr_open(env: &mut Environment, filename: ConstPtr<u8>, mode: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_open(filename={filename:?}, mode={mode:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_open(env, filename, mode)
}

fn mr_close(env: &mut Environment, handle: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_close(handle={handle:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_close(env, handle)
}

fn mr_info(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_info(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_info(env, filename)
}

fn mr_write(env: &mut Environment, handle: u32, buffer: ConstVoidPtr, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_write(handle={handle:#x}, buffer={buffer:?}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_write(env, handle, buffer, len)
}

fn mr_read(env: &mut Environment, handle: u32, buffer: MutVoidPtr, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_read(handle={handle:#x}, buffer={buffer:?}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_read(env, handle, buffer, len)
}

fn mr_seek(env: &mut Environment, handle: u32, pos: i32, method: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_seek(handle={handle:#x}, pos={pos:#x}, method={method}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_seek(env, handle, pos, method)
}

fn mr_get_len(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getLen(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_get_len(env, filename)
}

fn mr_remove(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_remove(filename={filename:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_remove(env, filename)
}

fn mr_rename(env: &mut Environment, oldname: ConstPtr<u8>, newname: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_rename(oldname={oldname:?}, newname={newname:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_rename(env, oldname, newname)
}

fn mr_mkdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_mkDir(name={name:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_mkdir(env, name)
}

fn mr_rmdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_rmDir(name={name:?}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_rmdir(env, name)
}

fn mr_find_start(env: &mut Environment, name: ConstPtr<u8>, buffer: MutPtr<u8>, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_findStart(name={name:?}, buffer={buffer:?}, len={len}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_find_start(env, name, buffer, len)
}

fn mr_find_get_next(
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

fn mr_find_stop(env: &mut Environment, search_handle: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_findStop(search_handle={search_handle}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_find_stop(env, search_handle)
}

fn mr_exit(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_exit() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mythroad
        .state
        .mr_state
        .set(&mut env.mem, MrRunState::Stop as u32);
    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle as u32);
    env.mythroad.state.mr_timer_interval = 0;

    MrResult::Success as i32
}

fn mr_start_shake(env: &mut Environment, ms: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_startShake(ms={ms}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    match haptics::start(ms) {
        Ok(()) => MrResult::Success as i32,
        Err(err) => {
            log!("Mythroad: mr_startShake failed: {err}");
            MrResult::Failed as i32
        }
    }
}

fn mr_stop_shake(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_stopShake() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    match haptics::stop() {
        Ok(()) => MrResult::Success as i32,
        Err(err) => {
            log!("Mythroad: mr_stopShake failed: {err}");
            MrResult::Failed as i32
        }
    }
}

fn mr_play_sound(
    env: &mut Environment,
    type_: i32,
    data: ConstPtr<u8>,
    data_len: u32,
    loop_: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_playSound(type={type_}, data={:#x}, dataLen={data_len:#x}, loop={loop_}) called from {:#x}",
        data.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Ok(sound_type) = audio::SoundType::try_from(type_) else {
        log!("Mythroad: mr_playSound failed: unsupported sound type: {type_}");
        return MrResult::Failed as i32;
    };

    let sound_data = env.mem.bytes_at(data, data_len).to_vec();
    match audio::play_sound(sound_type, &sound_data, loop_ != 0) {
        Ok(()) => MrResult::Success as i32,
        Err(err) => {
            log!("Mythroad: mr_playSound failed: {err}");
            MrResult::Failed as i32
        }
    }
}

fn mr_stop_sound(env: &mut Environment, type_: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_stopSound(type={type_}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let Ok(sound_type) = audio::SoundType::try_from(type_) else {
        log!("Mythroad: mr_stopSound failed: unsupported sound type: {type_}");
        return MrResult::Failed as i32;
    };

    match audio::stop_sound(sound_type) {
        Ok(()) => MrResult::Success as i32,
        Err(err) => {
            log!("Mythroad: mr_stopSound failed: {err}");
            MrResult::Failed as i32
        }
    }
}

fn mr_send_sms(
    env: &mut Environment,
    number: ConstPtr<u8>,
    content: ConstPtr<u8>,
    flags: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_sendSms(number={:#x}, content={:#x}, flags={flags:#x}) called from {:#x}",
        number.to_bits(),
        content.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Success as i32
}

fn mr_call(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_call(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_network_id(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getNetworkID(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_connect_wap(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_connectWAP(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_create(env: &mut Environment, title: ConstPtr<u8>, num: i16) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuCreate(title={:#x}, num={num}) called from {:#x}",
        title.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Failed as i32
}

fn mr_menu_set_item(env: &mut Environment, menu: i32, text: ConstPtr<u8>, index: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuSetItem(menu={menu}, text={:#x}, index={index}) called from {:#x}",
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Failed as i32
}

fn mr_menu_show(env: &mut Environment, menu: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuShow(menu={menu}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_menu_set_focus(env: &mut Environment, menu: i32, index: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuSetFocus(menu={menu}, index={index}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_menu_release(env: &mut Environment, menu: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuRelease(menu={menu}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_menu_refresh(env: &mut Environment, menu: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_menuRefresh(menu={menu}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_dialog_create(
    env: &mut Environment,
    title: ConstPtr<u8>,
    text: ConstPtr<u8>,
    type_: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_dialogCreate(title={:#x}, text={:#x}, type={type_}) called from {:#x}",
        title.to_bits(),
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_dialog_release(env: &mut Environment, dialog: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_dialogRelease(dialog={dialog}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_dialog_refresh(
    env: &mut Environment,
    dialog: i32,
    title: ConstPtr<u8>,
    text: ConstPtr<u8>,
    type_: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_dialogRefresh(dialog={dialog}, title={:#x}, text={:#x}, type={type_}) called from {:#x}",
        title.to_bits(),
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_text_create(
    env: &mut Environment,
    title: ConstPtr<u8>,
    text: ConstPtr<u8>,
    type_: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_textCreate(title={:#x}, text={:#x}, type={type_}) called from {:#x}",
        title.to_bits(),
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_text_release(env: &mut Environment, text: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_textRelease(text={text}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_text_refresh(
    env: &mut Environment,
    handle: i32,
    title: ConstPtr<u8>,
    text: ConstPtr<u8>,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_textRefresh(handle={handle}, title={:#x}, text={:#x}) called from {:#x}",
        title.to_bits(),
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_edit_create(
    env: &mut Environment,
    title: ConstPtr<u8>,
    text: ConstPtr<u8>,
    type_: i32,
    max_size: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_editCreate(title={:#x}, text={:#x}, type={type_}, max_size={max_size}) called from {:#x}",
        title.to_bits(),
        text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_edit_release(env: &mut Environment, edit: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_editRelease(edit={edit}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_edit_get_text(env: &mut Environment, edit: i32) -> ConstPtr<u8> {
    log_dbg!(
        "Mythroad: mr_editGetText(edit={edit}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    Ptr::from_bits(MrResult::Ignored as u32)
}

fn mr_win_create(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_winCreate() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_win_release(env: &mut Environment, win: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_winRelease(win={win}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Ignored as i32
}

fn mr_get_screen_info(env: &mut Environment, screen_info: MutPtr<u32>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getScreenInfo(s={:#x}) called from {:#x}",
        screen_info.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mem
        .write(screen_info, env.mythroad.state.sysinfo.screen_width as u32);
    env.mem.write(
        screen_info + 1,
        env.mythroad.state.sysinfo.screen_height as u32,
    );
    env.mem.write(
        screen_info + 2,
        env.mythroad.state.sysinfo.screen_bits as u32,
    );

    MrResult::Success as i32
}

fn mr_init_network(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_initNetwork(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_close_network(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_closeNetwork(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_host_by_name(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getHostByName(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_socket(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_socket(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_connect(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_connect(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_close_socket(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_closeSocket(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_recv(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_recv(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_recvfrom(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_recvfrom(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_send(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_send(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_sendto(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_sendto(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_md5_init(env: &mut Environment, pms: MutPtr<Md5State>) {
    log_dbg!(
        "Mythroad: mr_md5_init(pms={:#x}) called from {:#x}",
        pms.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    md5::init(env, pms);
}

fn mr_md5_append(env: &mut Environment, pms: MutPtr<Md5State>, data: ConstPtr<u8>, nbytes: i32) {
    log_dbg!(
        "Mythroad: mr_md5_append(pms={:#x}, data={:#x}, nbytes={nbytes}) called from {:#x}",
        pms.to_bits(),
        data.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    md5::append(env, pms, data, nbytes);
}

fn mr_md5_finish(env: &mut Environment, pms: MutPtr<Md5State>, digest: MutPtr<u8>) {
    log_dbg!(
        "Mythroad: mr_md5_finish(pms={:#x}, digest={:#x}) called from {:#x}",
        pms.to_bits(),
        digest.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    md5::finish(env, pms, digest);
}

fn mr_load_sms_cfg(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_load_sms_cfg(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_save_sms_cfg(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_save_sms_cfg(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn disp_up_ex(env: &mut Environment, x: i16, y: i16, w: u16, h: u16) -> i32 {
    log_dbg!(
        "Mythroad: _DispUpEx(x={x}, y={y}, w={w}, h={h}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if env.mythroad.state.vm_state.get(&env.mem) == 1 {
        let screen_buf = env.mythroad.state.mr_screen_buf.get(&env.mem);
        mr_draw_bitmap(env, screen_buf, x, y, w, h);
    }

    MrResult::Success as i32
}

fn draw_point(env: &mut Environment, x: i16, y: i16, native_color: u16) {
    log_dbg!(
        "Mythroad: _DrawPoint(x={x}, y={y}, native_color={native_color}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let state = &env.mythroad.state;

    let screen_buf = state.mr_screen_buf.get(&env.mem);
    let screen_w = state.mr_screen_w.get(&env.mem) as i16;
    let screen_h = state.mr_screen_h.get(&env.mem) as i16;

    if x < 0 || y < 0 || x >= screen_w || y >= screen_h {
        return;
    }

    let offset = screen_w.saturating_mul(y).saturating_add(x) as u32;

    env.mem.write(screen_buf + offset, native_color);
}

#[inline]
fn make_rgb565(r: u32, g: u32, b: u32) -> u16 {
    (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
}

#[inline]
fn make_plat_version(plat: u32, ver: u32, card: u32, r#impl: u32, brun: u32) -> u32 {
    100_000_000 + (plat * 1_000_000) + (ver * 10_000) + (card * 1_000) + (r#impl * 10) + brun
}

fn draw_bitmap(
    env: &mut Environment,
    p: MutPtr<u16>,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    rop: u16,
    transcolor: u16,
    sx: i16,
    sy: i16,
    mw: i16,
) {
    log_dbg!(
        "Mythroad: _DrawBitmap(p={:#x}, x={x}, y={y}, w={w}, h={h}, rop={rop}, transcolor={transcolor:#x}, sx={sx}, sy={sy}, mw={mw}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);

    let mut x = i32::from(x);
    let mut y = i32::from(y);
    let mut w = i32::from(w);
    let mut h = i32::from(h);
    let mut sx = i32::from(sx);
    let mut sy = i32::from(sy);
    let mw = i32::from(mw);

    if p.is_null()
        || screen_buf.is_null()
        || screen_w <= 0
        || screen_h <= 0
        || w == 0
        || h == 0
        || mw <= 0
        || sx > mw - 1
        || x + w <= 0
        || x > screen_w - 1
        || y > screen_h - 1
        || y + h <= 0
    {
        return;
    }

    if sx < 0 {
        sx = 0;
    }
    if sy < 0 {
        sy = 0;
    }

    if sx + w > mw - 1 {
        w = mw - sx;
    }
    if w == 0 {
        return;
    }

    if x < 0 {
        w += x;
        sx -= x;
        x = 0;
    }
    if y < 0 {
        h += y;
        sy -= y;
        y = 0;
    }
    if x + w > screen_w - 1 {
        w = screen_w - x;
    }
    if y + h > screen_h - 1 {
        h = screen_h - y;
    }
    if w <= 0 || h <= 0 {
        return;
    }

    let mut dest = screen_buf + (y as u32 * screen_w as u32 + x as u32);
    let mut src = p + (sy as u32 * mw as u32 + sx as u32);
    let rop = BitmapRasterOp::try_from(rop).unwrap_or(BitmapRasterOp::Copy);

    for _ in 0..h as u32 {
        for _ in 0..w as u32 {
            let src_pixel: u16 = env.mem.read(src);
            match rop {
                BitmapRasterOp::Transparent => {
                    if transcolor != src_pixel {
                        env.mem.write(dest, src_pixel);
                    }
                }
                BitmapRasterOp::Or => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel | src_pixel);
                }
                BitmapRasterOp::Xor => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel ^ src_pixel);
                }
                BitmapRasterOp::Not => {
                    env.mem.write(dest, !src_pixel);
                }
                BitmapRasterOp::MergeNot => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel | !src_pixel);
                }
                BitmapRasterOp::AndNot => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel & !src_pixel);
                }
                BitmapRasterOp::And => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel & src_pixel);
                }
                BitmapRasterOp::Gray => {
                    if transcolor != src_pixel {
                        let r = u32::from((src_pixel >> 11) & 0x1f);
                        let g = u32::from((src_pixel >> 5) & 0x3f);
                        let b = u32::from(src_pixel & 0x1f);
                        let gray = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as u32;
                        env.mem.write(dest, make_rgb565(gray, gray, gray));
                    }
                }
                BitmapRasterOp::Reverse => {
                    if transcolor != src_pixel {
                        env.mem.write(dest, !src_pixel);
                    }
                }
                BitmapRasterOp::Copy => {
                    env.mem.write(dest, src_pixel);
                }
            }
            dest += 1;
            src += 1;
        }
        dest += (screen_w - w) as u32;
        src += (mw - w) as u32;
    }
}

fn draw_bitmap_ex(
    env: &mut Environment,
    srcbmp: ConstPtr<MrBitmapDraw>,
    dstbmp: ConstPtr<MrBitmapDraw>,
    w: u16,
    h: u16,
    p_trans: ConstPtr<MrTransMatrix>,
    transcolor: u16,
) {
    log_dbg!(
        "Mythroad: _DrawBitmapEx(srcbmp={:#x}, dstbmp={:#x}, w={w}, h={h}, pTrans={:#x}, transcolor={transcolor:#x}) called from {:#x}",
        srcbmp.to_bits(),
        dstbmp.to_bits(),
        p_trans.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if srcbmp.is_null() || dstbmp.is_null() || p_trans.is_null() || w == 0 || h == 0 {
        return;
    }

    let srcbmp: MrBitmapDraw = env.mem.read(srcbmp);
    let dstbmp: MrBitmapDraw = env.mem.read(dstbmp);
    let trans: MrTransMatrix = env.mem.read(p_trans);
    if srcbmp.p.is_null() || dstbmp.p.is_null() || srcbmp.w == 0 || dstbmp.w == 0 {
        return;
    }

    let a = i32::from(trans.a);
    let b = i32::from(trans.b);
    let c = i32::from(trans.c);
    let d = i32::from(trans.d);
    let determinant = a * d - b * c;
    if determinant == 0 {
        return;
    }

    let w = i32::from(w);
    let h = i32::from(h);
    let center_x = i32::from(dstbmp.x) + w / 2;
    let center_y = i32::from(dstbmp.y) + h / 2;

    let mut max_y = (c.abs() * w + d.abs() * h) >> 9;
    let mut min_y = -max_y;
    max_y = max_y.min(i32::from(dstbmp.h) - center_y);
    min_y = min_y.max(-center_y);

    for dy in min_y..max_y {
        let half_w_det = (w * determinant) >> 9;
        let half_h_det = (h * determinant) >> 9;
        let max_x_by_d = if d == 0 {
            999
        } else {
            ((half_w_det + b * dy) / d).max((b * dy - half_w_det) / d)
        };
        let max_x_by_c = if c == 0 {
            999
        } else {
            ((a * dy + half_h_det) / c).max((a * dy - half_h_det) / c)
        };
        let min_x_by_d = if d == 0 {
            -999
        } else {
            ((b * dy - half_w_det) / d).min((half_w_det + b * dy) / d)
        };
        let min_x_by_c = if c == 0 {
            -999
        } else {
            ((a * dy - half_h_det) / c).min((a * dy + half_h_det) / c)
        };

        let max_x = max_x_by_d
            .min(max_x_by_c)
            .min(i32::from(dstbmp.w) - center_x);
        let min_x = min_x_by_d.min(min_x_by_c).max(-center_x);
        if min_x >= max_x {
            continue;
        }

        let mut dstp = dstbmp.p + ((dy + center_y) * i32::from(dstbmp.w) + min_x + center_x) as u32;
        match BitmapRasterOp::try_from(trans.rop).ok() {
            Some(BitmapRasterOp::Transparent) => {
                for dx in min_x..max_x {
                    let offset_y = ((((a * dy - c * dx) as i64) << 8) / i64::from(determinant)
                        + i64::from(h / 2)) as i32;
                    let offset_x = ((((d * dx - b * dy) as i64) << 8) / i64::from(determinant)
                        + i64::from(w / 2)) as i32;
                    if offset_y >= 0 && offset_y < h && offset_x >= 0 && offset_x < w {
                        let srcp = srcbmp.p
                            + ((offset_y + i32::from(srcbmp.y)) * i32::from(srcbmp.w)
                                + offset_x
                                + i32::from(srcbmp.x)) as u32;
                        let pixel: u16 = env.mem.read(srcp);
                        if pixel != transcolor {
                            env.mem.write(dstp, pixel);
                        }
                    }
                    dstp += 1;
                }
            }
            Some(BitmapRasterOp::Copy) => {
                for dx in min_x..max_x {
                    let offset_y = ((((a * dy - c * dx) as i64) << 8) / i64::from(determinant)
                        + i64::from(h / 2)) as i32;
                    let offset_x = ((((d * dx - b * dy) as i64) << 8) / i64::from(determinant)
                        + i64::from(w / 2)) as i32;
                    if offset_y >= 0 && offset_y < h && offset_x >= 0 && offset_x < w {
                        let srcp = srcbmp.p
                            + ((offset_y + i32::from(srcbmp.y)) * i32::from(srcbmp.w)
                                + offset_x
                                + i32::from(srcbmp.x)) as u32;
                        let pixel: u16 = env.mem.read(srcp);
                        env.mem.write(dstp, pixel);
                    }
                    dstp += 1;
                }
            }
            Some(_) | None => {}
        }
    }
}

fn draw_rect(env: &mut Environment, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    log_dbg!(
        "Mythroad: DrawRect(x={x}, y={y}, w={w}, h={h}, r={r}, g={g}, b={b}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    if screen_buf.is_null() {
        log_dbg!("Mythroad: DrawRect ignored because mr_screen_buf is null");
        return;
    }

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w <= 0 || screen_h <= 0 || w <= 0 || h <= 0 {
        return;
    }

    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;

    let x_min = x.max(0);
    let y_min = y.max(0);
    let x_max = (x + w).min(screen_w);
    let y_max = (y + h).min(screen_h);

    if x_max <= x_min || y_max <= y_min {
        return;
    }

    let color = make_rgb565(r as u32, g as u32, b as u32);
    let x_min = x_min as u32;
    let y_min = y_min as u32;
    let x_max = x_max as u32;
    let y_max = y_max as u32;
    let screen_w = screen_w as u32;
    let rect_w = x_max - x_min;

    let first_row = screen_buf + (y_min * screen_w + x_min);
    for x_d in 0..rect_w {
        env.mem.write(first_row + x_d, color);
    }

    if first_row.to_bits() & 0x3 != 0 {
        for y_d in (y_min + 1)..y_max {
            let row = screen_buf + (y_d * screen_w + x_min);
            env.mem.write(row, color);
            if rect_w > 1 {
                libc::string::memcpy(
                    env,
                    (row + 1).cast_void(),
                    (first_row + 1).cast_const().cast_void(),
                    (rect_w - 1) * guest_size_of::<u16>(),
                );
            }
        }
    } else {
        for y_d in (y_min + 1)..y_max {
            let row = screen_buf + (y_d * screen_w + x_min);
            libc::string::memcpy(
                env,
                row.cast_void(),
                first_row.cast_const().cast_void(),
                rect_w * guest_size_of::<u16>(),
            );
        }
    }
}

fn draw_text(
    env: &mut Environment,
    pc_text: ConstPtr<u8>,
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
    is_unicode: u32,
    font: u16,
) -> i32 {
    let color = make_rgb565(r as u32, g as u32, b as u32);

    log_dbg!(
        "Mythroad: _DrawText(pcText={:#x}, x={x}, y={y}, r={r}, g={g}, b={b}, is_unicode={is_unicode}, font={font}) called from {:#x}",
        pc_text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    if screen_w <= 0 {
        return MrResult::Success as i32;
    }

    let mut converted_len = 0u32;
    let pc_text = if is_unicode == 0 {
        let converted = encoding::c2u(env, pc_text, false);
        converted_len = converted.size;
        converted.ptr.cast::<u8>().cast_const()
    } else {
        pc_text
    };

    let mut sx = x;
    let mut p = pc_text;
    loop {
        let high: u8 = env.mem.read(p);
        let low: u8 = env.mem.read(p + 1);
        if high == 0 && low == 0 {
            break;
        }

        let ch = (u16::from(high) << 8) | u16::from(low);
        let (fw, _fh) = font::measure_char(env, ch, font);
        mr_plat_draw_char(env, ch, sx, y, u32::from(color));
        sx += fw;
        p += 2;

        if sx > screen_w {
            break;
        }
    }

    if is_unicode == 0 {
        mr_free(env, pc_text.cast_mut().cast_void(), converted_len);
    }
    MrResult::Success as i32
}

fn bitmap_check(
    env: &mut Environment,
    p: ConstPtr<u16>,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    transcolor: u16,
    color_check: u16,
) -> i32 {
    log_dbg!(
        "Mythroad: _BitmapCheck(p={:#x}, x={x}, y={y}, w={w}, h={h}, transcolor={transcolor:#x}, color_check={color_check:#x}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if p.is_null() || screen_buf.is_null() || screen_w <= 0 || screen_h <= 0 || w == 0 || h == 0 {
        return 0;
    }

    let x = i32::from(x);
    let y = i32::from(y);
    let w = i32::from(w);
    let h = i32::from(h);
    let max_y = screen_h.min(y + h);
    let max_x = screen_w.min(x + w);
    let min_y = 0.max(y);
    let min_x = 0.max(x);

    if min_y >= max_y || min_x >= max_x {
        return 0;
    }

    let mut result = 0i32;
    for dy in min_y..max_y {
        let mut dstp = screen_buf + (dy * screen_w + min_x) as u32;
        let mut srcp = p + ((dy - y) * w + (min_x - x)) as u32;
        for _ in min_x..max_x {
            let src_pixel: u16 = env.mem.read(srcp);
            if src_pixel != transcolor {
                let dst_pixel: u16 = env.mem.read(dstp);
                if dst_pixel != color_check {
                    result += 1;
                }
            }
            dstp += 1;
            srcp += 1;
        }
    }

    result
}

fn mr_read_file(
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

fn mr_wstrlen(env: &mut Environment, str: ConstPtr<u8>) -> i32 {
    log_dbg!(
        "Mythroad: mr_wstrlen(str={:#x}) called from {:#x}",
        str.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mem.wstr_len_bytes_at(str) as i32
}

fn mr_register_app(env: &mut Environment, p: MutPtr<u8>, len: i32, index: i32) -> i32 {
    log_dbg!(
        "Mythroad: mr_registerAPP(p={:#x}, len={len}, index={index}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if !env.mythroad.register_app(&mut env.mem, index, p) {
        log_dbg!("Mythroad: mr_registerAPP failed");
        return MrResult::Failed as i32;
    }

    MrResult::Success as i32
}

fn draw_text_ex(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DrawTextEx(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_eff_set_con(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_EffSetCon(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_test_com(env: &mut Environment, l: u32, input0: u32, input1: u32) -> i32 {
    log_dbg!(
        "Mythroad: _mr_TestCom(L={l}, input0={input0}, input1={input1}) called from {:#x}",
        env.cpu.regs()[Cpu::PC]
    );
    dsm::test_com(env, l, input0, input1)
}

fn mr_test_com1(env: &mut Environment, l: u32, input0: u32, input1: MutPtr<u8>, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: _mr_TestCom1(L={l:#x}, input0={input0:#x}, input1={:#x}, len={len}) called from {:#x}",
        input1.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::test_com1(env, l, input0, input1, len)
}

fn mr_c2u(
    env: &mut Environment,
    cp: ConstPtr<u8>,
    err: MutPtr<i32>,
    size: MutPtr<i32>,
) -> MutPtr<u16> {
    log_dbg!(
        "Mythroad: mr_c2u(cp={:#x}, err={:#x}, size={:#x}) called from {:#x}",
        cp.to_bits(),
        err.to_bits(),
        size.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    encoding::mr_c2u(env, cp, err, size)
}

fn mr_div(env: &mut Environment, a: i32, b: i32) -> i32 {
    log_dbg!(
        "Mythroad: _mr_div(a={a}, b={b}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    a / b
}

fn mr_mod(env: &mut Environment, a: i32, b: i32) -> i32 {
    log_dbg!(
        "Mythroad: _mr_mod(a={a}, b={b}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    a % b
}

fn mr_updcrc(env: &mut Environment, s: ConstPtr<u8>, n: u32) -> u32 {
    log_dbg!(
        "Mythroad: mr_updcrc(s={:#x}, n={n}) called from {:#x}",
        s.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if s.is_null() {
        env.mythroad.state.mr_updcrc = crc32fast::Hasher::new();
    } else if n != 0 {
        let bytes = env.mem.bytes_at(s, n);
        env.mythroad.state.mr_updcrc.update(bytes);
    }

    env.mythroad.state.mr_updcrc.clone().finalize()
}

fn mr_unzip(
    env: &mut Environment,
    input_buf: ConstPtr<u8>,
    input_len: i32,
    output_buf: MutPtr<MutPtr<u8>>,
    outputlen: MutPtr<i32>,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_unzip(inputbuf={:#x}, inputlen={input_len}, outputbuf={:#x}, outputlen={:#x}) called from {:#x}",
        input_buf.to_bits(),
        output_buf.to_bits(),
        outputlen.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if input_buf.is_null() || input_len <= 0 || output_buf.is_null() || outputlen.is_null() {
        return MrResult::Failed as i32;
    }

    let output = match gzip::ungzip(env.mem.bytes_at(input_buf, input_len as u32)) {
        Ok(output) => output,
        Err(err) => {
            log_dbg!("Mythroad: mr_unzip failed: {err}");
            return MrResult::Failed as i32;
        }
    };
    let output_len: u32 = output
        .len()
        .try_into()
        .expect("gzip output size is limited");

    let output_ptr: MutPtr<u8> = mr_malloc(env, output_len).cast();
    env.mem.write(output_buf, output_ptr);
    if output_ptr.is_null() {
        env.mem.write(outputlen, 0i32);
        return MrResult::Failed as i32;
    }

    env.mem
        .bytes_at_mut(output_ptr, output_len)
        .copy_from_slice(&output);
    env.mem.write(outputlen, output.len() as i32);
    MrResult::Success as i32
}

fn mr_plat_draw_char(env: &mut Environment, ch: u16, x: i32, y: i32, color: u32) {
    log_dbg!(
        "Mythroad: mr_platDrawChar(ch={ch}, x={x}, y={y}, color={color:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    font::draw_char(env, x, y, ch, color as u16);
}

fn mr_transbitmap_draw(
    env: &mut Environment,
    h_trans_bmp: ConstPtr<MrTransBitmap>,
    dst_buf: MutPtr<u16>,
    dest_max_w: i32,
    dest_max_h: i32,
    mut sx: i32,
    mut sy: i32,
    mut width: i32,
    mut height: i32,
    mut dx: i32,
    mut dy: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_transbitmapDraw(hTransBmp={:#x}, dstBuf={:#x}, dest_max_w={dest_max_w}, dest_max_h={dest_max_h}, sx={sx}, sy={sy}, width={width}, height={height}, dx={dx}, dy={dy}) called from {:#x}",
        h_trans_bmp.to_bits(),
        dst_buf.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if h_trans_bmp.is_null() {
        return MrResult::Failed as i32;
    }

    let trans_bmp: MrTransBitmap = env.mem.read(h_trans_bmp);
    if dst_buf.is_null()
        || trans_bmp.p_data.is_null()
        || trans_bmp.p_matrix.is_null()
        || dest_max_w <= 0
        || dest_max_h <= 0
        || trans_bmp.max_width <= 0
        || trans_bmp.mark_count <= 0
    {
        return MrResult::Failed as i32;
    }

    if dx < 0 {
        sx += -dx;
        width += dx;
        dx = 0;
    }

    if dy < 0 {
        sy += -dy;
        height += dy;
        dy = 0;
    }

    if sx < 0 {
        width += sx;
        dx += -sx;
        sx = 0;
    }

    if sy < 0 {
        height += sy;
        dy += -sy;
        sy = 0;
    }

    width = width.min(dest_max_w - dx);
    if height > dest_max_h - dy {
        height = dest_max_h - dy;
    }

    if width <= 0 || height <= 0 || dx >= dest_max_w || dy >= dest_max_h {
        return MrResult::Success as i32;
    }

    let mut matrix = trans_bmp.p_matrix + (sy * 2 * trans_bmp.mark_count) as u32;
    let mut src_line = trans_bmp.p_data.cast::<u16>() + (trans_bmp.max_width * sy) as u32;
    let mut dest_line = dst_buf + (dest_max_w * dy + dx) as u32;
    let pos1_e = sx + width;

    for _ in sy..(sy + height) {
        let mut has_next = false;
        let mut next_pixel = 0;

        for j in 0..trans_bmp.mark_count {
            let mark = matrix + (j * 2) as u32;
            let mut start_pixel: i32 = i32::from(env.mem.read(mark));
            if start_pixel < 0 {
                break;
            }

            let mut len: i32 = i32::from(env.mem.read(mark + 1));
            if j == trans_bmp.mark_count - 1 {
                has_next = (len & 0x4000) != 0;
                len &= 0x3fff;
                if has_next {
                    next_pixel = start_pixel + len;
                }
            }

            let mut pos2_e = start_pixel + len;
            if pos2_e < start_pixel || pos1_e < sx {
                continue;
            }

            start_pixel = start_pixel.max(sx);
            pos2_e = pos2_e.min(pos1_e);
            let actual_len = pos2_e - start_pixel;
            if actual_len > 0 {
                let src_pixel = src_line + start_pixel as u32;
                let dest_pixel = dest_line + (start_pixel - sx) as u32;
                libc::string::memcpy(
                    env,
                    dest_pixel.cast_void(),
                    src_pixel.cast_const().cast_void(),
                    actual_len as u32 * guest_size_of::<u16>(),
                );
            }
        }

        if has_next {
            let start_pixel = next_pixel.max(sx);
            let mut src_pixel = src_line + start_pixel as u32;
            let mut dest_pixel = dest_line + (start_pixel - sx) as u32;

            for _ in start_pixel..(sx + width) {
                let src: u16 = env.mem.read(src_pixel);
                if src != trans_bmp.trans_color {
                    env.mem.write(dest_pixel, src);
                }
                dest_pixel += 1;
                src_pixel += 1;
            }
        }

        src_line += trans_bmp.max_width as u32;
        dest_line += dest_max_w as u32;
        matrix += (trans_bmp.mark_count * 2) as u32;
    }

    MrResult::Success as i32
}

const TOP_GRAPHICS: i32 = 16;
const BASELINE_GRAPHICS: i32 = 64;
const BOTTOM_GRAPHICS: i32 = 32;
const VCENTER_GRAPHICS: i32 = 0x2;
const LEFT_GRAPHICS: i32 = 0x4;
const HCENTER_GRAPHICS: i32 = 0x1;
const RIGHT_GRAPHICS: i32 = 0x8;
const AP_V_MASK_GRAPHICS: i32 =
    TOP_GRAPHICS | BASELINE_GRAPHICS | BOTTOM_GRAPHICS | VCENTER_GRAPHICS;
const AP_H_MASK_GRAPHICS: i32 = LEFT_GRAPHICS | HCENTER_GRAPHICS | RIGHT_GRAPHICS;

const TRANS_NONE_SPRITE: i32 = 0;
const TRANS_MIRROR_ROT180_SPRITE: i32 = 1;
const TRANS_MIRROR_SPRITE: i32 = 2;
const TRANS_ROT180_SPRITE: i32 = 3;
const TRANS_MIRROR_ROT270_SPRITE: i32 = 4;
const TRANS_ROT90_SPRITE: i32 = 5;
const TRANS_ROT270_SPRITE: i32 = 6;
const TRANS_MIRROR_ROT90_SPRITE: i32 = 7;

fn ptr_offset<T, const MUT: bool>(ptr: Ptr<T, MUT>, offset: i32) -> Ptr<T, MUT> {
    let byte_offset = i64::from(offset) * i64::from(guest_size_of::<T>());
    Ptr::from_bits((i64::from(ptr.to_bits()) + byte_offset) as u32)
}

fn calc_anchor(
    g_context: MrJGraphicsContext,
    mut x: i32,
    mut y: i32,
    width: i32,
    height: i32,
    anchor: i32,
) -> Option<(i32, i32)> {
    x += g_context.mutable_values.translate_x;
    y += g_context.mutable_values.translate_y;

    let (anchor_h, anchor_v) = if anchor == 0 {
        (LEFT_GRAPHICS, TOP_GRAPHICS)
    } else {
        (AP_H_MASK_GRAPHICS & anchor, AP_V_MASK_GRAPHICS & anchor)
    };

    if anchor_h == LEFT_GRAPHICS && anchor_v == TOP_GRAPHICS {
        return Some((x, y));
    }

    let out_x = match anchor_h {
        RIGHT_GRAPHICS => x - width,
        LEFT_GRAPHICS => x,
        HCENTER_GRAPHICS => x - width / 2,
        _ => return None,
    };

    let out_y = match anchor_v {
        BOTTOM_GRAPHICS => y - height,
        TOP_GRAPHICS => y,
        BASELINE_GRAPHICS => y - height / 2 - 3,
        VCENTER_GRAPHICS => y - height / 2,
        _ => return None,
    };

    Some((out_x, out_y))
}

fn mr_draw_region(
    env: &mut Environment,
    g_context: ConstPtr<MrJGraphicsContext>,
    src: ConstPtr<MrJImage>,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    transform: i32,
    mut x: i32,
    mut y: i32,
    anchor: i32,
) {
    log_dbg!(
        "Mythroad: mr_drawRegion(gContext={:#x}, src={:#x}, sx={sx}, sy={sy}, w={w}, h={h}, transform={transform}, x={x}, y={y}, anchor={anchor}) called from {:#x}",
        g_context.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if g_context.is_null() || src.is_null() {
        return;
    }

    let g_context: MrJGraphicsContext = env.mem.read(g_context);
    let src: MrJImage = env.mem.read(src);
    if src.data.is_null() || g_context.screen_buffer.is_null() {
        return;
    }

    let transcolor = if src.trans != 0 {
        if src.trans == 1 {
            env.mem.read(src.data)
        } else {
            src.transcolor
        }
    } else {
        0
    };

    if anchor == 20 || anchor == 0 {
        x += g_context.mutable_values.translate_x;
        y += g_context.mutable_values.translate_y;
    } else {
        let dims = if (transform >> 2) == 0 {
            (w, h)
        } else {
            (h, w)
        };
        let Some((anchor_x, anchor_y)) = calc_anchor(g_context, x, y, dims.0, dims.1, anchor)
        else {
            return;
        };
        x = anchor_x;
        y = anchor_y;
    }

    let min_x = x.max(g_context.mutable_values.clip_x);
    let min_y = y.max(g_context.mutable_values.clip_y);
    let (max_x, max_y) = if (transform >> 2) == 0 {
        (
            (x + w).min(g_context.mutable_values.clip_x_right),
            (y + h).min(g_context.mutable_values.clip_y_bottom),
        )
    } else {
        (
            (x + h).min(g_context.mutable_values.clip_x_right),
            (y + w).min(g_context.mutable_values.clip_y_bottom),
        )
    };

    let mut dy = max_y - min_y;
    let dx_o = max_x - min_x;
    if dy <= 0 || dx_o <= 0 {
        return;
    }

    let src_width = i32::from(src.width);
    let screen_width = g_context.real_screen_width;
    if src_width <= 0 || g_context.screen_width <= 0 || screen_width <= 0 {
        return;
    }

    let mut value: MutPtr<u16>;
    let k: i32;
    let n: i32;

    match transform {
        TRANS_NONE_SPRITE => {
            if src.trans == 0 {
                let mut srcp = src.data + ((sx + min_y - y) * src_width + (min_x - x + sx)) as u32;
                let mut dstp =
                    g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;
                while dy > 0 {
                    libc::string::memcpy(
                        env,
                        dstp.cast_void(),
                        srcp.cast_const().cast_void(),
                        dx_o as u32 * guest_size_of::<u16>(),
                    );
                    dstp += screen_width as u32;
                    srcp += src_width as u32;
                    dy -= 1;
                }
                return;
            }
            value = src.data + ((sx + min_y - y) * src_width + (min_x - x + sx)) as u32;
            k = 1;
            n = src_width;
        }
        TRANS_MIRROR_ROT180_SPRITE => {
            value = src.data + ((h - 1 - (min_y - y - sy)) * src_width + (min_x - x + sx)) as u32;
            k = 1;
            n = -src_width;

            if src.trans == 0 {
                let mut srcp = value;
                let mut dstp =
                    g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;
                while dy > 0 {
                    libc::string::memcpy(
                        env,
                        dstp.cast_void(),
                        srcp.cast_const().cast_void(),
                        dx_o as u32 * guest_size_of::<u16>(),
                    );
                    dstp += screen_width as u32;
                    srcp = ptr_offset(srcp, n);
                    dy -= 1;
                }
                return;
            }
        }
        TRANS_ROT180_SPRITE => {
            value = src.data
                + ((h - 1 + sy) * src_width + w - (min_y - y) * src_width - 1 + sx - min_x + x)
                    as u32;
            k = -1;
            n = -src_width;
        }
        TRANS_MIRROR_SPRITE => {
            value = src.data + ((min_y - y + sy) * src_width + sx + w - 1 + x - min_x) as u32;
            k = -1;
            n = src_width;
        }
        TRANS_ROT90_SPRITE => {
            value = src.data
                + ((h + sy - 1) * src_width + (min_y - y) + sx - (min_x - x) * src_width) as u32;
            k = -src_width;
            n = 1;
        }
        TRANS_MIRROR_ROT90_SPRITE => {
            value = src.data
                + (sx + (h + sy - 1) * src_width + w - min_y + y - 1 - min_x * src_width
                    + x * src_width) as u32;
            k = -src_width;
            n = -1;
        }
        TRANS_ROT270_SPRITE => {
            value = src.data
                + (sx + sy * src_width + w - (min_y - y) - 1 + min_x * src_width - x * src_width)
                    as u32;
            k = src_width;
            n = -1;
        }
        TRANS_MIRROR_ROT270_SPRITE => {
            value = src.data
                + (sy * src_width - y + sx + min_y + min_x * src_width - x * src_width) as u32;
            k = src_width;
            n = 1;
        }
        _ => return,
    }

    let mut dstp_o = g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;

    while dy > 0 {
        let mut srcp = value;
        let mut dstp = dstp_o;
        let mut dx = dx_o;

        while dx > 0 {
            let pixel: u16 = env.mem.read(srcp);
            if src.trans == 0 || pixel != transcolor {
                env.mem.write(dstp, pixel);
            }

            srcp = ptr_offset(srcp, k);
            dstp += 1;
            dx -= 1;
        }

        dstp_o += screen_width as u32;
        value = ptr_offset(value, n);
        dy -= 1;
    }
}

fn null(_: &Mythroad) -> u32 {
    MutVoidPtr::null().to_bits()
}

#[rustfmt::skip]
pub const MR_C_FUNCTION_TABLE: FunctionExports = &[
    Export::Func(export_c_func!(mr_malloc(_))),
    Export::Func(export_c_func!(mr_free(_, _))),
    Export::Func(export_c_func!(mr_realloc(_, _, _))), // 3
    Export::Func(export_c_func!(mr_memcpy(_, _, _))),
    Export::Func(export_c_func!(mr_memmove(_, _, _))),
    Export::Func(export_c_func!(mr_strcpy(_, _))),
    Export::Func(export_c_func!(mr_strncpy(_, _, _))),
    Export::Func(export_c_func!(mr_strcat(_, _))),
    Export::Func(export_c_func!(mr_strncat(_, _, _))),
    Export::Func(export_c_func!(mr_memcmp(_, _, _))),
    Export::Func(export_c_func!(mr_strcmp(_, _))),
    Export::Func(export_c_func!(mr_strncmp(_, _, _))),
    Export::Func(export_c_func!(mr_strcoll(_, _))),
    Export::Func(export_c_func!(mr_memchr(_, _, _))),
    Export::Func(export_c_func!(mr_memset(_, _, _))),
    Export::Func(export_c_func!(mr_strlen(_))),
    Export::Func(export_c_func!(mr_strstr(_, _))),
    Export::Func(export_c_func!(mr_sprintf(_, _, _))),
    Export::Func(export_c_func!(mr_atoi(_))),
    Export::Func(export_c_func!(mr_strtoul(_, _, _))), // 20
    Export::Func(export_c_func!(mr_rand())),
    Export::Data(null),
    Export::Func(export_c_func!(mr_stop_ex(_))), // V1939
    Export::Data(export_c_data!(state.mr_c_internal_table)), // _mr_c_internal_table
    Export::Data(export_c_data!(state.mr_c_port_table)), // _mr_c_port_table
    Export::Func(export_c_func!(mr_c_function_new(_, _))), // 26
    Export::Func(export_c_func!(mr_printf(_, _))),
    Export::Func(export_c_func!(mr_mem_get(_, _))),
    Export::Func(export_c_func!(mr_mem_free(_, _))),
    Export::Func(export_c_func!(mr_draw_bitmap(_, _, _, _, _))),
    Export::Func(export_c_func!(mr_get_char_bitmap(_, _, _, _))),
    Export::Func(export_c_func!(mr_timer_start(_))),
    Export::Func(export_c_func!(mr_timer_stop())),
    Export::Func(export_c_func!(mr_get_time())),
    Export::Func(export_c_func!(mr_get_datetime(_))),
    Export::Func(export_c_func!(mr_get_user_info(_))),
    Export::Func(export_c_func!(mr_sleep(_))), // 37
    Export::Func(export_c_func!(mr_plat(_, _))),
    Export::Func(export_c_func!(mr_plat_ex(_, _, _, _, _, _))), // 39
    Export::Func(export_c_func!(mr_ferrno())),
    Export::Func(export_c_func!(mr_open(_, _))),
    Export::Func(export_c_func!(mr_close(_))),
    Export::Func(export_c_func!(mr_info(_))),
    Export::Func(export_c_func!(mr_write(_, _, _))),
    Export::Func(export_c_func!(mr_read(_, _, _))),
    Export::Func(export_c_func!(mr_seek(_, _, _))),
    Export::Func(export_c_func!(mr_get_len(_))),
    Export::Func(export_c_func!(mr_remove(_))),
    Export::Func(export_c_func!(mr_rename(_, _))),
    Export::Func(export_c_func!(mr_mkdir(_))),
    Export::Func(export_c_func!(mr_rmdir(_))),
    Export::Func(export_c_func!(mr_find_start(_, _, _))),
    Export::Func(export_c_func!(mr_find_get_next(_, _, _))),
    Export::Func(export_c_func!(mr_find_stop(_))), // 54
    Export::Func(export_c_func!(mr_exit())),
    Export::Func(export_c_func!(mr_start_shake(_))),
    Export::Func(export_c_func!(mr_stop_shake())),
    Export::Func(export_c_func!(mr_play_sound(_, _, _, _))),
    Export::Func(export_c_func!(mr_stop_sound(_))), // 59
    Export::Func(export_c_func!(mr_send_sms(_, _, _))),
    Export::Func(export_c_func!(mr_call(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_network_id(_, _, _, _))),
    Export::Func(export_c_func!(mr_connect_wap(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_create(_, _))),
    Export::Func(export_c_func!(mr_menu_set_item(_, _, _))),
    Export::Func(export_c_func!(mr_menu_show(_))),
    Export::Func(export_c_func!(mr_menu_set_focus(_, _))),
    Export::Func(export_c_func!(mr_menu_release(_))),
    Export::Func(export_c_func!(mr_menu_refresh(_))),
    Export::Func(export_c_func!(mr_dialog_create(_, _, _))),
    Export::Func(export_c_func!(mr_dialog_release(_))),
    Export::Func(export_c_func!(mr_dialog_refresh(_, _, _, _))),
    Export::Func(export_c_func!(mr_text_create(_, _, _))),
    Export::Func(export_c_func!(mr_text_release(_))),
    Export::Func(export_c_func!(mr_text_refresh(_, _, _))),
    Export::Func(export_c_func!(mr_edit_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_edit_release(_))),
    Export::Func(export_c_func!(mr_edit_get_text(_))),
    Export::Func(export_c_func!(mr_win_create())),
    Export::Func(export_c_func!(mr_win_release(_))),
    Export::Func(export_c_func!(mr_get_screen_info(_))),
    Export::Func(export_c_func!(mr_init_network(_, _, _, _))),
    Export::Func(export_c_func!(mr_close_network(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_host_by_name(_, _, _, _))),
    Export::Func(export_c_func!(mr_socket(_, _, _, _))),
    Export::Func(export_c_func!(mr_connect(_, _, _, _))),
    Export::Func(export_c_func!(mr_close_socket(_, _, _, _))),
    Export::Func(export_c_func!(mr_recv(_, _, _, _))),
    Export::Func(export_c_func!(mr_recvfrom(_, _, _, _))),
    Export::Func(export_c_func!(mr_send(_, _, _, _))),
    Export::Func(export_c_func!(mr_sendto(_, _, _, _))),
    Export::Data(export_c_data!(state.mr_screen_buf)), // &mr_screenBuf
    Export::Data(export_c_data!(state.mr_screen_w)),   // &mr_screen_w
    Export::Data(export_c_data!(state.mr_screen_h)),   // &mr_screen_h
    Export::Data(export_c_data!(state.mr_screen_bit)), // &mr_screen_bit
    Export::Data(export_c_data!(state.mr_bitmap)),
    Export::Data(export_c_data!(state.mr_tile)),
    Export::Data(export_c_data!(state.mr_map)),
    Export::Data(export_c_data!(state.mr_sound)),
    Export::Data(export_c_data!(state.mr_sprite)),
    Export::Data(export_c_data!(state.pack_filename)), // pack_filename
    Export::Data(export_c_data!(state.start_filename)), // start_filename
    Export::Data(export_c_data!(state.old_pack_filename)), // old_pack_filename
    Export::Data(export_c_data!(state.old_start_filename)), // old_start_filename
    Export::Data(export_c_data!(state.mr_ram_file)),   // &mr_ram_file
    Export::Data(export_c_data!(state.mr_ram_file_len)), // &mr_ram_file_len
    Export::Data(export_c_data!(state.mr_sound_on)),   // &mr_soundOn
    Export::Data(export_c_data!(state.mr_shake_on)),   // &mr_shakeOn
    Export::Data(export_c_data!(state.heap.mem_base)), // &LG_mem_base
    Export::Data(export_c_data!(state.heap.mem_len)),  // &LG_mem_len
    Export::Data(export_c_data!(state.heap.mem_end)),  // &LG_mem_end
    Export::Data(export_c_data!(state.heap.mem_left)), // &LG_mem_left
    Export::Data(export_c_data!(state.mr_sms_cfg_buf)), // &mr_sms_cfg_buf
    Export::Func(export_c_func!(mr_md5_init(_))),
    Export::Func(export_c_func!(mr_md5_append(_, _, _))),
    Export::Func(export_c_func!(mr_md5_finish(_, _))),
    Export::Func(export_c_func!(mr_load_sms_cfg(_, _, _, _))),
    Export::Func(export_c_func!(mr_save_sms_cfg(_, _, _, _))),
    Export::Func(export_c_func!(disp_up_ex(_, _, _, _))),
    Export::Func(export_c_func!(draw_point(_, _, _))),
    Export::Func(export_c_func!(draw_bitmap(_, _, _, _, _, _, _, _, _, _))),
    Export::Func(export_c_func!(draw_bitmap_ex(_, _, _, _, _, _))),
    Export::Func(export_c_func!(draw_rect(_, _, _, _, _, _, _))),
    Export::Func(export_c_func!(draw_text(_, _, _, _, _, _, _, _))),
    Export::Func(export_c_func!(bitmap_check(_, _, _, _, _, _, _))),
    Export::Func(export_c_func!(mr_read_file(_, _, _))),
    Export::Func(export_c_func!(mr_wstrlen(_))),
    Export::Func(export_c_func!(mr_register_app(_, _, _))),
    Export::Func(export_c_func!(draw_text_ex(_, _, _, _))), // 1936
    Export::Func(export_c_func!(mr_eff_set_con(_, _, _, _))),
    Export::Func(export_c_func!(mr_test_com(_, _, _))),
    Export::Func(export_c_func!(mr_test_com1(_, _, _, _))), // 1938
    Export::Func(export_c_func!(mr_c2u(_, _, _))),          // 1939
    Export::Func(export_c_func!(mr_div(_, _))),             // 1941
    Export::Func(export_c_func!(mr_mod(_, _))),
    Export::Data(export_c_data!(state.heap.mem_min)), // &LG_mem_min
    Export::Data(export_c_data!(state.heap.mem_top)), // &LG_mem_top
    Export::Func(export_c_func!(mr_updcrc(_, _))),    // 1943
    Export::Data(export_c_data!(state.start_file_parameter)), // start_fileparameter
    Export::Data(export_c_data!(state.mr_sms_return_flag)), // &mr_sms_return_flag
    Export::Data(export_c_data!(state.mr_sms_return_val)), // &mr_sms_return_val
    Export::Func(export_c_func!(mr_unzip(_, _, _, _))), // 1950
    Export::Data(export_c_data!(state.mr_exit_cb)),   // &mr_exit_cb
    Export::Data(export_c_data!(state.mr_exit_cb_data)), // &mr_exit_cb_data
    Export::Data(export_c_data!(state.entry)),        // mr_entry
    Export::Func(export_c_func!(mr_plat_draw_char(_, _, _, _))), // 2004
    Export::Data(export_c_data!(state.heap.mem_free)), // &LG_mem_free
    Export::Func(export_c_func!(mr_transbitmap_draw(_, _, _, _, _, _, _, _, _, _))),
    Export::Func(export_c_func!(mr_draw_region(_, _, _, _, _, _, _, _, _, _))),
    Export::Data(null),
];
