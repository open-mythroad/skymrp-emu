/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::{DotDotDot, VaList};
use crate::libc::stdlib::str_to_int_inner_generic;
use crate::mem::{ConstPtr, MutPtr};
use crate::Environment;
use std::io::Write;

const INTEGER_SPECIFIERS: [u8; 6] = [b'd', b'i', b'o', b'u', b'x', b'X'];
const FLOAT_SPECIFIERS: [u8; 1] = [b'f'];

fn printf_inner(env: &mut Environment, format: ConstPtr<u8>, mut args: VaList) -> Vec<u8> {
    log_dbg!(
        "Processing format string {:?}",
        env.mem.cstr_at_utf8(format)
    );

    let mut res = Vec::<u8>::new();

    let mut current_format = format;

    loop {
        let c = env.mem.read(current_format);
        current_format += 1;

        if c == b'\0' {
            break;
        }
        if c != b'%' {
            res.push(c);
            continue;
        }

        let pad_char = if env.mem.read(current_format) == b'0' {
            current_format += 1;
            '0'
        } else {
            ' '
        };
        let pad_width = {
            let mut pad_width = 0;
            while let c @ b'0'..=b'9' = env.mem.read(current_format) {
                pad_width = pad_width * 10 + (c - b'0') as usize;
                current_format += 1;
            }
            pad_width
        };

        let precision = if env.mem.read(current_format) == b'.' {
            current_format += 1;
            let mut precision: usize = 0;
            while let c @ b'0'..=b'9' = env.mem.read(current_format) {
                precision = precision * 10 + (c - b'0') as usize;
                current_format += 1;
            }
            Some(precision)
        } else {
            None
        };

        let specifier = env.mem.read(current_format);
        current_format += 1;

        assert!(specifier != b'\0');
        if specifier == b'%' {
            res.push(b'%');
            continue;
        }

        if precision.is_some() {
            assert!(
                INTEGER_SPECIFIERS.contains(&specifier) || FLOAT_SPECIFIERS.contains(&specifier)
            )
        }

        match specifier {
            b'c' => {
                let c: u8 = args.next(env);
                assert!(pad_char == ' ' && pad_width == 0); // TODO
                res.push(c);
            }
            b's' => {
                let c_string: ConstPtr<u8> = args.next(env);
                assert!(pad_char == ' ' && pad_width == 0); // TODO
                res.extend_from_slice(env.mem.cstr_at(c_string));
            }
            b'd' | b'i' | b'u' => {
                let int: i64 = if specifier == b'u' {
                    let uint: u32 = args.next(env);
                    uint.into()
                } else {
                    let int: i32 = args.next(env);
                    int.into()
                };

                let int_with_precision = if precision.is_some_and(|value| value > 0) {
                    format!("{:01$}", int, precision.unwrap())
                } else {
                    format!("{}", int)
                };

                if pad_width > 0 {
                    if pad_char == '0' && precision.is_none() {
                        write!(&mut res, "{:0>1$}", int_with_precision, pad_width).unwrap();
                    } else {
                        write!(&mut res, "{:>1$}", int_with_precision, pad_width).unwrap();
                    }
                } else {
                    res.extend_from_slice(int_with_precision.as_bytes());
                }
            }
            b'x' => {
                let uint: u32 = args.next(env);
                if pad_width > 0 {
                    assert!(precision.is_none()); // TODO
                    let pad_width = pad_width as usize;
                    if pad_char == '0' && precision.is_none() {
                        write!(&mut res, "{uint:0>pad_width$x}").unwrap();
                    } else {
                        write!(&mut res, "{uint:>pad_width$x}").unwrap();
                    }
                } else {
                    let tmp = if precision.is_some_and(|value| value > 0) {
                        format!("{:01$x}", uint, precision.unwrap())
                    } else {
                        if let Some(precision) = precision {
                            assert!(precision == 0 && uint != 0); // TODO
                        }
                        format!("{uint:x}")
                    };
                    res.extend_from_slice(tmp.as_bytes());
                }
            }
            // TODO: more specifiers
            _ => unimplemented!("Format character '{}'", specifier as char),
        }
    }

    log_dbg!("=> {:?}", std::str::from_utf8(&res));

    res
}

pub(crate) fn sprintf(
    env: &mut Environment,
    dest: MutPtr<u8>,
    format: ConstPtr<u8>,
    args: DotDotDot,
) -> i32 {
    let res = printf_inner(env, format, args.start());

    log_dbg!("sprintf({:?}, {:?}, ...)", dest, format);

    let dest_slice = env
        .mem
        .bytes_at_mut(dest, (res.len() + 1).try_into().unwrap());
    for (i, &byte) in res.iter().chain(b"\0".iter()).enumerate() {
        dest_slice[i] = byte;
    }

    res.len().try_into().unwrap()
}

pub(crate) fn printf(env: &mut Environment, format: ConstPtr<u8>, args: DotDotDot) -> i32 {
    let res = printf_inner(env, format, args.start());
    // TODO: I/O error handling
    let _ = std::io::stdout().write_all(&res);
    res.len().try_into().unwrap()
}
