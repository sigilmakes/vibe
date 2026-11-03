use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Moire Weave",
    "Interference Pool",
    "Cymatic Islands",
    "Chladni Plate",
    "Wave Tank",
    "Ripple Choir",
    "Diffraction Slits",
    "Standing Mesh",
    "Caustic Threads",
    "Phase Carpet",
];

const NX: usize = 80;
const NY: usize = 40;
const MAX_FRONTS: usize = 12;

fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A contour tangent from a field gradient. The gradient is normal to the
/// nodal line, so the line glyph runs across it.
fn gradient_glyph(gx: f32, gy: f32) -> char {
    let ax = gx.abs();
    let ay = gy.abs();
    if !ax.is_finite() || !ay.is_finite() || ax + ay < 1e-6 {
        return '·';
    }
    if ay > ax * 1.8 {
        '─'
    } else if ax > ay * 1.8 {
        '│'
    } else if gx * gy > 0.0 {
        '╱'
    } else {
        '╲'
    }
}

#[derive(Clone, Copy)]
struct Front {
    x: f32,
    y: f32,
    age: f32,
    strength: f32,
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    attack: f32,
    cooldown: f32,
    serial: u32,
    harmony: [f32; 12],
    fronts: Vec<Front>,
    height: Vec<f32>,
    velocity: Vec<f32>,
    next: Vec<f32>,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity, tank: bool) {
        let now = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |t| (now - t).clamp(0.0, 0.05));
        self.last_time = Some(now);
        self.phase += dt * (0.65 + unit(audio.energy) * 0.4);
        for (h, &c) in self.harmony.iter_mut().zip(&audio.chroma) {
            *h += (unit(c) - *h) * (dt * 4.0).min(1.0);
        }
        for f in &mut self.fronts {
            f.age += dt;
        }
        self.fronts.retain(|f| f.age < 4.0);
        self.cooldown = (self.cooldown - dt).max(0.0);
        let attack = audio
            .transients
            .iter()
            .copied()
            .map(unit)
            .fold(unit(audio.kick).max(unit(audio.onset)), f32::max);
        if attack - self.attack > 0.08 && attack > 0.16 && self.cooldown == 0.0 {
            self.serial = self.serial.wrapping_add(1);
            let a = self.serial as f32 * 2.399963;
            if self.fronts.len() == MAX_FRONTS {
                self.fronts.remove(0);
            }
            self.fronts.push(Front {
                x: 0.65 * a.cos(),
                y: 0.65 * a.sin(),
                age: 0.0,
                strength: attack,
            });
            self.cooldown = 0.09;
            if tank {
                self.init_tank();
                let x = ((0.5 + 0.3 * a.cos()) * (NX - 1) as f32) as usize;
                let y = ((0.5 + 0.3 * a.sin()) * (NY - 1) as f32) as usize;
                for j in y - 2..=y + 2 {
                    for i in x - 2..=x + 2 {
                        let r2 =
                            ((i as f32 - x as f32).powi(2) + (j as f32 - y as f32).powi(2)) * 0.7;
                        self.velocity[j * NX + i] += attack * 12.0 * (-r2).exp();
                    }
                }
            }
        }
        self.attack = attack;
        if tank {
            self.init_tank();
            let steps = (dt * 120.0).ceil().clamp(1.0, 1024.0) as usize;
            let h = dt / steps as f32;
            // Fixed edges, damped wave equation; c*dt < 0.13 cells/substep.
            for _ in 0..steps {
                for y in 1..NY - 1 {
                    for x in 1..NX - 1 {
                        let i = y * NX + x;
                        let lap = self.height[i - 1]
                            + self.height[i + 1]
                            + self.height[i - NX]
                            + self.height[i + NX]
                            - 4.0 * self.height[i];
                        self.velocity[i] = (self.velocity[i] + h * 225.0 * lap) * (-h * 0.65).exp();
                        self.next[i] = (self.height[i] + h * self.velocity[i]).clamp(-3.0, 3.0);
                    }
                }
                std::mem::swap(&mut self.height, &mut self.next);
                // A quiet, low-amplitude driven boundary-adjacent source.
                self.velocity[(NY / 2) * NX + 2] +=
                    h * self.phase.sin() * (8.0 + unit(audio.bass) * 5.0);
                for k in 0..3 {
                    let weight = self.harmony[k] + self.harmony[k + 4] + self.harmony[k + 8];
                    self.velocity[(NY / 3 + k * 5) * NX + NX / 2] +=
                        h * weight * (self.phase * (1.0 + k as f32 * 0.5)).sin() * 5.0;
                }
            }
        }
    }

    fn init_tank(&mut self) {
        if self.height.is_empty() {
            self.height = vec![0.0; NX * NY];
            self.velocity = vec![0.0; NX * NY];
            self.next = vec![0.0; NX * NY];
        }
    }

    fn fronts_at(&self, x: f32, y: f32) -> f32 {
        self.fronts
            .iter()
            .map(|f| {
                let d = (x - f.x).hypot(y - f.y);
                let q = d - f.age * 0.7;
                f.strength * (-q * q * 160.0).exp() * (q * 32.0).cos() * (-f.age * 0.65).exp()
            })
            .sum()
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
        let mode = style % NAMES.len();
        self.advance(time, audio, mode == 4);
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        let t = self.phase;
        let energy = unit(audio.energy);
        let harmony: [f32; 3] = std::array::from_fn(|k| {
            (0..4)
                .map(|j| self.harmony[k + j * 3] * (j + 1) as f32)
                .sum::<f32>()
        });
        // Separable geometry and integer eigenfunctions are evaluated once per axis.
        let xs: Vec<_> = (visible.x..visible.right())
            .map(|x| Axis::new((x - area.x) as f32 / area.width.max(1) as f32))
            .collect();
        let ys: Vec<_> = (visible.y..visible.bottom())
            .map(|y| Axis::new((y - area.y) as f32 / area.height.max(1) as f32))
            .collect();
        let slit_phase: [f32; 5] =
            std::array::from_fn(|i| t * 1.6 + harmony[i % 3] * i as f32 * 0.7);
        for (j, yy) in ys.iter().enumerate() {
            for (i, xx) in xs.iter().enumerate() {
                let x = xx.p;
                let y = yy.p;
                let r = x.hypot(y);
                let front = if mode == 4 { 0.0 } else { self.fronts_at(x, y) };
                let (field, strength, glyph) = match mode {
                    0 => {
                        let a = (x * 32.0 + y * 9.0 + t * 0.4).sin();
                        let b = (x * 30.0 - y * (10.0 + harmony[0]) - t * 0.3).sin();
                        let v = a * b;
                        let s = (v.abs() - 0.58).max(0.0) * 1.9;
                        (
                            v,
                            s + front.abs() * 0.5,
                            if a * b > 0.0 { '╱' } else { '╲' },
                        )
                    }
                    1 => {
                        let mut v = 0.0;
                        for (k, weight) in harmony.iter().enumerate() {
                            let angle = k as f32 * TAU / 3.0;
                            let d = (x - angle.cos() * 0.52).hypot(y - angle.sin() * 0.52);
                            v += (d * (19.0 + weight * 2.0) - t * 1.8).sin() / (1.0 + d);
                        }
                        (v, (v.abs() - 0.6).max(0.0) * 0.8 + front.abs() * 0.5, '◦')
                    }
                    2 => {
                        // Approximate nodal islands: regular angular polynomials, zero at rim.
                        let theta = y.atan2(x);
                        let envelope = (1.0 - r * r).max(0.0);
                        let v = envelope
                            * (r.powi(3) * (1.0 - 4.0 * r * r) * (theta * 3.0).cos()
                                + (harmony[0] + harmony[1])
                                    * r.powi(5)
                                    * (2.0 - 3.0 * r * r)
                                    * (theta * 5.0).sin());
                        let s = if r < 0.96 {
                            (0.09 - v.abs()).max(0.0) * 6.0 * envelope
                        } else {
                            0.0
                        };
                        (v, s + front.abs() * envelope * 0.4, '∘')
                    }
                    3 => {
                        // Free-edge rectangular plate Chladni degeneracy pair.
                        let v = xx.c[2] * yy.c[5] - xx.c[5] * yy.c[2]
                            + harmony[0] * (xx.c[3] * yy.c[6] - xx.c[6] * yy.c[3]) * 0.6
                            + front * 0.34;
                        // The eigenmode gradient is normal to its nodal line, so
                        // the contour tangent picks the straight line glyph.
                        let gx = -2.0 * PI * xx.s[2] * yy.c[5]
                            + 5.0 * PI * xx.s[5] * yy.c[2]
                            + harmony[0]
                                * 0.6
                                * (-3.0 * PI * xx.s[3] * yy.c[6] + 6.0 * PI * xx.s[6] * yy.c[3]);
                        let gy = -5.0 * PI * xx.c[2] * yy.s[5]
                            + 2.0 * PI * xx.c[5] * yy.s[2]
                            + harmony[0]
                                * 0.6
                                * (-6.0 * PI * xx.c[3] * yy.s[6] + 3.0 * PI * xx.c[6] * yy.s[3]);
                        (v, (0.16 - v.abs()).max(0.0) * 4.5, gradient_glyph(gx, gy))
                    }
                    4 => {
                        let gx = xx.u * (NX - 1) as f32;
                        let gy = yy.u * (NY - 1) as f32;
                        let ix = (gx as usize).min(NX - 2);
                        let iy = (gy as usize).min(NY - 2);
                        let fx = gx - ix as f32;
                        let fy = gy - iy as f32;
                        let k = iy * NX + ix;
                        let v = (self.height[k] * (1.0 - fx) + self.height[k + 1] * fx)
                            * (1.0 - fy)
                            + (self.height[k + NX] * (1.0 - fx) + self.height[k + NX + 1] * fx)
                                * fy;
                        (
                            v,
                            (v.abs() * 24.0).min(1.0),
                            if v > 0.0 { '≈' } else { '∽' },
                        )
                    }
                    5 => {
                        let v = front
                            + (0.22 * (r * 16.0 - t).cos()
                                + harmony[0] * 0.3 * (r * 25.0 - t * 1.3).sin()
                                + harmony[1] * 0.3 * (r * 32.0 - t * 1.6).cos())
                                / (1.0 + r * 2.0);
                        (v, (v.abs() - 0.07).max(0.0) * 1.6, '○')
                    }
                    6 => {
                        // Huygens sources behind five apertures; dark opaque barrier.
                        let mut v = 0.0;
                        if x > -0.48 {
                            for (k, phase) in slit_phase.iter().enumerate() {
                                let sy = (k as f32 - 2.0) * 0.28;
                                let d = (x + 0.5).hypot(y - sy);
                                v += (d * 26.0 - phase).cos() / (1.0 + d * 3.0);
                            }
                        } else if x < -0.54 {
                            v = 0.5 * (x * 26.0 - t * 1.6).cos();
                        }
                        (v, (v.abs() - 0.4).max(0.0) * 0.65 + front.abs() * 0.3, '┊')
                    }
                    7 => {
                        // Dirichlet rectangle eigenmode superposition.
                        let v = (1.0 + harmony[0] * 0.5) * xx.s[3] * yy.s[2] * t.cos()
                            + (0.3 + harmony[1]) * xx.s[5] * yy.s[4] * (t * 1.17).cos()
                            + harmony[2] * xx.s[7] * yy.s[3] * (t * 1.31).sin()
                            + front * 0.4;
                        (
                            v,
                            (v.abs() - 0.35).max(0.0) * 0.85,
                            if v > 0.0 { '╳' } else { '∙' },
                        )
                    }
                    8 => {
                        // Fold caustics of X=x+A*sin(a), Y=y+B*sin(b).
                        // Threshold the exact mapping determinant near its folds.
                        let a = x * 10.0 + (y * 4.0 + t * 0.35).sin();
                        let b = y * 11.0 + (x * 3.0 - t * 0.28).sin();
                        let a_scale = 0.13 + harmony[0] * 0.1;
                        let b_scale = 0.11;
                        let jac = (1.0 + 10.0 * a_scale * a.cos())
                            * (1.0 + 11.0 * b_scale * b.cos())
                            - 12.0
                                * a_scale
                                * b_scale
                                * a.cos()
                                * b.cos()
                                * (y * 4.0 + t * 0.35).cos()
                                * (x * 3.0 - t * 0.28).cos();
                        let s = (0.16 - jac.abs()).max(0.0) * 5.0;
                        (
                            jac,
                            s + front.abs() * 0.3,
                            if a.sin() > b.sin() { '⌒' } else { '⌢' },
                        )
                    }
                    _ => {
                        // Phase Carpet: nodal contours carry straight line
                        // glyphs; the rest of the field fills a braille density
                        // ramp, with attacks thickening the local pile.
                        let re = xx.c[4] * yy.c[3] + (0.3 + harmony[0]) * xx.s[6] * yy.s[5];
                        let im = xx.s[4] * yy.s[3] - (0.3 + harmony[2]) * xx.c[6] * yy.c[5];
                        let ct = t.cos();
                        let st = t.sin();
                        let v = re * ct - im * st + front * 0.2;
                        let amp = re.hypot(im);
                        let gx = (-4.0 * PI * xx.s[4] * yy.c[3]
                            + (0.3 + harmony[0]) * 6.0 * PI * xx.c[6] * yy.s[5])
                            * ct
                            - (4.0 * PI * xx.c[4] * yy.s[3]
                                + (0.3 + harmony[2]) * 6.0 * PI * xx.s[6] * yy.c[5])
                                * st;
                        let gy = (-3.0 * PI * xx.c[4] * yy.s[3]
                            + (0.3 + harmony[0]) * 5.0 * PI * xx.s[6] * yy.c[5])
                            * ct
                            - (3.0 * PI * xx.s[4] * yy.c[3]
                                + (0.3 + harmony[2]) * 5.0 * PI * xx.c[6] * yy.s[5])
                                * st;
                        const RAMP: [char; 6] = ['⠁', '⠉', '⠋', '⠛', '⠟', '⠿'];
                        let drive = amp.min(1.0) * (0.55 + energy * 0.45) + front.abs() * 0.6;
                        let glyph = if v.abs() < 0.18 && gx.abs() + gy.abs() > 1e-4 {
                            gradient_glyph(gx, gy)
                        } else {
                            RAMP[((drive * 4.8) as usize).min(RAMP.len() - 1)]
                        };
                        let s = (0.18 - v.abs()).max(0.0) * 3.6 * amp.min(1.0)
                            + amp.min(1.0) * (0.14 + energy * 0.10)
                            + front.abs() * 0.12;
                        (v, s, glyph)
                    }
                };
                let px = visible.x + i as u16;
                let py = visible.y + j as u16;
                // The Chladni plate has a visible rectangular rim. It is drawn
                // only at the requested area edge, never as overlay text.
                let (field, strength, glyph) = if mode == 3 {
                    let last_x = area.x.saturating_add(area.width.saturating_sub(1));
                    let last_y = area.y.saturating_add(area.height.saturating_sub(1));
                    let left = px == area.x;
                    let right = px == last_x;
                    let top = py == area.y;
                    let bottom = py == last_y;
                    if left || right || top || bottom {
                        let glyph = match (top, bottom, left, right) {
                            (true, _, true, _) if area.width > 1 && area.height > 1 => '┌',
                            (true, _, _, true) if area.width > 1 && area.height > 1 => '┐',
                            (_, true, true, _) if area.width > 1 && area.height > 1 => '└',
                            (_, true, _, true) if area.width > 1 && area.height > 1 => '┘',
                            (_, _, true, _) | (_, _, _, true) => '│',
                            _ => '─',
                        };
                        (0.0, 0.30 + energy * 0.16, glyph)
                    } else {
                        (field, strength, glyph)
                    }
                } else {
                    (field, strength, glyph)
                };
                let band = unit(bands[(xx.u * 31.0) as usize]);
                let bright = (strength * (0.55 + energy * 0.35 + band * 0.1)).clamp(0.0, 1.0);
                let cell = &mut buf[(px, py)];
                cell.reset();
                cell.set_bg(theme.sample(palette, 0.0, 0.0));
                cell.set_char(if bright > 0.045 { glyph } else { ' ' });
                cell.set_fg(theme.sample(palette, 0.52 + field * 0.12 + harmony[1] * 0.12, bright));
            }
        }
    }
}

