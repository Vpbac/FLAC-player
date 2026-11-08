//! Audio engine: decodes with Symphonia and renders through WASAPI.
//!
//! * Exclusive mode (default): the device is opened at the exact sample rate of the
//!   file and with an integer sample format that is at least as wide as the source,
//!   so samples reach the DAC untouched (bit-perfect) when volume is 100%.
//! * Shared mode: 32-bit float at the source rate, Windows mixer converts if needed.
//! * Gapless: the next track is decoded into the same stream as soon as the current
//!   one ends, as long as sample rate and channel count match.

use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, Handle, SampleType, StreamMode,
    WaveFormat, calculate_period_100ns, initialize_mta,
};

use crate::eq::{EqConfig, EqProcessor};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Default)]
pub struct EngineStatus {
    pub state: PlayState,
    pub path: Option<PathBuf>,
    pub position: f64,
    pub duration: f64,
    pub source_info: String,
    pub output_info: String,
    pub bit_perfect: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

pub enum Command {
    Play { path: PathBuf, start_at: f64 },
    Pause,
    Resume,
    Stop,
    Seek(f64),
    /// Track to continue with when the current one ends (gapless when compatible).
    SetNext(Option<PathBuf>),
    SetOutput { device_id: Option<String>, exclusive: bool },
    SetEq(EqConfig),
    ListDevices,
    Quit,
}

pub enum EngineEvent {
    /// The engine moved on to the queued "next" track by itself.
    TrackChanged(PathBuf),
    /// Playback reached the end and nothing was queued.
    Finished,
    Devices(Vec<DeviceInfo>),
    /// A track could not be opened.
    TrackFailed(PathBuf, String),
}

pub struct EngineHandle {
    pub tx: Sender<Command>,
    pub rx: Receiver<EngineEvent>,
    pub status: Arc<Mutex<EngineStatus>>,
    pub volume: Arc<AtomicU32>,
}

impl EngineHandle {
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn status(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }
}

pub fn spawn(repaint: impl Fn() + Send + 'static) -> EngineHandle {
    let (tx, cmd_rx) = crossbeam_channel::unbounded();
    let (ev_tx, rx) = crossbeam_channel::unbounded();
    let status = Arc::new(Mutex::new(EngineStatus::default()));
    let volume = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let st = status.clone();
    let vol = volume.clone();
    std::thread::Builder::new()
        .name("audio-engine".into())
        .spawn(move || {
            let _ = initialize_mta();
            let mut engine = Engine::new(cmd_rx, ev_tx, st, vol, Box::new(repaint));
            engine.run();
        })
        .expect("spawn audio thread");
    EngineHandle { tx, rx, status, volume }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

struct Source {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    channels: usize,
    bits: u32,
    duration: f64,
    codec_name: String,
    eof: bool,
    skip_frames: u64,
    tmp: Vec<i32>,
}

impl Source {
    fn open(path: &Path) -> Result<Source, String> {
        let file = File::open(path).map_err(|e| format!("Không mở được file: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| format!("Định dạng không hỗ trợ: {e}"))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("Không có track audio")?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or("Thiếu thông số codec")?
            .clone();
        let track_id = track.id;
        let rate = params.sample_rate.ok_or("Không rõ sample rate")?;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2).max(1);
        let bits = params.bits_per_sample.unwrap_or(16);
        let duration = match (track.time_base, track.duration) {
            (Some(tb), Some(d)) => tb.calc_duration(d).map(|t| t.as_secs_f64()).unwrap_or(0.0),
            _ => track.num_frames.map(|n| n as f64 / rate as f64).unwrap_or(0.0),
        };
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Codec không hỗ trợ: {e}"))?;
        let codec_name = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("?")
            .to_uppercase();
        Ok(Source {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            rate,
            channels,
            bits,
            duration,
            codec_name,
            eof: false,
            skip_frames: 0,
            tmp: Vec::new(),
        })
    }

    fn info(&self) -> String {
        let ch = match self.channels {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            n => format!("{n}ch"),
        };
        format!("{} {}-bit / {} • {}", self.codec_name, self.bits, fmt_rate(self.rate), ch)
    }

    /// Decode one packet and append interleaved i32 samples. Returns false at end of stream.
    fn decode_into(&mut self, fifo: &mut VecDeque<i32>) -> bool {
        if self.eof {
            return false;
        }
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => {
                    self.eof = true;
                    return false;
                }
                Err(SymError::DecodeError(_)) => continue,
                Err(_) => {
                    self.eof = true;
                    return false;
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    buf.copy_to_vec_interleaved::<i32>(&mut self.tmp);
                    let ch = self.channels;
                    let frames = self.tmp.len() / ch;
                    let mut start = 0usize;
                    if self.skip_frames > 0 {
                        let s = (self.skip_frames as usize).min(frames);
                        self.skip_frames -= s as u64;
                        start = s * ch;
                    }
                    fifo.extend(&self.tmp[start..]);
                    return true;
                }
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => continue,
                Err(_) => {
                    self.eof = true;
                    return false;
                }
            }
        }
    }

    fn seek(&mut self, secs: f64) -> f64 {
        let secs = secs.clamp(0.0, (self.duration - 0.05).max(0.0));
        let time = Time::try_from_secs_f64(secs).unwrap_or(Time::ZERO);
        match self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time { time, track_id: Some(self.track_id) },
        ) {
            Ok(seeked) => {
                self.decoder.reset();
                self.eof = false;
                let diff = seeked.required_ts.get() - seeked.actual_ts.get();
                let tb = self
                    .format
                    .tracks()
                    .iter()
                    .find(|t| t.id == self.track_id)
                    .and_then(|t| t.time_base);
                self.skip_frames = match tb {
                    Some(tb) if diff > 0 => {
                        let s = diff as f64 * tb.numer.get() as f64 / tb.denom.get() as f64;
                        (s * self.rate as f64).round() as u64
                    }
                    _ => 0,
                };
                secs
            }
            Err(_) => 0.0,
        }
    }
}

