//! Cityscapes: skylines, facades, streets, transit maps, bridges, roofs,
//! aerial blocks, clock towers, neon alleys, and traffic at rush hour.
//!
//! Static city geometry is precomputed; windows, lights, vehicles, and signals
//! are deterministic functions of time and the live audio state.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Skyline Pulse",
    "Window Matrix",
    "Street Canyon",
    "Metro Weave",
    "Bridge Span",
    "Rooftop Rain",
    "Aerial Grid",
    "Clock Tower",
    "Neon Alley",
    "Rush Hour",
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

/// One frame's glyph, brightness, and optional explicit hue. `hue` is negative
/// when the cell should follow its position-derived palette phase.
#[derive(Default)]
struct Canvas {
    w: usize,
    h: usize,
    level: Vec<f32>,
    mark: Vec<char>,
    hue: Vec<f32>,
}

impl Canvas {
    fn reset(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            self.w = w;
            self.h = h;
            self.level = vec![0.0; w * h];
            self.mark = vec![' '; w * h];
            self.hue = vec![-1.0; w * h];
        } else {
            self.level.fill(0.0);
            self.mark.fill(' ');
            self.hue.fill(-1.0);
        }
    }

    fn plot(&mut self, x: i32, y: i32, mark: char, level: f32) {
        self.plot_tinted(x, y, mark, level, -1.0);
    }

    fn plot_tinted(&mut self, x: i32, y: i32, mark: char, level: f32, hue: f32) {
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
            self.hue[i] = hue;
        }
    }

    fn dot(&mut self, x: f32, y: f32, mark: char, level: f32) {
        self.plot(x.round() as i32, y.round() as i32, mark, level);
    }

    fn dot_tinted(&mut self, x: f32, y: f32, mark: char, level: f32, hue: f32) {
        self.plot_tinted(x.round() as i32, y.round() as i32, mark, level, hue);
    }

    fn stroke(&mut self, a: [f32; 2], b: [f32; 2], level: f32) {
        self.stroke_tinted(a, b, level, -1.0);
    }

    fn stroke_tinted(&mut self, a: [f32; 2], b: [f32; 2], level: f32, hue: f32) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let glyph = line_glyph(dx, dy);
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        for step in 0..=steps {
            let f = step as f32 / steps as f32;
            self.plot_tinted(
                (a[0] + dx * f).round() as i32,
                (a[1] + dy * f).round() as i32,
                glyph,
                level,
                hue,
            );
        }
    }
}

fn point_along(pts: &[[f32; 2]], s: f32) -> [f32; 2] {
    let total: f32 = pts
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    if !total.is_finite() || total <= 0.0 {
        return pts.first().copied().unwrap_or([0.0, 0.0]);
    }
    let target = unit(s) * total;
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if acc + len >= target {
            let f = ((target - acc) / len.max(0.0001)).clamp(0.0, 1.0);
            return [
                w[0][0] + (w[1][0] - w[0][0]) * f,
                w[0][1] + (w[1][1] - w[0][1]) * f,
            ];
        }
        acc += len;
    }
    pts[pts.len() - 1]
}

fn hue_for(mode: usize, u: f32, v: f32, audio: &Reactivity) -> f32 {
    let base = match mode {
        0 => 0.62 + v * 0.22,
        1 => 0.50 + u * 0.30,
        2 => 0.10 + v * 0.25,
        3 => 0.35 + u * 0.25,
        4 => 0.55 + u * 0.25,
        5 => 0.20 + v * 0.25,
        6 => 0.45 + u * 0.30,
        7 => 0.75 + v * 0.18,
        8 => 0.05 + u * 0.35,
        _ => 0.58 + u * 0.30,
    };
    (base + unit(audio.centroid) * 0.16 + unit(audio.locked_beat_phase()) * 0.05).rem_euclid(1.0)
}

#[derive(Clone, Copy)]
struct Building {
    x: f32,
    w: f32,
    top: f32,
    layer: u8,
    seed: u32,
}

fn build_skyline() -> Vec<Building> {
    let mut out = Vec::with_capacity(60);
    for layer in 0..3u8 {
        let mut x = -0.03f32;
        let mut k = 0u32;
        while x < 1.0 {
            let seed = layer as u32 * 1000 + k * 17 + 1;
            let w = 0.045 + hash01(seed) * 0.090;
            let base = [0.62f32, 0.52, 0.42][layer as usize];
            let top = base - hash01(seed + 3) * 0.14 - layer as f32 * 0.04;
            out.push(Building {
                x,
                w: w * 0.90,
                top,
                layer,
                seed,
            });
            x += w + 0.006;
            k += 1;
        }
    }
    out
}

struct MetroLine {
    pts: Vec<[f32; 2]>,
    hue: f32,
}

fn build_metro() -> Vec<MetroLine> {
    (0..6)
        .map(|i| {
            let k = i as u32 * 11;
            let y0 = 0.08 + hash01(k + 1) * 0.78;
            let x1 = 0.08 + hash01(k + 2) * 0.28;
            let y1 = (y0 + (hash01(k + 3) - 0.5) * 0.45).clamp(0.08, 0.90);
            let dx = 0.04 + hash01(k + 4) * 0.10;
            let x2 = (x1 + dx + 0.10 + hash01(k + 5) * 0.20).min(0.74);
            let dx2 = 0.04 + hash01(k + 6) * 0.10;
            let y2 = (y1 + (hash01(k + 7) - 0.5) * 0.35).clamp(0.08, 0.92);
            MetroLine {
                pts: vec![
                    [-0.06, y0],
                    [x1, y0],
                    [x1 + dx, y1],
                    [x2, y1],
                    [x2 + dx2, y2],
                    [1.06, y2],
                ],
                hue: (i as f32 / 6.0 + 0.06).rem_euclid(1.0),
            }
        })
        .collect()
}

