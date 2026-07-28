/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::{CallFromHost, GuestFunction};
use crate::encoding;
use crate::fs::{GuestPath, GuestPathBuf, MYTHROAD};
use crate::haptics;
use crate::libc;
use crate::libc::posix_io::stat::mode_t;
use crate::libc::posix_io::{self, OpenFlag};
use crate::mem::{guest_size_of, ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr};
use crate::mrp;
use crate::mythroad::{
    mr_free, mr_malloc, mr_stop, reset_resource_tables, MrEvent, MrResult, MrRunState,
    MrTimerState, MR_FILE_MAX_LEN,
};
use crate::Environment;

use encoding_rs::GBK;
use std::time::Duration;

const DSM_MAX_FILE_LEN: usize = 256;
const DSM_HIDE_DRIVE: &str = ".disk";
const DSM_DRIVE_A: &str = "a";
const DSM_DRIVE_B: &str = "b";
const DSM_ROOT_PATH_SYS: &str = "mythroad";
const MR_FILE_HANDLE_OFFSET: i32 = 5;
const MR_FILE_RDONLY: u32 = 1;
const MR_FILE_WRONLY: u32 = 2;
const MR_FILE_RDWR: u32 = 4;
const MR_FILE_CREATE: u32 = 8;
const MR_IS_FILE: i32 = 1;
const MR_IS_DIR: i32 = 2;
const MR_IS_INVALID: i32 = 8;
const MR_MALLOC_EX: u32 = 1001;
const MR_MFREE_EX: u32 = 1002;
const MR_MALLOC_CACHE: u32 = 1012;
const MR_MFREE_CACHE: u32 = 1013;
const MR_MALLOC_SCRRAM: u32 = 1014;
const MR_FREE_SCRRAM: u32 = 1015;
const MR_CHARACTER_HEIGHT: u32 = 1201;
const MR_SWITCHPATH: u32 = 1204;
const MR_UCS2GB: u32 = 1207;
const MR_TURN_ON_BACKLIGHT: u32 = 1222;
const MR_TURN_OFF_BACKLIGHT: u32 = 1223;
const MR_CONNECT: u32 = 1001;
const MR_SET_SOCTIME: u32 = 1002;
const MR_SMS_PROMPT: u32 = 1011;
const MR_SIGNAL_INIT: u32 = 1016;
const MR_SMS_CENTER: u32 = 1106;
const MR_CHECK_TOUCH: u32 = 1205;
const MR_GET_HANDSET_LG: u32 = 1206;
const MR_GET_RAND: u32 = 1211;
const MR_SET_KEY_END: u32 = 1214;
const MR_MSDC_STATUS: u32 = 1218;
const MR_GET_FILE_POS: u32 = 1231;
const MR_SET_VOL: u32 = 1302;
const MR_WIFI_AVAILABLE: u32 = 1327;
const MR_USE_WIFI: u32 = 1328;
const MR_BACKGROUND_SUPPORT: u32 = 1391;
pub(crate) const MR_KEY_PRESS: i32 = 0;
pub(crate) const MR_KEY_RELEASE: i32 = 1;
pub(crate) const MR_MOUSE_DOWN: i32 = 2;
pub(crate) const MR_MOUSE_UP: i32 = 3;
pub(crate) const MR_MOUSE_MOVE: i32 = 12;

#[allow(dead_code)]
#[repr(i32)]
enum MrScreenType {
    Normal = 1000,
    Touch,
    OnlyTouch,
}

#[allow(dead_code)]
#[repr(i32)]
enum MrLanguage {
    Chinese = 1000,
    English,
    TraditionalChinese,
    Spanish,
    Danish,
    Polish,
    French,
    German,
    Italian,
    Thai,
    Russian,
    Bulgarian,
    Ukrainian,
    Portuguese,
    Turkish,
    Vietnamese,
    Indonesian,
    Czech,
    Malay,
    Finnish,
    Hungarian,
    Slovak,
    Dutch,
    Norwegian,
    Swedish,
    Croatian,
    Romanian,
    Slovenian,
    Greek,
    Hebrew,
    Arabic,
    Persian,
    Urdu,
    Hindi,
    Marathi,
    Tamil,
    Bengali,
    Punjabi,
    Telugu,
}

