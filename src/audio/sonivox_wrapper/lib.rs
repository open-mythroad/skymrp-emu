#![allow(dead_code)]

use std::fmt;
use std::os::raw::{c_char, c_int, c_long, c_uint, c_void};
use std::ptr;

type EasResult = c_long;
type EasHandle = *mut c_void;
type EasDataHandle = *mut c_void;

const EAS_SUCCESS: EasResult = 0;
const EAS_STATE_STOPPED: c_long = 4;
const EAS_STATE_ERROR: c_long = 7;

#[derive(Debug)]
pub enum Error {
    Sonivox(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sonivox(msg) => write!(f, "sonivox error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioParams {
    pub sample_rate: i32,
    pub channels: usize,
    pub mix_buffer_size: usize,
}

impl AudioParams {
    pub fn from_config() -> Result<Self, Error> {
        let config = unsafe {
            EAS_Config()
                .as_ref()
                .ok_or_else(|| Error::Sonivox("EAS_Config returned null".to_string()))?
        };

        Ok(Self {
            sample_rate: config.sample_rate,
            channels: config.num_channels.max(1) as usize,
            mix_buffer_size: config.mix_buffer_size.max(1) as usize,
        })
    }
}

#[repr(C)]
struct EasFile {
    handle: *mut c_void,
    read_at: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int) -> c_int>,
    size: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
}

#[repr(C)]
struct EasLibConfig {
    lib_version: u32,
    checked_version: c_uint,
    max_voices: i32,
    num_channels: i32,
    sample_rate: i32,
    mix_buffer_size: i32,
    filter_enabled: c_uint,
    build_timestamp: u32,
    build_guid: *mut c_char,
}

unsafe extern "C" {
    unsafe fn EAS_Init(data: *mut EasDataHandle) -> EasResult;
    unsafe fn EAS_Config() -> *const EasLibConfig;
    unsafe fn EAS_Shutdown(data: EasDataHandle) -> EasResult;
    unsafe fn EAS_OpenFile(
        data: EasDataHandle,
        locator: *mut EasFile,
        stream: *mut EasHandle,
    ) -> EasResult;
    unsafe fn EAS_Prepare(data: EasDataHandle, stream: EasHandle) -> EasResult;
    unsafe fn EAS_Render(
        data: EasDataHandle,
        output: *mut i16,
        requested_frames: i32,
        generated_frames: *mut i32,
    ) -> EasResult;
    unsafe fn EAS_CloseFile(data: EasDataHandle, stream: EasHandle) -> EasResult;
    unsafe fn EAS_State(data: EasDataHandle, stream: EasHandle, state: *mut c_long) -> EasResult;
    unsafe fn EAS_SetRepeat(data: EasDataHandle, stream: EasHandle, repeat_count: i32)
        -> EasResult;
}

struct MemoryMidi {
    data: Vec<u8>,
}

unsafe extern "C" fn read_at(
    handle: *mut c_void,
    buf: *mut c_void,
    offset: c_int,
    size: c_int,
) -> c_int {
    if handle.is_null() || buf.is_null() || offset < 0 || size <= 0 {
        return 0;
    }

    let midi = &*(handle as *const MemoryMidi);
    let start = offset as usize;
    if start >= midi.data.len() {
        return 0;
    }

    let end = start.saturating_add(size as usize).min(midi.data.len());
    let len = end - start;
    ptr::copy_nonoverlapping(midi.data[start..end].as_ptr(), buf as *mut u8, len);
    len as c_int
}

unsafe extern "C" fn size(handle: *mut c_void) -> c_int {
    if handle.is_null() {
        return 0;
    }
    let midi = &*(handle as *const MemoryMidi);
    midi.data.len().min(c_int::MAX as usize) as c_int
}

fn check(result: EasResult, operation: &str) -> Result<(), Error> {
    if result == EAS_SUCCESS {
        Ok(())
    } else {
        Err(Error::Sonivox(format!("{operation} returned {result}")))
    }
}

pub struct Sequence {
    data: EasDataHandle,
    stream: EasHandle,
    _source: Box<MemoryMidi>,
}

unsafe impl Send for Sequence {}

impl Sequence {
    pub fn new(midi_data: &[u8], looped: bool) -> Result<Self, Error> {
        let mut data = ptr::null_mut();
        check(unsafe { EAS_Init(&mut data) }, "EAS_Init")?;

        let mut source = Box::new(MemoryMidi {
            data: midi_data.to_vec(),
        });
        let mut locator = EasFile {
            handle: source.as_mut() as *mut MemoryMidi as *mut c_void,
            read_at: Some(read_at),
            size: Some(size),
        };
        let mut stream = ptr::null_mut();

        if let Err(err) = check(
            unsafe { EAS_OpenFile(data, &mut locator, &mut stream) },
            "EAS_OpenFile",
        ) {
            unsafe {
                EAS_Shutdown(data);
            }
            return Err(err);
        }

        let repeat_count = if looped { -1 } else { 0 };
        if let Err(err) = check(
            unsafe { EAS_SetRepeat(data, stream, repeat_count) },
            "EAS_SetRepeat",
        ) {
            unsafe {
                EAS_CloseFile(data, stream);
                EAS_Shutdown(data);
            }
            return Err(err);
        }

        if let Err(err) = check(unsafe { EAS_Prepare(data, stream) }, "EAS_Prepare") {
            unsafe {
                EAS_CloseFile(data, stream);
                EAS_Shutdown(data);
            }
            return Err(err);
        }

        Ok(Self {
            data,
            stream,
            _source: source,
        })
    }

