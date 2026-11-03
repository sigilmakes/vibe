//! Orbital mechanics scenes. Positions use a unit-height projection with the
//! terminal aspect corrected at rasterization, so circles stay circular on any
//! window shape.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::TAU;

pub const NAMES: &[&str] = &[
    "Kepler Drift",
    "Ring Shepherd",
    "Lagrange Bloom",
    "Total Eclipse",
    "Accretion Disc",
    "Comet Tail",
    "Gravity Assist",
    "Tidal Lock",
    "Resonance Chain",
    "Roche Limit",
];

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    n ^= n >> 13;
    n = n.wrapping_mul(0xC2B2_AE3D);
    n ^= n >> 16;
    (n & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    clock: f32,
    xs: Vec<f32>,
    ys: Vec<f32>,
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
        // Audio only scales the clock; every scene keeps its own fixed tempo.
        self.clock += dt * (0.6 + unit(audio.energy) * 0.9 + unit(audio.bass) * 0.2);
    }

    fn project(&mut self, area: Rect, visible: Rect) {
        let height = area.height.max(1) as f32;
        let scale = 2.0 / height;
        let cx = area.x as f32 + area.width as f32 * 0.5;
        let cy = area.y as f32 + area.height as f32 * 0.5;
        self.xs.clear();
        self.ys.clear();
        self.xs
            .extend((visible.x..visible.right()).map(|x| (x as f32 + 0.5 - cx) * scale));
        self.ys
            .extend((visible.y..visible.bottom()).map(|y| (y as f32 + 0.5 - cy) * scale));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    buf: &mut Buffer,
    col: u16,
    row: u16,
    mark: char,
    phase: f32,
    light: f32,
    bg_phase: f32,
    bg_light: f32,
    palette: usize,
    theme: &Theme,
) {
    let cell = &mut buf[(col, row)];
    cell.reset();
    cell.set_char(mark);
    cell.set_fg(theme.sample(palette, phase, light));
    cell.set_bg(theme.sample(palette, bg_phase, bg_light));
}

fn wash(buf: &mut Buffer, visible: Rect, palette: usize, theme: &Theme, phase: f32, light: f32) {
    for y in visible.y..visible.bottom() {
        for x in visible.x..visible.right() {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_char(' ');
            cell.set_bg(theme.sample(palette, phase, light));
        }
    }
}

fn sprinkle_stars(
    buf: &mut Buffer,
    visible: Rect,
    palette: usize,
    theme: &Theme,
    t: f32,
    gain: f32,
) {
    for y in visible.y..visible.bottom() {
        for x in visible.x..visible.right() {
            let h = hash(x as i32 * 3 + 11, y as i32 * 5 + 7);
            if h > 0.982 {
                let twinkle = 0.5 + 0.5 * (t * 1.9 + h * 47.0).sin();
                let light = (0.16 + 0.55 * twinkle) * gain;
                let cell = &mut buf[(x, y)];
                cell.set_char(if h > 0.997 { '✦' } else { '·' });
                cell.set_fg(theme.sample(palette, 0.55 + h * 0.35, light));
            }
        }
    }
}

fn body_light(dx: f32, dy: f32, z: f32) -> f32 {
    (0.12 + 0.88 * (dx * -0.55 + dy * -0.45 + z * 0.70).max(0.0)).clamp(0.0, 1.0)
}

struct Canvas<'a> {
    buf: &'a mut Buffer,
    clip: Rect,
    palette: usize,
    theme: &'a Theme,
    scale: f32,
    cx: f32,
    cy: f32,
}

impl<'a> Canvas<'a> {
    fn new(buf: &'a mut Buffer, area: Rect, clip: Rect, palette: usize, theme: &'a Theme) -> Self {
        let height = area.height.max(1) as f32;
        Self {
            buf,
            clip,
            palette,
            theme,
            scale: 2.0 / height,
            cx: area.x as f32 + area.width as f32 * 0.5,
            cy: area.y as f32 + area.height as f32 * 0.5,
        }
    }

    fn plot(&mut self, p: [f32; 2], mark: char, phase: f32, light: f32) {
        if !p[0].is_finite() || !p[1].is_finite() || light <= 0.0 {
            return;
        }
        let col = (self.cx + p[0] / self.scale - 0.5).round() as i32;
        let row = (self.cy + p[1] / self.scale - 0.5).round() as i32;
        if col >= self.clip.x as i32
            && col < self.clip.right() as i32
            && row >= self.clip.y as i32
            && row < self.clip.bottom() as i32
        {
            let cell = &mut self.buf[(col as u16, row as u16)];
            cell.set_char(mark);
            cell.set_fg(self.theme.sample(self.palette, phase, light));
        }
    }