#[derive(Default)]
pub struct Renderer {
    canvas: Canvas,
    skyline: Vec<Building>,
    metro: Vec<MetroLine>,
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
            0 => self.draw_skyline(w, h, now, bands, audio),
            1 => self.draw_windows(w, h, now, bands, audio),
            2 => self.draw_street(w, h, now, bands, audio),
            3 => self.draw_metro(w, h, now, bands, audio),
            4 => self.draw_bridge(w, h, now, bands, audio),
            5 => self.draw_rooftop(w, h, now, bands, audio),
            6 => self.draw_aerial(w, h, now, bands, audio),
            7 => self.draw_clocktower(w, h, now, bands, audio),
            8 => self.draw_neon(w, h, now, bands, audio),
            _ => self.draw_rush(w, h, now, bands, audio),
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
                let index = j * w + i;
                let level = self.canvas.level[index];
                if level <= 0.02 {
                    continue;
                }
                let u = i as f32 / w.max(1) as f32;
                let v = j as f32 / h.max(1) as f32;
                let tint = self.canvas.hue[index];
                let hue = if tint >= 0.0 {
                    (tint + unit(audio.centroid) * 0.10).rem_euclid(1.0)
                } else {
                    hue_for(mode, u, v, audio)
                };
                let bright = (level * (0.62 + energy * 0.30)).clamp(0.03, 1.0);
                cell.set_char(self.canvas.mark[index]);
                cell.set_fg(theme.sample(palette, hue, bright));
            }
        }
    }

    fn night_sky(&mut self, w: usize, h: usize, limit: f32, t: f32, treble: f32) {
        let fh = h as f32;
        let fw = w as f32;
        for j in 0..((limit * fh) as usize).min(h) {
            let v = j as f32 / fh;
            for i in 0..w {
                let level = 0.035 * (1.0 - v) + 0.01;
                self.canvas.plot(i as i32, j as i32, '·', level * 0.4);
                if hash2(i as i32, j as i32) < 0.0022 + treble * 0.004 {
                    self.canvas
                        .plot(i as i32, j as i32, '·', 0.25 + treble * 0.2);
                }
            }
        }
        let mx = fw * 0.82;
        let my = fh * 0.10;
        self.canvas.dot(mx, my, '◐', 0.65);
        for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            self.canvas.dot(mx + dx, my + dy, '·', 0.15);
        }
        let _ = t;
    }

    /// Layered skyline silhouettes with lit windows, antennas, and searchlights.
    fn draw_skyline(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.skyline.is_empty() {
            self.skyline = build_skyline();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        self.night_sky(w, h, 0.80, t, treble);
        let bucket = (t * 1.5) as i32;
        for building in &self.skyline {
            let x0 = (building.x * fw).round() as i32;
            let x1 = ((building.x + building.w) * fw).round() as i32;
            let y0 = (building.top * fh).round() as i32;
            if x1 <= x0 {
                continue;
            }
            let base = 0.40 + building.layer as f32 * 0.18;
            for y in y0..h as i32 {
                for x in x0..=x1 {
                    self.canvas.plot(x, y, ' ', base);
                }
                self.canvas.plot(x0, y, '│', base + 0.08);
                self.canvas.plot(x1, y, '│', base + 0.08);
            }
            for x in x0..=x1 {
                self.canvas.plot(x, y0, '─', base + 0.10);
            }
            for y in (y0 + 2..h as i32).step_by(3) {
                for x in (x0 + 1..x1).step_by(3) {
                    let band = unit(bands[(x + y).rem_euclid(32) as usize]);
                    let threshold = 0.10 + bass * 0.22 + band * 0.16;
                    if hash2(x + bucket, y) < threshold {
                        let level = base + 0.12 + band * 0.04;
                        self.canvas.plot(x, y, '▪', level);
                    }
                }
            }
            if building.seed % 5 == 0 {
                let ax = (x0 + x1) / 2;
                for d in 1..=3 {
                    self.canvas.plot(ax, y0 - d, '│', base + 0.14);
                }
                let blink = 0.5 + 0.5 * (t * 2.0 + building.seed as f32).sin();
                self.canvas
                    .plot(ax, y0 - 4, '·', base + 0.12 + blink * 0.06);
            }
            if energy > 0.65 && building.layer == 2 && building.seed % 11 == 0 {
                let ax = (x0 + x1) as f32 * 0.5;
                let spread = 0.10 + treble * 0.08;
                self.canvas.stroke(
                    [ax, y0 as f32],
                    [ax - fw * spread, 0.0],
                    0.08 + energy * 0.08,
                );
                self.canvas.stroke(
                    [ax, y0 as f32],
                    [ax + fw * spread, 0.0],
                    0.08 + energy * 0.08,
                );
            }
        }
        let wave = ((unit(audio.kick) * 0.9 + t * 0.35) % 1.0) * fh;
        for y in (wave as i32 - 1)..=(wave as i32 + 1) {
            for x in 0..w as i32 {
                self.canvas.plot(x, y, '─', 0.18 + unit(audio.kick) * 0.25);
            }
        }
    }

    /// A full-frame facade of glass windows and structural mullions.
    fn draw_windows(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let cols = (w / 4).max(1);
        let rows = (h / 4).max(1);
        let cw = fw / cols as f32;
        let ch = fh / rows as f32;
        let bucket = (t * 2.0) as i32;
        let sweep = (unit(audio.locked_beat_phase()) * rows as f32) as i32;
        for gy in 0..rows {
            let y0 = (gy as f32 * ch).round() as i32;
            let y1 = (((gy + 1) as f32 * ch).round() as i32 - 1).max(y0);
            for gx in 0..cols {
                let x0 = (gx as f32 * cw).round() as i32;
                let x1 = (((gx + 1) as f32 * cw).round() as i32 - 1).max(x0);
                let band = unit(bands[(gx * 3 + gy * 5) % 32]);
                let floor_lit = hash2(gy as i32 + bucket / 2, 71) < 0.45 + bass * 0.35;
                let lit = gy as i32 == sweep
                    || (floor_lit && hash2(gx as i32 + bucket, gy as i32) < 0.55 + band * 0.30);
                let (mark, level) = if lit {
                    ('▓', 0.30 + band * 0.42 + bass * 0.22)
                } else {
                    ('░', 0.055 + band * 0.04 + energy * 0.02)
                };
                for y in y0..=y1 {
                    self.canvas.plot(x1, y, '│', 0.10);
                }
                for x in x0..=x1 {
                    self.canvas.plot(x, y1, '─', 0.12);
                }
                for y in (y0 + 1)..y1 {
                    for x in (x0 + 1)..x1 {
                        self.canvas.plot(x, y, mark, level);
                    }
                }
                if lit && hash2(gx as i32, gy as i32 + bucket) < treble * 0.35 {
                    self.canvas.plot(x0, y0, '·', 0.45 + treble * 0.3);
                }
            }
        }
    }

    /// A perspective street canyon: converging walls, lane markings, headlights.
    fn draw_street(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let horizon = (fh * 0.40).round() as i32;
        let vp = fw * 0.5 + (t * 0.06).sin() * fw * 0.012;
        let bucket = (t * 1.6) as i32;
        self.night_sky(w, h, 0.40, t, treble);
        for j in horizon..h as i32 {
            let d = (j - horizon) as f32 / (h as i32 - horizon).max(1) as f32;
            let half = (0.025 + 0.46 * d) * fw;
            let left = vp - half;
            let right = vp + half;
            for i in 0..w as i32 {
                let x = i as f32;
                if x < left || x > right {
                    let (side, edge) = if x < left { (0, left) } else { (1, right) };
                    let dist = (x - edge).abs();
                    let window_w = 1.0 + 3.0 * (1.0 - d);
                    let window_h = 2.0 + 3.0 * (1.0 - d);
                    let wx = (dist / window_w).floor() as i32;
                    let wy = ((j - horizon) as f32 / window_h).floor() as i32;
                    let fx = (dist / window_w).fract();
                    let fy = ((j - horizon) as f32 / window_h).fract();
                    let lit = hash2(wx + side * 777 + bucket, wy)
                        < 0.16 + bass * 0.26 + unit(bands[(wx as usize + wy as usize) % 32]) * 0.2;
                    if lit && fx < 0.6 && fy < 0.55 {
                        self.canvas
                            .plot(i, j, '▪', 0.30 + bass * 0.34 + treble * 0.10);
                    } else {
                        self.canvas.plot(i, j, '▓', 0.10 + d * 0.02);
                    }
                } else {
                    let lane = (x - vp) / half;
                    if lane.abs() < 0.025 {
                        if ((j as f32 + t * 9.0) as i32 / 3) % 2 == 0 {
                            self.canvas.plot(i, j, '─', 0.32);
                        }
                    } else if (lane.abs() - 0.52).abs() < 0.018 {
                        if ((j as f32 + t * 6.0) as i32 / 2) % 2 == 0 {
                            self.canvas.plot(i, j, '│', 0.22);
                        }
                    } else if (lane.abs() - 0.96).abs() < 0.02 {
                        self.canvas.plot(i, j, '│', 0.30 + d * 0.2);
                    } else {
                        self.canvas.plot(i, j, '·', 0.05 + d * 0.03);
                    }
                }
            }
        }
        for (sx, hue) in [((fw * 0.06) as i32, 0.96), ((fw * 0.94) as i32, 0.55)] {
            let top = horizon + 1;
            for y in top..=(top + 5) {
                self.canvas.plot_tinted(sx, y, '│', 0.45, hue);
                self.canvas.plot_tinted(sx + 2, y, '│', 0.45, hue);
                self.canvas.plot_tinted(
                    sx + 1,
                    y,
                    if y % 2 == 0 { '▒' } else { '·' },
                    0.28 + treble * 0.15,
                    hue,
                );
            }
            for x in sx..=(sx + 2) {
                self.canvas.plot_tinted(x, top, '─', 0.50, hue);
                self.canvas.plot_tinted(x, top + 5, '─', 0.50, hue);
            }
        }
        // Distant skyline in the gap above the road.
        for x in (0..w as i32).step_by(9) {
            let width = 3 + (hash2(x, 91) * 4.0) as i32;
            let height = 2 + (hash2(x, 92) * 8.0) as i32;
            for bx in x..(x + width) {
                for y in (horizon - height)..horizon {
                    self.canvas.plot(bx, y, '▓', 0.06);
                    if hash2(bx, y) < 0.10 {
                        self.canvas.plot(bx, y, '▪', 0.16);
                    }
                }
            }
        }
        for k in 0..54u32 {
            let speed = 0.45 + hash01(k * 5 + 1) * 0.8 + energy * 0.6;
            let p = (t * speed + hash01(k * 5 + 2)).fract();
            let d = p * p * p;
            let y = horizon as f32 + d * (h as i32 - horizon) as f32;
            let half = (0.025 + 0.46 * d) * fw;
            let lane = (hash01(k * 5 + 3) - 0.5) * 1.8;
            let x = vp + lane * half;
            self.canvas.dot(x, y, '●', 0.45 + d * 0.5);
            self.canvas.dot(x, y - 1.0, '·', 0.20 + d * 0.3);
        }
        for step in 1..5 {
            let d = step as f32 / 5.0;
            let y = horizon as f32 + d * (h as i32 - horizon) as f32;
            let half = (0.025 + 0.46 * d) * fw;
            for side in [-1.0f32, 1.0] {
                let x = vp + side * half * 0.9;
                for d in 0..3 {
                    self.canvas.dot(x, y - d as f32, '│', 0.20);
                }
                self.canvas.dot(x, y - 3.0, '·', 0.35 + treble * 0.25);
            }
        }
    }
}

