use crate::abi::{CallFromHost, GuestFunction};
use crate::fs::GuestPath;
use crate::libc;
use crate::libc::posix_io::{self, OpenFlag};
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr};
use crate::mrp;
use crate::mythroad::{
    mr_free, mr_malloc, reset_resource_tables, MrResult, MrRunState, MrTimerState,
};
use crate::Environment;

use std::time::Duration;

const MR_FILE_HANDLE_OFFSET: i32 = 5;
const MR_FILE_RDONLY: u32 = 1;
const MR_FILE_WRONLY: u32 = 2;
const MR_FILE_RDWR: u32 = 4;
const MR_FILE_CREATE: u32 = 8;
const MR_IS_FILE: i32 = 1;
const MR_IS_DIR: i32 = 2;
const MR_IS_INVALID: i32 = 8;
pub(crate) const MR_KEY_PRESS: u32 = 0;
pub(crate) const MR_KEY_RELEASE: u32 = 1;
pub(crate) const MR_MOUSE_DOWN: u32 = 2;
pub(crate) const MR_MOUSE_UP: u32 = 3;
pub(crate) const MR_MOUSE_MOVE: u32 = 12;

pub fn mr_start_dsm_c(env: &mut Environment, entry: Option<&str>) -> u32 {
    let pack_filename = match entry {
        Some(entry) if entry.starts_with('*') => entry,
        Some(entry) if entry.starts_with('%') => &entry[1..],
        Some(entry) if entry.starts_with("#<") => &entry[2..],
        _ => "*A",
    };

    libc::string::memset(env, env.mythroad.state.pack_filename.cast_void(), 0, 128);

    let pack_filename_src = env.mem.alloc_and_write_cstr(pack_filename.as_bytes());
    libc::string::strcpy(
        env,
        env.mythroad.state.pack_filename,
        pack_filename_src.cast_const(),
    );

    libc::string::memset(
        env,
        env.mythroad.state.old_pack_filename.cast_void(),
        0,
        128,
    );
    libc::string::memset(
        env,
        env.mythroad.state.old_start_filename.cast_void(),
        0,
        128,
    );
    libc::string::memset(
        env,
        env.mythroad.state.start_file_parameter.cast_void(),
        0,
        128,
    );

    log_dbg!(
        "Mythroad: mr_start_dsmC(entry={entry:?}, pack_filename={})",
        pack_filename
    );

    intra_start(env, mrp::START_FILE_NAME, entry)
}

fn mr_file_handle_to_posix_fd(handle: u32) -> Option<posix_io::FileDescriptor> {
    let handle = i32::try_from(handle).ok()?;
    handle.checked_sub(MR_FILE_HANDLE_OFFSET)
}

fn posix_fd_to_mr_file_handle(fd: posix_io::FileDescriptor) -> i32 {
    fd + MR_FILE_HANDLE_OFFSET
}

pub(crate) fn mr_open(env: &mut Environment, filename: ConstPtr<u8>, mode: u32) -> i32 {
    if filename.is_null() {
        return 0;
    };

    let mut open_flag: OpenFlag = 0;
    if mode & MR_FILE_RDONLY != 0 {
        open_flag = posix_io::O_RDONLY;
    }
    if mode & MR_FILE_WRONLY != 0 {
        open_flag = posix_io::O_WRONLY;
    }
    if mode & MR_FILE_RDWR != 0 {
        open_flag = posix_io::O_RDWR;
    }

    let filename_str = env.mem.cstr_at_utf8(filename).unwrap().to_owned();
    if mode & MR_FILE_CREATE != 0 && !env.fs.exists(GuestPath::new(&filename_str)) {
        open_flag |= posix_io::O_CREAT;
    }

    let fd = posix_io::open_direct(env, filename, open_flag);

    if fd < 0 {
        log_dbg!(
            "Mythroad: dsm mr_open({filename_str:?}, mode={mode:#x}, flags={open_flag:#x}) failed"
        );
        return 0;
    }

    let handle = posix_fd_to_mr_file_handle(fd);
    log_dbg!(
        "Mythroad: dsm mr_open({filename_str:?}, mode={mode:#x}, flags={open_flag:#x}) -> {handle}"
    );
    handle
}