#[allow(dead_code)]
#[repr(i32)]
enum MrMsdcStatus {
    NotExist = 1000,
    Ok,
    NotUseful,
}

pub fn mr_start_dsm_c(env: &mut Environment, entry: Option<&str>) -> i32 {
    let pack_filename = match entry {
        Some(entry) if entry.starts_with('*') => entry,
        Some(entry) if entry.starts_with('%') => &entry[1..],
        Some(entry) if entry.starts_with("#<") => &entry[2..],
        _ => "*A",
    };

    libc::string::memset(
        env,
        env.mythroad.state.pack_filename.cast_void(),
        0,
        MR_FILE_MAX_LEN,
    );

    let pack_filename_src = env.mem.alloc_and_write_cstr(pack_filename.as_bytes());
    libc::string::strcpy(
        env,
        env.mythroad.state.pack_filename,
        pack_filename_src.cast_const(),
    );
    env.mem.free(pack_filename_src.cast_void());

    libc::string::memset(
        env,
        env.mythroad.state.old_pack_filename.cast_void(),
        0,
        MR_FILE_MAX_LEN,
    );
    libc::string::memset(
        env,
        env.mythroad.state.old_start_filename.cast_void(),
        0,
        MR_FILE_MAX_LEN,
    );
    libc::string::memset(
        env,
        env.mythroad.state.start_file_parameter.cast_void(),
        0,
        MR_FILE_MAX_LEN,
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
    env: &mut Environment,
    code: u32,
    input: ConstPtr<u8>,
    input_len: u32,
    output: MutPtr<MutPtr<u8>>,
    output_len: MutPtr<i32>,
    _cb: MutVoidPtr,
) -> i32 {
    match code {
        MR_MALLOC_EX => {
            if output.is_null() || output_len.is_null() {
                return MrResult::Failed as i32;
            }

            let screen_buf = env.mythroad.state.mr_screen_buf.get(&env.mem).cast();
            let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
            let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
            let screen_len = screen_w
                .checked_mul(screen_h)
                .and_then(|pixels| pixels.checked_mul(guest_size_of::<u16>() as i32))
                .unwrap_or(0);

            env.mem.write(output, screen_buf);
            env.mem.write(output_len, screen_len);
            MrResult::Success as i32
        }
        MR_MFREE_EX => MrResult::Success as i32,
        MR_MALLOC_CACHE => {
            if !output.is_null() {
                env.mem.write(output, MutPtr::null());
            }
            MrResult::Success as i32
        }
        MR_MFREE_CACHE => MrResult::Success as i32,
        MR_MALLOC_SCRRAM | MR_FREE_SCRRAM => MrResult::Ignored as i32,
        MR_CHARACTER_HEIGHT => {
            if output.is_null() || output_len.is_null() {
                return MrResult::Failed as i32;
            }

            let word_info = env.mem.alloc_and_write(0x1008_1010i32);
            env.mem.write(output, word_info.cast());
            env.mem.write(output_len, guest_size_of::<i32>() as i32);
            MrResult::Success as i32
        }
        MR_SWITCHPATH => dsm_switch_path(env, input, input_len, output, output_len),
        MR_UCS2GB => mr_ucs2gb(env, input, input_len, output),
        MR_TURN_ON_BACKLIGHT | MR_TURN_OFF_BACKLIGHT => MrResult::Success as i32,
        _ => {
            log_dbg!("Mythroad: mr_platEx(code={code}, input={input:?}, input_len={input_len}) not implemented");
            MrResult::Ignored as i32
        }
    }
}

fn dsm_switch_path(
    env: &mut Environment,
    input: ConstPtr<u8>,
    _input_len: u32,
    output: MutPtr<MutPtr<u8>>,
    output_len: MutPtr<i32>,
) -> i32 {
    if input.is_null() {
        return MrResult::Failed as i32;
    }

    let input_bytes = env.mem.cstr_at(input);
    if input_bytes.len() > DSM_MAX_FILE_LEN - 3 {
        return MrResult::Failed as i32;
    }

    match input_bytes
        .first()
        .copied()
        .map(|ch| ch.to_ascii_lowercase())
    {
        Some(b'z') => {
            set_dsm_work_path(env, MYTHROAD);
        }
        Some(b'y') => {
            if output.is_null() || output_len.is_null() {
                return MrResult::Failed as i32;
            }

            let drive_path = dsm_drive_path_from_guest_path(env.fs.working_directory());
            let output_buf = env.mem.alloc_and_write_cstr(drive_path.as_bytes());
            env.mem.write(output, output_buf);
            env.mem.write(output_len, drive_path.len() as i32);
        }
        Some(b'x') => {
            let path = GuestPathBuf::from(format!(
                "{}/{}/{}/{}",
                MYTHROAD.as_str(),
                DSM_HIDE_DRIVE,
                DSM_DRIVE_A,
                DSM_ROOT_PATH_SYS
            ));
            set_dsm_work_path(env, &path);
        }
        _ => {
            let input_path = String::from_utf8_lossy(input_bytes);
            let path = dsm_guest_path_from_drive_path(&input_path);
            set_dsm_work_path(env, &path);
        }
    }

    MrResult::Success as i32
}

fn set_dsm_work_path<P: AsRef<GuestPath>>(env: &mut Environment, path: P) {
    let path = path.as_ref();
    match env.fs.change_working_directory(path) {
        Ok(new) => {
            log_dbg!("Change directory success, new working directory: {:?}", new,);
        }
        Err(()) => {
            log!(
                "Warning: change directory failed, could not change working directory to {:?}",
                path
            );
        }
    }
}

fn dsm_guest_path_from_drive_path(path: &str) -> GuestPathBuf {
    let drive = path.as_bytes().first().copied().unwrap_or(b'c');
    let rest = if path.len() > 3 { &path[3..] } else { "" };

    match drive.to_ascii_lowercase() {
        b'a' => GuestPathBuf::from(format!(
            "{}/{}/{}/{}",
            MYTHROAD.as_str(),
            DSM_HIDE_DRIVE,
            DSM_DRIVE_A,
            rest.trim_start_matches(['/', '\\'])
        )),
        b'b' => GuestPathBuf::from(format!(
            "{}/{}/{}/{}",
            MYTHROAD.as_str(),
            DSM_HIDE_DRIVE,
            DSM_DRIVE_B,
            rest.trim_start_matches(['/', '\\'])
        )),
        _ if rest.is_empty() => GuestPathBuf::from(MYTHROAD),
        _ => GuestPathBuf::from(format!(
            "{}/{}",
            MYTHROAD.as_str(),
            rest.trim_start_matches(['/', '\\'])
        )),
    }
}

fn dsm_drive_path_from_guest_path(path: &GuestPath) -> String {
    let path = path.as_str().trim_end_matches('/');
    let drive_a = format!("{}/{}/{}", MYTHROAD.as_str(), DSM_HIDE_DRIVE, DSM_DRIVE_A);
    let drive_b = format!("{}/{}/{}", MYTHROAD.as_str(), DSM_HIDE_DRIVE, DSM_DRIVE_B);

    if path == drive_a {
        "a:/".to_owned()
    } else if let Some(rest) = path.strip_prefix(&(drive_a + "/")) {
        format!("a:/{rest}")
    } else if path == drive_b {
        "b:/".to_owned()
    } else if let Some(rest) = path.strip_prefix(&(drive_b + "/")) {
        format!("b:/{rest}")
    } else if path == MYTHROAD.as_str() {
        "c:/".to_owned()
    } else if let Some(rest) = path.strip_prefix(&(MYTHROAD.as_str().to_owned() + "/")) {
        format!("c:/{rest}")
    } else {
        format!("c:/{}", path.trim_start_matches('/'))
    }
}

fn mr_ucs2gb(
    env: &mut Environment,
    input: ConstPtr<u8>,
    input_len: u32,
    output: MutPtr<MutPtr<u8>>,
) -> i32 {
    if input.is_null() || input_len == 0 {
        log_dbg!("Mythroad: mr_platEx(1207) input error");
        return MrResult::Failed as i32;
    }
    if output.is_null() {
        log_dbg!("Mythroad: mr_platEx(1207) output pointer error");
        return MrResult::Failed as i32;
    }

    let output_buf: MutPtr<u8> = env.mem.read(output);
    if output_buf.is_null() {
        log_dbg!("Mythroad: mr_platEx(1207) output buffer error");
        return MrResult::Failed as i32;
    }

    let mut utf16 = Vec::new();
    let mut offset = 0;
    while offset + 1 < input_len {
        let word = u16::from_be_bytes([
            env.mem.read(input + offset),
            env.mem.read(input + offset + 1),
        ]);
        if word == 0 {
            break;
        }
        utf16.push(word);
        offset += 2;
    }

    let text = String::from_utf16_lossy(&utf16);
    let (gb, _, _) = GBK.encode(&text);
    let copy_len: u32 = gb.len().try_into().unwrap_or(u32::MAX);
    env.mem
        .bytes_at_mut(output_buf, copy_len)
        .copy_from_slice(gb.as_ref());
    env.mem.write(output_buf + copy_len, b'\0');

    MrResult::Success as i32
}

pub(crate) fn mr_plat(env: &mut Environment, code: u32, param: u32) -> i32 {
    match code {
        MR_GET_FILE_POS => {
            let Some(fd) = mr_file_handle_to_posix_fd(param) else {
                return MrResult::Failed as i32;
            };

            let ret = posix_io::lseek(env, fd, 0, posix_io::SEEK_CUR);
            if ret >= 0 {
                i32::try_from(ret)
                    .ok()
                    .and_then(|ret| ret.checked_add(MrScreenType::Normal as i32))
                    .unwrap_or(MrResult::Failed as i32)
            } else {
                MrResult::Failed as i32
            }
        }
        MR_CONNECT => MrResult::Success as i32,
        MR_SET_SOCTIME => MrResult::Ignored as i32,
        MR_GET_RAND => {
            let Ok(limit) = i32::try_from(param) else {
                return MrResult::Failed as i32;
            };
            if limit <= 0 {
                return MrResult::Failed as i32;
            }

            crate::libc::stdlib::srand(env, mr_get_time(env));
            MrScreenType::Normal as i32 + crate::libc::stdlib::rand(env) % limit
        }
        MR_CHECK_TOUCH => MrScreenType::Normal as i32,
        MR_GET_HANDSET_LG => MrLanguage::Chinese as i32,
        MR_BACKGROUND_SUPPORT => MrResult::Ignored as i32,
        MR_SMS_CENTER => MrResult::Waiting as i32,
        MR_SIGNAL_INIT => MrResult::Success as i32,
        MR_SET_VOL => MrResult::Success as i32,
        MR_SET_KEY_END => MrResult::Success as i32,
        MR_WIFI_AVAILABLE => MrResult::Ignored as i32,
        MR_USE_WIFI => MrResult::Success as i32,
        MR_SMS_PROMPT => MrResult::Success as i32,
        MR_MSDC_STATUS => MrMsdcStatus::Ok as i32,
        _ => {
            log_dbg!("Mythroad: mr_plat(code={code}, param={param}) not implemented");
            MrResult::Ignored as i32
        }
    }
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

    posix_io::stat::mkdir(env, name, 0o777 as mode_t)
}

pub(crate) fn mr_rmdir(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    if name.is_null() {
        return MrResult::Failed as i32;
    }

    posix_io::stat::rmdir(env, name)
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

pub(crate) fn mr_find_start(
    env: &mut Environment,
    name: ConstPtr<u8>,
    buffer: MutPtr<u8>,
    len: u32,
) -> i32 {
    if name.is_null() || buffer.is_null() || len == 0 {
        return MrResult::Failed as i32;
    }

    let dir = libc::dirent::opendir(env, name);
    if dir.is_null() {
        return MrResult::Failed as i32;
    }

    libc::string::memset(env, buffer.cast_void(), 0, len);
    let dirent = libc::dirent::readdir(env, dir);
    if !dirent.is_null() {
        let dirent = env.mem.read(dirent);
        encoding::utf8_to_gb_string(
            env,
            &dirent.d_name[..usize::from(dirent.d_namlen)],
            buffer,
            len,
        );
    }

    dir.to_bits().try_into().unwrap_or(MrResult::Failed as i32)
}

pub(crate) fn mr_find_get_next(
    env: &mut Environment,
    search_handle: i32,
    buffer: MutPtr<u8>,
    len: u32,
) -> i32 {
    if search_handle == 0
        || search_handle == MrResult::Failed as i32
        || buffer.is_null()
        || len == 0
    {
        return MrResult::Failed as i32;
    }

    let dir = MutPtr::<libc::dirent::DIR>::from_bits(search_handle as u32);
    libc::string::memset(env, buffer.cast_void(), 0, len);
    let dirent = libc::dirent::readdir(env, dir);
    if dirent.is_null() {
        return MrResult::Failed as i32;
    }

    let dirent = env.mem.read(dirent);
    encoding::utf8_to_gb_string(
        env,
        &dirent.d_name[..usize::from(dirent.d_namlen)],
        buffer,
        len,
    );
    MrResult::Success as i32
}

pub(crate) fn mr_find_stop(env: &mut Environment, search_handle: i32) -> i32 {
    if search_handle == 0 || search_handle == MrResult::Failed as i32 {
        return MrResult::Failed as i32;
    }

    let dir = MutPtr::<libc::dirent::DIR>::from_bits(search_handle as u32);
    libc::dirent::closedir(env, dir);
    MrResult::Success as i32
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

pub(crate) fn test_com(env: &mut Environment, _l: u32, input0: u32, input1: u32) -> i32 {
    match input0 {
        0x01 => mr_get_time(env) as i32,
        0x02 => {
            env.mythroad.state.mr_event_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success as i32
        }
        0x03 => {
            env.mythroad.state.mr_timer_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success as i32
        }
        0x04 => {
            env.mythroad.state.mr_stop_function = GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success as i32
        }
        0x05 => {
            env.mythroad.state.mr_pause_app_function =
                GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success as i32
        }
        0x06 => {
            env.mythroad.state.mr_resume_app_function =
                GuestFunction::from_addr_with_thumb_bit(input1);
            MrResult::Success as i32
        }
        0x07 => input1 as i32,
        0x64 => env.mythroad.state.heap.mem_min.get(&env.mem) as i32,
        0x65 => env.mythroad.state.heap.mem_top.get(&env.mem) as i32,
        0x66 => env.mythroad.state.heap.mem_left.get(&env.mem),
        0xc8 => {
            let mr_state = env.mythroad.state.mr_state.get(&env.mem);
            let mr_shake_on = env.mythroad.state.mr_shake_on.get(&env.mem);
            if mr_state == MrRunState::Run as u32 && mr_shake_on != 0 {
                mr_start_shake(env, input1)
            } else {
                MrResult::Success as i32
            }
        }
        0x12c => {
            env.mythroad
                .state
                .mr_sound_on
                .set(&mut env.mem, input1 as i8);
            MrResult::Success as i32
        }
        0x12d => {
            env.mythroad
                .state
                .mr_shake_on
                .set(&mut env.mem, input1 as i8);
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x4);
            MrResult::Success as i32
        }
        0x12e => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x4);
            MrResult::Success as i32
        }
        0x12f => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x4);
            MrResult::Success as i32
        }
        0x130 => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x8);
            MrResult::Success as i32
        }
        0x131 => {
            env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x8);
            MrResult::Success as i32
        }
        0x132 => {
            env.mythroad.state.mr_sms_return_flag.set(&mut env.mem, 1);
            env.mythroad
                .state
                .mr_sms_return_val
                .set(&mut env.mem, input1);
            MrResult::Success as i32
        }
        0x133 => {
            env.mythroad.state.mr_sms_return_flag.set(&mut env.mem, 0);
            MrResult::Success as i32
        }
        0x190 => {
            mr_sleep(input1);
            MrResult::Success as i32
        }
        0x191 => {
            let old = env.mythroad.state.sysinfo.screen_width;
            env.mythroad.state.sysinfo.screen_width = input1 as i32;
            env.mythroad
                .state
                .mr_screen_w
                .set(&mut env.mem, input1 as i32);
            old
        }
        0x194 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_newSIMInd((int16)input1, NULL);
            MrResult::Success as i32
        }
        0x195 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            crate::mythroad::network::mr_close_network(env)
        }
        0x196 => {
            let old = env.mythroad.state.sysinfo.screen_height;
            env.mythroad.state.sysinfo.screen_height = input1 as i32;
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
            MrResult::Success as i32
        }
        0x198 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: implement this branch.
            MrResult::Success as i32
        }
        0x1f4 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = _mr_load_sms_cfg();
            MrResult::Success as i32
        }
        0x1f7 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: _mr_smsGetBytes(5, &dst, 1); ret = dst;
            MrResult::Success as i32
        }
        0x1f8 => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            // TODO: ret = _mr_save_sms_cfg(input1);
            MrResult::Success as i32
        }
        0xcb3 => {
            if input1 == 0x9e67a {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi & !0x2);
            }
            MrResult::Success as i32
        }
        0xe2d => {
            if input1 == 0xb61 {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x1);
            }
            MrResult::Success as i32
        }
        0xf51 => {
            if input1 == 0x18030 {
                env.mythroad.state.bi.update(&mut env.mem, |bi| bi | 0x2);
            }
            MrResult::Success as i32
        }
        _ => {
            log_dbg!("Mythroad: _mr_TestCom got unknown param: code={input0}");
            MrResult::Ignored as i32
        }
    }
}

