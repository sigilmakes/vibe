//! Circuits and signals: copper traces, logic, buses, antennas, and silicon.
//!
//! Every scene draws in cell space, so the whole window is the board. Geometry
//! is precomputed where it is static and evaluated per axis where it is not.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::TAU;

pub const NAMES: &[&str] = &[
    "Trace Lattice",
    "Gate Cascade",
    "Oscillator Bank",
    "Bus Ribbon",
    "Phased Array",
    "Clock Tree",
    "Solder Bloom",
    "Crossbar Route",
    "Logic Analyser",
    "Die Floorplan",
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

/// One frame's glyph and brightness field. Reused so the draw path allocates
/// only when the visible size changes.
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
        0 => 0.05 + u * 0.18 + v * 0.08,
        1 => 0.55 + u * 0.22,
        2 => 0.10 + v * 0.45,
        3 => 0.70 + u * 0.20,
        4 => 0.35 + v * 0.25 + u * 0.15,
        5 => 0.25 + v * 0.20,
        6 => 0.80 + u * 0.18,
        7 => 0.45 + u * 0.35,
        8 => 0.60 + u * 0.30,
        _ => 0.15 + u * 0.20 + v * 0.20,
    };
    (base + unit(audio.centroid) * 0.16 + unit(audio.locked_beat_phase()) * 0.05).rem_euclid(1.0)
}

struct Net {
    pts: Vec<[f32; 2]>,
    /// Pulse phase offset, so strands do not flash together.
    offset: f32,
}

fn build_nets() -> Vec<Net> {
    let mut nets = Vec::with_capacity(14);
    for i in 0..14 {
        let k = i as f32;
        let mut pts = Vec::with_capacity(4);
        if i < 9 {
            let y0 = 0.07 + 0.86 * (i as f32 + 0.5) / 9.0 + (hash01(i as u32 + 3) - 0.5) * 0.02;
            let y1 = 0.07
                + 0.86 * (((i * 4 + 3) % 9) as f32 + 0.5) / 9.0
                + (hash01(i as u32 + 11) - 0.5) * 0.02;
            let x1 = 0.08 + 0.42 * hash01(i as u32 + 23);
            let x2 = x1 + 0.13 + 0.10 * hash01(i as u32 + 37);
            pts.push([-0.03, y0]);
            pts.push([x1, y0]);
            pts.push([x2, y1]);
            pts.push([1.03, y1]);
        } else {
            let x0 = 0.08 + 0.84 * ((i - 9) as f32 + 0.5) / 5.0;
            let x1 = 0.08
                + 0.84 * ((((i - 9) * 2 + 1) % 5) as f32 + 0.5) / 5.0
                + (hash01(i as u32 + 7) - 0.5) * 0.02;
            let y1 = 0.12 + 0.42 * hash01(i as u32 + 29);
            let y2 = y1 + 0.06 + 0.06 * hash01(i as u32 + 41);
            pts.push([x0, -0.03]);
            pts.push([x0, y1]);
            pts.push([x1, y2]);
            pts.push([x1, 1.03]);
        }
        nets.push(Net {
            pts,
            offset: (k * 0.371).fract(),
        });
    }
    nets
}

#[derive(Clone, Copy)]
struct Branch {
    a: [f32; 2],
    b: [f32; 2],
    depth: u8,
}

fn build_tree() -> (Vec<Branch>, Vec<[f32; 2]>) {
    fn recurse(
        branches: &mut Vec<Branch>,
        leaves: &mut Vec<[f32; 2]>,
        x: f32,
        y: f32,
        size: f32,
        depth: u8,
    ) {
        let half = size * 0.5;
        branches.push(Branch {
            a: [x - half, y],
            b: [x + half, y],
            depth,
        });
        branches.push(Branch {
            a: [x - half, y - half],
            b: [x - half, y + half],
            depth,
        });
        branches.push(Branch {
            a: [x + half, y - half],
            b: [x + half, y + half],
            depth,
        });
        if depth < 4 {
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    recurse(
                        branches,
                        leaves,
                        x + sx * half,
                        y + sy * half,
                        half,
                        depth + 1,
                    );
                }
            }
        } else {
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    leaves.push([x + sx * half, y + sy * half]);
                }
            }
        }
    }
    let mut branches = Vec::with_capacity(1100);
    let mut leaves = Vec::with_capacity(1100);
    recurse(&mut branches, &mut leaves, 0.5, 0.5, 0.94, 0);
    (branches, leaves)
}