fn fmt_rate(rate: u32) -> String {
    if rate % 1000 == 0 {
        format!("{} kHz", rate / 1000)
    } else {
        format!("{:.1} kHz", rate as f64 / 1000.0)
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum SampleKind {
    I16,
    I24,
    I24In32,
    I32,
    F32,
}

impl SampleKind {
    fn bytes(self) -> usize {
        match self {
            SampleKind::I16 => 2,
            SampleKind::I24 => 3,
            _ => 4,
        }
    }
    fn valid_bits(self) -> u32 {
        match self {
            SampleKind::I16 => 16,
            SampleKind::I24 | SampleKind::I24In32 => 24,
            SampleKind::I32 | SampleKind::F32 => 32,
        }
    }
    fn label(self) -> &'static str {
        match self {
            SampleKind::I16 => "16-bit",
            SampleKind::I24 => "24-bit",
            SampleKind::I24In32 => "24-bit (32 container)",
            SampleKind::I32 => "32-bit",
            SampleKind::F32 => "32-bit float",
        }
    }
}

struct Output {
    client: AudioClient,
    render: AudioRenderClient,
    event: Handle,
    kind: SampleKind,
    rate: u32,
    channels: usize,
    src_channels: usize,
    buffer_frames: u32,
    exclusive: bool,
    device_name: String,
    started: bool,
}

fn exclusive_candidates(bits: u32) -> Vec<SampleKind> {
    use SampleKind::*;
    match bits {
        0..=16 => vec![I16, I24In32, I24, I32],
        17..=24 => vec![I24In32, I24, I32, I16],
        _ => vec![I32, I24In32, I24, I16],
    }
}

fn open_output(
    device_id: Option<&str>,
    exclusive: bool,
    rate: u32,
    src_channels: usize,
    bits: u32,
) -> Result<Output, String> {
    let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    let device = match device_id {
        Some(id) => enumerator
            .get_device(id)
            .or_else(|_| enumerator.get_default_device(Direction::Render)),
        None => enumerator.get_default_device(Direction::Render),
    }
    .map_err(|e| format!("Không tìm thấy thiết bị âm thanh: {e}"))?;
    let device_name = device.get_friendlyname().unwrap_or_else(|_| "Unknown".into());
    let channels = if src_channels == 1 { 2 } else { src_channels };

    if exclusive {
        for kind in exclusive_candidates(bits) {
            let (store, valid) = match kind {
                SampleKind::I16 => (16, 16),
                SampleKind::I24 => (24, 24),
                SampleKind::I24In32 => (32, 24),
                _ => (32, 32),
            };
            let wf = WaveFormat::new(store, valid, SampleType::Int, rate as usize, channels, None);
            let Ok(mut client) = device.get_iaudioclient() else { continue };
            let Ok(wf) = client.is_supported_exclusive_with_quirks(&wf) else { continue };
            let (def_period, _min) = client.get_device_period().unwrap_or((100_000, 30_000));
            // ~30 ms buffer: low latency yet robust against scheduling hiccups.
            let want = (def_period * 3).max(200_000);
            let period = client
                .calculate_aligned_period_near(want, Some(128), &wf)
                .unwrap_or(want);
            let mode = StreamMode::EventsExclusive { period_hns: period };
            let ok = match client.initialize_client(&wf, Direction::Render, mode, None) {
                Ok(()) => true,
                Err(_) => {
                    // Possibly AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED: retry with the aligned size.
                    match client.get_buffer_size() {
                        Ok(frames) if frames > 0 => {
                            let p = calculate_period_100ns(frames as i64, rate as i64);
                            match device.get_iaudioclient() {
                                Ok(c) => {
                                    client = c;
                                    client
                                        .initialize_client(
                                            &wf,
                                            Direction::Render,
                                            StreamMode::EventsExclusive { period_hns: p },
                                            None,
                                        )
                                        .is_ok()
                                }
                                Err(_) => false,
                            }
                        }
                        _ => false,
                    }
                }
            };
            if !ok {
                continue;
            }
            let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
            let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
            let buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())?;
            return Ok(Output {
                client,
                render,
                event,
                kind,
                rate,
                channels,
                src_channels,
                buffer_frames,
                exclusive: true,
                device_name,
                started: false,
            });
        }
        // fall through to shared mode
    }

    let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;
    let wf = WaveFormat::new(32, 32, SampleType::Float, rate as usize, channels, None);
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 1_000_000 };
    client
        .initialize_client(&wf, Direction::Render, mode, None)
        .map_err(|e| format!("Không khởi tạo được thiết bị: {e}"))?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
    let buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())?;
    Ok(Output {
        client,
        render,
        event,
        kind: SampleKind::F32,
        rate,
        channels,
        src_channels,
        buffer_frames,
        exclusive: false,
        device_name,
        started: false,
    })
}