pub(crate) fn test_com1(
    env: &mut Environment,
    _l: u32,
    input0: u32,
    input1: MutPtr<u8>,
    len: u32,
) -> i32 {
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
            MrResult::Success as i32
        }
        3 => {
            libc::string::memset(
                env,
                env.mythroad.state.old_pack_filename.cast_void(),
                0,
                MR_FILE_MAX_LEN,
            );
            if !input1.is_null() {
                libc::string::strncpy(
                    env,
                    env.mythroad.state.old_pack_filename,
                    input1.cast_const(),
                    MR_FILE_MAX_LEN - 1,
                );
            }

            libc::string::memset(
                env,
                env.mythroad.state.old_start_filename.cast_void(),
                0,
                MR_FILE_MAX_LEN,
            );
            let start_file_name = env
                .mem
                .alloc_and_write_cstr(mrp::START_FILE_NAME.as_bytes());
            libc::string::strncpy(
                env,
                env.mythroad.state.old_start_filename,
                start_file_name.cast_const(),
                MR_FILE_MAX_LEN - 1,
            );
            env.mem.free(start_file_name.cast_void());
            MrResult::Success as i32
        }
        4 => {
            libc::string::memset(
                env,
                env.mythroad.state.start_file_parameter.cast_void(),
                0,
                MR_FILE_MAX_LEN,
            );
            if !input1.is_null() {
                libc::string::strncpy(
                    env,
                    env.mythroad.state.start_file_parameter,
                    input1.cast_const(),
                    MR_FILE_MAX_LEN - 1,
                );
            }
            MrResult::Success as i32
        }
        7 | 8 | 9 => MrResult::Success as i32,
        _ => {
            log_dbg!("Mythroad: _mr_TestCom1 got unknown param: code={input0}");
            MrResult::Ignored as i32
        }
    }
}

