//! Metaballs and soft bodies: implicit scalar fields, merging droplets, and
//! surface tension. Positions live in an aspect-corrected plane where one
//! vertical unit covers about two terminal columns.

use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Lava Lamp Merge",
    "Surface Tension",
    "Droplet Rain",
    "Soft Cell Colony",
    "Chrome Meniscus",
    "Split and Fuse",
    "Viscous Thread",
    "Implicit Bloom",
    "Magnetic Droplets",
    "Tide Pool Squash",
];

const EPS: f32 = 1e-4;

fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// An implicit field source. `ax`/`ay` are reciprocal radii, which keeps the
/// per-cell field evaluation to two multiplies and one divide per blob.
#[derive(Clone, Copy, Default)]
struct Blob {
    x: f32,
    y: f32,
    ax: f32,
    ay: f32,
}

impl Blob {
    fn round(x: f32, y: f32, radius: f32) -> Self {
        let radius = radius.max(1e-3);
        Self {
            x,
            y,
            ax: 1.0 / radius,
            ay: 1.0 / radius,
        }
    }

    fn oval(x: f32, y: f32, rx: f32, ry: f32) -> Self {
        Self {
            x,
            y,
            ax: 1.0 / rx.max(1e-3),
            ay: 1.0 / ry.max(1e-3),
        }
    }
}

/// Returns the summed field, the normalized distance to the nearest blob, and
/// that blob's index, all in one pass.
fn sample(blobs: &[Blob], x: f32, y: f32) -> (f32, f32, usize) {
    let mut value = 0.0;
    let mut best = f32::INFINITY;
    let mut index = 0;
    for (i, blob) in blobs.iter().enumerate() {
        let dx = (x - blob.x) * blob.ax;
        let dy = (y - blob.y) * blob.ay;
        let distance = dx * dx + dy * dy;
        if distance < best {
            best = distance;
            index = i;
        }
        value += 1.0 / (distance + EPS);
    }
    (value, best.sqrt(), index)
}

