/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use skymrp_sonivox_wrapper::{Error as SonivoxError, PlaybackState};
use std::cell::RefCell;
use std::fmt;
use std::sync::{Arc, Mutex};

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundType {
    Midi = 0,
    Wav = 1,
    Mp3 = 2,
    Amr = 3,
    Pcm = 4,
}

impl TryFrom<i32> for SoundType {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Midi),
            1 => Ok(Self::Wav),
            2 => Ok(Self::Mp3),
            3 => Ok(Self::Amr),
            4 => Ok(Self::Pcm),
            other => Err(Error::UnsupportedSoundType(other)),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    NotInitialized,
    Sdl(String),
    Sonivox(SonivoxError),
    UnsupportedSoundType(i32),
    UnsupportedPlayback(SoundType),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "audio system is not initialized"),
            Self::Sdl(msg) => write!(f, "SDL audio error: {msg}"),
            Self::Sonivox(err) => write!(f, "{err}"),
            Self::UnsupportedSoundType(type_) => write!(f, "unsupported sound type: {type_}"),
            Self::UnsupportedPlayback(type_) => write!(f, "unsupported playback: {type_:?}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<SonivoxError> for Error {
    fn from(value: SonivoxError) -> Self {
        Self::Sonivox(value)
    }
}

thread_local! {
    static AUDIO_SYSTEM: RefCell<Option<AudioSystem>> = const { RefCell::new(None) };
}

pub fn init(sdl: &sdl2::Sdl) -> Result<(), Error> {
    AUDIO_SYSTEM.with(|cell| {
        if cell.borrow().is_some() {
            return Ok(());
        }

        let system = AudioSystem::new(sdl)?;
        *cell.borrow_mut() = Some(system);
        Ok(())
    })
}

pub fn play_sound(type_: SoundType, data: &[u8], looped: bool) -> Result<(), Error> {
    with_system(|system| system.play_sound(type_, data, looped))
}

pub fn stop_sound(type_: SoundType) -> Result<(), Error> {
    with_system(|system| system.stop_sound(type_))
}

fn with_system<F>(f: F) -> Result<(), Error>
where
    F: FnOnce(&mut AudioSystem) -> Result<(), Error>,
{
    AUDIO_SYSTEM.with(|cell| {
        let mut system = cell.borrow_mut();
        let Some(system) = system.as_mut() else {
            return Err(Error::NotInitialized);
        };
        f(system)
    })
}

struct AudioSystem {
    state: Arc<Mutex<PlaybackState>>,
    _device: AudioDevice<SonivoxCallback>,
}

impl AudioSystem {
    fn new(sdl: &sdl2::Sdl) -> Result<Self, Error> {
        let audio = sdl.audio().map_err(Error::Sdl)?;
        let state = Arc::new(Mutex::new(PlaybackState::with_default_params()?));
        let params = state.lock().unwrap().params();

        let desired = AudioSpecDesired {
            freq: Some(params.sample_rate),
            channels: Some(params.channels.try_into().unwrap()),
            samples: Some(params.mix_buffer_size.try_into().unwrap()),
        };

        let device = audio
            .open_playback(None, &desired, |_spec| SonivoxCallback {
                state: Arc::clone(&state),
            })
            .map_err(Error::Sdl)?;
        device.resume();

        log_dbg!(
            "Audio initialized: {} Hz, {} channels, Sonivox embedded wavetable",
            params.sample_rate,
            params.channels
        );

        Ok(Self {
            state,
            _device: device,
        })
    }

    fn play_sound(&mut self, type_: SoundType, data: &[u8], looped: bool) -> Result<(), Error> {
        match type_ {
            SoundType::Midi => {
                let mut state = self.state.lock().unwrap();
                state.play_midi(data, looped)?;
                Ok(())
            }
            other => Err(Error::UnsupportedPlayback(other)),
        }
    }

    fn stop_sound(&mut self, type_: SoundType) -> Result<(), Error> {
        match type_ {
            SoundType::Midi => {
                self.state.lock().unwrap().stop();
                Ok(())
            }
            other => Err(Error::UnsupportedPlayback(other)),
        }
    }
}

struct SonivoxCallback {
    state: Arc<Mutex<PlaybackState>>,
}

impl AudioCallback for SonivoxCallback {
    type Channel = i16;

    fn callback(&mut self, output: &mut [i16]) {
        let mut state = self.state.lock().unwrap();
        state.render_i16(output);
    }
}