#[derive(Clone, Copy)]
struct Pad {
    p: [f32; 2],
    band: usize,
}

#[derive(Clone, Copy)]
struct Link {
    a: usize,
    b: usize,
    offset: f32,
}

fn build_pads() -> (Vec<Pad>, Vec<Link>) {
    const COLS: usize = 13;
    const ROWS: usize = 9;
    let mut pads = Vec::with_capacity(COLS * ROWS);
    for gy in 0..ROWS {
        for gx in 0..COLS {
            let jx = (hash2(gx as i32 + 5, gy as i32 + 71) - 0.5) * 0.55;
            let jy = (hash2(gx as i32 + 43, gy as i32 + 19) - 0.5) * 0.5;
            pads.push(Pad {
                p: [
                    ((gx as f32 + 0.5 + jx) / COLS as f32).clamp(0.02, 0.98),
                    ((gy as f32 + 0.5 + jy) / ROWS as f32).clamp(0.03, 0.97),
                ],
                band: (gx * 5 + gy * 3) % 32,
            });
        }
    }
    let mut links = Vec::new();
    for gy in 0..ROWS {
        for gx in 0..COLS {
            let here = gy * COLS + gx;
            if gx + 1 < COLS && hash2(gx as i32 + 9, gy as i32 + 2) < 0.82 {
                links.push(Link {
                    a: here,
                    b: here + 1,
                    offset: hash2(gx as i32, gy as i32),
                });
            }
            if gy + 1 < ROWS && hash2(gx as i32 + 21, gy as i32 + 33) < 0.7 {
                links.push(Link {
                    a: here,
                    b: here + COLS,
                    offset: hash2(gx as i32 + 8, gy as i32 + 4),
                });
            }
        }
    }
    (pads, links)
}

#[derive(Default)]
pub struct Renderer {
    canvas: Canvas,
    nets: Vec<Net>,
    tree: Vec<Branch>,
    leaves: Vec<[f32; 2]>,
    pads: Vec<Pad>,
    links: Vec<Link>,
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
            0 => self.draw_trace(w, h, now, bands, audio),
            1 => self.draw_gates(w, h, now, bands, audio),
            2 => self.draw_oscillators(w, h, now, bands, audio),
            3 => self.draw_buses(w, h, now, bands, audio),
            4 => self.draw_array(w, h, now, bands, audio),
            5 => self.draw_clock(w, h, now, bands, audio),
            6 => self.draw_solder(w, h, now, bands, audio),
            7 => self.draw_crossbar(w, h, now, bands, audio),
            8 => self.draw_analyser(w, h, now, bands, audio),
            _ => self.draw_floorplan(w, h, now, bands, audio),
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

    /// Copper strands with routed elbows, vias, and travelling signal sparks.
    fn draw_trace(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.nets.is_empty() {
            self.nets = build_nets();
        }
        let fw = w as f32;
        let fh = h as f32;
        let speed = 0.09 + unit(audio.energy) * 0.18 + unit(audio.bass) * 0.12;
        let pad_glow = 0.25 + unit(audio.treble) * 0.6;
        let loud = 0.55 + unit(audio.kick) * 0.3;
        for (i, net) in self.nets.iter().enumerate() {
            let pts: Vec<[f32; 2]> = net.pts.iter().map(|p| [p[0] * fw, p[1] * fh]).collect();
            let band = unit(bands[i % 32]);
            let base = 0.16 + band * 0.14 + unit(audio.mid) * 0.05;
            for pair in pts.windows(2) {
                self.canvas.stroke(pair[0], pair[1], base);
            }
            for point in &pts {
                self.canvas.dot(
                    point[0],
                    point[1],
                    if pad_glow > 0.5 { '◉' } else { '●' },
                    base + pad_glow * 0.35,
                );
            }
            let phase = (t * speed + net.offset).rem_euclid(1.0);
            for pulse in [phase, (phase + 0.47).rem_euclid(1.0)] {
                for tail in 0..4 {
                    let s = (pulse - tail as f32 * 0.01).rem_euclid(1.0);
                    let p = point_along(&pts, s);
                    let level = (1.0 - tail as f32 * 0.18) * loud;
                    self.canvas
                        .dot(p[0], p[1], if tail == 0 { '●' } else { '·' }, level);
                }
            }
        }
        let texture = 0.05 + unit(audio.transients[0]) * 0.12;
        for y in (1..h as i32).step_by(4) {
            for x in (1..w as i32).step_by(8) {
                if hash2(x, y) < 0.5 {
                    continue;
                }
                self.canvas.plot(x, y, '·', texture);
            }
        }
    }

