use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

const SAMPLE_RATE: usize = 48_000;
const FRAMES: usize = 2048;
const HOP: usize = 512;
const BANDS: usize = 32;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;
const STALL_AFTER: Duration = Duration::from_millis(600);
const STALL_MESSAGE: &str = "Audio stalled";

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SampleFormat {
    #[value(name = "f32le")]
    F32,
    #[value(name = "s16le")]
    S16,
    #[value(name = "s32le")]
    S32,
}

impl SampleFormat {
    pub fn bytes(self) -> usize {
        match self {
            Self::F32 | Self::S32 => 4,
            Self::S16 => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32le",
            Self::S16 => "s16le",
            Self::S32 => "s32le",
        }
    }

    fn pipewire(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::S16 => "s16",
            Self::S32 => "s32",
        }
    }

    fn pulse(self) -> &'static str {
        match self {
            Self::F32 => "float32le",
            Self::S16 => "s16le",
            Self::S32 => "s32le",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub rate: u32,
    pub channels: u16,
    pub sample: SampleFormat,
}

impl Default for PcmFormat {
    fn default() -> Self {
        Self {
            rate: SAMPLE_RATE as u32,
            channels: 2,
            sample: SampleFormat::F32,
        }
    }
}

impl PcmFormat {
    pub fn frame_bytes(self) -> usize {
        self.channels as usize * self.sample.bytes()
    }

    pub fn validate(self) -> io::Result<Self> {
        if self.rate == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audio sample rate must be greater than zero",
            ));
        }
        if self.channels == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audio channel count must be greater than zero",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub enum Backend {
    Auto,
    PipeWire,
    Pulse,
    Microphone,
    Command(String),
    File(PathBuf),
}

impl Backend {
    /// Live sources can be restarted without surprising the listener. Files
    /// and one-shot commands are left alone after they end.
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            Self::Auto | Self::PipeWire | Self::Pulse | Self::Microphone
        )
    }
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub backend: Backend,
    pub target: Option<String>,
    pub format: PcmFormat,
    pub loop_file: bool,
}

/// Build a capture config from typed audio settings. An empty command/file
/// with an explicit "command"/"file" source degrades to `auto` rather than
/// spawning an empty shell or opening an empty path. A non-empty command or
/// file does not override an explicit source.
pub fn capture_config(audio: &crate::config::AudioConfig) -> CaptureConfig {
    use clap::ValueEnum;
    let backend = match audio.source.as_str() {
        "pipewire" => Backend::PipeWire,
        "pulse" => Backend::Pulse,
        "microphone" => Backend::Microphone,
        "command" if !audio.command.is_empty() => Backend::Command(audio.command.clone()),
        "file" if !audio.file.is_empty() => Backend::File(PathBuf::from(&audio.file)),
        _ => Backend::Auto,
    };
    let format = PcmFormat {
        rate: audio.rate,
        channels: audio.channels,
        sample: SampleFormat::from_str(&audio.format, true).unwrap_or(SampleFormat::F32),
    };
    CaptureConfig {
        backend,
        target: (!audio.target.is_empty()).then(|| audio.target.clone()),
        format,
        loop_file: audio.loop_file,
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceInfo {
    pub kind: String,
    pub target: Option<String>,
    pub rate: u32,
    pub channels: u16,
    pub format: String,
}

impl SourceInfo {
    pub fn describe(&self) -> String {
        let mut text = self.kind.clone();
        if let Some(target) = &self.target {
            if !target.is_empty() {
                text.push_str(" · ");
                text.push_str(target);
            }
        }
        if self.rate > 0 {
            text.push_str(&format!(" · {} Hz · {} ch", self.rate, self.channels));
        }
        if !self.format.is_empty() {
            text.push_str(" · ");
            text.push_str(&self.format);
        }
        text
    }
}

#[derive(Clone, Debug, Default)]
pub struct AudioState {
    pub bands: [f32; 32],
    /// Recent signed waveform, normalized by the loudness reference and the
    /// sine crest factor, so steady material fills [-1, 1] without clipping.
    pub waveform: [f32; 32],
    pub beat: f32,
    /// Fraction of the current beat, in [0, 1).
    pub beat_phase: f32,
    /// Adaptive perceived loudness, rather than raw sample amplitude.
    pub energy: f32,
    pub rms: f32,
    pub peak: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    /// Octave-folded peak power, C through B (A440 tuning), from ~65–4000 Hz.
    /// Sums to 1 when audible peaks are present; below-gate input is zero.
    /// Approximate spectral pitch classes, not a key or chord estimate.
    pub chroma: [f32; 12],
    /// Power-weighted spectral centroid, log-scaled: 0 = 40 Hz, 1 = 16 kHz.
    /// Typical bass tones are near 0.15, 1 kHz near 0.54; silence is 0.
    pub centroid: f32,
    /// Geometric/arithmetic power ratio over 40 Hz–16 kHz: tone ~0, noise ~0.56.
    /// Bounded to [0, 1]; silence and below-gate input are 0.
    pub flatness: f32,
    /// Side/mid energy ratio in [0, 1]. Zero for mono or identical channels,
    /// about 0.5 for uncorrelated stereo, and near 1 for anti-phase material.
    /// Read from the raw channels, so material that cancels in the mono
    /// downmix still reports a width.
    pub width: f32,
    /// Right-minus-left loudness balance in [-1, 1]; zero when centred.
    pub balance: f32,
    pub onset: f32,
    pub kick: f32,
    /// Independent low/bass, low-mid/snare, presence, and air attack envelopes.
    pub transients: [f32; 4],
    /// Fast, normalized continuous spectral novelty, independent of tempo lock.
    pub novelty: f32,
    /// Periodicity estimate; half/double-tempo ambiguity is possible.
    pub bpm: Option<f32>,
    pub tempo_confidence: f32,
    pub updated: Option<Instant>,
    pub connected: bool,
    pub error: Option<String>,
    /// Where the samples came from, for diagnostics and `--check-audio`.
    pub source: SourceInfo,
    /// Time since the last block above the noise gate. A live pipe can be silent.
    pub silent_for: f32,
}

pub struct Capture {
    child: Option<Child>,
    shared: Arc<Mutex<AudioState>>,
    started: Instant,
    stop: Arc<AtomicBool>,
}

struct OpenedSource {
    child: Option<Child>,
    reader: Box<dyn Read + Send>,
    stderr: Arc<Mutex<String>>,
    info: SourceInfo,
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

fn stderr_text(stderr: &Arc<Mutex<String>>) -> String {
    stderr
        .lock()
        .ok()
        .map(|text| {
            text.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}

fn spawn_stderr(mut stderr: std::process::ChildStderr) -> Arc<Mutex<String>> {
    let text = Arc::new(Mutex::new(String::new()));
    let worker_text = Arc::clone(&text);
    let _ = thread::Builder::new()
        .name("audio-stderr".into())
        .spawn(move || {
            let mut buffer = [0_u8; 512];
            while let Ok(size) = stderr.read(&mut buffer) {
                if size == 0 {
                    break;
                }
                if let Ok(mut text) = worker_text.lock() {
                    text.push_str(&String::from_utf8_lossy(&buffer[..size]));
                    if text.len() > 4096 {
                        let keep = text.len() - 4096;
                        text.drain(..keep);
                    }
                }
            }
        });
    text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureKind {
    Output,
    Microphone,
}

impl CaptureKind {
    fn device_kind(self) -> &'static str {
        match self {
            Self::Output => "sink",
            Self::Microphone => "source",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Output => "default sink monitor",
            Self::Microphone => "default microphone",
        }
    }

    fn source_kind(self) -> &'static str {
        match self {
            Self::Output => "PipeWire",
            Self::Microphone => "PipeWire microphone",
        }
    }

    fn pulse_device(self) -> &'static str {
        match self {
            Self::Output => "@DEFAULT_MONITOR@",
            Self::Microphone => "@DEFAULT_SOURCE@",
        }
    }
}

fn default_device(kind: CaptureKind) -> Option<String> {
    let output = Command::new("pw-dump").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_pw_dump(&output.stdout)
        .ok()?
        .into_iter()
        .find(|device| device.kind == kind.device_kind() && device.default)
        .map(|device| device.name)
}

fn pipewire(config: &CaptureConfig, kind: CaptureKind) -> io::Result<OpenedSource> {
    let format = config.format.validate()?;
    let mut command = Command::new("pw-record");
    command.args([
        "--raw",
        "--format",
        format.sample.pipewire(),
        "--rate",
        &format.rate.to_string(),
        "--channels",
        &format.channels.to_string(),
        "--latency",
        "256",
    ]);
    if kind == CaptureKind::Output {
        command.args(["--properties", "{ stream.capture.sink = true }"]);
    }
    if let Some(target) = &config.target {
        command.arg("--target").arg(target);
    }
    let mut child = command
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = child.stdout.take().expect("piped capture stdout");
    let stderr = child.stderr.take().map(spawn_stderr).unwrap_or_default();
    Ok(OpenedSource {
        child: Some(child),
        reader: Box::new(reader),
        stderr,
        info: SourceInfo {
            kind: kind.source_kind().into(),
            target: config
                .target
                .clone()
                .or_else(|| default_device(kind))
                .or_else(|| Some(kind.label().into())),
            rate: format.rate,
            channels: format.channels,
            format: format.sample.name().into(),
        },
    })
}

fn pulse(config: &CaptureConfig, kind: CaptureKind) -> io::Result<OpenedSource> {
    let format = config.format.validate()?;
    let device = config
        .target
        .clone()
        .unwrap_or_else(|| kind.pulse_device().into());
    let mut child = Command::new("parec")
        .arg(format!("--format={}", format.sample.pulse()))
        .arg(format!("--rate={}", format.rate))
        .arg(format!("--channels={}", format.channels))
        .arg(format!("--device={device}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = child.stdout.take().expect("piped capture stdout");
    let stderr = child.stderr.take().map(spawn_stderr).unwrap_or_default();
    Ok(OpenedSource {
        child: Some(child),
        reader: Box::new(reader),
        stderr,
        info: SourceInfo {
            kind: if kind == CaptureKind::Output {
                "PulseAudio".into()
            } else {
                "PulseAudio microphone".into()
            },
            target: Some(device),
            rate: format.rate,
            channels: format.channels,
            format: format.sample.name().into(),
        },
    })
}

fn microphone(config: &CaptureConfig) -> io::Result<OpenedSource> {
    if which("pw-record") {
        pipewire(config, CaptureKind::Microphone)
    } else if which("parec") {
        pulse(config, CaptureKind::Microphone)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no supported audio capture tool found; install PipeWire (pw-record) or PulseAudio (parec)",
        ))
    }
}

fn command_source(command: &str, format: PcmFormat) -> io::Result<OpenedSource> {
    let format = format.validate()?;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = child.stdout.take().expect("piped command stdout");
    let stderr = child.stderr.take().map(spawn_stderr).unwrap_or_default();
    Ok(OpenedSource {
        child: Some(child),
        reader: Box::new(reader),
        stderr,
        info: SourceInfo {
            kind: "command".into(),
            target: Some(command.to_owned()),
            rate: format.rate,
            channels: format.channels,
            format: format.sample.name().into(),
        },
    })
}

struct LoopingFile {
    file: File,
    len: u64,
}

impl LoopingFile {
    fn open(path: &PathBuf) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }
}

impl Read for LoopingFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.file.read(buffer)?;
        if read == 0 && self.len > 0 {
            self.file.seek(SeekFrom::Start(0))?;
            self.file.read(buffer)
        } else {
            Ok(read)
        }
    }
}

