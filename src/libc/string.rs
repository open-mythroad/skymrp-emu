/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::generic_char::GenericChar;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::Environment;

pub(crate) fn memset(
    env: &mut Environment,
    dest: MutVoidPtr,
    ch: i32,
    count: GuestUSize,
) -> MutVoidPtr {
    GenericChar::<u8>::memset(env, dest.cast(), ch as u8, count, GuestUSize::MAX).cast()
}

pub(crate) fn memcpy(
    env: &mut Environment,
    dest: MutVoidPtr,
    src: ConstVoidPtr,
    size: GuestUSize,
) -> MutVoidPtr {
    GenericChar::<u8>::memcpy(env, dest.cast(), src.cast(), size, GuestUSize::MAX).cast()
}

pub(crate) fn memmove(
    env: &mut Environment,
    dest: MutVoidPtr,
    src: ConstVoidPtr,
    size: GuestUSize,
) -> MutVoidPtr {
    GenericChar::<u8>::memmove(env, dest.cast(), src.cast(), size, GuestUSize::MAX).cast()
}

pub(crate) fn strlen(env: &mut Environment, s: ConstPtr<u8>) -> GuestUSize {
    GenericChar::<u8>::strlen(env, s)
}

pub(crate) fn strcpy(env: &mut Environment, dest: MutPtr<u8>, src: ConstPtr<u8>) -> MutPtr<u8> {
    GenericChar::<u8>::strcpy(env, dest, src, GuestUSize::MAX)
}

pub(crate) fn strncpy(
    env: &mut Environment,
    dest: MutPtr<u8>,
    src: ConstPtr<u8>,
    size: GuestUSize,
) -> MutPtr<u8> {
    GenericChar::<u8>::strncpy(env, dest, src, size, GuestUSize::MAX)
}

pub(crate) fn strcat(env: &mut Environment, dest: MutPtr<u8>, src: ConstPtr<u8>) -> MutPtr<u8> {
    GenericChar::<u8>::strcat(env, dest, src, GuestUSize::MAX)
}

pub(crate) fn strncat(
    env: &mut Environment,
    s1: MutPtr<u8>,
    s2: ConstPtr<u8>,
    n: GuestUSize,
) -> MutPtr<u8> {
    GenericChar::<u8>::strncat(env, s1, s2, n)
}

pub(crate) fn memcmp(
    env: &mut Environment,
    a: ConstVoidPtr,
    b: ConstVoidPtr,
    size: GuestUSize,
) -> i32 {
    GenericChar::<u8>::memcmp(env, a.cast(), b.cast(), size)
}

pub(crate) fn strcmp(env: &mut Environment, a: ConstPtr<u8>, b: ConstPtr<u8>) -> i32 {
    GenericChar::<u8>::strcmp(env, a, b)
}

pub(crate) fn strncmp(
    env: &mut Environment,
    a: ConstPtr<u8>,
    b: ConstPtr<u8>,
    n: GuestUSize,
) -> i32 {
    GenericChar::<u8>::strncmp(env, a, b, n)
}

pub(crate) fn strcoll(env: &mut Environment, a: ConstPtr<u8>, b: ConstPtr<u8>) -> i32 {
    strcmp(env, a, b)
}

pub(crate) fn memchr(
    env: &mut Environment,
    string: ConstVoidPtr,
    c: u32,
    size: GuestUSize,
) -> ConstVoidPtr {
    GenericChar::<u8>::memchr(env, string.cast(), c as u8, size).cast()
}

pub(crate) fn strstr(
    env: &mut Environment,
    haystack: ConstPtr<u8>,
    needle: ConstPtr<u8>,
) -> ConstPtr<u8> {
    GenericChar::<u8>::strstr(env, haystack, needle)
}
