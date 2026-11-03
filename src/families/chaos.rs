use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{palette::Theme, visuals::Reactivity};

const STRANDS: usize = 28;
const HISTORY: usize = 320;
const WIDTH: usize = 112;
const HEIGHT: usize = 64;
const CELLS: usize = WIDTH * HEIGHT;

fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn lorenz(p: [f32; 3], rho: f32, sigma: f32) -> [f32; 3] {
    [
        sigma * (p[1] - p[0]),
        p[0] * (rho - p[2]) - p[1],
        p[0] * p[1] - 8.0 / 3.0 * p[2],
    ]
}

struct Storm {
    trails: Vec<[[f32; 3]; HISTORY]>,
    head: usize,
    camera: f32,
    breath: f32,
}

impl Default for Storm {
    fn default() -> Self {
        let mut storm = Self {
            trails: (0..STRANDS)
                .map(|i| [[1.0 + i as f32 * 0.035, 0.3, 23.0]; HISTORY])
                .collect(),
            head: 0,
            camera: 0.0,
            breath: 0.0,
        };
        for _ in 0..850 {
            storm.step(0.006, 28.0, 10.0);
        }
        storm
    }
}

impl Storm {
    fn step(&mut self, dt: f32, rho: f32, sigma: f32) {
        let next = (self.head + 1) % HISTORY;
        for (i, trail) in self.trails.iter_mut().enumerate() {
            let p = trail[self.head];
            let a = lorenz(p, rho + i as f32 * 0.006, sigma);
            let midpoint = std::array::from_fn(|axis| p[axis] + a[axis] * dt * 0.5);
            let b = lorenz(midpoint, rho + i as f32 * 0.006, sigma);
            let q = std::array::from_fn(|axis| p[axis] + b[axis] * dt);
            trail[next] = if q.iter().all(|v| v.is_finite() && v.abs() < 100.0) {
                q
            } else {
                [1.0 + i as f32 * 0.035, 0.3, 23.0]
            };
        }
        self.head = next;
    }

    fn advance(&mut self, dt: f32, sound: [f32; 3], hit: f32) {
        if dt <= 0.0 {
            return;
        }
        // The camera tilt uses 0.61 turns per orbit; both close after 100 orbits.
        self.camera = (self.camera + dt * (0.17 + sound[1] * 0.28)).rem_euclid(100.0 * TAU);
        self.breath += (sound[0] - self.breath) * (1.0 - (-dt * 5.0).exp());
        if hit > 0.0 {
            for (i, trail) in self.trails.iter_mut().enumerate() {
                let angle = i as f32 * 2.399_963;
                trail[self.head][0] += angle.cos() * hit * 2.6;
                trail[self.head][1] += angle.sin() * hit * 3.5;
                trail[self.head][2] += hit * (i as f32 * 0.8).sin() * 2.0;
            }
        }
        let duration = dt * (1.3 + sound[2] * 0.4);
        let steps = (duration / 0.006).ceil().max(1.0) as usize;
        for _ in 0..steps {
            self.step(
                duration / steps as f32,
                27.0 + sound[0] * 4.0,
                9.6 + sound[1],
            );
        }
    }

