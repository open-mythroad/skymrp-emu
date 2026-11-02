/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use rtrb::{Consumer, Producer, RingBuffer};
use rubato::{FftFixedIn, Resampler};
use std::collections::VecDeque;
use std::io::{Cursor, ErrorKind};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use symphonia::default::{get_codecs, get_probe};

const NO_EVENT: i32 = -1;
const COMPLETE_EVENT: i32 = 0;
const ERROR_EVENT: i32 = 1;
const BUFFER_SECONDS: usize = 2;
const MAX_VOLUME: i32 = 5;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle = 1001,
    Inited = 1002,
    Loaded = 1003,
    Playing = 1004,
    Paused = 1005,
    Suspended = 1006,
    Suspending = 1007,
    Null = 1008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Complete,
    Error,
}

#[derive(Clone, Copy)]
struct RingSample {
    generation: u64,
    value: i16,
}

struct Shared {
    state: AtomicI32,
    generation: AtomicU64,
    position_frames: AtomicU64,
    seek_position_ms: AtomicU64,
    total_ms: AtomicU64,
    looping: AtomicBool,
    eof: AtomicBool,
    event: AtomicI32,
    volume: AtomicI32,
    sample_rate: u32,
    channels: usize,
}

impl Shared {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            state: AtomicI32::new(State::Idle as i32),
            generation: AtomicU64::new(0),
            position_frames: AtomicU64::new(0),
            seek_position_ms: AtomicU64::new(0),
            total_ms: AtomicU64::new(0),
            looping: AtomicBool::new(false),
            eof: AtomicBool::new(false),
            event: AtomicI32::new(NO_EVENT),
            volume: AtomicI32::new(MAX_VOLUME),
            sample_rate,
            channels,
        }
    }

    fn reset_position(&self, position_ms: u64) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.position_frames.store(0, Ordering::Release);
        self.seek_position_ms.store(position_ms, Ordering::Release);
        self.eof.store(false, Ordering::Release);
        self.event.store(NO_EVENT, Ordering::Release);
        generation
    }

    fn current_position_ms(&self) -> u64 {
        let frames = self.position_frames.load(Ordering::Acquire);
        let elapsed_ms = frames.saturating_mul(1000) / u64::from(self.sample_rate);
        let position = self
            .seek_position_ms
            .load(Ordering::Acquire)
            .saturating_add(elapsed_ms);
        let total = self.total_ms.load(Ordering::Acquire);

        if total == 0 {
            position
        } else if self.looping.load(Ordering::Acquire) {
            position % total
        } else {
            position.min(total)
        }
    }
}

enum Command {
    Init(SyncSender<Result<(), String>>),
    Load(Vec<u8>, SyncSender<Result<(), String>>),
    ReplaceAndPlay(Vec<u8>, bool, SyncSender<Result<(), String>>),
    Play(Option<bool>, SyncSender<Result<(), String>>),
    Pause(SyncSender<Result<(), String>>),
    Resume(SyncSender<Result<(), String>>),
    Stop(SyncSender<Result<(), String>>),
    Close(SyncSender<Result<(), String>>),
    Seek(u64, SyncSender<Result<(), String>>),
    Free(SyncSender<Result<(), String>>),
    Shutdown,
}