fn file_source(path: &PathBuf, format: PcmFormat, loop_file: bool) -> io::Result<OpenedSource> {
    let format = format.validate()?;
    let reader: Box<dyn Read + Send> = if loop_file {
        Box::new(LoopingFile::open(path)?)
    } else {
        Box::new(File::open(path)?)
    };
    Ok(OpenedSource {
        child: None,
        reader,
        stderr: Arc::new(Mutex::new(String::new())),
        info: SourceInfo {
            kind: "file".into(),
            target: Some(path.display().to_string()),
            rate: format.rate,
            channels: format.channels,
            format: format.sample.name().into(),
        },
    })
}

fn open_source(config: &CaptureConfig) -> io::Result<OpenedSource> {
    match &config.backend {
        Backend::PipeWire => pipewire(config, CaptureKind::Output),
        Backend::Pulse => pulse(config, CaptureKind::Output),
        Backend::Microphone => microphone(config),
        Backend::Command(command) => command_source(command, config.format),
        Backend::File(path) => file_source(path, config.format, config.loop_file),
        Backend::Auto => {
            if which("pw-record") {
                pipewire(config, CaptureKind::Output)
            } else if which("parec") {
                pulse(config, CaptureKind::Output)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no supported audio capture tool found; install PipeWire (pw-record) or PulseAudio (parec), or use --audio-command",
                ))
            }
        }
    }
}

/// Per-hop stereo energy. Mono input yields identical left and right energy.
#[derive(Clone, Copy, Debug, Default)]
struct StereoStats {
    left: f32,
    right: f32,
    cross: f32,
}

impl StereoStats {
    /// Side/mid energy ratio: 0 for mono or identical channels, about 0.5 for
    /// uncorrelated stereo, and 1 for anti-phase material.
    fn width(self) -> f32 {
        let total = self.left + self.right;
        if total <= 1e-12 {
            return 0.0;
        }
        ((total - 2.0 * self.cross).max(0.0) / (2.0 * total)).clamp(0.0, 1.0)
    }

    /// Right-minus-left loudness balance in [-1, 1].
    fn balance(self) -> f32 {
        let left = self.left.sqrt();
        let right = self.right.sqrt();
        let total = left + right;
        if total <= 1e-12 {
            return 0.0;
        }
        ((right - left) / total).clamp(-1.0, 1.0)
    }
}

fn decode_pcm(bytes: &[u8], format: PcmFormat, samples: &mut [f32; HOP]) -> StereoStats {
    let frame_bytes = format.frame_bytes();
    let channels = format.channels as usize;
    let mut stereo = StereoStats::default();
    let mut frames = 0.0;
    for (sample, frame) in samples.iter_mut().zip(bytes.chunks_exact(frame_bytes)) {
        let mut sum = 0.0;
        let mut left = 0.0;
        let mut right_value = None;
        for channel in 0..channels {
            let offset = channel * format.sample.bytes();
            let value = match format.sample {
                SampleFormat::F32 => {
                    f32::from_le_bytes(frame[offset..offset + 4].try_into().expect("f32 sample"))
                }
                SampleFormat::S16 => {
                    i16::from_le_bytes(frame[offset..offset + 2].try_into().expect("s16 sample"))
                        as f32
                        / 32_768.0
                }
                SampleFormat::S32 => {
                    i32::from_le_bytes(frame[offset..offset + 4].try_into().expect("s32 sample"))
                        as f32
                        / 2_147_483_648.0
                }
            };
            if value.is_finite() {
                sum += value;
                match channel {
                    0 => left = value,
                    1 => right_value = Some(value),
                    _ => {}
                }
            }
        }
        *sample = sum / channels as f32;
        // Mono input has identical left and right energy, so a mono source
        // reads as centred rather than hard left.
        let right = right_value.unwrap_or(left);
        stereo.left += left * left;
        stereo.right += right * right;
        stereo.cross += left * right;
        frames += 1.0;
    }
    if frames > 0.0 {
        stereo.left /= frames;
        stereo.right /= frames;
        stereo.cross /= frames;
    }
    stereo
}

impl Capture {
    /// Open the configured source and start the analysis worker.
    ///
    /// The returned capture samples the source on its own thread. Dropping it
    /// stops the worker's source; the detached reader then exits.
    pub fn start(config: CaptureConfig) -> io::Result<Self> {
        let OpenedSource {
            child,
            mut reader,
            stderr,
            info,
        } = open_source(&config)?;
        let started = Instant::now();
        let source = info;
        let state = Arc::new(Mutex::new(AudioState {
            source: source.clone(),
            ..Default::default()
        }));
        let worker_state = Arc::clone(&state);
        let worker_stderr = Arc::clone(&stderr);
        let worker_info = source;
        let format = config.format;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let mut analyzer = Analyzer::with_sample_rate(format.rate as usize);
                let mut bytes = vec![0_u8; HOP * format.frame_bytes()];
                let mut samples = [0.0_f32; HOP];
                loop {
                    // A dropped capture stops a looping file reader, which
                    // otherwise keeps decoding into state nobody reads.
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(error) = reader.read_exact(&mut bytes) {
                        let detail = stderr_text(&worker_stderr);
                        let base = if error.kind() == io::ErrorKind::UnexpectedEof {
                            "Audio capture ended"
                        } else {
                            "Audio capture failed"
                        };
                        let message = if detail.is_empty() {
                            format!("{base}: {error}")
                        } else {
                            format!("{base}: {detail}")
                        };
                        if let Ok(mut state) = worker_state.lock() {
                            *state = AudioState {
                                source: worker_info.clone(),
                                error: Some(message),
                                ..Default::default()
                            };
                        }
                        break;
                    }
                    let stereo = decode_pcm(&bytes, format, &mut samples);
                    let updated = Instant::now();
                    let mut analyzed = analyzer.analyze_with(&samples, stereo);
                    analyzed.updated = Some(updated);
                    analyzed.connected = true;
                    analyzed.source = worker_info.clone();
                    if let Ok(mut state) = worker_state.lock() {
                        *state = analyzed;
                    } else {
                        break;
                    }
                }
            });
        if let Err(error) = worker {
            if let Some(mut child) = child {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(error);
        }
        Ok(Self {
            child,
            shared: state,
            started,
            stop,
        })
    }

    /// Clone the latest analyzer state for display.
    ///
    /// A capture that has not produced a block for 600 ms is replaced with an
    /// error state, so a stalled reader cannot freeze the visuals on old
    /// audio. The source is preserved so the UI can still name it.
    pub fn snapshot(&self) -> AudioState {
        let mut state = self
            .shared
            .lock()
            .ok()
            .map(|state| state.clone())
            .unwrap_or_default();
        let stale = state
            .updated
            .is_some_and(|updated| updated.elapsed() > STALL_AFTER);
        let empty = state.updated.is_none() && self.started.elapsed() > STALL_AFTER;
        if stale || empty {
            state = AudioState {
                source: state.source,
                error: Some(if empty {
                    "Audio capture produced no samples".into()
                } else {
                    STALL_MESSAGE.into()
                }),
                ..Default::default()
            };
        }
        state
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Killing the writer releases a reader blocked on a pipe; a file reader
        // sees the stop flag within one hop.
    }
}

const RETRY_INITIAL: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(4);

struct Retry {
    next: Option<Instant>,
    delay: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            next: None,
            delay: RETRY_INITIAL,
        }
    }
}

impl Retry {
    fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    fn fail(&mut self, now: Instant) {
        self.next = Some(now + self.delay);
        self.delay = (self.delay * 2).min(RETRY_MAX);
    }

    fn success(&mut self) {
        self.next = None;
        self.delay = RETRY_INITIAL;
    }
}

/// Supervises a live capture and restarts dropped sources with a bounded
/// backoff. Failures stay out of the UI; the analyser simply reports silence.
pub struct AudioLink {
    config: CaptureConfig,
    capture: Option<Capture>,
    retry: Retry,
    started: bool,
    retry_allowed: bool,
}

impl AudioLink {
    pub fn new(config: CaptureConfig, auto_reconnect: bool) -> Self {
        let retry_allowed = auto_reconnect && config.backend.is_live();
        Self {
            config,
            capture: None,
            retry: Retry::default(),
            started: false,
            retry_allowed,
        }
    }