fn paint(buf: &mut Buffer, x: u16, y: u16, mark: char, color: Color) {
    let cell = &mut buf[(x, y)];
    cell.set_char(mark);
    cell.set_fg(color);
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    clock: f32,
    envelope: f32,
    swell: f32,
    xs: Vec<f32>,
    ys: Vec<f32>,
    blobs: Vec<Blob>,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            self.last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.08))
        } else {
            0.0
        };
        if time.is_finite() {
            self.last_time = Some(time);
        }
        self.clock += dt * (0.55 + unit(audio.energy) * 0.75 + unit(audio.mid) * 0.25);
        let attack = unit(audio.kick)
            .max(unit(audio.onset))
            .max(unit(audio.transients[0]))
            .max(unit(audio.transients[1]) * 0.8);
        self.envelope += (attack - self.envelope) * (dt * 9.0).min(1.0);
        self.swell += (unit(audio.bass) - self.swell) * (dt * 4.0).min(1.0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        style: usize,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &Theme,
    ) {
        let mode = style % NAMES.len();
        self.advance(time, audio);
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }

        let width = area.width.max(1) as f32;
        let height = area.height.max(1) as f32;
        // Half-width of the plane in vertical units; keeps blobs circular on
        // terminals where a cell is about twice as tall as it is wide.
        let aspect = (width / (2.0 * height)).clamp(0.25, 4.0);

        self.xs.clear();
        self.ys.clear();
        self.xs.extend(
            (visible.x..visible.right())
                .map(|x| (((x - area.x) as f32 + 0.5) / width * 2.0 - 1.0) * aspect),
        );
        self.ys.extend(
            (visible.y..visible.bottom()).map(|y| ((y - area.y) as f32 + 0.5) / height * 2.0 - 1.0),
        );

        let background = theme.sample(palette, 0.0, 0.0);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                let cell = &mut buf[(x, y)];
                cell.reset();
                cell.set_char(' ');
                cell.set_bg(background);
            }
        }

        let t = self.clock;
        let energy = unit(audio.energy);
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let treble = unit(audio.treble);
        let onset = unit(audio.onset);
        let envelope = self.envelope;
        let swell = self.swell;

        self.blobs.clear();
        let mut center = [0.0f32; 2];
        let mut size = [0.0f32; 2];
        let mut spin = 0.0f32;
        match mode {
            0 => {
                // Seven slowly wandering lobes that merge through a thick neck.
                for i in 0..7 {
                    let angle = i as f32 * TAU / 7.0 + t * 0.19;
                    let orbit = 0.34 + 0.30 * (0.5 + 0.5 * (t * 0.31 + i as f32 * 1.9).sin());
                    let radius =
                        0.28 + 0.08 * (0.5 + 0.5 * (t * 0.7 + i as f32 * 2.3).sin()) + swell * 0.12;
                    self.blobs.push(Blob::round(
                        angle.cos() * orbit * aspect * 0.95,
                        angle.sin() * orbit * 0.9,
                        radius,
                    ));
                }
            }
            1 => {
                // One heavy drop ringed by smaller satellites; only its level
                // sets are drawn, so the field reads as stretched skin.
                self.blobs.push(Blob::round(0.0, 0.0, 0.40 + bass * 0.08));
                let reach = 0.54 + bass * 0.12;
                for i in 0..6 {
                    let angle = i as f32 * TAU / 6.0 + t * 0.16 + treble * 0.4;
                    let radius = 0.15 + 0.06 * unit(bands[(i * 5) % 32]) + envelope * 0.04;
                    self.blobs.push(Blob::round(
                        angle.cos() * reach * aspect * 0.85,
                        angle.sin() * reach * 0.8,
                        radius,
                    ));
                }
            }
            2 => {
                // Eighteen lanes of falling, vertically stretched droplets.
                let fall = 0.55 + energy * 0.9;
                for i in 0..12 {
                    let lane = (i as f32 * 0.618_034).rem_euclid(1.0);
                    let x = (lane * 2.0 - 1.0) * aspect * 0.92;
                    let speed = 0.35 + 0.02 * (i % 5) as f32;
                    let span = 2.4;
                    let y = (t * speed * fall + i as f32 * 0.37).rem_euclid(span) - span * 0.5;
                    let radius =
                        0.060 + 0.025 * unit(bands[(i * 7) % 32]) + treble * 0.02 + envelope * 0.02;
                    self.blobs.push(Blob::oval(
                        x + (t * 0.4 + i as f32).sin() * 0.03,
                        y,
                        radius,
                        radius * 2.6,
                    ));
                }
            }
            3 => {
                // A jittering sheet of cells whose membranes sit on the field
                // threshold, with a nucleus at each source.
                let cols = if aspect > 1.3 { 7 } else { 6 };
                let rows = 4;
                for gy in 0..rows {
                    for gx in 0..cols {
                        let i = gy * cols + gx;
                        let px = ((gx as f32 + 0.5) / cols as f32 * 2.0 - 1.0) * aspect * 0.94;
                        let py = ((gy as f32 + 0.5) / rows as f32 * 2.0 - 1.0) * 0.88;
                        let wobble = 0.018 + mid * 0.015;
                        let x = px + (t * 0.55 + i as f32 * 1.7).sin() * wobble;
                        let y = py + (t * 0.47 + i as f32 * 2.3).cos() * wobble;
                        let radius = 0.09
                            + 0.025 * unit(bands[(i * 3) % 32])
                            + 0.015 * mid
                            + 0.012 * (audio.beat_phase * TAU + i as f32).sin();
                        self.blobs.push(Blob::round(x, y, radius));
                    }
                }
            }
            4 => {
                // A single meniscus deformed by the waveform, plus satellites.
                center = [0.0, 0.05 * (t * 0.6).sin()];
                let radius = 0.58 + bass * 0.12 + 0.02 * (t * 1.7).sin();
                size = [radius, radius];
                spin = -2.2 + t * 0.12;
                for i in 0..2 {
                    let angle = t * (0.45 + i as f32 * 0.2) + i as f32 * PI;
                    self.blobs.push(Blob::round(
                        center[0] + angle.cos() * radius * 1.25,
                        center[1] + angle.sin() * radius * 0.95,
                        0.09 + 0.04 * unit(bands[8 + i * 5]) + treble * 0.03,
                    ));
                }
            }
            5 => {
                // Two bodies whose separation follows the energy envelope.
                let separation = 0.26 + energy * 0.55 + envelope * 0.18 + 0.04 * (t * 1.3).sin();
                let radius = 0.34 + bass * 0.12;
                center = [separation * aspect * 0.7, 0.0];
                size = [radius, radius];
                self.blobs.push(Blob::round(
                    -separation * aspect * 0.7,
                    0.06 * (t * 0.9).sin(),
                    radius,
                ));
                self.blobs.push(Blob::round(
                    separation * aspect * 0.7,
                    -0.04 * (t * 0.8).cos(),
                    radius,
                ));
                for i in 0..2 {
                    let angle = t * (0.6 + i as f32 * 0.25) + i as f32 * PI;
                    self.blobs.push(Blob::round(
                        angle.cos() * aspect * 0.75,
                        angle.sin() * 0.6,
                        0.10 + 0.03 * unit(bands[i * 9]),
                    ));
                }
            }
            6 => {
                // A bead chain on a Lissajous path; treble pinches the neck.
                let beads = 56;
                for i in 0..beads {
                    let s = i as f32 / (beads - 1) as f32;
                    let wave = audio.waveform[(s * 31.0) as usize];
                    let x = (s * 2.6 * PI + t * 0.75).sin() * aspect * 0.78
                        + (t * 0.3 + s * 7.0).sin() * 0.05 * aspect;
                    let y = (s * 1.9 * PI + t * 0.52).cos() * 0.72 + wave * 0.06;
                    let pinch = 1.0 - treble * 0.30 * (s * PI * 9.0).sin().abs();
                    let radius = (0.035 + 0.045 * (s * PI).sin()) * pinch.max(0.35) + bass * 0.012;
                    self.blobs.push(Blob::round(x, y, radius));
                }
            }
            7 => {
                // Eight petals on a ring, sized by the chroma profile.
                let angle = t * 0.16;
                self.blobs.push(Blob::round(0.0, 0.0, 0.13 + bass * 0.04));
                for i in 0..8 {
                    let a = i as f32 * TAU / 8.0 + angle;
                    let chroma = unit(audio.chroma[i] + audio.chroma[(i + 3) % 12] * 0.5);
                    let distance = 0.48 + 0.14 * chroma + 0.03 * (t * 0.9 + i as f32).sin();
                    let radius =
                        0.08 + 0.07 * chroma + bass * 0.04 + 0.015 * unit(bands[(i * 4) % 32]);
                    self.blobs.push(Blob::round(
                        a.cos() * distance * aspect * 0.8,
                        a.sin() * distance * 0.78,
                        radius,
                    ));
                }
            }
            8 => {
                // A regular lattice that swells into a connected net on attacks.
                let cols = if aspect > 1.1 { 6 } else { 5 };
                let rows = 4;
                let attack = envelope.max(unit(audio.kick));
                for gy in 0..rows {
                    for gx in 0..cols {
                        let i = gy * cols + gx;
                        let x = ((gx as f32 + 0.5) / cols as f32 * 2.0 - 1.0) * aspect * 0.93;
                        let y = ((gy as f32 + 0.5) / rows as f32 * 2.0 - 1.0) * 0.90;
                        let pull = attack * 0.10;
                        let angle = t * 0.8 + i as f32 * 1.9;
                        let radius =
                            0.085 + 0.025 * unit(bands[(i * 5) % 32]) + attack * 0.055 + 0.02 * mid;
                        self.blobs.push(Blob::round(
                            x + angle.cos() * pull + (t * 2.3 + i as f32).sin() * treble * 0.02,
                            y + angle.sin() * pull,
                            radius,
                        ));
                    }
                }
            }
            9 => {
                // A wide pool that squashes on bass while droplets fall into it.
                let squash = 1.0 + bass * 0.45;
                let rx = 0.62 * aspect * squash;
                let ry = 0.44 * (1.0 - bass * 0.30).max(0.4);
                center = [0.0, 0.08 * (t * 0.7).sin() - envelope * 0.06];
                size = [rx, ry];
                self.blobs.push(Blob::oval(center[0], center[1], rx, ry));
                for i in 0..4 {
                    let lane = i as f32 * 0.37 - 0.55;
                    let drop =
                        ((t * (0.35 + 0.06 * i as f32) * (0.7 + energy)).rem_euclid(1.7)) - 0.85;
                    let radius = 0.06 + 0.03 * unit(bands[(i * 8) % 32]) + treble * 0.02;
                    self.blobs
                        .push(Blob::oval(lane * aspect * 1.3, drop, radius, radius * 1.8));
                }
            }
            _ => unreachable!(),
        }

        let blobs = self.blobs.as_slice();
        let mut put = |i: usize, j: usize, mark: char, phase: f32, light: f32| {
            paint(
                buf,
                visible.x + i as u16,
                visible.y + j as u16,
                mark,
                theme.sample(palette, phase, light),
            );
        };

        match mode {
            0 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, _, _) = sample(blobs, x, y);
                        let (mark, light) = if value >= 1.35 {
                            ('█', 0.30 + value * 0.22)
                        } else if value >= 1.0 {
                            ('▓', 0.28 + value * 0.18)
                        } else if value >= 0.72 {
                            ('▒', 0.24 + value * 0.16)
                        } else if value >= 0.46 {
                            ('░', 0.18 + value * 0.14)
                        } else if value >= 0.26 {
                            ('·', 0.13)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = 0.06 + value * 0.09 + x * 0.04 + t * 0.01;
                            put(i, j, mark, phase, (light + envelope * 0.25).min(0.95));
                        }
                    }
                }
            }
            1 => {
                const LEVELS: [f32; 6] = [0.28, 0.52, 0.82, 1.20, 1.65, 2.20];
                let tolerance = 0.024 + onset * 0.014;
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, _, _) = sample(blobs, x, y);
                        let mut best = f32::INFINITY;
                        let mut level = 0;
                        for (k, &target) in LEVELS.iter().enumerate() {
                            let delta = (value - target).abs();
                            if delta < best {
                                best = delta;
                                level = k;
                            }
                        }
                        if best < tolerance {
                            let mark = ['·', '∘', '○', '◦', '∙', '·'][level];
                            let light =
                                (0.28 + 0.55 * (1.0 - best / tolerance) + energy * 0.1).min(0.9);
                            let phase = 0.08 + level as f32 * 0.13 + x * 0.05 + t * 0.01;
                            put(i, j, mark, phase, light);
                        }
                    }
                }
            }
            2 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, _, index) = sample(blobs, x, y);
                        let (mark, light) = if value >= 2.0 {
                            ('●', 0.8)
                        } else if value >= 1.2 {
                            ('○', 0.65)
                        } else if value >= 0.7 {
                            ('∘', 0.42)
                        } else if value >= 0.35 {
                            ('·', 0.16)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = (0.48 + index as f32 * 0.13 + t * 0.02 + value * 0.02)
                                .rem_euclid(1.0);
                            put(i, j, mark, phase, (light + treble * 0.12).min(0.95));
                        }
                    }
                }
            }
            3 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, distance, index) = sample(blobs, x, y);
                        let (mark, light) = if value >= 1.0 {
                            if distance < 0.05 {
                                ('▪', 0.9)
                            } else if (value - 1.0).abs() < 0.08 {
                                ('█', 0.75)
                            } else {
                                ('▒', 0.2)
                            }
                        } else if value >= 0.78 {
                            ('░', 0.24)
                        } else if value >= 0.52 {
                            ('·', 0.12)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = 0.08 + value * 0.08 + index as f32 * 0.03 + t * 0.01;
                            put(i, j, mark, phase, (light + mid * 0.12).min(0.95));
                        }
                    }
                }
            }
            4 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let dx = x - center[0];
                        let dy = y - center[1];
                        let distance = dx.hypot(dy);
                        let theta = dy.atan2(dx);
                        let wave_index = ((theta.rem_euclid(TAU) / TAU) * 31.0) as usize;
                        let wave = audio.waveform[wave_index.min(31)];
                        let surface = size[0]
                            * (1.0
                                + 0.10 * (3.0 * theta + t * 0.8).sin()
                                + 0.05 * (5.0 * theta - t * 1.1).sin()
                                + wave * 0.09
                                + treble * 0.05 * (9.0 * theta + t * 2.0).sin());
                        let q = distance / surface.max(0.05);
                        if q < 1.0 {
                            let rim = ((q - 0.90) / 0.10).clamp(0.0, 1.0);
                            let mut delta = (theta - spin).rem_euclid(TAU);
                            if delta > PI {
                                delta = TAU - delta;
                            }
                            let glint = (-delta * delta * 2.2 - (q - 0.62).powi(2) * 22.0).exp();
                            let (mark, light) = if glint > 0.55 {
                                ('✦', 0.85)
                            } else if rim > 0.5 {
                                ('▓', 0.55 + rim * 0.3)
                            } else {
                                ('█', 0.12 + 0.35 * q * q + glint * 0.4)
                            };
                            let phase = 0.10 + q * 0.22 + glint * 0.35 + x * 0.02;
                            put(i, j, mark, phase, (light + envelope * 0.1).min(0.95));
                        } else if q < 1.10 {
                            put(i, j, '·', 0.2 + q * 0.1, 0.18);
                        } else {
                            let (value, _, index) = sample(blobs, x, y);
                            if value >= 0.9 {
                                let mark = if value >= 1.8 { '◦' } else { '·' };
                                let phase = 0.55 + index as f32 * 0.2;
                                put(i, j, mark, phase, (0.35 + value * 0.15).min(0.9));
                            }
                        }
                    }
                }
            }
            5 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, _, _) = sample(blobs, x, y);
                        let bridge = x.abs() < center[0] * 0.95 && x.abs() > 0.02;
                        let (mark, light) = if value >= 1.25 {
                            ('█', 0.45 + value * 0.12)
                        } else if value >= 0.95 {
                            if bridge {
                                ('═', 0.7)
                            } else {
                                ('▓', 0.5)
                            }
                        } else if value >= 0.70 {
                            if bridge {
                                ('─', 0.42)
                            } else {
                                ('▒', 0.3)
                            }
                        } else if value >= 0.45 {
                            ('░', 0.22)
                        } else if value >= 0.26 {
                            ('·', 0.13)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = 0.08 + value * 0.06 + envelope * 0.18 + x * 0.02;
                            put(i, j, mark, phase, (light + energy * 0.12).min(0.95));
                        }
                    }
                }
            }
            6 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, distance, index) = sample(blobs, x, y);
                        let phase = 0.1 + index as f32 * 0.008 + t * 0.01;
                        if value >= 1.6 {
                            put(i, j, '●', phase, 0.85);
                        } else if value >= 1.0 {
                            put(i, j, '•', phase, 0.68);
                        } else if distance < 0.55 {
                            let tangent = if index > 0 && index + 1 < blobs.len() {
                                let a = blobs[index - 1];
                                let b = blobs[index + 1];
                                (b.y - a.y) / (b.x - a.x).abs().max(1e-3) * (b.x - a.x).signum()
                            } else {
                                0.0
                            };
                            let mark = if tangent.abs() < 0.45 {
                                '─'
                            } else if tangent.abs() > 2.2 {
                                '│'
                            } else if tangent > 0.0 {
                                '╲'
                            } else {
                                '╱'
                            };
                            let light = 0.22 + (1.0 - distance / 0.55) * 0.3;
                            put(i, j, mark, phase, light);
                        } else if value >= 0.30 {
                            put(i, j, '·', phase, 0.14);
                        }
                    }
                }
            }
            7 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, _, index) = sample(blobs, x, y);
                        let (mark, light) = if value >= 2.2 {
                            ('✿', 0.9)
                        } else if value >= 1.4 {
                            ('◆', 0.62)
                        } else if value >= 1.0 {
                            ('▒', 0.3)
                        } else if (value - 0.82).abs() < 0.035 {
                            ('∘', 0.48)
                        } else if value >= 0.55 {
                            ('·', 0.16)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = 0.12
                                + index as f32 / 9.0 * 0.55
                                + t * 0.02
                                + unit(audio.centroid) * 0.1;
                            put(i, j, mark, phase, (light + treble * 0.1).min(0.95));
                        }
                    }
                }
            }
            8 => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let (value, distance, index) = sample(blobs, x, y);
                        let (mark, light) = if distance < 0.05 || value >= 4.0 {
                            ('▪', 0.9)
                        } else if value >= 2.0 {
                            ('▒', 0.35)
                        } else if value >= 1.0 {
                            ('░', 0.22)
                        } else if value >= 0.5 {
                            ('·', 0.12)
                        } else {
                            (' ', 0.0)
                        };
                        if mark != ' ' {
                            let phase = 0.12 + index as f32 * 0.05 + t * 0.02;
                            put(i, j, mark, phase, (light + envelope * 0.12).min(0.95));
                        }
                    }
                }
            }
            _ => {
                for (j, &y) in self.ys.iter().enumerate() {
                    for (i, &x) in self.xs.iter().enumerate() {
                        let dx = (x - center[0]) / size[0].max(1e-3);
                        let dy = (y - center[1]) / size[1].max(1e-3);
                        let q = dx.hypot(dy);
                        let (value, _, index) = sample(blobs, x, y);
                        if q < 1.0 {
                            let ripple = ((y * 18.0 - t * 2.0) + x * 1.5).sin();
                            let (mark, light) = if value >= 2.6 {
                                if ripple > 0.93 {
                                    ('≈', 0.55)
                                } else {
                                    ('█', 0.5)
                                }
                            } else if value >= 1.6 {
                                ('▓', 0.36)
                            } else if value >= 1.0 {
                                ('▒', 0.28)
                            } else if value >= 0.82 {
                                ('≈', 0.6)
                            } else if value >= 0.55 {
                                ('░', 0.2)
                            } else {
                                ('·', 0.12)
                            };
                            let phase = 0.08 + value * 0.05 + swell * 0.15 + x * 0.02;
                            put(i, j, mark, phase, (light + energy * 0.1).min(0.95));
                        } else if value >= 1.0 {
                            let mark = if value >= 1.8 { '○' } else { '∘' };
                            let phase = 0.5 + index as f32 * 0.1;
                            put(i, j, mark, phase, (0.4 + value * 0.15).min(0.9));
                        } else if value >= 0.4 {
                            put(i, j, '·', 0.25 + q * 0.1, 0.14);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 80, 30)
    }

    fn snapshot(style: usize, audio: &Reactivity, frames: usize) -> (String, Renderer) {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(area());
        for frame in 0..frames {
            renderer.draw(
                &mut buf,
                area(),
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                audio,
                &Theme::default(),
            );
        }
        (
            buf.content.iter().map(|cell| cell.symbol()).collect(),
            renderer,
        )
    }

    fn loud() -> Reactivity {
        Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.25,
            novelty: 1.0,
            centroid: 1.0,
            flatness: 1.0,
            waveform: [0.8; 32],
            transients: [1.0; 4],
            chroma: [1.0; 12],
        }
    }

    #[test]
    fn ten_unique_names() {
        assert_eq!(NAMES.len(), 10);
        let unique: std::collections::HashSet<_> = NAMES.iter().collect();
        assert_eq!(unique.len(), NAMES.len());
    }

    #[test]
    fn every_style_paints_and_clips() {
        for style in 0..NAMES.len() {
            let (image, _) = snapshot(style, &Reactivity::default(), 48);
            assert!(
                image.chars().any(|c| c != ' '),
                "style {style} painted nothing"
            );
        }
        for style in 0..NAMES.len() {
            let outer = Rect::new(0, 0, 60, 24);
            let target = Rect::new(5, 4, 40, 14);
            let mut buf = Buffer::empty(outer);
            for cell in &mut buf.content {
                cell.set_char('X');
            }
            let mut renderer = Renderer::default();
            renderer.draw(
                &mut buf,
                target,
                1.0,
                style,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            for y in outer.y..outer.bottom() {
                for x in outer.x..outer.right() {
                    if !target.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), "X", "style {style} wrote outside");
                    }
                }
            }
        }
    }

    #[test]
    fn styles_are_distinct_and_deterministic() {
        let quiet = Reactivity::default();
        let mut images = std::collections::HashSet::new();
        for style in 0..NAMES.len() {
            let (image, _) = snapshot(style, &quiet, 48);
            assert!(images.insert(image.clone()), "duplicate style {style}");
            let (again, _) = snapshot(style, &quiet, 48);
            assert_eq!(image, again, "style {style} is not deterministic");
        }
        assert_eq!(images.len(), NAMES.len());
    }

    #[test]
    fn audio_changes_every_style() {
        let quiet = Reactivity::default();
        let loud = loud();
        for style in 0..NAMES.len() {
            let (calm, _) = snapshot(style, &quiet, 48);
            let (hot, _) = snapshot(style, &loud, 48);
            assert_ne!(calm, hot, "style {style} ignored audio");
        }
    }
}
