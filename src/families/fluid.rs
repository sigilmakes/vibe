//! Fluid and smoke scenes: ink, dye, plumes, vortices, currents, diffusion and
//! capillary flow. Every field is evaluated analytically in physical terminal
//! coordinates, so a frame depends only on the clock and the live audio.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Ink Bloom",
    "Dye Vortex",
    "Smoke Plume",
    "Vortex Street",
    "Capillary Rise",
    "Marangoni Cells",
    "Turbulence Cascade",
    "Viscous Fingers",
    "Current Lines",
    "Vapour Veil",
];

/// Scenes share a little memory: an audio-driven advection phase, a decaying
/// attack envelope and the capillary charge. None of them advance on the wall
/// clock alone, so a silent stream rests instead of animating by itself.
#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    transport: f32,
    attack: f32,
    impact: f32,
    level: f32,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let now = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (now - last).clamp(0.0, 0.05));
        self.last_time = Some(now);
        let attack = unit(audio.onset).max(
            audio
                .transients
                .iter()
                .copied()
                .map(unit)
                .fold(0.0, f32::max),
        );
        if attack > self.attack + 0.06 {
            self.impact = 1.0;
        }
        self.attack = attack;
        self.impact = (self.impact - dt * 2.2).clamp(0.0, 1.0);
        let energy = unit(audio.energy);
        // Advection is a function of the music: silence leaves the dye almost
        // still, while energy and attacks carry it downstream.
        self.transport = (self.transport
            + dt * (0.03 + energy * 1.7 + unit(audio.mid) * 0.55 + self.impact * 0.9))
            .rem_euclid(4096.0);
        // Capillary charge climbs on attacks and drains once they stop.
        self.level = (self.level
            + dt * (0.04 + energy * 0.30 + unit(audio.onset) * 1.1 + unit(audio.kick) * 0.25)
            - dt * 0.45 * self.level)
            .clamp(0.0, 1.0);
    }
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The clock must not wrap: fields mix many incommensurate frequencies, so
/// any modular reset would snap the whole frame.
fn clock(time: f32) -> f32 {
    if time.is_finite() {
        time
    } else {
        0.0
    }
}

/// Chroma centroid as a hue in [0, 1), pulled toward the brightness
/// centroid when the spectrum is too flat for pitch to be meaningful.
fn hue_of(audio: &Reactivity) -> f32 {
    let total: f32 = audio.chroma.iter().copied().map(unit).sum();
    let neutral = unit(audio.centroid) * 0.5;
    if total < 1e-4 {
        neutral
    } else {
        let weighted: f32 = audio
            .chroma
            .iter()
            .enumerate()
            .map(|(i, c)| i as f32 * unit(*c))
            .sum();
        let pitch = weighted / total / 12.0;
        pitch * audio.tonality() + neutral * (1.0 - audio.tonality())
    }
}

fn direction_glyph(dx: f32, dy: f32) -> char {
    if dx.abs() < dy.abs() * 0.55 {
        '│'
    } else if dy.abs() < dx.abs() * 0.45 {
        '─'
    } else if dx * dy > 0.0 {
        '╲'
    } else {
        '╱'
    }
}

fn shade(v: f32) -> char {
    if v > 0.68 {
        '█'
    } else if v > 0.44 {
        '▓'
    } else if v > 0.22 {
        '▒'
    } else if v > 0.09 {
        '░'
    } else if v > 0.03 {
        '·'
    } else {
        ' '
    }
}

struct Scene<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    visible: Rect,
    xs: &'a [f32],
    us: &'a [f32],
    ys: &'a [f32],
    vs: &'a [f32],
    bg_rows: Vec<Color>,
    t: f32,
    span: f32,
    palette: usize,
    audio: &'a Reactivity,
    bands: &'a [f32; 32],
    theme: &'a Theme,
    hue: f32,
    pan: f32,
    spread: f32,
    energy: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    kick: f32,
    beat: f32,
    onset: f32,
    flatness: f32,
    centroid: f32,
    transport: f32,
    impact: f32,
    level: f32,
}

impl Scene<'_> {
    fn width(&self) -> usize {
        self.visible.width as usize
    }

    fn height(&self) -> usize {
        self.visible.height as usize
    }

    fn x(&self, i: usize) -> f32 {
        self.xs[(self.visible.x - self.area.x) as usize + i]
    }

    fn u(&self, i: usize) -> f32 {
        self.us[(self.visible.x - self.area.x) as usize + i]
    }

    fn y(&self, j: usize) -> f32 {
        self.ys[(self.visible.y - self.area.y) as usize + j]
    }

    fn v(&self, j: usize) -> f32 {
        self.vs[(self.visible.y - self.area.y) as usize + j]
    }

    fn put(&mut self, i: usize, j: usize, mark: char, phase: f32, light: f32) {
        if i >= self.width() || j >= self.height() {
            return;
        }
        let col = self.visible.x + i as u16;
        let row = self.visible.y + j as u16;
        let bg = self.bg_rows[j];
        let fg = self.theme.sample(self.palette, phase, light);
        let cell = &mut self.buf[(col, row)];
        cell.reset();
        cell.set_char(mark);
        cell.set_fg(fg);
        cell.set_bg(bg);
    }
}