    fn render(&self, buf: &mut Buffer, area: Rect, palette: usize, theme: &Theme) {
        let visible = area.intersection(buf.area);
        let mut ink =
            vec![(0_u8, 0.0_f32, 0.0_f32); visible.width as usize * visible.height as usize];
        let (sin, cos) = self.camera.sin_cos();
        let tilt = (self.camera * 0.61).sin() * 0.30;
        let scale = (area.width as f32 * 0.88).min(area.height as f32 * 2.0 * 0.88)
            * (1.0 + self.breath * 0.065);
        for (strand, trail) in self.trails.iter().enumerate() {
            for age in 0..HISTORY {
                let p = trail[(self.head + HISTORY - age) % HISTORY];
                let x = p[0] / 25.0;
                let y = (p[2] - 25.0) / 25.0;
                let z = p[1] / 32.0;
                let rx = x * cos + z * sin;
                let depth = z * cos - x * sin;
                let ry = y * tilt.cos() - depth * tilt.sin();
                let perspective = 1.0 / (1.0 + depth * 0.22);
                let px = area.width as f32 + rx * scale * perspective;
                let py = area.height as f32 * 2.0 - ry * scale * perspective;
                if px < 0.0
                    || py < 0.0
                    || px >= area.width as f32 * 2.0
                    || py >= area.height as f32 * 4.0
                {
                    continue;
                }
                let sx = px as u16;
                let sy = py as u16;
                let x = area.x.saturating_add(sx / 2);
                let y = area.y.saturating_add(sy / 4);
                if !visible.contains((x, y).into()) {
                    continue;
                }
                let cell = &mut ink
                    [(y - visible.y) as usize * visible.width as usize + (x - visible.x) as usize];
                let bit = [[0, 1, 2, 6], [3, 4, 5, 7]][(sx % 2) as usize][(sy % 4) as usize];
                cell.0 |= 1 << bit;
                let fade = 1.0 - age as f32 / HISTORY as f32;
                let intensity = (0.24 + fade * 0.52 + (0.5 - depth * 0.25) * 0.24).clamp(0.1, 1.0);
                if intensity > cell.1 {
                    cell.1 = intensity;
                    cell.2 = 0.56
                        + strand as f32 / STRANDS as f32 * 0.24
                        + (y - area.y) as f32 / area.height as f32 * 0.12;
                }
            }
        }
        let bg = Color::Rgb(
            theme.background[0],
            theme.background[1],
            theme.background[2],
        );
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                let (dots, intensity, phase) = ink
                    [(y - visible.y) as usize * visible.width as usize + (x - visible.x) as usize];
                let cell = &mut buf[(x, y)];
                cell.reset();
                cell.set_bg(bg);
                if dots == 0 {
                    cell.set_char(' ');
                } else {
                    cell.set_char(char::from_u32(0x2800 + dots as u32).unwrap())
                        .set_fg(theme.sample(palette, phase, intensity));
                }
            }
        }
    }
}

struct Garden {
    field: Vec<[f32; 2]>,
    next: Vec<[f32; 2]>,
    remainder: f32,
    serial: usize,
}

impl Default for Garden {
    fn default() -> Self {
        let mut garden = Self {
            field: vec![[1.0, 0.0]; CELLS],
            next: vec![[1.0, 0.0]; CELLS],
            remainder: 0.0,
            serial: 0,
        };
        for i in 0..19 {
            let angle = i as f32 * 2.399_963;
            let radius = (i as f32 / 19.0).sqrt();
            garden.seed(
                (WIDTH as f32 * (0.5 + angle.cos() * radius * 0.38)) as usize,
                (HEIGHT as f32 * (0.5 + angle.sin() * radius * 0.36)) as usize,
                2 + i % 2,
            );
        }
        // Start with living colonies, not an empty field waiting to react.
        for _ in 0..220 {
            garden.step(0.031, 0.059, 0.90, 0.45);
        }
        garden
    }
}

impl Garden {
    fn seed(&mut self, x: usize, y: usize, radius: usize) {
        for yy in y.saturating_sub(radius)..=(y + radius).min(HEIGHT - 1) {
            for xx in x.saturating_sub(radius)..=(x + radius).min(WIDTH - 1) {
                let dx = xx as f32 - x as f32;
                let dy = yy as f32 - y as f32;
                if dx * dx + dy * dy <= (radius * radius) as f32 {
                    self.field[yy * WIDTH + xx] = [0.35, 0.65];
                }
            }
        }
    }

