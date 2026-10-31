use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

const SAMPLE_RATE: usize = 48_000;
const FRAMES: usize = 2048;
const BANDS: usize = 32;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;

#[derive(Clone, Debug, Default)]
pub struct AudioState {
    pub bands: [f32; 32],
    /// Adaptive perceived loudness, rather than raw sample amplitude.
    pub energy: f32,
    pub rms: f32,
    pub peak: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
    /// Periodicity estimate; half/double-tempo ambiguity is possible.
    pub bpm: Option<f32>,
    pub tempo_confidence: f32,
    pub updated: Option<Instant>,
    pub connected: bool,
    pub error: Option<String>,
}

pub struct Capture {
    child: Child,
}

impl Capture {
    pub fn start(target: Option<String>) -> io::Result<(Self, Arc<Mutex<AudioState>>)> {
        let mut command = Command::new("pw-record");
        command.args([
            "--raw",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--properties",
            "{ stream.capture.sink = true }",
        ]);
        if let Some(target) = target {
            command.arg("--target").arg(target);
        }
        let mut child = command
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("piped capture stdout");
        let state = Arc::new(Mutex::new(AudioState::default()));
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let mut analyzer = Analyzer::new();
                let mut bytes = [0_u8; FRAMES * 2 * 4];
                let mut samples = [0.0_f32; FRAMES];
                loop {
                    if let Err(error) = stdout.read_exact(&mut bytes) {
                        if let Ok(mut state) = worker_state.lock() {
                            *state = AudioState::default();
                            state.error = Some(if error.kind() == io::ErrorKind::UnexpectedEof {
                                "Audio capture ended; check the PipeWire output target".into()
                            } else {
                                format!("Audio capture failed: {error}")
                            });
                        }
                        break;
                    }
                    for (sample, frame) in samples.iter_mut().zip(bytes.as_chunks::<8>().0) {
                        let left = f32::from_le_bytes(frame[..4].try_into().unwrap());
                        let right = f32::from_le_bytes(frame[4..].try_into().unwrap());
                        *sample = if left.is_finite() && right.is_finite() {
                            left * 0.5 + right * 0.5
                        } else {
                            0.0
                        };
                    }
                    let updated = Instant::now();
                    let mut analyzed = analyzer.analyze(&samples);
                    analyzed.updated = Some(updated);
                    analyzed.connected = true;
                    if let Ok(mut state) = worker_state.lock() {
                        *state = analyzed;
                    } else {
                        break;
                    }
                }
            });
        if let Err(error) = worker {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((Self { child }, state))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Killing the writer releases the detached reader without joining it.
    }
}

// Envelopes run on the audio clock, independent of render rate or pipe scheduling.
const DT: f32 = FRAMES as f32 / SAMPLE_RATE as f32;
const NOISE_GATE: f32 = 0.0015;
const REF_MIN: f32 = 0.025;
const REF_MAX: f32 = 0.5;
const REFRACTORY: f32 = 0.128;

fn follow(current: f32, target: f32, attack: f32, release: f32) -> f32 {
    let tau = if target > current { attack } else { release };
    current + (target - current) * (1.0 - (-DT / tau).exp())
}

struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    ranges: [(usize, usize); BANDS],
    previous: Vec<f32>,
    state: AudioState,
    reference: f32,
    flux_mean: f32,
    cooldown: f32,
    quiet_time: f32,
    rhythm: Rhythm,
}

