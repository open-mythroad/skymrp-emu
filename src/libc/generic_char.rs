/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::mem::{guest_size_of, ConstPtr, GuestUSize, MutPtr, Ptr, SafeRead};
use crate::Environment;
use std::cmp::Ordering;
use std::fmt::Debug;

pub(super) struct GenericChar<T> {
    _spooky: std::marker::PhantomData<T>,
}

impl<T: Copy + Default + Eq + Ord + SafeRead + Debug> GenericChar<T> {
    fn null() -> T {
        Default::default()
    }

    pub(super) fn memset(
        env: &mut Environment,
        dest: MutPtr<T>,
        ch: T,
        count: GuestUSize,
        dest_count: GuestUSize,
    ) -> MutPtr<T> {
        if count > dest_count {
            panic!("Buffer overflow!");
        }
        for i in 0..count {
            env.mem.write(dest + i, ch);
        }
        dest
    }

    pub(super) fn memcpy(
        env: &mut Environment,
        dest: MutPtr<T>,
        src: ConstPtr<T>,
        size: GuestUSize,
        dest_size: GuestUSize,
    ) -> MutPtr<T> {
        if size > dest_size {
            panic!("Buffer overflow!");
        }
        env.mem
            .memmove(dest.cast(), src.cast(), size * guest_size_of::<T>());
        dest
    }

    pub(super) fn memmove(
        env: &mut Environment,
        dest: MutPtr<T>,
        src: ConstPtr<T>,
        size: GuestUSize,
        dest_size: GuestUSize,
    ) -> MutPtr<T> {
        if size > dest_size {
            panic!("buffer overflow!");
        }
        env.mem
            .memmove(dest.cast(), src.cast(), size * guest_size_of::<T>());
        dest
    }

    pub(super) fn memcmp(
        env: &mut Environment,
        a: ConstPtr<T>,
        b: ConstPtr<T>,
        n: GuestUSize,
    ) -> i32 {
        let mut offset = 0;
        while offset < n {
            let char_a = env.mem.read(a + offset);
            let char_b = env.mem.read(b + offset);
            offset += 1;

            match char_a.cmp(&char_b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => continue,
            }
        }
        0
    }

    pub(super) fn memchr(
        env: &mut Environment,
        string: ConstPtr<T>,
        c: T,
        size: GuestUSize,
    ) -> ConstPtr<T> {
        for i in 0..size {
            if env.mem.read(string + i) == c {
                return string + i;
            }
        }
        Ptr::null()
    }

    pub(super) fn strlen(env: &mut Environment, s: ConstPtr<T>) -> GuestUSize {
        let mut i = 0;
        while env.mem.read(s + i) != Self::null() {
            i += 1;
        }
        i
    }

    pub(super) fn strcpy(
        env: &mut Environment,
        dest: MutPtr<T>,
        src: ConstPtr<T>,
        mut bufsz: GuestUSize,
    ) -> MutPtr<T> {
        {
            let (mut dest, mut src) = (dest, src);
            loop {
                if bufsz == 0 {
                    panic!("Buffer overrun");
                }
                let c = env.mem.read(src);
                env.mem.write(dest, c);
                if c == Self::null() {
                    break;
                }
                dest += 1;
                src += 1;
                bufsz -= 1;
            }
        }
        dest
    }
    pub(super) fn strcat(
        env: &mut Environment,
        dest: MutPtr<T>,
        src: ConstPtr<T>,
        bufsz: GuestUSize,
    ) -> MutPtr<T> {
        {
            let dest_len = Self::strlen(env, dest.cast_const());
            let dest = dest + dest_len;
            let remaining = bufsz.checked_sub(dest_len).expect("strcat overflowed");
            Self::strcpy(env, dest, src, remaining);
        }
        dest
    }

    pub(super) fn strncpy(
        env: &mut Environment,
        dest: MutPtr<T>,
        src: ConstPtr<T>,
        size: GuestUSize,
        dest_size: GuestUSize,
    ) -> MutPtr<T> {
        if dest_size < size {
            panic!("Buffer overflow");
        }
        let mut end = false;
        for i in 0..size {
            if !end {
                let c = env.mem.read(src + i);
                if c == Self::null() {
                    end = true;
                }
                env.mem.write(dest + i, c);
            } else {
                env.mem.write(dest + i, Self::null());
            }
        }
        dest
    }

    pub(super) fn strcmp(env: &mut Environment, a: ConstPtr<T>, b: ConstPtr<T>) -> i32 {
        let mut offset = 0;
        loop {
            let char_a = env.mem.read(a + offset);
            let char_b = env.mem.read(b + offset);
            offset += 1;

            match char_a.cmp(&char_b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => {
                    if char_a == Self::null() {
                        return 0;
                    } else {
                        continue;
                    }
                }
            }
        }
    }

    pub(super) fn strncmp(
        env: &mut Environment,
        a: ConstPtr<T>,
        b: ConstPtr<T>,
        n: GuestUSize,
    ) -> i32 {
        if n == 0 {
            return 0;
        }

        let mut offset = 0;
        loop {
            let char_a = env.mem.read(a + offset);
            let char_b = env.mem.read(b + offset);
            offset += 1;

            match char_a.cmp(&char_b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => {
                    if offset == n || char_a == Self::null() {
                        return 0;
                    } else {
                        continue;
                    }
                }
            }
        }
    }

    pub(super) fn strncat(
        env: &mut Environment,
        s1: MutPtr<T>,
        s2: ConstPtr<T>,
        n: GuestUSize,
    ) -> MutPtr<T> {
        let s1end = s1 + Self::strlen(env, s1.cast_const());
        for i in 0..n {
            let c = env.mem.read(s2 + i);
            env.mem.write(s1end + i, c);
            if c == Self::null() {
                return s1;
            }
        }
        env.mem.write(s1end + n, Self::null());
        s1
    }

    pub(super) fn strstr(
        env: &mut Environment,
        string: ConstPtr<T>,
        substring: ConstPtr<T>,
    ) -> ConstPtr<T> {
        let mut offset = 0;
        loop {
            let mut inner_offset = 0;
            loop {
                let char_string = env.mem.read(string + offset + inner_offset);
                let char_substring = env.mem.read(substring + inner_offset);
                if char_substring == Self::null() {
                    return string + offset;
                } else if char_string == Self::null() {
                    return Ptr::null();
                } else if char_string != char_substring {
                    break;
                } else {
                    inner_offset += 1;
                }
            }
            offset += 1;
        }
    }
}
