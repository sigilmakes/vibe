//! Tunnel and warp scenes: perspective grids, portals, and travel toward a
//! vanishing point. Geometry is evaluated in physical cell coordinates, so rings
//! stay circular in terminals of any shape. Patterns are closed-form functions
//! of integrated time and audio envelopes; no per-frame allocation is needed.

use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Hyperspace Rail",
    "Wormhole Throat",
    "Portal Rings",
    "Vanishing Grid",
    "Folded Corridor",
    "Warp Streaks",
    "Depth Well",
    "Event Horizon",
    "Tesseract Lane",
    "Singularity Braid",
];

const ENV: usize = 7;
const FILL: [char; 5] = ['·', '░', '▒', '▓', '█'];

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    /// Radians, wrapped to a whole number of turns.
    phase: f32,
    /// Abstract travel distance, wrapped to a period shared by every pattern.
    scroll: f32,
    /// Bass, mid, treble, kick, onset, centroid, flatness.
    env: [f32; ENV],
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash(i: i32) -> f32 {
    let mut n = (i as u32)
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    n = ((n >> ((n >> 28) + 4)) ^ n).wrapping_mul(277_803_737);
    ((n >> 22) ^ n) as f32 / u32::MAX as f32
}

fn line_glyph(tangent: [f32; 2]) -> char {
    let slope = tangent[1] / (tangent[0] + 0.00001);
    if slope.abs() < 0.45 {
        '─'
    } else if slope.abs() > 2.2 {
        '│'
    } else if slope > 0.0 {
        '╲'
    } else {
        '╱'
    }
}

fn fill_char(brightness: f32) -> char {
    let last = FILL.len() - 1;
    let index = (unit(brightness) * last as f32).round() as usize;
    FILL[index.min(last)]
}

