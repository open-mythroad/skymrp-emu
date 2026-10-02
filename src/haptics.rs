/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::cell::RefCell;
use std::ffi::CStr;
use std::fmt;
use std::ptr::NonNull;

#[derive(Debug)]
pub enum Error {
    NotInitialized,
    Sdl(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "haptics system is not initialized"),
            Self::Sdl(err) => write!(f, "SDL haptics error: {err}"),
        }
    }
}

impl std::error::Error for Error {}

thread_local! {
    static HAPTICS_SYSTEM: RefCell<Option<HapticsSystem>> = const { RefCell::new(None) };
}

pub fn init(sdl: &sdl2::Sdl) -> Result<(), Error> {
    HAPTICS_SYSTEM.with(|cell| {
        if cell.borrow().is_some() {
            return Ok(());
        }

        *cell.borrow_mut() = Some(HapticsSystem::new(sdl));
        Ok(())
    })
}

pub fn start(ms: i32) -> Result<(), Error> {
    with_system(|system| system.start(ms))
}

pub fn stop() -> Result<(), Error> {
    with_system(|system| system.stop())
}

fn with_system<F>(f: F) -> Result<(), Error>
where
    F: FnOnce(&mut HapticsSystem) -> Result<(), Error>,
{
    HAPTICS_SYSTEM.with(|cell| {
        let mut system = cell.borrow_mut();
        let Some(system) = system.as_mut() else {
            return Err(Error::NotInitialized);
        };
        f(system)
    })
}

struct HapticsSystem {
    backend: HapticsBackend,
}

impl HapticsSystem {
    fn new(sdl: &sdl2::Sdl) -> Self {
        Self {
            backend: HapticsBackend::new(sdl),
        }
    }

    fn start(&mut self, ms: i32) -> Result<(), Error> {
        self.backend.start(ms)
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.backend.stop()
    }
}

enum HapticsBackend {
    Sdl(SdlHapticsBackend),
    Noop,
}

impl HapticsBackend {
    fn new(sdl: &sdl2::Sdl) -> Self {
        match SdlHapticsBackend::new(sdl) {
            Ok(backend) => Self::Sdl(backend),
            Err(err) => {
                log_dbg!("Haptics: {err}, using no-op backend");
                Self::Noop
            }
        }
    }

    fn start(&mut self, ms: i32) -> Result<(), Error> {
        match self {
            Self::Sdl(backend) => backend.start(ms),
            Self::Noop => {
                log_dbg!("Haptics: start requested for {ms} ms, no backend attached");
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        match self {
            Self::Sdl(backend) => backend.stop(),
            Self::Noop => {
                log_dbg!("Haptics: stop requested, no backend attached");
                Ok(())
            }
        }
    }
}

struct SdlHapticsBackend {
    _subsystem: sdl2::HapticSubsystem,
    device: NonNull<sdl2_sys::SDL_Haptic>,
}

impl SdlHapticsBackend {
    fn new(sdl: &sdl2::Sdl) -> Result<Self, String> {
        let subsystem = sdl.haptic()?;
        let device_count = unsafe { sdl2_sys::SDL_NumHaptics() };
        if device_count <= 0 {
            return Err("no haptic devices found".to_owned());
        }

        let device_index = preferred_device_index(device_count);
        let device = NonNull::new(unsafe { sdl2_sys::SDL_HapticOpen(device_index) })
            .ok_or_else(|| sdl_error("could not open haptic device"))?;

        if unsafe { sdl2_sys::SDL_HapticRumbleInit(device.as_ptr()) } != 0 {
            unsafe { sdl2_sys::SDL_HapticClose(device.as_ptr()) };
            return Err(sdl_error("could not initialize haptic rumble"));
        }

        Ok(Self {
            _subsystem: subsystem,
            device,
        })
    }

    fn start(&mut self, ms: i32) -> Result<(), Error> {
        if ms <= 0 {
            return self.stop();
        }

        let result =
            unsafe { sdl2_sys::SDL_HapticRumblePlay(self.device.as_ptr(), 1.0, ms as u32) };
        if result == 0 {
            Ok(())
        } else {
            Err(Error::Sdl(sdl_error("could not start haptic rumble")))
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        if unsafe { sdl2_sys::SDL_HapticRumbleStop(self.device.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(Error::Sdl(sdl_error("could not stop haptic rumble")))
        }
    }
}

impl Drop for SdlHapticsBackend {
    fn drop(&mut self) {
        unsafe {
            sdl2_sys::SDL_HapticRumbleStop(self.device.as_ptr());
            sdl2_sys::SDL_HapticClose(self.device.as_ptr());
        }
    }
}

fn preferred_device_index(device_count: i32) -> i32 {
    for index in 0..device_count {
        let name = unsafe { sdl2_sys::SDL_HapticName(index) };
        if name.is_null() {
            continue;
        }

        let name = unsafe { CStr::from_ptr(name) };
        if name.to_bytes() == b"VIBRATOR_SERVICE" {
            return index;
        }
    }
    0
}

fn sdl_error(context: &str) -> String {
    format!("{context}: {}", sdl2::get_error())
}