    fn step(&mut self, feed: f32, kill: f32, du: f32, dv: f32) {
        for y in 0..HEIGHT {
            let up = if y == 0 { HEIGHT - 1 } else { y - 1 } * WIDTH;
            let down = (y + 1) % HEIGHT * WIDTH;
            let row = y * WIDTH;
            for x in 0..WIDTH {
                let left = if x == 0 { WIDTH - 1 } else { x - 1 };
                let right = (x + 1) % WIDTH;
                let [u, v] = self.field[row + x];
                let mut lap = [-u, -v];
                for (index, weight) in [
                    (row + left, 0.2),
                    (row + right, 0.2),
                    (up + x, 0.2),
                    (down + x, 0.2),
                    (up + left, 0.05),
                    (up + right, 0.05),
                    (down + left, 0.05),
                    (down + right, 0.05),
                ] {
                    lap[0] += self.field[index][0] * weight;
                    lap[1] += self.field[index][1] * weight;
                }
                let reaction = u * v * v;
                self.next[row + x] = [
                    (u + du * lap[0] - reaction + feed * (1.0 - u)).clamp(0.0, 1.0),
                    (v + dv * lap[1] + reaction - (feed + kill) * v).clamp(0.0, 1.0),
                ];
            }
        }
        std::mem::swap(&mut self.field, &mut self.next);
    }

    fn advance(&mut self, dt: f32, sound: [f32; 3], hit: f32) {
        if hit > 0.0 {
            self.serial = self.serial.wrapping_add(1);
            let angle = self.serial as f32 * 2.399_963;
            let radius = 0.18 + (self.serial % 7) as f32 * 0.035;
            self.seed(
                (WIDTH as f32 * (0.5 + angle.cos() * radius)) as usize,
                (HEIGHT as f32 * (0.5 + angle.sin() * radius)) as usize,
                2 + (hit * 2.0) as usize,
            );
        }
        self.remainder += dt * 720.0;
        let steps = self.remainder.floor().clamp(0.0, 4096.0) as usize;
        self.remainder -= steps as f32;
        for _ in 0..steps {
            self.step(
                0.029 + sound[0] * 0.006,
                0.0575 + sound[1] * 0.003,
                0.90 + sound[2] * 0.06,
                0.45 + sound[2] * 0.03,
            );
        }
    }

    fn sample(&self, x: f32, y: f32, palette: usize, theme: &Theme) -> Color {
        if !(0.0..(WIDTH - 1) as f32).contains(&x) || !(0.0..(HEIGHT - 1) as f32).contains(&y) {
            return theme.sample(palette, 0.0, 0.0);
        }
        let ix = x as usize;
        let iy = y as usize;
        let a = self.field[iy * WIDTH + ix];
        let b = self.field[iy * WIDTH + ix + 1];
        let c = self.field[(iy + 1) * WIDTH + ix];
        let d = self.field[(iy + 1) * WIDTH + ix + 1];
        let interpolate = |channel: usize| {
            let top = a[channel] + (b[channel] - a[channel]) * x.fract();
            let bottom = c[channel] + (d[channel] - c[channel]) * x.fract();
            top + (bottom - top) * y.fract()
        };
        let u = interpolate(0);
        let v = interpolate(1);
        let edge = ((b[1] - a[1]).hypot(c[1] - a[1]) * 8.0).min(1.0);
        let pigment = (v * 3.4).clamp(0.0, 1.0);
        let intensity = (pigment * 0.58 + edge * 0.42).min(1.0);
        theme.sample(palette, 0.46 + u * 0.36 - v * 0.55 + edge * 0.12, intensity)
    }

    fn render(&self, buf: &mut Buffer, area: Rect, palette: usize, theme: &Theme) {
        let visible = area.intersection(buf.area);
        let step_x = (WIDTH - 1) as f32 / area.width as f32;
        let step_y = (HEIGHT - 1) as f32 / (area.height as f32 * 2.0);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                let gx = ((x - area.x) as f32 + 0.5) * step_x;
                let gy = ((y - area.y) as f32 * 2.0 + 0.5) * step_y;
                let top = self.sample(gx, gy, palette, theme);
                let bottom = self.sample(gx, gy + step_y, palette, theme);
                let cell = &mut buf[(x, y)];
                cell.reset();
                if top == bottom {
                    cell.set_char(' ').set_bg(top);
                } else {
                    cell.set_char('▀').set_fg(top).set_bg(bottom);
                }
            }
        }
    }
}