    pub fn snapshot(&mut self, now: Instant) -> AudioState {
        if self.capture.is_none() {
            if self.started && (!self.retry_allowed || !self.retry.due(now)) {
                return AudioState::default();
            }
            self.started = true;
            match Capture::start(self.config.clone()) {
                Ok(capture) => {
                    self.capture = Some(capture);
                    self.retry.success();
                }
                Err(error) => {
                    if self.retry_allowed {
                        self.retry.fail(now);
                    }
                    return AudioState {
                        error: Some(error.to_string()),
                        ..Default::default()
                    };
                }
            }
        }
        let state = self
            .capture
            .as_ref()
            .map(Capture::snapshot)
            .unwrap_or_default();
        if state.error.is_some() {
            self.capture = None;
            if self.retry_allowed {
                self.retry.fail(now);
            }
        } else if state.connected {
            self.retry.success();
        }
        state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub kind: &'static str,
    pub name: String,
    pub description: String,
    pub serial: Option<u64>,
    pub channels: Option<u64>,
    pub rate: Option<u64>,
    pub default: bool,
}

/// List PipeWire sink and source nodes, marking the default sink and source.
/// The output is intentionally based on `pw-dump` rather than a guessed node name.
pub fn list_audio() -> io::Result<Vec<AudioDevice>> {
    let pipewire_error = match Command::new("pw-dump").output() {
        Ok(output) if output.status.success() => match parse_pw_dump(&output.stdout) {
            Ok(devices) => return Ok(devices),
            Err(error) => format!("could not parse pw-dump output: {error}"),
        },
        Ok(output) => format!("pw-dump failed with {}", output.status),
        Err(error) => format!("could not run pw-dump: {error}"),
    };
    match list_pulse() {
        Ok(devices) => Ok(devices),
        Err(error) => Err(io::Error::other(format!(
            "{pipewire_error}; {error}; install PipeWire or PulseAudio, or use --audio-command"
        ))),
    }
}

fn list_pulse() -> io::Result<Vec<AudioDevice>> {
    let info = Command::new("pactl").arg("info").output()?;
    if !info.status.success() {
        return Err(io::Error::other("pactl info failed"));
    }
    let info = String::from_utf8_lossy(&info.stdout);
    let default_sink = info
        .lines()
        .find_map(|line| line.strip_prefix("Default Sink: "))
        .map(str::to_owned);
    let default_source = info
        .lines()
        .find_map(|line| line.strip_prefix("Default Source: "))
        .map(str::to_owned);
    let mut devices = Vec::new();
    for (kind, default) in [("sink", &default_sink), ("source", &default_source)] {
        let output = Command::new("pactl")
            .args(["list", "short", &format!("{kind}s")])
            .output()?;
        if !output.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() < 2 {
                continue;
            }
            let name = fields[1].to_owned();
            let serial = fields[0].parse::<u64>().ok();
            let mut channels = None;
            let mut rate = None;
            if let Some(spec) = fields.get(3) {
                for token in spec.split_whitespace() {
                    if let Some(value) = token.strip_suffix("ch") {
                        channels = value.parse().ok();
                    } else if let Some(value) = token.strip_suffix("Hz") {
                        rate = value.parse().ok();
                    }
                }
            }
            devices.push(AudioDevice {
                kind,
                description: name.clone(),
                default: default.as_deref() == Some(name.as_str()),
                name,
                serial,
                channels,
                rate,
            });
        }
    }
    devices.sort_by_key(|device| (device.kind != "sink", device.name.clone()));
    Ok(devices)
}

fn parse_pw_dump(bytes: &[u8]) -> Result<Vec<AudioDevice>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let Some(objects) = value.as_array() else {
        return Ok(Vec::new());
    };
    let mut defaults = std::collections::HashMap::new();
    for object in objects {
        if object.get("type").and_then(|v| v.as_str()) != Some("PipeWire:Interface:Metadata") {
            continue;
        }
        if object
            .pointer("/props/metadata.name")
            .and_then(|v| v.as_str())
            != Some("default")
        {
            continue;
        }
        if let Some(entries) = object.get("metadata").and_then(|v| v.as_array()) {
            for entry in entries {
                let Some(key) = entry.get("key").and_then(|v| v.as_str()) else {
                    continue;
                };
                if key != "default.audio.sink" && key != "default.audio.source" {
                    continue;
                }
                let value = entry.get("value");
                let name = value
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| value.and_then(|v| v.as_str()));
                if let Some(name) = name {
                    defaults.insert(
                        if key.ends_with("sink") {
                            "sink"
                        } else {
                            "source"
                        },
                        name.to_owned(),
                    );
                }
            }
        }
    }
    let mut devices = Vec::new();
    for object in objects {
        if object.get("type").and_then(|v| v.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = object.get("info").and_then(|v| v.get("props"));
        let Some(props) = props else { continue };
        let media_class = props
            .get("media.class")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let kind = match media_class {
            "Audio/Sink" => "sink",
            "Audio/Source" => "source",
            _ => continue,
        };
        let name = props
            .get("node.name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let description = props
            .get("node.description")
            .and_then(|v| v.as_str())
            .unwrap_or(name);
        let serial = props.get("object.serial").and_then(|v| v.as_u64());
        let channels = props.get("audio.channels").and_then(|v| v.as_u64());
        let rate = props.get("audio.rate").and_then(|v| v.as_u64());
        devices.push(AudioDevice {
            kind,
            name: name.to_owned(),
            description: description.to_owned(),
            serial,
            channels,
            rate,
            default: defaults.get(kind).map(String::as_str) == Some(name),
        });
    }
    devices.sort_by_key(|device| (device.kind != "sink", device.name.clone()));
    Ok(devices)
}

// Envelopes run on the audio clock, independent of render rate or pipe scheduling.
#[cfg(test)]
const DT: f32 = HOP as f32 / SAMPLE_RATE as f32;
const NOISE_GATE: f32 = 0.0015;
const REF_MIN: f32 = 0.025;
const REF_MAX: f32 = 0.5;
const REFRACTORY: f32 = 0.032;

fn follow(current: f32, target: f32, attack: f32, release: f32, dt: f32) -> f32 {
    let tau = if target > current { attack } else { release };
    current + (target - current) * (1.0 - (-dt / tau).exp())
}

struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    ranges: [(usize, usize); BANDS],
    samples: [f32; FRAMES],
    band_reference: [f32; BANDS],
    band_previous: [f32; BANDS],
    attack_mean: [f32; 4],
    attack_previous: [f32; 4],
    attack_cooldown: [f32; 4],
    state: AudioState,
    reference: f32,
    primed: bool,
    quiet_time: f32,
    dt: f32,
    sample_rate: f32,
    rhythm: Rhythm,
}