impl Analyzer {
    fn new() -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(FRAMES);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..FRAMES)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAMES as f32).cos())
            .collect();
        let mut ranges = [(0, 0); BANDS];
        for (i, range) in ranges.iter_mut().enumerate() {
            let low = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f32 / BANDS as f32);
            let high = LOW_HZ * (HIGH_HZ / LOW_HZ).powf((i + 1) as f32 / BANDS as f32);
            let start = (low * FRAMES as f32 / SAMPLE_RATE as f32).ceil() as usize;
            let end = (high * FRAMES as f32 / SAMPLE_RATE as f32).ceil() as usize;
            *range = (start.max(1), end.max(start + 1).min(FRAMES / 2));
        }
        Self {
            fft,
            scratch,
            window,
            ranges,
            buffer: vec![Complex::default(); FRAMES],
            previous: vec![0.0; FRAMES / 2],
            state: AudioState::default(),
            reference: 0.1,
            flux_mean: 0.0,
            cooldown: 0.0,
            quiet_time: 0.0,
            rhythm: Rhythm::default(),
        }
    }

    fn analyze(&mut self, samples: &[f32; FRAMES]) -> AudioState {
        let mut squares = 0.0;
        let mut peak = 0.0_f32;
        for ((bin, window), sample) in self.buffer.iter_mut().zip(&self.window).zip(samples) {
            squares += sample * sample;
            peak = peak.max(sample.abs());
            *bin = Complex::new(sample * window, 0.0);
        }
        let rms = (squares / FRAMES as f32).sqrt();
        let audible = rms > NOISE_GATE;
        self.quiet_time = if audible { 0.0 } else { self.quiet_time + DT };
        // Freeze adaptation below the gate: silence must never turn into extra gain.
        if audible {
            self.reference = follow(self.reference, rms.clamp(REF_MIN, REF_MAX), 2.0, 6.0)
                .clamp(REF_MIN, REF_MAX);
        }
        let gate = ((rms - NOISE_GATE) / NOISE_GATE).clamp(0.0, 1.0);
        let target = (0.65 * (rms / self.reference).sqrt()).min(1.0) * gate;
        self.state.energy = follow(self.state.energy, target, 0.045, 0.3);
        self.state.rms = rms.clamp(0.0, 1.0);
        self.state.peak = peak.clamp(0.0, 1.0);
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let mut power = [0.0_f32; 3];
        let mut flux = 0.0;
        let mut bass_flux = 0.0;
        for i in 1..FRAMES / 2 {
            let hz = i as f32 / DT;
            let magnitude = self.buffer[i].norm() * (2.0 / FRAMES as f32);
            let rise = (magnitude - self.previous[i]).max(0.0);
            self.previous[i] = magnitude;
            if (LOW_HZ..HIGH_HZ).contains(&hz) {
                let region = if hz < 250.0 {
                    0
                } else if hz < 2000.0 {
                    1
                } else {
                    2
                };
                power[region] += magnitude * magnitude;
                flux += rise * rise;
                if region == 0 {
                    bass_flux += rise * rise;
                }
            }
        }
        for (band, &(start, end)) in self.state.bands.iter_mut().zip(&self.ranges) {
            let magnitude = self.buffer[start..end]
                .iter()
                .map(|bin| bin.norm() * (2.0 / FRAMES as f32))
                .fold(0.0, f32::max);
            let target = (0.65 * (magnitude / self.reference).sqrt()).min(1.0) * gate;
            *band = follow(*band, target, 0.035, 0.23);
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
            *value = follow(*value, target, 0.04, 0.25);
        }
        let flux = flux.sqrt() / self.reference * gate;
        let bass_flux = bass_flux.sqrt() / self.reference * gate;
        self.cooldown = (self.cooldown - DT).max(0.0);
        self.state.onset *= (-DT / 0.12).exp();
        self.state.kick *= (-DT / 0.14).exp();
        let threshold = 0.06 + self.flux_mean * 2.5;
        let event = if audible && self.cooldown == 0.0 && flux > threshold {
            self.cooldown = REFRACTORY;
            self.state.onset = (flux / (threshold * 2.0)).clamp(0.0, 1.0);
            if bass_flux > 0.06 {
                self.state.kick = (bass_flux / (threshold * 2.0)).clamp(0.0, 1.0);
            }
            self.state.onset
        } else {
            0.0
        };
        self.flux_mean = follow(self.flux_mean, flux, 0.8, 0.8);
        self.rhythm.push(event);
        if self.quiet_time > 1.5 {
            self.rhythm = Rhythm::default();
        }
        self.state.bpm = self.rhythm.bpm;
        self.state.tempo_confidence = self.rhythm.confidence;
        self.state.clone()
    }
}

