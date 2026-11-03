//! Weather and storms: rain, lightning, cloud masses, fog, snow, cyclones,
//! hail, thunderheads, dust devils, and radar echoes.
//!
//! Geometry is computed in cell space; static particle fields and radar echoes
//! are generated once and reused. All motion is a pure function of time so a
//! frame is deterministic for a given audio state.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Rain Curtain",
    "Lightning Fork",
    "Cumulus Field",
    "Fog Bank",
    "Blizzard Veil",
    "Cyclone Eye",
    "Hail Shower",
    "Thunderhead",
    "Dust Devil",
    "Storm Front",
];

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash01(x: u32) -> f32 {
    let mut n = x.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    n ^= n >> 15;
    n = n.wrapping_mul(0x2545_F491);
    n ^= n >> 13;
    (n & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn hash2(x: i32, y: i32) -> f32 {
    hash01((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663))
}

fn noise2(x: f32, y: f32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let xf = x - xi;
    let yf = y - yi;
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let a = hash2(xi as i32, yi as i32);
    let b = hash2(xi as i32 + 1, yi as i32);
    let c = hash2(xi as i32, yi as i32 + 1);
    let d = hash2(xi as i32 + 1, yi as i32 + 1);
    let top = a + (b - a) * u;
    let bottom = c + (d - c) * u;
    top + (bottom - top) * v
}

fn line_glyph(dx: f32, dy: f32) -> char {
    let slope = dy / (dx.abs() + 0.0001);
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

/// One frame's glyph and brightness field, reused across frames.
#[derive(Default)]
struct Canvas {
    w: usize,
    h: usize,
    level: Vec<f32>,
    mark: Vec<char>,
}

impl Canvas {
    fn reset(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            self.w = w;
            self.h = h;
            self.level = vec![0.0; w * h];
            self.mark = vec![' '; w * h];
        } else {
            self.level.fill(0.0);
            self.mark.fill(' ');
        }
    }

    fn plot(&mut self, x: i32, y: i32, mark: char, level: f32) {
        if !level.is_finite() || level <= 0.0 || x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.w || y >= self.h {
            return;
        }
        let i = y * self.w + x;
        if level > self.level[i] {
            self.level[i] = level;
            self.mark[i] = mark;
        }
    }

    fn dot(&mut self, x: f32, y: f32, mark: char, level: f32) {
        self.plot(x.round() as i32, y.round() as i32, mark, level);
    }

    fn stroke(&mut self, a: [f32; 2], b: [f32; 2], level: f32) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let glyph = line_glyph(dx, dy);
        let steps = (dx.abs().max(dy.abs()) * 2.0).ceil().max(1.0) as i32;
        for step in 0..=steps {
            let f = step as f32 / steps as f32;
            self.plot(
                (a[0] + dx * f).round() as i32,
                (a[1] + dy * f).round() as i32,
                glyph,
                level,
            );
        }
    }
}

fn hue_for(mode: usize, u: f32, v: f32, audio: &Reactivity) -> f32 {
    let base = match mode {
        0 => 0.55 + v * 0.20,
        1 => 0.72 + u * 0.22,
        2 => 0.08 + v * 0.22,
        3 => 0.30 + u * 0.28,
        4 => 0.48 + u * 0.38,
        5 => 0.62 + v * 0.26,
        6 => 0.84 + u * 0.14,
        7 => 0.42 + v * 0.22,
        8 => 0.14 + v * 0.30,
        _ => 0.18 + u * 0.28,
    };
    (base + unit(audio.centroid) * 0.16 + unit(audio.locked_beat_phase()) * 0.05).rem_euclid(1.0)
}

struct Cloud {
    p: [f32; 2],
    r: f32,
    phase: f32,
    drift: f32,
}

fn build_clouds() -> Vec<Cloud> {
    (0..12)
        .map(|i| {
            let k = i as u32 * 5;
            let col = (i % 4) as f32;
            let row = (i / 4) as f32;
            Cloud {
                p: [
                    0.13 + col * 0.25 + (hash01(k + 1) - 0.5) * 0.07,
                    0.22 + row * 0.28 + (hash01(k + 2) - 0.5) * 0.08,
                ],
                r: 0.070 + hash01(k + 3) * 0.035,
                phase: hash01(k + 4) * TAU,
                drift: 0.02 + hash01(k + 5) * 0.03,
            }
        })
        .collect()
}

struct Echo {
    p: [f32; 2],
    r: f32,
    strength: f32,
}

fn build_echoes() -> Vec<Echo> {
    (0..12)
        .map(|i| {
            let k = i as u32 * 7;
            Echo {
                p: [0.08 + hash01(k + 1) * 0.84, 0.08 + hash01(k + 2) * 0.84],
                r: 0.04 + hash01(k + 3) * 0.07,
                strength: 0.5 + hash01(k + 4) * 0.5,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Bolt {
    a: [f32; 2],
    b: [f32; 2],
    depth: u8,
}

fn build_bolt(seed: u32, fw: f32, fh: f32, branch_skip: f32) -> Vec<Bolt> {
    #[allow(clippy::too_many_arguments)]
    fn grow(
        out: &mut Vec<Bolt>,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        len: f32,
        depth: u8,
        seed: u32,
        branch_skip: f32,
    ) {
        let ex = x + dx * len;
        let ey = y + dy * len;
        out.push(Bolt {
            a: [x, y],
            b: [ex, ey],
            depth,
        });
        if depth >= 5 || len < 2.5 {
            return;
        }
        for k in 0..2u32 {
            let r = hash01(
                seed.wrapping_mul(31)
                    .wrapping_add(depth as u32 * 17 + k * 101),
            );
            if r < branch_skip {
                continue;
            }
            let turn = (r - 0.5) * 1.15;
            let ndx = dx * turn.cos() - dy * turn.sin();
            let ndy = dx * turn.sin() + dy * turn.cos();
            grow(
                out,
                ex,
                ey,
                ndx,
                ndy,
                len * (0.54 + r * 0.14),
                depth + 1,
                seed.rotate_left(depth as u32 + k + 1),
                branch_skip,
            );
        }
    }
    let mut out = Vec::with_capacity(48);
    let start_x = fw * (0.18 + hash01(seed) * 0.64);
    let tilt = (hash01(seed ^ 0x55) - 0.5) * 0.32;
    grow(
        &mut out,
        start_x,
        -2.0,
        tilt,
        1.0,
        fh * 0.32,
        0,
        seed,
        branch_skip,
    );
    out
}

#[derive(Default)]
pub struct Renderer {
    canvas: Canvas,
    clouds: Vec<Cloud>,
    echoes: Vec<Echo>,
    last_time: Option<f32>,
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
        let now = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        self.last_time = Some(now);
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        let w = visible.width as usize;
        let h = visible.height as usize;
        self.canvas.reset(w, h);
        match mode {
            0 => self.draw_rain(w, h, now, bands, audio),
            1 => self.draw_lightning(w, h, now, bands, audio),
            2 => self.draw_cumulus(w, h, now, audio),
            3 => self.draw_fog(w, h, now, audio),
            4 => self.draw_snow(w, h, now, audio),
            5 => self.draw_cyclone(w, h, now, audio),
            6 => self.draw_hail(w, h, now, audio),
            7 => self.draw_thunderhead(w, h, now, audio),
            8 => self.draw_dust(w, h, now, audio),
            _ => self.draw_storm(w, h, now, audio),
        }
        self.composite(buf, visible, mode, palette, audio, theme);
    }

    fn composite(
        &self,
        buf: &mut Buffer,
        visible: Rect,
        mode: usize,
        palette: usize,
        audio: &Reactivity,
        theme: &Theme,
    ) {
        let w = self.canvas.w;
        let h = self.canvas.h;
        let energy = unit(audio.energy);
        for j in 0..h {
            for i in 0..w {
                let cell = &mut buf[(visible.x + i as u16, visible.y + j as u16)];
                cell.reset();
                cell.set_bg(theme.sample(palette, 0.0, 0.0));
                let level = self.canvas.level[j * w + i];
                if level <= 0.02 {
                    continue;
                }
                let u = i as f32 / w.max(1) as f32;
                let v = j as f32 / h.max(1) as f32;
                let hue = hue_for(mode, u, v, audio);
                let bright = (level * (0.62 + energy * 0.30)).clamp(0.03, 1.0);
                cell.set_char(self.canvas.mark[j * w + i]);
                cell.set_fg(theme.sample(palette, hue, bright));
            }
        }
    }

    /// Slanted rain streaks, a low mist, and splash rings on the ground.
    fn draw_rain(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let energy = unit(audio.energy);
        let treble = unit(audio.treble);
        let slant = 0.34 + unit(audio.mid) * 0.35;
        let density = 0.25 + treble * 0.55 + energy * 0.20 + unit(bands[5]) * 0.15;
        let wind = t * (2.0 + unit(audio.mid) * 5.0);
        for i in 0..420u32 {
            if hash01(i * 7 + 1) > density {
                continue;
            }
            let speed = 0.35 + hash01(i * 7 + 3) * 0.5 + unit(audio.bass) * 0.4;
            let len = 1.5 + hash01(i * 7 + 4) * 3.0;
            let y = ((hash01(i * 7 + 5) * (fh + 16.0) + t * speed * fh * 0.55)
                .rem_euclid(fh + 16.0))
                - 8.0;
            let x = (hash01(i * 7 + 2) * fw + y * slant + wind).rem_euclid(fw);
            let level = 0.20 + treble * 0.40 + hash01(i * 7 + 6) * 0.12;
            self.canvas
                .stroke([x, y], [x + slant * len, y + len], level);
            self.canvas.dot(x, y, '·', level + 0.15);
        }
        let mist = (h / 4).max(1);
        for j in 0..mist {
            for i in 0..w {
                let n = noise2(i as f32 * 0.10 + t * 0.04, j as f32 * 0.16);
                if n > 0.60 {
                    let level = (n - 0.60) * 0.35 + 0.03 + treble * 0.03;
                    self.canvas.plot(i as i32, j as i32, '░', level);
                }
            }
        }
        for k in 0..14u32 {
            let x = hash01(k * 13 + 3) * fw;
            let pulse = 0.5 + 0.5 * (t * 2.2 + hash01(k * 13 + 4) * TAU).sin();
            let level = (0.15 + unit(audio.kick) * 0.50) * pulse;
            let y = fh - 1.0;
            self.canvas.dot(x, y, '·', level);
            self.canvas.dot(x - 1.0, y - 1.0, '·', level * 0.5);
            self.canvas.dot(x + 1.0, y - 1.0, '·', level * 0.5);
            self.canvas.dot(x, y - 2.0, '·', level * 0.3);
        }
    }

    /// A cloud deck split by a recursive fork of lightning.
    fn draw_lightning(
        &mut self,
        w: usize,
        h: usize,
        t: f32,
        bands: &[f32; 32],
        audio: &Reactivity,
    ) {
        let fw = w as f32;
        let fh = h as f32;
        let energy = unit(audio.energy);
        let flash = unit(audio.onset)
            .max(unit(audio.kick))
            .max(unit(audio.transients[0]))
            .max(energy * 0.35);
        for j in 0..h {
            for i in 0..w {
                let n = noise2(i as f32 * 0.075 + t * 0.03, j as f32 * 0.11 - t * 0.012);
                let level = (n - 0.56).max(0.0) * 0.55 * (0.45 + energy) + flash * 0.04;
                if level > 0.02 {
                    let mark = if n > 0.76 { '▒' } else { '░' };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
        let seed = (t * 0.8).floor() as u32;
        let branch_skip =
            (0.38 - unit(audio.onset) * 0.20 - unit(audio.treble) * 0.06).clamp(0.04, 0.50);
        let bright = 0.35 + flash * 0.60;
        for bolt in build_bolt(seed, fw, fh, branch_skip) {
            self.canvas
                .stroke(bolt.a, bolt.b, bright - bolt.depth as f32 * 0.03);
            self.canvas
                .dot(bolt.b[0] + 1.0, bolt.b[1], '·', bright * 0.28);
            self.canvas
                .dot(bolt.b[0] - 1.0, bolt.b[1], '·', bright * 0.28);
        }
        if energy > 0.45 {
            for bolt in build_bolt(seed ^ 0xA5A5, fw, fh, branch_skip + 0.05) {
                self.canvas.stroke(
                    bolt.a,
                    bolt.b,
                    0.25 + flash * 0.45 - bolt.depth as f32 * 0.02,
                );
            }
        }
        let band = unit(bands[3]);
        if band > 0.3 {
            for k in 0..20 {
                let x = hash01(seed + k * 11) * fw;
                let y = hash01(seed + k * 11 + 7) * fh;
                self.canvas.dot(x, y, '·', band * 0.25);
            }
        }
    }

    /// Soft cumulus blobs drifting and swelling with the low end.
    fn draw_cumulus(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        if self.clouds.is_empty() {
            self.clouds = build_clouds();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let energy = unit(audio.energy);
        let treble = unit(audio.treble);
        for j in 0..h {
            let v = j as f32 / fh;
            for i in 0..w {
                let u = i as f32 / fw;
                let mut density = 0.0;
                for cloud in &self.clouds {
                    let cx = cloud.p[0] + (t * 0.02 + cloud.phase).sin() * cloud.drift;
                    let cy = cloud.p[1] + (t * 0.03 + cloud.phase * 1.3).cos() * cloud.drift * 0.6;
                    let dx = u - cx;
                    let dy = (v - cy) * 1.15;
                    let rr = cloud.r * (0.78 + bass * 0.50);
                    density += (-(dx * dx + dy * dy) / (rr * rr * 0.55)).exp();
                }
                let texture = 0.82 + 0.36 * noise2(u * 9.0 + t * 0.06, v * 9.0);
                let level = (density * texture * (0.65 + energy * 0.35) - 0.45).clamp(0.0, 1.0);
                if level > 0.02 {
                    let mark = if level > 0.60 {
                        '▓'
                    } else if level > 0.34 {
                        '▒'
                    } else if level > 0.15 {
                        '░'
                    } else if hash2(i as i32, j as i32) < treble * 0.5 {
                        '.'
                    } else {
                        '·'
                    };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
    }

    /// Layered fog banks drifting sideways with a fine internal texture.
    fn draw_fog(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let flatness = unit(audio.flatness);
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        for j in 0..h {
            let v = j as f32 / fh;
            for i in 0..w {
                let u = i as f32 / fw;
                let warp = (u * 5.0 + t * 0.06).sin() * 0.12 + (v * 3.0 + t * 0.04).cos() * 0.08;
                let band =
                    (v * 7.0 - t * 0.09 + warp * 4.0 + (u * 2.5 + t * 0.03).sin() * 0.9).sin();
                let fine = noise2(u * 14.0 + t * 0.05, v * 10.0 - t * 0.03);
                let density =
                    0.52 + band * 0.26 + (fine - 0.5) * 0.55 + flatness * 0.18 + bass * 0.05
                        - (v - 0.5).abs() * 0.22;
                let mut level = (density - 0.47).max(0.0);
                if level > 0.01 {
                    let mut mark = if level > 0.30 {
                        '▒'
                    } else if level > 0.14 {
                        '░'
                    } else {
                        '·'
                    };
                    if treble > 0.30 && hash2(i as i32, j as i32) < treble * 0.12 {
                        mark = '≈';
                        level += 0.10;
                    }
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
    }

    /// Three layers of drifting flakes over an accumulating ground drift.
    fn draw_snow(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let treble = unit(audio.treble);
        let density = 0.48 + treble * 0.45;
        let glyphs = ['❄', '✻', '·'];
        for layer in 0..3usize {
            let count = [130usize, 170, 210][layer];
            let level = 0.48 - layer as f32 * 0.13;
            for i in 0..count {
                let seed = layer as u32 * 1000 + i as u32 * 5;
                if hash01(seed + 1) > density {
                    continue;
                }
                let bx = hash01(seed + 2);
                let by = hash01(seed + 3);
                let fall =
                    (0.06 + hash01(seed + 4) * 0.05) * (1.0 + layer as f32 * 0.5) + bass * 0.03;
                let phase = hash01(seed + 5) * TAU;
                let y = (by * fh + t * fall * fh * 1.5).rem_euclid(fh);
                let sway = (t * (0.7 + hash01(seed + 6) * 1.2) + phase).sin()
                    * (1.0 + mid * 4.0)
                    * (0.6 + layer as f32 * 0.7);
                let x = (bx * fw + sway + t * (1.0 + mid * 3.0)).rem_euclid(fw);
                self.canvas.dot(x, y, glyphs[layer], level + treble * 0.12);
            }
        }
        for x in 0..w as i32 {
            let height = 1 + (noise2(x as f32 * 0.12 + t * 0.02, 3.7) * 2.0 + bass * 2.0) as i32;
            for k in 0..height.clamp(0, 4) {
                self.canvas
                    .plot(x, h as i32 - 1 - k, '░', 0.12 + treble * 0.08);
            }
        }
    }

    /// A rotating spiral with a calm eye and bands of rain.
    fn draw_cyclone(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let energy = unit(audio.energy);
        let treble = unit(audio.treble);
        let arms = 3.0 + unit(audio.chroma[0]) * 3.0 + treble * 1.5;
        let spin = t * (0.9 + energy * 2.2);
        let eye = 0.12 + bass * 0.06;
        for j in 0..h {
            let v = j as f32 / fh;
            for i in 0..w {
                let u = i as f32 / fw;
                let dx = u - 0.5;
                let dy = v - 0.5;
                let r = (dx * dx * 0.8 + dy * dy).sqrt() * 2.0;
                let theta = dy.atan2(dx);
                if r < eye {
                    if (r - eye).abs() < 0.035 {
                        self.canvas.plot(i as i32, j as i32, '·', 0.20);
                    }
                    continue;
                }
                let spiral = (arms * theta + 8.0 * r - spin).sin();
                let glow = (1.0 - r).max(0.0);
                let arm = (spiral * 0.5 + 0.5).powf(1.4) * glow * 1.35;
                let haze = glow * 0.09 + 0.02 * noise2(u * 8.0 + t * 0.10, v * 8.0);
                let level = (arm + haze).clamp(0.0, 1.0);
                if level > 0.02 {
                    let tangent = theta + PI * 0.5;
                    let mark = if level > 0.35 {
                        line_glyph(tangent.cos(), tangent.sin() * 2.0)
                    } else if level > 0.14 {
                        '░'
                    } else {
                        '·'
                    };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
        self.canvas
            .dot(fw * 0.5, fh * 0.5, '◉', 0.12 + unit(audio.kick) * 0.25);
    }

    /// Fast round hailstones, bouncing impacts, and a low accumulation ridge.
    fn draw_hail(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let density = 0.20 + treble * 0.35;
        let wind = t * (1.5 + unit(audio.mid) * 5.0);
        for i in 0..200u32 {
            if hash01(i * 9 + 1) > density {
                continue;
            }
            let x0 = hash01(i * 9 + 2) * fw;
            let speed = (0.8 + bass * 0.4) * (0.7 + hash01(i * 9 + 3) * 0.6);
            let y = ((hash01(i * 9 + 4) * (fh + 10.0) + t * speed * fh * 0.8)
                .rem_euclid(fh + 10.0))
                - 5.0;
            let x = (x0 + y * 0.16 + wind).rem_euclid(fw);
            let len = 1.5 + hash01(i * 9 + 5) * 2.0;
            self.canvas
                .stroke([x - 0.6 * len, y - len], [x, y], 0.20 + treble * 0.25);
            self.canvas
                .dot(x, y, if bass > 0.5 { '◉' } else { '●' }, 0.55 + bass * 0.25);
        }
        for k in 0..12u32 {
            let phase = (t * (0.7 + hash01(k * 17 + 1) * 0.7) + hash01(k * 17 + 2)).rem_euclid(1.0);
            let x = hash01(k * 17 + 3) * fw + phase * 10.0;
            let height = (2.0 + unit(audio.kick) * 5.0) * (phase * PI).sin();
            let y = fh - 1.0 - height;
            self.canvas.dot(x, y, '○', 0.45 + unit(audio.kick) * 0.40);
        }
        for x in 0..w as i32 {
            let n = noise2(x as f32 * 0.16 + t * 0.03, 1.3);
            let height = 1 + (n * 2.0 + bass * 2.0) as i32;
            for k in 0..height.clamp(0, 4) {
                self.canvas
                    .plot(x, h as i32 - 1 - k, '░', 0.14 + bass * 0.14);
            }
        }
    }

    /// A towering anvil cloud with internal flashes and rain beneath its base.
    fn draw_thunderhead(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let energy = unit(audio.energy);
        let flash = unit(audio.onset)
            .max(unit(audio.kick))
            .max(unit(audio.transients[1]));
        let grow = unit(audio.bass) * 0.10 + energy * 0.06;
        let flash_seed = (t * 9.0) as i32;
        for j in 0..h {
            let v = j as f32 / fh;
            for i in 0..w {
                let u = i as f32 / fw;
                let mut d = f32::NEG_INFINITY;
                let q = ((u - 0.5) / 0.45).powi(2) + ((v - 0.16) / 0.115).powi(2);
                d = d.max(1.0 - q);
                for k in 0..5 {
                    let kk = k as f32;
                    let cy = 0.30 + kk * 0.085;
                    let cx = 0.5 + (kk * 1.9 + t * 0.12).sin() * 0.025;
                    let r = 0.16 - kk * 0.005 + grow;
                    let q = ((u - cx) / r).powi(2) + ((v - cy) / (r * 0.8)).powi(2);
                    d = d.max(1.0 - q);
                }
                let q = ((u - 0.5) / 0.42).powi(2) + ((v - 0.70) / 0.095).powi(2);
                d = d.max(1.0 - q);
                d += (noise2(u * 9.0 + t * 0.05, v * 9.0) - 0.5) * 0.22;
                if d > 0.02 {
                    let mark = if d > 0.58 {
                        '▓'
                    } else if d > 0.30 {
                        '▒'
                    } else {
                        '░'
                    };
                    let level = (0.12 + d * 0.45 + energy * 0.08).clamp(0.0, 1.0);
                    if flash > 0.18 && hash2(i as i32 + flash_seed, j as i32) < flash * 0.35 {
                        self.canvas.plot(i as i32, j as i32, '▓', 0.80);
                    } else {
                        self.canvas.plot(i as i32, j as i32, mark, level);
                    }
                }
            }
        }
        let base = 0.78 * fh;
        let span = (fh - base).max(1.0);
        for i in 0..160u32 {
            if hash01(i * 7 + 1) > 0.40 + unit(audio.treble) * 0.40 {
                continue;
            }
            let x0 = hash01(i * 7 + 2) * fw;
            let speed = 0.9 + hash01(i * 7 + 3) * 0.8;
            let y = base + (hash01(i * 7 + 4) * span + t * speed * span * 1.2).rem_euclid(span);
            let x = (x0 + t * 2.0).rem_euclid(fw);
            self.canvas
                .stroke([x - 1.0, y - 3.0], [x, y], 0.18 + unit(audio.treble) * 0.15);
        }
    }

    /// A wobbling column of dust with orbiting debris.
    fn draw_dust(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let energy = unit(audio.energy);
        let onset = unit(audio.onset);
        let center_at =
            |v: f32| 0.5 + (v * 3.2 + t * 0.35).sin() * 0.045 + (v * 8.0 - t * 0.7).sin() * 0.012;
        for j in 0..h {
            let v = j as f32 / fh;
            let center = center_at(v);
            let width = 0.016 + 0.12 * v + bass * 0.03;
            for i in 0..w {
                let u = i as f32 / fw;
                let dx = (u - center) / width;
                let r = dx.abs();
                let swirl =
                    ((v * 22.0 - t * (2.0 + energy * 3.0) + dx * 4.0).sin() * 0.5 + 0.5).powf(1.6);
                let inside = (1.0 - r).max(0.0);
                let density = inside * (0.22 + swirl * 0.90);
                let haze = (1.0 - (r * 0.35).min(1.0)) * 0.055;
                let level = (density + haze).clamp(0.0, 1.0);
                if level > 0.02 {
                    let mark = if density > 0.62 {
                        '▒'
                    } else if density > 0.30 {
                        '░'
                    } else {
                        line_glyph(dx * 0.4, 1.0)
                    };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
        for k in 0..60u32 {
            let v = hash01(k * 7 + 1);
            let center = center_at(v);
            let phase = t * (0.7 + hash01(k * 7 + 2) * 1.8) + hash01(k * 7 + 3) * TAU;
            let spread = 0.02 + hash01(k * 7 + 4) * 0.16 + onset * 0.04;
            let x = (center + spread * phase.sin()) * fw;
            let y = v * fh;
            self.canvas.dot(
                x,
                y,
                if k % 7 == 0 { '•' } else { '·' },
                0.30 + onset * 0.30,
            );
        }
    }

    /// A radar display: sweeper beam, range rings, and precipitation echoes.
    fn draw_storm(&mut self, w: usize, h: usize, t: f32, audio: &Reactivity) {
        if self.echoes.is_empty() {
            self.echoes = build_echoes();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let energy = unit(audio.energy);
        let treble = unit(audio.treble);
        let sweep = t * (0.7 + energy * 1.1)
            + unit(audio.locked_beat_phase()) * TAU
            + unit(audio.centroid) * 2.0;
        for j in 0..h {
            let v = j as f32 / fh;
            for i in 0..w {
                let u = i as f32 / fw;
                let dx = u - 0.5;
                let dy = v - 0.5;
                let r = ((dx * dx + dy * dy).sqrt() * 2.0).min(2.0);
                let mut echo = 0.0;
                for e in &self.echoes {
                    let ex = u - e.p[0];
                    let ey = v - e.p[1];
                    echo += e.strength * (e.r * e.r * 0.35) / (ex * ex + ey * ey + e.r * e.r * 0.5);
                }
                echo *= 0.55 + bass * 0.80 + energy * 0.20;
                let angle = dy.atan2(dx);
                let diff = ((angle - sweep + PI).rem_euclid(TAU) - PI).abs();
                let beam = (1.0 - diff / 0.14).max(0.0);
                let trail = (1.0 - diff / 0.90).max(0.0) * 0.18;
                let ring = if (r * 6.0).fract() < 0.045 && hash2(i as i32, j as i32) < 0.35 {
                    0.10
                } else {
                    0.0
                };
                let level = (echo - 0.35)
                    .max(0.0)
                    .max(beam * 0.90)
                    .max(trail)
                    .max(ring)
                    .clamp(0.0, 1.0);
                if level > 0.01 {
                    let mark = if beam > 0.50 {
                        line_glyph(angle.cos(), angle.sin() * 2.0)
                    } else if echo > 0.70 {
                        '▓'
                    } else if echo > 0.40 {
                        '▒'
                    } else if echo > 0.15 {
                        '░'
                    } else {
                        '·'
                    };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
        if treble > 0.35 {
            let n = (treble * 80.0) as usize;
            for k in 0..n {
                let x = hash01(k as u32 * 13 + 5) * fw;
                let y = hash01(k as u32 * 13 + 9) * fh;
                self.canvas.dot(x, y, '·', treble * 0.20);
            }
        }
        let cx = (w / 2) as i32;
        let cy = (h / 2) as i32;
        self.canvas.plot(cx, cy, '┼', 0.55);
        for d in 1..=2 {
            self.canvas.plot(cx + d, cy, '─', 0.25);
            self.canvas.plot(cx - d, cy, '─', 0.25);
            self.canvas.plot(cx, cy + d, '│', 0.25);
            self.canvas.plot(cx, cy - d, '│', 0.25);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(mode: usize, audio: &Reactivity) -> String {
        let area = Rect::new(2, 3, 96, 36);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..90 {
            renderer.draw(
                &mut buf,
                area,
                frame as f32 / 60.0,
                mode,
                0,
                &[0.0; 32],
                audio,
                &Theme::default(),
            );
        }
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn names_are_ten_and_unique() {
        assert_eq!(NAMES.len(), 10);
        for (i, name) in NAMES.iter().enumerate() {
            assert!(!name.is_empty());
            for other in &NAMES[i + 1..] {
                assert_ne!(name, other, "duplicate scene name");
            }
        }
    }

    #[test]
    fn every_style_draws_and_scenes_differ() {
        let quiet = Reactivity::default();
        let mut images = Vec::new();
        for (mode, _) in NAMES.iter().enumerate() {
            let image = snapshot(mode, &quiet);
            assert!(
                image.chars().any(|mark| mark != ' '),
                "empty quiet scene {mode}"
            );
            assert!(!images.contains(&image), "duplicate scene {mode}");
            images.push(image);
        }
    }

    #[test]
    fn audio_changes_every_scene() {
        let quiet = Reactivity::default();
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 0.8,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.5,
            ..Reactivity::default()
        };
        for (mode, _) in NAMES.iter().enumerate() {
            assert_ne!(
                snapshot(mode, &quiet),
                snapshot(mode, &loud),
                "scene {mode} ignores audio"
            );
        }
    }

    #[test]
    fn drawing_stays_inside_the_visible_area() {
        let mut renderer = Renderer::default();
        let area = Rect::new(4, 5, 11, 7);
        for (mode, _) in NAMES.iter().enumerate() {
            for target in [
                Rect::new(0, 0, 0, 0),
                Rect::new(5, 6, 1, 1),
                Rect::new(0, 0, 10, 11),
                Rect::new(99, 99, 3, 3),
            ] {
                let mut buf = Buffer::empty(area);
                for cell in &mut buf.content {
                    cell.set_char('!');
                }
                renderer.draw(
                    &mut buf,
                    target,
                    f32::NAN,
                    mode,
                    0,
                    &[f32::NAN; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                let visible = area.intersection(target);
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        if !visible.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "!");
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "manual visual preview"]
    fn preview() {
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        let bands: [f32; 32] = std::array::from_fn(|i| (i as f32 * 0.7).sin().abs() * 0.8);
        let audio = Reactivity {
            energy: 0.8,
            bass: 0.9,
            mid: 0.6,
            treble: 0.7,
            onset: 0.2,
            kick: 0.6,
            beat: 1.0,
            beat_phase: 0.4,
            centroid: 0.6,
            flatness: 0.4,
            ..Reactivity::default()
        };
        for (mode, _) in NAMES.iter().enumerate() {
            for frame in 0..30 {
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 30.0,
                    mode,
                    0,
                    &bands,
                    &audio,
                    &Theme::default(),
                );
            }
            println!("=== {mode} {}", NAMES[mode]);
            for y in 0..area.height {
                let row: String = (0..area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                println!("{row}");
            }
        }
    }
}
