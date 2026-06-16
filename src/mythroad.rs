use crate::abi::{DotDotDot, GuestFunction};
use crate::cpu::Cpu;
use crate::dsm;
use crate::libc;
use crate::mem::{
    guest_size_of, ConstPtr, ConstVoidPtr, GuestUSize, GuestVar, Memory, MutPtr, MutVoidPtr, Ptr,
};
use crate::syscall::{export_c_data, export_c_func, Export, FunctionExports};
use crate::Environment;

pub struct Mythroad {
    pub state: State,
}

impl Mythroad {
    pub fn new(mem: &mut Memory) -> Mythroad {
        Self {
            state: State::new(mem),
        }
    }
}

#[repr(i32)]
pub enum MrResult {
    Failed = -1,
    Success = 0,
    Ignored = 1,
    Waiting = 2,
}

impl MrResult {
    pub fn to_bits(self) -> u32 {
        self as i32 as u32
    }
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

impl MrRunState {
    pub fn to_bits(self) -> u32 {
        self as u32
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

impl MrTimerState {
    pub fn to_bits(self) -> u32 {
        self as u32
    }
}

pub struct MrHeap {
    pub mem_base: GuestVar<u32>,
    pub mem_len: GuestVar<u32>,
    pub mem_end: GuestVar<u32>,
    pub mem_left: GuestVar<u32>,
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
            mem_base: GuestVar::new(mem, Self::MR_HEAP_BASE),
            mem_len: GuestVar::new(mem, Self::MR_HEAP_LEN),
            mem_end: GuestVar::new(mem, Self::MR_HEAP_END),
            mem_left: GuestVar::new(mem, Self::MR_HEAP_LEN),
            mem_min: GuestVar::new(mem, Self::MR_HEAP_LEN),
            mem_top: GuestVar::new(mem, 0u32),
            mem_free: write_u32_table(mem, &[0, 0]),
        }
    }

    pub fn malloc(&mut self, mem: &mut Memory, len: GuestUSize) -> MutVoidPtr {
        if len == 0 {
            return MutVoidPtr::null();
        }

        let ptr = mem.alloc(len);
        let left = self.mem_left.get(mem).saturating_sub(len);
        self.mem_left.set(mem, left);
        self.mem_min.update(mem, |min| min.min(left));
        ptr
    }