impl Renderer {
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
        self.advance(time, audio);
        let mode = style % NAMES.len();
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        let t = clock(time);
        let width = area.width.max(1) as f32;
        let height = area.height.max(1) as f32;
        let xs: Vec<f32> = (0..area.width)
            .map(|i| ((i as f32 + 0.5) - width * 0.5) / height)
            .collect();
        let us: Vec<f32> = (0..area.width).map(|i| (i as f32 + 0.5) / width).collect();
        let ys: Vec<f32> = (0..area.height)
            .map(|j| ((j as f32 + 0.5) - height * 0.5) / height)
            .collect();
        let vs: Vec<f32> = (0..area.height)
            .map(|j| (j as f32 + 0.5) / height)
            .collect();
        let energy = unit(audio.energy);
        let bg_rows: Vec<Color> = (0..visible.height)
            .map(|j| {
                let v = (j as f32 + 0.5) / visible.height.max(1) as f32;
                theme.sample(
                    palette,
                    0.60 + v * 0.16,
                    0.03 + energy * 0.04 + 0.015 * (v * PI).sin(),
                )
            })
            .collect();
        let mut scene = Scene {
            buf,
            area,
            visible,
            xs: &xs,
            us: &us,
            ys: &ys,
            vs: &vs,
            bg_rows,
            t,
            span: width / height,
            palette,
            audio,
            bands,
            theme,
            hue: hue_of(audio),
            pan: audio.pan(),
            spread: audio.spread(),
            energy,
            bass: unit(audio.bass),
            mid: unit(audio.mid),
            treble: unit(audio.treble),
            kick: unit(audio.kick),
            beat: unit(audio.beat),
            onset: unit(audio.onset),
            flatness: unit(audio.flatness),
            centroid: unit(audio.centroid),
            transport: self.transport,
            impact: self.impact,
            level: self.level,
        };
        match mode {
            0 => ink_bloom(&mut scene),
            1 => dye_vortex(&mut scene),
            2 => smoke_plume(&mut scene),
            3 => vortex_street(&mut scene),
            4 => capillary_rise(&mut scene),
            5 => marangoni_cells(&mut scene),
            6 => turbulence_cascade(&mut scene),
            7 => viscous_fingers(&mut scene),
            8 => current_lines(&mut scene),
            _ => vapour_veil(&mut scene),
        }
    }
}

/// Expanding ink drops: each drop fades as its ring grows, and bass widens the
/// bloom while treble sharpens the filament edge.
fn ink_bloom(s: &mut Scene<'_>) {
    let span = s.span;
    for j in 0..s.height() {
        let y = s.y(j);
        for i in 0..s.width() {
            let x = s.x(i);
            let mut ink = 0.0;
            for k in 0..5 {
                let a = k as f32 * 2.399_963 + 0.6;
                let bx = a.cos() * 0.26 * span;
                let by = a.sin() * 0.26;
                let life = (s.t * (0.10 + 0.018 * k as f32) + 0.19 * k as f32).rem_euclid(1.0);
                let radius = 0.015 + life * (0.18 + 0.10 * s.bass) + 0.03 * unit(s.bands[k * 6]);
                let dx = x - bx;
                let dy = y - by;
                let d = dx.hypot(dy);
                let theta = dy.atan2(dx);
                let wobble = 1.0 + 0.16 * (theta * 7.0 + life * 5.0 + k as f32).sin();
                let ring = (-((d - radius * wobble) / (0.016 + 0.018 * s.treble)).powi(2)).exp();
                let body = (-(d / (radius * 0.9 + 0.015)).powi(2) * 2.2).exp() * 0.35;
                let fade = (life * 4.0).min(1.0) * (1.0 - life).powf(1.1);
                ink += (ring * 1.15 + body) * fade;
            }
            let b = (ink * (0.65 + 0.7 * s.energy)).min(1.0);
            let phase = 0.52 + 0.16 * (x * 2.7 + y * 3.3 + s.t * 0.35).sin() + 0.28 * s.hue;
            s.put(i, j, shade(b), phase, (b * 1.1).min(1.0));
        }
    }
}

