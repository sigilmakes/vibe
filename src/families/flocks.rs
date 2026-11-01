use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect};

use crate::{palette::Theme, visuals::Reactivity};

const BIRDS: usize = 132;
const FIREFLIES: usize = 108;
const TRAIL: usize = 7;
const STEP: f32 = 1.0 / 120.0;
const MAX_DT: f32 = 1.0 / 15.0;

#[derive(Clone, Copy, Debug, Default)]
struct V {
    x: f32,
    y: f32,
}

impl V {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }

    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }

    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s)
    }

    fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    fn limit(self, max: f32) -> Self {
        self.mul((max / self.length().max(0.0001)).min(1.0))
    }

    fn unit(self) -> Self {
        self.mul(1.0 / self.length().max(0.0001))
    }
}

fn seed(i: usize, salt: u32) -> f32 {
    let mut n = (i as u32).wrapping_mul(747_796_405).wrapping_add(salt);
    n = (n ^ (n >> 16)).wrapping_mul(2_246_822_519);
    (n ^ (n >> 13)) as f32 / u32::MAX as f32
}

fn level(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Clone)]
struct Particle {
    p: V,
    v: V,
    trail: [V; TRAIL],
    group: usize,
    phase: f32,
}

#[derive(Default)]
struct World {
    particles: Vec<Particle>,
    next: Vec<V>,
    clock: f32,
    remainder: f32,
    trail_clock: f32,
    trail_head: usize,
    scatter: f32,
    pulse: f32,
    radius: f32,
    attack: f32,
    cooldown: f32,
}

impl World {
    fn anchor(&self, group: usize, fireflies: bool, aspect: f32) -> V {
        let g = group as f32;
        let t = self.clock;
        if fireflies {
            let a = g * TAU / 4.0 + t * 0.13;
            V::new(
                (a.cos() * 0.48 + (t * 0.23 + g).sin() * 0.12) * aspect,
                a.sin() * 0.48 + (t * 0.19 - g).cos() * 0.12,
            )
        } else {
            // Different lobes periodically fold together, then peel apart.
            let spread = 0.32 + 0.19 * (t * 0.19).sin();
            V::new(
                ((t * 0.31 + g * 2.1).sin() * spread + (t * 0.14).sin() * 0.18) * aspect,
                (t * 0.39 + g * 1.9).cos() * spread,
            )
        }
    }

    fn init(&mut self, fireflies: bool, aspect: f32) {
        let count = if fireflies { FIREFLIES } else { BIRDS };
        self.radius = 0.13;
        self.particles = (0..count)
            .map(|i| {
                let group = i % if fireflies { 4 } else { 3 };
                let phase = seed(i, 293) * TAU;
                let angle = seed(i, 891) * TAU;
                let radius = seed(i, 417).sqrt() * if fireflies { 0.17 } else { 0.24 };
                let p = self
                    .anchor(group, fireflies, aspect)
                    .add(V::new(angle.cos(), angle.sin()).mul(radius));
                let heading = group as f32 * 2.1 + 0.4;
                Particle {
                    p,
                    v: V::new(heading.cos(), heading.sin()).mul(0.28),
                    trail: [p; TRAIL],
                    group,
                    phase,
                }
            })
            .collect();
        self.next.resize(count, V::default());
    }

    fn advance(
        &mut self,
        dt: f32,
        fireflies: bool,
        aspect: f32,
        audio: &Reactivity,
        bands: &[f32; 32],
    ) {
        if self.particles.is_empty() {
            self.init(fireflies, aspect);
        }
        let attack = level(audio.transients[0])
            .max(level(audio.kick))
            .max(level(audio.onset) * 0.7);
        self.cooldown = (self.cooldown - dt).max(0.0);
        if attack - self.attack > 0.10 && attack > 0.18 && self.cooldown == 0.0 {
            self.scatter = self.scatter.max(attack);
            self.pulse = 1.0;
            self.cooldown = 0.12;
            let anchors: Vec<_> = (0..if fireflies { 4 } else { 3 })
                .map(|g| self.anchor(g, fireflies, aspect))
                .collect();
            for p in &mut self.particles {
                let radial = p.p.sub(anchors[p.group]).unit();
                p.v =
                    p.v.add(radial.mul(attack * if fireflies { 0.75 } else { 0.95 }));
            }
        }
        self.attack = attack;
        self.remainder += dt;
        while self.remainder >= STEP {
            self.step(fireflies, aspect, audio, bands);
            self.remainder -= STEP;
        }
    }