struct Axis {
    u: f32,
    p: f32,
    s: [f32; 8],
    c: [f32; 8],
}
impl Axis {
    fn new(u: f32) -> Self {
        Self {
            u,
            p: u * 2.0 - 1.0,
            s: std::array::from_fn(|n| (u * PI * n as f32).sin()),
            c: std::array::from_fn(|n| (u * PI * n as f32).cos()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(mode: usize, audio: &Reactivity) -> (String, Renderer) {
        let area = Rect::new(3, 4, 96, 36);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..100 {
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
        let image = buf.content.iter().map(|c| c.symbol()).collect::<String>();
        (image, renderer)
    }

    #[test]
    fn all_modes_clip_and_remain_bounded() {
        let mut renderer = Renderer::default();
        let area = Rect::new(7, 9, 11, 7);
        for mode in 0..NAMES.len() {
            for target in [
                Rect::new(0, 0, 0, 0),
                Rect::new(8, 10, 1, 1),
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
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        if !target.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "!");
                        }
                    }
                }
            }
        }
        let mut audio = Reactivity::default();
        for frame in 0..1200 {
            audio.kick = if frame % 12 == 0 { 1.0 } else { 0.0 };
            renderer.advance(frame as f32 / 60.0, &audio, true);
        }
        assert!(renderer.fronts.len() <= MAX_FRONTS);
        assert_eq!(renderer.height.len(), NX * NY);
        assert!(renderer
            .height
            .iter()
            .chain(&renderer.velocity)
            .all(|v| v.is_finite()));
        let before = renderer.phase;
        renderer.advance(1e9, &audio, true);
        assert!((renderer.phase - before).abs() < 0.1);
    }