pub(crate) fn mr_get_time(env: &Environment) -> u32 {
    env.startup_time.elapsed().as_millis() as u32
}

pub(crate) fn mr_timer(env: &mut Environment) -> i32 {
    if env.mythroad.state.mr_timer_state.get(&env.mem) != MrTimerState::Running as u32 {
        return MrResult::Ignored as i32;
    }

    let elapsed = mr_get_time(env).wrapping_sub(env.mythroad.state.mr_timer_start_time);
    if elapsed < env.mythroad.state.mr_timer_interval {
        return MrResult::Waiting as i32;
    }

    env.mythroad
        .state
        .mr_timer_state
        .set(&mut env.mem, MrTimerState::Idle as u32);

    let mr_state = env.mythroad.state.mr_state.get(&env.mem);
    let timer_runs_while_paused = env.mythroad.state.mr_timer_run_without_pause.get(&env.mem) != 0;

    if mr_state == MrRunState::Restart as u32 {
        let start_filename = env
            .mem
            .cstr_at_utf8(env.mythroad.state.start_filename.cast_const())
            .unwrap()
            .to_owned();
        mr_stop(env);
        intra_start(env, &start_filename, None);
        return MrResult::Success as i32;
    }

    if mr_state != MrRunState::Run as u32
        && !(timer_runs_while_paused && mr_state == MrRunState::Pause as u32)
    {
        return MrResult::Ignored as i32;
    }

    let timer_function = env.mythroad.state.mr_timer_function;
    if timer_function.addr_without_thumb_bit() == 0 {
        mr_test_com_c(env, 801, MutVoidPtr::null(), 1, 2);
        return MrResult::Success as i32;
    }

    let timer_status: i32 = timer_function.call_from_host(env, ());
    if timer_status != MrResult::Ignored as i32 {
        return timer_status;
    }

    mr_test_com_c(env, 801, MutVoidPtr::null(), 1, 2);
    MrResult::Success as i32
}

