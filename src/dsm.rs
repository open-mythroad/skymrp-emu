use crate::abi::{CallFromHost, GuestFunction};
use crate::libc;
use crate::mem::MutVoidPtr;
use crate::mrp;
use crate::mythroad::{mr_free, mr_malloc, MrResult, MrRunState, MrTimerState};
use crate::Environment;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MR_READ_MAX_LEN: usize = 1024 * 400;

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

    println!(
        "Mythroad: mr_start_dsmC(entry={entry:?}, pack_filename={})",
        pack_filename
    );

    intra_start(env, mrp::START_FILE_NAME, entry)
}

pub(crate) fn test_com(env: &mut Environment, _l: u32, input0: u32, input1: u32) -> u32 {
    match input0 {
        0x01 => mr_get_time(),
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
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_newSIMInd((int16)input1, NULL);
            MrResult::Success.to_bits()
        }
        0x195 => {
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
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
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: mr_timer_run_without_pause = (void *)input1;
            // TODO: mr_plat(1202, input1);
            MrResult::Success.to_bits()
        }
        0x198 => {
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: implement this branch.
            MrResult::Success.to_bits()
        }
        0x1f4 => {
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = _mr_load_sms_cfg();
            MrResult::Success.to_bits()
        }
        0x1f7 => {
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_smsGetBytes(5, &dst, 1); ret = dst;
            MrResult::Success.to_bits()
        }
        0x1f8 => {
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
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
            println!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            MrResult::Ignored.to_bits()
        }
    }
}

fn mr_get_time() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u32
}

fn mr_start_shake(_env: &mut Environment, ms: u32) -> u32 {
    println!("Mythroad: mr_startShake(ms={ms})");
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

    env.mythroad.state.mr_c_function_p = crate::mem::MutVoidPtr::null();
    env.mythroad.state.mr_c_function_p_len = 0;

    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle.to_bits());
    env.mythroad.state.bi.update(&mut env.mem, |bi| bi & 2);

    let entry = entry.unwrap_or("_dsm");
    libc::string::memset(env, env.mythroad.state.entry.cast_void(), 0, 128);

    let entry_src = env.mem.alloc_and_write_cstr(entry.as_bytes());
    libc::string::strncpy(env, env.mythroad.state.entry, entry_src.cast_const(), 127);

    env.mythroad
        .state
        .mr_state
        .set(&mut env.mem, MrRunState::Run.to_bits());

    println!("Mythroad: mr_intra_start(filename={start_file_name}, entry={entry})");

    let mut ret = mr_do_ext(env, start_file_name);
    if ret != MrResult::Success.to_bits() {
        ret = mr_do_ext(env, mrp::START_FILE_NAME);
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

fn mr_do_ext(env: &mut Environment, filename: &str) -> u32 {
    println!("Mythroad: mr_doExt(filename={filename})");

    match mr_read_file(env, filename, false) {
        Some(ext_data) if !ext_data.is_empty() => {
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
        _ => {
            println!("Mythroad: mr_doExt failed: {filename}");
            MrResult::Failed.to_bits()
        }
    }
}

fn mr_read_file(env: &mut Environment, filename: &str, lookfor: bool) -> Option<Vec<u8>> {
    println!("Mythroad: mr_readFile(filename={filename}, lookfor={lookfor})");

    if lookfor {
        return mrp::read_file(&env.executable.data, filename)
            .map(|_| Vec::new())
            .ok();
    }

    let data = match mrp::read_file(&env.executable.data, filename) {
        Ok(data) => data,
        Err(err) => {
            println!("Mythroad: mr_readFile failed: {err}");
            return None;
        }
    };

    if data.len() > MR_READ_MAX_LEN {
        println!(
            "Mythroad: read_mrp_file failed: {filename} is too large ({})",
            data.len()
        );
        return None;
    }

    Some(data)
}

fn mr_test_com_c(env: &mut Environment, kind: u32, input: MutVoidPtr, len: u32, code: u32) -> u32 {
    println!(
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
