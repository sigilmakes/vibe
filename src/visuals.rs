use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

pub const SCENES: &[&str] = &[
    "Prismatic Plasma",
    "Velvet Kaleidoscope",
    "Orbital Bloom",
    "Hyperspace",
    "Liquid Topography",
    "Electric Aurora",
    "Spectral Flower",
    "Lissajous Ribbons",
    "Particle Constellation",
    "Frequency Terrain",
];

pub const PALETTES: &[&str] = &["Cyber Candy", "Solar Flare", "Ocean Glass", "Ultraviolet"];

// Closed gradients; the final stop wraps back into the first one.
const COLORS: [[[f32; 3]; 5]; 4] = [
    [
        [38., 30., 175.],
        [163., 31., 245.],
        [255., 40., 135.],
        [30., 235., 255.],
        [69., 95., 255.],
    ],
    [
        [125., 16., 111.],
        [244., 37., 77.],
        [255., 123., 33.],
        [255., 224., 103.],
        [229., 61., 129.],
    ],
    [
        [12., 47., 136.],
        [20., 135., 219.],
        [40., 245., 218.],
        [160., 255., 188.],
        [73., 75., 223.],
    ],
    [
        [49., 12., 118.],
        [118., 43., 231.],
        [223., 76., 255.],
        [126., 189., 255.],
        [255., 119., 194.],
    ],
];

/// Smoothed, normalized visual controls; these are envelopes, not PCM samples.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reactivity {
    pub energy: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
}

type Rgb = [f32; 3];