    fn render(&mut self, frames: usize, channels: usize, output: &mut [i16]) -> usize {
        if self.is_stopped() {
            return 0;
        }

        let requested = frames.min(i32::MAX as usize) as i32;
        let mut generated = 0;
        let result =
            unsafe { EAS_Render(self.data, output.as_mut_ptr(), requested, &mut generated) };
        if result != EAS_SUCCESS || generated <= 0 {
            return 0;
        }

        (generated as usize).min(output.len() / channels.max(1))
    }

    fn is_stopped(&self) -> bool {
        let mut state = 0;
        let result = unsafe { EAS_State(self.data, self.stream, &mut state) };
        result != EAS_SUCCESS || state == EAS_STATE_STOPPED || state == EAS_STATE_ERROR
    }
}

impl Drop for Sequence {
    fn drop(&mut self) {
        unsafe {
            if !self.stream.is_null() {
                EAS_CloseFile(self.data, self.stream);
            }
            if !self.data.is_null() {
                EAS_Shutdown(self.data);
            }
        }
    }
}

pub struct PlaybackState {
    sequence: Option<Sequence>,
    params: AudioParams,
    pcm: Vec<i16>,
    pending: Vec<i16>,
    pending_offset_frames: usize,
}

impl PlaybackState {
    pub fn new(params: AudioParams) -> Self {
        Self {
            sequence: None,
            params,
            pcm: Vec::new(),
            pending: Vec::new(),
            pending_offset_frames: 0,
        }
    }

    pub fn with_default_params() -> Result<Self, Error> {
        Ok(Self::new(AudioParams::from_config()?))
    }

    pub fn params(&self) -> AudioParams {
        self.params
    }

    pub fn play_midi(&mut self, data: &[u8], looped: bool) -> Result<(), Error> {
        self.stop();
        self.sequence = Some(Sequence::new(data, looped)?);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.sequence = None;
        self.pcm.fill(0);
        self.pending.clear();
        self.pending_offset_frames = 0;
    }

    pub fn render_i16(&mut self, output: &mut [i16]) {
        output.fill(0);

        let channels = self.params.channels.max(1);
        if output.len() < channels || self.sequence.is_none() {
            return;
        }

        let requested_frames = output.len() / channels;
        let mut rendered = 0;
        while rendered < requested_frames {
            let copied = self.copy_pending(output, rendered, requested_frames);
            if copied > 0 {
                rendered += copied;
                continue;
            }

            let Some(sequence) = self.sequence.as_mut() else {
                break;
            };

            let samples = self.params.mix_buffer_size * channels;
            self.pcm.resize(samples, 0);

            let generated = sequence.render(self.params.mix_buffer_size, channels, &mut self.pcm);
            if generated == 0 {
                self.sequence = None;
                break;
            }

            self.pending.clear();
            self.pending
                .extend_from_slice(&self.pcm[..generated * channels]);
            self.pending_offset_frames = 0;

            if generated < self.params.mix_buffer_size {
                self.sequence = None;
            }
        }
    }

    fn copy_pending(
        &mut self,
        output: &mut [i16],
        output_offset_frames: usize,
        requested_frames: usize,
    ) -> usize {
        let channels = self.params.channels.max(1);
        let pending_frames = self.pending.len() / channels;
        if self.pending_offset_frames >= pending_frames {
            self.pending.clear();
            self.pending_offset_frames = 0;
            return 0;
        }

        let available = pending_frames - self.pending_offset_frames;
        let count = available.min(requested_frames - output_offset_frames);
        let src_start = self.pending_offset_frames * channels;
        let src_end = src_start + count * channels;
        let dst_start = output_offset_frames * channels;
        let dst_end = dst_start + count * channels;
        output[dst_start..dst_end].copy_from_slice(&self.pending[src_start..src_end]);

        self.pending_offset_frames += count;
        if self.pending_offset_frames >= pending_frames {
            self.pending.clear();
            self.pending_offset_frames = 0;
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_midi_to_pcm() {
        let mut state = PlaybackState::with_default_params().unwrap();
        state
            .play_midi(
                include_bytes!("../../../vendor/sonivox/test/res/test.mid"),
                false,
            )
            .unwrap();

        let params = state.params();
        let mut output = vec![0; params.mix_buffer_size * params.channels];
        let mut saw_nonzero_sample = false;

        for _ in 0..64 {
            state.render_i16(&mut output);
            saw_nonzero_sample |= output.iter().any(|sample| *sample != 0);
            if saw_nonzero_sample {
                break;
            }
        }

        assert!(saw_nonzero_sample);
    }
}