impl Renderer {
    /// A transit diagram with tinted lines, stations, and moving trains.
    fn draw_metro(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.metro.is_empty() {
            self.metro = build_metro();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        for j in 0..h {
            for i in 0..w {
                if hash2(i as i32, j as i32) < 0.022 {
                    self.canvas.plot(i as i32, j as i32, '·', 0.04);
                }
            }
        }
        for i in 0..w {
            let y = fh * (0.70 + 0.05 * i as f32 / fw) + (i as f32 * 0.02 + t * 0.2).sin() * 2.0;
            let band = unit(bands[i % 32]);
            self.canvas
                .plot(i as i32, y.round() as i32, '≈', 0.035 + band * 0.02);
        }
        for (li, line) in self.metro.iter().enumerate() {
            for pair in line.pts.windows(2) {
                self.canvas.stroke_tinted(
                    [pair[0][0] * fw, pair[0][1] * fh],
                    [pair[1][0] * fw, pair[1][1] * fh],
                    0.48 + bass * 0.18,
                    line.hue,
                );
            }
            for s in [0.10, 0.24, 0.38, 0.52, 0.66, 0.80, 0.92] {
                let p = point_along(&line.pts, s);
                let band = unit(bands[((s * 31.0) as usize + li * 3) % 32]);
                self.canvas.dot_tinted(
                    p[0] * fw,
                    p[1] * fh,
                    '●',
                    0.55 + band * 0.35 + treble * 0.10,
                    line.hue,
                );
            }
            for train in 0..2u32 {
                let speed = 0.045 + energy * 0.07 + bass * 0.02;
                let s = (t * speed + li as f32 * 0.17 + train as f32 * 0.5).rem_euclid(1.0);
                let p = point_along(&line.pts, s);
                self.canvas
                    .dot_tinted(p[0] * fw, p[1] * fh, '●', 0.95, line.hue);
                for tail in 1..4 {
                    let q = point_along(&line.pts, (s - tail as f32 * 0.007).rem_euclid(1.0));
                    self.canvas.dot_tinted(
                        q[0] * fw,
                        q[1] * fh,
                        '·',
                        0.55 - tail as f32 * 0.10,
                        line.hue,
                    );
                }
            }
        }
        self.canvas.plot((w / 2) as i32, (h / 2) as i32, '◉', 0.55);
    }

    /// A suspension bridge over rippling water with traffic and reflection.
    fn draw_bridge(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let bass = unit(audio.bass);
        self.night_sky(w, h, 0.60, t, treble);
        let deck = fh * 0.58;
        let top = fh * 0.20;
        let t1 = fw * 0.22;
        let t2 = fw * 0.78;
        let mid = (t1 + t2) * 0.5;
        let half = (t2 - t1) * 0.5;
        let sag = fh * 0.13;
        for j in (deck as i32 + 2)..h as i32 {
            for i in 0..w as i32 {
                let wave = (i as f32 * 0.13 + j as f32 * 0.5 + t * 2.2).sin();
                if wave > 0.86 {
                    self.canvas
                        .plot(i, j, '≈', 0.05 + (wave - 0.86) * 0.25 + bass * 0.03);
                } else if hash2(i, j) < 0.02 {
                    self.canvas.plot(i, j, '·', 0.05);
                }
            }
        }
        for tx in [t1, t2] {
            let x = tx.round() as i32;
            for y in top as i32..deck as i32 {
                self.canvas.plot(x, y, '│', 0.55);
                self.canvas.plot(x + 1, y, '│', 0.40);
                if y % 6 == 0 {
                    for dx in -2..=3 {
                        self.canvas.plot(x + dx, y, '─', 0.32);
                    }
                }
            }
            for dx in -2..=3 {
                self.canvas.plot(x + dx, top as i32, '─', 0.55);
                self.canvas
                    .plot(x + dx, (top + fh * 0.12) as i32, '─', 0.48);
            }
            let blink = 0.5 + 0.5 * (t * 2.4 + tx * 0.05).sin();
            self.canvas.plot(x, top as i32 - 1, '·', 0.25 + blink * 0.4);
        }
        for x in t1 as i32..=t2 as i32 {
            let u = (x as f32 - mid) / half;
            let y = top + sag * (1.0 - u * u);
            let slope = -2.0 * sag * u / half;
            self.canvas
                .plot(x, y.round() as i32, line_glyph(1.0, slope), 0.65);
            if x % 6 == 0 {
                for yy in (y.round() as i32)..deck as i32 {
                    self.canvas.plot(x, yy, '│', 0.12);
                }
            }
        }
        self.canvas.stroke([t1, top], [0.0, deck], 0.30);
        self.canvas.stroke([t2, top], [fw - 1.0, deck], 0.30);
        for x in 0..w as i32 {
            self.canvas.plot(x, deck as i32, '─', 0.50);
            if x % 5 == 0 {
                self.canvas.plot(x, deck as i32 - 1, '·', 0.18);
            }
        }
        for k in 0..14u32 {
            let speed = 0.12 + hash01(k * 11 + 1) * 0.16 + energy * 0.26;
            let p = (t * speed + hash01(k * 11 + 2)).fract();
            let x = p * fw;
            let lane = if k % 2 == 0 { deck - 1.0 } else { deck - 2.0 };
            self.canvas.dot(x, lane, '●', 0.72 + energy * 0.2);
            self.canvas.dot(x - 1.0, lane, '·', 0.30);
        }
        for x in (t1 as i32..=t2 as i32).step_by(6) {
            for k in 0..3 {
                self.canvas
                    .plot(x, deck as i32 + 2 + k * 2, '·', 0.07 + treble * 0.04);
            }
        }
        let band = unit(bands[7]);
        if band > 0.35 {
            for k in 0..12u32 {
                let x = hash01(k * 7 + 5) * fw;
                let y = deck + 3.0 + hash01(k * 7 + 9) * (fh - deck - 3.0);
                self.canvas.dot(x, y, '·', band * 0.2);
            }
        }
    }

    /// A rooftop in the rain: skyline, water tank, antenna, puddles.
    fn draw_rooftop(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.skyline.is_empty() {
            self.skyline = build_skyline();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let kick = unit(audio.kick);
        let roof = fh * 0.55;
        self.night_sky(w, h, 0.55, t, treble);
        let bucket = (t * 1.4) as i32;
        for building in &self.skyline {
            if building.layer < 2 {
                continue;
            }
            let x0 = (building.x * fw).round() as i32;
            let x1 = ((building.x + building.w) * fw).round() as i32;
            let top = (0.10 + (building.top - 0.22) * 0.75) * fh;
            let y0 = top.round() as i32;
            for y in y0..roof as i32 {
                for x in x0..=x1 {
                    self.canvas.plot(x, y, '▓', 0.09);
                }
            }
            for x in x0..=x1 {
                self.canvas.plot(x, y0, '─', 0.18);
            }
            for y in (y0 + 1..roof as i32).step_by(3) {
                for x in (x0 + 1..x1).step_by(3) {
                    if hash2(x + bucket, y) < 0.12 + bass * 0.20 {
                        self.canvas.plot(x, y, '▪', 0.22 + bass * 0.25);
                    }
                }
            }
        }
        for i in 0..220u32 {
            if hash01(i * 7 + 1) > 0.20 + treble * 0.35 {
                continue;
            }
            let x = (hash01(i * 7 + 2) * fw + t * 3.0).rem_euclid(fw);
            let speed = 0.5 + hash01(i * 7 + 3) * 0.6;
            let y = ((hash01(i * 7 + 4) * (fh + 12.0) + t * speed * fh * 0.5)
                .rem_euclid(fh + 12.0))
                - 6.0;
            if y >= roof - 1.0 {
                continue;
            }
            let level = 0.09 + treble * 0.15;
            self.canvas.stroke([x, y], [x + 0.4, y + 2.0], level);
        }
        for j in roof as i32..h as i32 {
            for i in 0..w as i32 {
                let n = noise2(i as f32 * 0.10, j as f32 * 0.18);
                let level = 0.03 + n * 0.05 + energy * 0.015;
                self.canvas
                    .plot(i, j, if n > 0.62 { '░' } else { '·' }, level);
            }
        }
        for k in 0..5u32 {
            let cx = hash01(k * 19 + 1) * fw;
            let cy = roof + 2.0 + hash01(k * 19 + 2) * (fh - roof - 3.0);
            let band = unit(bands[(k as usize * 5) % 32]);
            let ripple = 1.0 + (t * 2.0 + hash01(k * 19 + 3) * TAU).sin() * (0.4 + kick);
            for dx in 0..12 {
                let x = cx + (dx as f32 - 6.0) * ripple;
                self.canvas
                    .dot(x, cy, '≈', 0.14 + band * 0.14 + kick * 0.24);
            }
            if kick > 0.3 {
                for step in 0..6 {
                    let angle = step as f32 / 6.0 * TAU;
                    self.canvas.dot(
                        cx + angle.cos() * 4.0 * kick,
                        cy + angle.sin() * 1.5 * kick,
                        '·',
                        0.20 + kick * 0.25,
                    );
                }
            }
        }
        let tank_x0 = (fw * 0.12) as i32;
        let tank_x1 = (fw * 0.30) as i32;
        let tank_y0 = (fh * 0.58) as i32;
        let tank_y1 = (fh * 0.72) as i32;
        for x in tank_x0..=tank_x1 {
            self.canvas.plot(x, tank_y0, '─', 0.55);
            self.canvas.plot(x, tank_y1, '─', 0.50);
        }
        for y in tank_y0..=tank_y1 {
            self.canvas.plot(tank_x0, y, '│', 0.55);
            self.canvas.plot(tank_x1, y, '│', 0.55);
        }
        for y in (tank_y0 + 1)..tank_y1 {
            for x in (tank_x0 + 1)..tank_x1 {
                self.canvas.plot(x, y, '▒', 0.09 + treble * 0.03);
            }
        }
        for x in tank_x0..=tank_x1 {
            self.canvas
                .plot(x, tank_y0 - 1, if x % 2 == 0 { '╱' } else { '╲' }, 0.30);
            self.canvas.plot(x, tank_y0 - 2, '─', 0.22);
        }
        for x in [tank_x0 + 2, tank_x1 - 2] {
            for y in (tank_y1 + 1)..(tank_y1 + 4) {
                self.canvas.plot(x, y, '│', 0.25);
            }
        }
        let ax = (fw * 0.76) as i32;
        let ay = (fh * 0.44) as i32;
        let deck_y = (fh * 0.60) as i32;
        for y in ay..=deck_y {
            self.canvas.plot(ax, y, '│', 0.35);
        }
        for d in [-2, 2] {
            self.canvas.plot(ax + d, ay + 3, '─', 0.25);
            self.canvas.plot(ax + d, ay + 6, '─', 0.25);
        }
        let blink = 0.5 + 0.5 * (t * 2.0).sin();
        self.canvas.plot(ax, ay - 1, '·', 0.30 + blink * 0.4);
        let vent_x = (fw * 0.52) as i32;
        let vent_y = (fh * 0.66) as i32;
        for x in vent_x..(vent_x + 4) {
            for y in vent_y..(vent_y + 3) {
                self.canvas.plot(x, y, '▒', 0.20 + treble * 0.1);
            }
        }
    }
}

impl Renderer {
    /// Top-down city blocks with streets, rooftop details, and moving cars.
    fn draw_aerial(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let kick = unit(audio.kick);
        let sx = (w / 7).max(7) as i32;
        let sy = (h / 5).max(5) as i32;
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let road_x = i.rem_euclid(sx) < 2;
                let road_y = j.rem_euclid(sy) < 2;
                if road_x || road_y {
                    self.canvas.plot(i, j, '·', 0.045 + treble * 0.02);
                    if road_x && i.rem_euclid(sx) == 1 && (j + (t * 2.0) as i32).rem_euclid(6) < 3 {
                        self.canvas.plot(i, j, '│', 0.12);
                    }
                    if road_y && j.rem_euclid(sy) == 1 && (i + (t * 3.0) as i32).rem_euclid(6) < 3 {
                        self.canvas.plot(i, j, '─', 0.12);
                    }
                } else {
                    let bx = i / sx;
                    let by = j / sy;
                    let band = unit(bands[((bx * 5 + by * 7) as usize) % 32]);
                    let seed = hash2(bx, by);
                    if seed > 0.86 {
                        self.canvas.plot(i, j, '·', 0.03 + band * 0.02);
                        if hash2(i, j) < 0.07 {
                            self.canvas.plot(i, j, '▪', 0.16);
                        }
                    } else {
                        let level = 0.06 + band * 0.12 + bass * 0.04;
                        let mark = if seed > 0.45 { '▒' } else { '░' };
                        self.canvas.plot(i, j, mark, level);
                        if seed > 0.30 && i.rem_euclid(3) == 0 && j.rem_euclid(2) == 0 {
                            self.canvas.plot(i, j, '▪', 0.15 + band * 0.25);
                        }
                    }
                }
            }
        }
        for k in 0..56u32 {
            let speed = 1.5 + hash01(k * 3 + 1) * 2.5 + energy * 4.0;
            if k % 2 == 0 {
                let road = ((hash01(k * 3 + 2) * (w / sx as usize).max(1) as f32) as i32) * sx;
                let lane = road + if k % 4 == 0 { 0 } else { 1 };
                let y = (hash01(k * 3 + 3) * fh + t * speed).rem_euclid(fh);
                self.canvas.dot(lane as f32, y, '●', 0.45 + energy * 0.35);
                self.canvas.dot(lane as f32, y + 1.0, '·', 0.22);
            } else {
                let road = ((hash01(k * 3 + 4) * (h / sy as usize).max(1) as f32) as i32) * sy;
                let lane = road + if k % 4 == 1 { 0 } else { 1 };
                let x = (hash01(k * 3 + 5) * fw + t * speed).rem_euclid(fw);
                self.canvas.dot(x, lane as f32, '●', 0.45 + energy * 0.35);
                self.canvas.dot(x + 1.0, lane as f32, '·', 0.22);
            }
        }
        for ix in (0..w as i32).step_by(sx as usize) {
            for iy in (0..h as i32).step_by(sy as usize) {
                self.canvas.plot(ix, iy, '┼', 0.22 + kick * 0.35);
                self.canvas.plot(ix + 2, iy, '·', 0.35 + treble * 0.25);
                self.canvas.plot(ix, iy + 2, '·', 0.35 + treble * 0.25);
            }
        }
    }