impl Output {
    fn info(&self, src_bits: u32) -> (String, bool) {
        let mode = if self.exclusive { "WASAPI Exclusive" } else { "WASAPI Shared" };
        let bp = self.exclusive && self.kind.valid_bits() >= src_bits && self.channels == self.src_channels;
        (
            format!("{mode} • {} / {} • {}", self.kind.label(), fmt_rate(self.rate), self.device_name),
            bp,
        )
    }

    fn compatible(&self, src: &Source) -> bool {
        self.rate == src.rate && self.src_channels == src.channels
    }

    /// Convert `frames` frames from the fifo (in source channel layout) to device bytes.
    fn convert(&self, fifo: &mut VecDeque<i32>, frames: usize, volume: f32, eq: &mut EqProcessor, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(frames * self.channels * self.kind.bytes());
        eq.set_format(self.rate, self.src_channels);
        let unity = volume >= 0.9999;
        let gain = (volume as f64).powi(2); // perceptual-ish curve
        let push = |s: i32, out: &mut Vec<u8>| {
            let s = if unity {
                s
            } else {
                (s as f64 * gain).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
            };
            match self.kind {
                SampleKind::I16 => out.extend_from_slice(&((s >> 16) as i16).to_le_bytes()),
                SampleKind::I24 => out.extend_from_slice(&(s >> 8).to_le_bytes()[..3]),
                SampleKind::I24In32 => out.extend_from_slice(&((s as u32 & 0xFFFF_FF00) as i32).to_le_bytes()),
                SampleKind::I32 => out.extend_from_slice(&s.to_le_bytes()),
                SampleKind::F32 => out.extend_from_slice(&(s as f32 / 2_147_483_648.0).to_le_bytes()),
            }
        };
        for _ in 0..frames {
            if self.src_channels == 1 {
                let raw = fifo.pop_front().unwrap_or(0);
                let s = eq.process_sample(raw, 0);
                push(s, out);
                push(s, out);
            } else {
                for ch in 0..self.src_channels {
                    let raw = fifo.pop_front().unwrap_or(0);
                    let s = eq.process_sample(raw, ch);
                    push(s, out);
                }
            }
        }
    }

