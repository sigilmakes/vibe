use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

pub const SCENES: &[&str] = &[
    "Noise Field",
    "Spectral Flower",
    "Waveform Scope",
    "Liquid Topography",
    "Electric Aurora",
    "Orbital Bloom",
];

/// Normalized live audio. Transients carry independent low-to-high attacks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reactivity {
    pub energy: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
    pub beat: f32,
    pub beat_phase: f32,
    pub waveform: [f32; 32],
    pub transients: [f32; 4],
    pub novelty: f32,
}

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

fn noise_gradient(x: f32, y: f32) -> (f32, [f32; 2]) {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let sx = x - x.floor();
    let sy = y - y.floor();
    let dx = 6.0 * sx * (1.0 - sx);
    let dy = 6.0 * sy * (1.0 - sy);
    let sx = sx * sx * (3.0 - 2.0 * sx);
    let sy = sy * sy * (3.0 - 2.0 * sy);
    let aa = hash(ix, iy);
    let ab = hash(ix + 1, iy);
    let ba = hash(ix, iy + 1);
    let bb = hash(ix + 1, iy + 1);
    let a = aa * (1.0 - sx) + ab * sx;
    let b = ba * (1.0 - sx) + bb * sx;
    (
        a * (1.0 - sy) + b * sy,
        [((ab - aa) * (1.0 - sy) + (bb - ba) * sy) * dx, (b - a) * dy],
    )
}

fn noise(x: f32, y: f32) -> f32 {
    noise_gradient(x, y).0
}

fn line_glyph(tangent: [f32; 2]) -> char {
    let slope = tangent[1] / (tangent[0] + 0.00001);
    if slope.abs() < 0.4 {
        '─'
    } else if slope.abs() > 2.5 {
        '│'
    } else if slope > 0.0 {
        '╲'
    } else {
        '╱'
    }
}

fn distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let u = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
        / (d[0] * d[0] + d[1] * d[1]).max(0.00001))
    .clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * u).hypot(p[1] - a[1] - d[1] * u)
}

struct Pixel {
    color: Color,
    mark: Option<char>,
}

#[derive(Clone, Copy)]
struct Impact {
    age: f32,
    strength: f32,
    center: [f32; 2],
    normal: [f32; 2],
    channel: usize,
}

