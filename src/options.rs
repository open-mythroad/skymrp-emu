/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::cpu::CpuBackend;
use crate::font::FontSize;
use std::num::NonZeroU32;

pub const OPTIONS_HELP: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/OPTIONS_HELP.txt"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    Mini,
    Full,
}

#[derive(Clone)]
pub struct Options {
    pub fullscreen: bool,
    pub scale: NonZeroU32,
    pub screen_size: (u32, u32),
    pub font_size_override: Option<FontSize>,
    pub cpu_backend: CpuBackend,
    pub runtime_mode: RuntimeMode,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scale: NonZeroU32::new(1).unwrap(),
            fullscreen: false,
            screen_size: (240, 320),
            font_size_override: Some(FontSize::Medium),
            runtime_mode: RuntimeMode::Mini,
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            cpu_backend: CpuBackend::Dynarmic,
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            cpu_backend: CpuBackend::Interpreter,
        }
    }
}

impl Options {
    pub fn parse_argument(&mut self, arg: &str) -> Result<bool, String> {
        if arg == "--fullscreen" {
            self.fullscreen = true;
        } else if let Some(value) = arg.strip_prefix("--scale=") {
            self.scale = value
                .parse()
                .map_err(|_| "Invalid scale hack factor".to_string())?;
        } else if let Some(value) = arg.strip_prefix("--screen-size=") {
            let (width, height) = value
                .split_once('x')
                .ok_or_else(|| "Screen size must use WIDTHxHEIGHT format".to_string())?;
            let width: u32 = width
                .parse()
                .map_err(|_| "Invalid screen width".to_string())?;
            let height: u32 = height
                .parse()
                .map_err(|_| "Invalid screen height".to_string())?;
            if width == 0 || height == 0 {
                return Err("Screen dimensions must be greater than zero".to_string());
            }
            if width > i16::MAX as u32 || height > i16::MAX as u32 {
                return Err(format!("Screen dimensions must not exceed {}", i16::MAX));
            }
            self.screen_size = (width, height);
        } else if let Some(value) = arg.strip_prefix("--font-size-override=") {
            self.font_size_override = Some(FontSize::parse(value)?);
        } else if let Some(value) = arg.strip_prefix("--cpu=") {
            self.cpu_backend = match value {
                "dynarmic" => {
                    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
                    {
                        CpuBackend::Dynarmic
                    }
                    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
                    {
                        return Err(
                            "Dynarmic CPU backend is unavailable on this architecture".to_string()
                        );
                    }
                }
                "interpreter" => CpuBackend::Interpreter,
                _ => return Err(format!("Invalid CPU backend: {value}")),
            };
        } else if let Some(value) = arg.strip_prefix("--runtime=") {
            self.runtime_mode = match value {
                "mini" => RuntimeMode::Mini,
                "full" if cfg!(feature = "lua") => RuntimeMode::Full,
                "full" => return Err("This build only supports the mini runtime".to_owned()),
                _ => return Err(format!("Invalid runtime mode: {value}")),
            };
        } else {
            return Ok(false);
        }
        Ok(true)
    }
}
