use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{palette::Theme, visuals::Reactivity};

const ITERATIONS: usize = 96;
const COLORS: usize = 512;

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f64,
    bass: f64,
    mid: f64,
    treble: f64,
    harmony: [f64; 2],
    brightness: f64,
    noise: f64,
}

struct Frame {
    center: [f64; 2],
    height: f64,
    c: Option<[f64; 2]>,
    contrast: f32,
    rotation: f64,
}

fn unit(value: f32) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0) as f64
    } else {
        0.0
    }
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity, bands: &[f32; 32]) {
        let time = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1)) as f64;
        self.last_time = Some(time);
        self.phase = (self.phase + dt * (0.075 + unit(audio.energy) * 0.025))
            .rem_euclid(std::f64::consts::TAU * 10.0);
        let low = bands[..6].iter().map(|&v| unit(v)).sum::<f64>() / 6.0;
        let high = bands[24..].iter().map(|&v| unit(v)).sum::<f64>() / 8.0;
        // Circular pitch-class mean gives harmony a small, continuous Julia bias.
        let mut pitch = [0.0; 2];
        let total = audio.chroma.iter().map(|&v| unit(v)).sum::<f64>().max(1.0);
        for (index, &weight) in audio.chroma.iter().enumerate() {
            let angle = index as f64 * std::f64::consts::TAU / 12.0;
            let weight = unit(weight) / total * (1.0 - unit(audio.flatness));
            pitch[0] += weight * angle.cos();
            pitch[1] += weight * angle.sin();
        }
        for (value, target) in self.harmony.iter_mut().zip(pitch) {
            *value += (target - *value) * (1.0 - (-dt * 6.0).exp());
        }
        self.brightness += (unit(audio.centroid) - self.brightness) * (1.0 - (-dt * 18.0).exp());
        self.noise += (unit(audio.flatness) - self.noise) * (1.0 - (-dt * 10.0).exp());
        let targets = [
            unit(audio.bass)
                .max(unit(audio.kick))
                .max(unit(audio.beat))
                .max(unit(audio.transients[0]))
                .max(low),
            unit(audio.mid),
            unit(audio.treble)
                .max(unit(audio.transients[3]))
                .max(high)
                .max(unit(audio.centroid) * 0.35),
        ];
        for (value, target) in [&mut self.bass, &mut self.mid, &mut self.treble]
            .into_iter()
            .zip(targets)
        {
            let rate = if target > *value { 48.0 } else { 5.0 };
            *value += (target - *value) * (1.0 - (-dt * rate).exp());
        }
    }

    fn frame(&self, style: usize) -> Frame {
        let phase = self.phase;
        if style.is_multiple_of(2) {
            // The upper cardioid/seahorse boundary stays in view throughout the orbit.
            Frame {
                center: [
                    -0.69 + phase.sin() * 0.055,
                    0.19 + (phase * 0.7).sin() * 0.045,
                ],
                height: (1.85 + (phase * 0.6).cos() * 0.40) * (1.0 - self.bass * 0.38),
                c: None,
                contrast: self.treble as f32,
                rotation: 0.0,
            }
        } else {
            Frame {
                center: [(phase * 0.8).sin() * 0.035, (phase * 0.6).cos() * 0.025],
                height: (2.45 + (phase * 0.5).sin() * 0.18) * (1.0 - self.bass * 0.30),
                // A narrow filament-rich neighborhood, not a claim that every c is connected.
                c: Some([
                    -0.745 + phase.sin() * 0.0006 + self.mid * 0.0007 + self.harmony[0] * 0.002,
                    0.186
                        + (phase * 0.7).sin() * 0.0005
                        + self.mid * 0.0005
                        + self.harmony[1] * 0.002,
                ]),
                contrast: self.treble as f32,
                rotation: phase.sin() * 0.08 + self.harmony[1] * 0.30,
            }
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
        self.advance(time, audio, bands);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let frame = self.frame(style);
        let visible = area.intersection(buf.area);
        // Terminal cells are approximately twice as tall as wide. Each half-cell is square.
        let step = frame.height / (area.height as f64 * 2.0);
        let (sin, cos) = frame.rotation.sin_cos();
        let colors: [Color; COLORS] = std::array::from_fn(|i| {
            let escape = i as f32 / (COLORS - 1) as f32;
            let density = 5.0 + self.brightness as f32 * 9.0;
            let stripe = (escape * std::f32::consts::TAU * density).cos() * 0.5 + 0.5;
            let intensity = (0.36
                + escape.sqrt() * 0.37
                + stripe * (0.10 + frame.contrast * 0.20) * (1.0 - self.noise as f32 * 0.65))
                .min(0.94);
            theme.sample(
                palette,
                0.07 + escape * (1.4 + self.brightness as f32 * 1.4),
                intensity,
            )
        });
        let inside = Color::Rgb(
            theme.background[0],
            theme.background[1],
            theme.background[2],
        );
        for row in visible.y..visible.bottom() {
            let y = ((row - area.y) as f64 * 2.0 + 0.5 - area.height as f64) * step;
            for col in visible.x..visible.right() {
                let x = ((col - area.x) as f64 + 0.5 - area.width as f64 * 0.5) * step;
                let shade = |y| {
                    let point = [
                        frame.center[0] + x * cos - y * sin,
                        frame.center[1] + x * sin + y * cos,
                    ];
                    escape(point, frame.c).map_or(inside, |count| {
                        let index = ((count / ITERATIONS as f64).clamp(0.0, 1.0)
                            * (COLORS - 1) as f64) as usize;
                        colors[index]
                    })
                };
                let top = shade(y);
                let bottom = shade(y + step);
                let cell = &mut buf[(col, row)];
                cell.reset();
                cell.set_char('▀').set_fg(top).set_bg(bottom);
            }
        }
    }
}