    fn silence(&self, frames: usize, out: &mut Vec<u8>) {
        out.clear();
        out.resize(frames * self.channels * self.kind.bytes(), 0);
    }
}

// ---------------------------------------------------------------------------
// Engine loop
// ---------------------------------------------------------------------------

struct Engine {
    rx: Receiver<Command>,
    events: Sender<EngineEvent>,
    status: Arc<Mutex<EngineStatus>>,
    volume: Arc<AtomicU32>,
    repaint: Box<dyn Fn() + Send>,

    state: PlayState,
    source: Option<Source>,
    next_path: Option<PathBuf>,
    pending: Option<Source>,
    output: Option<Output>,
    eq: EqProcessor,
    fifo: VecDeque<i32>,
    bytes: Vec<u8>,
    track_frames: u64,
    /// Silence frames written after the end of the stream (used to detect drain).
    tail_silence: u64,
    device_id: Option<String>,
    exclusive: bool,
    error: Option<String>,
}

impl Engine {
    fn new(
        rx: Receiver<Command>,
        events: Sender<EngineEvent>,
        status: Arc<Mutex<EngineStatus>>,
        volume: Arc<AtomicU32>,
        repaint: Box<dyn Fn() + Send>,
    ) -> Self {
        Engine {
            rx,
            events,
            status,
            volume,
            repaint,
            state: PlayState::Stopped,
            source: None,
            next_path: None,
            pending: None,
            output: None,
            eq: EqProcessor::default(),
            fifo: VecDeque::new(),
            bytes: Vec::new(),
            track_frames: 0,
            tail_silence: 0,
            device_id: None,
            exclusive: false,
            error: None,
        }
    }

    fn emit(&self, ev: EngineEvent) {
        let _ = self.events.send(ev);
        (self.repaint)();
    }

