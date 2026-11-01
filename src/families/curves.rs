//! Eleven mono-audio and mathematical curve instruments, rasterized as braille.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
};

pub const NAMES: &[&str] = &[
    "Vector Scope",
    "Phase Portrait",
    "Rose Garden",
    "Epicycle Machine",
    "Spirograph",
    "Harmonograph",
    "Pendulum Trace",
    "Cycloid Scroll",
    "Standing Ribbons",
    "Orbit Rosette",
    "Superformula",
];
const SAMPLES: usize = 640;
const TRAIL: usize = 480;
// Raster storage is bounded even for pathological terminal dimensions.
const MAX_CELLS: usize = 262_144;
type Point = [f32; 2];

#[derive(Clone, Copy, Default)]
struct Dot {
    mask: u8,
    phase: f32,
    light: f32,
}
#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    motion: f32,
    pendulum_phase: f32,
    controls: [f32; 8],
    spectrum: [f32; 32],
    chroma: [f32; 12],
    trail: VecDeque<Point>,
    dots: Vec<Dot>,
}
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn pcm(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}
fn polar(r: f32, a: f32) -> Point {
    [r * a.cos(), r * a.sin()]
}
fn plus(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn waveform(w: &[f32; 32], u: f32) -> f32 {
    let p = u.clamp(0.0, 1.0) * 31.0;
    let i = (p as usize).min(30);
    pcm(w[i]) + (pcm(w[i + 1]) - pcm(w[i])) * (p - i as f32)
}

// Coordinates belong to the requested area, not its intersection. Partial
// redraws therefore crop a curve rather than moving it to a new center.
struct Canvas<'a> {
    dots: &'a mut [Dot],
    clip: Rect,
    area: Rect,
}
impl Canvas<'_> {
    fn dot(&mut self, p: Point, phase: f32, light: f32) {
        if !p[0].is_finite() || !p[1].is_finite() {
            return;
        }
        let x = self.area.x as f32 * 2.0 + (p[0] + 1.0) * self.area.width as f32;
        let y = self.area.y as f32 * 4.0 + (1.0 - p[1]) * self.area.height as f32 * 2.0;
        let x = x.floor() as i32;
        let y = y.floor() as i32;
        let cx = x.div_euclid(2);
        let cy = y.div_euclid(4);
        if cx < self.clip.x as i32
            || cy < self.clip.y as i32
            || cx >= self.clip.right() as i32
            || cy >= self.clip.bottom() as i32
        {
            return;
        }
        let index = (cy - self.clip.y as i32) as usize * self.clip.width as usize
            + (cx - self.clip.x as i32) as usize;
        let bit = [[0, 1, 2, 6], [3, 4, 5, 7]][x.rem_euclid(2) as usize][y.rem_euclid(4) as usize];
        let dot = &mut self.dots[index];
        dot.mask |= 1 << bit;
        if light >= dot.light {
            dot.phase = phase;
            dot.light = light;
        }
    }
    fn line(&mut self, a: Point, b: Point, phase: f32, light: f32) {
        if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
            return;
        }
        let n = (((a[0] - b[0]).abs() * self.area.width as f32
            + (a[1] - b[1]).abs() * self.area.height as f32 * 2.0)
            .ceil() as usize)
            .clamp(1, 1024);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.dot(
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
                phase,
                light,
            );
        }
    }
    fn curve(&mut self, count: usize, phase: f32, light: f32, f: impl Fn(f32) -> Point) {
        let mut previous = f(0.0);
        for i in 1..=count {
            let u = i as f32 / count as f32;
            let next = f(u);
            self.line(previous, next, phase + u * 0.23, light);
            previous = next;
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
            audio.energy,
            audio.bass,
            audio.mid,
            audio.treble,
            audio.kick.max(audio.transients[0]),
            audio.novelty,
            audio.centroid,
            audio.flatness,
        ];
        let blend = 1.0 - (-dt * 8.0).exp();
        for (v, t) in self.controls.iter_mut().zip(targets) {
            *v += (unit(t) - *v) * blend;
        }
        for (v, t) in self.spectrum.iter_mut().zip(bands) {
            *v += (unit(*t) - *v) * blend;
        }
        for (v, t) in self.chroma.iter_mut().zip(audio.chroma) {
            *v += (unit(t) - *v) * blend;
        }
        let speed = 0.16 + self.controls[2] * 0.45;
        self.motion += dt * speed;
        self.pendulum_phase =
            (self.pendulum_phase + dt * speed * (2.07 + self.controls[2] * 0.15)).rem_euclid(TAU);
        let [energy, bass, mid, treble, kick, novelty, centroid, flatness] = self.controls;
        let t = self.motion;
        let pitch: f32 = self
            .chroma
            .iter()
            .enumerate()
            .map(|(i, v)| i as f32 * v)
            .sum();
        let spec = &self.spectrum;
        let mode = style % NAMES.len();
        let pendulum = |s: f32| {
            let a = 0.7 * (s * 1.3).sin();
            let b = 0.65 * (self.pendulum_phase + (s - t) * (2.07 + mid * 0.15) + bass).sin();
            [
                0.44 * a.sin() + 0.40 * b.sin(),
                0.72 - 0.44 * a.cos() - 0.40 * b.cos(),
            ]
        };
        if mode != 6 {
            self.trail.clear();
        }
        if mode == 6 && dt > 0.0 {
            // Several physical samples per frame preserve a continuous trace.
            for i in 0..4 {
                if self.trail.len() == TRAIL {
                    self.trail.pop_front();
                }
                self.trail
                    .push_back(pendulum(t - dt * (3 - i) as f32 * 0.12));
            }
        }
        let clip = area.intersection(buf.area);
        let cells = clip.width as usize * clip.height as usize;
        if cells == 0 || cells > MAX_CELLS {
            return;
        }
        self.dots.resize(cells, Dot::default());
        self.dots.fill(Dot::default());
        let mut c = Canvas {
            dots: &mut self.dots,
            clip,
            area,
        };
        let light = 0.48 + 0.40 * energy;
        match mode {
            0 => {
                // The mono PCM buffer is an actual time-domain oscilloscope:
                // horizontal position is sample time, never a fabricated channel.
                c.curve(248, 0.52, light, |u| {
                    [u * 1.84 - 0.92, waveform(&audio.waveform, u) * 0.8]
                });
            }
            1 => {
                // Delay embedding of the same mono buffer; no synthetic stereo.
                let delay = 2.0 + centroid * 9.0;
                c.curve(248, 0.1, light, |u| {
                    let i = u * (31.0 - delay);
                    [
                        waveform(&audio.waveform, i / 31.0) * 0.84,
                        waveform(&audio.waveform, (i + delay) / 31.0) * 0.84,
                    ]
                });
            }
            2 => {
                // A garden of separate, pitch-class-weighted polar roses.
                for j in 0..6 {
                    let center = polar(0.52, j as f32 * TAU / 6.0 + t * 0.15);
                    let petals = (j + 2) as f32;
                    let size = 0.20 + self.chroma[j * 2] * 0.50 + spec[j * 5] * 0.09;
                    c.curve(200, j as f32 / 6.0, light, |u| {
                        let a = u * TAU * 2.0;
                        plus(center, polar(size * (petals * a).cos(), a + t))
                    });
                }
            }
            3 => {
                // Fourier sum of rotating arms, with visible generating circles.
                let radii = [
                    0.40,
                    0.16 + 0.12 * bass,
                    0.07 + 0.10 * mid,
                    0.03 + 0.08 * treble,
                ];
                let rates = [1.0, -3.0, 5.0, -9.0];
                let position =
                    |s: f32| (0..4).fold([0.0, 0.0], |p, j| plus(p, polar(radii[j], s * rates[j])));
                c.curve(SAMPLES, 0.65, light, |u| position(t + u * TAU));
                let mut origin = [0.0, 0.0];
                for j in 0..4 {
                    c.curve(80, j as f32 * 0.2, 0.28, |u| {
                        plus(origin, polar(radii[j], u * TAU))
                    });
                    let end = plus(origin, polar(radii[j], t * rates[j]));
                    c.line(origin, end, 0.1 + j as f32 * 0.2, 0.85);
                    origin = end;
                }
            }
            4 => {
                // Hypotrochoid: a pen inside a rolling circle, not a rose.
                let r = 0.19 + 0.07 * mid;
                let outer = 0.62;
                let pen = 0.10 + 0.18 * bass;
                c.curve(1200, 0.25, light, |u| {
                    let a = u * TAU * 7.0;
                    let b = (outer - r) / r * a;
                    let p = [
                        (outer - r) * a.cos() + pen * b.cos(),
                        (outer - r) * a.sin() - pen * b.sin(),
                    ];
                    let (s, co) = (t * 0.2).sin_cos();
                    [p[0] * co - p[1] * s, p[0] * s + p[1] * co]
                });
            }
            5 => {
                // Damped four-oscillator harmonograph, fading toward its center.
                c.curve(1400, 0.4, light, |u| {
                    let s = u * 38.0;
                    let decay = (-u * (1.8 + flatness * 2.5)).exp();
                    [
                        decay
                            * (0.52 * (s + t).sin() + 0.30 * (s * (2.0 + mid * 0.10) + 1.0).sin()),
                        decay
                            * (0.53 * (s * 1.51 + t * 0.7).sin()
                                + 0.27 * (s * (3.0 + bass * 0.12)).sin()),
                    ]
                });
            }
            6 => {
                let mut prev = None;
                let len = self.trail.len().max(1) as f32;
                for (i, &p) in self.trail.iter().enumerate() {
                    if let Some(a) = prev {
                        c.line(
                            a,
                            p,
                            0.15 + i as f32 / len * 0.5,
                            0.18 + i as f32 / len * 0.65,
                        );
                    }
                    prev = Some(p);
                }
                let a = 0.7 * (t * 1.3).sin();
                let elbow = [0.44 * a.sin(), 0.72 - 0.44 * a.cos()];
                c.line([0.0, 0.72], elbow, 0.13, 0.35);
                c.line(elbow, pendulum(t), 0.25, 0.65);
            }
            7 => {
                // A horizontal train of trochoid loops, with bass-controlled cusps.
                for row in 0..3 {
                    c.curve(SAMPLES, row as f32 * 0.28, light, |u| {
                        let a = u * TAU * 3.0 + t * (1.0 + row as f32 * 0.15);
                        let ratio = 0.55 + bass * 0.9 + kick * 0.3;
                        [
                            u * 2.3 - 1.15 - ratio * a.sin() * 0.10,
                            (row as f32 - 1.0) * 0.52 + 0.18 * ratio * a.cos(),
                        ]
                    });
                }
            }
            8 => {
                // Fixed-end string eigenmodes; spectrum changes nodes and bows.
                for row in 0..5 {
                    let weights: [f32; 5] =
                        std::array::from_fn(|j| 0.02 + spec[row * 5 + j] * 0.13);
                    c.curve(280, row as f32 / 5.0, light, |u| {
                        let y: f32 = weights
                            .iter()
                            .enumerate()
                            .map(|(j, w)| {
                                let n = (j + 1) as f32;
                                w * (u * PI * n).sin() * (t * (n + 1.0) + row as f32).cos()
                            })
                            .sum();
                        [u * 1.86 - 0.93, (row as f32 - 2.0) * 0.33 + y]
                    });
                }
            }
            9 => {
                // Three orbiting local rosettes, with distinct precession rates.
                for j in 0..3 {
                    let orientation = t * (0.35 + j as f32 * 0.13);
                    let center = polar(0.38, orientation + j as f32 * TAU / 3.0);
                    c.curve(SAMPLES, j as f32 / 3.0, light, |u| {
                        let a = u * TAU;
                        let r = 0.23
                            + 0.08 * ((5 + j) as f32 * a + t).cos()
                            + 0.09 * novelty * (a * 11.0).sin();
                        plus(center, polar(r, a + orientation))
                    });
                }
            }
            _ => {
                // Gielis superformula: exponents morph corners and concavities.
                let m = 6.0;
                let n1 = 0.65 + bass * 2.5 + pitch * 0.035;
                let n2 = 0.6 + mid * 3.0;
                let n3 = 0.6 + treble * 3.0;
                for shell in 0..3 {
                    c.curve(800, shell as f32 * 0.25, light - shell as f32 * 0.12, |u| {
                        let a = u * TAU;
                        let q =
                            (m * a / 4.0).cos().abs().powf(n2) + (m * a / 4.0).sin().abs().powf(n3);
                        let r = q.max(0.04).powf(-1.0 / n1).min(2.0) * (0.38 - shell as f32 * 0.09);
                        polar(r, a + t * 0.12)
                    });
                }
            }
        }
        // Only cells in the actual buffer/area intersection may be touched.
        let bg = theme.sample(palette, 0.0, 0.0);
        for y in 0..clip.height {
            for x in 0..clip.width {
                let dot = self.dots[y as usize * clip.width as usize + x as usize];
                let cell = &mut buf[(clip.x + x, clip.y + y)];
                cell.reset();
                cell.set_bg(bg);
                if dot.mask == 0 {
                    cell.set_symbol(" ");
                    cell.set_fg(bg);
                } else {
                    cell.set_char(char::from_u32(0x2800 + dot.mask as u32).unwrap_or(' '));
                    cell.set_fg(theme.sample(palette, dot.phase, dot.light));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn audio() -> Reactivity {
        Reactivity {
            energy: 0.8,
            bass: 0.7,
            mid: 0.6,
            treble: 0.8,
            kick: 0.4,
            novelty: 0.8,
            centroid: 0.7,
            flatness: 0.5,
            chroma: std::array::from_fn(|i| if i == 7 { 1.0 } else { 0.0 }),
            waveform: std::array::from_fn(|i| (i as f32 * 0.53).sin() * 0.8),
            ..Reactivity::default()
        }
    }
    fn picture(style: usize, a: &Reactivity, bands: &[f32; 32]) -> (String, Renderer) {
        let area = Rect::new(0, 0, 96, 32);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for i in 0..150 {
            renderer.draw(
                &mut buf,
                area,
                i as f32 / 60.0,
                style,
                0,
                bands,
                a,
                &Theme::default(),
            );
        }
        let marks = buf.content.iter().map(|c| c.symbol()).collect::<String>();
        (marks, renderer)
    }
    #[test]
    fn pendulum_pitch_changes_do_not_scale_with_uptime() {
        let area = Rect::new(0, 0, 8, 4);
        let mut buffer = Buffer::empty(area);
        let theme = Theme::default();
        let mut renderer = Renderer {
            motion: 10_000.0,
            last_time: Some(0.0),
            ..Default::default()
        };
        renderer.draw(
            &mut buffer,
            area,
            0.016,
            6,
            0,
            &[0.0; 32],
            &Reactivity::default(),
            &theme,
        );
        let before = *renderer.trail.back().unwrap();
        let audio = Reactivity {
            mid: 1.0,
            ..Default::default()
        };
        renderer.draw(&mut buffer, area, 0.032, 6, 0, &[0.0; 32], &audio, &theme);
        let after = *renderer.trail.back().unwrap();
        assert!((after[0] - before[0]).hypot(after[1] - before[1]) < 0.05);
    }

    #[test]
    fn all_modes_clip_and_keep_finite_bounded_state() {
        let mut a = audio();
        a.energy = f32::NAN;
        a.waveform[0] = f32::INFINITY;
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(8, 9, 1, 1),
                Rect::new(4, 5, 20, 12),
                Rect::new(30, 30, 4, 3),
                Rect::new(0, 0, 14, 10),
            ] {
                let bounds = Rect::new(6, 7, 18, 9);
                let mut buf = Buffer::empty(bounds);
                for cell in &mut buf.content {
                    cell.set_symbol("x");
                }
                let mut r = Renderer::default();
                for time in [0.0, 0.016, 9e8, f32::NAN, -5.0] {
                    r.draw(
                        &mut buf,
                        area,
                        time,
                        style,
                        2,
                        &[f32::NAN; 32],
                        &a,
                        &Theme::default(),
                    );
                }
                let clip = area.intersection(bounds);
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "x");
                        }
                    }
                }
                assert!(r
                    .controls
                    .iter()
                    .chain(r.spectrum.iter())
                    .all(|v| v.is_finite()));
                assert!(r.motion.is_finite());
                assert!(r.trail.len() <= TRAIL && r.dots.len() <= MAX_CELLS);
                assert!(r.trail.iter().flatten().all(|v| v.is_finite()));
            }
        }
    }
    #[test]
    fn audio_changes_geometry_and_modes_are_distinct() {
        let quiet = Reactivity::default();
        let loud = audio();
        let mut pictures = std::collections::HashSet::new();
        for mode in 0..NAMES.len() {
            let (q, _) = picture(mode, &quiet, &[0.0; 32]);
            let (l, r) = picture(mode, &loud, &[0.8; 32]);
            assert_ne!(q, l, "mode {mode} must change geometry, not just pigment");
            assert!(pictures.insert(l), "duplicate mode {mode}");
            assert!(r.trail.len() <= TRAIL);
        }
    }
    #[test]
    fn mono_paths_use_pcm_not_energy_or_fabricated_stereo() {
        let mut a = audio();
        let mut b = a;
        b.waveform.fill(0.0);
        for mode in 0..2 {
            assert_ne!(
                picture(mode, &a, &[0.0; 32]).0,
                picture(mode, &b, &[0.0; 32]).0
            );
            a.energy = 0.0;
            // Energy shades the trace but cannot alter its mono geometry.
            assert_eq!(
                picture(mode, &a, &[0.0; 32]).0,
                picture(mode, &audio(), &[0.0; 32]).0
            );
        }
    }
    #[test]
    fn partial_buffers_crop_without_moving_curves() {
        let r = Renderer::default();
        assert_eq!(r.dots.capacity(), 0);
        assert_eq!(r.trail.capacity(), 0);
        let area = Rect::new(3, 4, 80, 24);
        let crop = Rect::new(21, 10, 25, 11);
        for mode in 0..NAMES.len() {
            let mut full = Buffer::empty(area);
            let mut partial = Buffer::empty(crop);
            let mut a = Renderer::default();
            let mut b = Renderer::default();
            a.draw(
                &mut full,
                area,
                0.0,
                mode,
                0,
                &[0.7; 32],
                &audio(),
                &Theme::default(),
            );
            b.draw(
                &mut partial,
                area,
                0.0,
                mode,
                0,
                &[0.7; 32],
                &audio(),
                &Theme::default(),
            );
            for y in crop.y..crop.bottom() {
                for x in crop.x..crop.right() {
                    assert_eq!(full[(x, y)], partial[(x, y)], "mode {mode}");
                }
            }
        }
    }
    #[test]
    #[ignore = "manual release timing probe"]
    fn release_frame_budget() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 160, 50));
        let a = audio();
        for (mode, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let start = std::time::Instant::now();
            for frame in 0..180 {
                r.draw(
                    &mut buf,
                    Rect::new(0, 0, 160, 50),
                    frame as f32 / 60.0,
                    mode,
                    0,
                    &[0.7; 32],
                    &a,
                    &Theme::default(),
                );
            }
            let millis = start.elapsed().as_secs_f64() * 1000.0 / 180.0;
            eprintln!("{name}: {millis:.3} ms/frame");
            assert!(millis < 16.0);
        }
    }
}