pub(crate) fn mr_plat_ex(
    _env: &mut Environment,
    _code: u32,
    _input: ConstPtr<u8>,
    _input_len: u32,
    _output: MutPtr<MutPtr<u8>>,
    _output_len: MutPtr<i32>,
    _cb: MutVoidPtr,
) -> i32 {
    MrResult::Ignored as i32
}

pub(crate) fn mr_plat(_env: &mut Environment, _code: u32, _param: u32) -> i32 {
    MrResult::Ignored as i32
}

pub(crate) fn mr_read(env: &mut Environment, handle: u32, buffer: MutVoidPtr, len: u32) -> i32 {
    if handle == 0 {
        return MrResult::Failed as i32;
    }
    let Some(fd) = mr_file_handle_to_posix_fd(handle) else {
        return MrResult::Failed as i32;
    };

    let read_len = posix_io::read(env, fd, buffer, len);
    if read_len < 0 {
        MrResult::Failed as i32
    } else {
        read_len
    }
}

pub(crate) fn mr_write(env: &mut Environment, handle: u32, buffer: ConstVoidPtr, len: u32) -> i32 {
    if handle == 0 {
        return MrResult::Failed as i32;
    }

    let Some(fd) = mr_file_handle_to_posix_fd(handle) else {
        return MrResult::Failed as i32;
    };
    let write_len = posix_io::write(env, fd, buffer, len);
    if write_len < 0 {
        MrResult::Failed as i32
    } else {
        write_len
    }
}