pub(crate) fn mr_event(env: &mut Environment, code: i32, param0: i32, param1: i32) -> i32 {
    log_dbg!("Mythroad: mr_event(code={code}, param0={param0}, param1={param1})");

    let mr_state = env.mythroad.state.mr_state.get(&env.mem);
    let timer_runs_while_paused = env.mythroad.state.mr_timer_run_without_pause.get(&env.mem) != 0;

    if mr_state != MrRunState::Run as u32
        && !(timer_runs_while_paused && mr_state == MrRunState::Pause as u32)
    {
        return MrResult::Ignored as i32;
    }

    let event_function = env.mythroad.state.mr_event_function;
    if event_function.addr_without_thumb_bit() != 0 {
        let event_status: i32 = event_function.call_from_host(env, (code, param0, param1));
        if event_status != MrResult::Ignored as i32 {
            return event_status;
        }
    }

    if env.mythroad.state.mr_c_function.addr_without_thumb_bit() == 0 {
        return MrResult::Ignored as i32;
    }

    let event_ptr: MutPtr<MrEvent> = env.mem.alloc(guest_size_of::<MrEvent>()).cast();
    env.mem.write(event_ptr, MrEvent::new(code, param0, param1));

    let result = mr_test_com_c(
        env,
        801,
        event_ptr.cast_void(),
        guest_size_of::<MrEvent>(),
        1,
    );
    env.mem.free(event_ptr.cast_void());
    result
}