/// Pitch-class centre of mass, so harmony tints the scene instead of only
/// flashing on attacks.
fn chroma_hue(audio: &Reactivity) -> f32 {
    let mut weight = 0.0;
    let mut sum = 0.0;
    for (i, c) in audio.chroma.iter().enumerate() {
        let value = unit(*c);
        weight += value;
        sum += value * i as f32;
    }
    if weight > 1e-3 {
        (sum / weight) / 12.0
    } else {
        unit(audio.centroid) * 0.5
    }
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.05));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        let targets = [
            unit(audio.bass),
            unit(audio.mid),
            unit(audio.treble),
            unit(audio.kick),
            unit(audio.onset),
            unit(audio.centroid),
            unit(audio.flatness),
        ];
        let follow = 1.0 - (-dt * 9.0).exp();
        for (value, target) in self.env.iter_mut().zip(targets) {
            *value += (target - *value) * follow;
        }
        self.phase = (self.phase + dt * (0.4 + unit(audio.energy) * 1.2 + self.env[1] * 0.5))
            .rem_euclid(TAU * 16.0);
        self.scroll = (self.scroll + dt * (0.5 + unit(audio.energy) * 1.5 + self.env[3] * 0.4))
            .rem_euclid(20.0);
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
        let t = self.phase;
        let scroll = self.scroll;
        let [bass, mid, treble, kick, onset, centroid, flatness] = self.env;
        let energy = unit(audio.energy);
        let beat = unit(audio.beat);
        let beat_phase = if audio.locked_beat_phase().is_finite() {
            audio.locked_beat_phase().rem_euclid(1.0)
        } else {
            0.0
        };
        let transient = audio
            .transients
            .iter()
            .copied()
            .map(unit)
            .fold(0.0, f32::max);
        let chroma = chroma_hue(audio);
        let scale = 2.0 / area.height.max(1) as f32;
        let cx = area.x as f32 + (area.width as f32 - 1.0) * 0.5;
        let cy = area.y as f32 + (area.height as f32 - 1.0) * 0.5;
        let background = theme.sample(palette, 0.0, 0.0);
        let columns = visible.width.max(1) as usize;

        for row in visible.y..visible.bottom() {
            let py = (row as f32 - cy) * scale;
            for col in visible.x..visible.right() {
                let px = (col as f32 - cx) * scale;
                let band = unit(bands[((col - visible.x) as usize * 31 / columns) % 32]);
                let (brightness, hue, glyph) = match mode {
                    0 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let angular =
                            unit(bands[(((th / TAU + 0.5) * 28.0) as i32).rem_euclid(32) as usize]);
                        let depth = 1.0 / (r + 0.06);
                        let z = depth * 2.2 + scroll * 1.7;
                        let spokes = 12.0 + (treble * 10.0).round();
                        let u = th / TAU * spokes + beat_phase * 0.5;
                        let du = (u - u.round()).abs();
                        let spoke = (1.0 - du * 2.0).clamp(0.0, 1.0).powf(3.0);
                        let dz = (z - z.round()).abs();
                        let ring = (1.0 - dz * 2.0).clamp(0.0, 1.0).powf(4.0);
                        let fade =
                            ((r - 0.015) / 0.18).clamp(0.0, 1.0) * (1.0 - (r / 3.2).min(0.85));
                        let core = (-(r / 0.05).powi(2)).exp() * 0.45;
                        let bright = (spoke * (0.35 + mid * 0.45 + angular * 0.3)
                            + ring * (0.30 + treble * 0.3 + band * 0.3))
                            * fade
                            + core;
                        let glyph = if spoke > 0.55 && ring > 0.55 {
                            '┼'
                        } else if ring >= spoke {
                            line_glyph([-py, px])
                        } else {
                            line_glyph([px, py])
                        };
                        let hue = (chroma * 0.55 + depth * 0.035 + th / TAU * 0.05 + 0.15)
                            .rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    1 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let wi = (((th / TAU + 0.5) * 31.0) as i32).rem_euclid(32) as usize;
                        let wf = unit(audio.waveform[wi] * 0.5 + 0.5);
                        let wobble = 1.0 + 0.7 * (wf - 0.5) + 0.3 * bass * (th * 3.0 + t).sin();
                        let depth = 1.0 / (r * wobble.max(0.25) + 0.06);
                        let rings = depth * (4.8 + treble * 1.6) + scroll * 1.7;
                        let ring = (rings * PI).sin().abs().powf(2.5 + treble * 3.5);
                        let veins = ((th * (6.0 + mid * 10.0)) + depth * 2.5 + scroll * 1.2)
                            .sin()
                            .abs()
                            .powf(14.0);
                        let fade =
                            ((r - 0.015) / 0.12).clamp(0.0, 1.0) * (1.0 - (r / 2.6).min(0.92));
                        let bright = (ring * 0.95 + veins * 0.22)
                            * (0.5 + energy * 0.4 + band * 0.25)
                            * fade;
                        let hue =
                            (th / TAU * 0.6 + depth * 0.03 + chroma * 0.6 + 0.05).rem_euclid(1.0);
                        (bright, hue, fill_char(bright))
                    }
                    2 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let pulse = 0.05 * kick + 0.02 * beat;
                        let r0 = 0.12 + pulse;
                        let spacing = 0.14 + mid * 0.035;
                        let k = ((r - r0) / spacing).clamp(0.0, 9.0).round();
                        let ring_r = r0 + k * spacing + 0.012 * kick * ((k as usize) % 3) as f32;
                        let d = (r - ring_r).abs();
                        let ring = (1.0 - d / (0.036 + 0.014 * treble)).clamp(0.0, 1.0);
                        let su = th / TAU * 20.0;
                        let spoke = (1.0 - (su - su.round()).abs() * 2.0)
                            .clamp(0.0, 1.0)
                            .powf(5.0)
                            * if r > r0 { 1.0 } else { 0.0 }
                            * (0.25 + treble * 0.75);
                        let core =
                            (-(r / (0.08 + 0.04 * bass)).powi(2)).exp() * (0.5 + onset * 0.5);
                        let bright = (ring * 0.9 + spoke * 0.35 + core * 0.9)
                            * (0.55 + energy * 0.3 + band * 0.2);
                        let glyph = if core > 0.55 && r < r0 * 0.7 {
                            '●'
                        } else if ring > 0.35 {
                            '◎'
                        } else if spoke > 0.2 {
                            '·'
                        } else {
                            ' '
                        };
                        let hue = (0.1 * k + chroma * 0.5 + th / TAU * 0.2 + 0.12).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    3 => {
                        let horizon = -0.3 + 0.36 * bass - 0.1 * kick;
                        let above = py < horizon;
                        let d = (py - horizon).abs().max(1e-3);
                        let depth = 1.0 / (d * 3.0 + 0.035);
                        let z = depth * 0.8 + scroll * 1.4;
                        let dz = (z - z.round()).abs();
                        let hline = (1.0 - dz * 2.0).clamp(0.0, 1.0).powf(4.0);
                        let u = px / (d + 0.15) * 0.35;
                        let du = (u - u.round()).abs();
                        let vline = (1.0 - du * 2.0).clamp(0.0, 1.0).powf(3.0);
                        let near = 1.0 / (1.0 + d * 4.0);
                        let grid = (hline * 0.7 + vline * 0.65)
                            * (0.35 + 0.65 * near)
                            * (0.45 + 0.55 * band)
                            * if above { 0.55 } else { 1.0 };
                        let sun_r = 0.2 + 0.07 * kick + 0.03 * beat;
                        let sy = py - (horizon + sun_r * 0.25);
                        let sr = (px * px + sy * sy).sqrt();
                        let stripe = if (py / 0.045).rem_euclid(1.0) > 0.32 {
                            1.0
                        } else {
                            0.55
                        };
                        let sun = (1.0 - (sr / sun_r).clamp(0.0, 1.0)).powf(0.7)
                            * stripe
                            * (0.5 + 0.5 * onset);
                        let star = if above {
                            let h = hash((col as i32) * 131 + (row as i32) * 977 + 3);
                            if h > 0.985 {
                                (0.4 + treble * 0.6) * (0.5 + 0.5 * h)
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        let glow = (-((py - horizon) / (0.015 + 0.02 * bass)).powi(2)).exp()
                            * (0.4 + 0.4 * mid);
                        let bright = (grid * 0.8 + sun * 0.9 + glow * 0.7 + star).min(1.0);
                        let glyph = if sun > 0.3 && sr < sun_r {
                            '█'
                        } else if star > 0.0 {
                            '·'
                        } else if hline > 0.45 && vline > 0.45 {
                            '┼'
                        } else if hline >= vline {
                            '─'
                        } else {
                            line_glyph([px, d * 0.35])
                        };
                        let hue = (0.55 + sun * 0.2 + chroma * 0.35 + near * 0.05).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    4 => {
                        let skew = 0.12 * mid * (scroll * 0.7 + py * 2.6).sin()
                            + 0.08 * (scroll * 0.5 + py * 1.3).cos();
                        let x = px + skew * py;
                        let y = py + 0.1 * mid * (scroll * 0.6 + px * 1.9).sin();
                        let m = x.abs().max(y.abs());
                        let depth = 1.0 / (m + 0.03);
                        let z = depth * 1.1 + scroll * 1.3;
                        let dz = (z - z.round()).abs();
                        let frame = (1.0 - dz * 2.0).clamp(0.0, 1.0).powf(5.0);
                        let diag = (x.abs() - y.abs()).abs();
                        let crease = (1.0 - diag / (0.03 + 0.02 * treble)).clamp(0.0, 1.0)
                            * (0.25 + onset * 0.75);
                        let fade = ((m - 0.02) / 0.06).clamp(0.0, 1.0) * (1.0 - (m / 2.6).min(0.9));
                        let bright =
                            (frame * 0.7 + crease * 0.5) * fade * (0.5 + energy * 0.3 + band * 0.2);
                        let glyph = if crease > frame && crease > 0.4 {
                            if x * y > 0.0 {
                                '╲'
                            } else {
                                '╱'
                            }
                        } else if x.abs() > y.abs() {
                            '│'
                        } else {
                            '─'
                        };
                        let hue = (depth * 0.04 + chroma * 0.55 + 0.2).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    5 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        // Two angular layers of radial dashes, each a closed-form
                        // function of travel distance so a frame never allocates.
                        let layer = |bins: f32, salt: i32| -> (f32, f32) {
                            let u = (th / TAU + 0.5) * bins;
                            let idx = u.floor();
                            let f = u - idx;
                            let seed = hash(idx as i32 + salt);
                            let speed = 0.55 + (seed * 18.0).round() * 0.05;
                            let head = (scroll * speed + hash(idx as i32 + salt + 91) * 4.0)
                                .rem_euclid(1.0);
                            let len = 0.22 + 0.3 * seed + 0.2 * kick;
                            let radius = head * 1.9;
                            let along = (r - (radius - len)) / len;
                            let tail = if (0.0..1.0).contains(&along) {
                                along.powf(1.4)
                            } else {
                                0.0
                            };
                            let width = (1.0 - (f * 2.0 - 1.0).abs().powi(2)).max(0.0).powf(1.5);
                            (tail * width, seed)
                        };
                        let (near, seed) = layer(26.0, 0);
                        let (far, _) = layer(38.0, 37);
                        let core = (-(r / 0.05).powi(2)).exp() * (0.3 + onset * 0.5);
                        let bright = (near * 0.9 + far * 0.5 + core)
                            * (0.55 + treble * 0.6 + flatness * 0.3)
                            * (1.0 - (r / 2.6).min(0.9));
                        let hue =
                            (seed * 0.8 + chroma * 0.4 + centroid * 0.2 + 0.1).rem_euclid(1.0);
                        (bright, hue, line_glyph([px, py]))
                    }
                    6 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let hole = 0.24 + 0.07 * bass + 0.04 * kick;
                        let outside = (r - hole).max(0.0);
                        let edge = (1.0 - (outside / 1.05).min(1.0)).max(0.0);
                        let fade = edge * edge * (3.0 - 2.0 * edge) * 0.8;
                        let rim = if r >= hole {
                            (-(outside / (0.045 + 0.02 * kick)).powi(2)).exp()
                        } else {
                            0.0
                        };
                        let spacing = 9.0 + mid * 3.0;
                        let ripple = (outside * spacing - scroll * 4.0 + 0.5 * (th * 2.0).sin())
                            .sin()
                            .abs()
                            .powf(5.0)
                            * fade;
                        let glow = (-outside * 6.0).exp() * 0.1;
                        let bright =
                            rim * (0.85 + onset * 0.15) + glow + ripple * (0.32 + treble * 0.25);
                        let glyph = if rim > 0.6 { '●' } else { fill_char(bright) };
                        let hue = (0.5 + chroma * 0.5 + outside * 0.15).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    7 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let rad = 0.22 + 0.06 * kick + 0.02 * beat;
                        let inside = r < rad;
                        let disk_w = 0.09 + 0.06 * bass;
                        let ellipse =
                            ((px / (1.9 * rad.max(0.1))).powi(2) + (py / disk_w).powi(2)).sqrt();
                        let band = (-((ellipse - 1.0) / (0.3 + 0.15 * bass)).powi(2)).exp();
                        let swirl = 0.55 + 0.45 * (th * 3.0 - r * 12.0 + scroll * 2.2).sin();
                        let photon = (-((r - rad * 1.03) / (0.035 + 0.012 * treble)).powi(2)).exp();
                        let flare = (-((r - rad * 1.6) / (0.03 + 0.05 * transient)).powi(2)).exp()
                            * transient
                            * 0.7;
                        let halo = (-((r - rad * 2.2) / (0.5 + 0.3 * bass)).powi(2)).exp()
                            * (-((py / (0.16 + 0.12 * bass)).powi(2))).exp()
                            * 0.3;
                        let bright = if inside {
                            0.0
                        } else {
                            (band * (0.5 + 0.4 * swirl) + photon * 0.9 + flare + halo)
                                * (0.65 + energy * 0.35)
                        };
                        let glyph = if inside {
                            ' '
                        } else if photon > 0.5 {
                            '█'
                        } else if band > 0.35 {
                            '▒'
                        } else {
                            '·'
                        };
                        let hue = (chroma * 0.6 + th / TAU * 0.1 + 0.35).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    8 => {
                        let th = py.atan2(px);
                        let ang = t * 0.5 + scroll * 0.25 + beat_phase * 0.5;
                        let pulse = 1.0 + kick * 0.13 + beat * 0.06;
                        let mut bright = 0.0;
                        let mut glyph = ' ';
                        let mut nearest = 0usize;
                        for k in 0..5usize {
                            let turn = ang + k as f32 * 0.2 * if k % 2 == 0 { 1.0 } else { -1.0 };
                            let (s, c) = turn.sin_cos();
                            let x = px * c - py * s;
                            let y = px * s + py * c;
                            let m = x.abs().max(y.abs());
                            let radius = 1.15
                                * 0.7f32.powi(k as i32)
                                * pulse
                                * (1.0 + 0.05 * (scroll * 0.8 + k as f32 * 1.7).sin());
                            let d = (m - radius).abs();
                            let line = (1.0 - d / (0.04 + 0.012 * treble)).clamp(0.0, 1.0)
                                * (1.0 - k as f32 * 0.07);
                            if line > bright {
                                bright = line;
                                nearest = k;
                                glyph = if x.abs() > y.abs() { '│' } else { '─' };
                            }
                        }
                        let su = th / (PI * 0.25);
                        let lane = (1.0 - (su - su.round()).abs() * 2.0)
                            .clamp(0.0, 1.0)
                            .powf(4.0)
                            * (0.1 + 0.3 * mid);
                        if lane > bright {
                            bright = lane;
                            glyph = line_glyph([px, py]);
                        }
                        let bright = bright
                            * (0.45 + energy * 0.3 + band * 0.25)
                            * (1.0 - (px.hypot(py) / 3.0).min(0.8));
                        let hue = (chroma * 0.6 + nearest as f32 * 0.08 + 0.1).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    _ => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        let depth = 1.0 / (r + 0.07);
                        let twist = th * 2.0 + depth * 5.0 - scroll * 2.0 + beat_phase * 0.5;
                        let sep = 0.4 + 0.5 * bass + 0.3 * mid;
                        let wobble = 0.35 * (depth * 2.0 + scroll * 0.5).sin() * sep;
                        let strand = |phase: f32| {
                            let x = phase / PI;
                            let d = (x - x.round()).abs();
                            (1.0 - d * 2.0).clamp(0.0, 1.0).powf(9.0)
                        };
                        let a = strand(twist + wobble);
                        let b = strand(twist + PI - wobble);
                        let c = strand(twist * 2.0 + 1.0) * 0.35;
                        let fade = ((r - 0.02) / 0.1).clamp(0.0, 1.0)
                            * (1.0 - (r / 2.4).min(0.92)).powf(1.5);
                        let bright = (a.max(b) * (0.6 + onset * 0.4 + transient * 0.2) + c * 0.2)
                            * (0.5 + energy * 0.3 + band * 0.2)
                            * fade;
                        let glyph = if a > b { '╱' } else { '╲' };
                        let hue =
                            (depth * 0.04 + chroma * 0.6 + th / TAU * 0.15 + 0.18).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                };

                let cell = &mut buf[(col, row)];
                cell.reset();
                cell.set_bg(background);
                if brightness > 0.03 {
                    cell.set_char(glyph);
                    cell.set_fg(theme.sample(palette, hue, unit(brightness)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const AREA: Rect = Rect::new(2, 3, 90, 30);

    fn render(style: usize, audio: &Reactivity, frames: usize) -> Buffer {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        for frame in 0..frames {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                audio,
                &Theme::default(),
            );
        }
        buf
    }

    fn symbols(buf: &Buffer) -> String {
        buf.content.iter().map(|cell| cell.symbol()).collect()
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
            beat_phase: 0.5,
            waveform: std::array::from_fn(|i| (i as f32 * 0.7).sin()),
            transients: [1.0; 4],
            novelty: 1.0,
            chroma: std::array::from_fn(|i| (i % 3) as f32 / 2.0),
            centroid: 0.8,
            flatness: 0.7,
            ..Reactivity::default()
        }
    }

    #[test]
    fn names_are_ten_and_unique() {
        assert_eq!(NAMES.len(), 10);
        let unique: HashSet<_> = NAMES.iter().collect();
        assert_eq!(unique.len(), NAMES.len());
    }

    #[test]
    fn every_style_fills_the_area_and_stays_distinct() {
        let mut images = HashSet::new();
        for (style, name) in NAMES.iter().enumerate() {
            let image = symbols(&render(style, &Reactivity::default(), 36));
            assert!(
                image.chars().any(|c| c != ' '),
                "style {style} ({name}) drew nothing"
            );
            assert!(
                images.insert(image),
                "style {style} ({name}) duplicated another scene"
            );
        }
    }

    #[test]
    fn live_audio_changes_each_scene() {
        let quiet = Reactivity::default();
        let loud = loud();
        for style in 0..NAMES.len() {
            let calm = symbols(&render(style, &quiet, 36));
            let hit = symbols(&render(style, &loud, 36));
            assert_ne!(calm, hit, "style {style} ignored the audio");
        }
    }

    #[test]
    fn rendering_clips_to_the_visible_area_and_survives_bad_input() {
        let outer = Rect::new(8, 6, 40, 18);
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(10, 8, 22, 12),
                Rect::new(0, 0, 20, 14),
                Rect::new(70, 30, 9, 9),
                Rect::new(9, 9, 0, 0),
                Rect::new(20, 15, 1, 1),
            ] {
                let mut buf = Buffer::empty(outer);
                for cell in &mut buf.content {
                    cell.set_char('!');
                }
                let audio = Reactivity {
                    bass: f32::NAN,
                    mid: f32::INFINITY,
                    ..Reactivity::default()
                };
                Renderer::default().draw(
                    &mut buf,
                    area,
                    f32::NAN,
                    style,
                    0,
                    &[f32::NAN; 32],
                    &audio,
                    &Theme::default(),
                );
                let visible = area.intersection(outer);
                for y in outer.y..outer.bottom() {
                    for x in outer.x..outer.right() {
                        if !visible.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "!", "style {style}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn renders_are_deterministic_for_the_same_timeline() {
        for style in 0..NAMES.len() {
            let first = symbols(&render(style, &loud(), 24));
            let second = symbols(&render(style, &loud(), 24));
            assert_eq!(first, second, "style {style} was not deterministic");
        }
    }

    #[test]
    #[ignore = "manual release-mode frame timing"]
    fn release_frame_timing() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 160, 50));
        for (style, name) in NAMES.iter().enumerate() {
            let mut renderer = Renderer::default();
            for frame in 0..120 {
                renderer.draw(
                    &mut buf,
                    Rect::new(0, 0, 160, 50),
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &loud(),
                    &Theme::default(),
                );
            }
            let start = std::time::Instant::now();
            for frame in 120..240 {
                renderer.draw(
                    &mut buf,
                    Rect::new(0, 0, 160, 50),
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &loud(),
                    &Theme::default(),
                );
            }
            eprintln!(
                "{name}: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 1000.0 / 120.0
            );
        }
    }
}