pub struct Controller {
    sender: Sender<Command>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Controller {
    pub fn new(name: &str, sample_rate: u32, channels: usize) -> (Self, Mixer) {
        let capacity = sample_rate as usize * channels * BUFFER_SECONDS;
        let (producer, consumer) = RingBuffer::new(capacity.max(1));
        let shared = Arc::new(Shared::new(sample_rate, channels));
        let (sender, receiver) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name(format!("skymrp-{name}-decoder"))
            .spawn(move || Worker::new(receiver, producer, worker_shared).run())
            .expect("failed to start audio decoder thread");

        (
            Self {
                sender,
                shared: Arc::clone(&shared),
                thread: Some(thread),
            },
            Mixer { consumer, shared },
        )
    }

    pub fn init(&mut self) -> Result<(), String> {
        self.request(Command::Init)
    }

    pub fn load(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.request(|reply| Command::Load(data, reply))
    }

    pub fn replace_and_play(&mut self, data: Vec<u8>, looped: bool) -> Result<(), String> {
        self.request(|reply| Command::ReplaceAndPlay(data, looped, reply))
    }

    pub fn play(&mut self, looped: Option<bool>) -> Result<(), String> {
        self.request(|reply| Command::Play(looped, reply))
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.request(Command::Pause)
    }

    pub fn resume(&mut self) -> Result<(), String> {
        self.request(Command::Resume)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.request(Command::Stop)
    }

    pub fn close(&mut self) -> Result<(), String> {
        self.request(Command::Close)
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        self.request(|reply| Command::Seek(position_ms, reply))
    }

    pub fn free(&mut self) -> Result<(), String> {
        self.request(Command::Free)
    }

    pub fn set_volume(&self, volume: i32) {
        self.shared
            .volume
            .store(volume.clamp(0, MAX_VOLUME), Ordering::Release);
    }

    pub fn state(&self) -> i32 {
        self.shared.state.load(Ordering::Acquire)
    }

    pub fn total_time_ms(&self) -> u64 {
        self.shared.total_ms.load(Ordering::Acquire)
    }

    pub fn current_time_ms(&self) -> u64 {
        self.shared.current_position_ms()
    }

    pub fn take_event(&self) -> Option<Event> {
        match self.shared.event.swap(NO_EVENT, Ordering::AcqRel) {
            COMPLETE_EVENT => Some(Event::Complete),
            ERROR_EVENT => Some(Event::Error),
            _ => None,
        }
    }

    fn request<F>(&mut self, make_command: F) -> Result<(), String>
    where
        F: FnOnce(SyncSender<Result<(), String>>) -> Command,
    {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(make_command(reply_sender))
            .map_err(|_| "audio decoder thread stopped".to_owned())?;
        reply_receiver
            .recv()
            .map_err(|_| "audio decoder thread stopped".to_owned())?
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct Mixer {
    consumer: Consumer<RingSample>,
    shared: Arc<Shared>,
}

impl Mixer {
    pub fn clear(&mut self) {
        let slots = self.consumer.slots();
        if slots != 0 {
            self.consumer
                .read_chunk(slots)
                .expect("consumer slot count changed")
                .commit_all();
        }
    }

    pub fn mix_into(&mut self, output: &mut [i16]) {
        if self.shared.state.load(Ordering::Acquire) != State::Playing as i32 {
            return;
        }

        let generation = self.shared.generation.load(Ordering::Acquire);
        let volume = self.shared.volume.load(Ordering::Acquire);
        let mut mixed_samples = 0usize;

        for output_sample in output.iter_mut() {
            let sample = match self.consumer.pop() {
                Ok(sample) if sample.generation == generation => sample.value,
                Ok(_) => continue,
                Err(_) => break,
            };
            *output_sample = output_sample.saturating_add(scale_sample(sample, volume));
            mixed_samples += 1;
        }

        let mixed_frames = mixed_samples / self.shared.channels;
        if mixed_frames != 0 {
            self.shared
                .position_frames
                .fetch_add(mixed_frames as u64, Ordering::AcqRel);
        }

        if self.shared.eof.load(Ordering::Acquire) && self.consumer.is_empty() {
            self.shared
                .state
                .store(State::Loaded as i32, Ordering::Release);
            self.shared.event.store(COMPLETE_EVENT, Ordering::Release);
            self.shared.eof.store(false, Ordering::Release);
        }
    }
}

fn scale_sample(sample: i16, volume: i32) -> i16 {
    ((i32::from(sample) * volume) / MAX_VOLUME) as i16
}

struct Worker {
    receiver: Receiver<Command>,
    producer: Producer<RingSample>,
    shared: Arc<Shared>,
    source: Option<DecodedSource>,
    pending: VecDeque<i16>,
    playing: bool,
    generation: u64,
}

impl Worker {
    fn new(
        receiver: Receiver<Command>,
        producer: Producer<RingSample>,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            receiver,
            producer,
            shared,
            source: None,
            pending: VecDeque::new(),
            playing: false,
            generation: 0,
        }
    }

    fn run(mut self) {
        loop {
            match self.pump() {
                None => return,
                Some(true) => {}
                Some(false) => match self.receiver.recv_timeout(Duration::from_millis(2)) {
                    Ok(command) => {
                        if self.handle_command(command) {
                            return;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
            }
        }
    }

    fn pump(&mut self) -> Option<bool> {
        while let Ok(command) = self.receiver.try_recv() {
            if self.handle_command(command) {
                return None;
            }
        }

        let mut made_progress = false;
        if self.playing {
            made_progress |= self.push_pending();
            if self.pending.is_empty() {
                made_progress |= self.decode_more();
                made_progress |= self.push_pending();
            }
        }
        Some(made_progress)
    }

    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Init(reply) => {
                self.source = None;
                self.playing = false;
                self.pending.clear();
                self.generation = self.shared.reset_position(0);
                self.shared.total_ms.store(0, Ordering::Release);
                self.shared
                    .state
                    .store(State::Inited as i32, Ordering::Release);
                let _ = reply.send(Ok(()));
            }
            Command::Load(data, reply) => {
                let result =
                    DecodedSource::new(data, self.shared.sample_rate, self.shared.channels).map(
                        |source| {
                            self.playing = false;
                            self.pending.clear();
                            self.generation = self.shared.reset_position(0);
                            self.shared
                                .total_ms
                                .store(source.total_time_ms(), Ordering::Release);
                            self.source = Some(source);
                            self.shared
                                .state
                                .store(State::Loaded as i32, Ordering::Release);
                        },
                    );
                let _ = reply.send(result);
            }
            Command::ReplaceAndPlay(data, looped, reply) => {
                self.source = None;
                self.playing = false;
                self.pending.clear();
                self.generation = self.shared.reset_position(0);
                self.shared.total_ms.store(0, Ordering::Release);
                self.shared
                    .state
                    .store(State::Inited as i32, Ordering::Release);
                let result =
                    DecodedSource::new(data, self.shared.sample_rate, self.shared.channels).map(
                        |source| {
                            self.shared
                                .total_ms
                                .store(source.total_time_ms(), Ordering::Release);
                            self.source = Some(source);
                            self.playing = true;
                            self.shared.looping.store(looped, Ordering::Release);
                            self.shared
                                .state
                                .store(State::Playing as i32, Ordering::Release);
                        },
                    );
                let _ = reply.send(result);
            }
            Command::Play(looped, reply) => {
                if let Some(looped) = looped {
                    self.shared.looping.store(looped, Ordering::Release);
                }
                let state = self.shared.state.load(Ordering::Acquire);
                if self.source.is_some()
                    && (state == State::Loaded as i32 || state == State::Paused as i32)
                {
                    let total = self.shared.total_ms.load(Ordering::Acquire);
                    if total != 0 && self.shared.current_position_ms() >= total {
                        let _ = self.seek_to(0);
                    }
                    self.playing = true;
                    self.shared.eof.store(false, Ordering::Release);
                    self.shared.event.store(NO_EVENT, Ordering::Release);
                    self.shared
                        .state
                        .store(State::Playing as i32, Ordering::Release);
                }
                let _ = reply.send(Ok(()));
            }
            Command::Pause(reply) => {
                if self.shared.state.load(Ordering::Acquire) == State::Playing as i32 {
                    self.playing = false;
                    self.shared
                        .state
                        .store(State::Paused as i32, Ordering::Release);
                }
                let _ = reply.send(Ok(()));
            }
            Command::Resume(reply) => {
                if self.shared.state.load(Ordering::Acquire) == State::Paused as i32 {
                    self.playing = true;
                    self.shared
                        .state
                        .store(State::Playing as i32, Ordering::Release);
                }
                let _ = reply.send(Ok(()));
            }
            Command::Stop(reply) => {
                self.playing = false;
                let result = self.seek_to(0);
                self.shared
                    .state
                    .store(State::Loaded as i32, Ordering::Release);
                let _ = reply.send(result);
            }
            Command::Close(reply) => {
                self.source = None;
                self.playing = false;
                self.pending.clear();
                self.generation = self.shared.reset_position(0);
                self.shared.total_ms.store(0, Ordering::Release);
                self.shared
                    .state
                    .store(State::Idle as i32, Ordering::Release);
                let _ = reply.send(Ok(()));
            }
            Command::Seek(position_ms, reply) => {
                let result = self.seek_to(position_ms);
                let _ = reply.send(result);
            }
            Command::Free(reply) => {
                self.source = None;
                self.playing = false;
                self.pending.clear();
                self.generation = self.shared.reset_position(0);
                self.shared.total_ms.store(0, Ordering::Release);
                self.shared
                    .state
                    .store(State::Null as i32, Ordering::Release);
                let _ = reply.send(Ok(()));
            }
            Command::Shutdown => return true,
        }
        false
    }

    fn seek_to(&mut self, position_ms: u64) -> Result<(), String> {
        let total = self.shared.total_ms.load(Ordering::Acquire);
        let position_ms = if total == 0 {
            position_ms
        } else {
            position_ms.min(total)
        };
        let source = self
            .source
            .as_mut()
            .ok_or_else(|| "no audio is loaded".to_owned())?;
        source.seek(position_ms)?;
        self.pending.clear();
        self.generation = self.shared.reset_position(position_ms);
        Ok(())
    }

    fn push_pending(&mut self) -> bool {
        let mut pushed = false;
        while let Some(value) = self.pending.pop_front() {
            let sample = RingSample {
                generation: self.generation,
                value,
            };
            match self.producer.push(sample) {
                Ok(()) => pushed = true,
                Err(rtrb::PushError::Full(sample)) => {
                    self.pending.push_front(sample.value);
                    break;
                }
            }
        }
        pushed
    }

    fn decode_more(&mut self) -> bool {
        let Some(source) = self.source.as_mut() else {
            self.playing = false;
            return false;
        };

        match source.decode_next() {
            Ok(DecodeResult::Samples(samples)) => {
                self.pending.extend(samples);
                true
            }
            Ok(DecodeResult::End) => {
                if self.shared.looping.load(Ordering::Acquire) {
                    if let Err(err) = source.seek(0) {
                        self.fail(err);
                    }
                } else {
                    self.playing = false;
                    self.shared.eof.store(true, Ordering::Release);
                }
                true
            }
            Err(err) => {
                self.fail(err);
                true
            }
        }
    }

    fn fail(&mut self, error: String) {
        log!("Audio decoder failed: {error}");
        self.playing = false;
        self.pending.clear();
        self.shared.event.store(ERROR_EVENT, Ordering::Release);
    }
}

enum DecodeResult {
    Samples(Vec<i16>),
    End,
}

struct DecodedSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    total_time_ms: u64,
    target_rate: u32,
    target_channels: usize,
    resampler: Option<RubatoResampler>,
}

impl DecodedSource {
    fn new(data: Vec<u8>, target_rate: u32, target_channels: usize) -> Result<Self, String> {
        let source = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());
        let probed = get_probe()
            .format(
                &Hint::new(),
                source,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|err| format!("unsupported audio format: {err}"))?;
        let format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| "audio has no default track".to_owned())?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let total_time_ms = duration_ms(&params);
        let decoder = get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|err| format!("unsupported audio codec: {err}"))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            total_time_ms,
            target_rate,
            target_channels,
            resampler: None,
        })
    }

    fn total_time_ms(&self) -> u64 {
        self.total_time_ms
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        let time = Time::new(position_ms / 1000, (position_ms % 1000) as f64 / 1000.0);
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|err| format!("audio seek failed: {err}"))?;
        self.decoder.reset();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
        Ok(())
    }

    fn decode_next(&mut self) -> Result<DecodeResult, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => {
                    let samples = match self.resampler.as_mut() {
                        Some(resampler) => resampler.finish()?,
                        None => Vec::new(),
                    };
                    return if samples.is_empty() {
                        Ok(DecodeResult::End)
                    } else {
                        Ok(DecodeResult::Samples(samples))
                    };
                }
                Err(err) => return Err(format!("failed to read audio packet: {err}")),
            };
            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(err)) => {
                    log_dbg!("Skipping corrupt audio packet: {err}");
                    continue;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(err) => return Err(format!("failed to decode audio packet: {err}")),
            };

            let spec = *decoded.spec();
            let source_channels = spec.channels.count();
            let needs_resampler = self.resampler.as_ref().map_or(true, |resampler| {
                resampler.source_rate != spec.rate || resampler.source_channels != source_channels
            });
            if needs_resampler {
                self.resampler = Some(RubatoResampler::new(
                    spec.rate,
                    source_channels,
                    self.target_rate,
                    self.target_channels,
                )?);
            }

            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            let output = self
                .resampler
                .as_mut()
                .expect("resampler initialized from decoded audio spec")
                .process(samples.samples())?;
            if !output.is_empty() {
                return Ok(DecodeResult::Samples(output));
            }
        }
    }
}