pub(crate) fn mr_close(env: &mut Environment, handle: u32) -> i32 {
    if handle == 0 {
        return MrResult::Failed as i32;
    }
    let Some(fd) = mr_file_handle_to_posix_fd(handle) else {
        return MrResult::Failed as i32;
    };

    if posix_io::close(env, fd) == 0 {
        MrResult::Success as i32
    } else {
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_info(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    if filename.is_null() {
        return MR_IS_INVALID;
    }

    let filename = env.mem.cstr_at_utf8(filename).unwrap();
    let path = GuestPath::new(&filename);

    if env.fs.is_dir(path) {
        MR_IS_DIR
    } else if env.fs.is_file(path) {
        MR_IS_FILE
    } else {
        MR_IS_INVALID
    }
}

pub(crate) fn mr_mkdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    if name.is_null() {
        return MrResult::Failed as i32;
    }

    let name = env.mem.cstr_at_utf8(name).unwrap();
    match env.fs.create_dir(GuestPath::new(&name)) {
        Ok(()) => MrResult::Success as i32,
        Err(_) => MrResult::Failed as i32,
    }
}

pub(crate) fn mr_remove(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    if filename.is_null() {
        return MrResult::Failed as i32;
    }

    if libc::stdio::remove(env, filename) == 0 {
        MrResult::Success as i32
    } else {
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_rename(
    env: &mut Environment,
    oldname: ConstPtr<u8>,
    newname: ConstPtr<u8>,
) -> i32 {
    if oldname.is_null() || newname.is_null() {
        return MrResult::Failed as i32;
    }

    if posix_io::rename(env, oldname, newname) == 0 {
        MrResult::Success as i32
    } else {
        MrResult::Failed as i32
    }
}

pub(crate) fn mr_get_len(env: &mut Environment, filename: ConstPtr<u8>) -> i32 {
    if filename.is_null() {
        return MrResult::Failed as i32;
    }

    let filename = env.mem.cstr_at_utf8(filename).unwrap();
    let len = match env.fs.size(GuestPath::new(&filename)) {
        Ok(len) => len as i32,
        Err(_) => return MrResult::Failed as i32,
    };

    len.try_into().unwrap_or(MrResult::Failed as i32)
}

pub(crate) fn mr_seek(env: &mut Environment, handle: u32, pos: i32, method: i32) -> i32 {
    if handle == 0 {
        return MrResult::Failed as i32;
    }
    let Some(fd) = mr_file_handle_to_posix_fd(handle) else {
        return MrResult::Failed as i32;
    };

    let ret = posix_io::lseek(env, fd, i64::from(pos), method);
    if ret < 0 {
        MrResult::Failed as i32
    } else {
        MrResult::Success as i32
    }
}

pub(crate) fn test_com(env: &mut Environment, _l: u32, input0: u32, input1: u32) -> u32 {
    match input0 {
        0x01 => mr_get_time(env),
        0x02 => {
            env.mythroad.state.mr_event_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success.to_bits()
        }
        0x03 => {
            env.mythroad.state.mr_timer_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success.to_bits()
        }
        0x04 => {
            env.mythroad.state.mr_stop_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success.to_bits()
        }
        0x05 => {
            env.mythroad.state.mr_pause_app_function =
                GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success.to_bits()
        }
        0x06 => {
            env.mythroad.state.mr_resume_app_function =
                GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success.to_bits()
        }
        0x07 => input1,
        0x64 => env.mythroad.state.heap.mem_min.get(&env.mem),
        0x65 => env.mythroad.state.heap.mem_top.get(&env.mem),
        0x66 => env.mythroad.state.heap.mem_left.get(&env.mem),
        0xc8 => mr_start_shake(env, input1),
        0x12c => {
            env.mythroad
                .state
                .mr_sound_on
                .set(&mut env.mem, input1 as i8);
            MrResult::Success.to_bits()
        }
        0x12d => {
            env.mythroad
                .state
                .mr_shake_on
                .set(&mut env.mem, input1 as i8);
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x4);
            MrResult::Success.to_bits()
        }
        0x12e => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x4);
            MrResult::Success.to_bits()
        }
        0x12f => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x4);
            MrResult::Success.to_bits()
        }
        0x130 => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x8);
            MrResult::Success.to_bits()
        }
        0x131 => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x8);
            MrResult::Success.to_bits()
        }
        0x132 => {
            env.mythroad.state.mr_sms_return_flag.set(&mut env.mem, 1);
            env.mythroad
                .state
                .mr_sms_return_val
                .set(&mut env.mem, input1);
            MrResult::Success.to_bits()
        }
        0x133 => {
            env.mythroad.state.mr_sms_return_flag.set(&mut env.mem, 0);
            MrResult::Success.to_bits()
        }
        0x190 => {
            mr_sleep(input1);
            MrResult::Success.to_bits()
        }
        0x191 => {
            let old = env.mythroad.state.sysinfo.screen_width;
            env.mythroad.state.sysinfo.screen_width = input1;
            env.mythroad
                .state
                .mr_screen_w
                .set(&mut env.mem, input1 as i32);
            old
        }
        0x194 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_newSIMInd((int16)input1, NULL);
            MrResult::Success.to_bits()
        }
        0x195 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = mr_closeNetwork();
            MrResult::Success.to_bits()
        }
        0x196 => {
            let old = env.mythroad.state.sysinfo.screen_height;
            env.mythroad.state.sysinfo.screen_height = input1;
            env.mythroad
                .state
                .mr_screen_h
                .set(&mut env.mem, input1 as i32);
            old
        }
        0x197 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            env.mythroad
                .state
                .mr_timer_run_without_pause
                .set(&mut env.mem, input1);
            self::mr_plat(env, 1202, input1);
            MrResult::Success.to_bits()
        }
        0x198 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: implement this branch.
            MrResult::Success.to_bits()
        }
        0x1f4 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = _mr_load_sms_cfg();
            MrResult::Success.to_bits()
        }
        0x1f7 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_smsGetBytes(5, &dst, 1); ret = dst;
            MrResult::Success.to_bits()
        }
        0x1f8 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = _mr_save_sms_cfg(input1);
            MrResult::Success.to_bits()
        }
        0xcb3 => {
            if input1 == 0x9e67a {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x2);
            }
            MrResult::Success.to_bits()
        }
        0xe2d => {
            if input1 == 0xb61 {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x1);
            }
            MrResult::Success.to_bits()
        }
        0xf51 => {
            if input1 == 0x18030 {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x2);
            }
            MrResult::Success.to_bits()
        }
        _ => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            MrResult::Ignored.to_bits()
        }
    }
}