    /// A schematic of gates on a matrix, wired column to column under a clock spine.
    fn draw_gates(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let layers = (w / 16).clamp(3, 10);
        let rows = (h / 7).clamp(2, 8);
        let board = 0.04 + unit(audio.energy) * 0.05;
        for y in (1..h as i32).step_by(3) {
            for x in (1..w as i32).step_by(4) {
                if hash2(x, y) < 0.55 {
                    continue;
                }
                self.canvas.plot(x, y, '·', board);
            }
        }
        let mut gates = Vec::with_capacity(layers * rows);
        for l in 0..layers {
            let x = ((l as f32 + 0.5) * w as f32 / layers as f32).round() as i32;
            for r in 0..rows {
                let y = ((r as f32 + 0.5) * h as f32 / rows as f32).round() as i32;
                gates.push((x, y, l, r));
            }
        }
        for &(x, y, l, r) in &gates {
            let index = l * rows + r;
            let signal = gate_signal(index, bands, audio, t);
            if l == 0 {
                self.canvas
                    .stroke([0.0, y as f32], [(x - 2) as f32, y as f32], 0.16);
            } else {
                let source = (r + l % 2) % rows;
                let sx = gates[(l - 1) * rows + source].0;
                let sy = gates[(l - 1) * rows + source].1;
                let wire = 0.15 + gate_signal((l - 1) * rows + source, bands, audio, t) * 0.45;
                let mid = (sx + x) as f32 * 0.5;
                self.canvas
                    .stroke([(sx + 2) as f32, sy as f32], [mid, sy as f32], wire);
                self.canvas.stroke([mid, sy as f32], [mid, y as f32], wire);
                self.canvas
                    .stroke([mid, y as f32], [(x - 2) as f32, y as f32], wire);
                let dot_s = (t * (0.4 + signal * 0.8) + index as f32 * 0.17).rem_euclid(1.0);
                self.canvas.dot(
                    mid + ((x - 2) as f32 - mid) * dot_s,
                    y as f32,
                    '·',
                    0.3 + signal * 0.55,
                );
            }
            if l + 1 == layers {
                self.canvas.stroke(
                    [(x + 2) as f32, y as f32],
                    [w.saturating_sub(1) as f32, y as f32],
                    0.16 + signal * 0.35,
                );
            }
            let level = 0.32 + signal * 0.5;
            for dx in -2..=2 {
                self.canvas.plot(x + dx, y - 1, '─', level);
                self.canvas.plot(x + dx, y + 1, '─', level);
            }
            for dy in -1..=1 {
                self.canvas.plot(x - 2, y + dy, '│', level);
                self.canvas.plot(x + 2, y + dy, '│', level);
            }
            self.canvas.plot(x - 2, y - 1, '┌', level);
            self.canvas.plot(x + 2, y - 1, '┐', level);
            self.canvas.plot(x - 2, y + 1, '└', level);
            self.canvas.plot(x + 2, y + 1, '┘', level);
            let symbol = ['&', '≥', '=', '¬', '⊻', '⊕'][index % 6];
            self.canvas.plot(x, y, symbol, 0.45 + signal * 0.45);
            if signal > 0.55 {
                self.canvas.plot(x + 3, y, '●', 0.7 + signal * 0.3);
            }
        }
        // Clock distribution spine and taps.
        let spine_x = ((0.04 + 0.92 * unit(audio.locked_beat_phase())) * w as f32).round() as i32;
        let spine = 0.25 + unit(audio.beat) * 0.5;
        for y in 0..h as i32 {
            self.canvas.plot(spine_x, y, '┃', spine);
        }
        for r in 0..rows {
            let y = ((r as f32 + 0.5) * h as f32 / rows as f32).round() as i32;
            self.canvas.stroke(
                [spine_x as f32, y as f32],
                [spine_x as f32 + 4.0, y as f32],
                spine * 0.8,
            );
        }
    }