/// Two counter-rotating dye vortices with spiral arms. `mid` and `energy`
/// turn the spiral, and a transient throws an extra pair of arms plus a dye
/// ring that travels outward from each core instead of only raising brightness.
fn dye_vortex(s: &mut Scene<'_>) {
    let span = s.span;
    let arms = 2.5 + 1.5 * s.treble + 2.0 * s.impact;
    let spin = s.transport * (1.0 + 0.7 * s.mid);
    for j in 0..s.height() {
        let y = s.y(j);
        for i in 0..s.width() {
            let x = s.x(i);
            let mut field = 0.0;
            let mut tx = 0.0;
            let mut ty = 0.0;
            for (side, cx_scale, cy) in [(-1.0f32, -0.16, -0.04f32), (1.0, 0.16, 0.05)] {
                let cx = cx_scale * span * (1.0 + s.spread * 0.5) + s.pan * span * 0.10;
                let dx = x - cx;
                let dy = y - cy;
                let d = dx.hypot(dy).max(0.015);
                let theta = dy.atan2(dx);
                let swirl = 1.8 * (-d * d * 8.0).exp();
                let band =
                    (arms * theta + d * (11.0 + 4.0 * s.mid) + side * (swirl * 2.4 - spin)).sin();
                let envelope = (-d * d * 2.0).exp();
                let ripple = 0.25 * s.treble * (d * 37.0 - spin * 1.2).sin();
                // The attack ring starts at the core and travels outward while
                // the envelope decays.
                let ring_radius = 0.06 + 0.36 * (1.0 - s.impact);
                let ring =
                    s.impact * (-((d - ring_radius) / (0.018 + 0.010 * s.treble)).powi(2)).exp();
                field += band * envelope * (1.0 + ripple) + ring;
                tx += -dy / d * side * envelope;
                ty += dx / d * side * envelope;
            }
            let b = ((field.abs() * 1.25 - 0.06).max(0.0)
                * (0.22 + 0.85 * s.mid + 0.55 * s.energy))
                .min(1.0);
            let mark = if b > 0.05 + 0.12 * (1.0 - s.energy) {
                direction_glyph(tx, ty)
            } else {
                ' '
            };
            let phase = 0.5
                + 0.18 * field.signum()
                + 0.22 * s.hue
                + 0.08 * (y * 6.0).sin()
                + 0.20 * s.impact;
            s.put(i, j, mark, phase, b * 0.95);
        }
    }
}

/// A meandering plume. The column widens with height and bass, while treble
/// roughens its edges and the beat adds a warm source glow.
fn smoke_plume(s: &mut Scene<'_>) {
    for j in 0..s.height() {
        let v = s.v(j);
        let h = 1.0 - v;
        for i in 0..s.width() {
            let x = s.x(i);
            let u = s.u(i);
            let drift = 0.12 * (h * 2.6 + s.t * 0.35).sin()
                + 0.07 * (h * 6.0 - s.t * 0.8).sin() * (0.4 + s.treble);
            let cx = drift + 0.05 * (h * 9.0 + s.t * 0.2).sin();
            let width = 0.02 + h * (0.16 + 0.10 * s.bass) * (0.8 + 0.4 * s.energy);
            let core = (-((x - cx) / width).powi(2) * 1.7).exp();
            let turb = (0.62
                + 0.38
                    * (h * 24.0 - s.t * 2.0 + (x - cx) * 22.0 + u * 4.0).sin()
                    * (0.5 + 0.5 * s.treble)
                + 0.12 * (h * 51.0 - s.t * 3.3).cos())
            .max(0.0);
            let mut density = core * turb * (1.0 - 0.55 * h) * (0.5 + 0.8 * s.energy);
            density += 0.5 * ((0.05 - h).max(0.0) * 20.0).min(1.0) * (0.4 + s.beat);
            let b = (density * 1.15).min(1.0);
            let phase = 0.52 + 0.20 * h + 0.25 * s.hue + 0.05 * (s.t * 0.3).sin();
            s.put(i, j, shade(b), phase, b);
        }
    }
}

/// An alternating vortex street shed from a leading edge. The wake flows with
/// the music; an attack releases a strong counter-rotating pair at the leading
/// edge that travels downstream instead of a free-running animation.
fn vortex_street(s: &mut Scene<'_>) {
    let span = s.span;
    let travel = s.transport * 0.11;
    let mut shed = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 12];
    for (k, slot) in shed.iter_mut().enumerate() {
        let life = (travel + k as f32 * 0.083).rem_euclid(1.0);
        let u = 0.22 + life * 1.05;
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let v = 0.5
            + side * (0.15 + 0.10 * s.impact) * (life * 6.0).min(1.0) * (1.0 - 0.5 * life)
            + 0.02 * (life * 30.0).sin();
        let strength = (1.0 - life).powf(0.7) * (0.10 + 0.85 * s.energy + 0.30 * s.bass);
        let radius = 0.035 + life * 0.035 + 0.02 * s.kick;
        *slot = (u, v, side * strength, radius);
    }
    let obst_u = 0.16;
    let obst_v = 0.5;
    let obst_r = 0.045 + 0.012 * s.bass + 0.02 * s.impact;
    // The released pair starts at the leading edge and drifts downstream as
    // the attack envelope decays.
    let pair_u = 0.22 + 0.48 * (1.0 - s.impact);
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let mut spin = 0.0;
            for &(su, sv, strength, radius) in &shed {
                let dx = (u - su) * span;
                let dy = v - sv;
                let d2 = dx * dx + dy * dy;
                spin += strength * (-d2 / (radius * radius)).exp();
            }
            let shear = (-((v - 0.5) / 0.22).powi(2)).exp()
                * 0.18
                * (u - obst_u).max(0.0)
                * (u * 8.0 - s.transport * 0.8).cos();
            spin += shear;
            for (side, sv) in [(1.0f32, 0.38f32), (-1.0, 0.62)] {
                let dx = (u - pair_u) * span;
                let dy = v - sv;
                spin += side * s.impact * 1.5 * (-(dx * dx + dy * dy) / 0.0042).exp();
            }
            let du = (u - obst_u) * span;
            let dv = v - obst_v;
            let dr = (du * du + dv * dv).sqrt();
            let b = (spin.abs() * (1.25 + 0.5 * s.kick)).min(1.0);
            let mark = if dr < obst_r * 1.15 && dr > obst_r * 0.55 {
                '◍'
            } else if dr < obst_r * 0.55 {
                ' '
            } else if b > 0.03 {
                shade(b)
            } else {
                ' '
            };
            let phase = 0.45 + 0.28 * spin.signum() + 0.22 * s.hue;
            s.put(i, j, mark, phase, b);
        }
    }
}