pub(crate) fn test_com1(
    env: &mut Environment,
    _l: u32,
    input0: u32,
    input1: MutPtr<u8>,
    len: u32,
) -> u32 {
    match input0 {
        2 => {
            let old_ram_file = env.mythroad.state.mr_ram_file.get(&env.mem);
            let old_ram_file_len = env.mythroad.state.mr_ram_file_len.get(&env.mem);
            if !old_ram_file.is_null() {
                if let Ok(old_ram_file_len) = u32::try_from(old_ram_file_len) {
                    mr_free(env, old_ram_file.cast_void(), old_ram_file_len);
                }
            }

            env.mythroad.state.mr_ram_file.set(&mut env.mem, input1);
            env.mythroad
                .state
                .mr_ram_file_len
                .set(&mut env.mem, len as i32);
            MrResult::Success.to_bits()
        }
        3 => {
            libc::string::memset(
                env,
                env.mythroad.state.old_pack_filename.cast_void(),
                0,
                128,
            );
            if !input1.is_null() {
                libc::string::strncpy(
                    env,
                    env.mythroad.state.old_pack_filename,
                    input1.cast_const(),
                    127,
                );
            }

            libc::string::memset(
                env,
                env.mythroad.state.old_start_filename.cast_void(),
                0,
                128,
            );
            let start_file_name = env
                .mem
                .alloc_and_write_cstr(mrp::START_FILE_NAME.as_bytes());
            libc::string::strncpy(
                env,
                env.mythroad.state.old_start_filename,
                start_file_name.cast_const(),
                127,
            );
            MrResult::Success.to_bits()
        }
        4 => {
            libc::string::memset(
                env,
                env.mythroad.state.start_file_parameter.cast_void(),
                0,
                128,
            );
            if !input1.is_null() {
                libc::string::strncpy(
                    env,
                    env.mythroad.state.start_file_parameter,
                    input1.cast_const(),
                    127,
                );
            }
            MrResult::Success.to_bits()
        }
        7 | 8 | 9 => MrResult::Success.to_bits(),
        _ => {
            log_dbg!("Mythroad: _mr_TestCom1 got unknown param: code={input0}");
            MrResult::Ignored.to_bits()
        }
    }
}

pub(crate) fn mr_get_time(env: &Environment) -> u32 {
    env.startup_time.elapsed().as_millis() as u32
}

pub(crate) fn mr_timer(env: &mut Environment) -> u32 {
    if env.mythroad.state.mr_timer_state.get(&env.mem) != MrTimerState::Running.to_bits() {
        return MrResult::Ignored.to_bits();
    }

    let elapsed = mr_get_time(env).wrapping_sub(env.mythroad.state.mr_timer_start_time);
    if elapsed < env.mythroad.state.mr_timer_interval {
        return MrResult::Waiting.to_bits();
    }

    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle.to_bits());
    env.mythroad.state.mr_timer_interval = 0;

    let mr_state = env.mythroad.state.mr_state.get(&env.mem);
    let timer_runs_while_paused = env.mythroad.state.mr_timer_run_without_pause.get(&env.mem) != 0;

    if mr_state != MrRunState::Run.to_bits()
        && !(timer_runs_while_paused && mr_state == MrRunState::Pause.to_bits())
    {
        return MrResult::Ignored.to_bits();
    }

    let timer_function = env.mythroad.state.mr_timer_function;
    if timer_function.addr_without_thumb_bit() == 0 {
        return mr_test_com_c(env, 801, MutVoidPtr::null(), 1, 2);
    }

    let timer_status: u32 = timer_function.call_from_host(env, ());
    if timer_status == MrResult::Ignored.to_bits() {
        return mr_test_com_c(env, 801, MutVoidPtr::null(), 1, 2);
    }

    MrResult::Success.to_bits()
}

pub(crate) fn mr_event(env: &mut Environment, code: u32, param0: u32, param1: u32) -> u32 {
    log_dbg!("Mythroad: mr_event(code={code}, param0={param0}, param1={param1})");

    let mr_state = env.mythroad.state.mr_state.get(&env.mem);
    let timer_runs_while_paused = env.mythroad.state.mr_timer_run_without_pause.get(&env.mem) != 0;

    if mr_state != MrRunState::Run.to_bits()
        && !(timer_runs_while_paused && mr_state == MrRunState::Pause.to_bits())
    {
        return MrResult::Ignored.to_bits();
    }

    let event_function = env.mythroad.state.mr_event_function;
    if event_function.addr_without_thumb_bit() != 0 {
        let event_status: u32 = event_function.call_from_host(env, (code, param0, param1));
        if event_status != MrResult::Ignored.to_bits() {
            return event_status;
        }
    }

    let event = env.mem.alloc(5 * std::mem::size_of::<u32>() as u32);
    let event_words: MutPtr<u32> = event.cast();
    env.mem.write(event_words, code);
    env.mem.write(event_words + 1, param0);
    env.mem.write(event_words + 2, param1);
    env.mem.write(event_words + 3, 0u32);
    env.mem.write(event_words + 4, 0u32);

    let result = mr_test_com_c(env, 801, event, 5 * std::mem::size_of::<u32>() as u32, 1);
    env.mem.free(event);
    result
}