    /// Stacked oscilloscope lanes: each lane is one voice with its own waveform.
    fn draw_oscillators(
        &mut self,
        w: usize,
        h: usize,
        t: f32,
        bands: &[f32; 32],
        audio: &Reactivity,
    ) {
        let lanes = (h / 4).clamp(3, 12);
        let lane_h = h as f32 / lanes as f32;
        let wave_scale = 0.8 + unit(audio.bass) * 0.9;
        for lane in 0..lanes {
            let center = (lane as f32 + 0.5) * lane_h;
            let band = unit(bands[(lane * 3) % 32]);
            let amp = lane_h * 0.22 * (0.55 + unit(audio.bass) * 0.7 + band * 0.9) + 0.4;
            let freq = 0.8 + band * 2.4 + unit(audio.chroma[lane % 12]) * 1.2;
            let speed = 0.5 + unit(audio.mid) * 1.2;
            let phase = lane as f32 * 0.9 + t * speed;
            for x in (0..w as i32).step_by(3) {
                self.canvas.plot(x, center.round() as i32, '·', 0.07);
            }
            let mut prev_y = center;
            for x in 0..w {
                let u = x as f32 / w.max(1) as f32;
                let wave = audio.waveform[(x * 31 / w.max(1)).min(31)];
                let y = center
                    + amp
                        * ((u * TAU * freq + phase).sin() * 0.82
                            + (u * TAU * freq * 2.03 + phase * 1.7).sin() * 0.18)
                    + wave * wave_scale;
                let glyph = if x == 0 {
                    '·'
                } else {
                    line_glyph(1.0, y - prev_y)
                };
                self.canvas.plot(
                    x as i32,
                    y.round() as i32,
                    glyph,
                    0.48 + band * 0.3 + unit(audio.energy) * 0.1,
                );
                prev_y = y;
            }
        }
    }

    /// Parallel data buses with bit traffic and vertical taps between layers.
    fn draw_buses(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let buses = 6;
        let lanes = 3;
        let span = (h as f32 - 3.0).max(0.0);
        let bus_y = |b: usize| 1.0 + b as f32 * span / (buses - 1) as f32;
        let bit_speed = 1.8 + unit(audio.treble) * 6.0;
        let threshold = 0.25 + unit(audio.flatness) * 0.25;
        for b in 0..buses {
            let base = bus_y(b);
            let band = unit(bands[(b * 5) % 32]);
            for lane in 0..lanes {
                let y = base + (lane as f32 - 1.0);
                for x in 0..w {
                    let bit =
                        ((x as f32 * 0.35 + lane as f32 * 2.3 + t * bit_speed + b as f32 * 1.7)
                            .sin()
                            + band * 0.9)
                            > threshold;
                    let (glyph, level) = if bit {
                        ('━', 0.32 + band * 0.45)
                    } else {
                        ('─', 0.10 + band * 0.08)
                    };
                    self.canvas.plot(x as i32, y.round() as i32, glyph, level);
                }
            }
            let speed = 0.25 + unit(audio.bass) * 0.5 + unit(audio.energy) * 0.3;
            let px = (t * speed + b as f32 * 0.19).rem_euclid(1.0) * (w as f32 - 1.0);
            for lane in 0..lanes {
                self.canvas.dot(px, base + (lane as f32 - 1.0), '●', 0.95);
            }
            self.canvas.dot(1.0, base, '◉', 0.6 + band * 0.3);
            self.canvas.dot(w as f32 - 2.0, base, '◉', 0.6 + band * 0.3);
        }
        for b in 0..buses - 1 {
            let x = ((b as f32 + 0.5) * w as f32 / (buses - 1) as f32)
                .round()
                .clamp(1.0, (w as f32 - 2.0).max(1.0));
            let y0 = bus_y(b);
            let y1 = bus_y(b + 1);
            let level = 0.22 + unit(bands[(b * 11 + 3) % 32]) * 0.3;
            self.canvas.stroke([x, y0], [x, y1], level);
            self.canvas
                .plot(x as i32, y0.round() as i32, '┬', level + 0.08);
            self.canvas
                .plot(x as i32, y1.round() as i32, '┴', level + 0.08);
            let pulse = (t * 0.6 + b as f32 * 0.31 + unit(audio.onset) * 0.3).rem_euclid(1.0);
            self.canvas.dot(x, y0 + (y1 - y0) * pulse, '●', 0.75);
        }
    }