fn duration_ms(params: &CodecParameters) -> u64 {
    let Some(frames) = params.n_frames else {
        return 0;
    };
    let Some(time_base) = params.time_base else {
        return 0;
    };
    let time = time_base.calc_time(frames);
    time.seconds
        .saturating_mul(1000)
        .saturating_add((time.frac * 1000.0) as u64)
}

struct RubatoResampler {
    source_rate: u32,
    source_channels: usize,
    target_rate: u32,
    target_channels: usize,
    resampler: Option<FftFixedIn<f32>>,
    pending: Vec<Vec<f32>>,
    delay_frames: usize,
    remaining_delay_frames: usize,
    input_frames: u64,
    output_frames: u64,
}

impl RubatoResampler {
    fn new(
        source_rate: u32,
        source_channels: usize,
        target_rate: u32,
        target_channels: usize,
    ) -> Result<Self, String> {
        let resampler = if source_rate == target_rate {
            None
        } else {
            Some(
                FftFixedIn::new(
                    source_rate as usize,
                    target_rate as usize,
                    1024,
                    2,
                    target_channels,
                )
                .map_err(|err| format!("could not create audio resampler: {err}"))?,
            )
        };
        let delay_frames = resampler.as_ref().map(Resampler::output_delay).unwrap_or(0);

        Ok(Self {
            source_rate,
            source_channels,
            target_rate,
            target_channels,
            resampler,
            pending: vec![Vec::new(); target_channels],
            delay_frames,
            remaining_delay_frames: delay_frames,
            input_frames: 0,
            output_frames: 0,
        })
    }