fn mr_start_shake(_env: &mut Environment, ms: u32) -> u32 {
    log_dbg!("Mythroad: mr_startShake(ms={ms})");
    MrResult::Success.to_bits()
}

fn mr_sleep(ms: u32) {
    std::thread::sleep(Duration::from_millis(ms.into()));
}

fn intra_start(env: &mut Environment, start_file_name: &str, entry: Option<&str>) -> u32 {
    let null_function = GuestFunction::from_addr_with_thumb_bit(0);

    env.mythroad.state.mr_event_function = null_function;
    env.mythroad.state.mr_timer_function = null_function;
    env.mythroad.state.mr_stop_function = null_function;
    env.mythroad.state.mr_pause_app_function = null_function;
    env.mythroad.state.mr_resume_app_function = null_function;

    let previous_c_function_p = env.mythroad.state.mr_c_function_p;
    let previous_c_function_p_len = env.mythroad.state.mr_c_function_p_len;
    if !previous_c_function_p.is_null() {
        mr_free(env, previous_c_function_p, previous_c_function_p_len);
    }

    env.mythroad.state.mr_c_function_p = MutVoidPtr::null();
    env.mythroad.state.mr_c_function_p_len = 0;
    env.mythroad.state.mr_c_function = null_function;
    env.mythroad
        .state
        .mr_ram_file
        .set(&mut env.mem, MutPtr::<u8>::null());
    env.mythroad.state.mr_ram_file_len.set(&mut env.mem, 0);

    env.mythroad.state.vm_state.set(&mut env.mem, 0);
    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle.to_bits());
    env.mythroad
        .state
        .mr_timer_run_without_pause
        .set(&mut env.mem, 0);
    env.mythroad.state.mr_timer_start_time = mr_get_time(env);
    env.mythroad.state.mr_timer_interval = 0;
    env.mythroad.state.bi.update(&mut env.mem, |bi| bi & 2);

    if !reset_screen_buffer(env) {
        env.mythroad
            .state
            .mr_state
            .set(&mut env.mem, MrRunState::Error.to_bits());
        return MrResult::Failed.to_bits();
    }
    reset_resource_tables(env);

    let entry = entry.unwrap_or("_dsm");
    libc::string::memset(env, env.mythroad.state.entry.cast_void(), 0, 128);

    let entry_src = env.mem.alloc_and_write_cstr(entry.as_bytes());
    libc::string::strncpy(env, env.mythroad.state.entry, entry_src.cast_const(), 127);

    env.mythroad
        .state
        .mr_state
        .set(&mut env.mem, MrRunState::Run.to_bits());

    log_dbg!("Mythroad: mr_intra_start(filename={start_file_name}, entry={entry})");

    let mut ret = mr_do_ext(env, start_file_name);
    if ret != MrResult::Success.to_bits() {
        ret = mr_do_ext(env, mrp::LOGO_EXT_FILE_NAME);
    }

    if ret != MrResult::Success.to_bits() {
        env.mythroad
            .state
            .mr_state
            .set(&mut env.mem, MrRunState::Error.to_bits());
        return MrResult::Failed.to_bits();
    }

    MrResult::Success.to_bits()
}

fn reset_screen_buffer(env: &mut Environment) -> bool {
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    let Ok(screen_w) = u32::try_from(screen_w) else {
        return false;
    };
    let Ok(screen_h) = u32::try_from(screen_h) else {
        return false;
    };
    let Some(screen_pixels) = screen_w.checked_mul(screen_h) else {
        return false;
    };
    let Some(screen_buf_len) = screen_pixels.checked_mul(std::mem::size_of::<u16>() as u32) else {
        return false;
    };

    let mut screen_buf = env.mythroad.state.mr_screen_buf.get(&env.mem);
    if screen_buf.is_null() {
        screen_buf = mr_malloc(env, screen_buf_len).cast();
        if screen_buf.is_null() {
            return false;
        }
        env.mythroad
            .state
            .mr_screen_buf
            .set(&mut env.mem, screen_buf);
    }

    libc::string::memset(env, screen_buf.cast(), 0, screen_buf_len);
    true
}

