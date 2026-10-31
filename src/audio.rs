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
const HOP: usize = 512;
const BANDS: usize = 32;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;

#[derive(Clone, Debug, Default)]
pub struct AudioState {
    pub bands: [f32; 32],
    /// Recent signed waveform, normalized by the loudness reference.
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
            "--latency",
            "256",
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
                let mut bytes = [0_u8; HOP * 2 * 4];
                let mut samples = [0.0_f32; HOP];
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
const DT: f32 = HOP as f32 / SAMPLE_RATE as f32;
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
    samples: [f32; FRAMES],
    band_reference: [f32; BANDS],
    band_previous: [f32; BANDS],
    bass_mean: f32,
    bass_cooldown: f32,
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
            samples: [0.0; FRAMES],
            band_reference: [0.01; BANDS],
            band_previous: [0.0; BANDS],
            bass_mean: 0.0,
            bass_cooldown: 0.0,
            state: AudioState::default(),
            reference: 0.1,
            flux_mean: 0.0,
            cooldown: 0.0,
            quiet_time: 0.0,
            rhythm: Rhythm::default(),
        }
    }

    fn analyze(&mut self, samples: &[f32; HOP]) -> AudioState {
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
        for (point, chunk) in self
            .state
            .waveform
            .iter_mut()
            .zip(samples.as_chunks::<{ HOP / 32 }>().0)
        {
            *point =
                (chunk.iter().sum::<f32>() / chunk.len() as f32 / self.reference).clamp(-1.0, 1.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let mut power = [0.0_f32; 3];
        let mut flux = 0.0;
        let mut bass_flux = 0.0;
        for i in 1..FRAMES / 2 {
            let hz = i as f32 * SAMPLE_RATE as f32 / FRAMES as f32;
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
        let mut novelty = [0.0_f32; 2];
        let mut counts = [0.0_f32; 2];
        for (i, &(start, end)) in self.ranges.iter().enumerate() {
            let magnitude = self.buffer[start..end]
                .iter()
                .map(|bin| (bin.norm() * (2.0 / FRAMES as f32)).powi(2))
                .sum::<f32>()
                .sqrt();
            // Normalize each band before differencing, so loud instruments cannot
            // erase quieter percussion. The floor avoids amplifying empty bands.
            let compressed = (1.0 + magnitude / self.band_reference[i].max(0.003)).ln();
            let rise = (compressed - self.band_previous[i]).max(0.0) * gate;
            self.band_previous[i] = compressed;
            self.band_reference[i] = follow(self.band_reference[i], magnitude, 0.8, 1.5);
            novelty[0] += rise;
            counts[0] += 1.0;
            if end as f32 * SAMPLE_RATE as f32 / FRAMES as f32 <= 300.0 {
                novelty[1] += rise;
                counts[1] += 1.0;
            }
            let target = (0.65 * (magnitude / self.reference).sqrt()).min(1.0) * gate;
            self.state.bands[i] = follow(self.state.bands[i], target, 0.018, 0.14);
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
            *value = follow(*value, target, 0.04, 0.25);
        }
        let flux = flux.sqrt() / self.reference * gate;
        let bass_flux = bass_flux.sqrt() / self.reference * gate;
        self.cooldown = (self.cooldown - DT).max(0.0);
        self.bass_cooldown = (self.bass_cooldown - DT).max(0.0);
        self.state.onset *= (-DT / 0.12).exp();
        self.state.kick *= (-DT / 0.14).exp();
        let threshold = 0.025 + self.flux_mean * 2.0;
        let onset = audible && self.cooldown == 0.0 && flux > threshold;
        if onset {
            self.cooldown = REFRACTORY;
            self.state.onset = (flux / (threshold * 2.0)).clamp(0.0, 1.0);
        }
        let bass_threshold = 0.02 + self.bass_mean * 2.0;
        let kick = audible
            && self.bass_cooldown == 0.0
            && bass_flux > bass_threshold
            && power[0] > power.iter().sum::<f32>() * 0.04;
        if kick {
            self.bass_cooldown = REFRACTORY;
            self.state.kick = (bass_flux / (bass_threshold * 2.0)).clamp(0.0, 1.0);
        }
        self.flux_mean = follow(self.flux_mean, flux, 0.8, 0.8);
        self.bass_mean = follow(self.bass_mean, bass_flux, 0.8, 0.8);
        self.rhythm.push(novelty, onset || kick);
        if self.quiet_time > 1.5 {
            self.rhythm = Rhythm::default();
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
    history: VecDeque<[f32; 2]>,
    frames: usize,
    bpm: Option<f32>,
    confidence: f32,
    phase: f32,
    pending: Option<f32>,
    confirmations: usize,
}

impl Rhythm {
    fn push(&mut self, novelty: [f32; 2], attack: bool) {
        self.history.push_back(novelty);
        if self.history.len() > (10.0 / DT) as usize {
            self.history.pop_front();
        }
        if let Some(bpm) = self.bpm {
            self.phase = (self.phase + DT * bpm / 60.0).fract();
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
        if !self.frames.is_multiple_of(24) || self.history.len() < (4.0 / DT) as usize {
            return;
        }
        let min_lag = (60.0 / (180.0 * DT)).floor() as usize;
        let max_lag = (60.0 / (60.0 * DT)).ceil() as usize;
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
                    let bpm = 60.0 / (lag as f32 * DT);
                    let continuity = self.bpm.map_or(0.0, |old| {
                        0.025 * (1.0 - ((bpm / old).ln().abs() / 0.08).min(1.0))
                    });
                    scores[lag] + continuity
                };
                preference(a).total_cmp(&preference(b))
            })
            .unwrap();
        if let Some(old) = self.bpm {
            let old_lag = (60.0 / (old * DT)).round() as usize;
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
            .take((1.2 / DT) as usize)
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
        let bpm = (60.0 / ((lag as f32 + offset) * DT)).clamp(60.0, 180.0);
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
            let period = 60.0 / (self.bpm.unwrap() * DT);
            let bins = period.round() as usize;
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
    #[ignore = "requires CHROMAVIBE_REPLAY pointing to local f32le stereo 48kHz audio"]
    fn replay_music_diagnostic() {
        let path = std::env::var("CHROMAVIBE_REPLAY").expect("set CHROMAVIBE_REPLAY");
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
            if frame > 80 && state.onset > 0.5 {
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
                    let t = (frame * HOP + i) as f32 / SAMPLE_RATE as f32;
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
}