const CAPILLARIES: usize = 6;

/// Centre and half-width of a capillary in canvas coordinates. Widths vary, so
/// the surface never settles into a row of equal bars.
fn capillary(k: usize) -> (f32, f32) {
    let f = k as f32;
    let centre = 0.5 + (f - 2.5) * 0.155;
    let half = 0.052 - 0.008 * (f * 1.7).sin().abs();
    (centre, half)
}

/// Liquid height at `u`: one continuous meniscus curve with a peak standing in
/// each tube. `charge` lifts the columns and `kick` ripples the crest.
fn meniscus(u: f32, level: f32, charge: f32, kick: f32, ripple: f32) -> f32 {
    let mut h = level + 0.012 * (u * 13.0 + ripple).sin() * kick;
    for k in 0..CAPILLARIES {
        let (centre, half) = capillary(k);
        let q = (u - centre) / half;
        // Narrow tubes draw liquid higher, as real capillaries do.
        let climb = (0.08 + 0.42 * charge) * (0.055 / half);
        h += climb / (1.0 + q * q * 2.6);
    }
    h
}

/// Liquid climbing narrow capillaries. The charge built by attacks raises a
/// shared reservoir and its menisci; `bass` deepens the climb and treble
/// shimmers the surface. The liquid is one body, not a chart of columns.
fn capillary_rise(s: &mut Scene<'_>) {
    let charge = (s.level * 0.62 + s.bass * 0.30 + s.energy * 0.08).min(1.0);
    let level = 0.05 + 0.30 * s.level + 0.08 * s.bass;
    for j in 0..s.height() {
        let v = s.v(j);
        let d = 1.0 - v;
        for i in 0..s.width() {
            let u = s.u(i);
            let surface = meniscus(u, level, charge, s.kick, s.transport).min(0.95);
            let crest = (-((d - surface) / 0.016).powi(2)).exp();
            let mut wall: f32 = 1.0;
            for k in 0..CAPILLARIES {
                let (centre, half) = capillary(k);
                wall = wall.min(((u - centre).abs() - half).abs());
            }
            let (mark, phase, light) = if d < surface {
                let depth = surface - d;
                let near = (-depth * 16.0).exp();
                let streak = 0.5 + 0.5 * (d * 54.0 - s.transport * 2.2 + u * 18.0).sin();
                let light = (0.14 + near * 0.66 + streak * 0.07 + crest * 0.35).min(1.0);
                let mark = if crest > 0.5 {
                    '▄'
                } else if light > 0.58 {
                    '▓'
                } else if light > 0.34 {
                    '▒'
                } else if light > 0.15 {
                    '░'
                } else {
                    '·'
                };
                (
                    mark,
                    0.50 + 0.04 * (u * 9.0) + near * 0.18 + crest * 0.22 + 0.22 * s.hue,
                    light,
                )
            } else if wall < 0.006 {
                ('│', 0.62 + 0.10 * s.treble, 0.12 + 0.10 * s.treble)
            } else {
                let spark = (d * 26.0 + u * 31.0 - s.transport * 2.6).sin();
                if spark > 0.972 {
                    ('·', 0.70, 0.16 + 0.18 * s.treble)
                } else {
                    (' ', 0.5, 0.0)
                }
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// Surface-tension convection cells. A hexagonal lattice of upwelling cells is
/// drawn by its ridges, with contour-following flow glyphs inside each cell.
fn marangoni_cells(s: &mut Scene<'_>) {
    let ka = 10.0 + 3.5 * s.bass;
    let drift = s.t * 0.22;
    for j in 0..s.height() {
        let y = s.y(j);
        for i in 0..s.width() {
            let x = s.x(i);
            let a1 = x * ka + drift;
            let a2 = (0.5 * x + 0.866_025_4 * y) * ka - drift * 0.7 + 2.094_395;
            let a3 = (0.5 * x - 0.866_025_4 * y) * ka - drift * 0.4 + 4.188_79;
            let lattice = a1.cos() + a2.cos() + a3.cos();
            let edge = (-((lattice * 1.8).powi(2))).exp();
            let gx = -(ka * a1.sin() + 0.5 * ka * a2.sin() + 0.5 * ka * a3.sin());
            let gy = -(0.866_025_4 * ka * (a2.sin() - a3.sin()));
            let rise = (lattice.abs() / 3.0).min(1.0);
            let b = edge * (0.55 + 0.5 * s.kick) + rise * 0.10 * (0.6 + s.energy);
            let mark = if edge > 0.55 {
                '█'
            } else if edge > 0.25 {
                '▓'
            } else if edge > 0.08 {
                '▒'
            } else if rise > 0.70 {
                direction_glyph(gy, -gx)
            } else if rise > 0.35 {
                '·'
            } else {
                ' '
            };
            let phase = 0.72 + 0.08 * (lattice * 0.3).sin() + 0.18 * s.hue;
            s.put(i, j, mark, phase, b.min(1.0));
        }
    }
}

/// Five octaves of a curl-like velocity field. `flatness` weights the finest
/// octave, so noisy spectra break the large swirls into filaments.
fn turbulence_cascade(s: &mut Scene<'_>) {
    let base = 2.6 + s.centroid * 2.4;
    let speed = 0.4 + 0.9 * s.energy;
    let fine = 0.45 + 0.9 * s.flatness;
    let mut norm = 0.0;
    for n in 0..5 {
        norm += if n == 4 {
            0.55 * fine
        } else {
            1.0 / (1.0 + n as f32 * 0.9)
        };
    }
    for j in 0..s.height() {
        let y = s.y(j);
        for i in 0..s.width() {
            let x = s.x(i);
            let mut vx = 0.0;
            let mut vy = 0.0;
            let mut curl = 0.0;
            let mut amp = 1.0;
            let mut freq = base;
            let mut rot: f32 = 0.7;
            for n in 0..5 {
                let (sn, cs) = rot.sin_cos();
                let cx = x * cs - y * sn;
                let cy = x * sn + y * cs;
                let w = freq * cy + s.t * speed * (1.0 + n as f32 * 0.4) + n as f32 * 1.7;
                let q = freq * cx - s.t * speed * 0.6 + n as f32 * 2.3;
                let a = if n == 4 { amp * 0.55 * fine } else { amp };
                vx += a * q.sin() * w.cos();
                vy += a * q.cos() * w.sin();
                curl += a * (q - w).sin();
                amp *= 0.62;
                freq *= 1.85;
                rot += 0.83;
            }
            let speed_here = vx.hypot(vy) / norm.max(0.2);
            let b = (speed_here.powi(2) * 1.1).min(1.0);
            let mark = if b > 0.02 {
                direction_glyph(vx, vy)
            } else {
                ' '
            };
            let phase =
                0.48 + 0.24 * curl.signum() + 0.22 * s.hue + 0.05 * (x * 3.0 + y * 4.0).sin();
            s.put(i, j, mark, phase, b);
        }
    }
}

/// A radial displacement front with viscous fingers. Bass deepens the fingers,
/// treble wrinkles the rim, and the interior keeps a slow spiral.
fn viscous_fingers(s: &mut Scene<'_>) {
    let amp = 0.035 + 0.05 * s.bass + 0.02 * s.onset;
    let base = 0.20 + 0.05 * s.energy + 0.012 * (s.t * 0.6).sin();
    for j in 0..s.height() {
        let y = s.y(j);
        for i in 0..s.width() {
            let x = s.x(i);
            let r = x.hypot(y).max(0.0001);
            let theta = y.atan2(x);
            let lobes = 6.0 * theta + s.t * 0.8;
            let front = base
                + amp * (lobes.sin() + 0.55 * (2.0 * lobes - 0.6).sin())
                + 0.01 * (theta * 21.0 + s.t * 1.7).sin();
            let rim = (-((r - front) / 0.018).powi(2)).exp();
            let inside = if r < front {
                let depth = front - r;
                0.20 + 0.18 * (r * 26.0 - s.t * 1.4 + theta * 3.0).sin()
                    + 0.9 * (-depth * 26.0).exp()
            } else {
                let depth = r - front;
                0.06 + 0.12 * (-depth * 9.0).exp() * (0.5 + 0.5 * (theta * 4.0 + s.t * 0.7).sin())
            };
            let b = (inside.max(0.0) + rim * 0.9).min(1.0);
            let mark = if rim > 0.45 {
                '█'
            } else if b > 0.55 {
                '▓'
            } else if b > 0.32 {
                '▒'
            } else if b > 0.16 {
                '░'
            } else if b > 0.07 {
                '·'
            } else {
                ' '
            };
            let phase = if r < front {
                0.50 + 0.22 * s.hue
            } else {
                0.80 + 0.10 * s.hue
            };
            s.put(i, j, mark, phase, b);
        }
    }
}

/// Line-integral convection streamlines through a double gyre. `mid` and
/// `energy` set the flow speed, so a silent stream rests; an attack bends the
/// streamlines and drags a visible plume along them.
fn current_lines(s: &mut Scene<'_>) {
    let flow = 0.25 + 1.15 * s.mid + 0.85 * s.energy;
    let band = 28.0 + 10.0 * s.treble;
    let gust_u = (0.10 + s.transport * 0.22).rem_euclid(1.24) - 0.12;
    let gust_v = 0.5 + 0.28 * (s.transport * 0.8).sin();
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let wv = s.audio.waveform[(u * 31.0) as usize] * 0.6;
            let du = u - gust_u;
            let dv = v - gust_v;
            let plume = s.impact * (-(du * du + dv * dv) / 0.010).exp();
            let mut pu = (u + 0.035 * plume * (dv * 22.0).cos()).rem_euclid(1.0);
            let mut pv = (v + 0.035 * plume * (du * 22.0).sin()).rem_euclid(1.0);
            let mut acc = 0.0;
            let mut fx = 0.0;
            let mut fy = 0.0;
            for _ in 0..5 {
                let ux = PI * (PI * pu).sin() * (PI * pv).cos() * flow;
                let uy = 0.35 * (TAU * pu + s.transport * 0.4).sin()
                    - PI * (PI * pu).cos() * (PI * pv).sin() * flow;
                pu = (pu + ux * 0.004).rem_euclid(1.0);
                pv = (pv + uy * 0.004).rem_euclid(1.0);
                // The gust turns the local tangent, so the glyphs change
                // direction along the plume instead of only getting brighter.
                fx = ux + plume * 1.6 * (dv * 26.0).cos();
                fy = uy + plume * 1.6 * (du * 26.0).sin();
                acc += (band * (pu * 0.7 + pv * 0.4) - s.transport * 2.6 + wv).sin();
            }
            let b = ((acc / 5.0).abs().powf(1.4) * (0.24 + 0.90 * s.energy) + plume * 0.9).min(1.0);
            let mark = if b > 0.05 + 0.10 * (1.0 - s.energy) {
                direction_glyph(fx, fy)
            } else {
                ' '
            };
            let phase = 0.52 + 0.22 * ((pu - pv) * 2.0).sin() + 0.2 * s.hue + 0.24 * plume;
            s.put(i, j, mark, phase, (b * 1.2).min(1.0));
        }
    }
}

/// Translucent vapour sheets drifting sideways, displaced by the live waveform,
/// with sparse ash falling through the veil.
fn vapour_veil(s: &mut Scene<'_>) {
    let width = s.width();
    let height = s.height();
    for j in 0..height {
        let v = s.v(j);
        for i in 0..width {
            let u = s.u(i);
            let disp = s.audio.waveform[(u * 31.0) as usize] * 0.07 * (0.4 + s.treble);
            let mut fog = 0.0;
            for l in 0..4 {
                let scale = 1.0 + l as f32 * 0.55;
                let xx = u * scale + s.t * (0.008 + l as f32 * 0.005) + l as f32 * 0.17;
                let yy = (v + disp) * scale - s.t * (0.010 + l as f32 * 0.003) + l as f32 * 0.31;
                let n = (xx * 5.3 + yy * 3.1).sin()
                    + 0.6 * ((xx * 8.7 - yy * 6.2) + s.t * 0.21).sin()
                    + 0.35 * ((yy * 13.0 + xx * 2.2) - s.t * 0.33).cos();
                fog += 0.20 * (1.0 - (n.abs() * 0.85).min(1.0));
            }
            fog *= 0.55 + 0.8 * s.energy + 0.1 * s.bass;
            let mark = if fog > 0.78 {
                '▓'
            } else if fog > 0.58 {
                '▒'
            } else if fog > 0.38 {
                '░'
            } else if fog > 0.22 {
                '·'
            } else {
                ' '
            };
            let phase = 0.66 + 0.10 * (v * 3.0 + s.t * 0.2).sin() + 0.24 * s.hue;
            let light = (fog * 0.65 * (0.5 + 0.6 * s.treble)).min(0.8);
            s.put(i, j, mark, phase, light);
        }
    }
    let count = 40 + (s.energy * 40.0) as usize;
    for k in 0..count {
        let f = k as f32;
        let px = (f * 0.618_034 + 0.10 * (s.t * 0.35 + f * 1.7).sin()).rem_euclid(1.0);
        let py = (f * 0.377 + s.t * (0.010 + (k % 9) as f32 * 0.002)).rem_euclid(1.0);
        let ci = ((px * width as f32) as usize).min(width.saturating_sub(1));
        let cj = ((py * height as f32) as usize).min(height.saturating_sub(1));
        let mark = if k % 4 == 0 { '+' } else { '·' };
        s.put(ci, cj, mark, 0.9, 0.55 + 0.25 * (f * 0.7).sin().abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANDS: [f32; 32] = [0.5; 32];

    fn render(style: usize, audio: &Reactivity) -> String {
        let area = Rect::new(1, 2, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..90 {
            renderer.draw(
                &mut buf,
                area,
                frame as f32 / 30.0,
                style,
                0,
                &BANDS,
                audio,
                &Theme::default(),
            );
        }
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    fn loud() -> Reactivity {
        let mut audio = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.5,
            novelty: 1.0,
            centroid: 0.8,
            flatness: 0.9,
            ..Default::default()
        };
        audio.waveform = [0.7; 32];
        audio.transients = [1.0; 4];
        audio.chroma = [0.9; 12];
        audio
    }

    #[test]
    fn names_are_ten_and_unique() {
        assert_eq!(NAMES.len(), 10);
        let unique: std::collections::HashSet<_> = NAMES.iter().collect();
        assert_eq!(unique.len(), NAMES.len());
    }

    #[test]
    fn stereo_pan_and_width_move_the_vortex() {
        let calm = Reactivity::default();
        let base = render(1, &calm);
        let panned = Reactivity {
            balance: 0.8,
            ..Reactivity::default()
        };
        assert_ne!(base, render(1, &panned), "balance did not pan the vortex");
        let wide = Reactivity {
            width: 0.9,
            ..Reactivity::default()
        };
        assert_ne!(base, render(1, &wide), "width did not spread the vortex");
    }

    #[test]
    fn every_style_draws_a_non_empty_frame() {
        for style in 0..NAMES.len() {
            let image = render(style, &Reactivity::default());
            assert!(
                image.chars().filter(|c| *c != ' ').count() > 200,
                "style {style} barely painted"
            );
        }
    }

    #[test]
    fn styles_are_visibly_distinct() {
        let mut images = std::collections::HashSet::new();
        for style in 0..NAMES.len() {
            let image = render(style, &Reactivity::default());
            assert!(images.insert(image), "duplicate style {style}");
        }
    }

    #[test]
    fn each_style_responds_to_audio() {
        let quiet = Reactivity::default();
        let loud = loud();
        for style in 0..NAMES.len() {
            assert_ne!(
                render(style, &quiet),
                render(style, &loud),
                "style {style} ignores audio"
            );
        }
    }

    #[test]
    fn clock_does_not_wrap() {
        let t = 64.0 * TAU + 0.25;
        assert_eq!(clock(t), t);
        assert_eq!(clock(f32::NAN), 0.0);
    }

    #[test]
    fn styles_wrap_and_clip_to_the_buffer() {
        let area = Rect::new(3, 2, 32, 19);
        for style in 0..NAMES.len() + 2 {
            let mut buf = Buffer::empty(area);
            for cell in &mut buf.content {
                cell.set_char('X');
            }
            let target = Rect::new(7, 5, 19, 11);
            Renderer::default().draw(
                &mut buf,
                target,
                2.5,
                style,
                0,
                &BANDS,
                &loud(),
                &Theme::default(),
            );
            let clip = target.intersection(area);
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    if !clip.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), "X", "style {style} wrote outside");
                    }
                }
            }
        }
    }

    #[test]
    fn zero_and_degenerate_areas_are_safe() {
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(0, 0, 1, 1),
                Rect::new(9, 9, 2, 3),
            ] {
                let mut buf = Buffer::empty(Rect::new(0, 0, 5, 4));
                Renderer::default().draw(
                    &mut buf,
                    area,
                    f32::NAN,
                    style,
                    0,
                    &[f32::NAN; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
            }
        }
    }

    #[test]
    fn frames_are_deterministic() {
        let audio = loud();
        for style in 0..NAMES.len() {
            assert_eq!(render(style, &audio), render(style, &audio));
        }
    }

    const AREA: Rect = Rect::new(0, 0, 100, 40);

    fn draw_frame(
        renderer: &mut Renderer,
        buf: &mut Buffer,
        style: usize,
        audio: &Reactivity,
        frame: usize,
    ) {
        renderer.draw(
            buf,
            AREA,
            frame as f32 / 30.0,
            style,
            0,
            &BANDS,
            audio,
            &Theme::default(),
        );
    }

    fn render_buffer(style: usize, audio: &Reactivity, frames: usize) -> (Renderer, Buffer) {
        let mut buf = Buffer::empty(AREA);
        let mut renderer = Renderer::default();
        for frame in 0..frames {
            draw_frame(&mut renderer, &mut buf, style, audio, frame);
        }
        (renderer, buf)
    }

    fn luminance(color: Color) -> i32 {
        match color {
            Color::Rgb(r, g, b) => (r as i32 + g as i32 + b as i32) / 3,
            _ => 0,
        }
    }

    /// Perceptible cell change: a glyph swap or a luminance step that a viewer
    /// can see, so eight-bit colour rounding does not read as motion.
    fn signature(buf: &Buffer) -> Vec<(char, i32)> {
        buf.content
            .iter()
            .map(|c| {
                let glyph = c.symbol().chars().next().unwrap_or(' ');
                (glyph, (luminance(c.fg) + luminance(c.bg)) / 2)
            })
            .collect()
    }

    fn mask(buf: &Buffer) -> Vec<char> {
        buf.content
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    /// Mean fraction of cells that change per frame once the envelopes settle.
    fn movement(style: usize, audio: &Reactivity) -> f32 {
        let mut buf = Buffer::empty(AREA);
        let mut renderer = Renderer::default();
        for frame in 0..30 {
            draw_frame(&mut renderer, &mut buf, style, audio, frame);
        }
        let mut last = signature(&buf);
        let mut total = 0.0;
        for frame in 30..60 {
            draw_frame(&mut renderer, &mut buf, style, audio, frame);
            let now = signature(&buf);
            total += now
                .iter()
                .zip(&last)
                .filter(|(a, b)| a.0 != b.0 || (a.1 - b.1).abs() > 8)
                .count() as f32
                / now.len() as f32;
            last = now;
        }
        total / 30.0
    }

    #[test]
    fn named_scenes_rest_in_silence_and_follow_energy() {
        let quiet = Reactivity::default();
        let active = Reactivity {
            energy: 0.9,
            bass: 0.6,
            mid: 0.8,
            ..Reactivity::default()
        };
        for style in [1, 3, 8] {
            let rest = movement(style, &quiet);
            let moving = movement(style, &active);
            assert!(
                rest < 0.01,
                "{} still churns {:.1}% of its cells in silence",
                NAMES[style],
                rest * 100.0
            );
            assert!(
                moving > 0.05,
                "{} only moves {:.1}% of its cells with energy",
                NAMES[style],
                moving * 100.0
            );
        }
    }

    #[test]
    fn transients_bend_named_scenes_structurally() {
        let steady = Reactivity {
            energy: 0.5,
            bass: 0.4,
            mid: 0.4,
            ..Reactivity::default()
        };
        let hit = Reactivity {
            onset: 1.0,
            kick: 1.0,
            transients: [1.0; 4],
            ..steady
        };
        for style in [1, 3, 8] {
            let (mut plain, mut plain_buf) = render_buffer(style, &steady, 24);
            let (mut struck, mut struck_buf) = render_buffer(style, &steady, 24);
            draw_frame(&mut plain, &mut plain_buf, style, &steady, 24);
            draw_frame(&mut struck, &mut struck_buf, style, &hit, 24);
            let changed = mask(&plain_buf)
                .iter()
                .zip(mask(&struck_buf).iter())
                .filter(|(a, b)| a != b)
                .count();
            assert!(
                changed > 150,
                "{} attack changed only {changed} glyph cells",
                NAMES[style]
            );
        }
    }

    #[test]
    fn capillary_meniscus_is_a_continuous_rising_curve() {
        let sample = |charge: f32| -> Vec<f32> {
            (0..240)
                .map(|i| meniscus(i as f32 / 240.0, 0.10, charge, 0.4, 1.3))
                .collect()
        };
        let low = sample(0.05);
        let high = sample(0.95);
        let step = low
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            step < 0.08,
            "the surface steps {step:.3} between samples like separate bars"
        );
        let peak = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max);
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(
            peak(&high) > peak(&low) + 0.15,
            "charge did not raise the menisci"
        );
        assert!(
            mean(&high) > mean(&low) + 0.06,
            "charge did not raise the body"
        );
        for k in 0..CAPILLARIES {
            let (centre, _) = capillary(k);
            assert!(
                meniscus(centre, 0.10, 0.95, 0.0, 0.0)
                    > meniscus(centre, 0.10, 0.05, 0.0, 0.0) + 0.1,
                "tube {k} did not climb with charge"
            );
        }
    }

    #[test]
    fn capillary_attacks_raise_the_liquid_body() {
        let quiet = Reactivity::default();
        let (quiet_state, quiet_buf) = render_buffer(4, &quiet, 90);
        let mut struck = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        for frame in 0..90 {
            let pulse = if frame % 6 == 0 { 1.0 } else { 0.0 };
            let audio = Reactivity {
                energy: 0.7,
                bass: 0.6,
                kick: pulse,
                onset: pulse,
                transients: [pulse; 4],
                ..Reactivity::default()
            };
            draw_frame(&mut struck, &mut buf, 4, &audio, frame);
        }
        assert!(
            struck.level > quiet_state.level + 0.2,
            "attacks did not charge the capillaries ({:.3} vs {:.3})",
            struck.level,
            quiet_state.level
        );
        let liquid = |buf: &Buffer| {
            buf.content
                .iter()
                .filter(|c| matches!(c.symbol(), "▄" | "▓" | "▒" | "░"))
                .count()
        };
        assert!(
            liquid(&buf) > liquid(&quiet_buf) + 100,
            "charged capillaries did not fill more cells ({} vs {})",
            liquid(&buf),
            liquid(&quiet_buf)
        );
    }

    #[test]
    fn attack_memory_stays_bounded_and_drains() {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        for frame in 0..300 {
            let pulse = if frame % 5 == 0 { 1.0 } else { 0.0 };
            let audio = Reactivity {
                energy: 1.0,
                bass: 0.8,
                mid: 0.7,
                kick: pulse,
                onset: pulse,
                transients: [pulse; 4],
                ..Reactivity::default()
            };
            draw_frame(&mut renderer, &mut buf, 4, &audio, frame);
        }
        assert!((0.0..=1.0).contains(&renderer.impact));
        assert!((0.0..=1.0).contains(&renderer.level));
        assert!(renderer.transport.is_finite() && renderer.transport <= 4096.0);
        let charged = renderer.level;
        for frame in 300..900 {
            draw_frame(&mut renderer, &mut buf, 4, &Reactivity::default(), frame);
        }
        assert_eq!(renderer.impact, 0.0, "impact did not decay");
        assert!(
            renderer.level < charged * 0.5,
            "charge did not drain ({:.3} of {charged:.3})",
            renderer.level
        );
        let hostile = Reactivity {
            energy: f32::NAN,
            bass: f32::INFINITY,
            mid: 1e30,
            onset: f32::NAN,
            kick: f32::NAN,
            transients: [f32::INFINITY; 4],
            ..Reactivity::default()
        };
        for style in [1, 3, 4, 8] {
            draw_frame(&mut renderer, &mut buf, style, &hostile, 900);
        }
        assert!(renderer.impact.is_finite() && (0.0..=1.0).contains(&renderer.impact));
        assert!(renderer.level.is_finite() && (0.0..=1.0).contains(&renderer.level));
        assert!(renderer.transport.is_finite());
    }
}