fn mr_do_ext(env: &mut Environment, filename: &str) -> u32 {
    log_dbg!("Mythroad: mr_doExt(filename={filename})");

    match env.executable.read_file(&env.mem, filename) {
        Ok(ext_data) if !ext_data.is_empty() => {
            let len = ext_data.len().try_into().unwrap();
            let addr = mr_malloc(env, len);
            env.mem
                .bytes_at_mut(addr.cast(), len)
                .copy_from_slice(&ext_data);

            test_com(env, 0, 0xe2d, 0xb61);

            if mr_test_com_c(env, 800, addr, len, 0) != MrResult::Success.to_bits() {
                mr_free(env, addr, len);
                return MrResult::Failed.to_bits();
            }

            mr_test_com_c(env, 801, addr, 0x7cd, 6);

            let app_info = env.mythroad.state.app_info;
            mr_test_com_c(env, 801, app_info, 16, 8);

            mr_test_com_c(env, 801, addr, 0x7cd, 0);

            MrResult::Success.to_bits()
        }
        Ok(_) => {
            log_dbg!("Mythroad: mr_doExt failed: {filename} is empty");
            MrResult::Failed.to_bits()
        }
        Err(err) => {
            log_dbg!("Mythroad: mr_doExt failed: {err}");
            MrResult::Failed.to_bits()
        }
    }
}

pub(crate) fn mr_read_file(
    env: &mut Environment,
    filename: ConstPtr<u8>,
    filelen: MutPtr<i32>,
    lookfor: i32,
) -> MutVoidPtr {
    let filename = match env.mem.cstr_at_utf8(filename) {
        Ok(filename) => filename.to_owned(),
        Err(_) => return MutVoidPtr::null(),
    };

    let pack_filename = match env
        .mem
        .cstr_at_utf8(env.mythroad.state.pack_filename.cast_const())
    {
        Ok(pack_filename) => pack_filename.to_owned(),
        Err(_) => return MutVoidPtr::null(),
    };

    let pack_prefix = pack_filename.as_bytes().first().copied().unwrap_or(0);
    if pack_prefix != b'*' && pack_prefix != b'$' {
        read_mrp_file_from_disk(env, &pack_filename, &filename, filelen, lookfor)
    } else {
        read_mrp_file_from_memory(env, &pack_filename, &filename, filelen, lookfor)
    }
}

fn read_mrp_file_from_disk(
    env: &mut Environment,
    pack_filename: &str,
    filename: &str,
    filelen: MutPtr<i32>,
    lookfor: i32,
) -> MutVoidPtr {
    let pack_data = match env.fs.read(GuestPath::new(pack_filename)) {
        Ok(data) => data,
        Err(_) => return MutVoidPtr::null(),
    };

    if lookfor == 1 {
        return match mrp::find_entry(&pack_data, filename) {
            Ok(Some(_)) => MutVoidPtr::from_bits(1),
            Err(_) => MutVoidPtr::null(),
            Ok(None) => MutVoidPtr::null(),
        };
    }

    let file_data = match mrp::read_file_from_bytes(&pack_data, filename) {
        Ok(file_data) => file_data,
        Err(_) => return MutVoidPtr::null(),
    };

    write_file_data_to_guest(env, &file_data, filelen)
}