    /// A tower clock over the city, with hands and gear teeth.
    fn draw_clocktower(
        &mut self,
        w: usize,
        h: usize,
        t: f32,
        bands: &[f32; 32],
        audio: &Reactivity,
    ) {
        if self.skyline.is_empty() {
            self.skyline = build_skyline();
        }
        let fw = w as f32;
        let fh = h as f32;
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let kick = unit(audio.kick);
        self.night_sky(w, h, 0.60, t, treble);
        for building in &self.skyline {
            if building.layer < 2 {
                continue;
            }
            let x0 = (building.x * fw).round() as i32;
            let x1 = ((building.x + building.w) * fw).round() as i32;
            let y0 = (building.top * fh).round() as i32;
            for y in y0..h as i32 {
                for x in x0..=x1 {
                    self.canvas.plot(x, y, '▓', 0.065);
                }
            }
            for y in (y0 + 2..h as i32).step_by(3) {
                for x in (x0 + 1..x1).step_by(3) {
                    if hash2(x, y) < 0.2 {
                        self.canvas.plot(x, y, '▪', 0.18);
                    }
                }
            }
        }
        let cx = fw * 0.5;
        let cy = fh * 0.44;
        let radius = (fw.min(fh * 2.0) * 0.30 * (1.0 + bass * 0.06)).max(5.0);
        let second = -PI * 0.5 + unit(audio.locked_beat_phase()) * TAU + t * 0.6;
        let minute = -PI * 0.5 + t * 0.22;
        let hour = -PI * 0.5 + t * 0.018;
        for j in 0..h {
            let dy = (j as f32 - cy) * 2.0;
            for i in 0..w {
                let dx = i as f32 - cx;
                let d = dx.hypot(dy);
                if d > radius * 1.10 {
                    continue;
                }
                if (d - radius).abs() < 0.9 {
                    self.canvas.plot(i as i32, j as i32, '○', 0.45 + kick * 0.2);
                } else if d < radius * 0.96 {
                    self.canvas
                        .plot(i as i32, j as i32, '░', 0.09 + treble * 0.03);
                    let angle = dy.atan2(dx);
                    let tick = (angle / TAU * 12.0).round();
                    let frac = (angle / TAU * 12.0 - tick).abs();
                    let band = unit(bands[((tick as i32).rem_euclid(12)) as usize]);
                    if frac < 0.055 && d > radius * 0.80 {
                        self.canvas
                            .plot(i as i32, j as i32, '▪', 0.30 + band * 0.35);
                    }
                }
            }
        }
        for (angle, len, level) in [
            (hour, radius * 0.52, 0.85),
            (minute, radius * 0.74, 0.85),
            (second, radius * 0.88, 0.62),
        ] {
            let end = [cx + angle.cos() * len, cy + angle.sin() * len * 0.5];
            self.canvas.stroke([cx, cy], end, level);
            self.canvas.stroke(
                [cx + 0.7, cy + 0.35],
                [end[0] + 0.7, end[1] + 0.35],
                level * 0.55,
            );
            self.canvas.dot(end[0], end[1], '●', level);
        }
        self.canvas.dot(cx, cy, '◉', 0.55 + kick * 0.3);
        for k in 0..24 {
            let angle = k as f32 / 24.0 * TAU + t * 0.02;
            let x = cx + angle.cos() * radius * 1.10;
            let y = cy + angle.sin() * radius * 1.10 * 0.5;
            self.canvas.dot(x, y, '·', 0.12 + treble * 0.12);
        }
    }