    fn run(&mut self) {
        loop {
            // 1. Commands
            if self.state == PlayState::Playing {
                while let Ok(cmd) = self.rx.try_recv() {
                    if !self.handle(cmd) {
                        return;
                    }
                }
            } else {
                match self.rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(cmd) => {
                        if !self.handle(cmd) {
                            return;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                self.publish();
                continue;
            }
            if self.state != PlayState::Playing {
                continue;
            }

            // 2. Make sure the device is open in the right format
            if self.output.is_none() && !self.open_device() {
                continue;
            }

            // 3. Feed the device
            if let Err(e) = self.fill() {
                self.error = Some(format!("Lỗi thiết bị: {e}"));
                self.output = None;
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            self.publish();

            // 4. Wait for the device to ask for more
            if let Some(out) = &self.output {
                if out.event.wait_for_event(500).is_err() {
                    // Device lost or stalled: reopen on next iteration.
                    self.output = None;
                }
            }
        }
    }

    fn open_device(&mut self) -> bool {
        let Some(src) = &self.source else {
            self.state = PlayState::Stopped;
            return false;
        };
        match open_output(self.device_id.as_deref(), self.exclusive, src.rate, src.channels, src.bits) {
            Ok(out) => {
                self.output = Some(out);
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                self.state = PlayState::Stopped;
                self.publish();
                false
            }
        }
    }

    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                self.output = None;
                return false;
            }
            Command::Play { path, start_at } => self.start_track(&path, start_at),
            Command::Pause => {
                if self.state == PlayState::Playing {
                    if self.exclusive {
                        // In Exclusive mode, drop output handle so other apps (YouTube, games, Spotify)
                        // are immediately unblocked and do not get 'Audio renderer error'!
                        self.output = None;
                    } else if let Some(out) = &mut self.output {
                        let _ = out.client.stop_stream();
                        out.started = false;
                    }
                    self.state = PlayState::Paused;
                }
            }
            Command::Resume => {
                if self.state == PlayState::Paused {
                    self.state = PlayState::Playing;
                } else if self.state == PlayState::Stopped {
                    if let Some(p) = self.source.as_ref().map(|s| s.path.clone()) {
                        self.start_track(&p, 0.0);
                    }
                }
            }
            Command::Stop => {
                self.output = None;
                self.fifo.clear();
                if let Some(src) = &mut self.source {
                    src.seek(0.0);
                }
                self.track_frames = 0;
                self.state = PlayState::Stopped;
            }
            Command::Seek(secs) => {
                if let Some(src) = &mut self.source {
                    let pos = src.seek(secs);
                    self.fifo.clear();
                    self.eq.reset_state();
                    self.tail_silence = 0;
                    self.track_frames = (pos * src.rate as f64) as u64;
                    if let Some(out) = &mut self.output {
                        let _ = out.client.stop_stream();
                        let _ = out.client.reset_stream();
                        out.started = false;
                    }
                }
            }
            Command::SetNext(p) => {
                self.next_path = p;
                self.pending = None;
            }
            Command::SetOutput { device_id, exclusive } => {
                self.device_id = device_id;
                self.exclusive = exclusive;
                self.output = None; // reopened lazily with new settings
            }
            Command::SetEq(cfg) => {
                self.eq.update_config(cfg);
            }
            Command::ListDevices => {
                let list = list_devices();
                self.emit(EngineEvent::Devices(list));
            }
        }
        self.publish();
        true
    }