fn read_mrp_file_from_memory(
    env: &mut Environment,
    pack_filename: &str,
    filename: &str,
    filelen: MutPtr<i32>,
    lookfor: i32,
) -> MutVoidPtr {
    let Some((pack_base, pack_len)) = memory_pack_range(env, pack_filename) else {
        return MutVoidPtr::null();
    };

    let entry = if let Some(entry) = executable_entry_for_memory_pack(env, pack_filename, filename)
    {
        entry
    } else {
        let pack_data = env.mem.bytes_at(pack_base.cast_const(), pack_len);
        match mrp::find_entry(pack_data, filename) {
            Ok(Some(entry)) => entry,
            Ok(None) | Err(_) => return MutVoidPtr::null(),
        }
    };

    if lookfor == 1 {
        return MutVoidPtr::from_bits(1);
    }

    if !filelen.is_null() {
        let Ok(file_len_i32) = i32::try_from(entry.size) else {
            return MutVoidPtr::null();
        };
        env.mem.write(filelen, file_len_i32);
    }

    let file_ptr = pack_base + entry.offset;
    if lookfor == 2 {
        return file_ptr.cast_void();
    }

    let is_gzip = {
        let raw = env.mem.bytes_at(file_ptr.cast_const(), entry.size);
        crate::gzip::is_gzip(raw)
    };
    if !is_gzip {
        return file_ptr.cast_void();
    }

    let file_data = {
        let raw = env.mem.bytes_at(file_ptr.cast_const(), entry.size);
        match crate::gzip::ungzip(raw) {
            Ok(data) => data,
            Err(_) => return MutVoidPtr::null(),
        }
    };

    write_file_data_to_guest(env, &file_data, filelen)
}

fn executable_entry_for_memory_pack(
    env: &Environment,
    pack_filename: &str,
    filename: &str,
) -> Option<mrp::MrpEntry> {
    if pack_filename.as_bytes().get(0..2) != Some(b"*A") {
        return None;
    }

    let pack_base_bits: u32 = env.mem.read(env.mythroad.state.mr_m0_files);
    if pack_base_bits != env.executable.guest_base.to_bits() {
        return None;
    }

    env.executable.entry(filename).cloned()
}

fn memory_pack_range(env: &Environment, pack_filename: &str) -> Option<(MutPtr<u8>, u32)> {
    match pack_filename.as_bytes().first().copied()? {
        b'*' => {
            let index = pack_filename
                .as_bytes()
                .get(1)
                .copied()?
                .checked_sub(b'A')?;
            if index >= 8 {
                return None;
            }
            let pack_base_bits: u32 = env
                .mem
                .read(env.mythroad.state.mr_m0_files + u32::from(index));
            let pack_base = MutPtr::<u8>::from_bits(pack_base_bits);
            if pack_base.is_null() {
                return None;
            }

            let header = env.mem.bytes_at(pack_base.cast_const(), 16);
            let header = mrp::MrpHeader::parse(header).ok()?;
            Some((pack_base, header.mrp_file_size))
        }
        b'$' => {
            let pack_base = env.mythroad.state.mr_ram_file.get(&env.mem);
            let pack_len = env.mythroad.state.mr_ram_file_len.get(&env.mem);
            let pack_len = u32::try_from(pack_len).ok()?;
            if pack_base.is_null() || pack_len == 0 {
                return None;
            }
            Some((pack_base, pack_len))
        }
        _ => None,
    }
}

fn write_file_data_to_guest(
    env: &mut Environment,
    file_data: &[u8],
    filelen: MutPtr<i32>,
) -> MutVoidPtr {
    let Ok(file_len_i32) = i32::try_from(file_data.len()) else {
        return MutVoidPtr::null();
    };

    if !filelen.is_null() {
        env.mem.write(filelen, file_len_i32);
    }

    let Ok(file_len_u32) = u32::try_from(file_data.len()) else {
        return MutVoidPtr::null();
    };

    let out = mr_malloc(env, file_len_u32);
    if out.is_null() {
        return MutVoidPtr::null();
    }

    if mrp::copy_bytes_to_guest(&mut env.mem, out.cast(), file_data).is_err() {
        return MutVoidPtr::null();
    }

    out
}

pub(crate) fn mr_test_com_c(
    env: &mut Environment,
    kind: u32,
    input: MutVoidPtr,
    len: u32,
    code: u32,
) -> u32 {
    log_dbg!(
        "Mythroad: mr_testComC(type={kind}, input={:#x}, len={len}, code={code})",
        input.to_bits()
    );

    if kind == 800 {
        env.mem
            .write(input.cast(), env.syscall.function_table_ptr());
        env.mythroad.state.mr_c_function_load =
            GuestFunction::from_addr_with_thumb_bit(input.to_bits() + 8);

        let mr_c_function_load = env.mythroad.state.mr_c_function_load;
        return mr_c_function_load.call_from_host(env, ());
    }

    let mr_c_function = env.mythroad.state.mr_c_function;
    let mr_c_function_p = env.mythroad.state.mr_c_function_p;

    mr_c_function.call_from_host(env, (mr_c_function_p, code, input, len))
}
