use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};

pub const NAMES: &[&str] = &[
    "Voronoi Glass",
    "Kinetic Grid",
    "Truchet Flow",
    "Quilt Engine",
    "Isometric Bloom",
    "Glass Shards",
    "Tangram Dance",
    "Hex Resonance",
    "Orbit Tiles",
    "Crystal Assembly",
];

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    envelopes: [f32; 5],
}

const TAU: f32 = std::f32::consts::TAU;
type Point = [f32; 2];

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn hash(i: usize) -> f32 {
    let mut n = (i as u32).wrapping_mul(747796405).wrapping_add(2891336453);
    n = ((n >> ((n >> 28) + 4)) ^ n).wrapping_mul(277803737);
    ((n >> 22) ^ n) as f32 / u32::MAX as f32
}
fn rotate(p: Point, angle: f32) -> Point {
    let (s, c) = angle.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1]]
}

struct Shape {
    vertices: Vec<Point>,
    pigment: f32,
    shade: f32,
    wire: bool,
}
struct Frame {
    shapes: Vec<Shape>,
    paths: Vec<(Vec<Point>, f32)>,
    seeds: Vec<Point>,
}
impl Frame {
    fn polygon(&mut self, points: &[Point], pigment: f32, shade: f32, wire: bool) {
        self.shapes.push(Shape {
            vertices: points.to_vec(),
            pigment,
            shade,
            wire,
        });
    }
    fn regular(
        &mut self,
        center: Point,
        radius: f32,
        n: usize,
        angle: f32,
        pigment: f32,
        wire: bool,
    ) {
        let points: Vec<_> = (0..n)
            .map(|i| {
                let a = angle + i as f32 * TAU / n as f32;
                [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
            })
            .collect();
        self.polygon(&points, pigment, 0.68, wire);
    }
}
impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
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
            unit(audio.bass.max(audio.kick)),
            unit(audio.mid),
            unit(audio.treble),
            unit(audio.onset.max(audio.novelty)),
            unit(audio.centroid),
        ];
        for (value, target) in self.envelopes.iter_mut().zip(targets) {
            *value += (target - *value) * (1.0 - (-dt * 9.0).exp());
        }
        self.phase = (self.phase + dt * (0.19 + self.envelopes[1] * 0.24)).rem_euclid(TAU * 20.0);
    }

    fn geometry(&self, style: usize, aspect: f32, bands: &[f32; 32]) -> Frame {
        let mut f = Frame {
            shapes: Vec::with_capacity(128),
            paths: Vec::with_capacity(128),
            seeds: Vec::new(),
        };
        let [bass, mid, high, attack, centroid] = self.envelopes;
        let t = self.phase;
        match style % NAMES.len() {
            0 => {
                // Moving sites define genuine nearest-site partitions, not polygon stamps.
                for i in 0..24 {
                    let x = (i % 6) as f32 / 5.0 * 1.8 - 0.9;
                    let y = (i / 6) as f32 / 3.0 * 1.8 - 0.9;
                    f.seeds.push([
                        x * aspect + (t + i as f32).sin() * (0.025 + bass * 0.15),
                        y + (t * 0.7 + i as f32 * 1.7).cos() * (0.03 + mid * 0.13),
                    ]);
                }
            }
            1 => {
                let cols = 9;
                let rows = 6;
                let point = |x: usize, y: usize| -> Point {
                    let u = x as f32 / cols as f32;
                    let v = y as f32 / rows as f32;
                    [
                        aspect * (u * 1.8 - 0.9) + (v * TAU + t).sin() * (0.03 + bass * 0.13),
                        v * 1.8 - 0.9 + (u * TAU - t).sin() * (0.025 + mid * 0.13),
                    ]
                };
                for y in 0..rows {
                    for x in 0..cols {
                        let a = point(x, y);
                        let b = point(x + 1, y);
                        let c = point(x + 1, y + 1);
                        let d = point(x, y + 1);
                        let center = [
                            (a[0] + b[0] + c[0] + d[0]) * 0.25,
                            (a[1] + b[1] + c[1] + d[1]) * 0.25,
                        ];
                        let shrink = 0.70 + unit(bands[(x + y * cols) % 32]) * 0.16;
                        let p = [a, b, c, d].map(|p| {
                            [
                                center[0] + (p[0] - center[0]) * shrink,
                                center[1] + (p[1] - center[1]) * shrink,
                            ]
                        });
                        f.polygon(&p, x as f32 / cols as f32, 0.6, true);
                    }
                }
            }
            2 => {
                // Quarter circles meet at tile-edge midpoints. Radius and tile pitch
                // change together, preserving continuous curved Truchet connections.
                let pitch = (0.32 + bass * 0.10).max(aspect / 20.0);
                let cols = ((aspect * 2.0 / pitch).ceil() as usize).min(40);
                let rows = (2.0 / pitch).ceil() as usize;
                for y in 0..rows {
                    for x in 0..cols {
                        let left = x as f32 * pitch - aspect;
                        let top = y as f32 * pitch - 1.0;
                        let flip = hash(x + y * 47) > 0.5;
                        for side in 0..2 {
                            let (cx, cy, start) = match (flip, side) {
                                (false, 0) => (left, top, 0.0),
                                (false, _) => (left + pitch, top + pitch, std::f32::consts::PI),
                                (true, 0) => (left + pitch, top, std::f32::consts::FRAC_PI_2),
                                (true, _) => (left, top + pitch, -std::f32::consts::FRAC_PI_2),
                            };
                            let path = (0..19)
                                .map(|i| {
                                    let a = start + i as f32 / 18.0 * std::f32::consts::FRAC_PI_2;
                                    [cx + pitch * 0.5 * a.cos(), cy + pitch * 0.5 * a.sin()]
                                })
                                .collect();
                            f.paths.push((path, hash(x + y * 17 + side)));
                        }
                    }
                }
            }
            3 => {
                // Each square is completely fitted by four triangles sharing one
                // wandering internal junction: no gaps or overlapping random tiles.
                let cols = 7;
                for y in 0..5 {
                    for x in 0..cols {
                        let w = aspect * 2.0 / cols as f32;
                        let h = 0.4;
                        let a = [-aspect + x as f32 * w, -1.0 + y as f32 * h];
                        let corners = [a, [a[0] + w, a[1]], [a[0] + w, a[1] + h], [a[0], a[1] + h]];
                        let center = [
                            a[0] + w * (0.5 + (t + x as f32).sin() * (0.08 + mid * 0.2)),
                            a[1] + h * (0.5 + (t + y as f32).cos() * (0.08 + bass * 0.2)),
                        ];
                        for j in 0..4 {
                            f.polygon(
                                &[corners[j], corners[(j + 1) % 4], center],
                                (x + y * cols + j) as f32 * 0.083,
                                0.32 + j as f32 * 0.15,
                                false,
                            );
                            f.paths
                                .push((vec![corners[j], center], (x + y * cols) as f32 * 0.083));
                        }
                    }
                }
            }
            4 => {
                // Three visible parallelogram faces share edges on each lifted cube.
                for row in 0..5 {
                    for col in 0..5 {
                        let u = col as f32 - 2.0;
                        let v = row as f32 - 2.0;
                        let size = 0.115;
                        let x = (u - v) * size * 1.5;
                        let y = (u + v) * size * 0.75;
                        let lift = 0.07
                            + bass * 0.18 * (1.0 - ((u * u + v * v) / 10.0).min(1.0))
                            + 0.035 * (t + u + v).sin();
                        let a = [x, y - lift - size];
                        let b = [x + size, y - lift - size * 0.5];
                        let c = [x, y - lift];
                        let d = [x - size, y - lift - size * 0.5];
                        let h = 0.10 + unit(bands[(row * 5 + col) % 32]) * 0.12;
                        f.polygon(
                            &[d, c, [c[0], c[1] + h], [d[0], d[1] + h]],
                            0.15 + row as f32 * 0.06,
                            0.36,
                            false,
                        );
                        f.polygon(
                            &[c, b, [b[0], b[1] + h], [c[0], c[1] + h]],
                            0.32 + col as f32 * 0.06,
                            0.58,
                            false,
                        );
                        f.polygon(&[a, b, c, d], 0.12 + (u + v) * 0.07, 0.85, false);
                    }
                }
            }
            5 => {
                // Radial triangular fragments open along fracture lines. Each shard
                // has its own angular velocity and an outward attack displacement.
                for i in 0..18 {
                    let a = i as f32 * TAU / 18.0;
                    let spread = 0.025 + attack * 0.25 + high * hash(i) * 0.12;
                    let drift = rotate([spread, 0.0], a);
                    let radius = 0.68 + hash(i + 31) * 0.23;
                    let points = [
                        [0.05, 0.0],
                        [radius, -0.07 - hash(i) * 0.1],
                        [radius * 0.79, 0.08],
                    ]
                    .map(|p| {
                        let q = rotate(p, a + (t * 0.22).sin() * hash(i) * 0.14);
                        [q[0] + drift[0], q[1] + drift[1]]
                    });
                    f.polygon(&points, hash(i + 50), 0.3 + hash(i + 8) * 0.55, false);
                }
            }
            6 => {
                // The canonical seven-piece square tangram: two large triangles,
                // one medium, two small, a square and a parallelogram.
                let pieces: [&[Point]; 7] = [
                    &[[0., 0.], [1., 0.], [0.5, 0.5]],
                    &[[0., 0.], [0.5, 0.5], [0., 1.]],
                    &[[1., 1.], [0.5, 1.], [1., 0.5]],
                    &[[0.5, 0.5], [0.75, 0.25], [0.75, 0.75]],
                    &[[0., 1.], [0.25, 0.75], [0.5, 1.]],
                    &[[0.5, 0.5], [0.75, 0.75], [0.5, 1.], [0.25, 0.75]],
                    &[[1., 0.], [1., 0.5], [0.75, 0.75], [0.75, 0.25]],
                ];
                for (i, piece) in pieces.iter().enumerate() {
                    let center = piece.iter().fold([0., 0.], |a, p| {
                        [
                            a[0] + p[0] / piece.len() as f32,
                            a[1] + p[1] / piece.len() as f32,
                        ]
                    });
                    let angle = (t + i as f32 * 0.7).sin() * (0.04 + mid * 0.8);
                    let separation = 0.08 + attack * 0.6;
                    let points: Vec<_> = piece
                        .iter()
                        .map(|p| {
                            let q = rotate([p[0] - center[0], p[1] - center[1]], angle);
                            [
                                (q[0] + (center[0] - 0.5) * (1.0 + separation)) * 1.4,
                                (q[1] + (center[1] - 0.5) * (1.0 + separation)) * 1.4,
                            ]
                        })
                        .collect();
                    f.polygon(&points, i as f32 / 7.0, 0.45 + i as f32 * 0.06, false);
                }
            }
            7 => {
                let r = 0.16;
                for row in 0..7 {
                    for col in 0..9 {
                        let x = (col as f32 - 4.0) * r * 1.5;
                        let y = (row as f32 - 3.0) * r * 1.732 + (col % 2) as f32 * r * 0.866;
                        let wave = (x * 3.0 + y * 4.0 - t * 2.0).sin();
                        let radius = r
                            * (0.68
                                + 0.1 * wave
                                + bass * 0.15
                                + unit(bands[(col + row * 9) % 32]) * 0.08);
                        f.regular([x, y], radius, 6, 0.0, 0.3 + wave * 0.16, true);
                        if high > 0.15 && (row + col) % 3 == 0 {
                            f.regular([x, y], radius * 0.38, 6, t * 0.2, 0.7, false);
                        }
                    }
                }
            }
            8 => {
                for ring in 0..3 {
                    let radius = 0.25 + ring as f32 * 0.24 + bass * 0.07;
                    let count = 7 + ring * 4;
                    // Thin elliptical trajectories, with independently spinning
                    // rectangular tiles rather than another circular polygon field.
                    f.paths.push((
                        (0..81)
                            .map(|i| {
                                let a = i as f32 * TAU / 80.0;
                                [radius * a.cos() * 1.25, radius * a.sin() * 0.78]
                            })
                            .collect(),
                        0.12 + ring as f32 * 0.2,
                    ));
                    for i in 0..count {
                        let a = i as f32 * TAU / count as f32
                            + t * (if ring % 2 == 0 { 1.0 } else { -0.7 });
                        let center = [radius * a.cos() * 1.25, radius * a.sin() * 0.78];
                        let w = 0.035 + high * 0.026;
                        let h = 0.055 + mid * 0.045;
                        let points = [[-w, -h], [w, -h], [w, h], [-w, h]].map(|p| {
                            let q = rotate(p, a + t * (0.3 + centroid));
                            [center[0] + q[0], center[1] + q[1]]
                        });
                        f.polygon(&points, i as f32 / count as f32, 0.8, false);
                    }
                }
            }
            _ => {
                // Six elongated crystal prisms dock toward a common nucleus.
                // Each prism has a tip, two side facets and a central ridge.
                let docking = 0.5 + 0.5 * (t * 0.6).sin();
                for i in 0..6 {
                    let a = i as f32 * TAU / 6.0 + 0.12 * (t * 0.3).sin();
                    let distance = 0.09 + (1.0 - docking) * 0.24 + attack * 0.17;
                    let length = 0.40 + unit(bands[i * 5]) * 0.18;
                    let w = 0.075 + mid * 0.035;
                    let p = [
                        [distance, -w],
                        [distance + length * 0.8, -w],
                        [distance + length, 0.0],
                        [distance + length * 0.8, w],
                        [distance, w],
                        [distance + length * 0.72, 0.0],
                    ]
                    .map(|p| rotate(p, a));
                    f.polygon(&[p[0], p[1], p[2], p[5]], i as f32 / 6.0, 0.8, false);
                    f.polygon(
                        &[p[5], p[2], p[3], p[4]],
                        i as f32 / 6.0 + 0.04,
                        0.40,
                        false,
                    );
                    f.polygon(&[p[0], p[5], p[4]], i as f32 / 6.0 + 0.08, 0.60, false);
                }
                f.regular([0., 0.], 0.08 + bass * 0.05, 6, t * 0.2, 0.5, true);
            }
        }
        f
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
        if area.width == 0 || area.height == 0 || visible.width == 0 || visible.height == 0 {
            return;
        }
        let aspect = area.width as f32 / (area.height as f32 * 2.0);
        let frame = self.geometry(style, aspect, bands);
        let background = theme.sample(palette, 0.0, 0.025);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)].reset();
                buf[(x, y)].set_char(' ').set_bg(background);
            }
        }
        if style.is_multiple_of(NAMES.len()) {
            let colors: Vec<_> = (0..frame.seeds.len())
                .map(|i| theme.sample(palette, hash(i + 100), 0.30 + hash(i + 90) * 0.42))
                .collect();
            for y in visible.y..visible.bottom() {
                for x in visible.x..visible.right() {
                    let mut shades = [background; 2];
                    for (half, shade) in shades.iter_mut().enumerate() {
                        let p = world(x, y, half, area);
                        let mut nearest = (f32::MAX, 0);
                        let mut second = f32::MAX;
                        for (i, s) in frame.seeds.iter().enumerate() {
                            let d = (p[0] - s[0]).powi(2) + (p[1] - s[1]).powi(2);
                            if d < nearest.0 {
                                second = nearest.0;
                                nearest = (d, i);
                            } else {
                                second = second.min(d);
                            }
                        }
                        // Difference of distances gives a seam whose width does not
                        // expand with the distance from a site's center.
                        if second.sqrt() - nearest.0.sqrt() > 0.018 {
                            *shade = colors[nearest.1];
                        }
                    }
                    buf[(x, y)]
                        .set_char('▀')
                        .set_fg(shades[0])
                        .set_bg(shades[1]);
                }
            }
        }
        for shape in &frame.shapes {
            let color = theme.sample(palette, shape.pigment, shape.shade);
            if shape.wire {
                for i in 0..shape.vertices.len() {
                    stroke(
                        buf,
                        visible,
                        area,
                        shape.vertices[i],
                        shape.vertices[(i + 1) % shape.vertices.len()],
                        color,
                    );
                }
            } else {
                fill(buf, visible, area, &shape.vertices, color);
            }
        }
        for (path, pigment) in &frame.paths {
            let color = theme.sample(palette, *pigment, 0.75);
            for pair in path.windows(2) {
                stroke(buf, visible, area, pair[0], pair[1], color);
            }
        }
    }
}