    fn step(&mut self, fireflies: bool, aspect: f32, audio: &Reactivity, bands: &[f32; 32]) {
        self.clock += STEP;
        self.scatter *= (-STEP * 1.8).exp();
        self.pulse *= (-STEP * 4.5).exp();
        let bass = level(audio.bass);
        let treble = level(audio.treble);
        self.radius += (0.13 + bass * 0.10 + self.scatter * 0.25 - self.radius) * STEP * 3.0;
        let anchors: Vec<_> = (0..if fireflies { 4 } else { 3 })
            .map(|g| self.anchor(g, fireflies, aspect))
            .collect();
        for (i, p) in self.particles.iter().enumerate() {
            let to_anchor = anchors[p.group].sub(p.p);
            let force = if fireflies {
                let r = to_anchor.length().max(0.015);
                let outward = to_anchor.mul(-1.0 / r);
                let orbit =
                    V::new(-outward.y, outward.x).mul(if p.group % 2 == 0 { 1.0 } else { -1.0 });
                let desired = self.radius * (0.55 + seed(i, 928) * 0.85);
                let local_band = level(bands[p.group * 8 + i % 8]);
                outward
                    .mul((desired - r) * 5.0)
                    .add(orbit.mul(0.38 + local_band * 0.65 + self.scatter * 0.4))
                    .sub(p.v.mul(1.4))
                    .add(
                        V::new(
                            (p.phase + self.clock * 2.4).sin(),
                            (p.phase * 1.3 + self.clock * 1.8).cos(),
                        )
                        .mul(0.06 + treble * 0.26),
                    )
            } else {
                let mut center = V::default();
                let mut alignment = V::default();
                let mut separation = V::default();
                let mut neighbors = 0.0;
                for (j, other) in self.particles.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let delta = p.p.sub(other.p);
                    let d2 = delta.x * delta.x + delta.y * delta.y;
                    if d2 < 0.34 * 0.34 {
                        center = center.add(other.p);
                        alignment = alignment.add(other.v);
                        neighbors += 1.0;
                        if d2 < 0.09 * 0.09 {
                            separation = separation.add(delta.mul(1.0 / d2.max(0.0003)));
                        }
                    }
                }
                let mut flock = separation.limit(2.0).mul(0.42);
                if neighbors > 0.0 {
                    flock = flock
                        .add(center.mul(1.0 / neighbors).sub(p.p).mul(1.6))
                        .add(alignment.mul(1.0 / neighbors).sub(p.v).mul(2.5));
                }
                let flutter = V::new(
                    (self.clock * 5.1 + p.phase).cos(),
                    (self.clock * 4.3 + p.phase).sin(),
                );
                flock
                    .add(to_anchor.mul(0.65 + (1.0 - self.scatter) * 1.5))
                    .add(flutter.mul(0.025 + treble * 0.25))
            };
            let wall = V::new(
                -p.p.x.signum() * ((p.p.x.abs() / aspect - 0.78).max(0.0) * 7.0),
                -p.p.y.signum() * ((p.p.y.abs() - 0.78).max(0.0) * 7.0),
            );
            let acceleration = if fireflies {
                force.add(wall)
            } else {
                // Anticipate the edge in physical units, before the safety
                // clamp. Near an edge, steering takes priority over flock
                // alignment so a dense group cannot keep pushing outward.
                let steer = |position: f32, velocity: f32, extent: f32, flock: f32| {
                    let predicted = position + velocity * 0.45;
                    let risk = ((predicted.abs() / extent - 0.66) / 0.27).clamp(0.0, 1.0);
                    let weight = risk * risk * (3.0 - 2.0 * risk);
                    let inward = -predicted.signum() * 0.55;
                    flock * (1.0 - weight) + (inward - velocity) * 6.0 * weight
                };
                V::new(
                    steer(p.p.x, p.v.x, aspect, force.x),
                    steer(p.p.y, p.v.y, 1.0, force.y),
                )
            };
            let mut velocity = p.v.add(acceleration.mul(STEP));
            if !fireflies && velocity.length() < 0.22 {
                velocity = velocity.add(V::new(p.phase.cos(), p.phase.sin()).mul(STEP * 0.3));
            }
            self.next[i] = velocity.limit(if fireflies {
                0.85
            } else {
                0.55 + bass * 0.15 + self.scatter * 0.65
            });
        }
        self.trail_clock += STEP;
        let record = self.trail_clock >= 1.0 / 30.0;
        if record {
            self.trail_clock -= 1.0 / 30.0;
            self.trail_head = (self.trail_head + 1) % TRAIL;
        }
        for (p, &v) in self.particles.iter_mut().zip(&self.next) {
            p.v = v;
            p.p = p.p.add(v.mul(STEP));
            let bounds = V::new(aspect * 0.97, 0.97);
            if p.p.x.abs() > bounds.x {
                p.p.x = p.p.x.clamp(-bounds.x, bounds.x);
                p.v.x = -p.v.x * 0.7;
            }
            if p.p.y.abs() > bounds.y {
                p.p.y = p.p.y.clamp(-bounds.y, bounds.y);
                p.v.y = -p.v.y * 0.7;
            }
            if record {
                p.trail[self.trail_head] = p.p;
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Ink {
    dots: u8,
    strength: f32,
    phase: f32,
    mark: Option<char>,
}

/// Two independent, bounded simulations; switching styles preserves their state.
#[derive(Default)]
pub struct Renderer {
    worlds: [World; 2],
    last_time: Option<f32>,
    ink: Vec<Ink>,
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
        let time = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, MAX_DT));
        self.last_time = Some(time);
        let visible = area.intersection(buf.area);
        if visible.width == 0 || visible.height == 0 || area.width == 0 || area.height == 0 {
            return;
        }
        let fireflies = style % 2 == 1;
        let aspect = (area.width as f32 / (area.height as f32 * 2.0)).clamp(0.25, 6.0);
        let world = &mut self.worlds[usize::from(fireflies)];
        world.advance(dt, fireflies, aspect, audio, bands);
        self.ink.clear();
        self.ink.resize(
            visible.width as usize * visible.height as usize,
            Ink::default(),
        );
        for p in &world.particles {
            let phase = if fireflies { 0.18 } else { 0.48 } + p.group as f32 * 0.025;
            let flicker = (world.clock * (1.1 + p.phase * 0.14) + p.phase).sin();
            let glow = if fireflies {
                0.19 + 0.66 * flicker.max(0.0).powi(6)
            } else {
                0.70
            };
            for age in (1..TRAIL).rev() {
                let point = p.trail[(world.trail_head + TRAIL - age) % TRAIL];
                Self::plot(
                    &mut self.ink,
                    visible,
                    area,
                    aspect,
                    point,
                    (if fireflies { glow * 0.32 } else { 0.26 })
                        * (1.0 - age as f32 / TRAIL as f32),
                    phase,
                    None,
                );
            }
            let mark = if fireflies {
                (flicker > 0.987 || (world.pulse > 0.65 && p.phase < 0.7)).then_some('✦')
            } else {
                Some(if p.v.y.abs() > p.v.x.abs() * 2.0 {
                    '│'
                } else if p.v.y.abs() < p.v.x.abs() * 0.4 {
                    '─'
                } else if p.v.x * p.v.y > 0.0 {
                    '╲'
                } else {
                    '╱'
                })
            };
            Self::plot(
                &mut self.ink,
                visible,
                area,
                aspect,
                p.p,
                if mark == Some('✦') { 1.0 } else { glow },
                phase,
                mark,
            );
        }
        for y in 0..visible.height {
            for x in 0..visible.width {
                let ink = self.ink[y as usize * visible.width as usize + x as usize];
                let cell = &mut buf[(visible.x + x, visible.y + y)];
                cell.reset();
                cell.set_bg(theme.sample(palette, 0.0, 0.0));
                if ink.dots != 0 {
                    cell.set_char(
                        ink.mark
                            .unwrap_or(char::from_u32(0x2800 + ink.dots as u32).unwrap()),
                    )
                    .set_fg(theme.sample(palette, ink.phase, ink.strength));
                } else {
                    cell.set_char(' ');
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn plot(
        ink: &mut [Ink],
        visible: Rect,
        area: Rect,
        aspect: f32,
        p: V,
        strength: f32,
        phase: f32,
        mark: Option<char>,
    ) {
        let px = (p.x / aspect + 1.0) * area.width as f32;
        let py = (p.y + 1.0) * area.height as f32 * 2.0;
        if px < 0.0 || py < 0.0 || px >= area.width as f32 * 2.0 || py >= area.height as f32 * 4.0 {
            return;
        }
        let sx = px as u32;
        let sy = py as u32;
        let x = area.x as u32 + sx / 2;
        let y = area.y as u32 + sy / 4;
        if x < visible.x as u32
            || y < visible.y as u32
            || x >= visible.right() as u32
            || y >= visible.bottom() as u32
        {
            return;
        }
        let bits = [[0, 1, 2, 6], [3, 4, 5, 7]];
        let cell = &mut ink[(y - visible.y as u32) as usize * visible.width as usize
            + (x - visible.x as u32) as usize];
        cell.dots |= 1 << bits[(sx % 2) as usize][(sy % 4) as usize];
        if strength > cell.strength {
            cell.strength = strength;
            cell.phase = phase;
            cell.mark = mark;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmuration_turns_out_of_corners() {
        for aspect in [0.25, 1.6, 6.0] {
            for audio in [
                Reactivity::default(),
                Reactivity {
                    bass: 1.0,
                    treble: 1.0,
                    transients: [1.0; 4],
                    ..Default::default()
                },
            ] {
                let mut world = World::default();
                world.init(false, aspect);
                for (i, p) in world.particles.iter_mut().enumerate() {
                    let sx = if i % 2 == 0 { -1.0 } else { 1.0 };
                    let sy = if i % 4 < 2 { -1.0 } else { 1.0 };
                    p.p = V::new(
                        sx * aspect * (0.91 + seed(i, 91) * 0.05),
                        sy * (0.91 + seed(i, 23) * 0.05),
                    );
                    p.v = V::new(sx, sy).unit().mul(0.65);
                    p.trail.fill(p.p);
                }
                let mut edge_frames = vec![0; BIRDS];
                for frame in 0..240 {
                    world.advance(1.0 / 60.0, false, aspect, &audio, &[0.6; 32]);
                    for (i, p) in world.particles.iter().enumerate() {
                        assert!(p.p.x.abs() <= aspect && p.p.y.abs() <= 1.0);
                        if frame > 60 && (p.p.x.abs() > aspect * 0.90 || p.p.y.abs() > 0.90) {
                            edge_frames[i] += 1;
                        }
                    }
                }
                let lingering = edge_frames.iter().copied().max().unwrap();
                assert!(
                    lingering < 30,
                    "birds linger at boundary: aspect={aspect}, frames={lingering}"
                );
            }
        }
    }

    #[test]
    fn particles_and_history_stay_bounded() {
        for fireflies in [false, true] {
            let mut world = World::default();
            let audio = Reactivity {
                bass: 1.0,
                treble: 1.0,
                ..Default::default()
            };
            for _ in 0..1200 {
                world.advance(1.0 / 60.0, fireflies, 1.6, &audio, &[1.0; 32]);
            }
            assert_eq!(
                world.particles.len(),
                if fireflies { FIREFLIES } else { BIRDS }
            );
            for p in &world.particles {
                assert!(p.v.length().is_finite());
                for q in std::iter::once(&p.p).chain(p.trail.iter()) {
                    assert!(q.x.abs() <= 1.6 && q.y.abs() <= 1.0);
                }
            }
        }
    }

    #[test]
    fn attack_changes_motion_then_recovers() {
        for fireflies in [false, true] {
            let mut quiet = World::default();
            let mut loud = World::default();
            let silence = Reactivity::default();
            quiet.advance(1.0 / 60.0, fireflies, 1.6, &silence, &[0.0; 32]);
            loud.advance(1.0 / 60.0, fireflies, 1.6, &silence, &[0.0; 32]);
            let hit = Reactivity {
                transients: [1.0, 0.0, 0.0, 0.0],
                ..silence
            };
            quiet.advance(1.0 / 60.0, fireflies, 1.6, &silence, &[0.0; 32]);
            loud.advance(1.0 / 60.0, fireflies, 1.6, &hit, &[0.0; 32]);
            let change: f32 = quiet
                .particles
                .iter()
                .zip(&loud.particles)
                .map(|(a, b)| a.v.sub(b.v).length())
                .sum();
            assert!(change > 20.0);
            assert!(loud.scatter > 0.9);
            for _ in 0..240 {
                loud.advance(1.0 / 60.0, fireflies, 1.6, &silence, &[0.0; 32]);
            }
            assert!(loud.scatter < 0.01);
        }
    }

    #[test]
    fn styles_have_distinct_dynamics_and_images() {
        let mut bird = Renderer::default();
        let mut fly = Renderer::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut a = Buffer::empty(area);
        let mut b = Buffer::empty(area);
        for frame in 0..90 {
            bird.draw(
                &mut a,
                area,
                frame as f32 / 60.0,
                0,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            fly.draw(
                &mut b,
                area,
                frame as f32 / 60.0,
                1,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
        }
        assert_ne!(a, b);
        let speed = |w: &World| {
            w.particles.iter().map(|p| p.v.length()).sum::<f32>() / w.particles.len() as f32
        };
        assert!((speed(&bird.worlds[0]) - speed(&fly.worlds[1])).abs() > 0.01);
        assert!(a.content.iter().filter(|c| c.symbol() == " ").count() > 6500);
    }

    #[test]
    fn clips_tiny_offset_and_disjoint_areas() {
        let bounds = Rect::new(5, 7, 12, 8);
        let mut buf = Buffer::empty(bounds);
        let mut renderer = Renderer::default();
        for style in 0..2 {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(6, 8, 1, 1),
                Rect::new(0, 0, 10, 10),
                Rect::new(100, 100, 2, 2),
            ] {
                buf.reset();
                renderer.draw(
                    &mut buf,
                    area,
                    0.0,
                    style,
                    2,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        if !area.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), " ");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stalled_clock_does_not_teleport_or_grow_history() {
        let mut renderer = Renderer::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut buf = Buffer::empty(area);
        for time in [0.0, 1000000.0, f32::NAN, -100.0] {
            renderer.draw(
                &mut buf,
                area,
                time,
                0,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
        }
        assert!(renderer.worlds[0].clock < 0.1);
        assert_eq!(renderer.worlds[0].particles.len(), BIRDS);
    }

    #[test]
    #[ignore = "manual release timing at 160x50"]
    fn frame_budget() {
        let mut renderer = Renderer::default();
        let area = Rect::new(0, 0, 160, 50);
        let mut buf = Buffer::empty(area);
        for style in 0..2 {
            let start = std::time::Instant::now();
            for frame in 0..600 {
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.4; 32],
                    &Reactivity {
                        bass: 0.6,
                        treble: 0.5,
                        ..Default::default()
                    },
                    &Theme::default(),
                );
            }
            eprintln!(
                "flocks style {style}: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 1000.0 / 600.0
            );
        }
    }
}