    /// A neon alley: sign panels, wet floor reflections, and steam.
    fn draw_neon(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let energy = unit(audio.energy);
        let treble = unit(audio.treble);
        let kick = unit(audio.kick);
        let bucket = (t * 2.0) as u32;
        let left_wall = fw * 0.30;
        let right_wall = fw * 0.70;
        for j in 0..h {
            for i in 0..w {
                let x = i as f32;
                if x < left_wall || x > right_wall {
                    self.canvas.plot(i as i32, j as i32, '▓', 0.07);
                } else if (j as f32) < fh * 0.40 {
                    if hash2(i as i32, j as i32) < 0.01 + treble * 0.01 {
                        self.canvas.plot(i as i32, j as i32, '·', 0.18);
                    }
                } else {
                    self.canvas.plot(i as i32, j as i32, '·', 0.035);
                }
            }
        }
        for side in 0..2 {
            for row in 0..8 {
                for col in 0..2 {
                    let id = side * 100 + row * 10 + col;
                    let hue = hash01(id as u32 * 7 + 1);
                    let vertical = id % 3 == 0;
                    let sw = if vertical { 3 } else { 6 };
                    let sh = if vertical { 6 } else { 3 };
                    let x0 = if side == 0 {
                        2 + col * 9
                    } else {
                        w as i32 - 3 - col * 9 - sw
                    };
                    let y0 = 2 + row * 5;
                    if x0 < 0 || y0 < 0 || x0 + sw >= w as i32 || y0 + sh >= h as i32 {
                        continue;
                    }
                    let band = unit(bands[((row * 3 + col) as usize) % 32]);
                    let lit = hash01(id as u32 * 13 + bucket) < 0.45 + treble * 0.3 + band * 0.2;
                    let level = if lit {
                        0.25 + band * 0.40 + treble * 0.15
                    } else {
                        0.055
                    };
                    for x in x0..=x0 + sw {
                        self.canvas.plot_tinted(x, y0, '─', level + 0.10, hue);
                        self.canvas.plot_tinted(x, y0 + sh, '─', level + 0.10, hue);
                    }
                    for y in y0..=y0 + sh {
                        self.canvas.plot_tinted(x0, y, '│', level + 0.10, hue);
                        self.canvas.plot_tinted(x0 + sw, y, '│', level + 0.10, hue);
                    }
                    for y in (y0 + 1)..(y0 + sh) {
                        for x in (x0 + 1)..(x0 + sw) {
                            self.canvas
                                .plot_tinted(x, y, if lit { '▒' } else { '░' }, level, hue);
                        }
                    }
                    let cx = x0 as f32 + sw as f32 * 0.5;
                    if lit {
                        for y in (y0 + sh + 1)..h as i32 {
                            let wobble = (y as f32 * 0.5 + t * 2.2).sin() * 1.2;
                            if (y + id) % 2 == 0 {
                                self.canvas.dot_tinted(
                                    cx + wobble,
                                    y as f32,
                                    '·',
                                    0.10 + band * 0.14,
                                    hue,
                                );
                            }
                        }
                    }
                }
            }
        }
        for j in (fh * 0.62) as i32..h as i32 {
            for i in 0..w as i32 {
                self.canvas.plot(i, j, '·', 0.02);
                let n = noise2(i as f32 * 0.12, j as f32 * 0.2);
                if n > 0.62 {
                    self.canvas.plot(i, j, '░', 0.04 + n * 0.04);
                }
            }
        }
        for k in 0..40u32 {
            let base = hash01(k * 7 + 1) * fw;
            let speed = 0.06 + hash01(k * 7 + 2) * 0.10 + energy * 0.06;
            let p = (t * speed + hash01(k * 7 + 3)).fract();
            let y = fh * 0.78 - p * fh * 0.45;
            let x = base + (p * 9.0 + t * 0.8).sin() * 3.0;
            self.canvas.dot(x, y, '░', 0.07 + energy * 0.08);
        }
        let door_x = (w / 2) as i32;
        let door_y = (fh * 0.63) as i32;
        for x in door_x - 3..=door_x + 3 {
            self.canvas.plot_tinted(x, door_y, '─', 0.40, 0.10);
        }
        for y in door_y..=door_y + 3 {
            self.canvas.plot_tinted(door_x - 3, y, '│', 0.40, 0.10);
            self.canvas.plot_tinted(door_x + 3, y, '│', 0.40, 0.10);
        }
        for y in (door_y + 1)..=(door_y + 3) {
            for x in (door_x - 2)..=(door_x + 2) {
                self.canvas.plot_tinted(x, y, '░', 0.22 + kick * 0.20, 0.10);
            }
        }
        for side in [-1.0f32, 1.0] {
            let end_x = door_x as f32 + side * fw * 0.22;
            self.canvas.stroke(
                [door_x as f32 + side * 3.0, (door_y + 3) as f32],
                [end_x, fh - 1.0],
                0.06,
            );
        }
        for k in 0..8u32 {
            let cx = hash01(k * 11 + 2) * fw;
            let cy = (fh * 0.66) + hash01(k * 11 + 3) * (fh * 0.25);
            for dx in 0..6 {
                self.canvas
                    .dot(cx + (dx as f32 - 3.0) * 1.2, cy, '≈', 0.08 + kick * 0.18);
            }
        }
    }

