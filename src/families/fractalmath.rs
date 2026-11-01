//! Parameter-plane escape sets, Newton basins, and finite recursive curves.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Burning Ship",
    "Tricorn",
    "Cubic Multibrot",
    "Quartic Multibrot",
    "Newton Cubic",
    "Newton Quartic",
    "Sierpinski Echo",
    "Koch Snowflake",
    "Dragon Curve",
    "Hilbert Pulse",
];

const MAX_ITER: usize = 64;
const MAX_SEGMENTS: usize = 16_384;
type Point = [f32; 2];
type Segment = (Point, Point);

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    bass: f32,
    attack: f32,
    segments: Vec<Segment>,
}

fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn mul(a: Point, b: Point) -> Point {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}
fn power(z: Point, n: usize) -> Point {
    let mut v = [1.0, 0.0];
    for _ in 0..n {
        v = mul(v, z);
    }
    v
}
fn mid(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |old| (time - old).clamp(0.0, 0.05));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        self.phase = (self.phase + dt * (0.07 + 0.11 * unit(audio.mid))).rem_euclid(TAU);
        self.bass += (unit(audio.bass) - self.bass) * (1.0 - (-dt * 8.0).exp());
        let transient = audio
            .transients
            .iter()
            .copied()
            .map(unit)
            .fold(unit(audio.onset).max(unit(audio.kick)), f32::max);
        self.attack = (self.attack * (-dt * 7.0).exp()).max(transient);
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
        self.advance(time, audio);
        let visible = area.intersection(buf.area);
        if visible.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        let style = style % NAMES.len();
        let background = theme.sample(palette, 0.0, 0.0);
        if style >= 6 {
            self.curves(style, audio);
            for y in visible.y..visible.bottom() {
                for x in visible.x..visible.right() {
                    buf[(x, y)].reset();
                    buf[(x, y)].set_char(' ').set_bg(background);
                }
            }
            // Uniform fit in physical terminal coordinates; no extreme-aspect cropping.
            let scale = (area.width.saturating_sub(2) as f32 * 0.5)
                .min(area.height.saturating_sub(2) as f32)
                .max(0.0);
            let zoom = 0.88 + self.bass * 0.08;
            let map = |p: Point| {
                [
                    area.x as f32 + area.width as f32 * 0.5 + p[0] * scale * zoom,
                    area.y as f32 + area.height as f32 * 0.5 + p[1] * scale * zoom * 0.5,
                ]
            };
            for (i, &(a, b)) in self.segments.iter().enumerate() {
                let a = map(a);
                let b = map(b);
                let delta = [b[0] - a[0], b[1] - a[1]];
                let steps = delta[0].abs().max(delta[1].abs()).ceil().max(1.0) as usize;
                let position = i as f32 / self.segments.len().max(1) as f32;
                let band = unit(bands[(position * 31.0) as usize]);
                let pulse = (position * TAU * 3.0 - self.phase * 2.0).cos() * 0.5 + 0.5;
                let intensity = 0.35 + 0.24 * pulse + 0.20 * band;
                let color = theme.sample(
                    palette,
                    position + (1.0 + self.phase.sin()) * 0.04,
                    intensity,
                );
                let mark = match style {
                    6 => '·',
                    7 => {
                        if delta[1].abs() < 0.15 {
                            '─'
                        } else if delta[0] * delta[1] > 0.0 {
                            '╲'
                        } else {
                            '╱'
                        }
                    }
                    8 => {
                        if pulse > 0.8 {
                            '◆'
                        } else {
                            '•'
                        }
                    }
                    _ => {
                        if delta[0].abs() > delta[1].abs() {
                            '━'
                        } else {
                            '┃'
                        }
                    }
                };
                for step in 0..=steps {
                    let u = step as f32 / steps as f32;
                    let x = (a[0] + delta[0] * u).round() as i32;
                    let y = (a[1] + delta[1] * u).round() as i32;
                    if x >= visible.x as i32
                        && x < visible.right() as i32
                        && y >= visible.y as i32
                        && y < visible.bottom() as i32
                    {
                        buf[(x as u16, y as u16)].set_char(mark).set_fg(color);
                    }
                }
            }
            return;
        }
        // A small bounded camera excursion keeps both the set and empty corners visible.
        let span = if style == 0 {
            3.6
        } else if style < 4 {
            3.4
        } else {
            3.8
        };
        let zoom = 1.0 + self.bass * 0.22;
        let half_x = span * 0.5 / zoom;
        let aspect = area.width as f32 / (2.0 * area.height as f32);
        let half_y = half_x / aspect.max(0.1);
        let half_x = half_x.max(half_y * 0.65);
        let center = if style == 0 {
            [-0.35, -0.40]
        } else {
            [0.0, 0.0]
        };
        let drift = self.camera_drift();
        let limit = 40 + (unit(audio.centroid) * 24.0) as usize;
        // Chroma rotates the target polynomial z^n = a, not arbitrary basin noise.
        let chroma = audio
            .chroma
            .iter()
            .enumerate()
            .fold([0.0, 0.0], |s, (i, &v)| {
                let theta = i as f32 * TAU / 12.0;
                [s[0] + unit(v) * theta.cos(), s[1] + unit(v) * theta.sin()]
            });
        let root_angle =
            chroma[1].atan2(chroma[0]) * 0.12 * (chroma[0].hypot(chroma[1]) / 3.0).min(1.0);
        let target = [root_angle.cos(), root_angle.sin()];
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                let px = ((x - area.x) as f32 + 0.5) / area.width as f32 * 2.0 - 1.0;
                let py = ((y - area.y) as f32 + 0.25) / area.height as f32 * 2.0 - 1.0;
                let sample = |offset: f32| {
                    let z = [
                        center[0] + drift[0] + px * half_x,
                        center[1] + drift[1] + (py + offset) * half_y,
                    ];
                    if style < 4 {
                        escape(z, style, limit)
                    } else {
                        newton(z, style - 1, target, limit)
                    }
                };
                let top = sample(0.0);
                let bottom = sample(1.0 / area.height as f32);
                let a = theme.sample(palette, top.0 + (1.0 + self.phase.sin()) * 0.015, top.1);
                let b = theme.sample(
                    palette,
                    bottom.0 + (1.0 + self.phase.sin()) * 0.015,
                    bottom.1,
                );
                buf[(x, y)].reset();
                buf[(x, y)].set_char('▀').set_fg(a).set_bg(b);
            }
        }
    }

    fn camera_drift(&self) -> Point {
        [self.phase.sin() * 0.035, self.phase.cos() * 0.025]
    }

    fn curves(&mut self, style: usize, audio: &Reactivity) {
        self.segments.clear();
        let detail = usize::from(self.attack > 0.30) + usize::from(unit(audio.centroid) > 0.65);
        match style {
            6 => sierpinski(
                &mut self.segments,
                [-1.0, 0.72],
                [1.0, 0.72],
                [0.0, -1.0],
                4 + detail,
            ),
            7 => {
                let pts = [[-0.866, -0.5], [0.866, -0.5], [0.0, 1.0]];
                for i in 0..3 {
                    koch(
                        &mut self.segments,
                        pts[i],
                        pts[(i + 1) % 3],
                        3 + detail.min(1),
                    );
                }
            }
            8 => {
                // Heighway dragon: append the reversed path rotated 90 degrees about its tip.
                let mut points = vec![[0.0, 0.0], [1.0, 0.0]];
                for _ in 0..(10 + detail) {
                    let n = points.len();
                    let pivot = points[n - 1];
                    for i in (0..n - 1).rev() {
                        let d = [points[i][0] - pivot[0], points[i][1] - pivot[1]];
                        points.push([pivot[0] - d[1], pivot[1] + d[0]]);
                    }
                }
                self.segments
                    .extend(points.windows(2).map(|s| (s[0], s[1])));
            }
            _ => {
                let side = 1_u32 << (4 + detail.min(1));
                let mut previous = hilbert(side, 0);
                for d in 1..side * side {
                    let p = hilbert(side, d);
                    self.segments.push((previous, p));
                    previous = p;
                }
            }
        }
        debug_assert!(self.segments.len() <= MAX_SEGMENTS);
        // Normalize once, preserving topology and keeping recursive corners framed.
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for &(a, b) in &self.segments {
            for p in [a, b] {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(0.001) * 0.5;
        let angle = if style == 8 {
            self.phase.sin() * 0.08
        } else {
            0.0
        };
        let (sin, cos) = angle.sin_cos();
        for (a, b) in &mut self.segments {
            for p in [a, b] {
                let x = (p[0] - (hi[0] + lo[0]) * 0.5) / span;
                let y = (p[1] - (hi[1] + lo[1]) * 0.5) / span;
                *p = [(x * cos - y * sin) * 0.92, (x * sin + y * cos) * 0.92];
            }
        }
    }
}

fn escape(c: Point, style: usize, limit: usize) -> (f32, f32) {
    let degree = if style < 2 { 2 } else { style + 1 };
    let mut z = [0.0_f32; 2];
    for i in 0..limit.min(MAX_ITER) {
        let input = match style {
            0 => [z[0].abs(), z[1].abs()],
            1 => [z[0], -z[1]],
            _ => z,
        };
        let p = power(input, degree);
        z = [p[0] + c[0], p[1] + c[1]];
        let norm = z[0] * z[0] + z[1] * z[1];
        if norm > 256.0 {
            let smooth = i as f32 + 1.0 - (norm.ln() * 0.5).ln() / (degree as f32).ln();
            let t = (smooth / 28.0).max(0.0);
            return (t, 0.18 + 0.65 * (1.0 - (-t * 3.0).exp()));
        }
    }
    (0.65, 0.035)
}

fn newton(mut z: Point, degree: usize, target: Point, limit: usize) -> (f32, f32) {
    for i in 0..limit.min(MAX_ITER) {
        let p = power(z, degree);
        let f = [p[0] - target[0], p[1] - target[1]];
        let error = f[0] * f[0] + f[1] * f[1];
        if error < 1e-8 {
            let root = ((z[1].atan2(z[0]) - target[1].atan2(target[0]) / degree as f32)
                * degree as f32
                / TAU)
                .round();
            let phase = root.rem_euclid(degree as f32) / degree as f32;
            let smooth = (i as f32 + (error.max(1e-20).ln().abs().ln() / 4.0).clamp(0.0, 1.0))
                / MAX_ITER as f32;
            return (phase + smooth * 0.30, 0.78 - (smooth * 0.8).min(0.55));
        }
        let d = power(z, degree - 1);
        let denom = (d[0] * d[0] + d[1] * d[1]) * degree as f32;
        if denom < 1e-16 || !denom.is_finite() {
            break;
        }
        z[0] -= (f[0] * d[0] + f[1] * d[1]) / denom;
        z[1] -= (f[1] * d[0] - f[0] * d[1]) / denom;
        if !z[0].is_finite() || !z[1].is_finite() {
            break;
        }
    }
    (0.0, 0.04)
}

fn sierpinski(out: &mut Vec<Segment>, a: Point, b: Point, c: Point, depth: usize) {
    if depth == 0 {
        out.extend([(a, b), (b, c), (c, a)]);
        return;
    }
    let ab = mid(a, b);
    let bc = mid(b, c);
    let ca = mid(c, a);
    sierpinski(out, a, ab, ca, depth - 1);
    sierpinski(out, ab, b, bc, depth - 1);
    sierpinski(out, ca, bc, c, depth - 1);
}
fn koch(out: &mut Vec<Segment>, a: Point, b: Point, depth: usize) {
    if depth == 0 {
        out.push((a, b));
        return;
    }
    let v = [(b[0] - a[0]) / 3.0, (b[1] - a[1]) / 3.0];
    let p = [a[0] + v[0], a[1] + v[1]];
    let q = [a[0] + 2.0 * v[0], a[1] + 2.0 * v[1]];
    let peak = [
        p[0] + v[0] * 0.5 + v[1] * (PI / 3.0).sin(),
        p[1] - v[0] * (PI / 3.0).sin() + v[1] * 0.5,
    ];
    for (u, v) in [(a, p), (p, peak), (peak, q), (q, b)] {
        koch(out, u, v, depth - 1);
    }
}
fn hilbert(side: u32, mut d: u32) -> Point {
    let (mut x, mut y) = (0_i32, 0_i32);
    let mut s = 1;
    while s < side as i32 {
        let rx = ((d / 2) & 1) as i32;
        let ry = ((d ^ (rx as u32)) & 1) as i32;
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - x;
                y = s - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += s * rx;
        y += s * ry;
        d /= 4;
        s *= 2;
    }
    [x as f32, y as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn loud() -> Reactivity {
        Reactivity {
            bass: 1.0,
            mid: 0.8,
            centroid: 0.9,
            onset: 0.9,
            chroma: std::array::from_fn(|i| if i == 3 { 1.0 } else { 0.0 }),
            transients: [1.0; 4],
            ..Reactivity::default()
        }
    }
    fn render(style: usize, audio: &Reactivity) -> Buffer {
        let area = Rect::new(0, 0, 100, 32);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        renderer.draw(
            &mut buf,
            area,
            0.0,
            style,
            0,
            &[0.0; 32],
            audio,
            &Theme::default(),
        );
        buf
    }
    fn structure(buf: &Buffer) -> Vec<(String, bool)> {
        // Geometry must change even without palette changes: line occupancy or basin boundary.
        buf.content
            .iter()
            .map(|cell| (cell.symbol().to_owned(), cell.fg == cell.bg))
            .collect()
    }
    #[test]
    fn ten_nonempty_distinct_quiet_modes() {
        assert_eq!(NAMES.len(), 10);
        let frames: Vec<_> = (0..10).map(|i| render(i, &Reactivity::default())).collect();
        for (i, a) in frames.iter().enumerate() {
            assert!(
                a.content.iter().any(|c| c.symbol() != " " && c.fg != c.bg),
                "mode {i}"
            );
            for b in &frames[..i] {
                assert_ne!(a, b, "mode {i} duplicates");
            }
        }
    }
    #[test]
    fn clipping_matches_offset_crop_and_disjoint_is_untouched() {
        let theme = Theme::default();
        let audio = Reactivity::default();
        for style in 0..10 {
            let area = Rect::new(3, 4, 61, 23);
            let mut full = Buffer::empty(Rect::new(0, 0, 70, 32));
            let mut crop = Buffer::empty(Rect::new(17, 12, 15, 8));
            Renderer::default().draw(&mut full, area, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            Renderer::default().draw(&mut crop, area, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            for y in 12..20 {
                for x in 17..32 {
                    assert_eq!(full[(x, y)], crop[(x, y)], "style {style}");
                }
            }
            let before = crop.clone();
            Renderer::default().draw(
                &mut crop,
                Rect::new(100, 100, 5, 5),
                0.0,
                style,
                0,
                &[0.0; 32],
                &audio,
                &theme,
            );
            assert_eq!(before, crop);
            for tiny in [
                Rect::new(17, 12, 0, 1),
                Rect::new(17, 12, 1, 0),
                Rect::new(17, 12, 1, 1),
            ] {
                Renderer::default()
                    .draw(&mut crop, tiny, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            }
        }
    }
    #[test]
    fn audio_changes_geometry_not_only_palette() {
        for style in 0..10 {
            let quiet = render(style, &Reactivity::default());
            let active = render(style, &loud());
            if style >= 6 {
                assert_ne!(structure(&quiet), structure(&active), "style {style}");
            } else {
                // Fixed theme with identical pigment roles removes phase-only coloring.
                let theme = Theme {
                    ansi: [[170, 170, 170]; 16],
                    background: [0; 3],
                    foreground: [170; 3],
                };
                let area = Rect::new(0, 0, 100, 32);
                let mut a = Buffer::empty(area);
                let mut b = a.clone();
                Renderer::default().draw(
                    &mut a,
                    area,
                    0.0,
                    style,
                    0,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &theme,
                );
                Renderer::default().draw(&mut b, area, 0.0, style, 0, &[0.0; 32], &loud(), &theme);
                assert_ne!(a, b, "style {style} geometry/intensity unchanged");
            }
        }
    }
    #[test]
    fn recurrence_landmarks_and_curve_topology() {
        assert_eq!(escape([0.0, 0.0], 0, 64).1, 0.035);
        for style in 0..4 {
            assert!(escape([4.0, 3.0], style, 64).1 > 0.035);
        }
        for degree in [3, 4] {
            for root in 0..degree {
                let angle = root as f32 * TAU / degree as f32;
                let (phase, brightness) =
                    newton([angle.cos(), angle.sin()], degree, [1.0, 0.0], 64);
                assert!((phase - root as f32 / degree as f32).abs() < 0.04);
                assert!(brightness > 0.7);
            }
        }
        let mut r = Renderer::default();
        for style in 6..10 {
            r.curves(style, &Reactivity::default());
            let expected = match style {
                6 => 3 * 3_usize.pow(4),
                7 => 3 * 4_usize.pow(3),
                8 => 1024,
                _ => 255,
            };
            assert_eq!(r.segments.len(), expected);
            if style >= 7 {
                for pair in r.segments.windows(2) {
                    assert!((pair[0].1[0] - pair[1].0[0]).abs() < 1e-5);
                    assert!((pair[0].1[1] - pair[1].0[1]).abs() < 1e-5);
                }
            }
        }
        let mut previous = hilbert(16, 0);
        for d in 1..256 {
            let p = hilbert(16, d);
            assert_eq!((p[0] - previous[0]).abs() + (p[1] - previous[1]).abs(), 1.0);
            previous = p;
        }
    }
    #[test]
    fn phase_wrap_preserves_geometry_and_palette() {
        let area = Rect::new(0, 0, 100, 32);
        let theme = Theme::default();
        let audio = Reactivity::default();
        for style in 0..NAMES.len() {
            let mut before = Renderer {
                phase: TAU - 0.000001,
                last_time: Some(0.0),
                ..Renderer::default()
            };
            let mut after = Renderer {
                phase: 0.000001,
                last_time: Some(0.0),
                ..Renderer::default()
            };
            let drift_before = before.camera_drift();
            let drift_after = after.camera_drift();
            for k in 0..2 {
                assert!((drift_before[k] - drift_after[k]).abs() < 0.000001);
            }
            let mut a = Buffer::empty(area);
            let mut b = a.clone();
            before.draw(&mut a, area, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            after.draw(&mut b, area, 0.0, style, 0, &[0.0; 32], &audio, &theme);
            let rgb = |color: ratatui::style::Color| match color {
                ratatui::style::Color::Rgb(r, g, b) => [r, g, b],
                _ => [0, 0, 0],
            };
            let mut error = 0_u64;
            for (a, b) in a.content.iter().zip(&b.content) {
                for (a, b) in [(a.fg, b.fg), (a.bg, b.bg)] {
                    for (a, b) in rgb(a).iter().zip(rgb(b)) {
                        error += a.abs_diff(b) as u64;
                    }
                }
            }
            // Escape boundaries amplify float rounding. Mean channel error
            // captures visible changes without treating one-bit rounding as a jump.
            let mean_error = error as f32 / (a.content.len() * 6) as f32;
            assert!(
                mean_error < 2.0,
                "wrap mean channel error {mean_error} in mode {style}"
            );
        }
    }

    #[test]
    fn finite_bounded_state_and_capped_clock() {
        let mut r = Renderer::default();
        let audio = loud();
        r.advance(0.0, &audio);
        let before = r.phase;
        r.advance(1e20, &audio);
        assert!((r.phase - before).abs() < 0.01);
        r.advance(f32::NAN, &audio);
        for i in 0..2000 {
            r.advance(i as f32, &audio);
            r.curves(6 + i % 4, &audio);
            assert!(r.phase.is_finite() && r.bass.is_finite() && r.attack.is_finite());
            assert!(r.segments.len() <= MAX_SEGMENTS);
            assert!(r
                .segments
                .iter()
                .flat_map(|(a, b)| a.iter().chain(b.iter()))
                .all(|v| v.is_finite() && v.abs() < 1.5));
        }
    }
    #[test]
    #[ignore = "release frame timing: cargo test --release fractalmath::tests::frame_timing -- --ignored --nocapture"]
    fn frame_timing() {
        let area = Rect::new(0, 0, 160, 50);
        let mut buf = Buffer::empty(area);
        let theme = Theme::default();
        let audio = loud();
        for (style, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let mut total = std::time::Duration::ZERO;
            for frame in 0..30 {
                let start = std::time::Instant::now();
                r.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.4; 32],
                    &audio,
                    &theme,
                );
                total += start.elapsed();
            }
            println!(
                "{}: {:.3} ms/frame",
                name,
                total.as_secs_f64() * 1000.0 / 30.0
            );
            assert!(total.as_secs_f64() / 30.0 < 0.016);
        }
    }
}