/// Motion is integrated, so changing an envelope never jumps a time multiplier.
/// Short-lived impacts preserve rapid attacks after their source envelope falls.
#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    offset: [f32; 2],
    warp: [f32; 2],
    rotation: f32,
    expansion: f32,
    expansion_velocity: f32,
    attacks: [f32; 4],
    cooldown: [f32; 4],
    serial: i32,
    impacts: Vec<Impact>,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1));
        self.last_time = Some(time);
        self.offset[0] += dt * (0.025 + audio.bass * 0.7 + audio.energy * 0.2);
        self.offset[1] += dt * (0.018 + audio.mid * 0.5 - audio.bass * 0.13);
        self.warp[0] += dt * (0.015 + audio.mid * 0.42);
        self.warp[1] -= dt * (0.012 + audio.treble * 0.55);
        self.rotation += dt * (0.012 + audio.mid * 0.13);
        for impact in &mut self.impacts {
            impact.age += dt;
        }
        self.impacts.retain(|impact| impact.age < 0.18);
        for channel in 0..4 {
            self.cooldown[channel] = (self.cooldown[channel] - dt).max(0.0);
            let fallback = match channel {
                0 => audio.kick,
                1 => audio.onset * 0.75,
                2 => audio.novelty,
                _ => audio.onset * audio.treble,
            };
            let attack = audio.transients[channel].max(fallback).clamp(0.0, 1.0);
            let rise = attack - self.attacks[channel];
            if rise > 0.09 && attack > 0.16 && self.cooldown[channel] == 0.0 {
                self.serial = self.serial.wrapping_add(1);
                let seed = hash(self.serial, channel as i32);
                let angle = seed * TAU;
                if self.impacts.len() == 32 {
                    self.impacts.remove(0);
                }
                self.impacts.push(Impact {
                    age: 0.0,
                    strength: attack,
                    center: [
                        (hash(self.serial, 19) - 0.5) * 1.6,
                        (hash(self.serial, 23) - 0.5) * 1.2,
                    ],
                    normal: [angle.cos(), angle.sin()],
                    channel,
                });
                if channel == 0 {
                    self.expansion_velocity += attack * 2.7;
                }
                self.cooldown[channel] = 0.035;
            }
            self.attacks[channel] = attack;
        }
        // A damped spring supplies expansion followed by an angular recoil.
        // Substeps keep this stable when a terminal frame takes longer than usual.
        let steps = (dt / 0.008).ceil().max(1.0) as usize;
        let step = dt / steps as f32;
        for _ in 0..steps {
            self.expansion_velocity +=
                (-900.0 * self.expansion - 36.0 * self.expansion_velocity) * step;
            self.expansion += self.expansion_velocity * step;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        scene: usize,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &crate::palette::Theme,
    ) {
        self.advance(time, audio);
        if area.width == 0 || area.height == 0 {
            return;
        }
        // Seven recognizable scalloped petals, drawn as hard facets. Each
        // frequency region and recent attack can displace an individual petal.
        let vertices: [[f32; 2]; 42] = std::array::from_fn(|i| {
            let angle = i as f32 / 42.0 * TAU;
            let petal = i / 6;
            let mut radius = 0.40
                + (angle * 7.0).cos() * 0.105
                + self.expansion
                + audio.bass * 0.055
                + audio.beat * 0.025
                + (angle * 7.0 - audio.beat_phase * TAU).cos() * audio.beat * 0.008
                + bands[petal * 4].clamp(0.0, 1.0) * 0.10
                + if i % 2 == 0 { 0.055 } else { -0.045 }
                + bands[i * 31 / 41].clamp(0.0, 1.0) * 0.08;
            for impact in &self.impacts {
                let alignment = (angle.cos() * impact.normal[0] + angle.sin() * impact.normal[1])
                    .max(0.0)
                    .powi(6);
                radius += alignment
                    * impact.strength
                    * (1.0 - impact.age / 0.12).max(0.0)
                    * if impact.channel < 2 {
                        0.12
                    } else {
                        if i % 2 == 0 {
                            0.09
                        } else {
                            -0.04
                        }
                    };
            }
            let angle = angle + self.rotation;
            [angle.cos() * radius, angle.sin() * radius]
        });
        let visible = area.intersection(buf.area);
        let scale = 2.0 / area.height as f32;
        for row in visible.y..visible.bottom() {
            for col in visible.x..visible.right() {
                let x =
                    ((col - area.x) as f32 + 0.5 - area.width as f32 * 0.5) / area.height as f32;
                let y = ((row - area.y) as f32 + 0.25 - area.height as f32 * 0.5) * scale;
                let top = self.pixel(
                    [x, y],
                    &vertices,
                    scene,
                    palette,
                    audio,
                    theme,
                    scale,
                    area.width as f32 / area.height as f32 * 0.5,
                    bands,
                );
                let bottom = self.pixel(
                    [x, y + scale * 0.5],
                    &vertices,
                    scene,
                    palette,
                    audio,
                    theme,
                    scale,
                    area.width as f32 / area.height as f32 * 0.5,
                    bands,
                );
                let cell = &mut buf[(col, row)];
                cell.reset();
                if let Some(mark) = top.mark.or(bottom.mark) {
                    let color = if top.mark.is_some() {
                        top.color
                    } else {
                        bottom.color
                    };
                    cell.set_char(mark)
                        .set_fg(color)
                        .set_bg(theme.sample(palette, 0.0, 0.035));
                } else {
                    cell.set_char('▀').set_fg(top.color).set_bg(bottom.color);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pixel(
        &self,
        p: [f32; 2],
        vertices: &[[f32; 2]; 42],
        scene: usize,
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
        half_width: f32,
        bands: &[f32; 32],
    ) -> Pixel {
        // Dispatch before sampling: a scene cannot inherit another scene's layers.
        match scene {
            1 => return self.flower(p, vertices, palette, audio, theme, scale),
            2 => return self.scope(p, palette, audio, theme, scale, half_width),
            3..=5 => return self.restored_soft(p, scene, palette, audio, bands, theme),
            _ => {}
        }
        let mut shift = [0.0, 0.0];
        let mut percussion = 0.0_f32;
        for impact in &self.impacts {
            let dx = p[0] - impact.center[0];
            let dy = p[1] - impact.center[1];
            let across = dx * impact.normal[0] + dy * impact.normal[1];
            let along = -dx * impact.normal[1] + dy * impact.normal[0];
            let front = across - impact.age * (0.4 + impact.channel as f32 * 0.18);
            let width = 0.035 + impact.age * 0.06;
            let local = (1.0 - front.abs() / width).max(0.0)
                * (1.0 - along.abs() / 0.7).max(0.0)
                * (1.0 - impact.age / 0.18).max(0.0)
                * impact.strength;
            shift[0] += impact.normal[1] * local * 0.3;
            shift[1] -= impact.normal[0] * local * 0.3;
            percussion = percussion.max(local);
        }
        let wx = noise(p[0] * 1.8 + self.warp[0], p[1] * 1.8 + self.warp[1]);
        let wy = noise(p[0] * 1.8 - self.warp[1] + 7.1, p[1] * 1.8 + self.warp[0]);
        let nx = p[0] * 2.5 + self.offset[0] + wx * (0.7 + audio.mid * 1.2) + shift[0];
        let ny = p[1] * 2.5 + self.offset[1] + wy * (0.7 + audio.bass) + shift[1];
        let (base, gradient) = noise_gradient(nx, ny);
        let field = base * 0.62
            + noise(nx * 2.1 + self.warp[1], ny * 2.1) * 0.26
            + noise(nx * 4.2, ny * 4.2 + self.warp[0]) * 0.12;
        let heat = ((field * 7.0).floor() / 7.0).clamp(0.0, 1.0);
        let contour = (field * 11.0).fract();
        let ink = if contour < 0.13 {
            0.48
        } else {
            0.08 + heat * 0.16
        };
        let intensity = ink * (0.85 + audio.energy * 0.25);
        // The raster retains color bands, while selected contour cells become
        // printed marks. Their placement belongs to the field, not random text.
        let cell_x = (p[0] / (scale * 0.5)).floor() as i32;
        let cell_y = (p[1] / scale).floor() as i32;
        let paper = hash(cell_x, cell_y);
        let mark = if percussion > 0.12 && paper < 0.62 {
            Some(if percussion > 0.55 {
                '*'
            } else if percussion > 0.32 {
                '+'
            } else if percussion > 0.2 {
                ':'
            } else {
                '·'
            })
        } else if contour < 0.13 && paper < 0.64 {
            // Follow the contour tangent, rather than sprinkling unrelated text.
            Some(line_glyph([-gradient[1], gradient[0]]))
        } else if paper < 0.14 && contour > 0.18 && contour < 0.38 {
            Some(if heat > 0.57 {
                '▒'
            } else if heat > 0.4 {
                '░'
            } else {
                '⠂'
            })
        } else {
            None
        };
        Pixel {
            color: theme.sample(
                palette,
                (heat * 1.35 + audio.bass * 0.06).min(0.98),
                if mark.is_some() {
                    intensity.max(0.28)
                } else {
                    intensity
                },
            ),
            mark,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn restored_soft(
        &self,
        [x, y]: [f32; 2],
        scene: usize,
        palette: usize,
        audio: &Reactivity,
        bands: &[f32; 32],
        theme: &crate::palette::Theme,
    ) -> Pixel {
        let t = self.last_time.unwrap_or(0.0);
        let e = audio.energy;
        let bass = audio.bass;
        let mid = audio.mid;
        let treble = audio.treble;
        let onset = audio.onset;
        let low_attack = audio.transients[0].max(audio.kick);
        let high_attack = audio.transients[2].max(audio.transients[3]);
        let glow = |distance: f32, width: f32| {
            let d = distance / width;
            1.0 / (1.0 + d * d)
        };
        // These are the original contour and filament equations, not the broad
        // Gaussian washes. Fast attacks deform them without changing their clock.
        let (phase, light, highlight) = if scene == 3 {
            let u = x
                + (0.24 + bass * 0.15) * (y * 2.4 + t * 0.25).sin()
                + low_attack * 0.10 * (y * 5.0).sin();
            let v = y
                + (0.23 + mid * 0.13) * (x * 2.0 - t * 0.2).sin()
                + onset * 0.06
                + high_attack * 0.065 * (x * 7.0).sin();
            let height = (u * 2.5 + t * 0.17).sin() * 0.55
                + (v * 3.3 - t * 0.23).cos() * 0.35
                + (u * 4.0 + v * 2.0 + t * 0.2).sin() * 0.18;
            let contours = glow(
                (height * 16.0 + t * 0.25).sin(),
                0.08 + e * 0.05 + treble * 0.025,
            );
            let broad = (height * 2.0 + 1.0).sin() * 0.5 + 0.5;
            (
                height * 0.6 + t * 0.018,
                0.12 + broad * 0.24 + contours * 0.62,
                contours * 0.25,
            )
        } else if scene == 4 {
            let sway = (x * 2.2 + t * 0.25).sin() * 0.28 + (x * 4.7 - t * 0.32).sin() * 0.1;
            let position = (x * 0.22 + 0.5).clamp(0.0, 1.0) * 31.0;
            let i = position.floor() as usize;
            let frequency = bands[i] + (bands[(i + 1).min(31)] - bands[i]) * position.fract();
            let curtain = y
                + sway * (1.0 + mid * 0.6)
                + 0.12
                + frequency * 0.28
                + bass * 0.12
                + onset * 0.05
                + low_attack * 0.10
                + high_attack * 0.045 * (x * 13.0).sin();
            let ribbon = glow(curtain, 0.12 + e * 0.08);
            let silk = (x * 27.0 + (y * 3.0 + t * 0.4).sin() * 3.0 + t * 0.5).sin() * 0.5 + 0.5;
            let haze = glow(curtain - 0.25, 0.45) * 0.18;
            (
                x * 0.13 + y * 0.22 + silk * 0.08 + t * 0.025,
                haze + ribbon * (0.35 + silk * (0.6 + treble * 0.15)),
                ribbon.powi(3) * silk * 0.38,
            )
        } else {
            let r = x.hypot(y);
            let a = y.atan2(x);
            let position = (a / TAU + 0.5).rem_euclid(1.0) * 31.0;
            let i = position.floor() as usize;
            let spectrum = bands[i] + (bands[(i + 1).min(31)] - bands[i]) * position.fract();
            let wobble = (a * 5.0 + t * 0.5).sin() * 0.07 + (a * 3.0 - t * 0.3).cos() * 0.055;
            let radius = r / (1.0 + audio.beat * 0.16 + bass * 0.10 + low_attack * 0.10)
                + wobble * (0.4 + r + mid * 0.4)
                - spectrum * 0.1
                - onset * 0.06
                - high_attack * 0.03 * (a * 11.0).sin();
            let wave = (radius * (14.0 + e * 2.0) - audio.beat_phase * TAU).sin();
            let rings = glow(wave, 0.085 + e * 0.06 + treble * 0.025);
            let petals = (a * 8.0 + r * 3.0 - t * 0.3).cos() * 0.5 + 0.5;
            let core = glow(r, 0.13 + e * 0.05);
            (
                radius * 0.33 - t * 0.055 + petals * 0.13,
                0.07 + rings * (0.42 + petals * 0.5) + core * 0.5,
                rings * petals * 0.4 + core * 0.5,
            )
        };
        let base = theme.rgb(palette, phase, (light * (0.8 + e * 0.35)).clamp(0.0, 1.0));
        let rgb: [u8; 3] = std::array::from_fn(|i| {
            (base[i] as f32
                + (theme.foreground[i] as f32 - base[i] as f32) * highlight.clamp(0.0, 0.8))
            .round() as u8
        });
        Pixel {
            color: Color::Rgb(rgb[0], rgb[1], rgb[2]),
            mark: None,
        }
    }

    fn flower(
        &self,
        p: [f32; 2],
        vertices: &[[f32; 2]; 42],
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
    ) -> Pixel {
        let mut edge = f32::MAX;
        let mut tangent = [1.0, 0.0];
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let d = distance(p, a, b);
            if d < edge {
                edge = d;
                tangent = [b[0] - a[0], b[1] - a[1]];
            }
        }
        let ink = edge < (scale * 0.32).max(0.009);
        Pixel {
            color: theme.sample(
                palette,
                0.82 + audio.treble * 0.15,
                if ink { 0.96 } else { 0.035 },
            ),
            mark: ink.then(|| line_glyph(tangent)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scope(
        &self,
        p: [f32; 2],
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
        half_width: f32,
    ) -> Pixel {
        // Pixel centers of the first and last canvas columns are the endpoints.
        let span = (half_width - scale * 0.25).max(scale * 0.25);
        let position = ((p[0] / span + 1.0) * 0.5 * 31.0).clamp(0.0, 31.0);
        let index = (position.floor() as usize).min(30);
        let mut edge = f32::MAX;
        let mut tangent = [1.0, 0.0];
        for i in index.saturating_sub(1)..=(index + 1).min(30) {
            // Only real PCM determines the path; no synthetic oscillation.
            let a = [
                ((i as f32 / 31.0) * 2.0 - 1.0) * span,
                audio.waveform[i].clamp(-1.0, 1.0) * 0.88,
            ];
            let b = [
                (((i + 1) as f32 / 31.0) * 2.0 - 1.0) * span,
                audio.waveform[i + 1].clamp(-1.0, 1.0) * 0.88,
            ];
            let d = distance(p, a, b);
            if d < edge {
                edge = d;
                tangent = [b[0] - a[0], b[1] - a[1]];
            }
        }
        let ink = edge < (scale * 0.32).max(0.008);
        Pixel {
            color: theme.sample(
                palette,
                0.32 + p[0] / span * 0.12,
                if ink { 1.0 } else { 0.035 },
            ),
            mark: ink.then(|| line_glyph(tangent)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_scenes_use_continuous_half_block_shading() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        for scene in [3, 4, 5] {
            let mut buffer = Buffer::empty(area);
            Renderer::default().draw(
                &mut buffer,
                area,
                0.0,
                scene,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &theme,
            );
            assert!(buffer.content.iter().all(|cell| cell.symbol() == "▀"));
            let shades: std::collections::HashSet<_> = buffer
                .content
                .iter()
                .flat_map(|cell| [cell.fg, cell.bg])
                .collect();
            assert!(
                shades.len() > 80,
                "soft scenes need rich shading, not color bands"
            );
        }
    }

    #[test]
    fn soft_scenes_respond_to_each_audio_region_and_the_active_theme() {
        let area = Rect::new(0, 0, 80, 28);
        let theme = crate::palette::Theme::default();
        let custom = crate::palette::Theme {
            ansi: [[100, 120, 140]; 16],
            background: [20, 30, 40],
            foreground: [150, 160, 170],
        };
        for scene in [3, 4, 5] {
            let render = |audio: &Reactivity, theme: &crate::palette::Theme| {
                let mut renderer = Renderer::default();
                let mut buffer = Buffer::empty(area);
                for frame in 0..=12 {
                    renderer.draw(
                        &mut buffer,
                        area,
                        frame as f32 / 60.0,
                        scene,
                        0,
                        &[0.0; 32],
                        audio,
                        theme,
                    );
                }
                buffer
            };
            let quiet = render(&Reactivity::default(), &theme);
            for audio in [
                Reactivity {
                    bass: 0.8,
                    ..Reactivity::default()
                },
                Reactivity {
                    mid: 0.8,
                    ..Reactivity::default()
                },
                Reactivity {
                    treble: 0.8,
                    ..Reactivity::default()
                },
            ] {
                let active = render(&audio, &theme);
                let changed = quiet
                    .content
                    .iter()
                    .zip(&active.content)
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    changed > area.area() as usize / 10,
                    "each audio region must visibly affect soft scene {scene}"
                );
            }
            let themed = render(&Reactivity::default(), &custom);
            assert_ne!(quiet, themed);
            for color in themed.content.iter().flat_map(|cell| [cell.fg, cell.bg]) {
                let Color::Rgb(r, g, b) = color else {
                    panic!("RGB shading expected")
                };
                assert!((20..=150).contains(&r));
                assert!((30..=160).contains(&g));
                assert!((40..=170).contains(&b));
            }
        }
    }

    #[test]
    fn restored_designs_react_to_attacks_on_the_first_frame() {
        let area = Rect::new(0, 0, 100, 36);
        let theme = crate::palette::Theme::default();
        for scene in [3, 4, 5] {
            let render = |audio: &Reactivity| {
                let mut buffer = Buffer::empty(area);
                Renderer::default().draw(
                    &mut buffer,
                    area,
                    2.0,
                    scene,
                    0,
                    &[0.0; 32],
                    audio,
                    &theme,
                );
                buffer
            };
            let quiet = render(&Reactivity::default());
            for transients in [[0.85, 0.0, 0.0, 0.0], [0.0, 0.0, 0.85, 0.85]] {
                let hit = render(&Reactivity {
                    transients,
                    ..Reactivity::default()
                });
                let changed = quiet
                    .content
                    .iter()
                    .zip(&hit.content)
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    changed > area.area() as usize / 5,
                    "attack must deform the design immediately"
                );
            }
        }
    }

    #[test]
    fn field_uses_real_terminal_texture() {
        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        let audio = Reactivity {
            energy: 0.7,
            bass: 0.6,
            mid: 0.5,
            transients: [0.8; 4],
            ..Reactivity::default()
        };
        Renderer::default().draw(
            &mut buf,
            area,
            0.0,
            0,
            0,
            &[0.5; 32],
            &audio,
            &crate::palette::Theme::default(),
        );
        let glyphs: std::collections::HashSet<_> =
            buf.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            glyphs.len() >= 4,
            "actual marks, not only half-block pixels: {glyphs:?}"
        );
    }

    #[test]
    fn pcm_moves_the_visible_trace_at_equal_energy() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let mut flat = Buffer::empty(area);
        let mut wriggly = Buffer::empty(area);
        let audio = Reactivity::default();
        let wave = Reactivity {
            waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin()),
            ..audio
        };
        Renderer::default().draw(&mut flat, area, 0.0, 2, 0, &[0.0; 32], &audio, &theme);
        Renderer::default().draw(&mut wriggly, area, 0.0, 2, 0, &[0.0; 32], &wave, &theme);
        let changed = flat
            .content
            .iter()
            .zip(&wriggly.content)
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 100,
            "PCM must change visible geometry, not a hidden input"
        );
    }

    #[test]
    fn scenes_are_distinct_and_pcm_belongs_only_to_scope() {
        assert_eq!(SCENES.len(), 6);
        let area = Rect::new(0, 0, 100, 36);
        let theme = crate::palette::Theme::default();
        let quiet = Reactivity::default();
        let wave = Reactivity {
            waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin()),
            ..quiet
        };
        let mut scenes = Vec::new();
        for (scene, name) in SCENES.iter().enumerate() {
            let mut plain = Buffer::empty(area);
            let mut pcm = Buffer::empty(area);
            Renderer::default().draw(&mut plain, area, 0.0, scene, 0, &[0.0; 32], &quiet, &theme);
            Renderer::default().draw(&mut pcm, area, 0.0, scene, 0, &[0.0; 32], &wave, &theme);
            if scene == 2 {
                assert_ne!(plain, pcm);
            } else {
                assert_eq!(plain, pcm, "PCM must not leak into {name}");
            }
            scenes.push(plain);
        }
        for a in 0..scenes.len() {
            for b in a + 1..scenes.len() {
                assert_ne!(scenes[a], scenes[b], "scenes must not be duplicate layers");
            }
        }
    }

    #[test]
    fn flower_and_scope_leave_the_corners_empty() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        for scene in [1, 2] {
            let mut buf = Buffer::empty(area);
            Renderer::default().draw(
                &mut buf,
                area,
                0.0,
                scene,
                0,
                &[0.5; 32],
                &Reactivity::default(),
                &theme,
            );
            let corner = &buf[(0, 0)];
            assert_eq!(corner.symbol(), "▀");
            assert_eq!(corner.fg, corner.bg, "no field underneath the geometry");
        }
    }

    #[test]
    fn scope_pcm_endpoints_reach_both_canvas_edges() {
        let theme = crate::palette::Theme::default();
        for (width, height) in [(120, 40), (80, 24), (160, 50)] {
            let area = Rect::new(3, 2, width, height);
            let mut buf = Buffer::empty(area);
            let audio = Reactivity {
                waveform: std::array::from_fn(|i| if i % 2 == 0 { 0.75 } else { -0.75 }),
                ..Reactivity::default()
            };
            Renderer::default().draw(&mut buf, area, 0.0, 2, 0, &[0.0; 32], &audio, &theme);
            for x in [area.x, area.right() - 1] {
                assert!(
                    (area.y..area.bottom()).any(|y| buf[(x, y)].symbol() != "▀"),
                    "live PCM must reach column {x} at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn independent_attacks_persist_and_recoil() {
        let mut renderer = Renderer::default();
        let quiet = Reactivity::default();
        renderer.advance(0.0, &quiet);
        let hit = Reactivity {
            transients: [0.9, 0.8, 0.7, 0.6],
            ..quiet
        };
        renderer.advance(0.02, &hit);
        assert_eq!(renderer.impacts.len(), 4);
        let expansion = renderer.expansion;
        renderer.advance(0.04, &quiet);
        assert_eq!(renderer.impacts.len(), 4);
        assert!(renderer.expansion > expansion);
        for i in 3..100 {
            renderer.advance(i as f32 * 0.02, &quiet);
        }
        assert!(renderer.impacts.is_empty());
        assert!(renderer.expansion.abs() < 0.01);
    }

    #[test]
    fn motion_integrates_instead_of_multiplying_time_by_audio() {
        let mut early = Renderer::default();
        let mut late = Renderer::default();
        let quiet = Reactivity::default();
        let active = Reactivity {
            bass: 0.8,
            mid: 0.7,
            ..quiet
        };
        early.advance(0.0, &quiet);
        late.advance(1000.0, &quiet);
        early.advance(0.02, &active);
        late.advance(1000.02, &active);
        assert!((early.offset[0] - late.offset[0]).abs() < 0.0001);
    }

    #[test]
    fn flowing_field_changes_without_a_clock_jump() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let audio = Reactivity {
            energy: 0.8,
            mid: 0.8,
            ..Reactivity::default()
        };
        let mut renderer = Renderer::default();
        let mut before = Buffer::empty(area);
        let mut after = Buffer::empty(area);
        renderer.draw(&mut before, area, 0.0, 0, 0, &[0.0; 32], &audio, &theme);
        for i in 1..=6 {
            renderer.draw(
                &mut after,
                area,
                i as f32 / 60.0,
                0,
                0,
                &[0.0; 32],
                &audio,
                &theme,
            );
        }
        assert!(
            before
                .content
                .iter()
                .zip(&after.content)
                .filter(|(a, b)| a != b)
                .count()
                > 100
        );
    }

    #[test]
    fn short_attacks_change_geometry_and_clear_before_the_next_phrase() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let quiet = Reactivity::default();
        let hit = Reactivity {
            transients: [0.95; 4],
            ..quiet
        };
        let mut renderer = Renderer::default();
        let mut idle = Renderer::default();
        let mut struck = Buffer::empty(area);
        let mut plain = Buffer::empty(area);
        renderer.advance(0.0, &quiet);
        idle.advance(0.0, &quiet);
        renderer.draw(&mut struck, area, 0.016, 0, 0, &[0.0; 32], &hit, &theme);
        idle.draw(&mut plain, area, 0.016, 0, 0, &[0.0; 32], &quiet, &theme);
        assert!(renderer.expansion > 0.02);
        assert!(
            struck
                .content
                .iter()
                .zip(&plain.content)
                .filter(|(a, b)| a.symbol() != b.symbol())
                .count()
                > 15
        );
        for i in 2..=13 {
            renderer.advance(i as f32 * 0.016, &quiet);
        }
        assert!(renderer.impacts.is_empty());
        assert!(renderer.expansion.abs() < 0.005);
    }

    #[test]
    #[ignore = "release-only renderer timing, excludes terminal I/O"]
    fn renderer_frame_budget() {
        for (scene, width, height) in [
            (0, 120, 40),
            (0, 160, 50),
            (3, 120, 40),
            (3, 160, 50),
            (4, 120, 40),
            (4, 160, 50),
        ] {
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            let mut renderer = Renderer::default();
            let theme = crate::palette::Theme::default();
            let mut audio = Reactivity {
                energy: 0.8,
                bass: 0.7,
                mid: 0.6,
                treble: 0.8,
                waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin() * 0.7),
                ..Reactivity::default()
            };
            let start = std::time::Instant::now();
            for i in 0..180 {
                audio.transients = [if i % 3 == 0 { 0.95 } else { 0.0 }; 4];
                renderer.draw(
                    &mut buffer,
                    area,
                    i as f32 / 60.0,
                    scene,
                    0,
                    &[0.6; 32],
                    &audio,
                    &theme,
                );
                std::hint::black_box(&buffer);
            }
            eprintln!(
                "{} {width}x{height}: {:.2} ms/frame",
                SCENES[scene],
                start.elapsed().as_secs_f64() * 1000.0 / 180.0
            );
        }
    }

    #[test]
    fn rapid_attacks_are_bounded_and_survive_silence() {
        let mut renderer = Renderer::default();
        for i in 0..100 {
            let value = if i % 2 == 0 { 1.0 } else { 0.0 };
            renderer.advance(
                i as f32 * 0.025,
                &Reactivity {
                    transients: [value; 4],
                    ..Reactivity::default()
                },
            );
        }
        assert!(renderer.impacts.len() <= 32);
        assert!(renderer.impacts.len() > 8);
    }
}
