/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::{DotDotDot, VaList};
use crate::encoding;
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr};
use crate::Environment;
use std::io::Write;

const INTEGER_SPECIFIERS: [u8; 6] = [b'd', b'i', b'o', b'u', b'x', b'X'];
const FLOAT_SPECIFIERS: [u8; 3] = [b'f', b'e', b'g'];

fn printf_inner(env: &mut Environment, format: ConstPtr<u8>, mut args: VaList) -> Vec<u8> {
    log_dbg!(
        "Processing format string {:?}",
        encoding::gb_to_utf8_string(env.mem.cstr_at(format))
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

        let length_modifier = match env.mem.read(current_format) {
            b'l' => {
                current_format += 1;
                if env.mem.read(current_format) == b'l' {
                    current_format += 1;
                    Some("ll")
                } else {
                    Some("l")
                }
            }
            _ => None,
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
                // TODO: support length modifier
                assert!(length_modifier.is_none());
                let c: u8 = args.next(env);
                assert!(pad_char == ' ' && pad_width == 0); // TODO
                res.push(c);
            }
            b's' => {
                // TODO: support length modifier
                assert!(length_modifier.is_none());
                let c_string: ConstPtr<u8> = args.next(env);
                assert!(pad_char == ' ' && pad_width == 0); // TODO
                res.extend_from_slice(env.mem.cstr_at(c_string));
            }
            b'd' | b'i' | b'u' => {
                // Note: on 32-bit system int and long are i32,
                // so length_modifier is ignored
                let int: i64 = if specifier == b'u' {
                    if length_modifier == Some("ll") {
                        let uint: u64 = args.next(env);
                        uint.try_into().unwrap()
                    } else {
                        assert!(length_modifier.is_none() || length_modifier == Some("l"));
                        let uint: u32 = args.next(env);
                        uint.into()
                    }
                } else if length_modifier == Some("ll") {
                    args.next(env)
                } else {
                    assert!(length_modifier.is_none() || length_modifier == Some("l"));
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
                // Note: on 32-bit system unsigned int and unsigned long
                // are u32, so length_modifier is ignored
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
            b'p' => {
                assert!(length_modifier.is_none());
                let ptr: MutVoidPtr = args.next(env);
                let tmp = format!("{:#x}", ptr.to_bits());
                if pad_width > 0 {
                    let pad_width = pad_width as usize;
                    assert!(pad_char == ' '); // TODO
                    write!(&mut res, "{tmp:>pad_width$}").unwrap();
                } else {
                    res.extend_from_slice(tmp.as_bytes());
                }
            }
            // Float specifiers
            b'f' => {
                let float: f64 = args.next(env);
                let pad_width = pad_width as usize;
                let precision = precision.unwrap_or(6);

                let formatted = f_format(float, pad_width, pad_char, precision);
                res.extend_from_slice(formatted.as_bytes());
            }
            b'e' => {
                let float: f64 = args.next(env);
                let pad_width = pad_width as usize;
                let precision = precision.unwrap_or(6);

                let formatted = e_format(float, pad_width, pad_char, precision);
                res.extend_from_slice(formatted.as_bytes());
            }
            b'g' => {
                let float: f64 = args.next(env);
                let pad_width = pad_width as usize;

                // Reference https://en.cppreference.com/w/c/io/vfprintf
                let P: i32 = if let Some(precision) = precision {
                    if precision == 0 {
                        1
                    } else {
                        precision.try_into().unwrap()
                    }
                } else {
                    6
                };
                let X: i32 = if float == 0.0 {
                    0
                } else {
                    float.abs().log10().floor() as i32
                };
                log_dbg!(
                    "float {}, pad_width {}, pad_char '{}', P {}, X {}",
                    float,
                    pad_width,
                    pad_char,
                    P,
                    X
                );
                if P > X && X >= -4 {
                    let precision: usize = (P - X - 1).try_into().unwrap();

                    let result = f_format(float, pad_width, pad_char, precision);

                    // TODO: skip if alternative representation is requested
                    let trimmed_result = if result.contains('.') {
                        result.trim_end_matches('0').trim_end_matches('.')
                    } else {
                        &result
                    };

                    let trimmed_result = if pad_width > 0 && trimmed_result.len() < pad_width {
                        if pad_char == '0' {
                            format!("{trimmed_result:0>pad_width$}")
                        } else {
                            format!("{trimmed_result:>pad_width$}")
                        }
                    } else {
                        trimmed_result.to_string()
                    };

                    res.extend_from_slice(trimmed_result.as_bytes());
                } else {
                    let precision: usize = (P - 1).try_into().unwrap();

                    let formatted = e_format(float, pad_width, pad_char, precision);
                    res.extend_from_slice(formatted.as_bytes());
                }
            }
            // TODO: more specifiers
            _ => unimplemented!("Format character '{}'", specifier as char),
        }
    }

    log_dbg!("=> {:?}", encoding::gb_to_utf8_string(&res));

    res
}

fn f_format(float: f64, pad_width: usize, pad_char: char, precision: usize) -> String {
    if pad_char == '0' {
        format!("{float:0pad_width$.precision$}")
    } else {
        assert!(pad_char == ' '); // TODO
        format!("{float:pad_width$.precision$}")
    }
}

fn e_format(float: f64, pad_width: usize, pad_char: char, precision: usize) -> String {
    let exponent = if float == 0.0 {
        0.0
    } else {
        float.abs().log10().floor()
    };
    let mantissa = float.abs() / 10f64.powf(exponent);
    let sign = if float.is_sign_negative() { "-" } else { "" };
    if pad_char == '0' {
        let float_exp_notation = format!("{mantissa:.precision$}e{exponent:+03}");
        format!(
            "{0}{1:0>2$}",
            sign,
            float_exp_notation,
            pad_width.saturating_sub(sign.len())
        )
    } else {
        assert!(pad_char == ' '); // TODO
        let float_exp_notation = format!("{sign}{mantissa:.precision$}e{exponent:+03}");
        format!("{float_exp_notation:>pad_width$}")
    }
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
    let text = encoding::gb_to_utf8_string(&res);
    let _ = std::io::stdout().write_all(text.as_bytes());
    res.len().try_into().unwrap()
}
