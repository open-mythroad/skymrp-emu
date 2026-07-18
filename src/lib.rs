/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#[macro_use]
mod log;
mod abi;
mod audio;
mod cpu;
mod dsm;
mod encoding;
mod environment;
mod font;
mod fs;
mod gzip;
mod haptics;
mod libc;
mod md5;
mod mem;
mod mrp;
mod mythroad;
mod options;
mod stack;
mod syscall;
mod window;

use environment::Environment;
use std::path::PathBuf;

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
            log_dbg!("{}", USAGE);
            return Ok(());
        } else if options.parse_argument(&arg)? {
            option_args.push(arg);
        } else if mrp_path.is_none() {
            mrp_path = Some(PathBuf::from(arg));
        } else {
            log!("{}", USAGE);
            log!("{}", options::OPTIONS_HELP);
            return Err(format!("Unexpected arguments: {:?}", arg));
        }
    }

    let mrp_path = if let Some(mrp_path) = mrp_path {
        mrp_path
    } else {
        log!("No app specified, Use the --help flag to see command-line usage.");
        return Err("Path to mrp must be specified".to_string());
    };

    for option_arg in option_args {
        let parse_result = options.parse_argument(&option_arg);
        assert!(parse_result == Ok(true));
    }

    let mut env = Environment::new(mrp_path, options.clone())?;
    env.run();
    Ok(())
}
