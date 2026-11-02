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

mod player;
pub use player::Event as MediaEvent;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundType {
    Midi = 0,
    Wav = 1,
    Mp3 = 2,
    Pcm = 3,
    M4a = 4,
    Amr = 5,
    AmrWb = 6,
}

impl TryFrom<i32> for SoundType {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Midi),
            1 => Ok(Self::Wav),
            2 => Ok(Self::Mp3),
            3 => Ok(Self::Pcm),
            4 => Ok(Self::M4a),
            5 => Ok(Self::Amr),
            6 => Ok(Self::AmrWb),
            other => Err(Error::UnsupportedSoundType(other)),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    NotInitialized,
    Sdl(String),
    Sonivox(SonivoxError),
    Player(String),
    UnsupportedSoundType(i32),
    UnsupportedPlayback(SoundType),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "audio system is not initialized"),
            Self::Sdl(msg) => write!(f, "SDL audio error: {msg}"),
            Self::Sonivox(err) => write!(f, "{err}"),
            Self::Player(err) => write!(f, "audio playback error: {err}"),
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

pub fn media_init() -> Result<(), Error> {
    with_system(AudioSystem::media_init)
}

pub fn media_load(data: Vec<u8>) -> Result<(), Error> {
    with_system(|system| system.media_load(data))
}

pub fn media_play(looped: Option<bool>) -> Result<(), Error> {
    with_system(|system| system.media_player.play(looped).map_err(Error::Player))
}

pub fn media_pause() -> Result<(), Error> {
    with_system(|system| system.media_player.pause().map_err(Error::Player))
}

pub fn media_resume() -> Result<(), Error> {
    with_system(|system| system.media_player.resume().map_err(Error::Player))
}

pub fn media_stop() -> Result<(), Error> {
    with_system(AudioSystem::media_stop)
}

pub fn media_close() -> Result<(), Error> {
    with_system(AudioSystem::media_close)
}

pub fn media_seek(position_ms: u64) -> Result<(), Error> {
    with_system(|system| system.media_seek(position_ms))
}

pub fn media_free() -> Result<(), Error> {
    with_system(AudioSystem::media_free)
}

pub fn media_set_volume(volume: i32) -> Result<(), Error> {
    with_system(|system| {
        system.media_player.set_volume(volume);
        Ok(())
    })
}

pub fn media_state() -> Result<i32, Error> {
    with_system(|system| Ok(system.media_player.state()))
}

pub fn media_total_time_ms() -> Result<u64, Error> {
    with_system(|system| Ok(system.media_player.total_time_ms()))
}

pub fn media_current_time_ms() -> Result<u64, Error> {
    with_system(|system| Ok(system.media_player.current_time_ms()))
}

pub fn take_media_event() -> Result<Option<MediaEvent>, Error> {
    with_system(|system| Ok(system.media_player.take_event()))
}

fn with_system<T, F>(f: F) -> Result<T, Error>
where
    F: FnOnce(&mut AudioSystem) -> Result<T, Error>,
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
    midi_state: Arc<Mutex<PlaybackState>>,
    sound_player: player::Controller,
    media_player: player::Controller,
    device: AudioDevice<AudioMixer>,
}

impl AudioSystem {
    fn new(sdl: &sdl2::Sdl) -> Result<Self, Error> {
        let audio = sdl.audio().map_err(Error::Sdl)?;
        let midi_state = Arc::new(Mutex::new(PlaybackState::with_default_params()?));
        let params = midi_state.lock().unwrap().params();
        let samples = if cfg!(target_os = "emscripten") {
            2048
        } else {
            params.mix_buffer_size
        };

        let desired = AudioSpecDesired {
            freq: Some(params.sample_rate),
            channels: Some(params.channels.try_into().unwrap()),
            samples: Some(samples.try_into().unwrap()),
        };

        let sample_rate = params.sample_rate.try_into().unwrap();
        let (sound_player, sound_mixer) =
            player::Controller::new("sound", sample_rate, params.channels);
        let (media_player, media_mixer) =
            player::Controller::new("media", sample_rate, params.channels);
        let callback_midi_state = Arc::clone(&midi_state);
        let device = audio
            .open_playback(None, &desired, move |_spec| AudioMixer {
                midi_state: callback_midi_state,
                sound_mixer,
                media_mixer,
            })
            .map_err(Error::Sdl)?;
        device.resume();

        log_dbg!(
            "Audio initialized: {} Hz, {} channels, Sonivox embedded wavetable",
            params.sample_rate,
            params.channels
        );

        Ok(Self {
            midi_state,
            sound_player,
            media_player,
            device,
        })
    }

    fn reset_media_player<F>(&mut self, operation: F) -> Result<(), Error>
    where
        F: FnOnce(&mut player::Controller) -> Result<(), String>,
    {
        let mut callback = self.device.lock();
        let result = operation(&mut self.media_player).map_err(Error::Player);
        callback.media_mixer.clear();
        result
    }

    fn media_init(&mut self) -> Result<(), Error> {
        self.reset_media_player(player::Controller::init)
    }

    fn media_load(&mut self, data: Vec<u8>) -> Result<(), Error> {
        self.reset_media_player(|player| player.load(data))
    }

    fn media_stop(&mut self) -> Result<(), Error> {
        self.reset_media_player(player::Controller::stop)
    }

    fn media_close(&mut self) -> Result<(), Error> {
        self.reset_media_player(player::Controller::close)
    }

    fn media_seek(&mut self, position_ms: u64) -> Result<(), Error> {
        self.reset_media_player(|player| player.seek(position_ms))
    }

    fn media_free(&mut self) -> Result<(), Error> {
        self.reset_media_player(player::Controller::free)
    }

    fn play_sound(&mut self, type_: SoundType, data: &[u8], looped: bool) -> Result<(), Error> {
        match type_ {
            SoundType::Midi => {
                self.reset_sound_player(player::Controller::close)?;
                self.midi_state.lock().unwrap().play_midi(data, looped)?;
                Ok(())
            }
            SoundType::Wav | SoundType::Mp3 | SoundType::M4a => {
                self.midi_state.lock().unwrap().stop();
                self.reset_sound_player(|player| player.replace_and_play(data.to_vec(), looped))
            }
            other => Err(Error::UnsupportedPlayback(other)),
        }
    }

    fn stop_sound(&mut self, _type: SoundType) -> Result<(), Error> {
        self.midi_state.lock().unwrap().stop();
        self.reset_sound_player(player::Controller::close)
    }

    fn reset_sound_player<F>(&mut self, operation: F) -> Result<(), Error>
    where
        F: FnOnce(&mut player::Controller) -> Result<(), String>,
    {
        let mut callback = self.device.lock();
        let result = operation(&mut self.sound_player).map_err(Error::Player);
        callback.sound_mixer.clear();
        result
    }
}

struct AudioMixer {
    midi_state: Arc<Mutex<PlaybackState>>,
    sound_mixer: player::Mixer,
    media_mixer: player::Mixer,
}

impl AudioCallback for AudioMixer {
    type Channel = i16;

    fn callback(&mut self, output: &mut [i16]) {
        let mut state = self.midi_state.lock().unwrap();
        state.render_i16(output);
        self.sound_mixer.mix_into(output);
        self.media_mixer.mix_into(output);
    }
}