    pub fn free(&mut self, mem: &mut Memory, ptr: MutVoidPtr, len: GuestUSize) {
        if ptr.is_null() || len == 0 {
            return;
        }

        mem.free(ptr);
        let left = self
            .mem_left
            .get(mem)
            .saturating_add(len)
            .min(Self::MR_HEAP_LEN);
        self.mem_left.set(mem, left);
    }
}

pub struct State {
    pub mr_c_function_p: MutVoidPtr,
    pub mr_c_function_p_len: u32,
    pub mr_c_function: GuestFunction,
    pub mr_state: GuestVar<u32>,
    pub mr_timer_state: GuestVar<u32>,
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
    pub mr_ram_file: GuestVar<MutPtr<u8>>,
    pub mr_ram_file_len: GuestVar<i32>,
    pub mr_sms_cfg_buf: MutPtr<u8>,
    pub mr_exit_cb: GuestVar<u32>,
    pub mr_exit_cb_data: GuestVar<i32>,
}

const MR_FILE_MAX_LEN: GuestUSize = 128;
const MR_M0_FILES: GuestUSize = 8;
const MR_SMS_CFG_BUF_LEN: GuestUSize = 120 * 36;

pub struct SysInfo {
    pub screen_width: u32,
    pub screen_height: u32,
}

impl SysInfo {
    pub fn new() -> SysInfo {
        Self::default()
    }
}

impl Default for SysInfo {
    fn default() -> SysInfo {
        Self {
            screen_width: 240,
            screen_height: 320,
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
        let mr_state = GuestVar::new(mem, MrRunState::Idle.to_bits());
        let bi = GuestVar::new(mem, 0u32);
        let mr_timer_p = GuestVar::new(mem, 0u32);
        let mr_timer_state = GuestVar::new(mem, MrTimerState::Idle.to_bits());
        let mr_timer_run_without_pause = GuestVar::new(mem, 0u32);
        let mr_sound_on = GuestVar::new(mem, 1i8);
        let mr_shake_on = GuestVar::new(mem, 1i8);
        let mr_sms_return_flag = GuestVar::new(mem, 0u32);
        let mr_sms_return_val = GuestVar::new(mem, 0u32);
        let screen_buf = alloc_array(mem, 240 * 320);
        let mr_screen_buf = GuestVar::new(mem, screen_buf);
        let mr_screen_w = GuestVar::new(mem, 240i32);
        let mr_screen_h = GuestVar::new(mem, 320i32);
        let mr_screen_bit = GuestVar::new(mem, 16i32);
        let mr_ram_file = GuestVar::new(mem, MutPtr::<u8>::null());
        let mr_ram_file_len = GuestVar::new(mem, 0i32);
        let mr_sms_cfg_buf = alloc_array(mem, MR_SMS_CFG_BUF_LEN);
        let mr_exit_cb = GuestVar::new(mem, 0u32);
        let mr_exit_cb_data = GuestVar::new(mem, 0i32);

        let mr_c_function_p = MutVoidPtr::null();
        let mr_c_function_p_len = 0;
        let mr_c_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_c_function_load = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_event_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_timer_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_stop_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_pause_app_function = GuestFunction::from_addr_with_thumb_bit(0);
        let mr_resume_app_function = GuestFunction::from_addr_with_thumb_bit(0);
        let sysinfo = SysInfo::default();
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
            mr_ram_file,
            mr_ram_file_len,
            mr_sms_cfg_buf,
            mr_exit_cb,
            mr_exit_cb_data,
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

    libc::stdio::sprintf(env, buf, fmt, args)
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

fn mr_stop_ex(env: &mut Environment) {
    log_dbg!(
        "Mythroad: mr_stop_ex() called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_c_function_new(env: &mut Environment, func: GuestFunction, len: u32) -> u32 {
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
            .set(&mut env.mem, MrRunState::Error.to_bits());
        return MrResult::Failed.to_bits();
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

    MrResult::Success.to_bits()
}

fn mr_printf(env: &mut Environment, format: ConstPtr<u8>, args: DotDotDot) -> i32 {
    log_dbg!(
        "Mythroad: mr_printf(format={:#x}) called from {:#x}",
        format.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    libc::stdio::printf(env, format, args)
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

fn mr_draw_bitmap(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_drawBitmap(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_char_bitmap(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getCharBitmap(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_timer_start(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_timerStart(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_timer_stop(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_timerStop(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_time(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getTime(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_datetime(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getDatetime(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_user_info(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getUserInfo(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_sleep(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_sleep(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_plat(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_plat(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_plat_ex(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_platEx(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_ferrno(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_ferrno(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
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

fn mr_info(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_info(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_write(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_write(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_read(env: &mut Environment, handle: u32, buffer: MutVoidPtr, len: u32) -> i32 {
    log_dbg!(
        "Mythroad: mr_read(handle={handle:#x}, buffer={buffer:?}, len={len:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    dsm::mr_read(env, handle, buffer, len)
}

fn mr_seek(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_seek(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_len(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getLen(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_remove(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_remove(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_rename(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_rename(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_mk_dir(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_mkDir(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_rm_dir(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_rmDir(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_find_start(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_findStart(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_find_get_next(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_findGetNext(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_find_stop(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_findStop(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_exit(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_exit(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_start_shake(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_startShake(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_stop_shake(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_stopShake(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_play_sound(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_playSound(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_stop_sound(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_stopSound(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_send_sms(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_sendSms(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
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

fn mr_menu_create(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuCreate(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_set_item(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuSetItem(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_show(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuShow(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_set_focus(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuSetFocus(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_release(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuRelease(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_menu_refresh(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_menuRefresh(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_dialog_create(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_dialogCreate(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_dialog_release(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_dialogRelease(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_dialog_refresh(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_dialogRefresh(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_text_create(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_textCreate(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_text_release(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_textRelease(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_text_refresh(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_textRefresh(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_edit_create(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_editCreate(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_edit_release(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_editRelease(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_edit_get_text(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_editGetText(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_win_create(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_winCreate(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_win_release(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_winRelease(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_get_screen_info(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_getScreenInfo(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
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

fn mr_bitmap(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_bitmap(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_tile(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_tile(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_map(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_map(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_sound(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_sound(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_sprite(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_sprite(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_md5_init(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_md5_init(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_md5_append(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_md5_append(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_md5_finish(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_md5_finish(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
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

fn disp_up_ex(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DispUpEx(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn draw_point(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DrawPoint(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn draw_bitmap(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DrawBitmap(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn draw_bitmap_ex(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DrawBitmapEx(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn draw_rect(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: DrawRect(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn draw_text(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _DrawText(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn bitmap_check(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _BitmapCheck(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_read_file(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_readFile(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_wstrlen(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_wstrlen(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_register_app(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_registerAPP(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
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

fn mr_test_com(env: &mut Environment, l: u32, input0: u32, input1: u32) -> u32 {
    log_dbg!(
        "Mythroad: _mr_TestCom(L={l}, input0={input0}, input1={input1}) called from {:#x}",
        env.cpu.regs()[Cpu::PC]
    );
    dsm::test_com(env, l, input0, input1)
}

fn mr_test_com1(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_TestCom1(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn c2u(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: c2u(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_div(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_div(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_mod(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: _mr_mod(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_updcrc(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_updcrc(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_unzip(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_unzip(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_entry(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_entry(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_plat_draw_char(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_platDrawChar(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_transbitmap_draw(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_transbitmapDraw(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn mr_draw_region(env: &mut Environment, a0: u32, a1: u32, a2: u32, a3: u32) {
    log_dbg!(
        "Mythroad: mr_drawRegion(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
}

fn null(_: &Mythroad) -> u32 {
    MutVoidPtr::null().to_bits()
}

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
    Export::Func(export_c_func!(mr_stop_ex())), // V1939
    Export::Data(export_c_data!(state.mr_c_internal_table)), // _mr_c_internal_table
    Export::Data(export_c_data!(state.mr_c_port_table)), // _mr_c_port_table
    Export::Func(export_c_func!(mr_c_function_new(_, _))), // 26
    Export::Func(export_c_func!(mr_printf(_, _))),
    Export::Func(export_c_func!(mr_mem_get(_, _))),
    Export::Func(export_c_func!(mr_mem_free(_, _))),
    Export::Func(export_c_func!(mr_draw_bitmap(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_char_bitmap(_, _, _, _))),
    Export::Func(export_c_func!(mr_timer_start(_, _, _, _))),
    Export::Func(export_c_func!(mr_timer_stop(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_time(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_datetime(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_user_info(_, _, _, _))),
    Export::Func(export_c_func!(mr_sleep(_, _, _, _))), // 37
    Export::Func(export_c_func!(mr_plat(_, _, _, _))),
    Export::Func(export_c_func!(mr_plat_ex(_, _, _, _))), // 39
    Export::Func(export_c_func!(mr_ferrno(_, _, _, _))),
    Export::Func(export_c_func!(mr_open(_, _))),
    Export::Func(export_c_func!(mr_close(_))),
    Export::Func(export_c_func!(mr_info(_, _, _, _))),
    Export::Func(export_c_func!(mr_write(_, _, _, _))),
    Export::Func(export_c_func!(mr_read(_, _, _))),
    Export::Func(export_c_func!(mr_seek(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_len(_, _, _, _))),
    Export::Func(export_c_func!(mr_remove(_, _, _, _))),
    Export::Func(export_c_func!(mr_rename(_, _, _, _))),
    Export::Func(export_c_func!(mr_mk_dir(_, _, _, _))),
    Export::Func(export_c_func!(mr_rm_dir(_, _, _, _))),
    Export::Func(export_c_func!(mr_find_start(_, _, _, _))),
    Export::Func(export_c_func!(mr_find_get_next(_, _, _, _))),
    Export::Func(export_c_func!(mr_find_stop(_, _, _, _))), // 54
    Export::Func(export_c_func!(mr_exit(_, _, _, _))),
    Export::Func(export_c_func!(mr_start_shake(_, _, _, _))),
    Export::Func(export_c_func!(mr_stop_shake(_, _, _, _))),
    Export::Func(export_c_func!(mr_play_sound(_, _, _, _))),
    Export::Func(export_c_func!(mr_stop_sound(_, _, _, _))), // 59
    Export::Func(export_c_func!(mr_send_sms(_, _, _, _))),
    Export::Func(export_c_func!(mr_call(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_network_id(_, _, _, _))),
    Export::Func(export_c_func!(mr_connect_wap(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_set_item(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_show(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_set_focus(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_release(_, _, _, _))),
    Export::Func(export_c_func!(mr_menu_refresh(_, _, _, _))),
    Export::Func(export_c_func!(mr_dialog_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_dialog_release(_, _, _, _))),
    Export::Func(export_c_func!(mr_dialog_refresh(_, _, _, _))),
    Export::Func(export_c_func!(mr_text_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_text_release(_, _, _, _))),
    Export::Func(export_c_func!(mr_text_refresh(_, _, _, _))),
    Export::Func(export_c_func!(mr_edit_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_edit_release(_, _, _, _))),
    Export::Func(export_c_func!(mr_edit_get_text(_, _, _, _))),
    Export::Func(export_c_func!(mr_win_create(_, _, _, _))),
    Export::Func(export_c_func!(mr_win_release(_, _, _, _))),
    Export::Func(export_c_func!(mr_get_screen_info(_, _, _, _))),
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
    Export::Func(export_c_func!(mr_bitmap(_, _, _, _))),
    Export::Func(export_c_func!(mr_tile(_, _, _, _))),
    Export::Func(export_c_func!(mr_map(_, _, _, _))),
    Export::Func(export_c_func!(mr_sound(_, _, _, _))),
    Export::Func(export_c_func!(mr_sprite(_, _, _, _))),
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
    Export::Func(export_c_func!(mr_md5_init(_, _, _, _))),
    Export::Func(export_c_func!(mr_md5_append(_, _, _, _))),
    Export::Func(export_c_func!(mr_md5_finish(_, _, _, _))),
    Export::Func(export_c_func!(mr_load_sms_cfg(_, _, _, _))),
    Export::Func(export_c_func!(mr_save_sms_cfg(_, _, _, _))),
    Export::Func(export_c_func!(disp_up_ex(_, _, _, _))),
    Export::Func(export_c_func!(draw_point(_, _, _, _))),
    Export::Func(export_c_func!(draw_bitmap(_, _, _, _))),
    Export::Func(export_c_func!(draw_bitmap_ex(_, _, _, _))),
    Export::Func(export_c_func!(draw_rect(_, _, _, _))),
    Export::Func(export_c_func!(draw_text(_, _, _, _))),
    Export::Func(export_c_func!(bitmap_check(_, _, _, _))),
    Export::Func(export_c_func!(mr_read_file(_, _, _, _))),
    Export::Func(export_c_func!(mr_wstrlen(_, _, _, _))),
    Export::Func(export_c_func!(mr_register_app(_, _, _, _))),
    Export::Func(export_c_func!(draw_text_ex(_, _, _, _))), // 1936
    Export::Func(export_c_func!(mr_eff_set_con(_, _, _, _))),
    Export::Func(export_c_func!(mr_test_com(_, _, _))),
    Export::Func(export_c_func!(mr_test_com1(_, _, _, _))), // 1938
    Export::Func(export_c_func!(c2u(_, _, _, _))),          // 1939
    Export::Func(export_c_func!(mr_div(_, _, _, _))),       // 1941
    Export::Func(export_c_func!(mr_mod(_, _, _, _))),
    Export::Data(export_c_data!(state.heap.mem_min)), // &LG_mem_min
    Export::Data(export_c_data!(state.heap.mem_top)), // &LG_mem_top
    Export::Func(export_c_func!(mr_updcrc(_, _, _, _))), // 1943
    Export::Data(export_c_data!(state.start_file_parameter)), // start_fileparameter
    Export::Data(export_c_data!(state.mr_sms_return_flag)), // &mr_sms_return_flag
    Export::Data(export_c_data!(state.mr_sms_return_val)), // &mr_sms_return_val
    Export::Func(export_c_func!(mr_unzip(_, _, _, _))), // 1950
    Export::Data(export_c_data!(state.mr_exit_cb)),   // &mr_exit_cb
    Export::Data(export_c_data!(state.mr_exit_cb_data)), // &mr_exit_cb_data
    Export::Func(export_c_func!(mr_entry(_, _, _, _))), // 1952
    Export::Func(export_c_func!(mr_plat_draw_char(_, _, _, _))), // 2004
    Export::Data(export_c_data!(state.heap.mem_free)), // &LG_mem_free
    Export::Func(export_c_func!(mr_transbitmap_draw(_, _, _, _))),
    Export::Func(export_c_func!(mr_draw_region(_, _, _, _))),
    Export::Data(null),
];