fn mr_start_shake(_env: &mut Environment, ms: u32) -> i32 {
    log_dbg!("Mythroad: mr_startShake(ms={ms})");
    match haptics::start(ms as i32) {
        Ok(()) => MrResult::Success as i32,
        Err(err) => {
            log!("Mythroad: mr_startShake failed: {err}");
            MrResult::Failed as i32
        }
    }
}

pub(crate) fn mr_sleep(ms: u32) {
    std::thread::sleep(Duration::from_millis(ms.into()));
}

fn intra_start(env: &mut Environment, start_file_name: &str, entry: Option<&str>) -> i32 {
    if env.mythroad.state.heap.create(&mut env.mem).is_none() {
        env.mythroad
            .state
            .mr_state
            .set(&mut env.mem, MrRunState::Error as u32);
        return MrResult::Failed as i32;
    }

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
        .set(&mut env.mem, MrTimerState::Idle as u32);
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
            .set(&mut env.mem, MrRunState::Error as u32);
        return MrResult::Failed as i32;
    }
    reset_resource_tables(env);

    let entry = entry.unwrap_or("_dsm");
    libc::string::memset(
        env,
        env.mythroad.state.entry.cast_void(),
        0,
        MR_FILE_MAX_LEN,
    );

    let entry_src = env.mem.alloc_and_write_cstr(entry.as_bytes());
    libc::string::strncpy(
        env,
        env.mythroad.state.entry,
        entry_src.cast_const(),
        MR_FILE_MAX_LEN - 1,
    );
    env.mem.free(entry_src.cast_void());

    env.mythroad
        .state
        .mr_state
        .set(&mut env.mem, MrRunState::Run as u32);

    log_dbg!("Mythroad: mr_intra_start(filename={start_file_name}, entry={entry})");

    let mut ret = mr_do_ext(env, start_file_name);
    if ret != MrResult::Success as i32 {
        ret = mr_do_ext(env, mrp::LOGO_EXT_FILE_NAME);
    }

    if ret != MrResult::Success as i32 {
        env.mythroad
            .state
            .mr_state
            .set(&mut env.mem, MrRunState::Error as u32);
        mr_stop(env);
        return MrResult::Failed as i32;
    }

    MrResult::Success as i32
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