/// Autocorrelation measures repeated onsets, not pitch or a guaranteed musical beat.
#[derive(Default)]
struct Rhythm {
    history: VecDeque<f32>,
    frames: usize,
    bpm: Option<f32>,
    confidence: f32,
}

impl Rhythm {
    fn push(&mut self, event: f32) {
        self.history.push_back(event);
        if self.history.len() > (10.0 / DT) as usize {
            self.history.pop_front();
        }
        self.frames += 1;
        if !self.frames.is_multiple_of(12) {
            return;
        }
        self.bpm = None;
        self.confidence = 0.0;
        let events = self.history.iter().filter(|&&v| v > 0.0).count();
        let recent = self
            .history
            .iter()
            .rev()
            .take((1.2 / DT) as usize)
            .any(|&v| v > 0.0);
        if self.history.len() < (4.0 / DT) as usize || events < 6 || !recent {
            return;
        }
        // Spread each impulse over three frames to tolerate frame quantization.
        let x: Vec<f32> = (0..self.history.len())
            .map(|i| {
                self.history[i]
                    + if i > 0 {
                        self.history[i - 1] * 0.5
                    } else {
                        0.0
                    }
                    + self.history.get(i + 1).copied().unwrap_or(0.0) * 0.5
            })
            .collect();
        let min_lag = (60.0 / (180.0 * DT)).floor() as usize;
        let max_lag = (60.0 / (60.0 * DT)).ceil() as usize;
        let mut scores = vec![0.0; max_lag + 2];
        for (lag, score) in scores
            .iter_mut()
            .enumerate()
            .take(max_lag + 2)
            .skip(min_lag - 1)
        {
            let n = (x.len() - lag) as f32;
            let a = &x[lag..];
            let b = &x[..x.len() - lag];
            let ma = a.iter().sum::<f32>() / n;
            let mb = b.iter().sum::<f32>() / n;
            let mut cross = 0.0;
            let mut aa = 0.0;
            let mut bb = 0.0;
            for (&a, &b) in a.iter().zip(b) {
                cross += (a - ma) * (b - mb);
                aa += (a - ma).powi(2);
                bb += (b - mb).powi(2);
            }
            *score = cross / (aa * bb).sqrt().max(1e-6);
        }
        let lag = (min_lag..=max_lag)
            .max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
            .unwrap();
        let confidence = scores[lag].clamp(0.0, 1.0);
        if confidence < 0.65 {
            return;
        }
        let curvature = scores[lag - 1] - 2.0 * scores[lag] + scores[lag + 1];
        let offset = if curvature.abs() > 1e-6 {
            (0.5 * (scores[lag - 1] - scores[lag + 1]) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        self.bpm = Some((60.0 / ((lag as f32 + offset) * DT)).clamp(60.0, 180.0));
        self.confidence = confidence;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perceived_energy_is_stable_across_input_amplitudes() {
        let mut quiet = Analyzer::new();
        let mut loud = Analyzer::new();
        let tone = |amplitude: f32| {
            std::array::from_fn(|i| {
                amplitude * (std::f32::consts::TAU * 1000.0 * i as f32 / SAMPLE_RATE as f32).sin()
            })
        };
        let mut quiet_energy = 0.0;
        let mut loud_energy = 0.0;
        for _ in 0..500 {
            quiet_energy = quiet.analyze(&tone(0.05)).energy;
            loud_energy = loud.analyze(&tone(0.5)).energy;
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

    fn frame_at(frame: usize, hz: f32, amplitude: f32) -> [f32; FRAMES] {
        std::array::from_fn(|i| {
            amplitude
                * (std::f32::consts::TAU * hz * (frame * FRAMES + i) as f32 / SAMPLE_RATE as f32)
                    .sin()
        })
    }

    #[test]
    fn steady_tone_has_no_retrigger_or_tempo() {
        let mut analyzer = Analyzer::new();
        let mut retriggers = 0;
        for frame in 0..350 {
            let state = analyzer.analyze(&frame_at(frame, 1000.0, 0.25));
            if frame > 20 && state.onset > 0.5 {
                retriggers += 1;
            }
            assert!(state.bpm.is_none());
        }
        assert_eq!(retriggers, 0);
    }

    #[test]
    fn silence_decays_and_noise_does_not_gain() {
        let mut analyzer = Analyzer::new();
        for frame in 0..100 {
            analyzer.analyze(&frame_at(frame, 100.0, 0.4));
        }
        for _ in 0..100 {
            analyzer.analyze(&[0.0; FRAMES]);
        }
        let reference = analyzer.reference;
        let mut seed = 1_u32;
        for _ in 0..300 {
            let noise = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.001
            });
            let state = analyzer.analyze(&noise);
            assert!(state.energy < 0.001 && state.bass < 0.001);
            assert!(state.onset < 0.001 && state.kick < 0.001);
            assert!(state.bpm.is_none());
        }
        assert_eq!(reference, analyzer.reference);
        assert!(analyzer.state.bands.iter().all(|v| *v < 0.001));
    }

    #[test]
    fn regular_bass_pulses_have_onsets_kicks_and_tempo() {
        for bpm in [90.0, 120.0] {
            let period = 60.0 / bpm;
            let mut analyzer = Analyzer::new();
            let mut triggers = 0;
            let mut kicks = 0;
            let mut previous = 0.0;
            let mut state = AudioState::default();
            for frame in 0..(12.0 / DT) as usize {
                let samples = std::array::from_fn(|i| {
                    let t = (frame * FRAMES + i) as f32 / SAMPLE_RATE as f32;
                    let phase = t % period;
                    if phase < 0.09 {
                        0.4 * (-phase / 0.04).exp() * (std::f32::consts::TAU * 100.0 * t).sin()
                    } else {
                        0.0
                    }
                });
                state = analyzer.analyze(&samples);
                if state.onset > previous + 0.2 {
                    triggers += 1;
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
            for _ in 0..50 {
                state = analyzer.analyze(&[0.0; FRAMES]);
            }
            assert!(state.bpm.is_none());
            assert_eq!(state.tempo_confidence, 0.0);
        }
    }

    #[test]
    fn tonal_regions_and_treble_onsets_do_not_imply_kicks() {
        for (hz, region) in [(100.0, 0), (1000.0, 1), (6000.0, 2)] {
            let mut analyzer = Analyzer::new();
            let first = analyzer.analyze(&frame_at(0, hz, 0.3));
            assert!(first.onset > 0.5);
            if region != 0 {
                assert!(first.kick < 0.1);
            }
            let mut state = first;
            for frame in 1..50 {
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
        for _ in 0..400 {
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
    fn silence_is_zero() {
        let mut analyzer = Analyzer::new();
        let AudioState {
            bands,
            energy,
            peak,
            ..
        } = analyzer.analyze(&[0.0; FRAMES]);
        assert_eq!(bands, [0.0; BANDS]);
        assert_eq!(energy, 0.0);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn tone_lands_in_its_logarithmic_band() {
        let mut analyzer = Analyzer::new();
        let frequency = 1000.0;
        let samples = std::array::from_fn(|i| {
            0.5 * (std::f32::consts::TAU * frequency * i as f32 / SAMPLE_RATE as f32).sin()
        });
        let AudioState {
            bands, rms, peak, ..
        } = analyzer.analyze(&samples);
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
}