    fn line(&mut self, a: [f32; 2], b: [f32; 2], mark: char, phase: f32, light: f32) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let steps = (dx.abs().max(dy.abs()) / self.scale).ceil() as usize;
        let steps = steps.clamp(1, 512);
        for i in 0..=steps {
            let s = i as f32 / steps as f32;
            self.plot([a[0] + dx * s, a[1] + dy * s], mark, phase, light);
        }
    }

    fn circle(&mut self, c: [f32; 2], r: f32, mark: char, phase: f32, light: f32) {
        let count = ((r * TAU / self.scale).ceil() as usize).clamp(8, 96);
        for i in 0..count {
            let a = i as f32 / count as f32 * TAU;
            self.plot([c[0] + a.cos() * r, c[1] + a.sin() * r], mark, phase, light);
        }
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
        let style = style % NAMES.len();
        self.advance(time, audio);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        self.project(area, visible);
        let t = self.clock;
        match style {
            0 => self.kepler(buf, visible, audio, theme, palette, t),
            1 => self.rings(buf, visible, audio, bands, theme, palette, t),
            2 => self.lagrange(buf, visible, audio, theme, palette, t),
            3 => self.eclipse(buf, visible, audio, theme, palette, t),
            4 => self.accretion(buf, visible, audio, theme, palette, t),
            5 => self.comet(buf, area, visible, audio, theme, palette, t),
            6 => self.assist(buf, area, visible, audio, theme, palette, t),
            7 => self.tidal(buf, area, visible, audio, theme, palette, t),
            8 => self.resonance(buf, area, visible, audio, theme, palette, t),
            _ => self.roche(buf, area, visible, audio, theme, palette, t),
        }
    }

    fn kepler(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let a = [0.62_f32, 1.04, 1.58];
        let ecc = [0.16_f32, 0.26, 0.33];
        let rot = [0.35 + t * 0.03, 2.15 - t * 0.02, 4.3 + t * 0.014];
        let mut cs = [0.0_f32; 3];
        let mut sn = [0.0_f32; 3];
        let mut bodies = [[0.0_f32; 2]; 3];
        let mut angles = [0.0_f32; 3];
        let mut semi = [0.0_f32; 3];
        for k in 0..3 {
            let b = a[k] * (1.0 - ecc[k] * ecc[k]).sqrt();
            let mean = t * 0.6 / a[k].powf(1.5) + k as f32 * 2.1;
            let mut e = mean;
            for _ in 0..4 {
                e -= (e - ecc[k] * e.sin() - mean) / (1.0 - ecc[k] * e.cos());
            }
            let x0 = a[k] * (e.cos() - ecc[k]);
            let y0 = b * e.sin();
            cs[k] = rot[k].cos();
            sn[k] = rot[k].sin();
            bodies[k] = [x0 * cs[k] - y0 * sn[k], x0 * sn[k] + y0 * cs[k]];
            angles[k] = (y0 / b).atan2(x0 / a[k]);
            semi[k] = b;
        }
        let bass = unit(audio.bass);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r2 = x * x + y * y;
                let core = (-r2 * 240.0).exp();
                let halo = 0.30 / (1.0 + r2 * 70.0) * (0.5 + bass * 1.2);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                for k in 0..3 {
                    let xr = x * cs[k] + y * sn[k];
                    let yr = -x * sn[k] + y * cs[k];
                    let d = (xr / a[k]).hypot(yr / semi[k]);
                    let ring = (-((d - 1.0) * 22.0).powi(2)).exp();
                    let ang = (yr / semi[k]).atan2(xr / a[k]);
                    let behind = (angles[k] - ang).rem_euclid(TAU);
                    let trail = if behind < 2.0 {
                        (-behind * 1.35).exp()
                    } else {
                        0.0
                    };
                    let line = ring * (0.20 + 0.85 * trail);
                    if line > light && line > 0.05 {
                        mark = '·';
                        light = line * 0.82;
                        phase = 0.50 + k as f32 * 0.09;
                    }
                    let pd2 = (x - bodies[k][0]).powi(2) + (y - bodies[k][1]).powi(2);
                    let head = (-pd2 * 620.0).exp();
                    if head > light && head > 0.05 {
                        mark = '●';
                        light = head;
                        phase = 0.62 + k as f32 * 0.08;
                    }
                }
                if core > light && core > 0.04 {
                    mark = '✦';
                    light = core;
                    phase = 0.10;
                }
                let h = hash(i as i32 * 17 + 5, j as i32 * 23 + 9);
                if h > 0.985 {
                    let twinkle = 0.25 + 0.35 * (t * 1.7 + h * 31.0).sin().abs();
                    if twinkle > light {
                        mark = '·';
                        light = twinkle;
                        phase = 0.55 + h * 0.3;
                    }
                }
                let bg_light = 0.016 + halo * 0.35 + (light * 0.10).min(0.07);
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.56 + y * 0.04,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rings(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        bands: &[f32; 32],
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let flatten = 0.30 + unit(audio.bass) * 0.03;
        let tilt = 0.18 + 0.05 * (t * 0.07).sin();
        let ct = tilt.cos();
        let st = tilt.sin();
        let planet = 0.30 + unit(audio.bass) * 0.06;
        let gap_a = 0.92 + unit(audio.mid) * 0.06;
        let gap_b = 1.38 - unit(audio.mid) * 0.04;
        let moons = [(gap_a, t * 0.8, 1.0_f32), (gap_b, -t * 0.55 + 2.0, 1.4_f32)];
        let mut moon_pos = [[0.0_f32; 2]; 2];
        for (k, (q, ang, _)) in moons.iter().enumerate() {
            let x0 = q * ang.cos();
            let y0 = q * ang.sin() * flatten;
            moon_pos[k] = [x0 * ct - y0 * st, x0 * st + y0 * ct];
        }
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let xr = x * ct + y * st;
                let yr = -x * st + y * ct;
                let q = (xr * xr + (yr / flatten).powi(2)).sqrt();
                let phi = (yr / flatten).atan2(xr);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                if q < planet {
                    let z = (1.0 - (q / planet).powi(2)).max(0.0).sqrt();
                    let lat = (yr / flatten) / planet;
                    let band = 0.5 + 0.5 * (lat * 16.0 + z * 5.0 + t * 0.12).sin();
                    light = (0.38 + 0.30 * band + 0.18 * z).clamp(0.0, 1.0);
                    mark = if light > 0.62 { '█' } else { '▓' };
                    phase = 0.18 + band * 0.10;
                } else {
                    let win =
                        ((q - 0.42) / 0.06).clamp(0.0, 1.0) * ((2.45 - q) / 0.18).clamp(0.0, 1.0);
                    let band_wave = unit(bands[(phi.rem_euclid(TAU) / TAU * 31.0) as usize]);
                    let band = 0.5
                        + 0.5
                            * (q * 34.0 + (q * 8.0).sin() * 2.6 - t * 0.25 + band_wave * 1.5).sin();
                    let gap = 1.0
                        - 0.94 * (-((q - gap_a) / 0.035).powi(2)).exp()
                        - 0.86 * (-((q - gap_b) / 0.05).powi(2)).exp();
                    let speck = 0.35 + 0.65 * hash(i as i32 * 7 + 3, j as i32 * 11 + 5);
                    let dens = (win * band * gap * speck).clamp(0.0, 1.0)
                        * (0.55 + 0.45 * unit(audio.treble));
                    if dens > 0.06 {
                        mark = if dens > 0.55 {
                            '▒'
                        } else if dens > 0.22 {
                            '░'
                        } else {
                            '·'
                        };
                        light = 0.22 + dens * 0.72;
                        phase = 0.42 + dens * 0.16 + q * 0.03;
                    }
                    for (k, m) in moon_pos.iter().enumerate() {
                        let d = (x - m[0]).hypot(y - m[1]);
                        if d < 0.05 {
                            mark = '●';
                            light = 0.85;
                            phase = 0.62 + k as f32 * 0.12;
                        }
                    }
                }
                let glow = 0.22 / (1.0 + q * 6.0);
                let bg_light = 0.014 + light * 0.12 + glow * 0.10;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.12 + q * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn lagrange(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let m2 = 0.30 + unit(audio.bass) * 0.26;
        let x1 = -0.48_f32;
        let x2 = 0.48_f32;
        let l1 = [x2 - 0.20, 0.0];
        let l2 = [x2 + 0.17, 0.0];
        let l3 = [x1 - 0.56, 0.0];
        let l4 = [0.0, 0.83];
        let l5 = [0.0, -0.83];
        let levels = [-4.2_f32, -3.2, -2.55, -2.10, -1.80, -1.57, -1.39];
        let trojan = t * 0.65 + unit(audio.locked_beat_phase()) * TAU;
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r1 = ((x - x1).powi(2) + y * y + 0.015).sqrt();
                let r2 = ((x - x2).powi(2) + y * y + 0.015).sqrt();
                let r = (x * x + y * y).sqrt();
                let u = -1.0 / r1 - m2 / r2 - 0.30 * r * r;
                let mut near = f32::MAX;
                for level in levels {
                    near = near.min((u - level).abs());
                }
                let contour = (-(near * 24.0).powi(2)).exp();
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                if contour > 0.10 {
                    mark = '·';
                    light = 0.18 + contour * 0.52;
                    phase = 0.44 + (u * 0.05).fract() * 0.20;
                }
                for (k, p) in [l1, l2, l3, l4, l5].iter().enumerate() {
                    let d = (x - p[0]).hypot(y - p[1]);
                    if d < 0.055 {
                        mark = '◇';
                        light = 0.92;
                        phase = 0.68 + k as f32 * 0.03;
                    }
                    if k >= 3 {
                        let ring = (-((d - 0.15) / 0.022).powi(2)).exp();
                        if ring > light * 0.9 && ring > 0.05 {
                            mark = '·';
                            light = ring * 0.55;
                            phase = 0.60;
                        }
                        for n in 0..4 {
                            let a = trojan + n as f32 * TAU / 4.0 + k as f32;
                            let q = [p[0] + a.cos() * 0.15, p[1] + a.sin() * 0.15];
                            if (x - q[0]).hypot(y - q[1]) < 0.022 {
                                mark = '•';
                                light = 0.78;
                                phase = 0.64;
                            }
                        }
                    }
                }
                let glow = 0.5 / (1.0 + r1 * 3.0) + 0.35 / (1.0 + r2 * 3.0) + 0.10 / (1.0 + r);
                let bg_light = 0.012 + glow * 0.06 + contour * 0.10;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.55 + u * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn eclipse(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let sun_r = 0.60 + unit(audio.bass) * 0.05;
        let moon_r = 0.63 + unit(audio.mid) * 0.02;
        let cycle = (t * 0.072 + unit(audio.locked_beat_phase()) * 0.12).fract();
        let mx = (cycle * 2.5 - 1.25) * sun_r;
        let my = -0.03 + 0.06 * (t * 0.31).sin();
        let energy = unit(audio.energy);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r = x.hypot(y);
                let dm = (x - mx).hypot(y - my);
                let theta = y.atan2(x);
                let ray = (0.5 + 0.5 * (theta * 11.0 + (theta * 3.0 + t * 0.15).sin() * 2.0).sin())
                    .powi(3);
                let corona = (-(r - sun_r) * 2.2).exp() * ray * (0.5 + energy * 0.8);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.010 + corona * 0.13;
                if dm < moon_r {
                    mark = '█';
                    light = 0.008;
                    phase = 0.0;
                } else if r < sun_r {
                    let grain =
                        0.5 + 0.5 * (x * 34.0 + y * 21.0 + (x * 13.0).sin() * 1.7 + t * 0.2).sin();
                    light = 0.70 + 0.24 * grain;
                    mark = if light > 0.82 { '█' } else { '▓' };
                    phase = 0.10 + grain * 0.07;
                    bg_light += 0.10;
                } else if corona > 0.05 {
                    mark = if corona > 0.35 { '│' } else { '·' };
                    light = corona * 0.8;
                    phase = 0.16 + ray * 0.10;
                }
                let limb = (-((r - sun_r) / 0.025).powi(2)).exp();
                if limb > light && limb > 0.05 && dm > moon_r {
                    mark = '░';
                    light = limb;
                    phase = 0.20;
                }
                let contact =
                    (-((dm - moon_r) / 0.03).powi(2)).exp() * (-((r - sun_r) / 0.06).powi(2)).exp();
                if contact > light && contact > 0.04 && dm > moon_r {
                    mark = '✦';
                    light = contact;
                    phase = 0.30;
                }
                let h = hash(i as i32 * 19 + 1, j as i32 * 29 + 3);
                if h > 0.988 && r > sun_r + 0.05 {
                    let twinkle = 0.2 + 0.35 * (t * 1.5 + h * 37.0).sin().abs();
                    if twinkle > light {
                        mark = '·';
                        light = twinkle;
                        phase = 0.6 + h * 0.25;
                    }
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.15 + ray * 0.06,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn accretion(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let flatten = 0.30 + unit(audio.bass) * 0.04;
        let inner = 0.30 + unit(audio.bass) * 0.05;
        let mid = unit(audio.mid);
        let treble = unit(audio.treble);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let yy = y / flatten;
                let r = (x * x + yy * yy).sqrt();
                let phi = yy.atan2(x);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.006;
                if r > inner * 1.01 {
                    let spiral = (phi + 2.7 * (r + 0.01).ln() - t * 0.75).sin();
                    let arm = (0.5 + 0.5 * spiral).powf(2.5);
                    let radial = (inner / (r + 0.02)).powf(0.95);
                    let doppler = 0.62 + 0.38 * (phi + t * 0.35).sin();
                    let grain = 0.55 + 0.45 * hash(i as i32 * 5 + 2, j as i32 * 3 + 7);
                    let fade = (-r * 0.45).exp();
                    let e = 3.0
                        * radial
                        * (0.34 + 0.66 * arm)
                        * doppler
                        * grain
                        * (0.70 + 0.45 * mid)
                        * fade;
                    if e > 0.06 {
                        mark = if e > 0.78 {
                            '█'
                        } else if e > 0.52 {
                            '▓'
                        } else if e > 0.30 {
                            '▒'
                        } else if e > 0.14 {
                            '░'
                        } else {
                            '·'
                        };
                        light = e.min(1.0);
                        phase = 0.30 + r * 0.05 + arm * 0.10;
                        bg_light = 0.01 + e * 0.16;
                    }
                }
                let photon = (-((r - inner * 1.015) / 0.018).powi(2)).exp();
                if photon > light * 0.8 && photon > 0.05 {
                    mark = '█';
                    light = photon * (0.75 + unit(audio.kick) * 0.3);
                    phase = 0.18;
                    bg_light = bg_light.max(0.06);
                }
                let jet = (-x * x / 0.0016).exp()
                    * (-y.abs() * 0.55).exp()
                    * (0.30 + treble * 0.8 + 0.12 * (y.abs() * 9.0 - t * 3.0).sin());
                if jet > light * 0.9 && jet > 0.05 {
                    mark = '│';
                    light = jet.min(1.0);
                    phase = 0.52;
                    bg_light = bg_light.max(jet * 0.10);
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.05 + phi.rem_euclid(TAU) * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn comet(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.55, 0.012);
        sprinkle_stars(buf, visible, palette, theme, t, 0.8);
        let energy = unit(audio.energy);
        let bass = unit(audio.bass);
        let star = [0.45_f32, 0.10];
        let a = 1.00_f32;
        let e = 0.45_f32;
        let b = a * (1.0 - e * e).sqrt();
        let rot = 0.15_f32;
        let cr = rot.cos();
        let sr = rot.sin();
        let focus = |anomaly: f32| -> [f32; 2] {
            let x0 = a * (anomaly.cos() - e);
            let y0 = b * anomaly.sin();
            [star[0] + x0 * cr - y0 * sr, star[1] + x0 * sr + y0 * cr]
        };
        let anom = t * 0.35 + 1.1;
        let head = focus(anom);
        let mut away = [head[0] - star[0], head[1] - star[1]];
        let d = away[0].hypot(away[1]).max(0.001);
        away = [away[0] / d, away[1] / d];
        let perp = [-away[1], away[0]];
        let len = 1.15 + energy * 0.8 + bass * 0.45;
        let mut c = Canvas::new(buf, area, visible, palette, theme);
        for k in 0..60 {
            let p0 = focus(k as f32 / 60.0 * TAU);
            let p1 = focus((k + 1) as f32 / 60.0 * TAU);
            c.line(p0, p1, '·', 0.50, 0.16);
        }
        for k in 1..=26 {
            let past = focus(anom - k as f32 * 0.035);
            let light = (1.0 - k as f32 / 26.0) * 0.45;
            c.plot(past, '·', 0.34, light);
        }
        for s in 0..5 {
            let off = (s as f32 - 2.0) * 0.020;
            let mut prev = head;
            for i in 1..=20 {
                let u = i as f32 / 20.0;
                let wiggle = (u * 13.0 - t * 4.0 + s as f32).sin() * 0.014 * u;
                let p = [
                    head[0] + away[0] * u * len + perp[0] * (off * (0.4 + u) + wiggle),
                    head[1] + away[1] * u * len + perp[1] * (off * (0.4 + u) + wiggle),
                ];
                c.line(
                    prev,
                    p,
                    '·',
                    0.55 + s as f32 * 0.02,
                    (1.0 - u) * 0.60 + 0.05,
                );
                prev = p;
            }
        }
        let bend = 0.55 + unit(audio.mid) * 0.25;
        for s in 0..8 {
            let off = (s as f32 - 3.5) * 0.018;
            let mut prev = head;
            for i in 1..=18 {
                let u = i as f32 / 18.0;
                let p = [
                    head[0] + away[0] * u * len * 0.78 + perp[0] * (u * u * bend + off),
                    head[1] + away[1] * u * len * 0.78 + perp[1] * (u * u * bend + off),
                ];
                c.line(
                    prev,
                    p,
                    '·',
                    0.24 + s as f32 * 0.015,
                    (1.0 - u) * 0.40 + 0.04,
                );
                prev = p;
            }
        }
        let burst = unit(audio.transients[0]).max(unit(audio.transients[1]));
        c.circle(head, 0.13 + burst * 0.05, '·', 0.36, 0.26 + burst * 0.3);
        c.circle(head, 0.065, '∘', 0.40, 0.55);
        c.plot(head, '●', 0.55, 0.96);
        c.plot(star, '✦', 0.12, 0.95);
        c.circle(star, 0.045, '·', 0.14, 0.60);
        c.circle(star, 0.085, '·', 0.14, 0.32);
        let treble = unit(audio.treble);
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU + t * 0.06;
            c.line(
                [star[0] + a.cos() * 0.04, star[1] + a.sin() * 0.04],
                [
                    star[0] + a.cos() * (0.10 + treble * 0.05),
                    star[1] + a.sin() * (0.10 + treble * 0.05),
                ],
                '·',
                0.14,
                0.42,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn assist(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let planet = [0.42_f32, -0.04];
        let pull = 0.10 * (1.0 + unit(audio.bass) * 0.9);
        let mid = unit(audio.mid);
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            for gx in 0..13 {
                let x = -3.0 + gx as f32 * 0.5;
                let mut y = -1.05;
                while y <= 1.05 {
                    let dx = x - planet[0];
                    let dy = y - planet[1];
                    let d2 = dx * dx + dy * dy + 0.02;
                    let mut ox = -pull * dx / d2;
                    let mut oy = -pull * dy / d2;
                    let mag = ox.hypot(oy);
                    if mag > 0.32 {
                        ox *= 0.32 / mag;
                        oy *= 0.32 / mag;
                    }
                    let light = 0.10 + mid * 0.10 + 0.25 / (1.0 + d2 * 8.0) * 0.35;
                    c.plot([x + ox, y + oy], '·', 0.52, light);
                    y += 0.035;
                }
            }
            for gy in 0..9 {
                let y = -1.0 + gy as f32 * 0.25;
                let mut x = -3.2;
                while x <= 3.2 {
                    let dx = x - planet[0];
                    let dy = y - planet[1];
                    let d2 = dx * dx + dy * dy + 0.02;
                    let mut ox = -pull * dx / d2;
                    let mut oy = -pull * dy / d2;
                    let mag = ox.hypot(oy);
                    if mag > 0.32 {
                        ox *= 0.32 / mag;
                        oy *= 0.32 / mag;
                    }
                    let light = 0.08 + mid * 0.08 + 0.20 / (1.0 + d2 * 8.0) * 0.30;
                    c.plot([x + ox, y + oy], '·', 0.52, light);
                    x += 0.035;
                }
            }
        }
        let rp = 0.46_f32;
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let dx = x - planet[0];
                let dy = y - planet[1];
                let d = (dx * dx + dy * dy).sqrt();
                if d < rp {
                    let z = (1.0 - (d / rp).powi(2)).max(0.0).sqrt();
                    let light = body_light(dx / rp, dy / rp, z);
                    let mark = if light > 0.68 {
                        '█'
                    } else if light > 0.45 {
                        '▓'
                    } else if light > 0.22 {
                        '▒'
                    } else {
                        '░'
                    };
                    paint(
                        buf,
                        visible.x + i as u16,
                        visible.y + j as u16,
                        mark,
                        0.22 + light * 0.08,
                        light,
                        0.55,
                        0.03,
                        palette,
                        theme,
                    );
                }
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            c.circle(planet, 0.72, '·', 0.45, 0.16);
            let moon_a = t * 0.9;
            c.plot(
                [
                    planet[0] + moon_a.cos() * 0.72,
                    planet[1] + moon_a.sin() * 0.72,
                ],
                '•',
                0.60,
                0.7,
            );
            let xmax = area.width as f32 / area.height.max(1) as f32;
            let p0 = [-xmax - 0.3, 0.78];
            let p1 = [planet[0] - 0.05, planet[1] + 0.52];
            let p2 = [xmax + 0.3, -0.82];
            let point = |u: f32| {
                let v = 1.0 - u;
                [
                    v * v * p0[0] + 2.0 * v * u * p1[0] + u * u * p2[0],
                    v * v * p0[1] + 2.0 * v * u * p1[1] + u * u * p2[1],
                ]
            };
            let progress = (t * 0.085 + unit(audio.locked_beat_phase()) * 0.15).fract();
            let last = (progress * 90.0) as usize;
            for k in 0..last {
                let u = k as f32 / 90.0;
                c.plot(point(u), '·', 0.40, 0.35 + u * 0.25);
            }
            let ship = point(progress);
            let flare = 0.4 + unit(audio.transients[3]) * 0.8;
            c.circle(ship, 0.045, '·', 0.15, flare * 0.5);
            c.plot(ship, '◆', 0.12, 0.95);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tidal(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.012);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let (px, py, pr) = (-1.10_f32, 0.05_f32, 1.28_f32);
        let (mx, my, mr) = (0.88_f32, -0.02_f32, 0.62_f32);
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let lib = 0.22 * (t * 0.52 + unit(audio.locked_beat_phase()) * 0.6).sin();
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.010;
                let mdx = (x - mx) / mr;
                let mdy = (y - my) / mr;
                let meff = mdx.hypot(mdy);
                let bulge = 0.12 * (1.0 + bass * 1.1);
                let limit = 1.0 + bulge * (-mdx).max(0.0);
                if meff < limit {
                    let z = (1.0 - (meff / limit).powi(2)).max(0.0).sqrt();
                    let lon = mdx.atan2(z.max(0.06)) + lib;
                    let lat = mdy.clamp(-1.0, 1.0).asin();
                    let ch = hash((lon * 3.2).floor() as i32, (lat * 5.5).floor() as i32);
                    let mut l = (0.10 + 0.90 * (-mdx * 0.82 + z * 0.38).max(0.0) + treble * 0.06)
                        .clamp(0.0, 1.0);
                    if ch > 0.82 {
                        l *= 0.55;
                        mark = if ch > 0.94 { '○' } else { '·' };
                    } else {
                        mark = if l > 0.62 {
                            '█'
                        } else if l > 0.38 {
                            '▓'
                        } else if l > 0.16 {
                            '▒'
                        } else {
                            '░'
                        };
                    }
                    light = l;
                    phase = 0.42 + ch * 0.12;
                    bg_light = 0.012 + l * 0.05;
                } else if (x - px).hypot(y - py) < pr {
                    let dx = (x - px) / pr;
                    let dy = (y - py) / pr;
                    let z = (1.0 - (dx * dx + dy * dy).clamp(0.0, 1.0)).max(0.0).sqrt();
                    let band = 0.5 + 0.5 * (dy * 21.0 + (dx * 5.0 + t * 0.12).sin() * 2.2).sin();
                    let l = (body_light(dx, dy, z) * (0.72 + 0.28 * band)).clamp(0.0, 1.0);
                    mark = if l > 0.66 {
                        '█'
                    } else if l > 0.42 {
                        '▓'
                    } else if l > 0.20 {
                        '▒'
                    } else {
                        '░'
                    };
                    light = l;
                    phase = 0.18 + band * 0.10;
                    bg_light = 0.014 + l * 0.07;
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.55 + mdy * 0.03,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            for s in 0..3 {
                let lift = (s as f32 - 1.0) * 0.16;
                let start = [mx - 0.48, my + lift * 0.4];
                let control = [0.10, my + lift + 0.22 * (1.0 + bass)];
                let end = [px + pr * 0.55, py + lift * 0.5];
                let mut prev = start;
                for k in 1..=24 {
                    let u = k as f32 / 24.0;
                    let v = 1.0 - u;
                    let p = [
                        v * v * start[0] + 2.0 * v * u * control[0] + u * u * end[0],
                        v * v * start[1] + 2.0 * v * u * control[1] + u * u * end[1],
                    ];
                    c.line(
                        prev,
                        p,
                        '·',
                        0.34 + s as f32 * 0.04,
                        (1.0 - u) * 0.35 + 0.05,
                    );
                    prev = p;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resonance(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let beat = unit(audio.beat);
        let radii = [
            0.34 + bass * 0.05,
            0.56 + bass * 0.04,
            0.84,
            1.18,
            1.58,
            2.08,
        ];
        let pulse_r = unit(audio.locked_beat_phase()) * 2.35;
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r = x.hypot(y);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                for (k, radius) in radii.iter().enumerate() {
                    let w = (-((r - radius) / 0.026).powi(2)).exp();
                    if w > 0.06 {
                        mark = if w > 0.55 { '─' } else { '·' };
                        light = (0.16 + w * 0.45) * (0.6 + mid * 0.4);
                        phase = 0.38 + k as f32 * 0.055;
                    }
                }
                let pulse = (-((r - pulse_r) / 0.030).powi(2)).exp() * (0.25 + beat * 0.7);
                if pulse > light && pulse > 0.05 {
                    mark = '◦';
                    light = pulse;
                    phase = 0.68;
                }
                let dust = 0.05 + 0.05 * (r * 60.0 - t * 0.7).sin().abs();
                let bg_light = 0.008 + light * 0.07 + dust * 0.02;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.50 + r * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        let counts = [1usize, 1, 2, 3, 5, 8];
        let mut bodies: Vec<Vec<[f32; 2]>> = Vec::with_capacity(radii.len());
        for (k, radius) in radii.iter().enumerate() {
            let speed = 1.10 / radius.powf(1.5) * (0.75 + unit(audio.energy) * 0.5);
            let base = t * speed + k as f32 * 0.9;
            let mut ring = Vec::with_capacity(counts[k]);
            for n in 0..counts[k] {
                let a = base + n as f32 * TAU / counts[k] as f32;
                ring.push([a.cos() * radius, a.sin() * radius]);
            }
            bodies.push(ring);
        }
        let mut c = Canvas::new(buf, area, visible, palette, theme);
        c.plot([0.0, 0.0], '✦', 0.12, 0.85);
        c.circle([0.0, 0.0], 0.05, '·', 0.14, 0.4);
        for (k, ring) in bodies.iter().enumerate() {
            for p in ring {
                c.circle(*p, 0.035, '·', 0.30 + k as f32 * 0.05, 0.35);
                c.plot(*p, '●', 0.40 + k as f32 * 0.06, 0.85);
            }
        }
        for k in 0..bodies.len() - 1 {
            let a = bodies[k][0];
            let b = bodies[k + 1][0];
            let align = a[1].atan2(a[0]).cos() * b[1].atan2(b[0]).cos()
                + a[1].atan2(a[0]).sin() * b[1].atan2(b[0]).sin();
            if align > 0.93 {
                c.line(a, b, '┈', 0.60, 0.30 + align * 0.4);
            }
        }
        for n in 0..48 {
            let a = n as f32 / 48.0 * TAU + t * 0.1;
            c.plot(
                [a.cos() * pulse_r, a.sin() * pulse_r],
                '·',
                0.66,
                0.25 + beat * 0.4,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn roche(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let (px, py, pr) = (-1.25_f32, 0.06_f32, 1.38_f32);
        let (mx, my) = (0.74_f32, -0.48_f32);
        let bass = unit(audio.bass);
        let debris = unit(audio.transients[1]).max(unit(audio.transients[2]));
        let stretch = 0.46 * (1.0 + bass * 0.22);
        let thin = 0.18 * (1.0 - bass * 0.10);
        let spin = -0.38_f32;
        let cr = spin.cos();
        let sr = spin.sin();
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.010;
                let rx = (x - mx) * cr - (y - my) * sr;
                let ry = (x - mx) * sr + (y - my) * cr;
                let md = (rx / stretch).hypot(ry / thin);
                if md < 1.0 {
                    let z = (1.0 - md * md).max(0.0).sqrt();
                    let seam = (rx * 16.0 + (ry * 22.0).sin() * 1.8 + t * 0.4).sin();
                    let crack = (-(seam / 0.10).powi(2)).exp();
                    let l = (0.18 + 0.62 * z + crack * 0.3).clamp(0.0, 1.0);
                    mark = if crack > 0.55 {
                        '╱'
                    } else if l > 0.55 {
                        '▓'
                    } else {
                        '▒'
                    };
                    light = l;
                    phase = 0.26 + crack * 0.12;
                    bg_light = 0.012 + l * 0.05;
                } else if (x - px).hypot(y - py) < pr {
                    let dx = (x - px) / pr;
                    let dy = (y - py) / pr;
                    let z = (1.0 - (dx * dx + dy * dy).clamp(0.0, 1.0)).max(0.0).sqrt();
                    let band = 0.5 + 0.5 * (dy * 24.0 + (dx * 6.0 + t * 0.1).sin() * 2.0).sin();
                    let storm = hash((x * 4.0) as i32, (y * 7.0) as i32);
                    let mut l = (body_light(dx, dy, z) * (0.70 + 0.30 * band)).clamp(0.0, 1.0);
                    if storm > 0.93 {
                        l = (l * 1.25).min(1.0);
                    }
                    mark = if l > 0.68 {
                        '█'
                    } else if l > 0.44 {
                        '▓'
                    } else if l > 0.21 {
                        '▒'
                    } else {
                        '░'
                    };
                    light = l;
                    phase = 0.20 + band * 0.08;
                    bg_light = 0.014 + l * 0.07;
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.55 + y * 0.03,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            let energy = unit(audio.energy);
            // The stream leaves the moon's near face and falls toward the planet.
            let start = [mx - 0.22, my + 0.12];
            let control = [0.56, my - 0.80];
            let end = [px + pr * 0.92, py + 0.52];
            for k in 0..140 {
                let u = (k as f32 + 0.5) / 140.0;
                let v = 1.0 - u;
                let h1 = hash(k, 7);
                let h2 = hash(k, 19);
                let spread = 0.04 + debris * 0.11 + u * 0.03;
                let p = [
                    v * v * start[0]
                        + 2.0 * v * u * control[0]
                        + u * u * end[0]
                        + (h1 - 0.5) * spread,
                    v * v * start[1]
                        + 2.0 * v * u * control[1]
                        + u * u * end[1]
                        + (h2 - 0.5) * spread * 1.5,
                ];
                let glyph = if h2 > 0.92 {
                    '●'
                } else if h2 > 0.55 {
                    '•'
                } else {
                    '·'
                };
                c.plot(
                    p,
                    glyph,
                    0.24 + h1 * 0.18,
                    0.22 + 0.62 * h2 * (0.65 + 0.35 * energy),
                );
            }
            for k in 0..30 {
                let h1 = hash(k, 31);
                let h2 = hash(k, 41);
                let ang = -0.85 + h1 * 1.7 + t * 0.05;
                let rad = 1.44 + h2 * 0.32;
                let p = [px + rad * ang.cos(), py + rad * ang.sin() * 0.95];
                let glyph = if h2 > 0.86 { '●' } else { '·' };
                c.plot(p, glyph, 0.30 + h1 * 0.12, 0.18 + h2 * 0.45);
            }
            c.plot([mx - 0.18, my + 0.06], '✶', 0.34, 0.9);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(style: usize, audio: &Reactivity, frames: usize) -> Buffer {
        let area = Rect::new(0, 0, 80, 30);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..frames {
            renderer.draw(
                &mut buf,
                area,
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

    #[test]
    fn ten_unique_scene_names() {
        assert_eq!(NAMES.len(), 10);
        let names: std::collections::HashSet<_> = NAMES.iter().collect();
        assert_eq!(names.len(), NAMES.len());
    }

    #[test]
    fn every_style_draws_and_styles_differ() {
        let quiet = Reactivity::default();
        let mut images = std::collections::HashSet::new();
        for style in 0..NAMES.len() {
            let image = symbols(&render(style, &quiet, 36));
            assert!(
                image.chars().any(|c| c != ' '),
                "style {style} drew nothing"
            );
            assert!(images.insert(image), "duplicate style {style}");
        }
        assert_eq!(images.len(), NAMES.len());
    }

    #[test]
    fn audio_changes_every_scene() {
        let quiet = Reactivity::default();
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.35,
            chroma: [0.8; 12],
            transients: [0.9; 4],
            waveform: [0.5; 32],
            novelty: 0.6,
            centroid: 0.5,
            flatness: 0.4,
            ..Reactivity::default()
        };
        for style in 0..NAMES.len() {
            let a = symbols(&render(style, &quiet, 36));
            let b = symbols(&render(style, &loud, 36));
            assert_ne!(a, b, "audio does not change style {style}");
        }
    }

    #[test]
    fn clipped_areas_leave_outside_cells_untouched() {
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(4, 3, 17, 9),
                Rect::new(0, 0, 1, 1),
                Rect::new(0, 0, 0, 0),
                Rect::new(120, 80, 4, 4),
                Rect::new(20, 10, 24, 14),
            ] {
                let mut buf = Buffer::empty(Rect::new(2, 1, 30, 18));
                for cell in &mut buf.content {
                    cell.set_char('X');
                }
                let mut renderer = Renderer::default();
                renderer.draw(
                    &mut buf,
                    area,
                    f32::NAN,
                    style,
                    0,
                    &[f32::NAN; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                let clip = area.intersection(buf.area);
                for y in buf.area.y..buf.area.bottom() {
                    for x in buf.area.x..buf.area.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "X", "style {style}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn same_state_renders_identically() {
        let audio = Reactivity {
            energy: 0.6,
            bass: 0.5,
            mid: 0.4,
            treble: 0.3,
            kick: 0.8,
            onset: 0.7,
            beat_phase: 0.25,
            ..Reactivity::default()
        };
        for style in 0..NAMES.len() {
            let a = symbols(&render(style, &audio, 24));
            let b = symbols(&render(style, &audio, 24));
            assert_eq!(a, b, "style {style} is not deterministic");
        }
    }
}