    fn start_track(&mut self, path: &Path, start_at: f64) {
        match Source::open(path) {
            Ok(mut src) => {
                let pos = if start_at > 0.0 { src.seek(start_at) } else { 0.0 };
                let reopen = match &self.output {
                    Some(out) => !out.compatible(&src) || out.kind.valid_bits() < src.bits.min(32),
                    None => true,
                };
                self.track_frames = (pos * src.rate as f64) as u64;
                self.eq.set_format(src.rate, src.channels);
                self.eq.reset_state();
                self.source = Some(src);
                self.pending = None;
                self.fifo.clear();
                self.tail_silence = 0;
                if reopen {
                    self.output = None;
                } else if let Some(out) = &mut self.output {
                    let _ = out.client.stop_stream();
                    let _ = out.client.reset_stream();
                    out.started = false;
                }
                self.state = PlayState::Playing;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e.clone());
                self.emit(EngineEvent::TrackFailed(path.to_path_buf(), e));
            }
        }
    }

    /// Open the queued next track (once) into `self.pending`.
    fn load_pending(&mut self) {
        if self.pending.is_some() {
            return;
        }
        if let Some(path) = self.next_path.take() {
            match Source::open(&path) {
                Ok(src) => self.pending = Some(src),
                Err(e) => self.emit(EngineEvent::TrackFailed(path, e)),
            }
        }
    }

    fn advance(&mut self) -> Option<Source> {
        self.load_pending();
        self.pending.take()
    }

    fn fill(&mut self) -> Result<(), String> {
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let Some(out) = &self.output else { return Ok(()) };
        let avail = out.client.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
        if avail == 0 {
            return Ok(());
        }
        let src_ch = out.src_channels;
        let need = avail * src_ch;

        let mut ended = false;
        while self.fifo.len() < need {
            let src = self.source.as_mut().unwrap();
            if src.decode_into(&mut self.fifo) {
                continue;
            }
            // End of current track: try gapless continuation.
            if let Some(next) = self.peek_next_compatible() {
                // Partially written samples of the old track count toward old track.
                self.track_frames = 0;
                let path = next.path.clone();
                self.source = Some(next);
                self.emit(EngineEvent::TrackChanged(path));
                continue;
            }
            ended = true;
            break;
        }

        let out = self.output.as_mut().unwrap();
        let frames = (self.fifo.len() / src_ch).min(avail);
        if frames > 0 {
            out.convert(&mut self.fifo, frames, volume, &mut self.eq, &mut self.bytes);
            out.render.write_to_device(&self.bytes, None).map_err(|e| e.to_string())?;
            self.track_frames += frames as u64;
        }
        if frames < avail && ended {
            let pad = avail - frames;
            out.silence(pad, &mut self.bytes);
            out.render.write_to_device(&self.bytes, None).map_err(|e| e.to_string())?;
            self.tail_silence += pad as u64;
        }
        if !out.started {
            out.client.start_stream().map_err(|e| e.to_string())?;
            out.started = true;
        }

        if ended && self.tail_silence >= out.buffer_frames as u64 {
            // Everything real has been played.
            self.tail_silence = 0;
            if let Some(next) = self.advance() {
                let path = next.path.clone();
                let compatible = self.output.as_ref().map(|o| o.compatible(&next)).unwrap_or(false);
                if !compatible {
                    self.output = None;
                }
                self.source = Some(next);
                self.track_frames = 0;
                self.fifo.clear();
                self.emit(EngineEvent::TrackChanged(path));
            } else {
                self.output = None;
                self.state = PlayState::Stopped;
                if let Some(src) = &mut self.source {
                    src.seek(0.0);
                }
                self.track_frames = 0;
                self.emit(EngineEvent::Finished);
            }
        }
        Ok(())
    }

    /// Open the queued next track only if it can be played gaplessly on the current device.
    fn peek_next_compatible(&mut self) -> Option<Source> {
        self.load_pending();
        let out = self.output.as_ref()?;
        let src = self.pending.as_ref()?;
        if out.compatible(src) && out.kind.valid_bits() >= src.bits.min(32) {
            self.pending.take()
        } else {
            None // handled after the drain (device reopen)
        }
    }

    fn publish(&self) {
        let mut st = self.status.lock().unwrap();
        st.state = self.state;
        st.error = self.error.clone();
        if let Some(src) = &self.source {
            st.path = Some(src.path.clone());
            st.duration = src.duration;
            st.source_info = src.info();
            let latency = self
                .output
                .as_ref()
                .filter(|o| o.started)
                .and_then(|o| o.client.get_current_padding().ok())
                .unwrap_or(0) as u64;
            let latency = latency.saturating_sub(self.tail_silence);
            let frames = self.track_frames.saturating_sub(latency);
            let mut pos = frames as f64 / src.rate as f64;
            if src.duration > 0.0 {
                pos = pos.min(src.duration);
            }
            st.position = pos;
        } else {
            st.path = None;
            st.position = 0.0;
            st.duration = 0.0;
            st.source_info.clear();
        }
        match (&self.output, &self.source) {
            (Some(out), Some(src)) => {
                let (info, bp) = out.info(src.bits);
                let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
                st.output_info = info;
                st.bit_perfect = bp && vol >= 0.9999 && !self.eq.is_enabled();
            }
            _ => {
                st.output_info = if self.exclusive {
                    "WASAPI Exclusive (chờ phát)".into()
                } else {
                    "WASAPI Shared (chờ phát)".into()
                };
                st.bit_perfect = false;
            }
        }
    }
}

fn list_devices() -> Vec<DeviceInfo> {
    let mut v = Vec::new();
    if let Ok(en) = DeviceEnumerator::new() {
        if let Ok(coll) = en.get_device_collection(Direction::Render) {
            for dev in &coll {
                if let Ok(dev) = dev {
                    if let (Ok(id), Ok(name)) = (dev.get_id(), dev.get_friendlyname()) {
                        v.push(DeviceInfo { id, name });
                    }
                }
            }
        }
    }
    v
}
