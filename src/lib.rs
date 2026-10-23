/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#[macro_use]
mod log;
mod abi;
#[cfg(target_os = "android")]
mod android;
mod audio;
mod cpu;
mod dsm;
mod editbox;
mod encoding;
mod environment;
mod font;
mod fs;
mod gzip;
mod haptics;
mod libc;
#[cfg(feature = "lua")]
mod lua;
mod md5;
mod mem;
mod mmio;
mod mrp;
mod mythroad;
mod options;
mod paths;
mod stack;
mod syscall;
mod window;

use environment::Environment;
#[cfg(target_os = "android")]
use std::ffi::CStr;
use std::path::PathBuf;

/// This is the true entry point on Android (SDLActivity calls it after
/// initialization). On other platforms the true entry point is in src/bin.rs.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn SDL_main(
    argc: std::ffi::c_int,
    argv: *const *const std::ffi::c_char,
) -> std::ffi::c_int {
    let args = if argc > 0 && !argv.is_null() {
        let raw_args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
        raw_args
            .iter()
            .filter_map(|arg| {
                if arg.is_null() {
                    None
                } else {
                    Some(
                        unsafe { CStr::from_ptr(*arg) }
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            })
            .collect()
    } else {
        vec![String::new()]
    };

    match main(args.into_iter()) {
        Ok(_) => echo!("skymrp finished"),
        Err(e) => echo!("skymrp error: {e:?}"),
    }
    return 0;
}

const USAGE: &str = "\
Usage:
    skymrp path/to/example.mrp

Options:
    --help
        Print this help text.
";

pub fn main<T: Iterator<Item = String>>(mut args: T) -> Result<(), String> {
    let _ = args.next().unwrap(); // skip argv[0]

    let mut mrp_path: Option<PathBuf> = None;
    let mut option_args = Vec::new();
    let mut options = options::Options::default();
    for arg in args {
        if arg == "--help" {
            echo!("{}", USAGE);
            echo!("{}", options::OPTIONS_HELP);
            return Ok(());
        } else if options.parse_argument(&arg)? {
            option_args.push(arg);
        } else if mrp_path.is_none() {
            mrp_path = Some(PathBuf::from(arg));
        } else {
            echo!("{}", USAGE);
            echo!("{}", options::OPTIONS_HELP);
            return Err(format!("Unexpected arguments: {:?}", arg));
        }
    }

    let mrp_path = if let Some(mrp_path) = mrp_path {
        mrp_path
    } else {
        paths::ensure_mythroad_dir()?;
        let cookie_path = paths::cookie_mrp_path()?;
        if cookie_path.is_file() {
            cookie_path
        } else {
            echo!("MRP file not found: {}", cookie_path.display());

            #[cfg(not(target_os = "android"))]
            let buttons = [
                sdl2::messagebox::ButtonData {
                    flags: sdl2::messagebox::MessageBoxButtonFlag::RETURNKEY_DEFAULT,
                    button_id: 0,
                    text: "Open Folder",
                },
                sdl2::messagebox::ButtonData {
                    flags: sdl2::messagebox::MessageBoxButtonFlag::ESCAPEKEY_DEFAULT,
                    button_id: 1,
                    text: "Close",
                },
            ];
            #[cfg(target_os = "android")]
            let buttons = [sdl2::messagebox::ButtonData {
                flags: sdl2::messagebox::MessageBoxButtonFlag::RETURNKEY_DEFAULT,
                button_id: 1,
                text: "Close",
            }];
            let clicked_button = sdl2::messagebox::show_message_box(
                sdl2::messagebox::MessageBoxFlag::WARNING,
                &buttons,
                "MRP File Not Found",
                "Place cookie.mrp in the mythroad folder.\n",
                None,
                None,
            )
            .map_err(|e| {
                format!(
                    "Message box for {} could not be shown: {e}",
                    cookie_path.display()
                )
            })?;

            #[cfg(not(target_os = "android"))]
            if matches!(
                clicked_button,
                sdl2::messagebox::ClickedButton::CustomButton(button) if button.button_id == 0
            ) {
                let url = paths::url_for_opening_user_data_dir()?;
                sdl2::url::open_url(&url)
                    .map_err(|e| format!("Could not open SkyMRP folder: {e}"))?;
            }
            #[cfg(target_os = "android")]
            let _ = clicked_button;
            return Ok(());
        }
    };

    for option_arg in option_args {
        let parse_result = options.parse_argument(&option_arg);
        assert!(parse_result == Ok(true));
    }

    let env = Environment::new(mrp_path, options.clone())?;
    env.run();
    Ok(())
}