/// None means bounded within the finite iteration budget, not proven membership.
fn escape(point: [f64; 2], julia: Option<[f64; 2]>) -> Option<f64> {
    let (mut z, c) = match julia {
        Some(c) => (point, c),
        None => {
            let [x, y] = point;
            let q = (x - 0.25).powi(2) + y * y;
            if q * (q + x - 0.25) <= 0.25 * y * y || (x + 1.0).powi(2) + y * y <= 0.0625 {
                return None;
            }
            ([0.0, 0.0], point)
        }
    };
    for iteration in 0..ITERATIONS {
        z = [z[0] * z[0] - z[1] * z[1] + c[0], 2.0 * z[0] * z[1] + c[1]];
        let norm = z[0] * z[0] + z[1] * z[1];
        if norm > 256.0 {
            return Some(iteration as f64 + 2.0 - (norm.ln() * 0.5).log2());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_known_points() {
        for point in [[0.0, 0.0], [-1.0, 0.0], [-0.5, 0.0]] {
            assert!(escape(point, None).is_none());
        }
        for point in [[1.0, 1.0], [2.0, 0.0], [-2.1, 0.0]] {
            assert!(escape(point, None).is_some());
        }
        // z² with c=0 has the unit disk as its filled Julia set.
        assert!(escape([0.5, 0.5], Some([0.0, 0.0])).is_none());
        assert!(escape([1.1, 0.0], Some([0.0, 0.0])).is_some());
        assert!(escape([2.0, 2.0], Some([-0.745, 0.186])).is_some());
        assert!(escape([0.0, 0.0], Some([-0.745, 0.186])).is_none());
    }

    #[test]
    fn bounded_camera_and_fast_audio_without_clock_jump() {
        let mut renderer = Renderer::default();
        let silence = Reactivity::default();
        renderer.advance(10000.0, &silence, &[0.0; 32]);
        let before = renderer.frame(0);
        let phase = renderer.phase;
        let loud = Reactivity {
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            ..silence
        };
        renderer.advance(10000.016, &loud, &[1.0; 32]);
        assert!((renderer.phase - phase).abs() < 0.01);
        assert!(renderer.frame(0).height < before.height);
        assert!(renderer.frame(0).contrast > 0.2);
        assert!(renderer.frame(0).c.is_none());
        assert!(renderer.frame(1).c.is_some());
        for i in 0..2000 {
            renderer.advance(10000.0 + i as f32 * 0.1, &loud, &[1.0; 32]);
            for style in 0..2 {
                let frame = renderer.frame(style);
                assert!((0.8..3.0).contains(&frame.height));
                if let Some(c) = frame.c {
                    assert!((-0.748..-0.741).contains(&c[0]));
                    assert!((0.183..0.190).contains(&c[1]));
                }
            }
        }
    }

    #[test]
    fn pitch_shapes_julia_and_spectrum_changes_shading() {
        let mut c_note = Reactivity::default();
        c_note.chroma[0] = 1.0;
        let mut g_note = c_note;
        g_note.chroma = [0.0; 12];
        g_note.chroma[7] = 1.0;
        let mut a = Renderer::default();
        let mut b = Renderer::default();
        for i in 0..30 {
            a.advance(i as f32 / 60.0, &c_note, &[0.0; 32]);
            b.advance(i as f32 / 60.0, &g_note, &[0.0; 32]);
        }
        let ca = a.frame(1).c.unwrap();
        let cb = b.frame(1).c.unwrap();
        assert!((ca[0] - cb[0]).abs() > 0.002);
        assert!((a.frame(1).rotation - b.frame(1).rotation).abs() > 0.1);
        let changed_points = (0..50)
            .filter(|&i| {
                let point = [-1.0 + i as f64 * 0.04, 0.2];
                escape(point, Some(ca)) != escape(point, Some(cb))
            })
            .count();
        assert!(changed_points > 10);
        let area = Rect::new(0, 0, 40, 16);
        let mut dark = Buffer::empty(area);
        let mut bright = Buffer::empty(area);
        let mut noise = Buffer::empty(area);
        let mut tonal_renderer = Renderer::default();
        let mut bright_renderer = Renderer::default();
        let mut noise_renderer = Renderer::default();
        for i in 0..20 {
            let time = i as f32 / 60.0;
            tonal_renderer.draw(
                &mut dark,
                area,
                time,
                0,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            bright_renderer.draw(
                &mut bright,
                area,
                time,
                0,
                0,
                &[0.0; 32],
                &Reactivity {
                    centroid: 1.0,
                    ..Reactivity::default()
                },
                &Theme::default(),
            );
            noise_renderer.draw(
                &mut noise,
                area,
                time,
                0,
                0,
                &[0.0; 32],
                &Reactivity {
                    flatness: 1.0,
                    ..Reactivity::default()
                },
                &Theme::default(),
            );
        }
        assert_ne!(dark, bright);
        assert_ne!(dark, noise);
        let mut attack = Renderer::default();
        attack.advance(0.0, &Reactivity::default(), &[0.0; 32]);
        let before = attack.frame(0).height;
        attack.advance(
            1.0 / 60.0,
            &Reactivity {
                kick: 1.0,
                ..Reactivity::default()
            },
            &[0.0; 32],
        );
        assert!(attack.frame(0).height < before * 0.8);
    }

    #[test]
    fn clipping_and_halfblocks() {
        let bounds = Rect::new(7, 9, 8, 5);
        let mut buffer = Buffer::empty(bounds);
        let mut renderer = Renderer::default();
        let theme = Theme::default();
        for style in 0..2 {
            renderer.draw(
                &mut buffer,
                Rect::new(8, 10, 20, 20),
                1.0,
                style,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &theme,
            );
            assert_eq!(buffer[(7, 9)].symbol(), " ");
            assert_eq!(buffer[(8, 10)].symbol(), "▀");
            assert!(matches!(buffer[(8, 10)].fg, Color::Rgb(_, _, _)));
        }
        renderer.draw(
            &mut buffer,
            Rect::new(0, 0, 0, 0),
            1.0,
            0,
            0,
            &[0.0; 32],
            &Reactivity::default(),
            &theme,
        );
        renderer.draw(
            &mut buffer,
            Rect::new(10, 11, 1, 1),
            1.0,
            1,
            0,
            &[0.0; 32],
            &Reactivity::default(),
            &theme,
        );
    }

    #[test]
    #[ignore = "run with --release --ignored --nocapture for frame timing"]
    fn release_frame_benchmark() {
        let mut renderer = Renderer::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut buffer = Buffer::empty(area);
        let theme = Theme::default();
        for style in 0..2 {
            let start = std::time::Instant::now();
            for i in 0..60 {
                renderer.draw(
                    &mut buffer,
                    area,
                    i as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &Reactivity::default(),
                    &theme,
                );
                std::hint::black_box(&buffer);
            }
            eprintln!(
                "fractal {style}: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 1000.0 / 60.0
            );
        }
    }
}