fn segment_distance(x: f32, y: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let u = ((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy).max(0.00001);
    let u = u.clamp(0.0, 1.0);
    (x - a[0] - dx * u).hypot(y - a[1] - dy * u)
}

fn hash(x: f32, y: f32) -> f32 {
    ((x * 127.1 + y * 311.7).sin() * 43_758.547).fract().abs()
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn gradient(phase: f32, palette: usize) -> Rgb {
    let p = phase.rem_euclid(1.0) * 5.0;
    let index = p.floor() as usize;
    let t = p.fract();
    let stops = &COLORS[palette % COLORS.len()];
    mix(
        stops[index],
        stops[(index + 1) % 5],
        t * t * (3.0 - 2.0 * t),
    )
}

fn glow(distance: f32, width: f32) -> f32 {
    let d = distance / width;
    1.0 / (1.0 + d * d)
}

fn band(bands: &[f32; 32], position: f32) -> f32 {
    let p = position.clamp(0.0, 1.0) * 31.0;
    let i = p as usize;
    let value = bands[i] + (bands[(i + 1).min(31)] - bands[i]) * p.fract();
    value.clamp(0.0, 1.0)
}

struct Field<'a> {
    time: f32,
    palette: usize,
    bands: &'a [f32; 32],
    audio: &'a Reactivity,
    ribbons: [[[f32; 2]; 33]; 2],
}

impl Field<'_> {
    fn sample(&self, x: f32, y: f32, scene: usize) -> Rgb {
        let t = self.time;
        let e = self.audio.energy.clamp(0.0, 1.0);
        let bass = self.audio.bass.clamp(0.0, 1.0);
        let mid = self.audio.mid.clamp(0.0, 1.0);
        let treble = self.audio.treble.clamp(0.0, 1.0);
        let onset = self.audio.onset.clamp(0.0, 1.0);
        let kick = self.audio.kick.clamp(0.0, 1.0);
        let r = x.hypot(y);
        let a = y.atan2(x);
        let audio = band(self.bands, (a / TAU + 0.5).rem_euclid(1.0));
        let (phase, light, highlight) = match scene % SCENES.len() {
            0 => {
                let warp = (y * 2.7 + t * 0.31 + bass * 0.8).sin()
                    + (x * 2.1 - t * 0.27 + mid * 0.6).cos()
                    + onset * 0.3;
                let p = (x * 3.4 + warp + t * 0.42).sin()
                    + (y * 4.1 - warp * 0.7 - t * 0.33).sin()
                    + (r * 5.0 - t * 0.6).cos();
                let filament = glow((p * 2.7 + t * 0.2).sin(), 0.10 + e * 0.06);
                (
                    p * 0.15 + t * 0.025,
                    0.24 + p.sin().abs() * 0.38 + filament * 0.45,
                    filament * 0.28,
                )
            }
            1 => {
                let angle = ((a + t * 0.085) * 6.0).cos();
                let fold = r * (1.3 + bass * 0.25 + 0.35 * angle)
                    + (0.13 + mid * 0.1) * (t * 0.4 + r * 5.0).sin()
                    - onset * 0.08;
                let lace = (fold * 11.0 - t * 0.65 + angle * 2.8).sin();
                let fine = (fold * 22.0 + angle * 4.0 + t * 0.3).cos();
                let line = glow(lace, 0.09 + audio * 0.15);
                (
                    fold * 0.45 + angle * 0.12 - t * 0.025,
                    0.10 + line * 0.83 + fine.max(0.0) * 0.12,
                    line.powi(3) * 0.42,
                )
            }
            2 => {
                let wobble = (a * 5.0 + t * 0.5).sin() * 0.07 + (a * 3.0 - t * 0.3).cos() * 0.055;
                let radius = r + wobble * (0.4 + r + mid * 0.4) - audio * 0.1 - onset * 0.06;
                let wave = (radius * (14.0 + e * 2.0) - t * 1.2).sin();
                let rings = glow(wave, 0.085 + e * 0.06);
                let petals = (a * 8.0 + r * 3.0 - t * 0.3).cos() * 0.5 + 0.5;
                let core = glow(r, 0.13 + e * 0.05);
                (
                    radius * 0.33 - t * 0.055 + petals * 0.13,
                    0.07 + rings * (0.42 + petals * 0.5) + core * 0.5,
                    rings * petals * 0.4 + core * 0.5,
                )
            }
            3 => {
                let depth = -(r + 0.055).ln();
                let twist = a + depth * (0.28 + mid * 0.12) + t * 0.12 + onset * 0.08;
                let lanes = (twist * 13.0).sin();
                let travel = depth * 7.0 - t * 1.7 - bass * 0.7;
                let streaks = glow(lanes, 0.05 + audio * 0.1);
                let gates = glow(travel.sin(), 0.08);
                let sparks = glow((twist * 37.0 + depth * 2.0).sin(), 0.035)
                    * glow((depth * 17.0 - t * 3.1).sin(), 0.12);
                let fade = (r * 3.5).clamp(0.0, 1.0);
                (
                    depth * 0.16 + a / TAU + t * 0.045,
                    (0.05 + streaks * 0.5 + gates * 0.32 + sparks * 0.8) * fade,
                    (streaks * gates * 0.6 + sparks) * fade,
                )
            }
            4 => {
                let u = x + (0.24 + bass * 0.15) * (y * 2.4 + t * 0.25).sin();
                let v = y + (0.23 + mid * 0.13) * (x * 2.0 - t * 0.2).sin() + onset * 0.06;
                let height = (u * 2.5 + t * 0.17).sin() * 0.55
                    + (v * 3.3 - t * 0.23).cos() * 0.35
                    + (u * 4.0 + v * 2.0 + t * 0.2).sin() * 0.18;
                let contours = glow((height * 16.0 + t * 0.25).sin(), 0.08 + e * 0.05);
                let broad = (height * 2.0 + 1.0).sin() * 0.5 + 0.5;
                (
                    height * 0.6 + t * 0.018,
                    0.12 + broad * 0.24 + contours * 0.62,
                    contours * 0.25,
                )
            }
            5 => {
                let sway = (x * 2.2 + t * 0.25).sin() * 0.28 + (x * 4.7 - t * 0.32).sin() * 0.1;
                let frequency = band(self.bands, (x * 0.22 + 0.5).clamp(0.0, 1.0));
                let curtain = y
                    + sway * (1.0 + mid * 0.6)
                    + 0.12
                    + frequency * 0.28
                    + bass * 0.12
                    + onset * 0.05;
                let ribbon = glow(curtain, 0.12 + e * 0.08);
                let silk = (x * 27.0 + (y * 3.0 + t * 0.4).sin() * 3.0 + t * 0.5).sin() * 0.5 + 0.5;
                let haze = glow(curtain - 0.25, 0.45) * 0.18;
                (
                    x * 0.13 + y * 0.22 + silk * 0.08 + t * 0.025,
                    haze + ribbon * (0.35 + silk * 0.6),
                    ribbon.powi(3) * silk * 0.38,
                )
            }
            6 => {
                // The angular spectrum shapes the petal edge, not just its colour.
                let angle = a + t * 0.09;
                let spectrum = band(self.bands, (angle / TAU).rem_euclid(1.0));
                let petals = (angle * 7.0 + (t * 0.27).sin() * mid).cos();
                let radius = 0.35
                    + bass * 0.2
                    + kick * 0.12
                    + onset * 0.03
                    + (0.12 + mid * 0.12) * petals
                    + spectrum * 0.18;
                let edge = glow(r - radius, 0.014 + treble * 0.012);
                let inner = glow(r - radius * 0.57, 0.009) * 0.45;
                let veins = glow((angle * 7.0 + r * 4.0).sin(), 0.07)
                    * glow(r - radius * 0.75, 0.13)
                    * 0.25;
                (
                    angle / TAU + r * 0.3 + t * 0.02,
                    0.012 + edge * 0.8 + inner + veins,
                    edge * 0.32,
                )
            }
            7 => {
                // Two parametric paths suggest stereo motion without implying PCM input.
                let mut distance = f32::MAX;
                let mut phase = 0.0;
                for (side, points) in self.ribbons.iter().enumerate() {
                    let mut previous = points[0];
                    for (i, &next) in points.iter().enumerate().skip(1) {
                        let d = segment_distance(x, y, previous, next);
                        if d < distance {
                            distance = d;
                            phase = i as f32 / 32.0 + side as f32 * 0.28;
                        }
                        previous = next;
                    }
                }
                let line = glow(distance, 0.012 + treble * 0.012);
                (phase + t * 0.025, 0.008 + line * 0.82, line.powi(3) * 0.35)
            }
            8 => {
                let spacing = 0.36;
                let drift_x = x + t * 0.025;
                let drift_y = y - t * 0.018;
                let cell_x = (drift_x / spacing).floor();
                let cell_y = (drift_y / spacing).floor();
                let mut light = 0.0_f32;
                let mut phase = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let cx = cell_x + dx as f32;
                        let cy = cell_y + dy as f32;
                        let seed = hash(cx, cy);
                        if seed < 0.48 {
                            continue;
                        }
                        let envelope = band(self.bands, seed);
                        let px = (cx + 0.2 + seed * 0.6) * spacing
                            + (t * 0.3 + seed * TAU).sin() * (0.025 + bass * 0.09);
                        let py = (cy + 0.2 + hash(cy, cx + 3.0) * 0.6) * spacing
                            + (t * 0.24 + seed * 9.0).cos() * (0.02 + mid * 0.07);
                        let tail = [
                            px - 0.025 - envelope * 0.12 - onset * 0.04,
                            py + 0.018 + treble * 0.055,
                        ];
                        let star =
                            glow((drift_x - px).hypot(drift_y - py), 0.005 + envelope * 0.009);
                        let trail =
                            glow(segment_distance(drift_x, drift_y, tail, [px, py]), 0.004) * 0.25;
                        let value = star + trail;
                        if value > light {
                            light = value;
                            phase = seed + t * 0.01;
                        }
                    }
                }
                (phase, light * 0.85, light.powi(3) * 0.55)
            }
            _ => {
                let horizon = -0.28 - bass * 0.08 - kick * 0.04;
                let below = y - horizon;
                if below <= 0.0 {
                    (0.65 + x * 0.04, 0.008, 0.0)
                } else {
                    let depth = 0.55 / (below + 0.09);
                    let world_x = x * depth;
                    let z = depth + t * 0.28;
                    let frequency = band(self.bands, (world_x * 0.13 + 0.5).clamp(0.0, 1.0));
                    let height = frequency * (0.8 + bass * 0.5)
                        + (world_x * 1.8 + z * 0.6).sin() * mid * 0.22
                        + onset * 0.13 * (world_x * 2.0).cos();
                    let across = glow(((z + height) * 3.0).sin(), 0.045 + below * 0.035);
                    let along = glow((world_x * 3.0).sin(), 0.035 + below * 0.025);
                    let fade = (below * 4.0).min(1.0) / (1.0 + depth * 0.1);
                    let grid = (across * 0.8 + along * 0.45) * fade;
                    (
                        z * 0.06 + frequency * 0.3 + x * 0.09,
                        0.008 + grid,
                        across * along * fade * 0.35,
                    )
                }
            }
        };
        let color = gradient(phase, self.palette);
        let brightness = light * (0.8 + e * 0.35);
        std::array::from_fn(|i| {
            let base = color[i] * brightness;
            (base + (255.0 - base).max(0.0) * highlight.clamp(0.0, 0.8)).clamp(0.0, 255.0)
        })
    }
}

