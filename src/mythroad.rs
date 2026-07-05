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

mod fs;
mod graphics;
use self::fs::{
    mr_close, mr_find_get_next, mr_find_start, mr_find_stop, mr_get_len, mr_info, mr_mkdir,
    mr_open, mr_read, mr_read_file, mr_remove, mr_rename, mr_rmdir, mr_seek, mr_write,
};
use graphics::{
    bitmap_check, disp_up_ex, draw_bitmap, draw_bitmap_ex, draw_point, draw_rect, draw_text,
    draw_text_ex, mr_draw_bitmap, mr_draw_region, mr_eff_set_con, mr_get_char_bitmap,
    mr_get_screen_info, mr_plat_draw_char, mr_transbitmap_draw, MrBitmap, MrSprite, MrTile,
};

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
pub struct MrSound {
    pub p: MutVoidPtr,
    pub buflen: u32,
    pub type_: i32,
}

impl SafeRead for MrSound {}

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

fn mr_call(env: &mut Environment, number: ConstPtr<u8>) {
    log_dbg!(
        "Mythroad: mr_call(number={:#x}) called from {:#x}",
        number.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_network_id(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: mr_getNetworkID() -> 0 called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    0
}

fn mr_connect_wap(env: &mut Environment, wap: ConstPtr<u8>) {
    log_dbg!(
        "Mythroad: mr_connectWAP(wap={:#x}) called from {:#x}",
        wap.to_bits(),
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

fn mr_load_sms_cfg(env: &mut Environment) -> i32 {
    log_dbg!(
        "Mythroad: _mr_load_sms_cfg() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::string::memset(
        env,
        env.mythroad.state.mr_sms_cfg_buf.cast_void(),
        0,
        MR_SMS_CFG_BUF_LEN,
    );

    MrResult::Success as i32
}

fn mr_save_sms_cfg(env: &mut Environment, f: i32) -> i32 {
    log_dbg!(
        "Mythroad: _mr_save_sms_cfg(f={f}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    MrResult::Success as i32
}

#[inline]
fn make_plat_version(plat: u32, ver: u32, card: u32, r#impl: u32, brun: u32) -> u32 {
    100_000_000 + (plat * 1_000_000) + (ver * 10_000) + (card * 1_000) + (r#impl * 10) + brun
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
    Export::Func(export_c_func!(mr_call(_))),
    Export::Func(export_c_func!(mr_get_network_id())),
    Export::Func(export_c_func!(mr_connect_wap(_))),
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
    Export::Func(export_c_func!(mr_load_sms_cfg())),
    Export::Func(export_c_func!(mr_save_sms_cfg(_))),
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
    Export::Func(export_c_func!(draw_text_ex(_, _, _, _, _, _, _))), // 1936
    Export::Func(export_c_func!(mr_eff_set_con(_, _, _, _, _, _, _))),
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