    fn reset(&mut self) {
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
        for channel in &mut self.pending {
            channel.clear();
        }
        self.remaining_delay_frames = self.delay_frames;
        self.input_frames = 0;
        self.output_frames = 0;
    }

    fn process(&mut self, input: &[f32]) -> Result<Vec<i16>, String> {
        let frame_count = input.len() / self.source_channels;
        self.input_frames = self.input_frames.saturating_add(frame_count as u64);

        if self.resampler.is_none() {
            let mut output = Vec::with_capacity(frame_count * self.target_channels);
            for frame in input.chunks_exact(self.source_channels) {
                for channel in 0..self.target_channels {
                    output.push(float_to_i16(map_channel(
                        frame,
                        channel,
                        self.target_channels,
                    )));
                }
            }
            self.output_frames = self.output_frames.saturating_add(frame_count as u64);
            return Ok(output);
        }

        for frame in input.chunks_exact(self.source_channels) {
            for channel in 0..self.target_channels {
                self.pending[channel].push(map_channel(frame, channel, self.target_channels));
            }
        }

        let mut output = Vec::new();
        loop {
            let needed = self
                .resampler
                .as_ref()
                .expect("resampler checked above")
                .input_frames_next();
            if self.pending[0].len() < needed {
                break;
            }

            let resampled = {
                let input: Vec<&[f32]> = self
                    .pending
                    .iter()
                    .map(|channel| &channel[..needed])
                    .collect();
                self.resampler
                    .as_mut()
                    .expect("resampler checked above")
                    .process(&input, None)
                    .map_err(|err| format!("audio resampling failed: {err}"))?
            };
            for channel in &mut self.pending {
                channel.drain(..needed);
            }
            self.append_output(&resampled, u64::MAX, &mut output);
        }
        Ok(output)
    }