#[derive(Default)]
pub struct Renderer {
    // Each world precomputes its own history. Build only the world on screen:
    // one shared frame budget should not pay for the unused style.
    storm: Option<Storm>,
    garden: Option<Garden>,
    last_time: [Option<f32>; 2],
    sound: [[f32; 3]; 2],
    attack: [f32; 2],
    cooldown: [f32; 2],
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
        let style = style % 2;
        let dt = if time.is_finite() {
            let dt =
                self.last_time[style].map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.05));
            self.last_time[style] = Some(time);
            dt
        } else {
            0.0
        };
        let target = [
            unit(audio.bass).max(unit(bands[2]) * 0.6),
            unit(audio.mid).max(unit(bands[14]) * 0.5),
            unit(audio.treble).max(unit(bands[27]) * 0.5),
        ];
        for (value, target) in self.sound[style].iter_mut().zip(target) {
            *value += (target - *value) * (1.0 - (-dt * 4.0).exp());
        }
        let attack = unit(audio.kick)
            .max(unit(audio.transients[0]))
            .max(unit(audio.transients[2]) * 0.7)
            .max(unit(audio.onset) * 0.7);
        self.cooldown[style] = (self.cooldown[style] - dt).max(0.0);
        let hit = if dt > 0.0
            && attack > 0.18
            && attack - self.attack[style] > 0.10
            && self.cooldown[style] == 0.0
        {
            self.cooldown[style] = 0.12;
            attack
        } else {
            0.0
        };
        self.attack[style] = attack;
        if style == 0 {
            self.storm
                .get_or_insert_with(Default::default)
                .advance(dt, self.sound[style], hit);
        } else {
            self.garden
                .get_or_insert_with(Default::default)
                .advance(dt, self.sound[style], hit);
        }
        if area.width == 0 || area.height == 0 || area.intersection(buf.area).is_empty() {
            return;
        }
        if style == 0 {
            if let Some(storm) = &self.storm {
                storm.render(buf, area, palette, theme);
            }
        } else if let Some(garden) = &self.garden {
            garden.render(buf, area, palette, theme);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_tilt_is_continuous_across_camera_turns() {
        let mut storm = Storm {
            camera: TAU - 0.0001,
            ..Default::default()
        };
        let before = (storm.camera * 0.61).sin();
        storm.advance(0.001, [0.0; 3], 0.0);
        assert!(((storm.camera * 0.61).sin() - before).abs() < 0.001);
    }

    #[test]
    fn integrated_worlds_evolve_stay_bounded_and_respond_to_music() {
        let mut storm = Storm::default();
        let mut garden = Garden::default();
        let initial = garden.field.clone();
        for frame in 0..480 {
            let hit = if frame % 30 == 0 { 1.0 } else { 0.0 };
            storm.advance(1.0 / 60.0, [1.0; 3], hit);
            garden.advance(1.0 / 60.0, [1.0; 3], hit);
        }
        assert_eq!(storm.trails.len(), STRANDS);
        assert!(storm
            .trails
            .iter()
            .flatten()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 100.0));
        assert_eq!(garden.field.len(), CELLS);
        assert!(garden
            .field
            .iter()
            .flatten()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        let changed = garden
            .field
            .iter()
            .zip(&initial)
            .filter(|(a, b)| (a[1] - b[1]).abs() > 0.05)
            .count();
        assert!(
            changed > CELLS / 5,
            "garden must grow across the field: {changed}"
        );
        let mut quiet = Garden::default();
        let mut struck = Garden::default();
        struck.advance(1.0 / 60.0, [1.0; 3], 1.0);
        quiet.advance(1.0 / 60.0, [0.0; 3], 0.0);
        assert_ne!(struck.field, quiet.field);
        let mut calm = Storm::default();
        let before = calm.trails[0][calm.head];
        calm.advance(1.0 / 60.0, [0.0; 3], 0.0);
        let mut kicked = Storm::default();
        kicked.advance(1.0 / 60.0, [1.0; 3], 1.0);
        assert_ne!(before, calm.trails[0][calm.head]);
        assert_ne!(calm.trails[0][calm.head], kicked.trails[0][kicked.head]);
    }

    #[test]
    fn chemical_seed_propagates_beyond_its_injection_site() {
        let mut garden = Garden {
            field: vec![[1.0, 0.0]; CELLS],
            next: vec![[1.0, 0.0]; CELLS],
            remainder: 0.0,
            serial: 0,
        };
        garden.seed(WIDTH / 2, HEIGHT / 2, 3);
        for _ in 0..1200 {
            garden.step(0.031, 0.059, 0.90, 0.45);
        }
        let spread = garden
            .field
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let dx = (i % WIDTH) as f32 - WIDTH as f32 * 0.5;
                let dy = (i / WIDTH) as f32 - HEIGHT as f32 * 0.5;
                p[1] > 0.08 && dx * dx + dy * dy > 16.0
            })
            .count();
        assert!(spread > 40, "localized reaction must propagate: {spread}");
    }

    #[test]
    fn garden_covers_wide_and_tall_viewports_without_bars() {
        let mut garden = Garden::default();
        garden.field.fill([0.5, 0.3]);
        let theme = Theme::default();
        let background = theme.sample(0, 0.0, 0.0);
        for area in [Rect::new(3, 5, 180, 30), Rect::new(3, 5, 30, 90)] {
            let mut buf = Buffer::empty(area);
            garden.render(&mut buf, area, 0, &theme);
            assert!(buf.content.iter().all(|cell| cell.bg != background));
        }
    }

    #[test]
    fn distinct_images_clip_tiny_offset_and_invalid_clocks() {
        let mut renderer = Renderer::default();
        let theme = Theme::default();
        let audio = Reactivity::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut storm = Buffer::empty(area);
        let mut garden = Buffer::empty(area);
        renderer.draw(&mut storm, area, 0.0, 0, 0, &[0.0; 32], &audio, &theme);
        renderer.draw(&mut garden, area, 0.0, 1, 0, &[0.0; 32], &audio, &theme);
        assert_ne!(storm, garden);
        assert!(storm.content.iter().filter(|c| c.symbol() == " ").count() > 3500);
        assert!(storm.content.iter().filter(|c| c.symbol() != " ").count() > 400);
        let bounds = Rect::new(5, 7, 12, 8);
        let mut buf = Buffer::empty(bounds);
        for style in 0..2 {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(6, 8, 1, 1),
                Rect::new(0, 0, 10, 10),
                Rect::new(100, 100, 2, 2),
            ] {
                buf.reset();
                for time in [0.0, 1e6, f32::NAN, -2.0] {
                    renderer.draw(&mut buf, area, time, style, 0, &[0.0; 32], &audio, &theme);
                }
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        if !area.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), " ");
                        }
                    }
                }
            }
        }
        assert!(
            renderer
                .storm
                .as_ref()
                .expect("style 0 builds the storm")
                .camera
                < 0.1
        );
    }

    #[test]
    fn only_the_drawn_world_is_initialized() {
        let area = Rect::new(0, 0, 40, 12);
        let theme = Theme::default();
        let audio = Reactivity::default();
        for (style, storm, garden) in [(0, true, false), (1, false, true)] {
            let mut renderer = Renderer::default();
            assert!(renderer.storm.is_none() && renderer.garden.is_none());
            let mut buf = Buffer::empty(area);
            renderer.draw(&mut buf, area, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            assert_eq!(renderer.storm.is_some(), storm, "style {style}");
            assert_eq!(renderer.garden.is_some(), garden, "style {style}");
        }
    }

    #[test]
    #[ignore = "manual release benchmark at 160x50"]
    fn frame_budget() {
        let mut renderer = Renderer::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut buf = Buffer::empty(area);
        for style in 0..2 {
            let start = std::time::Instant::now();
            for frame in 0..300 {
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &Reactivity {
                        bass: 0.7,
                        mid: 0.4,
                        treble: 0.6,
                        kick: if frame % 30 == 0 { 1.0 } else { 0.0 },
                        ..Default::default()
                    },
                    &Theme::default(),
                );
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0 / 300.0;
            eprintln!("chaos style {style}: {ms:.3} ms/frame");
            assert!(ms < 16.0, "release frame budget exceeded");
        }
    }
}
