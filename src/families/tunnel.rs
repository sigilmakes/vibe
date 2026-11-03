//! Tunnel and warp scenes: perspective grids, portals, and travel toward a
//! vanishing point. Geometry is evaluated in physical cell coordinates, so rings
//! stay circular in terminals of any shape. Patterns are closed-form functions
//! of integrated time and audio envelopes; no per-frame allocation is needed.

use crate::{
    palette::Theme,
    visuals::{approach_hue, Reactivity},
};
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
    /// Smoothed chroma hue, so noise does not flicker the palette.
    chroma: f32,
    /// Drop envelope: rises on an attack and releases over about half a
    /// second, so the throat pinches and then reopens.
    attack: f32,
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
    let neutral = unit(audio.centroid) * 0.5;
    if weight > 1e-3 {
        let pitch = (sum / weight) / 12.0;
        pitch * audio.tonality() + neutral * (1.0 - audio.tonality())
    } else {
        neutral
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
        self.chroma = approach_hue(self.chroma, chroma_hue(audio), (dt * 8.0).min(1.0));
        let hit = unit(audio.onset)
            .max(unit(audio.kick))
            .max(unit(audio.transients[0]))
            .max(unit(audio.novelty) * 0.8);
        self.attack = (self.attack * (-dt * 1.8).exp()).max(hit);
        self.phase = (self.phase + dt * (0.4 + unit(audio.energy) * 1.2 + self.env[1] * 0.5))
            .rem_euclid(TAU * 16.0);
        // No modular reset: scroll feeds sines with many different rates, so a
        // wrap would snap every ring and rail at once.
        self.scroll += dt * (0.5 + unit(audio.energy) * 1.5 + self.env[3] * 0.4);
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
        let attack = self.attack;
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
        let chroma = self.chroma;
        let scale = 2.0 / area.height.max(1) as f32;
        let cx = area.x as f32
            + (area.width as f32 - 1.0) * 0.5
            + audio.pan() * area.width as f32 * 0.06;
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
                        // A drop pinches the throat shut, then it springs back.
                        let collapse = 1.0 - 0.7 * attack;
                        let pulse = 0.05 * kick + 0.02 * beat;
                        let r0 = (0.12 + pulse) * collapse;
                        // Bass deepens the well: the rings pack toward the
                        // throat instead of sitting at a fixed radius.
                        let spacing =
                            (0.14 + mid * 0.035) * (1.0 - 0.25 * bass) * (1.0 - 0.3 * attack);
                        let k = ((r - r0) / spacing).clamp(0.0, 9.0).round();
                        let ring_r = r0 + k * spacing + 0.014 * kick * ((k as usize) % 3) as f32;
                        let d = (r - ring_r).abs();
                        let ring = (1.0 - d / (0.04 + 0.015 * treble)).clamp(0.0, 1.0);
                        let su = th / TAU * 20.0;
                        let spoke = (1.0 - (su - su.round()).abs() * 2.0)
                            .clamp(0.0, 1.0)
                            .powf(5.0)
                            * if r > r0 { 1.0 } else { 0.0 }
                            * (0.3 + treble * 0.7);
                        let core =
                            (-(r / (0.08 + 0.04 * bass)).powi(2)).exp() * (0.5 + onset * 0.5);
                        // A dim wash between the rings keeps the portal legible
                        // on a dark terminal instead of leaving black gaps.
                        let haze = (0.07 + 0.11 * energy + 0.05 * band)
                            * (-(r / 1.3).powi(2)).exp()
                            * (1.0 - 0.5 * attack);
                        let bright = (ring * 0.95 + spoke * 0.35 + core * 0.9 + haze * 0.5)
                            * (0.55 + energy * 0.3 + band * 0.2);
                        let glyph = if core > 0.55 && r < r0 * 0.7 {
                            '●'
                        } else if ring > 0.3 {
                            '◎'
                        } else if spoke > 0.2 {
                            '·'
                        } else {
                            fill_char(bright)
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
                        // The well deepens with bass; a drop collapses the
                        // throat and the rim rushes inward.
                        let depth = 1.0 + 1.1 * bass + 0.35 * attack;
                        let hole = (0.24 + 0.07 * bass + 0.04 * kick) * (1.0 - 0.75 * attack);
                        let outside = (r - hole).max(0.0);
                        let reach = 1.05 * depth;
                        let edge = (1.0 - (outside / reach).min(1.0)).max(0.0);
                        let fade = edge * edge * (3.0 - 2.0 * edge) * 0.8;
                        let rim = if r >= hole {
                            (-(outside / (0.055 + 0.025 * kick + 0.03 * attack)).powi(2)).exp()
                        } else {
                            0.0
                        };
                        let spacing = (9.0 + mid * 3.0) / depth;
                        let wave = (outside * spacing - scroll * 4.0 + 0.5 * (th * 2.0).sin())
                            .sin()
                            .abs()
                            .powf(3.0);
                        // The ripple keeps a visible floor, so the well reads as
                        // concentric depth rather than isolated specks.
                        let ripple = (0.12 + 0.88 * wave) * fade;
                        let glow = (-outside * 6.0).exp() * 0.16;
                        let bright = rim * (0.9 + onset * 0.1)
                            + glow
                            + ripple * (0.55 + treble * 0.3 + energy * 0.2);
                        let glyph = if rim > 0.6 {
                            '●'
                        } else if ripple > 0.5 {
                            '◎'
                        } else if bright > 0.03 {
                            fill_char(bright)
                        } else {
                            ' '
                        };
                        let hue = (0.5 + chroma * 0.5 + outside * 0.15).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    7 => {
                        let r = px.hypot(py).max(1e-3);
                        let th = py.atan2(px);
                        // The horizon shrinks on a drop and grows back with the
                        // bass, so the hole breathes instead of sitting still.
                        let rad = (0.26 + 0.12 * bass + 0.06 * kick + 0.03 * beat)
                            * (1.0 - 0.55 * attack);
                        let inside = r < rad;
                        let disk_w = (0.12 + 0.09 * bass) * (1.0 + 0.5 * bass);
                        let ellipse =
                            ((px / (2.6 * rad.max(0.1))).powi(2) + (py / disk_w).powi(2)).sqrt();
                        let band = (-((ellipse - 1.0) / (0.32 + 0.18 * bass)).powi(2)).exp();
                        let swirl = 0.55 + 0.45 * (th * 3.0 - r * 12.0 + scroll * 2.2).sin();
                        let photon = (-((r - rad * 1.03) / (0.04 + 0.014 * treble)).powi(2)).exp();
                        let lens = (-((r - rad * 3.3) / (0.03 + 0.015 * treble)).powi(2)).exp()
                            * (0.3 + 0.4 * swirl);
                        let flare = (-((r - rad * 1.6) / (0.03 + 0.05 * transient)).powi(2)).exp()
                            * transient
                            * 0.7;
                        // The drop releases a shell that travels outward as the
                        // attack envelope decays.
                        let shock_r = rad * 1.9 + (1.0 - attack) * 1.4;
                        let shock =
                            (-((r - shock_r) / (0.05 + 0.08 * attack)).powi(2)).exp() * attack;
                        let halo = (-((r - rad * 2.2) / (0.5 + 0.3 * bass)).powi(2)).exp()
                            * (-((py / (0.16 + 0.12 * bass)).powi(2))).exp()
                            * 0.3;
                        // A dim dust field fills the frame around the disk.
                        let haze = (0.16 + 0.12 * energy) * (-(r / 2.0).powi(2)).exp();
                        let bright = if inside {
                            0.0
                        } else {
                            (band * (0.75 + 0.3 * swirl)
                                + photon * 0.9
                                + lens * 0.5
                                + flare
                                + shock * 0.7
                                + halo
                                + haze * 0.4)
                                * (0.85 + energy * 0.25)
                        };
                        let glyph = if inside {
                            ' '
                        } else if photon > 0.5 {
                            '█'
                        } else if shock > 0.4 {
                            '●'
                        } else if band > 0.5 {
                            '▒'
                        } else if band > 0.05 {
                            fill_char(bright)
                        } else {
                            '·'
                        };
                        let hue = (chroma * 0.6 + th / TAU * 0.1 + 0.35).rem_euclid(1.0);
                        (bright, hue, glyph)
                    }
                    8 => {
                        let th = py.atan2(px);
                        let ang = t * 0.5 + scroll * 0.25 + beat_phase * 0.5;
                        // A drop collapses the lane: every square shrinks
                        // toward the vanishing point, then expands again.
                        let pulse = (1.0 + kick * 0.13 + beat * 0.06) * (1.0 - 0.4 * attack);
                        // Bass opens the corridor; energy and treble thicken
                        // its walls.
                        let shrink = 0.7 - 0.08 * bass;
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
                                * shrink.powi(k as i32)
                                * pulse
                                * (1.0 + 0.05 * (scroll * 0.8 + k as f32 * 1.7).sin());
                            let d = (m - radius).abs();
                            let line = (1.0 - d / (0.055 + 0.02 * energy + 0.015 * treble))
                                .clamp(0.0, 1.0)
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
                        // A faint lattice between the squares gives the lane a
                        // floor, so the corridor reads as structure, not wires.
                        let step = 0.16 + 0.14 * px.hypot(py);
                        let lx = (px / step).round();
                        let ly = (py / step).round();
                        let node = if (px / step - lx).abs() < 0.16 && (py / step - ly).abs() < 0.16
                        {
                            0.28 + 0.16 * energy
                        } else {
                            0.0
                        };
                        let (bright, glyph) = if node > bright * 0.6 {
                            (node, '·')
                        } else {
                            (bright, glyph)
                        };
                        let bright = bright
                            * (0.5 + energy * 0.3 + band * 0.2)
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
    fn scroll_keeps_accumulating_past_one_turn() {
        let mut renderer = Renderer {
            scroll: 20.0 - 0.001,
            last_time: Some(0.0),
            ..Renderer::default()
        };
        let mut buf = Buffer::empty(AREA);
        for frame in 1..=4 {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                0,
                0,
                &[0.4; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
        }
        assert!(
            renderer.scroll > 20.0,
            "scroll reset to {} instead of accumulating",
            renderer.scroll
        );
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
    fn stereo_balance_pans_every_style() {
        for style in 0..NAMES.len() {
            let base = symbols(&render(style, &Reactivity::default(), 12));
            let panned = symbols(&render(
                style,
                &Reactivity {
                    balance: 0.7,
                    ..Reactivity::default()
                },
                12,
            ));
            assert_ne!(base, panned, "style {style} ignored stereo balance");
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

    /// A bar-length audio pulse: a hard hit every half second with a drifting
    /// spectrum in between.
    fn music(frame: usize) -> Reactivity {
        let hit = if frame % 30 < 2 { 1.0 } else { 0.05 };
        let swing = 0.5 + 0.5 * (frame as f32 * 0.07).sin();
        Reactivity {
            energy: 0.45 + 0.4 * swing,
            bass: 0.3 + 0.5 * hit,
            mid: 0.25 + 0.3 * swing,
            treble: 0.2 + 0.5 * swing,
            onset: hit,
            kick: hit,
            beat: hit,
            beat_phase: (frame as f32 / 30.0).fract(),
            tempo_confidence: 0.8,
            transients: [hit, hit * 0.5, 0.2, 0.1],
            chroma: std::array::from_fn(|i| (i % 3) as f32 / 2.0),
            centroid: 0.5 + 0.2 * swing,
            flatness: 0.4,
            ..Reactivity::default()
        }
    }

    fn glyphs(buf: &Buffer) -> Vec<char> {
        buf.content
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    fn ink(glyphs: &[char]) -> f32 {
        glyphs.iter().filter(|c| **c != ' ').count() as f32 / glyphs.len() as f32
    }

    /// Median ink and median frame-to-frame glyph change over a timeline.
    fn motion(style: usize, audio: &dyn Fn(usize) -> Reactivity) -> (f32, f32) {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        let mut moves = Vec::new();
        let mut inks = Vec::new();
        let mut prev: Vec<char> = Vec::new();
        for frame in 0..150 {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                &audio(frame),
                &Theme::default(),
            );
            let current = glyphs(&buf);
            inks.push(ink(&current));
            if !prev.is_empty() {
                moves.push(
                    current
                        .iter()
                        .zip(prev.iter())
                        .filter(|(a, b)| a != b)
                        .count() as f32
                        / current.len() as f32,
                );
            }
            prev = current;
        }
        moves.sort_by(|a, b| a.partial_cmp(b).unwrap());
        inks.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (moves[moves.len() / 2], inks[inks.len() / 2])
    }

    /// A frame settled in silence, optionally followed by the first frames of
    /// a drop.
    fn settled(style: usize, drop: bool) -> Buffer {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        let quiet = Reactivity::default();
        for frame in 0..60 {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                &quiet,
                &Theme::default(),
            );
        }
        if drop {
            let hit = Reactivity {
                energy: 1.0,
                onset: 1.0,
                kick: 1.0,
                transients: [1.0; 4],
                ..Reactivity::default()
            };
            for frame in 60..63 {
                renderer.draw(
                    &mut buf,
                    AREA,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.4; 32],
                    &hit,
                    &Theme::default(),
                );
            }
        }
        buf
    }

    /// Ink in the middle third of the frame, where a throat closes.
    fn throat_ink(buf: &Buffer) -> f32 {
        let mut ink = 0;
        let mut total = 0;
        for row in AREA.y + AREA.height / 3..AREA.y + AREA.height * 2 / 3 {
            for col in AREA.x + AREA.width / 3..AREA.x + AREA.width * 2 / 3 {
                total += 1;
                if buf[(col, row)].symbol() != " " {
                    ink += 1;
                }
            }
        }
        ink as f32 / total as f32
    }

    /// Distance from the centre to the first lit cell along the middle row.
    fn throat_radius(buf: &Buffer) -> usize {
        let row = AREA.y + AREA.height / 2;
        let cx = AREA.x + AREA.width / 2;
        (1..AREA.width / 2)
            .find(|d| buf[(cx + d, row)].symbol() != " " || buf[(cx - d, row)].symbol() != " ")
            .unwrap_or(0) as usize
    }

    #[test]
    fn named_tunnel_scenes_keep_a_readable_ink_floor() {
        for style in [2usize, 6, 7, 8] {
            let quiet = motion(style, &|_| Reactivity::default()).1;
            let loud = motion(style, &music).1;
            assert!(
                quiet >= 0.24,
                "{} drew {quiet:.3} ink in silence",
                NAMES[style]
            );
            assert!(
                loud >= 0.24,
                "{} drew {loud:.3} ink under music",
                NAMES[style]
            );
        }
    }

    #[test]
    fn bass_deepens_every_named_tunnel() {
        let shallow = Reactivity {
            bass: 0.1,
            ..Reactivity::default()
        };
        let deep = Reactivity {
            bass: 0.9,
            ..Reactivity::default()
        };
        for style in [2usize, 6, 7, 8] {
            let calm = glyphs(&render(style, &shallow, 60));
            let lowered = glyphs(&render(style, &deep, 60));
            let changed = calm
                .iter()
                .zip(lowered.iter())
                .filter(|(a, b)| a != b)
                .count() as f32
                / calm.len() as f32;
            assert!(
                changed > 0.08,
                "{} changed {:.3} of its cells with bass",
                NAMES[style],
                changed
            );
        }
    }

    #[test]
    fn a_drop_collapses_the_named_throats() {
        for style in [2usize, 6, 7, 8] {
            let calm = glyphs(&settled(style, false));
            let hit = glyphs(&settled(style, true));
            let changed = calm.iter().zip(hit.iter()).filter(|(a, b)| a != b).count() as f32
                / calm.len() as f32;
            assert!(
                changed > 0.08,
                "{} changed {:.3} of its cells on a drop",
                NAMES[style],
                changed
            );
        }
        // The black hole's shadow shrinks, and the portal and lane pull more
        // material into the throat.
        let calm = settled(7, false);
        let hit = settled(7, true);
        assert!(
            throat_radius(&hit) < throat_radius(&calm),
            "event horizon throat grew from {} to {}",
            throat_radius(&calm),
            throat_radius(&hit)
        );
        for style in [2usize, 8] {
            let calm = throat_ink(&settled(style, false));
            let hit = throat_ink(&settled(style, true));
            assert!(
                hit > calm,
                "{} throat went from {calm:.3} to {hit:.3} ink on a drop",
                NAMES[style]
            );
        }
    }

    #[test]
    fn attack_rises_on_a_drop_and_releases() {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        let hit = Reactivity {
            onset: 1.0,
            kick: 1.0,
            ..Reactivity::default()
        };
        renderer.draw(
            &mut buf,
            AREA,
            0.0,
            7,
            0,
            &[0.4; 32],
            &hit,
            &Theme::default(),
        );
        assert!(
            renderer.attack > 0.9,
            "drop only raised the attack to {:.3}",
            renderer.attack
        );
        for frame in 1..=90 {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                7,
                0,
                &[0.4; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
        }
        assert!(
            renderer.attack < 0.2,
            "attack held at {:.3} after 1.5 s",
            renderer.attack
        );
    }

    #[test]
    fn named_tunnel_scenes_stay_bounded_under_extremes() {
        let wild = Reactivity {
            energy: 1.0,
            bass: f32::INFINITY,
            mid: f32::NAN,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: f32::NAN,
            transients: [f32::NAN; 4],
            ..Reactivity::default()
        };
        for style in [2usize, 6, 7, 8] {
            let drawn = ink(&glyphs(&render(style, &wild, 60)));
            assert!(
                (0.05..0.98).contains(&drawn),
                "{} drew {drawn:.3} ink under extreme audio",
                NAMES[style]
            );
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