/// Render square virtual pixels using the upper and lower halves of each cell.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    buf: &mut Buffer,
    area: Rect,
    time: f32,
    scene: usize,
    next_scene: usize,
    blend: f32,
    palette: usize,
    bands: &[f32; 32],
    audio: &Reactivity,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let field = Field {
        time,
        palette,
        bands,
        audio,
        // Precompute the parametric paths once per frame, not once per pixel.
        ribbons: std::array::from_fn(|side| {
            std::array::from_fn(|i| {
                let side = side as f32 * 2.0 - 1.0;
                let u = i as f32 / 32.0 * TAU;
                let spectrum = band(bands, u / TAU);
                let bass = audio.bass.clamp(0.0, 1.0);
                let mid = audio.mid.clamp(0.0, 1.0);
                [
                    (u * 2.0 + time * 0.22 + side * (0.22 + mid * 0.3)).sin()
                        * (0.75 + bass * 0.22),
                    (u * 3.0 - time * 0.17 + side * 0.35).sin()
                        * (0.34 + mid * 0.16 + spectrum * 0.12)
                        + side * audio.onset.clamp(0.0, 1.0) * 0.07,
                ]
            })
        }),
    };
    let blend = blend.clamp(0.0, 1.0);
    let visible = area.intersection(buf.area);
    let scale = 1.0 / area.height as f32;
    for row in visible.y..visible.bottom() {
        let y = ((row - area.y) as f32 * 2.0 + 0.5 - area.height as f32) * scale;
        for col in visible.x..visible.right() {
            let x = ((col - area.x) as f32 + 0.5 - area.width as f32 * 0.5) * scale;
            let pixel = |y| {
                let rgb = if blend <= 0.0 || scene == next_scene {
                    field.sample(x, y, scene)
                } else if blend >= 1.0 {
                    field.sample(x, y, next_scene)
                } else {
                    mix(
                        field.sample(x, y, scene),
                        field.sample(x, y, next_scene),
                        blend,
                    )
                };
                Color::Rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8)
            };
            buf[(col, row)].reset();
            buf[(col, row)]
                .set_char('▀')
                .set_fg(pixel(y))
                .set_bg(pixel(y + scale));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_endpoints_match_direct_rendering() {
        let area = Rect::new(0, 0, 12, 5);
        let bands = [0.4; 32];
        let audio = Reactivity {
            energy: 0.5,
            ..Reactivity::default()
        };
        let mut direct = Buffer::empty(area);
        let mut transition = Buffer::empty(area);
        for (blend, target) in [(0.0, 1), (1.0, 4)] {
            draw(
                &mut direct,
                area,
                2.3,
                target,
                target,
                0.0,
                2,
                &bands,
                &audio,
            );
            draw(&mut transition, area, 2.3, 1, 4, blend, 2, &bands, &audio);
            assert_eq!(direct, transition);
        }
    }

    fn render(scene: usize, bands: &[f32; 32], audio: &Reactivity) -> Buffer {
        let area = Rect::new(0, 0, 48, 16);
        let mut buf = Buffer::empty(area);
        draw(&mut buf, area, 2.3, scene, scene, 0.0, 0, bands, audio);
        buf
    }

    #[test]
    fn every_scene_renders_distinct_silent_and_loud_geometry() {
        let silent = Reactivity::default();
        // Keep energy unchanged to verify deformation, not the global brightness gain.
        let loud = Reactivity {
            energy: 0.0,
            bass: 0.9,
            mid: 0.7,
            treble: 0.8,
            onset: 0.8,
            kick: 1.0,
        };
        let bands = std::array::from_fn(|i| 0.2 + i as f32 / 40.0);
        let mut silent_frames = Vec::new();
        let mut loud_frames = Vec::new();
        for (scene, name) in SCENES.iter().enumerate() {
            let quiet = render(scene, &[0.0; 32], &silent);
            let active = render(scene, &bands, &loud);
            assert_ne!(quiet, active, "scene {} ignores envelopes", name);
            for other in &silent_frames {
                assert_ne!(&quiet, other, "duplicate silent scene {}", name);
            }
            for other in &loud_frames {
                assert_ne!(&active, other, "duplicate active scene {}", name);
            }
            silent_frames.push(quiet);
            loud_frames.push(active);
        }
    }

    #[test]
    fn kick_changes_flower_and_terrain_without_broadband_onset() {
        let kick = Reactivity {
            kick: 1.0,
            ..Reactivity::default()
        };
        for scene in [6, 9] {
            assert_ne!(
                render(scene, &[0.0; 32], &Reactivity::default()),
                render(scene, &[0.0; 32], &kick)
            );
        }
    }

    #[test]
    fn drawing_clips_to_buffer_and_preserves_other_cells() {
        let mut buf = Buffer::empty(Rect::new(3, 2, 6, 4));
        buf[(3, 2)].set_char('x');
        draw(
            &mut buf,
            Rect::new(5, 3, 20, 20),
            0.0,
            0,
            1,
            0.5,
            0,
            &[0.0; 32],
            &Reactivity::default(),
        );
        assert_eq!(buf[(3, 2)].symbol(), "x");
        assert_eq!(buf[(8, 5)].symbol(), "▀");
    }
}