    fn finish(&mut self) -> Result<Vec<i16>, String> {
        let expected_frames = self
            .input_frames
            .saturating_mul(u64::from(self.target_rate))
            / u64::from(self.source_rate);
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(Vec::new());
        };

        let resampled = if self.pending[0].is_empty() {
            resampler.process_partial::<Vec<f32>>(None, None)
        } else {
            resampler.process_partial(Some(&self.pending), None)
        }
        .map_err(|err| format!("audio resampling failed: {err}"))?;

        let mut output = Vec::new();
        self.append_output(&resampled, expected_frames, &mut output);
        self.reset();
        Ok(output)
    }

    fn append_output(&mut self, channels: &[Vec<f32>], frame_limit: u64, output: &mut Vec<i16>) {
        let available_frames = channels.first().map(Vec::len).unwrap_or(0);
        let skipped = self.remaining_delay_frames.min(available_frames);
        self.remaining_delay_frames -= skipped;

        for frame in skipped..available_frames {
            if self.output_frames >= frame_limit {
                break;
            }
            for channel in channels {
                output.push(float_to_i16(channel[frame]));
            }
            self.output_frames += 1;
        }
    }
}

fn map_channel(frame: &[f32], channel: usize, target_channels: usize) -> f32 {
    if target_channels == 1 {
        return frame.iter().copied().sum::<f32>() / frame.len() as f32;
    }

    let left = frame[0];
    let right = frame.get(1).copied().unwrap_or(left);
    match channel {
        0 => left,
        1 => right,
        _ => (left + right) * 0.5,
    }
}

fn float_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}
