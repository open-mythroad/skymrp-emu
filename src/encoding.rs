use encoding_rs::GBK;

use crate::mem::{ConstPtr, MutPtr};
use crate::mythroad::{mr_free, mr_malloc};
use crate::Environment;

pub(crate) struct C2uResult {
    pub ptr: MutPtr<u16>,
    pub size: u32,
    pub err: i32,
}

pub(crate) fn c2u(env: &mut Environment, cp: ConstPtr<u8>, fail_on_error: bool) -> C2uResult {
    let mut ch_count = 0u32;
    let mut i = 0u32;
    while env.mem.read::<u8, false>(cp + i) != 0 {
        let ch: u8 = env.mem.read(cp + i);
        let next: u8 = env.mem.read(cp + i + 1);
        if (0xa1..=0xfe).contains(&ch) && next != 0 {
            i += 1;
        }
        ch_count += 1;
        i += 1;
    }

    let uni_size = 2 * ch_count + 2;

    let uni_buf: MutPtr<u16> = mr_malloc(env, uni_size).cast();
    if uni_buf.is_null() {
        return C2uResult {
            ptr: uni_buf,
            size: uni_size,
            err: -1,
        };
    }

    ch_count = 0;
    i = 0;
    while env.mem.read::<u8, false>(cp + i) != 0 {
        let ch: u8 = env.mem.read(cp + i);
        if ch < 0x80 {
            env.mem
                .write(uni_buf + ch_count, unicode_to_guest_word(u16::from(ch)));
            ch_count += 1;
            i += 1;
        } else if let Some(unicode) = gb_code_to_unicode(env, cp + i) {
            env.mem.write(uni_buf + ch_count, unicode);
            ch_count += 1;
            i += 2;
        } else if fail_on_error {
            let err = i as i32;
            mr_free(env, uni_buf.cast_void(), uni_size);
            return C2uResult {
                ptr: MutPtr::null(),
                size: uni_size,
                err,
            };
        } else {
            env.mem
                .write(uni_buf + ch_count, unicode_to_guest_word(0x3000));
            ch_count += 1;
            i += 2;
        }
    }

    env.mem.write(uni_buf + ch_count, 0u16);
    C2uResult {
        ptr: uni_buf,
        size: uni_size,
        err: -1,
    }
}

pub(crate) fn mr_c2u(
    env: &mut Environment,
    cp: ConstPtr<u8>,
    err: MutPtr<i32>,
    size: MutPtr<i32>,
) -> MutPtr<u16> {
    let result = c2u(env, cp, !err.is_null());

    if !err.is_null() {
        env.mem.write(err, result.err);
    }
    if !size.is_null() {
        env.mem.write(size, result.size as i32);
    }

    result.ptr
}

fn gb_code_to_unicode(env: &Environment, cp: ConstPtr<u8>) -> Option<u16> {
    let bytes = [env.mem.read(cp), env.mem.read(cp + 1)];
    let (text, had_errors) = GBK.decode_without_bom_handling(&bytes);
    if had_errors {
        return None;
    }

    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    let code = u32::from(ch);
    if code > u32::from(u16::MAX) {
        return None;
    }
    Some(unicode_to_guest_word(code as u16))
}

fn unicode_to_guest_word(ch: u16) -> u16 {
    ch.to_be()
}
