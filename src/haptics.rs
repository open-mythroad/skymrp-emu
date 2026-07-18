/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::cell::RefCell;
use std::fmt;

#[derive(Debug)]
pub enum Error {
    NotInitialized,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "haptics system is not initialized"),
        }
    }
}

impl std::error::Error for Error {}

thread_local! {
    static HAPTICS_SYSTEM: RefCell<Option<HapticsSystem>> = const { RefCell::new(None) };
}

pub fn init(_sdl: &sdl2::Sdl) -> Result<(), Error> {
    HAPTICS_SYSTEM.with(|cell| {
        if cell.borrow().is_some() {
            return Ok(());
        }

        *cell.borrow_mut() = Some(HapticsSystem::new());
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
    fn new() -> Self {
        Self {
            backend: HapticsBackend::Noop,
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
    Noop,
}

impl HapticsBackend {
    fn start(&mut self, ms: i32) -> Result<(), Error> {
        match self {
            Self::Noop => {
                log_dbg!("Haptics: start requested for {ms} ms, no backend attached");
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        match self {
            Self::Noop => {
                log_dbg!("Haptics: stop requested, no backend attached");
                Ok(())
            }
        }
    }
}
