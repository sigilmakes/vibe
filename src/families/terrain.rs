//! Terrain and horizon scenes: mountains, coasts, dunes, canyons, glaciers,
//! islands, deltas, cliffs, tundra and volcanoes. Fields are analytic and
//! deterministic in (time, audio); no state or randomness is kept between
//! frames.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::PI;

pub const NAMES: &[&str] = &[
    "Ridge Horizon",
    "Shoreline Drift",
    "Dune Sea",
    "Canyon Strata",
    "Glacier Crevasse",
    "Island Chain",
    "River Delta",
    "Sea Cliff",
    "Tundra Silence",
    "Volcanic Rift",
];

#[derive(Default)]
pub struct Renderer;

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

/// A one-dimensional ridge or shoreline profile built from four octaves.
fn wave(x: f32, seed: f32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.55;
    let mut freq = 1.7;
    for k in 0..4 {
        v += amp * (x * freq + seed * (1.7 + k as f32 * 2.3) + k as f32 * 1.9).sin();
        amp *= 0.55;
        freq *= 2.15;
    }
    (v / 1.05).clamp(-1.0, 1.0)
}

/// A smooth two-dimensional field from rotated sine products.
fn noise2(x: f32, y: f32, seed: f32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.6;
    for k in 0..4 {
        v += amp
            * ((x * freq + seed * 3.1 + k as f32 * 2.7).sin()
                * (y * freq * 0.83 - seed * 1.4 + k as f32 * 1.3).cos());
        amp *= 0.55;
        freq *= 2.1;
    }
    v
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
    energy: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    kick: f32,
    beat: f32,
    onset: f32,
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
        let bg_phase = match mode {
            2 | 3 => 0.12,
            8 => 0.50,
            9 => 0.75,
            _ => 0.60,
        };
        let bg_rows: Vec<Color> = (0..visible.height)
            .map(|j| {
                let v = (j as f32 + 0.5) / visible.height.max(1) as f32;
                theme.sample(
                    palette,
                    bg_phase + v * 0.12,
                    0.025 + energy * 0.04 + 0.015 * (v * PI).sin(),
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
            energy,
            bass: unit(audio.bass),
            mid: unit(audio.mid),
            treble: unit(audio.treble),
            kick: unit(audio.kick),
            beat: unit(audio.beat),
            onset: unit(audio.onset),
        };
        match mode {
            0 => ridge_horizon(&mut scene),
            1 => shoreline_drift(&mut scene),
            2 => dune_sea(&mut scene),
            3 => canyon_strata(&mut scene),
            4 => glacier_crevasse(&mut scene),
            5 => island_chain(&mut scene),
            6 => river_delta(&mut scene),
            7 => sea_cliff(&mut scene),
            8 => tundra_silence(&mut scene),
            _ => volcanic_rift(&mut scene),
        }
    }
}

/// Layered mountain silhouettes with atmospheric haze. Bass lifts the ridges,
/// onsets light their crests and treble thickens the star field.
#[allow(clippy::needless_range_loop)]
fn ridge_horizon(s: &mut Scene<'_>) {
    let width = s.width();
    let height = s.height();
    let mut tops = vec![[0.0f32; 6]; width];
    for i in 0..width {
        let u = s.u(i);
        for k in 0..6 {
            let base = 0.46 + k as f32 * 0.085;
            let amp = 0.05 + k as f32 * 0.028;
            let drift = s.t * (0.01 + 0.004 * k as f32);
            tops[i][k] = base
                - amp * wave(u * (1.3 + k as f32 * 0.35) + drift, 2.0 + k as f32 * 7.3)
                - s.bass * 0.035;
        }
    }
    let sun_glow = 0.05 + 0.02 * s.energy;
    for j in 0..height {
        let v = s.v(j);
        let y = s.y(j);
        for i in 0..width {
            let u = s.u(i);
            let x = s.x(i);
            let mut layer = None;
            for k in 0..6 {
                if v >= tops[i][k] {
                    layer = Some(k);
                }
            }
            let (mark, phase, light) = if let Some(k) = layer {
                let top = tops[i][k];
                let depth = (v - top).max(0.0);
                let slope = if i + 1 < width {
                    tops[i + 1][k] - top
                } else {
                    0.0
                };
                let crest = (-depth * height as f32 * 1.6).exp();
                let face = 0.5 + 0.5 * (slope * height as f32 * 3.0).clamp(-1.0, 1.0);
                let light =
                    (0.10 + 0.18 * face + crest * (0.35 + 0.25 * s.onset) + 0.06 * s.energy)
                        .min(1.0);
                let mark = if crest > 0.55 {
                    '▄'
                } else if k < 2 {
                    '░'
                } else if k < 4 {
                    '▒'
                } else {
                    '▓'
                };
                let texture = 0.04 * (u * 37.0 + v * 19.0 + s.t * 0.3).sin();
                (mark, 0.06 + 0.035 * k as f32 + texture + 0.1 * s.hue, light)
            } else {
                let d = (x - 0.20 * s.span).hypot(y + 0.24);
                let glow = (-(d / sun_glow).powi(2)).exp();
                let star = ((u * 127.1 + v * 311.7).sin() * 43_758.547).fract().abs();
                let threshold = 0.9975 - 0.004 * s.treble;
                let mark = if d < 0.035 {
                    '●'
                } else if star > threshold {
                    '·'
                } else {
                    ' '
                };
                let phase = if d < 0.035 {
                    0.13 + 0.03 * s.t
                } else {
                    0.55 + 0.05 * (u - 0.5)
                };
                let light = (glow * 0.8
                    + if star > threshold {
                        0.30 + 0.4 * s.treble
                    } else {
                        0.0
                    })
                .min(1.0);
                (mark, phase, light)
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// An oblique coastline: sea on the left, land on the right and a bright surf
/// line between. Bass advances the tide and kicks widen the foam.
fn shoreline_drift(s: &mut Scene<'_>) {
    let tide = 0.06 * s.bass;
    for j in 0..s.height() {
        let v = s.v(j);
        let coast =
            0.55 + 0.18 * wave(v * 2.4 + 0.3, 11.0) + 0.05 * (v * 9.0 - s.t * 0.4).sin() - tide;
        for i in 0..s.width() {
            let u = s.u(i);
            let inland = u - coast;
            let foam = 0.012 + 0.03 * s.kick + 0.01 * s.treble;
            let (mark, phase, light) = if inland.abs() < foam {
                ('█', 0.10 + 0.05 * s.hue, 0.85)
            } else if inland < 0.0 {
                let depth = (-inland).min(1.0);
                let swirl = noise2(u * 5.0 + s.t * 0.05, v * 4.0, 2.5);
                let wave_line = (v * 38.0 + u * 14.0 - s.t * 2.0
                    + s.audio.waveform[(u * 31.0) as usize] * 4.0
                    + swirl * 2.0)
                    .sin();
                let b = (wave_line * 0.5 + 0.5).powi(2) * (0.30 + 0.55 * s.energy) + depth * 0.25;
                let mark = if b > 0.55 {
                    '≈'
                } else if b > 0.30 {
                    '~'
                } else if b > 0.16 {
                    '·'
                } else {
                    ' '
                };
                (mark, 0.58 + 0.06 * swirl + 0.08 * s.hue, b.min(1.0))
            } else {
                let elev = inland * 3.2 + 0.55 * noise2(u * 4.0, v * 4.0, 7.0);
                let contour = (elev * 6.5).sin();
                let ridge = (contour.abs() - 0.72).max(0.0) * 2.5;
                let mark = if ridge > 0.4 {
                    '▒'
                } else if elev > 0.9 {
                    '▓'
                } else if elev > 0.35 {
                    '░'
                } else if elev > 0.08 {
                    '·'
                } else {
                    ' '
                };
                let light = (0.10 + 0.25 * ridge + 0.12 * elev.min(1.0) + 0.08 * s.energy).min(1.0);
                (
                    mark,
                    0.13 + 0.05 * noise2(u * 6.0, v * 3.0, 9.0) + 0.08 * s.hue,
                    light,
                )
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// Wind-built dune ridges. Bass migrates them, mid changes their spacing, and
/// treble adds blowing sand sparkle.
fn dune_sea(s: &mut Scene<'_>) {
    let freq = 8.0 + 3.5 * s.mid;
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let warp = noise2(u * 2.2, v * 2.6, 4.0);
            let phase = v * freq
                + warp * 1.8
                + s.t * (0.25 + 0.15 * s.bass)
                + s.audio.waveform[(u * 31.0) as usize] * 0.7;
            let d = (phase * PI).sin();
            let slope = (phase * PI).cos();
            let crest = (d.abs() - 0.90).max(0.0) * 10.0;
            let lit = (slope * 0.5 + 0.5).powi(2);
            let ripple = (u * 64.0 + v * 10.0 + s.t * 2.0).sin();
            let b = (0.10
                + 0.35 * lit
                + crest * 0.5
                + 0.06 * s.energy
                + 0.10 * unit(s.bands[(u * 31.0) as usize])
                + if ripple > 0.96 {
                    0.18 + 0.2 * s.treble
                } else {
                    0.0
                })
            .min(1.0);
            let mark = if crest > 0.45 { '▄' } else { shade(b) };
            let phase_c = 0.10 + 0.05 * d + 0.06 * s.hue + 0.02 * (u * 9.0).sin();
            s.put(i, j, mark, phase_c, b);
        }
    }
}

/// Strata walls above a meandering canyon river. Bass widens the water, kicks
/// light the rim and the walls erode with smooth noise.
fn canyon_strata(s: &mut Scene<'_>) {
    for j in 0..s.height() {
        let v = s.v(j);
        let meander = 0.5
            + 0.10 * (v * 3.2 + s.t * 0.5).sin()
            + 0.05 * (v * 7.1 - s.t * 0.9).sin()
            + 0.02 * s.audio.waveform[(v * 31.0) as usize];
        let river_w = 0.07 + 0.05 * s.bass + 0.02 * (s.t * 0.7).sin().abs();
        for i in 0..s.width() {
            let u = s.u(i);
            let d = (u - meander).abs();
            let (mark, phase, light) = if d < river_w {
                let flow = (v * 52.0 - s.t * 3.5 + s.mid * 2.0).sin();
                let b = (0.30 + 0.30 * (flow * 0.5 + 0.5) + 0.15 * s.energy).min(1.0);
                let mark = if flow > 0.4 {
                    '≈'
                } else if flow > -0.3 {
                    '~'
                } else {
                    '·'
                };
                (mark, 0.60 + 0.05 * flow + 0.06 * s.hue, b)
            } else {
                let wall = d - river_w;
                let strata = v * 26.0 + noise2(u * 4.0, v * 2.0, 9.0) * 2.2 + wall * 4.0;
                let band = strata.sin();
                let rim = (-wall * 90.0).exp() * (0.5 + s.kick);
                let b = (0.08 + 0.28 * (band * 0.5 + 0.5) + rim * 0.5 + 0.05 * s.energy).min(1.0);
                let mark = if rim > 0.4 {
                    '█'
                } else if band > 0.4 {
                    '▒'
                } else if band > -0.2 {
                    '░'
                } else {
                    '·'
                };
                (mark, 0.09 + 0.06 * band + 0.08 * s.hue, b)
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// A fractured glacier tongue. Dark crevasses cut blue ice; treble opens more
/// cracks and the bass slowly shifts the flow lines.
fn glacier_crevasse(s: &mut Scene<'_>) {
    let crack_width = 0.10 + 0.06 * s.treble;
    let shift = s.t * (0.02 + 0.01 * s.bass);
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let warp = noise2(u * 3.0, v * 2.2, 5.0);
            let field_a = (u * 7.0 + warp * 2.4 + shift).sin().abs();
            let field_b = (v * 13.0 + warp * 1.8 - shift * 1.4).sin().abs();
            let field_c = ((u * 19.0 + v * 17.0 + warp * 3.0).sin()).abs();
            let crack = field_a.min(field_b).min(field_c * (0.5 + 0.5 * s.treble));
            let deep = crack < crack_width;
            let serac = (noise2(u * 5.0, v * 5.0, 13.0) * 3.0).sin();
            let b = if deep {
                0.02
            } else {
                (0.28
                    + 0.20 * serac
                    + 0.10 * s.energy
                    + 0.15 * s.treble * (u * 40.0 - s.t * 2.0).sin().abs())
                .min(1.0)
            };
            let mark = if deep {
                if field_a < field_b {
                    '│'
                } else {
                    '─'
                }
            } else if serac > 0.5 {
                '▒'
            } else if serac > -0.2 {
                '░'
            } else {
                '·'
            };
            let phase = if deep {
                0.72 + 0.05 * s.hue
            } else {
                0.58 + 0.05 * serac + 0.08 * s.hue
            };
            s.put(i, j, mark, phase, b);
        }
    }
}

/// A top-down archipelago. Bass raises the sea and drowns the smaller islands;
/// kicks push foam rings around each shore.
fn island_chain(s: &mut Scene<'_>) {
    let span = s.span;
    let sea = 0.28 + 0.10 * s.bass;
    let islands = [
        (0.24f32, 0.34f32, 0.13f32),
        (0.62, 0.28, 0.09),
        (0.72, 0.62, 0.16),
        (0.38, 0.72, 0.11),
        (0.52, 0.48, 0.06),
    ];
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let mut elev = -sea;
            for &(iu, iv, r) in &islands {
                let dx = (u - iu) * span;
                let dy = v - iv;
                let rr = r * (1.0 + 0.10 * s.beat);
                elev += (-(dx * dx + dy * dy) / (rr * rr)).exp();
            }
            let (mark, phase, light) = if elev > 0.0 {
                if elev < 0.05 {
                    ('█', 0.10, 0.75 + 0.2 * s.kick)
                } else if elev > 0.55 {
                    ('▓', 0.20, 0.55)
                } else {
                    let ring = (elev * 32.0).sin();
                    let mark = if ring > 0.55 {
                        '▒'
                    } else if ring > -0.1 {
                        '░'
                    } else {
                        '·'
                    };
                    (
                        mark,
                        0.16 + 0.06 * ring + 0.08 * s.hue,
                        (0.25 + 0.25 * elev + 0.1 * s.energy).min(1.0),
                    )
                }
            } else {
                let depth = -elev;
                let wave = (u * 30.0 + v * 24.0 - s.t * 1.5 + noise2(u * 4.0, v * 4.0, 3.0)).sin();
                if depth < 0.04 + 0.02 * s.kick {
                    ('≈', 0.55, 0.5 + 0.3 * s.kick)
                } else {
                    let b = (0.10 + 0.25 * (wave * 0.5 + 0.5) + 0.10 * s.energy).min(0.8);
                    let mark = if b > 0.35 {
                        '~'
                    } else if b > 0.20 {
                        '·'
                    } else {
                        ' '
                    };
                    (mark, 0.60 + 0.05 * wave + 0.06 * s.hue, b)
                }
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// A branching delta seen from above. Channels fan from the trunk into a
/// sediment plume; mid widens them and the waveform wiggles their banks.
fn river_delta(s: &mut Scene<'_>) {
    for j in 0..s.height() {
        let v = s.v(j);
        let sea = 0.86 + 0.03 * s.bass;
        for i in 0..s.width() {
            let u = s.u(i);
            let mut channel = f32::INFINITY;
            let mut flow = 0.0;
            for k in 0..5 {
                let idx = k as f32 - 2.0;
                let spread = (v - 0.16).max(0.0).powf(1.15) * 0.22;
                let path = 0.5
                    + idx * spread
                    + 0.03 * (v * 8.0 + idx * 1.7 + s.t * 0.4).sin()
                    + 0.02 * s.audio.waveform[(u * 31.0) as usize] * idx;
                let width = (0.010 + 0.030 * (1.0 - v)) * (0.8 + 0.6 * s.mid);
                let d = (u - path).abs() / width;
                if d < channel {
                    channel = d;
                    flow = idx;
                }
            }
            if v < 0.33 {
                let trunk = (u - 0.5).abs() / (0.02 + 0.02 * (1.0 - v));
                if trunk < channel {
                    channel = trunk;
                    flow = 0.0;
                }
            }
            let (mark, phase, light) = if v > sea {
                let plume = (-channel * 1.2).exp() * (1.0 - (v - sea) * 2.0).max(0.0);
                let wave = (u * 28.0 + v * 18.0 - s.t * 1.8).sin();
                let b = (0.15 + 0.25 * (wave * 0.5 + 0.5) + 0.35 * plume + 0.1 * s.energy).min(1.0);
                let mark = if plume > 0.35 {
                    '░'
                } else if b > 0.35 {
                    '~'
                } else {
                    '·'
                };
                (mark, 0.55 + 0.05 * wave + 0.05 * s.hue, b)
            } else if channel < 1.0 {
                let stream = (v * 48.0 - s.t * 3.0 + flow * 1.5 + s.mid * 2.0).sin();
                let b = (0.40 + 0.30 * (stream * 0.5 + 0.5) + 0.15 * s.energy).min(1.0);
                let mark = if stream > 0.35 {
                    '≈'
                } else if stream > -0.4 {
                    '~'
                } else {
                    '─'
                };
                (mark, 0.58 + 0.05 * stream + 0.05 * s.hue, b)
            } else if channel < 3.0 {
                let sed = (-(channel - 1.0) * 1.3).exp();
                let mark = if sed > 0.4 { '░' } else { '·' };
                (
                    mark,
                    0.12 + 0.05 * sed + 0.06 * s.hue,
                    (0.15 + 0.30 * sed + 0.08 * s.energy).min(0.9),
                )
            } else {
                let marsh = noise2(u * 6.0, v * 5.0, 17.0);
                let mark = if marsh > 0.35 {
                    '▒'
                } else if marsh > -0.1 {
                    '░'
                } else {
                    '·'
                };
                (
                    mark,
                    0.11 + 0.04 * marsh + 0.06 * s.hue,
                    (0.12 + 0.15 * marsh.abs() + 0.06 * s.energy).min(0.8),
                )
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// A profile view of a sea cliff: ledges and cracks on the left, sky and water
/// on the right. Bass raises the sea and kicks throw surf against the rock.
fn sea_cliff(s: &mut Scene<'_>) {
    let horizon = 0.60 - 0.03 * s.bass;
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let edge = 0.42 + 0.05 * wave(v * 3.0, 21.0) + 0.03 * (v * 12.0 - s.t * 0.5).sin();
            let (mark, phase, light) = if u < edge {
                let strata = v * 34.0 + noise2(u * 9.0, v * 3.0, 23.0) * 2.4 + u * 6.0;
                let band = strata.sin();
                let ledge = (band - 0.75).max(0.0) * 3.0;
                let crack = (u * 46.0 + noise2(u * 5.0, v * 6.0, 29.0) * 2.0).sin();
                let b =
                    (0.10 + 0.30 * ledge + 0.10 * (band * 0.5 + 0.5) + 0.06 * s.energy).min(1.0);
                let mark = if ledge > 0.5 {
                    '▄'
                } else if crack.abs() < 0.06 {
                    '│'
                } else if band > 0.3 {
                    '▒'
                } else if band > -0.3 {
                    '░'
                } else {
                    '·'
                };
                (mark, 0.08 + 0.06 * band + 0.07 * s.hue, b)
            } else if v < horizon {
                let cloud = noise2(u * 4.0, v * 5.0, 31.0);
                let gull = (u * 40.0 + v * 20.0 + s.t * 1.2).sin();
                let mark = if gull > 0.985 - 0.01 * s.treble {
                    '⌁'
                } else if cloud > 0.35 {
                    '·'
                } else {
                    ' '
                };
                let b =
                    (0.10 + 0.10 * cloud.abs() + if gull > 0.985 { 0.3 * s.treble } else { 0.0 })
                        .min(0.7);
                (mark, 0.62 + 0.05 * cloud + 0.05 * s.hue, b)
            } else {
                let depth = v - horizon;
                let wave_line =
                    (v * 40.0 - s.t * 2.2 + u * 10.0 + noise2(u * 3.0, v * 3.0, 37.0) * 2.0).sin();
                let surf = (-(u - edge).abs() * 30.0).exp() * (-depth * 8.0).exp() * (0.4 + s.kick);
                let b =
                    (0.18 + 0.30 * (wave_line * 0.5 + 0.5) + surf * 0.7 + 0.1 * s.energy).min(1.0);
                let mark = if surf > 0.35 {
                    '█'
                } else if wave_line > 0.4 {
                    '≈'
                } else if wave_line > -0.3 {
                    '~'
                } else {
                    '·'
                };
                (mark, 0.57 + 0.05 * wave_line + 0.06 * s.hue, b)
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// A quiet tundra plain under aurora curtains. Bass breathes the horizon and
/// snow relief, while treble adds ice sparkle to the drifts.
fn tundra_silence(s: &mut Scene<'_>) {
    let horizon = 0.44 - 0.015 * s.bass;
    for j in 0..s.height() {
        let v = s.v(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let (mark, phase, light) = if v < horizon {
                let ribbon = ((u * 5.0 + 0.35 * noise2(u * 3.0, v * 4.0, 41.0) + s.t * 0.12).sin()
                    + 0.55 * ((u * 9.0 - v * 3.0 - s.t * 0.17).sin()))
                .abs();
                let curtain = (-((ribbon - 0.15) * 4.0).powi(2)).exp() * (0.4 + 0.5 * s.energy);
                let spark = (u * 90.0 + v * 70.0 + s.t * 1.7).sin();
                let b = (curtain * 0.7 + if spark > 0.99 { 0.3 * s.treble } else { 0.0 }).min(0.9);
                let mark = if curtain > 0.45 {
                    '░'
                } else if curtain > 0.2 || spark > 0.994 - 0.004 * s.treble {
                    '·'
                } else {
                    ' '
                };
                (mark, 0.50 + 0.08 * ribbon + 0.12 * s.hue, b)
            } else {
                let depth = v - horizon;
                let drift = noise2(u * 3.5, v * 6.0, 43.0);
                let dune = (v * 30.0 + drift * 3.0 - s.t * 0.15).sin();
                let pond = noise2(u * 2.0, v * 2.5, 47.0);
                let spark = (u * 83.0 + v * 61.0 + s.t * 2.0).sin();
                if pond < -0.45 {
                    let ice = (u * 50.0 + v * 40.0 + s.t).sin();
                    let b = (0.18 + 0.1 * (ice * 0.5 + 0.5) + 0.08 * s.treble).min(0.6);
                    (if b > 0.3 { '·' } else { ' ' }, 0.60, b)
                } else {
                    let relief = (0.5 + 0.5 * dune) * (0.6 + 0.5 * depth);
                    let b = (0.06
                        + 0.14 * relief
                        + 0.06 * s.energy
                        + if spark > 0.99 - 0.006 * s.treble {
                            0.15 + 0.15 * s.treble
                        } else {
                            0.0
                        })
                    .min(0.7);
                    let mark = if spark > 0.99 - 0.006 * s.treble {
                        '·'
                    } else if dune > 0.85 {
                        '─'
                    } else if dune > 0.30 {
                        '░'
                    } else if dune > -0.4 {
                        '·'
                    } else {
                        ' '
                    };
                    (mark, 0.55 + 0.05 * drift + 0.08 * s.hue, b)
                }
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

/// A volcanic cone with glowing fissures, a bright crater and an ash plume.
/// Kicks flare the crater, bass widens the lava and treble roughens the ash.
fn volcanic_rift(s: &mut Scene<'_>) {
    let apex = 0.30;
    for j in 0..s.height() {
        let v = s.v(j);
        let y = s.y(j);
        for i in 0..s.width() {
            let u = s.u(i);
            let x = s.x(i);
            let slope = (v - apex).max(0.0) * 0.55;
            let on_cone = v >= apex && (u - 0.5).abs() < slope + 0.02;
            let ground = v > 0.72;
            let (mark, phase, light) = if on_cone {
                let ang = ((u - 0.5) / slope.max(0.02)).clamp(-1.0, 1.0);
                let chan = (ang * 8.0 + s.t * 0.35 + noise2(u * 3.0, v * 3.0, 53.0) * 2.0).sin();
                let lava = ((chan.abs() - 0.72).max(0.0) * 6.0).min(1.0) * (0.6 + 0.6 * s.bass);
                let crater =
                    (-(((x - 0.0).powi(2) + (y + 0.20).powi(2)) / 0.0018)).exp() * (0.5 + s.kick);
                let rock = noise2(u * 6.0, v * 5.0, 59.0);
                let b = (0.10 + 0.15 * rock.abs() + lava * 0.75 + crater * 0.9).min(1.0);
                let mark = if crater > 0.45 || lava > 0.55 {
                    '█'
                } else if lava > 0.25 {
                    '▓'
                } else if b > 0.2 {
                    '▒'
                } else {
                    '░'
                };
                (mark, 0.07 + 0.05 * rock + 0.04 * s.hue, b)
            } else if v < apex {
                let h = (apex - v).max(0.0);
                let centre = 0.5 + 0.10 * (h * 6.0 + s.t * 0.6).sin();
                let width = 0.03 + h * 0.26;
                let dens = (-((u - centre) / width).powi(2)).exp()
                    * (0.4 + 0.8 * s.treble)
                    * (0.7 + 0.4 * s.kick)
                    * (1.0 - h * 0.35);
                let ember = (u * 70.0 + v * 90.0 + s.t * 3.0).sin();
                let b = (dens * 1.2 + if ember > 0.995 { 0.4 * s.onset } else { 0.0 }).min(1.0);
                let mark = if ember > 0.995 - 0.003 * s.treble {
                    '·'
                } else {
                    shade(b * 0.7)
                };
                (mark, 0.78 + 0.05 * s.hue, b * 0.6)
            } else if ground {
                let rock = noise2(u * 5.0, v * 7.0, 61.0);
                let crack = (u * 34.0 + rock * 3.0 + s.t * 0.3).sin();
                let ember = (u * 90.0 + v * 80.0 + s.t * 2.5).sin();
                let b = (0.06
                    + 0.1 * rock.abs()
                    + if crack > 0.96 {
                        0.5 + 0.3 * s.kick
                    } else {
                        0.0
                    }
                    + if ember > 0.99 { 0.3 * s.onset } else { 0.0 })
                .min(1.0);
                let mark = if crack > 0.96 {
                    '─'
                } else if ember > 0.99 {
                    '·'
                } else if b > 0.12 {
                    '░'
                } else {
                    ' '
                };
                (mark, 0.06 + 0.04 * rock + 0.03 * s.hue, b)
            } else {
                let haze = noise2(u * 3.0, v * 2.0, 67.0);
                (
                    if haze > 0.45 { '·' } else { ' ' },
                    0.72 + 0.05 * haze + 0.05 * s.hue,
                    (0.08 + 0.1 * haze.abs() + 0.05 * s.energy).min(0.5),
                )
            };
            s.put(i, j, mark, phase, light);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANDS: [f32; 32] = [0.5; 32];

    fn render(
        style: usize,
        audio: &Reactivity,
    ) -> Vec<(String, ratatui::style::Color, ratatui::style::Color)> {
        render_for(style, audio, 90)
    }

    fn render_for(
        style: usize,
        audio: &Reactivity,
        frames: usize,
    ) -> Vec<(String, ratatui::style::Color, ratatui::style::Color)> {
        let area = Rect::new(1, 2, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer;
        for frame in 0..frames {
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
        buf.content
            .iter()
            .map(|c| (c.symbol().to_owned(), c.fg, c.bg))
            .collect()
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
    fn every_style_draws_a_non_empty_frame() {
        for style in 0..NAMES.len() {
            let image = render(style, &Reactivity::default());
            assert!(
                image.iter().filter(|(c, ..)| c != " ").count() > 200,
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
            assert!(
                render(style, &quiet) != render(style, &loud),
                "style {style} ignores audio"
            );
        }
    }

    #[test]
    fn clock_does_not_wrap() {
        let t = 64.0 * std::f32::consts::TAU + 0.25;
        assert_eq!(clock(t), t);
        assert_eq!(clock(f32::NAN), 0.0);
    }

    #[test]
    fn each_style_keeps_its_audio_wiring() {
        // Terrain shifts height from band energy and hue from chroma and the
        // brightness centroid. Hue-only inputs are invisible to a symbol
        // snapshot, so probe each input against the full cell contents.
        let quiet = Reactivity::default();
        let probes = [
            (
                "bass",
                Reactivity {
                    bass: 0.85,
                    ..Reactivity::default()
                },
            ),
            (
                "energy",
                Reactivity {
                    energy: 0.7,
                    ..Reactivity::default()
                },
            ),
            (
                "centroid",
                Reactivity {
                    centroid: 0.9,
                    ..Reactivity::default()
                },
            ),
            (
                "chroma",
                Reactivity {
                    chroma: std::array::from_fn(|i| if i == 3 { 0.9 } else { 0.0 }),
                    ..Reactivity::default()
                },
            ),
        ];
        for (style, name) in NAMES.iter().enumerate() {
            let base = render_for(style, &quiet, 30);
            for (input, audio) in &probes {
                assert!(
                    base != render_for(style, audio, 30),
                    "{name} ignores {input}"
                );
            }
        }
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
            Renderer.draw(
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
                Renderer.draw(
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
}