impl Analyzer {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_sample_rate(SAMPLE_RATE)
    }

    fn with_sample_rate(sample_rate: usize) -> Self {
        let sample_rate = sample_rate.max(1) as f32;
        let dt = HOP as f32 / sample_rate;
        let fft = FftPlanner::<f32>::new().plan_fft_forward(FRAMES);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..FRAMES)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAMES as f32).cos())
            .collect();
        // Bands above Nyquist collapse to an empty range instead of indexing
        // past the FFT output when the source rate is low. Below about 120 Hz
        // the log edges are narrower than one FFT bin, so each range starts
        // where the previous one ended: every band owns distinct bins instead
        // of three adjacent bass bands reading the same bin.
        let nyquist_bin = FRAMES / 2;
        let mut ranges = [(0, 0); BANDS];
        let mut previous_end = 1;
        for (i, range) in ranges.iter_mut().enumerate() {
            let low = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f32 / BANDS as f32);
            let high = LOW_HZ * (HIGH_HZ / LOW_HZ).powf((i + 1) as f32 / BANDS as f32);
            let start = ((low * FRAMES as f32 / sample_rate).ceil() as usize)
                .max(previous_end)
                .min(nyquist_bin);
            let end = ((high * FRAMES as f32 / sample_rate).ceil() as usize)
                .max(start + 1)
                .min(nyquist_bin);
            *range = (start, end);
            previous_end = end;
        }
        Self {
            fft,
            scratch,
            window,
            ranges,
            buffer: vec![Complex::default(); FRAMES],
            samples: [0.0; FRAMES],
            band_reference: [0.01; BANDS],
            band_previous: [0.0; BANDS],
            attack_mean: [0.0; 4],
            attack_previous: [0.0; 4],
            attack_cooldown: [0.0; 4],
            state: AudioState::default(),
            reference: 0.1,
            primed: false,
            quiet_time: 0.0,
            dt,
            sample_rate,
            rhythm: Rhythm::new(dt),
        }
    }

    fn analyze_spectral_shape(&mut self, audible: bool) {
        self.state.chroma = [0.0; 12];
        self.state.centroid = 0.0;
        self.state.flatness = 0.0;
        if !audible {
            return;
        }
        let bin_hz = self.sample_rate / FRAMES as f32;
        let nyquist_bin = FRAMES / 2;
        let start = ((LOW_HZ / bin_hz).ceil() as usize).min(nyquist_bin - 1);
        let end = ((HIGH_HZ / bin_hz).ceil() as usize).clamp(start + 1, nyquist_bin);
        let mut powers = [0.0_f32; FRAMES / 2];
        let mut total = 0.0;
        let mut weighted = 0.0;
        let mut log_sum = 0.0;
        let mut maximum = 0.0_f32;
        for (i, power) in powers.iter_mut().enumerate().take(end).skip(start) {
            *power = self.buffer[i].norm_sqr() * (2.0 / FRAMES as f32).powi(2);
            total += *power;
            weighted += *power * i as f32 * bin_hz;
            maximum = maximum.max(*power);
            log_sum += power.max(1e-20).ln();
        }
        // Fixed numerical floor, never an adaptive gain for quiet noise.
        if total <= 1e-12 {
            return;
        }
        self.state.centroid =
            ((weighted / total / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln()).clamp(0.0, 1.0);
        let count = (end - start) as f32;
        self.state.flatness = ((log_sum / count).exp() / (total / count)).clamp(0.0, 1.0);
        // Interpolate local peaks before octave folding: raw 23.4 Hz FFT bins
        // would misclassify bass notes (110 Hz is between the A and A# bins).
        // The Hann main-lobe's three-bin power preserves each peak's energy.
        for i in start + 1..end - 1 {
            let hz = i as f32 * bin_hz;
            if !(65.0..=4000.0).contains(&hz)
                || powers[i] < maximum * 1e-4
                || powers[i] <= powers[i - 1]
                || powers[i] <= powers[i + 1]
            {
                continue;
            }
            let left = powers[i - 1].max(1e-20).ln();
            let center = powers[i].max(1e-20).ln();
            let right = powers[i + 1].max(1e-20).ln();
            let curvature = left - 2.0 * center + right;
            let offset = if curvature < -1e-6 {
                (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let frequency = (i as f32 + offset) * bin_hz;
            let pitch = (69.0 + 12.0 * (frequency / 440.0).log2()).rem_euclid(12.0);
            let lower = pitch.floor() as usize;
            let fraction = pitch.fract();
            let power = powers[i - 1] + powers[i] + powers[i + 1];
            self.state.chroma[lower] += power * (1.0 - fraction);
            self.state.chroma[(lower + 1) % 12] += power * fraction;
        }
        let chroma_total = self.state.chroma.iter().sum::<f32>();
        if chroma_total > 1e-12 {
            for value in &mut self.state.chroma {
                *value /= chroma_total;
            }
        }
    }

    /// Mono analysis for tests; capture passes per-hop stereo statistics.
    #[cfg(test)]
    fn analyze(&mut self, samples: &[f32; HOP]) -> AudioState {
        self.analyze_with(samples, StereoStats::default())
    }

    fn analyze_with(&mut self, samples: &[f32; HOP], stereo: StereoStats) -> AudioState {
        self.samples.copy_within(HOP.., 0);
        self.samples[FRAMES - HOP..].copy_from_slice(samples);
        let mut squares = 0.0;
        let mut peak = 0.0_f32;
        for sample in samples {
            squares += sample * sample;
            peak = peak.max(sample.abs());
        }
        for ((bin, window), sample) in self.buffer.iter_mut().zip(&self.window).zip(&self.samples) {
            *bin = Complex::new(sample * window, 0.0);
        }
        let rms = (squares / HOP as f32).sqrt();
        let audible = rms > NOISE_GATE;
        self.quiet_time = if audible {
            0.0
        } else {
            self.quiet_time + self.dt
        };
        self.state.silent_for = self.quiet_time;
        // Stereo metrics come from the raw channels, so they are gated on the
        // channel energy rather than the mono downmix. Anti-phase material
        // cancels in the downmix but still has a width.
        let channel_energy = (stereo.left + stereo.right) * 0.5;
        let (width, balance) = if channel_energy > NOISE_GATE * NOISE_GATE {
            (stereo.width(), stereo.balance())
        } else {
            (0.0, 0.0)
        };
        self.state.width = follow(self.state.width, width, 0.15, 0.4, self.dt);
        self.state.balance = follow(self.state.balance, balance, 0.1, 0.3, self.dt);
        // Freeze adaptation below the gate: silence must never turn into extra gain.
        // The first audible hop seeds the reference instead of easing down from
        // 0.1, which otherwise leaves the display over- or under-driven for
        // several seconds after every start and every source change.
        if audible {
            if !self.primed {
                self.reference = rms.clamp(REF_MIN, REF_MAX);
                self.primed = true;
            }
            self.reference = follow(
                self.reference,
                rms.clamp(REF_MIN, REF_MAX),
                2.0,
                6.0,
                self.dt,
            )
            .clamp(REF_MIN, REF_MAX);
        }
        let gate = ((rms - NOISE_GATE) / NOISE_GATE).clamp(0.0, 1.0);
        let target = (0.65 * (rms / self.reference).sqrt()).min(1.0) * gate;
        self.state.energy = follow(self.state.energy, target, 0.045, 0.3, self.dt);
        self.state.rms = rms.clamp(0.0, 1.0);
        self.state.peak = peak.clamp(0.0, 1.0);
        // The reference is an RMS, so a sine at the reference level would peak
        // at 1.414 and clip. Scale by the sine crest factor so the trace shows
        // its real shape and only genuine transients touch the rails.
        let trace_scale = self.reference * std::f32::consts::SQRT_2;
        for (point, chunk) in self
            .state
            .waveform
            .iter_mut()
            .zip(samples.as_chunks::<{ HOP / 32 }>().0)
        {
            *point =
                (chunk.iter().sum::<f32>() / chunk.len() as f32 / trace_scale).clamp(-1.0, 1.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        self.analyze_spectral_shape(audible);
        let mut power = [0.0_f32; 3];
        for i in 1..FRAMES / 2 {
            let hz = i as f32 * self.sample_rate / FRAMES as f32;
            let magnitude = self.buffer[i].norm() * (2.0 / FRAMES as f32);
            if (LOW_HZ..HIGH_HZ).contains(&hz) {
                let region = if hz < 250.0 {
                    0
                } else if hz < 2000.0 {
                    1
                } else {
                    2
                };
                power[region] += magnitude * magnitude;
            }
        }
        let mut attacks = [0.0_f32; 4];
        let mut attack_counts = [0.0_f32; 4];
        let mut attack_power = [0.0_f32; 4];
        let mut novelty = [0.0_f32; 2];
        let mut counts = [0.0_f32; 2];
        for (i, &(start, end)) in self.ranges.iter().enumerate() {
            // Empty ranges are bands above Nyquist at the current sample rate.
            let magnitude = self.buffer.get(start..end).map_or(0.0, |bins| {
                bins.iter()
                    .map(|bin| (bin.norm() * (2.0 / FRAMES as f32)).powi(2))
                    .sum::<f32>()
                    .sqrt()
            });
            // Normalize each band before differencing, so loud instruments cannot
            // erase quieter percussion. The floor avoids amplifying empty bands.
            let compressed = (1.0 + magnitude / self.band_reference[i].max(0.003)).ln();
            let rise = (compressed - self.band_previous[i]).max(0.0) * gate;
            self.band_previous[i] = compressed;
            self.band_reference[i] = follow(self.band_reference[i], magnitude, 0.8, 1.5, self.dt);
            let hz = (start + end) as f32 * 0.5 * self.sample_rate / FRAMES as f32;
            let region = if hz < 250.0 {
                0
            } else if hz < 2000.0 {
                1
            } else if hz < 6000.0 {
                2
            } else {
                3
            };
            attacks[region] += rise;
            attack_power[region] += magnitude * magnitude;
            attack_counts[region] += 1.0;
            novelty[0] += rise;
            counts[0] += 1.0;
            if end as f32 * self.sample_rate / FRAMES as f32 <= 300.0 {
                novelty[1] += rise;
                counts[1] += 1.0;
            }
            let target = (0.65 * (magnitude / self.reference).sqrt()).min(1.0) * gate;
            self.state.bands[i] = follow(self.state.bands[i], target, 0.018, 0.14, self.dt);
        }
        for (value, count) in novelty.iter_mut().zip(counts) {
            *value /= count.max(1.0);
        }
        for (value, power) in [
            &mut self.state.bass,
            &mut self.state.mid,
            &mut self.state.treble,
        ]
        .into_iter()
        .zip(power)
        {
            let target = (0.65 * (power.sqrt() / self.reference).sqrt()).min(1.0) * gate;
            *value = follow(*value, target, 0.04, 0.25, self.dt);
        }
        let mut onset = false;
        let mut fast_novelty = 0.0_f32;
        for region in 0..4 {
            let rise = attacks[region] / attack_counts[region].max(1.0);
            let threshold = 0.008 + self.attack_mean[region] * 2.0;
            self.attack_cooldown[region] = (self.attack_cooldown[region] - self.dt).max(0.0);
            self.state.transients[region] *= (-self.dt / 0.045).exp();
            // Independent refractory periods preserve adjacent instruments and
            // rapid subdivisions; only rising novelty can start a new attack.
            let minimum_share = if region == 0 { 0.04 } else { 0.004 };
            let present = attack_power[region] > power.iter().sum::<f32>() * minimum_share;
            if audible
                && present
                && self.attack_cooldown[region] == 0.0
                && rise > threshold
                && rise > self.attack_previous[region]
            {
                self.state.transients[region] = (rise / (threshold * 2.0)).clamp(0.0, 1.0);
                self.attack_cooldown[region] = REFRACTORY;
                onset = true;
            }
            if present {
                fast_novelty = fast_novelty.max((rise / (threshold * 2.0)).min(1.0));
            }
            self.attack_previous[region] = rise;
            self.attack_mean[region] = follow(self.attack_mean[region], rise, 0.8, 0.8, self.dt);
        }
        self.state.novelty = follow(self.state.novelty, fast_novelty, 0.005, 0.025, self.dt);
        self.state.onset = self.state.transients.iter().copied().fold(0.0, f32::max);
        self.state.kick = self.state.transients[0];
        self.rhythm.push(novelty, onset);
        if self.quiet_time > 1.5 {
            self.rhythm = Rhythm::new(self.dt);
        }
        self.state.bpm = self.rhythm.bpm;
        self.state.tempo_confidence = self.rhythm.confidence;
        self.state.beat_phase = self.rhythm.phase;
        self.state.beat = if let Some(bpm) = self.rhythm.bpm {
            (-self.rhythm.phase * (60.0 / bpm) / 0.075).exp() * self.rhythm.confidence
        } else {
            0.0
        };
        self.state.clone()
    }
}

/// Continuous full-spectrum and low-band novelty retain subthreshold rhythm.
#[derive(Default)]
struct Rhythm {
    dt: f32,
    history: VecDeque<[f32; 2]>,
    frames: usize,
    bpm: Option<f32>,
    confidence: f32,
    phase: f32,
    pending: Option<f32>,
    confirmations: usize,
}

impl Rhythm {
    fn new(dt: f32) -> Self {
        Self {
            dt,
            ..Default::default()
        }
    }

    fn push(&mut self, novelty: [f32; 2], attack: bool) {
        self.history.push_back(novelty);
        if self.history.len() > (10.0 / self.dt) as usize {
            self.history.pop_front();
        }
        if let Some(bpm) = self.bpm {
            self.phase = (self.phase + self.dt * bpm / 60.0).fract();
            // Correct near the expected beat, never reset on arbitrary offbeats.
            let error = if self.phase > 0.5 {
                self.phase - 1.0
            } else {
                self.phase
            };
            if attack && error.abs() < 0.16 {
                self.phase = (self.phase - error * 0.35).rem_euclid(1.0);
            }
        }
        self.frames += 1;
        if !self.frames.is_multiple_of(24) || self.history.len() < (4.0 / self.dt) as usize {
            return;
        }
        // At coarse hops (< ~2.2 kHz sources) the 260 BPM lag rounds to zero,
        // which would underflow the score window. One hop is the finest period
        // the estimator can represent.
        let min_lag = ((60.0 / (260.0 * self.dt)).floor() as usize).max(1);
        let max_lag = (60.0 / (60.0 * self.dt)).ceil() as usize;
        let n = self.history.len();
        let mut correlations = vec![[0.0_f32; 2]; max_lag * 2 + 3];
        for channel in 0..2 {
            let mean = self.history.iter().map(|v| v[channel]).sum::<f32>() / n as f32;
            let x: Vec<f32> = self.history.iter().map(|v| v[channel] - mean).collect();
            for (lag, values) in correlations.iter_mut().enumerate().skip(1) {
                let mut cross = 0.0;
                let mut aa = 0.0;
                let mut bb = 0.0;
                for i in lag..n {
                    cross += x[i] * x[i - lag];
                    aa += x[i] * x[i];
                    bb += x[i - lag] * x[i - lag];
                }
                values[channel] = cross / (aa * bb).sqrt().max(1e-8);
            }
        }
        let mut scores = vec![0.0_f32; max_lag + 2];
        for lag in min_lag - 1..=max_lag + 1 {
            // A second repeated period helps distinguish real periodicity from
            // one accidental match. Either channel may carry the rhythm.
            let full = 0.7 * correlations[lag][0] + 0.3 * correlations[lag * 2][0];
            let low = 0.7 * correlations[lag][1] + 0.3 * correlations[lag * 2][1];
            scores[lag] = full.max(low) * 0.75 + full.min(low) * 0.25;
        }
        let candidates = &scores[min_lag..=max_lag];
        let mean = candidates.iter().sum::<f32>() / candidates.len() as f32;
        let spread = (candidates.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
            / candidates.len() as f32)
            .sqrt();
        let mut lag = (min_lag..=max_lag)
            .max_by(|&a, &b| {
                let preference = |lag: usize| {
                    let bpm = 60.0 / (lag as f32 * self.dt);
                    let continuity = self.bpm.map_or(0.0, |old| {
                        0.025 * (1.0 - ((bpm / old).ln().abs() / 0.08).min(1.0))
                    });
                    scores[lag] + continuity
                };
                preference(a).total_cmp(&preference(b))
            })
            .unwrap();
        if let Some(old) = self.bpm {
            let old_lag = (60.0 / (old * self.dt)).round() as usize;
            let ratio = lag as f32 / old_lag as f32;
            // Half/double time is often equally valid. Keep the existing level
            // unless the new level has substantially better periodic evidence.
            let harmonic = (ratio - 2.0).abs() < 0.12 || (ratio - 0.5).abs() < 0.03;
            if harmonic
                && (min_lag..=max_lag).contains(&old_lag)
                && scores[old_lag] >= scores[lag] * 0.8
                && scores[old_lag] >= 0.12
                && (scores[old_lag] - mean) / spread.max(0.001) >= 2.5
            {
                lag = old_lag;
            }
        }
        let prominence = (scores[lag] - mean) / spread.max(0.001);
        let recent = self
            .history
            .iter()
            .rev()
            .take((1.2 / self.dt) as usize)
            .any(|v| v[0] > 0.005 || v[1] > 0.005);
        // Confidence is periodic salience, not the fraction of detected clicks.
        if scores[lag] < 0.12 || prominence < 2.5 || !recent {
            self.confidence *= 0.8;
            self.confirmations = 0;
            if self.confidence < 0.3 || !recent {
                self.bpm = None;
                self.confidence = 0.0;
            }
            return;
        }
        let curvature = scores[lag - 1] - 2.0 * scores[lag] + scores[lag + 1];
        let offset = if curvature < -1e-6 {
            (0.5 * (scores[lag - 1] - scores[lag + 1]) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let bpm = (60.0 / ((lag as f32 + offset) * self.dt)).clamp(60.0, 260.0);
        if self
            .pending
            .is_some_and(|old| (bpm / old).ln().abs() < 0.06)
        {
            self.confirmations += 1;
        } else {
            self.pending = Some(bpm);
            self.confirmations = 1;
        }
        if self.confirmations < 3 {
            return;
        }
        let fresh = self.bpm.is_none_or(|old| (bpm / old).ln().abs() >= 0.08);
        self.bpm = Some(
            self.bpm
                .filter(|old| (bpm / old).ln().abs() < 0.08)
                .map_or(bpm, |old| old * 0.75 + bpm * 0.25),
        );
        self.confidence =
            ((scores[lag] - mean).max(0.0).sqrt() * (prominence / 4.0).min(1.0)).clamp(0.0, 1.0);
        if fresh {
            // Fold recent novelty onto the estimated period to find a beat
            // anchor; tempo alone has no information about beat phase.
            let period = 60.0 / (self.bpm.unwrap() * self.dt);
            // A very coarse hop can round the period below one bin.
            let bins = (period.round() as usize).max(1);
            let mut folded = vec![0.0; bins];
            for (age, v) in self
                .history
                .iter()
                .rev()
                .take((period * 4.0) as usize)
                .enumerate()
            {
                let i = (age as f32 % period).round() as usize % bins;
                folded[i] += v[0] + v[1];
            }
            let age = (0..bins)
                .max_by(|&a, &b| folded[a].total_cmp(&folded[b]))
                .unwrap();
            self.phase = age as f32 / period;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_with(state: AudioState) -> Capture {
        Capture {
            child: None,
            shared: Arc::new(Mutex::new(state)),
            started: Instant::now(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn capture_backend_follows_the_explicit_source() {
        let mut audio = crate::config::AudioConfig {
            command: "parec".into(),
            file: "track.raw".into(),
            ..Default::default()
        };
        audio.source = "auto".into();
        assert!(matches!(capture_config(&audio).backend, Backend::Auto));
        audio.source = "command".into();
        assert!(matches!(
            capture_config(&audio).backend,
            Backend::Command(ref command) if command == "parec"
        ));
        audio.source = "file".into();
        assert!(matches!(
            capture_config(&audio).backend,
            Backend::File(ref file) if file == &PathBuf::from("track.raw")
        ));
        audio.source = "pipewire".into();
        assert!(matches!(capture_config(&audio).backend, Backend::PipeWire));
        audio.source = "microphone".into();
        assert!(matches!(
            capture_config(&audio).backend,
            Backend::Microphone
        ));
    }

    #[test]
    fn looping_file_wraps_at_eof_and_does_not_spin_on_empty_files() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-loop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("pcm.raw");
        std::fs::write(&path, [1_u8, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut reader = LoopingFile::open(&path).unwrap();
        let mut bytes = [0_u8; 12];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4]);

        let empty = directory.join("empty.raw");
        std::fs::write(&empty, []).unwrap();
        let mut reader = LoopingFile::open(&empty).unwrap();
        let mut byte = [0_u8; 1];
        assert_eq!(reader.read(&mut byte).unwrap(), 0);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retry_backoff_grows_caps_and_resets() {
        let mut retry = Retry::default();
        let now = Instant::now();
        assert!(retry.due(now));
        retry.fail(now);
        assert!(!retry.due(now + Duration::from_millis(100)));
        assert!(retry.due(now + RETRY_INITIAL));
        retry.fail(now + RETRY_INITIAL);
        assert!(!retry.due(now + Duration::from_millis(600)));
        assert!(retry.due(now + RETRY_INITIAL + Duration::from_millis(500)));
        for _ in 0..8 {
            retry.fail(now);
        }
        assert!(retry.due(now + RETRY_MAX));
        retry.success();
        retry.fail(now);
        assert!(retry.due(now + RETRY_INITIAL));
    }

    #[test]
    fn only_live_sources_are_retried() {
        assert!(Backend::Auto.is_live());
        assert!(Backend::PipeWire.is_live());
        assert!(Backend::Pulse.is_live());
        assert!(Backend::Microphone.is_live());
        assert!(!Backend::File(PathBuf::from("x")).is_live());
        assert!(!Backend::Command("x".into()).is_live());
    }

    #[test]
    fn a_capture_with_no_samples_reports_an_error_after_the_grace_period() {
        let capture = Capture {
            child: None,
            shared: Arc::new(Mutex::new(AudioState::default())),
            started: Instant::now() - Duration::from_secs(1),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let state = capture.snapshot();
        assert!(state.error.is_some());
        assert!(!state.connected);
    }

    #[test]
    fn snapshot_passes_fresh_audio_through_and_halts_stale_audio() {
        let source = SourceInfo {
            kind: "PipeWire".into(),
            ..Default::default()
        };
        let fresh = capture_with(AudioState {
            energy: 0.8,
            bpm: Some(120.0),
            connected: true,
            updated: Some(Instant::now()),
            source: source.clone(),
            ..Default::default()
        });
        let state = fresh.snapshot();
        assert_eq!(state.energy, 0.8);
        assert_eq!(state.bpm, Some(120.0));
        assert!(state.error.is_none());

        let stale = capture_with(AudioState {
            energy: 0.8,
            bpm: Some(120.0),
            connected: true,
            updated: Some(Instant::now() - Duration::from_secs(2)),
            source,
            ..Default::default()
        });
        let state = stale.snapshot();
        assert_eq!(state.energy, 0.0);
        assert!(state.bpm.is_none());
        assert!(!state.connected);
        assert_eq!(state.source.kind, "PipeWire");
        assert_eq!(state.error.as_deref(), Some(STALL_MESSAGE));
    }

    #[test]
    fn chroma_folds_octaves_and_uses_c_first_pitch_classes() {
        for (hz, pitch) in [
            (110.0, 9),
            (220.0, 9),
            (440.0, 9),
            (130.8128, 0),
            (261.6256, 0),
            (523.2511, 0),
        ] {
            let mut analyzer = Analyzer::new();
            let mut state = AudioState::default();
            for frame in 0..12 {
                state = analyzer.analyze(&frame_at(frame, hz, 0.2));
            }
            let strongest = (0..12)
                .max_by(|&a, &b| state.chroma[a].total_cmp(&state.chroma[b]))
                .unwrap();
            assert_eq!(strongest, pitch, "{hz}Hz chroma: {:?}", state.chroma);
            assert!(
                state.chroma[pitch] > 0.7,
                "{hz}Hz chroma: {:?}",
                state.chroma
            );
            assert!((state.chroma.iter().sum::<f32>() - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn spectral_shape_is_scale_invariant_and_brightness_tracks_frequency() {
        let mut features = Vec::new();
        for (hz, amplitude) in [(220.0, 0.03), (220.0, 0.3), (6000.0, 0.3)] {
            let mut analyzer = Analyzer::new();
            let mut state = AudioState::default();
            for frame in 0..12 {
                state = analyzer.analyze(&frame_at(frame, hz, amplitude));
            }
            assert!(state.flatness < 0.01, "tone flatness: {}", state.flatness);
            features.push(state);
        }
        assert!((features[0].centroid - features[1].centroid).abs() < 0.001);
        for (a, b) in features[0].chroma.iter().zip(features[1].chroma) {
            assert!((a - b).abs() < 0.001);
        }
        assert!(features[2].centroid > features[1].centroid + 0.4);
        let expected = (220.0_f32 / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln();
        assert!((features[1].centroid - expected).abs() < 0.01);
    }

    #[test]
    fn broadband_noise_has_high_flatness_and_silence_clears_spectral_features() {
        let mut analyzer = Analyzer::new();
        let mut seed = 1_u32;
        let mut total = 0.0;
        for frame in 0..40 {
            let noise = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
            });
            let state = analyzer.analyze(&noise);
            if frame >= 4 {
                total += state.flatness;
            }
        }
        assert!(total / 36.0 > 0.45, "noise flatness: {}", total / 36.0);
        for _ in 0..8 {
            let state = analyzer.analyze(&[0.0; HOP]);
            assert_eq!(state.chroma, [0.0; 12]);
            assert_eq!(state.centroid, 0.0);
            assert_eq!(state.flatness, 0.0);
        }
        for frame in 0..8 {
            let state = analyzer.analyze(&frame_at(frame, 440.0, NOISE_GATE * 0.1));
            assert_eq!(state.chroma, [0.0; 12]);
            assert_eq!(state.centroid, 0.0);
            assert_eq!(state.flatness, 0.0);
        }
    }

    #[test]
    fn bass_and_snare_attacks_thirty_two_ms_apart_remain_independent() {
        let mut analyzer = Analyzer::new();
        let mut previous = [0.0; 4];
        let mut counts = [0; 2];
        for frame in 0..(6.0 / DT) as usize {
            let samples = std::array::from_fn(|i| {
                let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                let bass_age = t % (1.0 / 6.0);
                let snare_age = (t - 0.032).rem_euclid(1.0 / 6.0);
                0.06 * (std::f32::consts::TAU * 440.0 * t).sin()
                    + 0.14 * (-bass_age / 0.016).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
                    + 0.10 * (-snare_age / 0.010).exp() * (std::f32::consts::TAU * 1100.0 * t).sin()
            });
            let state = analyzer.analyze(&samples);
            if frame as f32 * DT > 1.0 {
                for region in 0..2 {
                    counts[region] +=
                        usize::from(state.transients[region] > previous[region] + 0.15);
                }
            }
            assert!((0.0..=1.0).contains(&state.novelty));
            previous = state.transients;
        }
        assert!(
            counts.iter().all(|&n| n >= 23),
            "lost bass/snare attacks: {counts:?}"
        );
    }

    #[test]
    fn soft_twelve_hz_attacks_survive_a_sustained_texture() {
        let mut analyzer = Analyzer::new();
        let mut attacks = 0;
        let mut previous = 0.0;
        for frame in 0..(6.0 / DT) as usize {
            let samples = std::array::from_fn(|i| {
                let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                let age = t % (1.0 / 12.0);
                0.12 * (std::f32::consts::TAU * 220.0 * t).sin()
                    + 0.08 * (std::f32::consts::TAU * 329.63 * t).sin()
                    + 0.07 * (-age / 0.012).exp() * (std::f32::consts::TAU * 1100.0 * t).sin()
            });
            let state = analyzer.analyze(&samples);
            if frame as f32 * DT > 1.0 && state.onset > previous + 0.15 {
                attacks += 1;
            }
            previous = state.onset;
        }
        assert!(
            attacks >= 45,
            "only {attacks}/60 soft rapid attacks survived"
        );
    }

    #[test]
    fn tempo_survives_a_continuous_mix_with_soft_kicks() {
        let mut analyzer = Analyzer::new();
        let mut detected = Vec::new();
        for frame in 0..(18.0 / DT) as usize {
            let samples = std::array::from_fn(|i| {
                let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                let phase = t % 0.5;
                let chord = 0.12 * (std::f32::consts::TAU * 220.0 * t).sin()
                    + 0.10 * (std::f32::consts::TAU * 329.63 * t).sin()
                    + 0.08 * (std::f32::consts::TAU * 440.0 * t).sin();
                let texture =
                    0.025 * (std::f32::consts::TAU * (1600.0 * t + 20.0 * (t * 3.1).sin())).sin();
                let kick = 0.08 * (-phase / 0.055).exp() * (std::f32::consts::TAU * 65.0 * t).sin();
                chord + texture + kick
            });
            let state = analyzer.analyze(&samples);
            if frame as f32 * DT > 10.0 {
                if let Some(bpm) = state.bpm {
                    detected.push(bpm);
                }
            }
        }
        assert!(
            detected.len() > 60,
            "mixed rhythmic audio rarely locks: {} frames",
            detected.len()
        );
        assert!(
            detected
                .iter()
                .filter(|&&bpm| (bpm - 120.0).abs() < 5.0)
                .count()
                > detected.len() * 3 / 4,
            "wrong tempo in mix: {detected:?}"
        );
    }

    #[test]
    #[ignore = "requires VIBE_REPLAY pointing to local f32le stereo 48kHz audio"]
    fn replay_music_diagnostic() {
        let path = std::env::var("VIBE_REPLAY").expect("set VIBE_REPLAY");
        let bytes = std::fs::read(path).unwrap();
        let mut analyzer = Analyzer::new();
        let mut locks = 0;
        let mut onsets = 0;
        let mut kicks = 0;
        let mut max_onset = 0.0f32;
        let mut last_onset = 0.0;
        let mut last_kick = 0.0;
        let mut frames = 0;
        for chunk in bytes.as_chunks::<{ HOP * 8 }>().0 {
            let samples = std::array::from_fn(|i| {
                let left = f32::from_le_bytes(chunk[i * 8..i * 8 + 4].try_into().unwrap());
                let right = f32::from_le_bytes(chunk[i * 8 + 4..i * 8 + 8].try_into().unwrap());
                (left + right) * 0.5
            });
            let state = analyzer.analyze(&samples);
            frames += 1;
            locks += usize::from(state.bpm.is_some());
            onsets += usize::from(state.onset - last_onset > 0.2);
            kicks += usize::from(state.kick - last_kick > 0.2);
            last_onset = state.onset;
            last_kick = state.kick;
            max_onset = max_onset.max(state.onset);
            if frames % 23 == 0 {
                eprintln!(
                    "t={:.1} energy={:.2} onset={:.2} kick={:.2} bpm={:?} confidence={:.2}",
                    frames as f32 * DT,
                    state.energy,
                    state.onset,
                    state.kick,
                    state.bpm,
                    state.tempo_confidence
                );
            }
        }
        eprintln!("REPLAY frames={frames} locks={locks} onsets={onsets} kicks={kicks} max_onset={max_onset}");
        assert!(
            locks > frames / 4,
            "tempo missing through most of rhythmic real music"
        );
    }

    #[test]
    fn perceived_energy_is_stable_across_input_amplitudes() {
        let mut quiet = Analyzer::new();
        let mut loud = Analyzer::new();
        let mut quiet_energy = 0.0;
        let mut loud_energy = 0.0;
        for frame in 0..2000 {
            quiet_energy = quiet.analyze(&frame_at(frame, 1000.0, 0.05)).energy;
            loud_energy = loud.analyze(&frame_at(frame, 1000.0, 0.5)).energy;
        }
        assert!(
            quiet_energy > 0.3,
            "quiet audio must remain visible: {quiet_energy}"
        );
        assert!(
            (loud_energy - quiet_energy).abs() < 0.15,
            "perceived energy should adapt: quiet={quiet_energy}, loud={loud_energy}"
        );
    }

    fn frame_at(frame: usize, hz: f32, amplitude: f32) -> [f32; HOP] {
        std::array::from_fn(|i| {
            amplitude
                * (std::f32::consts::TAU * hz * (frame * HOP + i) as f32 / SAMPLE_RATE as f32).sin()
        })
    }

    #[test]
    fn steady_tone_has_no_retrigger_or_tempo() {
        let mut analyzer = Analyzer::new();
        let mut retriggers = 0;
        for frame in 0..1400 {
            let state = analyzer.analyze(&frame_at(frame, 1000.0, 0.25));
            if frame > 80 && (state.onset > 0.1 || state.novelty > 0.1) {
                retriggers += 1;
            }
            assert!(state.bpm.is_none());
        }
        assert_eq!(retriggers, 0);
    }

    #[test]
    fn silence_decays_and_noise_does_not_gain() {
        let mut analyzer = Analyzer::new();
        for frame in 0..1600 {
            analyzer.analyze(&frame_at(frame, 100.0, 0.4));
        }
        for _ in 0..1600 {
            analyzer.analyze(&[0.0; HOP]);
        }
        let reference = analyzer.reference;
        let mut seed = 1_u32;
        for _ in 0..1200 {
            let noise = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.001
            });
            let state = analyzer.analyze(&noise);
            assert!(state.energy < 0.001 && state.bass < 0.001);
            assert!(state.onset < 0.001 && state.kick < 0.001);
            assert!(state.transients.iter().all(|v| *v < 0.001) && state.novelty < 0.001);
            assert!(state.bpm.is_none());
        }
        assert_eq!(reference, analyzer.reference);
        assert!(analyzer.state.bands.iter().all(|v| *v < 0.001));
    }

    #[test]
    fn regular_bass_pulses_have_onsets_kicks_and_tempo() {
        for bpm in [90.0, 120.0, 200.0] {
            let period = 60.0 / bpm;
            let mut analyzer = Analyzer::new();
            let mut triggers = 0;
            let mut kicks = 0;
            let mut previous = 0.0;
            let mut last_trigger = 0;
            let mut state = AudioState::default();
            for frame in 0..(12.0 / DT) as usize {
                let samples = std::array::from_fn(|i| {
                    let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                    let phase = t % period;
                    if phase < 0.09 {
                        0.4 * (-phase / 0.04).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
                    } else {
                        0.0
                    }
                });
                state = analyzer.analyze(&samples);
                // A single hit can reach adjacent spectral regions one hop apart.
                if state.onset > previous + 0.2 && (triggers == 0 || frame - last_trigger > 3) {
                    triggers += 1;
                    last_trigger = frame;
                }
                if state.kick > 0.5 {
                    kicks += 1;
                }
                previous = state.onset;
            }
            let expected = (12.0 / period) as usize;
            assert!(
                (expected - 1..=expected + 1).contains(&triggers),
                "onset count at {bpm}: {triggers}"
            );
            assert!(kicks >= expected - 1, "kick frames: {kicks}");
            assert!(
                state.tempo_confidence > 0.65,
                "confidence: {}",
                state.tempo_confidence
            );
            assert!(
                (state.bpm.expect("periodic pulses should yield tempo") - bpm).abs() < 5.0,
                "tempo: {:?}, expected {bpm}",
                state.bpm
            );
            for _ in 0..200 {
                state = analyzer.analyze(&[0.0; HOP]);
            }
            assert!(state.bpm.is_none());
            assert_eq!(state.tempo_confidence, 0.0);
        }
    }

    #[test]
    fn tonal_regions_and_treble_onsets_do_not_imply_kicks() {
        for (hz, region) in [(100.0, 0), (1000.0, 1), (6000.0, 2)] {
            let mut analyzer = Analyzer::new();
            let mut first = AudioState::default();
            let mut attack = 0.0_f32;
            for frame in 0..4 {
                first = analyzer.analyze(&frame_at(frame, hz, 0.3));
                attack = attack.max(first.onset);
            }
            assert!(attack > 0.5);
            if region != 0 {
                assert!(first.kick < 0.1);
            }
            let mut state = first;
            for frame in 4..200 {
                state = analyzer.analyze(&frame_at(frame, hz, 0.3));
            }
            let features = [state.bass, state.mid, state.treble];
            assert!(features[region] > 0.5);
            for (i, value) in features.iter().enumerate() {
                if i != region {
                    assert!(*value < 0.1, "{hz}Hz leaked into region {i}: {value}");
                }
            }
        }
    }

    #[test]
    fn stationary_broadband_noise_has_no_tempo() {
        let mut analyzer = Analyzer::new();
        let mut seed = 1_u32;
        for _ in 0..1600 {
            let noise = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
            });
            let state = analyzer.analyze(&noise);
            assert!(
                state.bpm.is_none(),
                "stationary noise tempo: {:?}",
                state.bpm
            );
        }
    }

    #[test]
    fn beat_phase_tracks_periodic_attacks_without_frame_resets() {
        let mut analyzer = Analyzer::new();
        let mut phases = Vec::new();
        let mut last_phase = 0.0;
        let mut last_bpm = None;
        for frame in 0..(16.0 / DT) as usize {
            let samples = std::array::from_fn(|i| {
                let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                let age = (t - 0.17).rem_euclid(0.5);
                0.4 * (-age / 0.04).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
            });
            let state = analyzer.analyze(&samples);
            if frame as f32 * DT > 10.0 {
                let bpm = state.bpm.expect("periodic rhythm must stay locked");
                if last_bpm.is_some() {
                    let step = (state.beat_phase - last_phase).rem_euclid(1.0);
                    assert!(step < 0.08, "phase jumped: {step}");
                }
                let t = (frame * HOP) as f32 / SAMPLE_RATE as f32;
                if (t - 0.17).rem_euclid(0.5) < DT {
                    phases.push(state.beat_phase.min(1.0 - state.beat_phase));
                }
                last_bpm = Some(bpm);
                last_phase = state.beat_phase;
            }
        }
        assert!(phases.len() > 8);
        assert!(
            phases.iter().sum::<f32>() / (phases.len() as f32) < 0.12,
            "beat phase does not align with attacks: {phases:?}"
        );
    }

    #[test]
    fn treble_attack_does_not_consume_the_bass_detector_cooldown() {
        let mut analyzer = Analyzer::new();
        let mut onset = false;
        let mut kick = false;
        for frame in 0..(0.15 / DT) as usize {
            let samples = std::array::from_fn(|i| {
                let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
                let high = if t < 0.025 {
                    0.3 * (std::f32::consts::TAU * 6000.0 * t).sin()
                } else {
                    0.0
                };
                let low = if t >= 0.064 {
                    0.4 * (-(t - 0.064) / 0.04).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
                } else {
                    0.0
                };
                high + low
            });
            let state = analyzer.analyze(&samples);
            onset |= state.onset > 0.5;
            kick |= state.kick > 0.5;
        }
        assert!(
            onset && kick,
            "closely spaced treble and bass attacks must both register"
        );
    }

    #[test]
    fn fresh_attacks_reach_visuals_within_three_hops() {
        let mut analyzer = Analyzer::new();
        for _ in 0..100 {
            analyzer.analyze(&[0.0; HOP]);
        }
        let mut delay = None;
        for frame in 0..8 {
            let state = analyzer.analyze(&frame_at(frame, 100.0, 0.4));
            if state.kick > 0.5 && delay.is_none() {
                delay = Some(frame + 1);
            }
            assert!(state.waveform.iter().any(|v| v.abs() > 0.01));
        }
        assert!(
            delay.is_some_and(|hops| hops <= 3),
            "kick delay in hops: {delay:?}"
        );
    }

    #[test]
    fn silence_is_zero() {
        let mut analyzer = Analyzer::new();
        let AudioState {
            bands,
            energy,
            peak,
            ..
        } = analyzer.analyze(&[0.0; HOP]);
        assert_eq!(bands, [0.0; BANDS]);
        assert_eq!(energy, 0.0);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn tone_lands_in_its_logarithmic_band() {
        let mut analyzer = Analyzer::new();
        let frequency = 1000.0;
        let mut state = AudioState::default();
        for frame in 0..8 {
            state = analyzer.analyze(&frame_at(frame, frequency, 0.5));
        }
        let AudioState {
            bands, rms, peak, ..
        } = state;
        let strongest = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let expected =
            ((frequency / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln() * BANDS as f32).floor() as usize;
        assert_eq!(strongest, expected);
        assert!((rms - 0.5 / 2.0_f32.sqrt()).abs() < 0.01);
        assert!((peak - 0.5).abs() < 0.01);
    }

    #[test]
    fn low_sample_rates_clamp_analysis_to_nyquist() {
        let mut analyzer = Analyzer::with_sample_rate(8_000);
        let mut state = AudioState::default();
        for frame in 0..8 {
            state = analyzer.analyze(&frame_at(frame, 1_000.0, 0.5));
        }
        assert!(state.centroid.is_finite() && (0.0..=1.0).contains(&state.centroid));
        assert!(state.flatness.is_finite() && (0.0..=1.0).contains(&state.flatness));
        assert!(state.chroma.iter().all(|value| value.is_finite()));
        assert!(state.bands.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn pcm_decoder_mixes_channels_and_accepts_s16_and_f32() {
        let mut samples = [0.0_f32; HOP];
        let format = PcmFormat {
            rate: 48_000,
            channels: 2,
            sample: SampleFormat::F32,
        };
        let mut bytes = Vec::new();
        for frame in [[0.25_f32, 0.75], [0.5, -0.5]] {
            for channel in frame {
                bytes.extend_from_slice(&channel.to_le_bytes());
            }
        }
        decode_pcm(&bytes, format, &mut samples);
        assert!((samples[0] - 0.5).abs() < 1e-6);
        assert_eq!(samples[1], 0.0);

        let format = PcmFormat {
            rate: 48_000,
            channels: 1,
            sample: SampleFormat::S16,
        };
        let bytes = [0x00, 0x40, 0x00, 0x80];
        let mono = decode_pcm(&bytes, format, &mut samples);
        assert!((samples[0] - 0.5).abs() < 1e-6);
        assert_eq!(samples[1], -1.0);
        assert!(mono.width() < 1e-6, "mono must read as centred");
        assert!(mono.balance().abs() < 1e-6, "mono must read as centred");

        let format = PcmFormat {
            rate: 48_000,
            channels: 1,
            sample: SampleFormat::S32,
        };
        let bytes = 1_073_741_824_i32.to_le_bytes();
        decode_pcm(&bytes, format, &mut samples);
        assert!((samples[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stereo_stats_measure_width_and_balance_from_the_first_two_channels() {
        fn decode_constant(left: f32, right: f32) -> StereoStats {
            let format = PcmFormat {
                rate: 48_000,
                channels: 2,
                sample: SampleFormat::F32,
            };
            let mut samples = [0.0_f32; HOP];
            let mut bytes = Vec::with_capacity(HOP * 8);
            for _ in 0..HOP {
                bytes.extend_from_slice(&left.to_le_bytes());
                bytes.extend_from_slice(&right.to_le_bytes());
            }
            decode_pcm(&bytes, format, &mut samples)
        }

        let centred = decode_constant(0.5, 0.5);
        assert!(centred.width() < 1e-6);
        assert!(centred.balance().abs() < 1e-6);

        let anti_phase = decode_constant(0.5, -0.5);
        assert!((anti_phase.width() - 1.0).abs() < 1e-6);
        assert!(anti_phase.balance().abs() < 1e-6);

        let left_only = decode_constant(0.5, 0.0);
        assert!((left_only.width() - 0.5).abs() < 1e-6);
        assert!((left_only.balance() + 1.0).abs() < 1e-6);
    }

    #[test]
    fn stereo_width_and_balance_reach_the_analyzer_state() {
        let mut analyzer = Analyzer::new();
        let samples = frame_at(0, 440.0, 0.4);
        let mut state = AudioState::default();
        for _ in 0..40 {
            state = analyzer.analyze_with(
                &samples,
                StereoStats {
                    left: 0.04,
                    right: 0.0,
                    cross: 0.0,
                },
            );
        }
        assert!(state.width > 0.35, "width {}", state.width);
        assert!(state.balance < -0.5, "balance {}", state.balance);
    }

    #[test]
    fn list_audio_parser_marks_defaults_and_orders_sinks_first() {
        let dump = br#"[
          {
            "type": "PipeWire:Interface:Metadata",
            "props": {"metadata.name": "default"},
            "metadata": [
              {"key": "default.audio.sink", "value": {"name": "auto_null"}},
              {"key": "default.audio.source", "value": {"name": "mic"}}
            ]
          },
          {
            "type": "PipeWire:Interface:Node",
            "info": {"props": {
              "media.class": "Audio/Sink",
              "node.name": "auto_null",
              "node.description": "Dummy Output",
              "object.serial": 9568,
              "audio.channels": 2,
              "audio.rate": 48000
            }}
          },
          {
            "type": "PipeWire:Interface:Node",
            "info": {"props": {
              "media.class": "Audio/Source",
              "node.name": "mic",
              "node.description": "Microphone"
            }}
          }
        ]"#;
        let devices = parse_pw_dump(dump).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].kind, "sink");
        assert_eq!(devices[0].name, "auto_null");
        assert!(devices[0].default);
        assert_eq!(devices[0].serial, Some(9568));
        assert_eq!(devices[1].kind, "source");
        assert!(devices[1].default);
        assert!(!devices[1].name.is_empty());
    }

    #[test]
    fn analyzer_uses_the_supplied_sample_rate() {
        let sample_rate = 44_100;
        let frequency = 1_000.0;
        let mut analyzer = Analyzer::with_sample_rate(sample_rate);
        let mut state = AudioState::default();
        for frame in 0..8 {
            let samples = std::array::from_fn(|i| {
                0.5 * (std::f32::consts::TAU * frequency * (frame * HOP + i) as f32
                    / sample_rate as f32)
                    .sin()
            });
            state = analyzer.analyze(&samples);
        }
        let strongest = state
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let expected =
            ((frequency / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln() * BANDS as f32).floor() as usize;
        assert_eq!(strongest, expected);
    }

    #[test]
    fn silent_time_tracks_live_but_silent_capture() {
        let mut analyzer = Analyzer::new();
        let mut state = AudioState::default();
        for _ in 0..100 {
            state = analyzer.analyze(&[0.0; HOP]);
        }
        assert!(state.silent_for > 1.0);
        for frame in 0..8 {
            state = analyzer.analyze(&frame_at(frame, 440.0, 0.2));
        }
        assert!(state.silent_for < 0.2);
    }

    #[test]
    fn every_band_owns_distinct_bins_and_the_ranges_tile() {
        for rate in [44_100_usize, 48_000, 96_000, 192_000] {
            let analyzer = Analyzer::with_sample_rate(rate);
            for pair in analyzer.ranges.windows(2) {
                assert_eq!(
                    pair[0].1, pair[1].0,
                    "bands must tile without gaps or overlap at {rate} Hz: {:?}",
                    analyzer.ranges
                );
            }
            let mut used = std::collections::BTreeSet::new();
            for &(start, end) in &analyzer.ranges {
                assert!(end <= FRAMES / 2, "band beyond Nyquist at {rate} Hz");
                for bin in start..end {
                    assert!(
                        used.insert(bin),
                        "bin {bin} drives more than one band at {rate} Hz"
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent_bass_bands_do_not_show_the_same_bin() {
        let mut analyzer = Analyzer::new();
        let mut state = AudioState::default();
        for frame in 0..40 {
            state = analyzer.analyze(&frame_at(frame, 70.0, 0.4));
        }
        let bass = &state.bands[..4];
        assert!(
            bass.windows(2).any(|pair| (pair[0] - pair[1]).abs() > 0.01),
            "70 Hz looks identical in all four bass bands: {bass:?}"
        );
    }

    #[test]
    fn coarse_hops_do_not_panic_the_tempo_tracker() {
        for rate in [2_000_usize, 1_000, 500, 1] {
            let mut analyzer = Analyzer::with_sample_rate(rate);
            let dt = HOP as f32 / rate as f32;
            let frames = (20.0 / dt).max(120.0) as usize;
            for frame in 0..frames {
                let samples: [f32; HOP] = std::array::from_fn(|i| {
                    let t = (frame * HOP + i) as f32 / rate as f32;
                    let age = t % 0.5;
                    0.4 * (-age / 0.04).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
                });
                let state = analyzer.analyze(&samples);
                assert!(state.energy.is_finite() && state.novelty.is_finite());
                assert!(
                    (0.0..=1.0).contains(&state.beat_phase),
                    "phase {} at {rate} Hz",
                    state.beat_phase
                );
            }
        }
    }

    #[test]
    fn waveform_trace_shows_a_sine_instead_of_clipping_it() {
        let mut analyzer = Analyzer::new();
        let mut low = AudioState::default();
        for frame in 0..200 {
            low = analyzer.analyze(&frame_at(frame, 100.0, 0.4));
        }
        let peak = low.waveform.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
        assert!((0.9..=1.0).contains(&peak), "100 Hz peak {peak}");

        let mut fast = Analyzer::new();
        let mut high = AudioState::default();
        for frame in 0..200 {
            high = fast.analyze(&frame_at(frame, 1000.0, 0.4));
        }
        assert!(
            high.waveform.iter().all(|v| v.abs() < 0.99),
            "a steady tone is railed: {:?}",
            &high.waveform[..8]
        );
    }

    #[test]
    fn reference_seeds_on_the_first_audible_hop_instead_of_drifting_for_seconds() {
        let mut analyzer = Analyzer::new();
        let mut early = 0.0;
        for frame in 0..100 {
            early = analyzer.analyze(&frame_at(frame, 500.0, 0.02)).energy;
        }
        let mut settled = early;
        for frame in 100..3000 {
            settled = analyzer.analyze(&frame_at(frame, 500.0, 0.02)).energy;
        }
        assert!(
            (early - settled).abs() < 0.05,
            "quiet audio takes seconds to reach its gain: {early} vs {settled}"
        );
    }

    #[test]
    fn anti_phase_stereo_keeps_its_width_through_the_mono_downmix() {
        let format = PcmFormat {
            rate: 48_000,
            channels: 2,
            sample: SampleFormat::F32,
        };
        let mut bytes = Vec::with_capacity(HOP * 8);
        for i in 0..HOP {
            let value = 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin();
            bytes.extend_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&(-value).to_le_bytes());
        }
        let mut samples = [0.0_f32; HOP];
        let stereo = decode_pcm(&bytes, format, &mut samples);
        assert!((stereo.width() - 1.0).abs() < 1e-6);
        assert_eq!(
            samples[0], 0.0,
            "anti-phase material cancels in the downmix"
        );

        let mut analyzer = Analyzer::new();
        let mut state = AudioState::default();
        for _ in 0..40 {
            state = analyzer.analyze_with(&samples, stereo);
        }
        assert!(state.width > 0.9, "anti-phase width {}", state.width);
        assert!(state.balance.abs() < 0.05);
        assert_eq!(state.energy, 0.0, "the downmix itself is silent");
    }

    #[test]
    fn dropping_a_capture_stops_its_reader() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("pcm.raw");
        let pcm: Vec<u8> = (0..HOP)
            .flat_map(|i| {
                let value = 0.25 * (i as f32 * 0.1).sin();
                value.to_le_bytes().into_iter().chain(value.to_le_bytes())
            })
            .collect();
        std::fs::write(&path, pcm).unwrap();

        let capture = Capture::start(CaptureConfig {
            backend: Backend::File(path),
            target: None,
            format: PcmFormat::default(),
            loop_file: true,
        })
        .unwrap();
        let weak = Arc::downgrade(&capture.shared);
        for _ in 0..100 {
            if capture.snapshot().connected {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            capture.snapshot().connected,
            "looping file never produced a block"
        );
        drop(capture);
        let mut stopped = false;
        for _ in 0..100 {
            if weak.upgrade().is_none() {
                stopped = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_dir_all(&directory).unwrap();
        assert!(stopped, "the capture reader thread outlived its Capture");
    }

    #[test]
    fn source_info_describes_target_and_format() {
        let info = SourceInfo {
            kind: "PipeWire".into(),
            target: Some("auto_null".into()),
            rate: 48_000,
            channels: 2,
            format: "f32le".into(),
        };
        let text = info.describe();
        assert!(text.contains("PipeWire"));
        assert!(text.contains("auto_null"));
        assert!(text.contains("48000 Hz"));
        assert!(text.contains("f32le"));
    }
}