    /// A phased emitter row: interference bands radiate across the whole panel.
    fn draw_array(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let fw = w as f32;
        let fh = h as f32;
        let elements: i32 = 5;
        let mid_y = fh * 0.5;
        let steer = (unit(audio.centroid) - 0.5) * 2.2
            + (unit(audio.chroma[0]) - unit(audio.chroma[6])) * 0.5;
        let period = 19.0 - unit(audio.flatness) * 3.0;
        let speed = 1.1 + unit(audio.energy) * 1.6;
        let amp = 0.55 + unit(audio.bass) * 0.45;
        let spacing = fw * (0.14 + unit(bands[8]) * 0.04);
        for j in 0..h {
            let dy = (j as f32 + 0.5 - mid_y) * 2.0;
            for i in 0..w {
                let dx = i as f32 + 0.5 - fw * 0.5;
                let mut field = 0.0;
                for k in 0..elements {
                    let ex = (k as f32 - (elements as f32 - 1.0) * 0.5) * spacing;
                    let r = (dx - ex).hypot(dy);
                    field += (r / period * TAU - t * speed - k as f32 * steer).cos()
                        / (1.0 + r / period * 0.45);
                }
                let level = ((field.abs() - 0.45) * 1.3 * amp).clamp(0.0, 1.0);
                if level > 0.02 {
                    let mark = if level > 0.55 { '▒' } else { '░' };
                    self.canvas.plot(i as i32, j as i32, mark, level);
                }
            }
        }
        for k in 0..elements {
            let x = fw * 0.5 + (k as f32 - (elements as f32 - 1.0) * 0.5) * spacing;
            self.canvas.dot(x, mid_y, '◆', 0.85);
            for d in 1..=2 {
                self.canvas.dot(x, mid_y - d as f32, '│', 0.28);
                self.canvas.dot(x, mid_y + d as f32, '│', 0.28);
            }
        }
        for i in 0..w {
            self.canvas.plot(i as i32, mid_y.round() as i32, '─', 0.10);
        }
    }

    /// An H-tree clock distribution network with a pulse front running to the leaves.
    fn draw_clock(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.tree.is_empty() {
            let (tree, leaves) = build_tree();
            self.tree = tree;
            self.leaves = leaves;
        }
        let fw = w as f32;
        let fh = h as f32;
        let pulse = (t * 1.3 + unit(audio.locked_beat_phase()) * 1.6 + unit(audio.bass) * 1.1)
            .rem_euclid(6.2);
        for b in &self.tree {
            let d = b.depth as f32;
            let ring = (-((d + 0.5 - pulse) * (d + 0.5 - pulse)) * 1.8).exp();
            let base = 0.16 + 0.04 * (4.0 - d.min(4.0));
            self.canvas.stroke(
                [b.a[0] * fw, b.a[1] * fh],
                [b.b[0] * fw, b.b[1] * fh],
                base + ring * 0.8,
            );
        }
        let leaf_level = 0.18 + unit(audio.treble) * 0.5 + unit(audio.onset) * 0.3;
        let leaf_ring = (-((4.4 - pulse) * (4.4 - pulse)) * 1.8).exp();
        for leaf in &self.leaves {
            self.canvas.dot(
                leaf[0] * fw,
                leaf[1] * fh,
                '▪',
                leaf_level + leaf_ring * 0.6,
            );
        }
        for j in (1..h as i32).step_by(3) {
            for i in (1..w as i32).step_by(3) {
                if hash2(i, j) < 0.65 {
                    continue;
                }
                let level = 0.05 + unit(bands[(i as usize * 3 + j as usize * 5) % 32]) * 0.08;
                self.canvas.plot(i, j, '·', level);
            }
        }
    }