fn mr_do_ext(env: &mut Environment, filename: &str) -> i32 {
    log_dbg!("Mythroad: mr_doExt(filename={filename})");

    match env.executable.read_file(&env.mem, filename) {
        Ok(ext_data) if !ext_data.is_empty() => {
            let len = ext_data.len().try_into().unwrap();
            let addr = mr_malloc(env, len);
            env.mem
                .bytes_at_mut(addr.cast(), len)
                .copy_from_slice(&ext_data);

            test_com(env, 0, 0xe2d, 0xb61);

            if mr_test_com_c(env, 800, addr, len, 0) != MrResult::Success as i32 {
                mr_free(env, addr, len);
                return MrResult::Failed as i32;
            }

            mr_test_com_c(env, 801, addr, 0x7cd, 6);

            let app_info = env.mythroad.state.app_info;
            mr_test_com_c(env, 801, app_info, 16, 8);

            mr_test_com_c(env, 801, addr, 0x7cd, 0);

            MrResult::Success as i32
        }
        Ok(_) => {
            log_dbg!("Mythroad: mr_doExt failed: {filename} is empty");
            MrResult::Failed as i32
        }
        Err(err) => {
            log_dbg!("Mythroad: mr_doExt failed: {err}");
            MrResult::Failed as i32
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
) -> i32 {
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
        return mr_c_function_load.call_from_host(env, (code,));
    }

    let mr_c_function = env.mythroad.state.mr_c_function;
    let mr_c_function_p = env.mythroad.state.mr_c_function_p;

    mr_c_function.call_from_host(env, (mr_c_function_p, code, input, len))
}
