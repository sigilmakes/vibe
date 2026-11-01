//! Bounded organic simulations. Positions use a unit square; terminal aspect is
//! corrected at rasterization rather than in the simulation.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Coral Branches",
    "Mycelium",
    "Root Network",
    "Vine Waltz",
    "Barnsley Fern",
    "Phyllotaxis Bloom",
    "Wind Branches",
    "Lava Cells",
    "Ink Diffusion",
    "Elastic Membrane",
];
const GRID_W: usize = 80;
const GRID_H: usize = 48;
const MAX_SEGMENTS: usize = 1800;

#[derive(Clone, Copy)]
struct Segment {
    a: [f32; 2],
    b: [f32; 2],
    depth: usize,
}
#[derive(Clone, Copy)]
struct Tip {
    p: [f32; 2],
    angle: f32,
    depth: usize,
    parent: usize,
}
#[derive(Clone, Copy)]
struct Blob {
    p: [f32; 2],
    v: [f32; 2],
    radius: f32,
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    style: Option<usize>,
    age: f32,
    clock: f32,
    credit: f32,
    rng: u32,
    segments: Vec<Segment>,
    tips: Vec<Tip>,
    points: Vec<[f32; 2]>,
    fern: [f32; 2],
    blobs: Vec<Blob>,
    field: Vec<f32>,
    velocity: Vec<f32>,
    next: Vec<f32>,
    attack: f32,
}
fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    (*seed >> 8) as f32 / 16777216.0
}
impl Renderer {
    fn reset(&mut self, style: usize) {
        self.style = Some(style);
        self.age = 0.0;
        self.credit = 0.0;
        self.rng = 7321 + style as u32 * 971;
        self.segments.clear();
        self.tips.clear();
        self.points.clear();
        self.fern = [0.0; 2];
        self.blobs.clear();
        if matches!(style, 0 | 1 | 2 | 6) {
            let roots = if style == 1 { 3 } else { 1 };
            for i in 0..roots {
                let p = if style == 1 {
                    [0.25 + i as f32 * 0.25, 0.5]
                } else if style == 2 {
                    [0.5, 0.12]
                } else {
                    [0.5, 0.94]
                };
                self.tips.push(Tip {
                    p,
                    angle: if style == 2 { PI * 0.5 } else { -PI * 0.5 },
                    depth: 0,
                    parent: usize::MAX,
                });
            }
        }
        if style == 7 {
            for i in 0..9 {
                self.blobs.push(Blob {
                    p: [0.15 + random(&mut self.rng) * 0.7, random(&mut self.rng)],
                    v: [
                        (random(&mut self.rng) - 0.5) * 0.04,
                        (if i % 2 == 0 { 1.0 } else { -1.0 })
                            * (0.025 + random(&mut self.rng) * 0.025),
                    ],
                    radius: 0.07 + random(&mut self.rng) * 0.055,
                });
            }
        }
        if style >= 8 {
            self.field = vec![0.0; GRID_W * GRID_H];
            self.velocity = vec![0.0; GRID_W * GRID_H];
            self.next = vec![0.0; GRID_W * GRID_H];
            self.inject(0.5, 0.5, 0.8, style == 9);
        }
    }
    fn inject(&mut self, x: f32, y: f32, strength: f32, membrane: bool) {
        let cx = (x * (GRID_W - 1) as f32) as i32;
        let cy = (y * (GRID_H - 1) as f32) as i32;
        for dy in -4..=4 {
            for dx in -6..=6 {
                let xx = cx + dx;
                let yy = cy + dy;
                if xx <= 0 || yy <= 0 || xx >= GRID_W as i32 - 1 || yy >= GRID_H as i32 - 1 {
                    continue;
                }
                let value = strength * (-(dx * dx) as f32 * 0.12 - (dy * dy) as f32 * 0.22).exp();
                let i = yy as usize * GRID_W + xx as usize;
                if membrane {
                    self.velocity[i] = (self.velocity[i] + value * 7.0).clamp(-12.0, 12.0);
                } else {
                    self.field[i] = (self.field[i] + value).min(1.0);
                }
            }
        }
    }
    fn grow(&mut self, style: usize, audio: &Reactivity, dt: f32) {
        self.credit +=
            dt * match style {
                1 => 48.0,
                6 => 18.0,
                _ => 28.0,
            } * (0.45 + unit(audio.energy) * 1.8 + unit(audio.mid));
        let steps = (self.credit as usize).min(64);
        self.credit -= steps as f32;
        for _ in 0..steps {
            if self.tips.is_empty() || self.segments.len() >= MAX_SEGMENTS {
                break;
            }
            let index = (random(&mut self.rng) * self.tips.len() as f32) as usize;
            let mut t = self.tips.swap_remove(index.min(self.tips.len() - 1));
            let noise = random(&mut self.rng) - 0.5;
            let length = if style == 6 {
                0.12 * 0.74_f32.powi(t.depth as i32)
            } else {
                0.017
            };
            t.angle += noise * if style == 1 { 1.0 } else { 0.35 };
            // Roots favor gravity; coral favors the light. Tree daughters diverge.
            if style == 2 {
                t.angle += (PI * 0.5 - t.angle) * 0.07;
            }
            if style == 0 {
                t.angle += (-PI * 0.5 - t.angle) * 0.035;
            }
            let b = [
                t.p[0] + t.angle.cos() * length,
                t.p[1] + t.angle.sin() * length,
            ];
            if !(0.04..0.96).contains(&b[0]) || !(0.04..0.96).contains(&b[1]) {
                continue;
            }
            if style == 1
                && self
                    .segments
                    .iter()
                    .enumerate()
                    .any(|(i, s)| i != t.parent && (s.b[0] - b[0]).hypot(s.b[1] - b[1]) < 0.012)
            {
                // An approaching hypha joins an existing strand, ending this tip.
                if let Some(s) = self
                    .segments
                    .iter()
                    .find(|s| (s.b[0] - b[0]).hypot(s.b[1] - b[1]) < 0.025)
                {
                    self.segments.push(Segment {
                        a: t.p,
                        b: s.b,
                        depth: t.depth,
                    });
                }
                continue;
            }
            let parent = self.segments.len();
            self.segments.push(Segment {
                a: t.p,
                b,
                depth: t.depth,
            });
            if style == 6 {
                if t.depth < 8 {
                    for side in [-1.0, 1.0] {
                        self.tips.push(Tip {
                            p: b,
                            angle: t.angle + side * (0.36 + unit(audio.mid) * 0.35),
                            depth: t.depth + 1,
                            parent,
                        });
                    }
                }
            } else {
                let split = random(&mut self.rng)
                    < (if style == 1 { 0.14 } else { 0.12 }) + unit(audio.onset) * 0.15;
                if split && self.tips.len() < 90 && t.depth < 10 {
                    let side = if random(&mut self.rng) < 0.5 {
                        -1.0
                    } else {
                        1.0
                    };
                    self.tips.push(Tip {
                        p: b,
                        angle: t.angle + side * (0.6 + unit(audio.treble) * 0.5),
                        depth: t.depth + 1,
                        parent,
                    });
                }
                t.p = b;
                t.parent = parent;
                self.tips.push(t);
            }
        }
    }
    fn advance(&mut self, style: usize, dt: f32, audio: &Reactivity, bands: &[f32; 32]) {
        self.age += dt;
        self.clock = (self.clock + dt * (0.4 + unit(audio.mid) * 0.65)).rem_euclid(TAU * 100.0);
        if self.age
            > match style {
                1 => 30.0,
                0 | 2 | 6 => 26.0,
                4 => 22.0,
                5 => 28.0,
                _ => f32::INFINITY,
            }
        {
            self.reset(style);
        }
        match style {
            0 | 1 | 2 | 6 => self.grow(style, audio, dt),
            4 => {
                self.credit += dt * (360.0 + unit(audio.energy) * 1200.0);
                let count = (self.credit as usize).min(220);
                self.credit -= count as f32;
                for _ in 0..count {
                    let [x, y] = self.fern;
                    let r = random(&mut self.rng);
                    // Barnsley's four affine maps with probabilities 1/85/7/7%.
                    self.fern = if r < 0.01 {
                        [0.0, 0.16 * y]
                    } else if r < 0.86 {
                        [0.85 * x + 0.04 * y, -0.04 * x + 0.85 * y + 1.6]
                    } else if r < 0.93 {
                        [0.2 * x - 0.26 * y, 0.23 * x + 0.22 * y + 1.6]
                    } else {
                        [-0.15 * x + 0.28 * y, 0.26 * x + 0.24 * y + 0.44]
                    };
                    if self.points.len() < 16000 {
                        self.points.push(self.fern);
                    }
                }
            }
            7 => {
                for b in &mut self.blobs {
                    b.p[0] += b.v[0] * dt * (1.0 + unit(audio.treble));
                    b.p[1] += b.v[1] * dt * (1.0 + unit(audio.bass) * 3.0);
                    for axis in 0..2 {
                        if b.p[axis] < 0.06 || b.p[axis] > 0.94 {
                            b.p[axis] = b.p[axis].clamp(0.06, 0.94);
                            b.v[axis] = -b.v[axis];
                        }
                    }
                }
            }
            8 | 9 => {
                let attack = unit(audio.onset)
                    .max(unit(audio.kick))
                    .max(unit(audio.transients[2]));
                self.credit += dt;
                if attack > self.attack + 0.08 || self.credit > 2.5 {
                    let x = 0.16 + random(&mut self.rng) * 0.68;
                    let y = 0.18 + random(&mut self.rng) * 0.64;
                    self.inject(x, y, 0.25 + attack * 0.75, style == 9);
                    self.credit = 0.0;
                }
                self.attack = attack;
                let steps = (dt / 0.008).ceil().max(1.0) as usize;
                let h = dt / steps as f32;
                for _ in 0..steps {
                    for y in 1..GRID_H - 1 {
                        for x in 1..GRID_W - 1 {
                            let i = y * GRID_W + x;
                            let c = self.field[i];
                            let lap = self.field[i - 1]
                                + self.field[i + 1]
                                + self.field[i - GRID_W]
                                + self.field[i + GRID_W]
                                - 4.0 * c;
                            if style == 9 {
                                self.velocity[i] = (self.velocity[i]
                                    + (lap * (180.0 + unit(audio.bass) * 180.0)
                                        - self.velocity[i] * 1.8)
                                        * h)
                                    .clamp(-12.0, 12.0);
                                self.next[i] = (c + self.velocity[i] * h).clamp(-1.0, 1.0);
                            } else {
                                // Diffusion with a slow rotating incompressible transport field.
                                let vx = (y as f32 / GRID_H as f32 - 0.5)
                                    * (1.0 + unit(audio.mid) * 3.0);
                                let vy = -(x as f32 / GRID_W as f32 - 0.5) * 1.5;
                                let adv = vx * (self.field[i + 1] - self.field[i - 1]) * 0.5
                                    + vy * (self.field[i + GRID_W] - self.field[i - GRID_W]) * 0.5;
                                let feed = unit(bands[(x * 31 / (GRID_W - 1)).min(31)]) * 0.002;
                                self.next[i] = (c + h * (lap * 9.0 - adv - c * 0.12 + feed * c))
                                    .clamp(0.0, 1.0);
                            }
                        }
                    }
                    std::mem::swap(&mut self.field, &mut self.next);
                }
            }
            _ => {}
        }
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
        let style = style % NAMES.len();
        let dt = if time.is_finite() {
            self.last_time
                .map_or(1.0 / 60.0, |t| (time - t).clamp(0.0, 0.08))
        } else {
            0.0
        };
        if time.is_finite() {
            self.last_time = Some(time);
        }
        if self.style != Some(style) {
            self.reset(style);
        }
        self.advance(style, dt, audio, bands);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let visible = area.intersection(buf.area);
        let bg = theme.sample(palette, 0.0, 0.0);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)].reset();
                buf[(x, y)].set_char(' ').set_bg(bg);
            }
        }
        // All geometry is assembled once, then rasterized directly into the clip.
        let mut canvas = Canvas {
            buf,
            area,
            clip: visible,
            palette,
            theme,
        };
        match style {
            0 | 1 | 2 | 6 => {
                let wind = self.clock.sin() * (0.025 + unit(audio.bass) * 0.045);
                for s in &self.segments {
                    let bend = |mut p: [f32; 2]| {
                        if style == 6 {
                            p[0] += wind * (0.94 - p[1]).max(0.0).powi(2);
                        }
                        p
                    };
                    let glyph = match style {
                        0 => '•',
                        1 => '·',
                        2 => '┄',
                        _ => ' ',
                    };
                    canvas.line(
                        bend(s.a),
                        bend(s.b),
                        glyph,
                        0.12 + s.depth as f32 * 0.045,
                        0.45 + s.depth as f32 * 0.035,
                    );
                    if style == 6 && s.depth >= 6 {
                        canvas.point(bend(s.b), '♣', 0.34, 0.65);
                    }
                    if style == 0 && s.depth > 2 {
                        canvas.point(s.b, '●', 0.08 + s.depth as f32 * 0.03, 0.65);
                    }
                }
                for t in &self.tips {
                    canvas.point(
                        t.p,
                        if style == 1 {
                            '∘'
                        } else if style == 2 {
                            '╵'
                        } else {
                            '•'
                        },
                        0.4,
                        0.85,
                    );
                }
            }
            3 => {
                let extent = (self.age * (0.055 + unit(audio.energy) * 0.025)).min(0.88);
                let count = (extent * 180.0) as usize;
                let mut previous = [0.5, 0.96];
                for i in 1..=count {
                    let t = i as f32 / 180.0;
                    let p = [
                        0.5 + (t * 12.0 - self.clock).sin() * t * (0.17 + unit(audio.mid) * 0.1),
                        0.96 - t,
                    ];
                    canvas.line(previous, p, ' ', 0.30, 0.65);
                    if i % 10 == 0 {
                        let side = if (i / 10) % 2 == 0 { 1.0 } else { -1.0 };
                        let leaf = [
                            p[0] + side * (0.05 + unit(bands[i % 32]) * 0.045),
                            p[1] - 0.025,
                        ];
                        canvas.line(p, leaf, '╱', 0.4, 0.55);
                        canvas.point(leaf, if side > 0.0 { '❧' } else { '❦' }, 0.38, 0.8);
                    }
                    previous = p;
                }
            }
            4 => {
                for (i, p) in self.points.iter().enumerate() {
                    let sway = self.clock.sin() * p[1] * 0.0015;
                    canvas.point(
                        [0.5 + p[0] * 0.14 + sway, 0.94 - p[1] * 0.087],
                        if i % 7 == 0 { '⠿' } else { '⠂' },
                        0.31 + p[1] * 0.012,
                        0.52 + unit(audio.treble) * 0.18,
                    );
                }
            }
            5 => {
                let n = (16.0 + self.age * (14.0 + unit(audio.energy) * 22.0)).min(620.0) as usize;
                // The golden angle packs each new floret between older florets.
                let golden_angle = PI * (3.0 - 5.0_f32.sqrt());
                for i in (0..n).rev() {
                    let r = 0.016 * (i as f32).sqrt();
                    let a = i as f32 * golden_angle + self.clock * 0.07;
                    let p = [0.5 + r * a.cos(), 0.5 + r * a.sin()];
                    canvas.point(
                        p,
                        if i % 13 == 0 {
                            '✿'
                        } else if i % 3 == 0 {
                            '◆'
                        } else {
                            '•'
                        },
                        i as f32 / 620.0,
                        0.45 + 0.35 * (1.0 - i as f32 / 700.0),
                    );
                }
            }
            7 => {
                let blobs: Vec<_> = self
                    .blobs
                    .iter()
                    .map(|b| (b.p, b.radius * (1.0 + unit(audio.bass) * 0.25)))
                    .collect();
                for y in visible.y..visible.bottom() {
                    for x in visible.x..visible.right() {
                        let p = [
                            (x - area.x) as f32 / area.width as f32,
                            (y - area.y) as f32 / area.height as f32,
                        ];
                        let mut field = 0.0;
                        for &(q, r) in &blobs {
                            field +=
                                r * r / ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + 0.0002);
                        }
                        if field > 0.85 {
                            let mark = if field < 1.15 {
                                '░'
                            } else if field < 1.8 {
                                '▒'
                            } else {
                                '█'
                            };
                            canvas.cell(
                                x,
                                y,
                                mark,
                                0.08 + field.min(3.0) * 0.05,
                                (0.18 + field * 0.13).min(0.85),
                            );
                        }
                    }
                }
            }
            8 | 9 => {
                for y in visible.y..visible.bottom() {
                    for x in visible.x..visible.right() {
                        let gx =
                            ((x - area.x) as usize * GRID_W / area.width as usize).min(GRID_W - 1);
                        let gy =
                            ((y - area.y) as usize * GRID_H / area.height as usize).min(GRID_H - 1);
                        let i = gy * GRID_W + gx;
                        let v = self.field[i];
                        if style == 8 {
                            if v > 0.008 {
                                let mark = if v < 0.035 {
                                    '·'
                                } else if v < 0.10 {
                                    '░'
                                } else if v < 0.25 {
                                    '▒'
                                } else {
                                    '▓'
                                };
                                canvas.cell(x, y, mark, 0.6 + v * 0.2, (v.sqrt() * 1.4).min(0.8));
                            }
                        } else {
                            let slope = self.field[gy * GRID_W + (gx + 1).min(GRID_W - 1)] - v;
                            if v.abs() > 0.012 || slope.abs() > 0.015 {
                                let mark = if v.abs() < 0.04 {
                                    '·'
                                } else if slope.abs() > 0.03 {
                                    if slope > 0.0 {
                                        '╱'
                                    } else {
                                        '╲'
                                    }
                                } else {
                                    '─'
                                };
                                canvas.cell(
                                    x,
                                    y,
                                    mark,
                                    if v > 0.0 { 0.53 } else { 0.82 },
                                    (v.abs() * 1.8 + slope.abs() * 2.0 + 0.15).min(0.9),
                                );
                            }
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}
struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    clip: Rect,
    palette: usize,
    theme: &'a Theme,
}
impl Canvas<'_> {
    fn cell(&mut self, x: u16, y: u16, mark: char, phase: f32, light: f32) {
        self.buf[(x, y)]
            .set_char(mark)
            .set_fg(self.theme.sample(self.palette, phase, light));
    }
    fn position(&self, p: [f32; 2]) -> [f32; 2] {
        // Limit horizontal expansion on wide terminals while retaining square cells.
        let width = (self.area.height as f32 * 2.0).min(self.area.width as f32);
        [
            self.area.x as f32 + self.area.width as f32 * 0.5 + (p[0] - 0.5) * width,
            self.area.y as f32 + p[1] * self.area.height.saturating_sub(1) as f32,
        ]
    }
    fn plot(&mut self, p: [f32; 2], mark: char, phase: f32, light: f32) {
        let x = p[0].round() as i32;
        let y = p[1].round() as i32;
        if x >= self.clip.x as i32
            && x < self.clip.right() as i32
            && y >= self.clip.y as i32
            && y < self.clip.bottom() as i32
        {
            self.cell(x as u16, y as u16, mark, phase, light);
        }
    }
    fn point(&mut self, p: [f32; 2], mark: char, phase: f32, light: f32) {
        let p = self.position(p);
        self.plot(p, mark, phase, light);
    }
    fn line(&mut self, a: [f32; 2], b: [f32; 2], mut mark: char, phase: f32, light: f32) {
        let a = self.position(a);
        let b = self.position(b);
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        if mark == ' ' {
            mark = if dx.abs() < dy.abs() * 0.5 {
                '│'
            } else if dy.abs() < dx.abs() * 0.35 {
                '─'
            } else if dx * dy > 0.0 {
                '╲'
            } else {
                '╱'
            };
        }
        let n = (dx.abs().max(dy.abs()).ceil() as usize).clamp(1, 1024);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.plot([a[0] + dx * t, a[1] + dy * t], mark, phase, light);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(style: usize, audio: &Reactivity) -> (Renderer, Buffer) {
        let mut r = Renderer::default();
        let mut b = Buffer::empty(Rect::new(0, 0, 80, 30));
        for i in 0..240 {
            r.draw(
                &mut b,
                Rect::new(0, 0, 80, 30),
                i as f32 / 30.0,
                style,
                0,
                &[0.5; 32],
                audio,
                &Theme::default(),
            );
        }
        (r, b)
    }
    #[test]
    fn all_styles_clip_to_intersection() {
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(7, 5, 19, 11),
                Rect::new(0, 0, 1, 1),
                Rect::new(0, 0, 0, 0),
                Rect::new(100, 100, 2, 2),
                Rect::new(30, 15, 20, 20),
            ] {
                let mut b = Buffer::empty(Rect::new(3, 2, 32, 19));
                for c in &mut b.content {
                    c.set_char('X');
                }
                let mut r = Renderer::default();
                r.draw(
                    &mut b,
                    area,
                    1.0,
                    style,
                    0,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                let clip = area.intersection(b.area);
                for y in b.area.y..b.area.bottom() {
                    for x in b.area.x..b.area.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(b[(x, y)].symbol(), "X", "style {style}");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn ten_distinct_scenes_with_structural_audio_response() {
        let quiet = Reactivity::default();
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            ..Default::default()
        };
        let mut outputs = std::collections::HashSet::new();
        for style in 0..NAMES.len() {
            let (_, q) = run(style, &quiet);
            let (_, l) = run(style, &loud);
            let symbols = |b: &Buffer| b.content.iter().map(|c| c.symbol()).collect::<String>();
            let qs = symbols(&q);
            assert!(qs.chars().any(|c| c != ' '));
            assert!(outputs.insert(qs.clone()), "duplicate style {style}");
            // Fern has an exact fixed attractor, so density responds, not its maps.
            assert_ne!(qs, symbols(&l), "no structural response in style {style}");
        }
    }
    #[test]
    fn simulations_stay_finite_bounded_and_time_jumps_are_capped() {
        for style in 0..NAMES.len() {
            let (mut r, mut b) = run(style, &Reactivity::default());
            let age = r.age;
            r.draw(
                &mut b,
                Rect::new(0, 0, 80, 30),
                1.0e9,
                style,
                0,
                &[1.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            assert!((r.age - age).abs() < 0.081);
            assert!(r.segments.len() <= MAX_SEGMENTS);
            assert!(r.points.len() <= 16000);
            assert!(r.tips.len() <= 512);
            assert!(r.field.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
            assert!(r.velocity.iter().all(|x| x.is_finite() && x.abs() <= 12.0));
            assert!(r.points.iter().flatten().all(|x| x.is_finite()));
            assert!(r.segments.iter().all(|s| s
                .a
                .iter()
                .chain(s.b.iter())
                .all(|x| x.is_finite() && *x >= 0.0 && *x <= 1.0)));
        }
    }
    #[test]
    #[ignore = "manual release-mode frame timing"]
    fn release_frame_timing() {
        let mut b = Buffer::empty(Rect::new(0, 0, 160, 50));
        for (style, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            for i in 0..400 {
                r.draw(
                    &mut b,
                    Rect::new(0, 0, 160, 50),
                    i as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
            }
            let start = std::time::Instant::now();
            for i in 400..520 {
                r.draw(
                    &mut b,
                    Rect::new(0, 0, 160, 50),
                    i as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
            }
            eprintln!(
                "{}: {:.3} ms/frame",
                name,
                start.elapsed().as_secs_f64() * 1000.0 / 120.0
            );
        }
    }
}