    /// A dense multi-lane highway of light trails at rush hour.
    fn draw_rush(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let treble = unit(audio.treble);
        let energy = unit(audio.energy);
        let bass = unit(audio.bass);
        let kick = unit(audio.kick);
        let roads = 7usize;
        let bh = fh / roads as f32;
        for b in 0..roads {
            let y0 = b as f32 * bh;
            for j in (y0 as i32)..((y0 + bh) as i32).min(h as i32) {
                for i in 0..w as i32 {
                    self.canvas.plot(i, j, '·', 0.035);
                }
            }
            for i in 0..w as i32 {
                self.canvas.plot(i, y0 as i32, '─', 0.06);
            }
            for lane in 1..3 {
                let ly = (y0 + bh * lane as f32 / 3.0).round() as i32;
                for i in 0..w as i32 {
                    if (i + b as i32 * 4 + (t * 6.0) as i32).rem_euclid(11) < 3 {
                        self.canvas.plot(i, ly, '─', 0.10 + treble * 0.04);
                    }
                }
            }
            let band = unit(bands[(b * 5) % 32]);
            let count = 8 + (treble * 10.0) as usize + b;
            for k in 0..count {
                let lane = (k % 3) as f32;
                let ly = y0 + bh * (lane + 0.5) / 3.0;
                let dir = if (b + k) % 2 == 0 { 1.0 } else { -1.0 };
                let speed = dir * (0.20 + hash01((b * 31 + k) as u32 + 1) * 0.35 + energy * 0.55);
                let p = (t * speed + hash01((b * 31 + k) as u32 + 2)).rem_euclid(1.0);
                let x = p * fw;
                let len = 2.0 + hash01((b * 31 + k) as u32 + 3) * 4.0 + treble * 2.5;
                let level =
                    0.45 + band * 0.3 + bass * 0.2 + if k % 5 == 0 { kick * 0.3 } else { 0.0 };
                for d in 0..len as i32 {
                    let px = x as i32 + dir as i32 * d;
                    self.canvas.plot(px, ly.round() as i32, '━', level);
                }
                self.canvas.dot(x, ly, '●', (level + 0.35).min(1.0));
            }
        }
        for x in [0.28f32, 0.62] {
            let xi = (x * fw) as i32;
            for j in (0..h as i32).step_by(7) {
                for d in 0..3 {
                    self.canvas.plot(xi, j + d, '│', 0.10);
                    self.canvas.plot(xi + 1, j + d, '│', 0.06);
                }
                self.canvas.plot(xi, j, '─', 0.10);
                self.canvas.plot(xi + 1, j, '·', 0.30 + treble * 0.25);
            }
        }
        for k in 0..18u32 {
            let x = hash01(k * 13 + 5) * fw;
            let y = hash01(k * 13 + 9) * fh;
            self.canvas.dot(x, y, '·', 0.10 + kick * 0.18);
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
            tempo_confidence: 0.9,
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
}