fn world(x: u16, y: u16, half: usize, area: Rect) -> Point {
    let h = area.height as f32;
    [
        ((x - area.x) as f32 + 0.5 - area.width as f32 * 0.5) / h,
        ((y - area.y) as f32 * 2.0 + half as f32 + 0.5) / h - 1.0,
    ]
}
fn screen(p: Point, area: Rect) -> Point {
    [
        area.x as f32 + area.width as f32 * 0.5 + p[0] * area.height as f32,
        area.y as f32 + (p[1] + 1.0) * area.height as f32 * 0.5,
    ]
}
fn inside(p: Point, vertices: &[Point]) -> bool {
    let mut hit = false;
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            hit = !hit;
        }
    }
    hit
}
fn fill(
    buf: &mut Buffer,
    clip: Rect,
    area: Rect,
    vertices: &[Point],
    color: ratatui::style::Color,
) {
    let mut min = [f32::MAX; 2];
    let mut max = [f32::MIN; 2];
    for p in vertices {
        let p = screen(*p, area);
        for i in 0..2 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let x0 = (min[0].floor().max(clip.x as f32) as u16).min(clip.right());
    let x1 = (max[0].ceil().max(clip.x as f32) as u16).min(clip.right());
    let y0 = (min[1].floor().max(clip.y as f32) as u16).min(clip.bottom());
    let y1 = (max[1].ceil().max(clip.y as f32) as u16).min(clip.bottom());
    for y in y0..y1 {
        for x in x0..x1 {
            for half in 0..2 {
                if inside(world(x, y, half, area), vertices) {
                    let cell = &mut buf[(x, y)];
                    if cell.symbol() != "▀" {
                        let old = cell.bg;
                        cell.set_char('▀').set_fg(old);
                    }
                    if half == 0 {
                        cell.set_fg(color);
                    } else {
                        cell.set_bg(color);
                    }
                }
            }
        }
    }
}
fn stroke(
    buf: &mut Buffer,
    clip: Rect,
    area: Rect,
    a: Point,
    b: Point,
    color: ratatui::style::Color,
) {
    let a = screen(a, area);
    let b = screen(b, area);
    // Two by four dot samples per terminal cell; bounded by the viewport scale.
    let steps = ((b[0] - a[0]).abs() * 2.0)
        .max((b[1] - a[1]).abs() * 4.0)
        .ceil()
        .clamp(1.0, 4096.0) as usize;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = a[0] + (b[0] - a[0]) * t;
        let y = a[1] + (b[1] - a[1]) * t;
        if x < clip.x as f32
            || x >= clip.right() as f32
            || y < clip.y as f32
            || y >= clip.bottom() as f32
        {
            continue;
        }
        let col = x.floor() as u16;
        let row = y.floor() as u16;
        let dx = ((x - x.floor()) * 2.0) as usize;
        let dy = ((y - y.floor()) * 4.0) as usize;
        let bit = [[0, 1, 2, 6], [3, 4, 5, 7]][dx.min(1)][dy.min(3)];
        let cell = &mut buf[(col, row)];
        let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
        let mask = if (0x2800..=0x28ff).contains(&old) {
            old - 0x2800
        } else {
            0
        };
        cell.set_char(char::from_u32(0x2800 | mask | (1 << bit)).unwrap())
            .set_fg(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(style: usize, audio: Reactivity, area: Rect) -> (Renderer, Buffer) {
        let mut r = Renderer::default();
        let mut b = Buffer::empty(area);
        for i in 0..45 {
            r.draw(
                &mut b,
                area,
                i as f32 / 60.0,
                style,
                0,
                &[0.; 32],
                &audio,
                &Theme::default(),
            );
        }
        (r, b)
    }
    fn mask(b: &Buffer) -> Vec<(String, bool, bool)> {
        let bg = Theme::default().sample(0, 0., 0.025);
        b.content
            .iter()
            .map(|c| (c.symbol().to_string(), c.fg != bg, c.bg != bg))
            .collect()
    }
    #[test]
    fn clips_and_preserves_outside() {
        let outer = Rect::new(10, 7, 31, 17);
        for style in 0..10 {
            for area in [
                Rect::new(14, 10, 20, 9),
                Rect::new(0, 0, 15, 12),
                Rect::new(60, 60, 8, 8),
                Rect::new(9, 9, 0, 0),
                Rect::new(40, 23, 1, 1),
            ] {
                let mut b = Buffer::empty(outer);
                for c in &mut b.content {
                    c.set_char('!');
                }
                Renderer::default().draw(
                    &mut b,
                    area,
                    0.,
                    style,
                    0,
                    &[0.; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                let visible = area.intersection(outer);
                for y in outer.y..outer.bottom() {
                    for x in outer.x..outer.right() {
                        if !visible.contains((x, y).into()) {
                            assert_eq!(b[(x, y)].symbol(), "!");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn audio_changes_structure_in_every_mode() {
        let audio = Reactivity {
            bass: 1.,
            mid: 1.,
            treble: 1.,
            onset: 1.,
            centroid: 1.,
            ..Default::default()
        };
        for style in 0..10 {
            let (_, quiet) = render(style, Reactivity::default(), Rect::new(0, 0, 100, 34));
            let (_, loud) = render(style, audio, Rect::new(0, 0, 100, 34));
            let difference = mask(&quiet)
                .iter()
                .zip(mask(&loud))
                .filter(|(a, b)| **a != *b)
                .count();
            assert!(
                difference > 12,
                "mode {style} changed only {difference} structural cells"
            );
        }
    }
    #[test]
    fn distinct_topologies_and_bounded_finite_geometry() {
        let mut masks = Vec::new();
        for style in 0..10 {
            let (mut r, b) = render(style, Reactivity::default(), Rect::new(0, 0, 80, 30));
            let current = mask(&b);
            assert!(!masks.contains(&current), "duplicate mode {style}");
            masks.push(current);
            for time in [f32::NAN, f32::INFINITY, -1000., 100000., 100001.] {
                r.advance(
                    time,
                    &Reactivity {
                        bass: f32::NAN,
                        mid: f32::INFINITY,
                        ..Default::default()
                    },
                );
                let f = r.geometry(style, 1.5, &[f32::NAN; 32]);
                assert!(f.shapes.len() <= 150 && f.paths.len() <= 560 && f.seeds.len() <= 24);
                assert!(f
                    .shapes
                    .iter()
                    .flat_map(|s| s.vertices.iter())
                    .chain(f.paths.iter().flat_map(|(p, _)| p.iter()))
                    .chain(f.seeds.iter())
                    .flatten()
                    .all(|v| v.is_finite()));
            }
            assert!(r.phase.is_finite() && r.envelopes.iter().all(|v| v.is_finite()));
            let old = r.phase;
            r.advance(1e20, &Reactivity::default());
            assert!((r.phase - old).abs() < 0.1);
        }
    }
    #[test]
    fn fitted_quilt_and_partition_cover_extreme_aspects() {
        for (width, height) in [(200, 9), (9, 90), (1, 20), (80, 1)] {
            for style in [0, 3] {
                let (_, b) = render(style, Reactivity::default(), Rect::new(4, 6, width, height));
                assert!(
                    b.content.iter().all(|c| c.symbol() != " "),
                    "unfilled mode {style} at {width}x{height}"
                );
            }
        }
    }
    #[test]
    fn truchet_is_curved_and_tangram_has_seven_fitted_pieces() {
        let r = Renderer::default();
        let arcs = r.geometry(2, 1.5, &[0.; 32]);
        for (path, _) in arcs.paths {
            assert_eq!(path.len(), 19);
            let a = path[0];
            let b = path[9];
            let c = path[18];
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(cross.abs() > 0.001, "Truchet must not be straight chords");
        }
        let f = r.geometry(6, 1.5, &[0.; 32]);
        assert_eq!(f.shapes.len(), 7);
        let mut areas: Vec<f32> = f
            .shapes
            .iter()
            .map(|s| {
                let v = &s.vertices;
                (0..v.len())
                    .map(|i| v[i][0] * v[(i + 1) % v.len()][1] - v[(i + 1) % v.len()][0] * v[i][1])
                    .sum::<f32>()
                    .abs()
                    * 0.5
            })
            .collect();
        areas.sort_by(f32::total_cmp);
        let expected = [0.0625, 0.0625, 0.125, 0.125, 0.125, 0.25, 0.25];
        for (actual, expected) in areas.iter().zip(expected) {
            assert!((actual / 1.96 - expected).abs() < 0.0001);
        }
    }
    #[test]
    #[ignore]
    fn frame_budget() {
        let area = Rect::new(0, 0, 160, 50);
        let mut b = Buffer::empty(area);
        for (style, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let start = std::time::Instant::now();
            for i in 0..120 {
                r.draw(
                    &mut b,
                    area,
                    i as f32 / 60.,
                    style,
                    0,
                    &[0.5; 32],
                    &Reactivity {
                        bass: 0.7,
                        mid: 0.4,
                        treble: 0.5,
                        ..Default::default()
                    },
                    &Theme::default(),
                );
            }
            let ms = start.elapsed().as_secs_f64() * 1000. / 120.;
            eprintln!("{name}: {ms:.3} ms/frame");
            assert!(ms < 16., "{name} exceeds frame budget");
        }
    }
}