    /// Pad field with solder joints and links carrying packets.
    fn draw_solder(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        if self.pads.is_empty() {
            let (pads, links) = build_pads();
            self.pads = pads;
            self.links = links;
        }
        let fw = w as f32;
        let fh = h as f32;
        let speed = 0.3 + unit(audio.bass) * 0.5 + unit(audio.energy) * 0.3;
        for link in &self.links {
            let pa = [self.pads[link.a].p[0] * fw, self.pads[link.a].p[1] * fh];
            let pb = [self.pads[link.b].p[0] * fw, self.pads[link.b].p[1] * fh];
            let band = unit(bands[self.pads[link.b].band]);
            self.canvas
                .stroke(pa, pb, 0.09 + band * 0.16 + unit(audio.flatness) * 0.05);
            let s = (t * speed + link.offset).rem_euclid(1.0);
            let p = [pa[0] + (pb[0] - pa[0]) * s, pa[1] + (pb[1] - pa[1]) * s];
            self.canvas
                .dot(p[0], p[1], '·', 0.4 + band * 0.3 + unit(audio.onset) * 0.3);
        }
        let kick = unit(audio.kick);
        for pad in &self.pads {
            let band = unit(bands[pad.band]);
            let x = pad.p[0] * fw;
            let y = pad.p[1] * fh;
            let mark = if kick > 0.55 && band > 0.3 {
                '◉'
            } else if band > 0.35 {
                '●'
            } else {
                '◍'
            };
            self.canvas.dot(x, y, mark, 0.35 + band * 0.55 + kick * 0.1);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                self.canvas.dot(x + dx, y + dy, '·', 0.12 + band * 0.18);
            }
            if kick > 0.3 {
                for (dx, dy) in [(-2.0, -1.0), (2.0, -1.0), (-2.0, 1.0), (2.0, 1.0)] {
                    self.canvas.dot(x + dx, y + dy, '∙', 0.3 + kick * 0.2);
                }
            }
        }
    }

    /// A routing mesh with a live path threading through crosspoint switches.
    fn draw_crossbar(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let cols = (w / 6).clamp(1, 40);
        let rows = (h / 3).clamp(1, 20);
        let xs: Vec<i32> = (0..cols)
            .map(|c| {
                (((c as f32 + 0.5) * w as f32 / cols as f32).round() as i32).clamp(0, w as i32 - 1)
            })
            .collect();
        let ys: Vec<i32> = (0..rows)
            .map(|r| {
                (((r as f32 + 0.5) * h as f32 / rows as f32).round() as i32).clamp(0, h as i32 - 1)
            })
            .collect();
        let dim = 0.06 + unit(audio.mid) * 0.05;
        for &x in &xs {
            for y in 0..h as i32 {
                self.canvas.plot(x, y, '│', dim);
            }
        }
        for &y in &ys {
            for x in 0..w as i32 {
                self.canvas.plot(x, y, '─', dim);
            }
        }
        let sparkle = unit(audio.treble);
        for &x in &xs {
            for &y in &ys {
                let switch = hash2(x, y) < 0.24;
                let level = (0.10 + sparkle * 0.32) * if switch { 1.7 } else { 0.6 };
                self.canvas
                    .plot(x, y, if switch { '×' } else { '┼' }, level);
            }
        }
        let route_level = 0.7 + unit(audio.onset) * 0.25;
        let mut row = ((hash01(7) * rows as f32) as usize).min(rows - 1);
        let mut prev_x = 0.0f32;
        let mut route: Vec<[f32; 2]> = Vec::with_capacity(cols * 2 + 2);
        route.push([0.0, ys[row] as f32]);
        let drift_speed = 0.5 + unit(audio.mid) * 0.5;
        for (c, &x) in xs.iter().enumerate() {
            let xf = x as f32;
            self.canvas
                .stroke([prev_x, ys[row] as f32], [xf, ys[row] as f32], route_level);
            route.push([xf, ys[row] as f32]);
            let drift = ((t * drift_speed + c as f32 * 0.9 + unit(audio.centroid) * 4.0).sin()
                * 2.4
                + unit(bands[(c * 5) % 32]) * 3.0)
                .round() as i32;
            let next = (row as i32 + drift).rem_euclid(rows as i32) as usize;
            self.canvas
                .stroke([xf, ys[row] as f32], [xf, ys[next] as f32], route_level);
            route.push([xf, ys[next] as f32]);
            let corner = if next > row {
                '└'
            } else if next < row {
                '┌'
            } else {
                '─'
            };
            self.canvas.plot(x, ys[next], corner, route_level + 0.05);
            row = next;
            prev_x = xf;
        }
        let right = w.saturating_sub(1) as f32;
        self.canvas.stroke(
            [prev_x, ys[row] as f32],
            [right, ys[row] as f32],
            route_level,
        );
        route.push([right, ys[row] as f32]);
        let s = (t * (0.18 + unit(audio.bass) * 0.2) + unit(audio.locked_beat_phase()) * 0.5)
            .rem_euclid(1.0);
        for tail in 0..4 {
            let p = point_along(&route, (s - tail as f32 * 0.012).rem_euclid(1.0));
            self.canvas.dot(
                p[0],
                p[1],
                if tail == 0 { '●' } else { '·' },
                (0.95 - tail as f32 * 0.2).max(0.2),
            );
        }
    }

    /// Digital channels with hard edges, timing ticks, and a live bit readout.
    fn draw_analyser(&mut self, w: usize, h: usize, t: f32, bands: &[f32; 32], audio: &Reactivity) {
        let rows = ((h as i32 - 1) / 3).clamp(1, 20) as usize;
        let speed = 0.6 + unit(audio.mid) * 1.6;
        let jitter = unit(audio.onset);
        for r in 0..rows {
            let base = 1 + r as i32 * 3;
            if base >= h as i32 {
                break;
            }
            let band = unit(bands[(r * 5) % 32]);
            let freq = 0.05 + band * 0.35 + unit(audio.chroma[r % 12]) * 0.15;
            let phase = r as f32 * 1.3 + t * speed * (0.6 + band * 1.4);
            let mut prev: Option<bool> = None;
            for x in 0..w as i32 {
                let wave = audio.waveform[(x as usize * 31 / w.max(1)).min(31)];
                let mut v = (x as f32 * freq * TAU + phase).sin() + wave * 0.55;
                if jitter > 0.35 {
                    v += (x as f32 * 1.7 + t * 9.0 + r as f32).sin() * jitter * 0.9;
                }
                let bit = v > 0.0;
                let y = if bit { base - 1 } else { base };
                match prev {
                    None => {
                        self.canvas.plot(x, y, '─', 0.5);
                    }
                    Some(previous) => {
                        let glyph = if previous == bit { '─' } else { '│' };
                        let level = if previous == bit {
                            0.4 + band * 0.2
                        } else {
                            0.85
                        };
                        self.canvas.plot(x, y, glyph, level);
                        if previous != bit {
                            self.canvas
                                .plot(x, if previous { base } else { base - 1 }, '│', 0.85);
                        }
                    }
                }
                prev = Some(bit);
            }
            let live = (t * speed + r as f32).sin() > 0.0;
            let (mark, level) = if live { ('1', 0.9) } else { ('0', 0.25) };
            self.canvas.plot(w as i32 - 1, base, mark, level);
            self.canvas.plot(w as i32 - 2, base - 1, mark, level * 0.8);
        }
        for x in (2..w as i32).step_by(10) {
            for y in 0..h as i32 {
                self.canvas.plot(x, y, '·', 0.05);
            }
        }
    }

    /// A silicon die: weighted functional blocks, hatched, with buses and a hot block.
    fn draw_floorplan(
        &mut self,
        w: usize,
        h: usize,
        t: f32,
        bands: &[f32; 32],
        audio: &Reactivity,
    ) {
        let cols = (w / 18).clamp(2, 6);
        let rows = (h / 8).clamp(2, 5);
        let fw = w as f32;
        let fh = h as f32;
        let weights: Vec<f32> = (0..cols)
            .map(|c| 1.0 + unit(bands[(c * 7 + 1) % 32]) * 1.5 + unit(audio.energy) * 0.5)
            .collect();
        let mut xs: Vec<i32> = Vec::with_capacity(cols + 1);
        xs.push(0);
        let total: f32 = weights.iter().sum();
        let mut acc = 0.0;
        for &weight in weights.iter().take(cols - 1) {
            acc += weight;
            let previous = *xs.last().unwrap_or(&0);
            let x = (acc / total * fw).round() as i32;
            xs.push(x.clamp(previous, (w as i32 - 1).max(0)));
        }
        xs.push(w as i32 - 1);
        let row_weights: Vec<f32> = (0..rows)
            .map(|r| 1.0 + unit(bands[(r * 11 + 5) % 32]) * 1.4 + unit(audio.energy) * 0.4)
            .collect();
        let mut ys: Vec<i32> = Vec::with_capacity(rows + 1);
        ys.push(0);
        let row_total: f32 = row_weights.iter().sum();
        let mut racc = 0.0;
        for &weight in row_weights.iter().take(rows - 1) {
            racc += weight;
            let previous = *ys.last().unwrap_or(&0);
            let y = (racc / row_total * fh).round() as i32;
            ys.push(y.clamp(previous, (h as i32 - 1).max(0)));
        }
        ys.push(h as i32 - 1);
        for by in 0..rows {
            for bx in 0..cols {
                let x0 = xs[bx];
                let x1 = xs[bx + 1];
                let y0 = ys[by];
                let y1 = ys[by + 1];
                if x1 < x0 || y1 < y0 {
                    continue;
                }
                let value = unit(bands[(bx * 5 + by * 11) % 32] * 0.7 + audio.energy * 0.3);
                let edge = 0.22 + value * 0.25;
                for x in x0..=x1 {
                    self.canvas.plot(x, y0, '─', edge);
                    self.canvas.plot(x, y1, '─', edge);
                }
                for y in y0..=y1 {
                    self.canvas.plot(x0, y, '│', edge);
                    self.canvas.plot(x1, y, '│', edge);
                }
                self.canvas.plot(x0, y0, '┌', edge + 0.1);
                self.canvas.plot(x1, y0, '┐', edge + 0.1);
                self.canvas.plot(x0, y1, '└', edge + 0.1);
                self.canvas.plot(x1, y1, '┘', edge + 0.1);
                let stride = (1.0 + (1.0 - value) * 3.0).round().max(1.0) as i32;
                let glyph = ['·', '░', '▒', '╱'][(bx * 7 + by * 13) % 4];
                for y in y0 + 1..y1 {
                    for x in x0 + 1..x1 {
                        if (x + y * 3 + bx as i32) % stride != 0 {
                            continue;
                        }
                        self.canvas.plot(x, y, glyph, 0.08 + value * 0.3);
                    }
                }
                let span = (x1 - x0).max(1) as f32;
                let p = (t * 0.4 + bx as f32 * 0.21 + by as f32 * 0.37).rem_euclid(1.0);
                self.canvas
                    .dot(x0 as f32 + span * p, y1 as f32, '●', 0.35 + value * 0.35);
                let vspan = (y1 - y0).max(1) as f32;
                let q = (t * 0.33 + bx as f32 * 0.31 + by as f32 * 0.11).rem_euclid(1.0);
                self.canvas
                    .dot(x1 as f32, y0 as f32 + vspan * q, '●', 0.3 + value * 0.35);
            }
        }
        let kick = unit(audio.kick);
        if kick > 0.25 {
            let count = cols * rows;
            let index = ((t.abs() * 0.8) as usize) % count;
            let bx = index % cols;
            let by = index / cols;
            let x0 = xs[bx];
            let x1 = xs[bx + 1];
            let y0 = ys[by];
            let y1 = ys[by + 1];
            for y in y0 + 1..y1 {
                for x in x0 + 1..x1 {
                    self.canvas.plot(x, y, '▓', 0.65 + kick * 0.2);
                }
            }
        }
    }
}

fn gate_signal(index: usize, bands: &[f32; 32], audio: &Reactivity, t: f32) -> f32 {
    let band = unit(bands[(index * 7 + 3) % 32]);
    let osc = 0.5 + 0.5 * (t * (0.8 + (index % 7) as f32 * 0.17) + index as f32 * 1.7).sin();
    unit(band * 0.62 + osc * 0.30 + unit(audio.beat) * 0.25 + unit(audio.transients[1]) * 0.15)
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
        for mode in 0..NAMES.len() {
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
        for mode in 0..NAMES.len() {
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
        for mode in 0..NAMES.len() {
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