    #[test]
    fn quiet_modes_are_distinct_and_audio_changes_geometry() {
        let quiet = Reactivity::default();
        let mut images = Vec::new();
        let mut chord = Reactivity::default();
        chord.chroma[0] = 1.0;
        let percussion = Reactivity {
            kick: 1.0,
            ..Reactivity::default()
        };
        for mode in 0..NAMES.len() {
            let (base, state) = snapshot(mode, &quiet);
            assert!(
                base.chars().any(|c| c != ' '),
                "empty quiet mode {mode}, maximum height {:?}",
                state.height.iter().map(|v| v.abs()).fold(0.0, f32::max)
            );
            assert!(!images.contains(&base), "duplicate mode {mode}");
            images.push(base.clone());
            let (hit, _) = snapshot(mode, &percussion);
            assert_ne!(base, hit, "percussion does not change geometry {mode}");
            // Chroma changes eigenmode weights, not just pigment.
            let (harmonic, _) = snapshot(mode, &chord);
            assert_ne!(base, harmonic, "harmony does not change geometry {mode}");
        }
    }

    const W: usize = 80;
    const H: usize = 30;

    fn painted(mode: usize, audio: &Reactivity) -> Buffer {
        let area = Rect::new(0, 0, W as u16, H as u16);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..60 {
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
        buf
    }

    fn symbols(buf: &Buffer) -> Vec<String> {
        buf.content
            .iter()
            .map(|cell| cell.symbol().to_owned())
            .collect()
    }

    fn glyphs(buf: &Buffer) -> Vec<char> {
        buf.content
            .iter()
            .filter_map(|cell| {
                let mut chars = cell.symbol().chars();
                let ch = chars.next()?;
                (ch != ' ' && chars.next().is_none()).then_some(ch)
            })
            .collect()
    }

    fn braille_stats(buf: &Buffer) -> (usize, usize) {
        glyphs(buf).iter().fold((0, 0), |(cells, dots), &ch| {
            let code = ch as u32;
            if (0x2801..=0x28ff).contains(&code) {
                (cells + 1, dots + (code - 0x2800).count_ones() as usize)
            } else {
                (cells, dots)
            }
        })
    }

    #[test]
    fn chladni_plate_has_rim_nodal_lines_and_front_response() {
        let quiet = Reactivity::default();
        let buf = painted(3, &quiet);
        let glyphs = glyphs(&buf);
        for corner in ['┌', '┐', '└', '┘'] {
            assert_eq!(
                glyphs.iter().filter(|&&ch| ch == corner).count(),
                1,
                "plate rim is missing {corner}"
            );
        }
        let coverage = glyphs.len() as f32 / (W * H) as f32;
        assert!(
            (0.10..0.60).contains(&coverage),
            "plate ink coverage {coverage}"
        );
        for glyph in ['─', '│', '╱', '╲'] {
            assert!(glyphs.contains(&glyph), "plate is missing nodal {glyph}");
        }
        let kick = Reactivity {
            kick: 1.0,
            onset: 1.0,
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        let moved = symbols(&buf)
            .iter()
            .zip(symbols(&painted(3, &kick)))
            .filter(|(a, b)| a.as_str() != b.as_str())
            .count();
        assert!(moved > 10, "fronts moved only {moved} plate cells");
        assert_eq!(symbols(&buf), symbols(&painted(3, &quiet)));
    }

    #[test]
    fn phase_carpet_uses_braille_density_and_transient_thickening() {
        let quiet = Reactivity::default();
        let buf = painted(9, &quiet);
        let (braille_cells, dots) = braille_stats(&buf);
        assert!(
            braille_cells > 400,
            "carpet has only {braille_cells} braille cells"
        );
        assert!(
            dots as f32 / braille_cells as f32 > 2.0,
            "braille ramp averages {:.2} dots per cell",
            dots as f32 / braille_cells as f32
        );
        let distinct: std::collections::HashSet<_> = glyphs(&buf)
            .into_iter()
            .filter(|&ch| (0x2801..=0x28ff).contains(&(ch as u32)))
            .collect();
        assert!(distinct.len() >= 2, "density ramp has no steps");
        let lines = ['─', '│', '╱', '╲']
            .iter()
            .filter(|glyph| glyphs(&buf).contains(glyph))
            .count();
        assert!(lines >= 2, "phase contours have no line glyphs");
        let coverage = glyphs(&buf).len() as f32 / (W * H) as f32;
        assert!(coverage > 0.40, "carpet ink coverage {coverage}");
        let kick = Reactivity {
            kick: 1.0,
            onset: 1.0,
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        let (_, hit_dots) = braille_stats(&painted(9, &kick));
        assert!(hit_dots > dots, "attacks do not thicken the carpet");
        let moved = symbols(&buf)
            .iter()
            .zip(symbols(&painted(9, &kick)))
            .filter(|(a, b)| a.as_str() != b.as_str())
            .count();
        assert!(moved > 40, "attacks moved only {moved} carpet cells");
        assert_eq!(symbols(&buf), symbols(&painted(9, &quiet)));
    }

    #[test]
    #[ignore = "manual release timing probe"]
    fn frame_budget() {
        let mut r = Renderer::default();
        let mut b = Buffer::empty(Rect::new(0, 0, 160, 50));
        let audio = Reactivity {
            kick: 1.0,
            ..Reactivity::default()
        };
        for mode in 0..10 {
            r.fronts = (0..MAX_FRONTS)
                .map(|k| Front {
                    x: (k as f32).sin() * 0.6,
                    y: (k as f32).cos() * 0.6,
                    age: k as f32 * 0.05,
                    strength: 1.0,
                })
                .collect();
            let start = std::time::Instant::now();
            for f in 0..100 {
                r.draw(
                    &mut b,
                    Rect::new(0, 0, 160, 50),
                    f as f32 / 60.0,
                    mode,
                    0,
                    &[0.2; 32],
                    &audio,
                    &Theme::default(),
                );
            }
            eprintln!(
                "waves mode {mode} mean frame (12 fronts): {:?}",
                start.elapsed() / 100
            );
        }
    }
}
