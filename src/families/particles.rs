use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};

pub const NAMES: &[&str] = &[
    "Gravity Well",
    "Magnetic Lines",
    "Vortex Ink",
    "Rain Streaks",
    "Snow Drift",
    "Spark Forge",
    "Comet Trails",
    "Firework Score",
    "Fluid Shoal",
    "Solar Wind",
];

#[derive(Default)]
pub struct Renderer {
    particles: Vec<Particle>,
    last_time: Option<f32>,
    style: Option<usize>,
    serial: u32,
    emission: f32,
    attack: f32,
    cooldown: f32,
    clock: f32,
}

struct Particle {
    p: [f32; 2],
    trail: Vec<[f32; 2]>,
    v: [f32; 2],
    age: f32,
    life: f32,
    seed: f32,
    lane: usize,
}

const LIMIT: usize = 256;
const MAX_DT: f32 = 0.08;
const STEP: f32 = 0.01;

fn noise(n: u32) -> f32 {
    let mut x = n.wrapping_add(0x9e3779b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85ebca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2ae35);
    (x ^ (x >> 16)) as f32 / u32::MAX as f32
}

impl Renderer {
    fn emit(&mut self, style: usize, count: usize, strength: f32, audio: &Reactivity) {
        let burst = [
            0.22 + noise(self.serial + 7) * 0.56,
            0.24 + noise(self.serial + 9) * 0.32,
        ];
        let pitch = audio
            .chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i);
        for _ in 0..count.min(LIMIT - self.particles.len()) {
            self.serial = self.serial.wrapping_add(1);
            let s = noise(self.serial);
            let q = noise(self.serial.wrapping_add(391));
            let angle = s * std::f32::consts::TAU;
            let lane = (self.serial as usize) % 32;
            let p;
            let mut v = [0.0, 0.0];
            let mut life = 4.0 + q * 3.0;
            match style {
                0 => {
                    p = [0.5 + angle.cos() * 0.44, 0.5 + angle.sin() * 0.40];
                    v = [-angle.sin() * 0.16, angle.cos() * 0.16];
                }
                1 => {
                    p = [0.28 + (q - 0.5) * 0.07, 0.5 + (s - 0.5) * 0.10];
                    life = 5.0;
                }
                2 => {
                    p = [0.5 + angle.cos() * 0.45, 0.5 + angle.sin() * 0.45];
                    life = 6.0;
                }
                3 => {
                    p = [s * 1.2 - 0.1, -q * 0.2];
                    v = [0.09, 0.65 + q * 0.38];
                    life = 2.0;
                }
                4 => {
                    p = [s, -q * 0.1];
                    v = [0.0, 0.06 + q * 0.06];
                    life = 14.0;
                }
                5 => {
                    p = [0.5 + (s - 0.5) * 0.15, 0.79];
                    v = [(q - 0.5) * 0.75, -0.35 - s * strength * 0.65];
                    life = 0.35 + q * 0.65;
                }
                6 => {
                    p = [-0.04, q * 0.7];
                    v = [0.23 + s * 0.24, 0.08];
                    life = 5.0;
                }
                7 => {
                    p = burst;
                    let speed = (0.12 + q * 0.22) * (0.6 + strength);
                    v = [angle.cos() * speed, angle.sin() * speed];
                    life = 0.65 + q * 0.65;
                }
                8 => {
                    p = [s, 0.5 + (q - 0.5) * 0.4];
                    v = [0.15, 0.0];
                    life = 8.0;
                }
                _ => {
                    p = [0.12, 0.5 + (q - 0.5) * 0.16];
                    v = [0.24 + s * 0.18, (q - 0.5) * 0.1];
                    life = 4.0;
                }
            }
            // Chroma selects a stream's pigment, not an unrelated global pulse.
            self.particles.push(Particle {
                p,
                trail: Vec::with_capacity(8),
                v,
                age: 0.0,
                life,
                seed: (s + pitch as f32 / 12.0).fract(),
                lane,
            });
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
        if self.style != Some(style) {
            self.particles.clear();
            self.emission = 0.0;
            self.attack = 0.0;
            self.cooldown = 0.0;
            self.style = Some(style);
            self.last_time = None;
            if style != 5 && style != 7 {
                self.emit(style, 28, 0.0, audio);
            }
        }
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, MAX_DT));
        let dt = if dt.is_finite() { dt } else { 0.0 };
        self.last_time = Some(time);
        self.clock += dt;
        self.cooldown = (self.cooldown - dt).max(0.0);
        let attack = if style == 5 {
            audio.transients[3].max(audio.onset * audio.treble)
        } else {
            audio.transients[0].max(audio.kick).max(audio.novelty * 0.8)
        }
        .clamp(0.0, 1.0);
        if (style == 5 || style == 7)
            && attack > 0.14
            && attack - self.attack > 0.08
            && self.cooldown == 0.0
        {
            self.emit(style, if style == 5 { 14 } else { 38 }, attack, audio);
            self.cooldown = 0.10;
        }
        self.attack = attack;
        if style == 5 {
            // A forge keeps a low ember stream between air attacks. Without it
            // the scene is black whenever the track has no fourth transient.
            self.emission += dt * (36.0 + audio.energy.clamp(0.0, 1.0) * 36.0);
            let count = self.emission as usize;
            self.emission -= count as f32;
            self.emit(style, count, 0.0, audio);
        }
        if style != 5 && style != 7 {
            let rate = match style {
                3 => 30.0,
                4 => 8.0,
                6 => 7.0,
                _ => 15.0,
            };
            self.emission += dt * rate * (0.32 + audio.energy.clamp(0.0, 1.0) * 1.3);
            let count = self.emission as usize;
            self.emission -= count as f32;
            self.emit(style, count, 0.0, audio);
        }
        let steps = (dt / STEP).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for p in &mut self.particles {
            if dt > 0.0 {
                if p.trail.len() == 8 {
                    p.trail.remove(0);
                }
                p.trail.push(p.p);
            }
            p.age += dt;
            for _ in 0..steps {
                let x = p.p[0] - 0.5;
                let y = p.p[1] - 0.5;
                let spectrum = bands[p.lane].clamp(0.0, 1.0);
                match style {
                    0 => {
                        let r = (x * x + y * y + 0.025).sqrt();
                        let f = (0.023 + audio.bass * 0.018) / (r * r * r);
                        p.v[0] += (-x * f - p.v[0] * 0.12) * h;
                        p.v[1] += (-y * f - p.v[1] * 0.12) * h;
                    }
                    1 => {
                        // Dipole field: leave the left pole and curve back to the right pole.
                        let mut field = [0.0, 0.0];
                        let spacing = 0.22 + audio.mid.clamp(0.0, 1.0) * 0.06;
                        for (pole, sign) in [(0.5 - spacing, 1.0), (0.5 + spacing, -1.0)] {
                            let dx = p.p[0] - pole;
                            let d = (dx * dx + y * y + 0.012).powf(1.5);
                            field[0] += sign * dx / d;
                            field[1] += sign * y / d;
                        }
                        let norm = field[0].hypot(field[1]).max(0.001);
                        let speed = 0.16 + audio.centroid.clamp(0.0, 1.0) * 0.12;
                        p.v = [field[0] / norm * speed, field[1] / norm * speed];
                    }
                    2 => {
                        let r = x.hypot(y).max(0.04);
                        let swirl = 0.14 + audio.mid * 0.12;
                        let inward = 0.018 + audio.bass * 0.025 + audio.novelty * 0.018;
                        p.v = [(-y * swirl - x * inward) / r, (x * swirl - y * inward) / r];
                    }
                    3 => {
                        p.v[0] = 0.07 + audio.mid * 0.13;
                    }
                    4 => {
                        p.v[0] = 0.025
                            + (self.clock * 0.7 + p.seed * 20.0 + p.p[1] * 4.0).sin() * 0.045
                            + audio.mid * 0.03;
                    }
                    5 => {
                        p.v[1] += 0.95 * h;
                        p.v[0] *= 1.0 - h * 0.6;
                    }
                    6 => {
                        p.v[1] += ((p.p[0] * 5.0 + p.seed * 6.0).sin() * 0.12
                            + audio.waveform[p.lane] * 0.10)
                            * h;
                    }
                    7 => {
                        p.v[1] += 0.23 * h;
                        p.v[0] *= 1.0 - h * 0.8;
                        p.v[1] *= 1.0 - h * 0.45;
                    }
                    8 => {
                        let a = (p.p[0] * 6.0 + self.clock * 0.4).sin();
                        let b = (p.p[1] * 7.0 - self.clock * 0.3).cos();
                        p.v[0] += (0.13 + b * 0.06 - p.v[0]) * h * 3.0;
                        p.v[1] += (a * 0.12 + audio.waveform[p.lane] * spectrum * 0.12 - p.v[1])
                            * h
                            * 3.0;
                    }
                    _ => {
                        let bend = (p.p[0] * 9.0 + self.clock + p.seed * 2.0).sin();
                        p.v[1] += (bend * (0.10 + audio.treble * 0.16) - p.v[1]) * h * 2.0;
                        p.v[0] += (0.35 + audio.centroid * 0.15 - p.v[0]) * h;
                    }
                }
                p.p[0] += p.v[0] * h;
                p.p[1] += p.v[1] * h;
            }
        }
        self.particles.retain(|p| {
            p.age < p.life && p.p[0] > -0.3 && p.p[0] < 1.3 && p.p[1] > -0.3 && p.p[1] < 1.3
        });
        let visible = area.intersection(buf.area);
        if visible.is_empty() || area.is_empty() {
            return;
        }
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)]
                    .set_char(' ')
                    .set_bg(theme.sample(palette, 0.0, 0.0));
            }
        }
        for p in &self.particles {
            let fade = ((p.life - p.age) / 0.45).clamp(0.0, 1.0);
            let intensity =
                (0.58 + bands[p.lane].clamp(0.0, 1.0) * 0.28 + audio.energy.clamp(0.0, 1.0) * 0.10)
                    .clamp(0.0, 0.96)
                    * fade;
            let length = match style {
                3 => 0.045,
                4 => 0.0,
                5 | 7 => 0.025,
                6 => 0.12,
                _ => 0.028,
            };
            let speed = p.v[0].hypot(p.v[1]).max(0.001);
            let samples = if style == 4 { 1 } else { 7 };
            for i in (0..samples).rev() {
                let u = i as f32 / samples as f32;
                let pos = if matches!(style, 0 | 1 | 2 | 6 | 8 | 9) && i > 0 && !p.trail.is_empty()
                {
                    p.trail[p.trail.len().saturating_sub(1 + i.min(p.trail.len() - 1))]
                } else {
                    [
                        p.p[0] - p.v[0] / speed * length * u,
                        p.p[1] - p.v[1] / speed * length * u,
                    ]
                };
                let x = area.x as f32 + pos[0] * area.width as f32;
                let y = area.y as f32 + pos[1] * area.height as f32;
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                let x = x.floor() as i32;
                let y = y.floor() as i32;
                if x < visible.x as i32
                    || x >= visible.right() as i32
                    || y < visible.y as i32
                    || y >= visible.bottom() as i32
                {
                    continue;
                }
                let glyph = match style {
                    3 => '╲',
                    4 => {
                        if p.seed > 0.7 {
                            '✧'
                        } else {
                            '·'
                        }
                    }
                    5 | 7 => {
                        if i == 0 {
                            '•'
                        } else {
                            '·'
                        }
                    }
                    8 => {
                        if p.v[1] > 0.05 {
                            '╲'
                        } else {
                            '─'
                        }
                    }
                    _ => {
                        if i == 0 {
                            '•'
                        } else {
                            '·'
                        }
                    }
                };
                buf[(x as u16, y as u16)]
                    .set_char(glyph)
                    .set_fg(theme.sample(
                        palette,
                        p.seed + p.age * 0.025,
                        intensity * (1.0 - u * 0.75),
                    ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(
        renderer: &mut Renderer,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        style: usize,
        audio: &Reactivity,
    ) {
        renderer.draw(
            buf,
            area,
            time,
            style,
            0,
            &[0.4; 32],
            audio,
            &Theme::default(),
        );
    }

    #[test]
    fn spark_forge_keeps_embers_between_air_attacks() {
        let area = Rect::new(0, 0, 100, 35);
        let mut buffer = Buffer::empty(area);
        let mut renderer = Renderer::default();
        let quiet = Reactivity {
            energy: 0.5,
            ..Reactivity::default()
        };
        for i in 0..=120 {
            render(&mut renderer, &mut buffer, area, i as f32 / 60.0, 5, &quiet);
        }
        let ink = buffer
            .content
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .count();
        assert!(
            ink > 25,
            "spark forge went dark between attacks: {ink} cells"
        );
    }

    #[test]
    fn every_style_changes_geometry_with_audio() {
        let area = Rect::new(0, 0, 100, 35);
        let quiet = Reactivity::default();
        let active = Reactivity {
            energy: 1.0,
            bass: 0.9,
            mid: 0.8,
            treble: 0.9,
            onset: 1.0,
            kick: 1.0,
            novelty: 0.9,
            centroid: 0.8,
            waveform: [0.6; 32],
            transients: [1.0; 4],
            ..quiet
        };
        for (style, name) in NAMES.iter().enumerate() {
            let mut a = Renderer::default();
            let mut b = Renderer::default();
            let mut ab = Buffer::empty(area);
            let mut bb = Buffer::empty(area);
            for i in 0..90 {
                let loud = if i % 18 < 3 {
                    active
                } else {
                    Reactivity {
                        onset: 0.0,
                        kick: 0.0,
                        novelty: 0.0,
                        transients: [0.0; 4],
                        ..active
                    }
                };
                render(&mut a, &mut ab, area, i as f32 / 60.0, style, &quiet);
                render(&mut b, &mut bb, area, i as f32 / 60.0, style, &loud);
            }
            let shape_a: Vec<_> = ab.content.iter().map(|c| c.symbol()).collect();
            let shape_b: Vec<_> = bb.content.iter().map(|c| c.symbol()).collect();
            assert_ne!(shape_a, shape_b, "{}", name);
        }
    }

    #[test]
    #[ignore = "release performance measurement"]
    fn release_frame_timing() {
        let area = Rect::new(0, 0, 160, 50);
        let audio = Reactivity {
            energy: 1.0,
            bass: 0.8,
            mid: 0.7,
            treble: 0.8,
            kick: 1.0,
            onset: 1.0,
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        for (style, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let mut buf = Buffer::empty(area);
            render(&mut r, &mut buf, area, 0.0, style, &audio);
            let start = std::time::Instant::now();
            for i in 1..=300 {
                r.emit(style, LIMIT - r.particles.len(), 1.0, &audio);
                render(&mut r, &mut buf, area, i as f32 / 60.0, style, &audio);
            }
            println!(
                "{}: {:.3} ms/frame",
                name,
                start.elapsed().as_secs_f64() * 1000.0 / 300.0
            );
        }
    }

    #[test]
    fn clipping_respects_offset_tiny_and_disjoint_rectangles() {
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(12, 8, 1, 1),
                Rect::new(13, 9, 12, 9),
                Rect::new(100, 100, 2, 2),
            ] {
                let mut buf = Buffer::empty(Rect::new(10, 7, 8, 6));
                for cell in &mut buf.content {
                    cell.set_char('X');
                }
                let mut renderer = Renderer::default();
                render(
                    &mut renderer,
                    &mut buf,
                    area,
                    0.0,
                    style,
                    &Reactivity::default(),
                );
                let clip = area.intersection(buf.area);
                for y in buf.area.y..buf.area.bottom() {
                    for x in buf.area.x..buf.area.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "X");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn simulation_is_bounded_and_long_gaps_are_capped() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let audio = Reactivity {
            energy: 1.0,
            bass: 1.0,
            ..Reactivity::default()
        };
        for style in 0..NAMES.len() {
            let mut r = Renderer::default();
            for i in 0..400 {
                render(
                    &mut r,
                    &mut buf,
                    Rect::new(0, 0, 80, 24),
                    i as f32,
                    style,
                    &audio,
                );
            }
            render(
                &mut r,
                &mut buf,
                Rect::new(0, 0, 80, 24),
                f32::NAN,
                style,
                &audio,
            );
            assert!(r.particles.len() <= LIMIT);
            assert!(r.clock < 33.0);
            assert!(
                r.particles
                    .iter()
                    .all(|p| p.trail.len() <= 8
                        && p.p.iter().chain(p.v.iter()).all(|v| v.is_finite()))
            );
        }
    }

    #[test]
    fn all_compositions_have_distinct_symbol_geometry() {
        let area = Rect::new(0, 0, 120, 40);
        let mut shapes = Vec::new();
        for (style, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let mut buf = Buffer::empty(area);
            for i in 0..120 {
                let audio = if i % 24 == 0 {
                    Reactivity {
                        kick: 0.7,
                        onset: 0.8,
                        treble: 0.8,
                        transients: [0.7; 4],
                        ..Reactivity::default()
                    }
                } else {
                    Reactivity::default()
                };
                render(&mut r, &mut buf, area, i as f32 / 60.0, style, &audio);
            }
            let shape: Vec<_> = buf.content.iter().map(|c| c.symbol().to_owned()).collect();
            assert!(shape.iter().any(|s| s != " "), "{} empty", name);
            for (other, previous) in shapes.iter().enumerate() {
                assert_ne!(&shape, previous, "{} equals {}", name, NAMES[other]);
            }
            shapes.push(shape);
        }
    }

    #[test]
    fn bursts_are_attack_driven_and_short_lived() {
        let mut r = Renderer::default();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let quiet = Reactivity::default();
        render(&mut r, &mut buf, area, 0.0, 7, &quiet);
        assert!(r.particles.is_empty());
        let attack = Reactivity { kick: 1.0, ..quiet };
        render(&mut r, &mut buf, area, 0.02, 7, &attack);
        assert_eq!(r.particles.len(), 38);
        render(&mut r, &mut buf, area, 0.04, 7, &attack);
        assert_eq!(r.particles.len(), 38);
        for i in 0..110 {
            render(&mut r, &mut buf, area, 0.06 + i as f32 * 0.02, 7, &quiet);
        }
        assert!(r.particles.is_empty());
    }

    #[test]
    fn precipitation_and_curved_flow_have_distinct_trajectories() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let quiet = Reactivity::default();
        let mut velocities = Vec::new();
        for style in [0, 1, 2, 3, 4, 8, 9] {
            let mut r = Renderer::default();
            for i in 0..30 {
                render(&mut r, &mut buf, area, i as f32 / 60.0, style, &quiet);
            }
            let p = &r.particles[0];
            if style == 3 {
                assert!(p.v[1] > 0.6);
            }
            if style == 4 {
                assert!(p.v[1] < 0.13 && p.v[1] > 0.0);
            }
            velocities.push(p.v);
        }
        for i in 0..velocities.len() {
            for j in i + 1..velocities.len() {
                assert_ne!(velocities[i], velocities[j]);
            }
        }
    }
}
