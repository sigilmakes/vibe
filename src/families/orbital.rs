//! Orbital mechanics scenes. Positions use a unit-height projection with the
//! terminal aspect corrected at rasterization, so circles stay circular on any
//! window shape.
use crate::{
    palette::Theme,
    visuals::{BeatClock, Reactivity},
};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::TAU;

pub const NAMES: &[&str] = &[
    "Kepler Drift",
    "Ring Shepherd",
    "Lagrange Bloom",
    "Total Eclipse",
    "Accretion Disc",
    "Comet Tail",
    "Gravity Assist",
    "Tidal Lock",
    "Resonance Chain",
    "Roche Limit",
];

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    n ^= n >> 13;
    n = n.wrapping_mul(0xC2B2_AE3D);
    n ^= n >> 16;
    (n & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    clock: f32,
    /// Bass-driven orbital precession shared by the comet and diagram scenes.
    precession: f32,
    /// Decaying attack envelope from the transient channels.
    burst: f32,
    /// Fraction of the encounter path the ship has flown.
    ship: f32,
    /// Continuous beat position, so beat-synced paths glide instead of stepping
    /// once per beat.
    beat: BeatClock,
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let now = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (now - last).clamp(0.0, 0.05));
        self.last_time = Some(now);
        let bass = unit(audio.bass);
        // Audio only scales the clock; every scene keeps its own fixed tempo.
        self.clock += dt * (0.6 + unit(audio.energy) * 0.9 + bass * 0.2);
        // Bass advances the orbit frame, so radius and precession answer the
        // low end instead of leaving the diagrams fixed.
        self.precession = (self.precession + dt * (0.02 + bass * 0.55)).rem_euclid(TAU);
        let attack = unit(audio.transients[0])
            .max(unit(audio.transients[1]))
            .max(unit(audio.onset));
        self.burst = (self.burst * (-dt * 2.6).exp()).max(attack);
        self.ship = (self.ship + dt * (0.09 + unit(audio.energy) * 0.25)).fract();
        self.beat.advance(dt, audio);
    }

    fn project(&mut self, area: Rect, visible: Rect) {
        let height = area.height.max(1) as f32;
        let scale = 2.0 / height;
        let cx = area.x as f32 + area.width as f32 * 0.5;
        let cy = area.y as f32 + area.height as f32 * 0.5;
        self.xs.clear();
        self.ys.clear();
        self.xs
            .extend((visible.x..visible.right()).map(|x| (x as f32 + 0.5 - cx) * scale));
        self.ys
            .extend((visible.y..visible.bottom()).map(|y| (y as f32 + 0.5 - cy) * scale));
    }
}

/// A gap carved by a shepherd moon. `period_ratio` is the ring-particle
/// period divided by the moon's period: 1 is the moon's own orbit, 1/2 the
/// inner 2:1 Lindblad resonance, and 2 the outer 2:1 resonance.
#[derive(Clone, Copy)]
struct RingGap {
    center: f32,
    half_width: f32,
    depth: f32,
}

fn shepherd_gap(
    moon_orbit: f32,
    moon_radius: f32,
    planet_radius: f32,
    period_ratio: f32,
    depth: f32,
) -> RingGap {
    let hill = moon_orbit * (moon_radius / planet_radius) * 0.693;
    RingGap {
        center: moon_orbit * period_ratio.powf(2.0 / 3.0),
        half_width: hill * if period_ratio == 1.0 { 1.8 } else { 1.1 },
        depth,
    }
}

fn ring_gaps(moons: &[(f32, f32)], planet_radius: f32) -> Vec<RingGap> {
    let mut gaps = Vec::with_capacity(moons.len() * 3);
    for &(orbit, radius) in moons {
        gaps.push(shepherd_gap(orbit, radius, planet_radius, 1.0, 0.94));
        gaps.push(shepherd_gap(orbit, radius, planet_radius, 0.5, 0.60));
        gaps.push(shepherd_gap(orbit, radius, planet_radius, 2.0, 0.38));
    }
    gaps
}

#[allow(clippy::too_many_arguments)]
fn paint(
    buf: &mut Buffer,
    col: u16,
    row: u16,
    mark: char,
    phase: f32,
    light: f32,
    bg_phase: f32,
    bg_light: f32,
    palette: usize,
    theme: &Theme,
) {
    let cell = &mut buf[(col, row)];
    cell.reset();
    cell.set_char(mark);
    cell.set_fg(theme.sample(palette, phase, light));
    cell.set_bg(theme.sample(palette, bg_phase, bg_light));
}

fn wash(buf: &mut Buffer, visible: Rect, palette: usize, theme: &Theme, phase: f32, light: f32) {
    for y in visible.y..visible.bottom() {
        for x in visible.x..visible.right() {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_char(' ');
            cell.set_bg(theme.sample(palette, phase, light));
        }
    }
}

fn sprinkle_stars(
    buf: &mut Buffer,
    visible: Rect,
    palette: usize,
    theme: &Theme,
    t: f32,
    gain: f32,
) {
    for y in visible.y..visible.bottom() {
        for x in visible.x..visible.right() {
            let h = hash(x as i32 * 3 + 11, y as i32 * 5 + 7);
            if h > 0.982 {
                let twinkle = 0.5 + 0.5 * (t * 1.9 + h * 47.0).sin();
                let light = (0.16 + 0.55 * twinkle) * gain;
                let cell = &mut buf[(x, y)];
                cell.set_char(if h > 0.997 { '✦' } else { '·' });
                cell.set_fg(theme.sample(palette, 0.55 + h * 0.35, light));
            }
        }
    }
}

fn body_light(dx: f32, dy: f32, z: f32) -> f32 {
    (0.12 + 0.88 * (dx * -0.55 + dy * -0.45 + z * 0.70).max(0.0)).clamp(0.0, 1.0)
}

/// Solves Kepler's equation `M = E - e sin E` by Newton iteration.
fn eccentric_anomaly(mean: f32, e: f32) -> f32 {
    let e = e.clamp(0.0, 0.95);
    let mut ecc = mean;
    for _ in 0..6 {
        let step = (ecc - e * ecc.sin() - mean) / (1.0 - e * ecc.cos());
        ecc -= step;
        if step.abs() < 1.0e-6 {
            break;
        }
    }
    ecc
}

fn true_anomaly(ecc: f32, e: f32) -> f32 {
    ((1.0 - e * e).sqrt() * ecc.sin()).atan2(ecc.cos() - e)
}

fn orbital_period(semi_major: f32, mu: f32) -> f32 {
    TAU * (semi_major.powi(3) / mu).sqrt()
}

/// Small-velocity Doppler boost for a Keplerian disc: the approaching limb is
/// brighter by `1 + 3 beta`, with beta the line-of-sight speed over c.
fn doppler_boost(radius: f32, phi: f32, inc_cos: f32, gain: f32) -> f32 {
    let speed = (1.0 / radius.max(0.02)).sqrt();
    let sin_i = (1.0 - inc_cos * inc_cos).max(0.0).sqrt();
    (1.0 + 3.0 * gain * speed * phi.cos() * sin_i).clamp(0.25, 2.4)
}

fn angular_velocity(radius: f32, mu: f32) -> f32 {
    (mu / radius.powi(3)).max(0.0).sqrt()
}

fn angular_radius(body_radius: f32, distance: f32) -> f32 {
    body_radius / distance.max(1.0e-3)
}

/// Adjacent period ratios of the resonance chain, as (p, q) for T_out / T_in.
const RESONANCE_STEPS: [(f32, f32); 5] =
    [(2.0, 1.0), (2.0, 1.0), (5.0, 3.0), (3.0, 2.0), (8.0, 5.0)];

fn resonance_radii(base: f32) -> [f32; 6] {
    let mut radii = [base; 6];
    for (k, (p, q)) in RESONANCE_STEPS.iter().enumerate() {
        radii[k + 1] = radii[k] * (*p / *q).powf(2.0 / 3.0);
    }
    radii
}

fn orbit_point(center: [f32; 2], radius: f32, angle: f32, flatten: f32) -> [f32; 2] {
    [
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin() * flatten,
    ]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Projected Keplerian ellipse. The focus (the star) is at the origin; `inc`
/// tilts the orbital plane away from the screen plane.
#[derive(Clone, Copy)]
struct KeplerOrbit {
    a: f32,
    e: f32,
    inc: f32,
    arg: f32,
}

impl KeplerOrbit {
    fn semi_minor(&self) -> f32 {
        self.a * (1.0 - self.e * self.e).sqrt() * self.inc.cos()
    }

    fn period(&self, mu: f32) -> f32 {
        orbital_period(self.a, mu)
    }

    fn point(&self, mean: f32) -> [f32; 2] {
        self.point_from_eccentric(eccentric_anomaly(mean, self.e))
    }

    fn point_from_eccentric(&self, ecc: f32) -> [f32; 2] {
        let x = self.a * (ecc.cos() - self.e);
        let y = self.semi_minor() * ecc.sin();
        self.orient([x, y])
    }

    fn point_from_true(&self, nu: f32) -> [f32; 2] {
        let r = self.a * (1.0 - self.e * self.e) / (1.0 + self.e * nu.cos());
        let x = r * nu.cos();
        let y = r * nu.sin() * self.inc.cos();
        self.orient([x, y])
    }

    fn orient(&self, p: [f32; 2]) -> [f32; 2] {
        let (c, s) = (self.arg.cos(), self.arg.sin());
        [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
    }

    fn radius(&self, mean: f32) -> f32 {
        self.a * (1.0 - self.e * eccentric_anomaly(mean, self.e).cos())
    }
}

/// Fluid Roche limit for a moon of the given mean density relative to its planet.
fn roche_limit(planet_radius: f32, planet_density: f32, moon_density: f32) -> f32 {
    2.44 * planet_radius * (planet_density / moon_density).cbrt()
}

/// Tidal-force ratio across a moon: 1 at the Roche limit, above 1 inside it.
fn tidal_stretch(distance: f32, roche: f32) -> f32 {
    (roche / distance.max(1.0e-3)).powi(3)
}

/// Prolate tidal axes for a moon of the given radius. The volume is conserved:
/// major * minor^2 equals the unperturbed radius cubed.
fn tidal_axes(moon_radius: f32, stretch: f32) -> (f32, f32) {
    let along = (1.0 + 0.9 * (stretch - 1.0).max(0.0)).min(3.0);
    (moon_radius * along, moon_radius / along.sqrt())
}

fn disrupted_fraction(distance: f32, roche: f32, impact: f32) -> f32 {
    ((tidal_stretch(distance, roche) - 1.0) * 2.0 * (0.65 + 0.35 * unit(impact))).clamp(0.0, 1.0)
}

/// Fraction of the sun's disc hidden by the moon for two circles.
fn occulted_fraction(sun_radius: f32, moon_radius: f32, separation: f32) -> f32 {
    let r1 = sun_radius.max(1.0e-4);
    let r2 = moon_radius.max(1.0e-4);
    let d = separation.max(0.0);
    if d >= r1 + r2 {
        return 0.0;
    }
    if d <= (r1 - r2).abs() {
        return if r2 >= r1 { 1.0 } else { (r2 / r1).powi(2) };
    }
    let a1 = ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1))
        .clamp(-1.0, 1.0)
        .acos();
    let a2 = ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2))
        .clamp(-1.0, 1.0)
        .acos();
    let area = r1 * r1 * (a1 - (2.0 * a1).sin() * 0.5) + r2 * r2 * (a2 - (2.0 * a2).sin() * 0.5);
    (area / (std::f32::consts::PI * r1 * r1)).clamp(0.0, 1.0)
}

fn bisect_root(f: impl Fn(f32) -> f32, mut lo: f32, mut hi: f32) -> f32 {
    let mut flo = f(lo);
    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        let fm = f(mid);
        if (fm > 0.0) == (flo > 0.0) {
            lo = mid;
            flo = fm;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Collinear libration points L1, L2, L3 for a unit separation and mass ratio
/// `m2 / m1` (with the primary at `-m2/(1+m2)` and the secondary at `1/(1+m2)`).
fn lagrange_collinear(m2: f32) -> [f32; 3] {
    let m2 = m2.clamp(1.0e-4, 10.0);
    let x1 = -m2 / (1.0 + m2);
    let x2 = 1.0 / (1.0 + m2);
    let omega2 = 1.0 + m2;
    let l1 = bisect_root(
        |x| 1.0 / (x - x1).powi(2) - m2 / (x2 - x).powi(2) - omega2 * x,
        x1 + 1.0e-5,
        x2 - 1.0e-5,
    );
    let l2 = bisect_root(
        |x| 1.0 / (x - x1).powi(2) + m2 / (x - x2).powi(2) - omega2 * x,
        x2 + 1.0e-5,
        x2 + 2.0,
    );
    let l3 = bisect_root(
        |x| -1.0 / (x1 - x).powi(2) - m2 / (x2 - x).powi(2) - omega2 * x,
        x1 - 2.0,
        x1 - 1.0e-5,
    );
    [l1, l2, l3]
}

/// Effective potential in the rotating frame, in units of the separation.
fn restricted_potential(p: [f32; 2], m2: f32) -> f32 {
    let x1 = -m2 / (1.0 + m2);
    let x2 = 1.0 / (1.0 + m2);
    let r1 = ((p[0] - x1).powi(2) + p[1] * p[1]).sqrt().max(1.0e-3);
    let r2 = ((p[0] - x2).powi(2) + p[1] * p[1]).sqrt().max(1.0e-3);
    -1.0 / r1 - m2 / r2 - 0.5 * (1.0 + m2) * (p[0] * p[0] + p[1] * p[1])
}

/// Critical potential levels: the L4 island edge, the L3 and L2 saddles, and
/// the L1 gateway, with interpolated contours between them.
fn lagrange_levels(m2: f32) -> [f32; 9] {
    let collinear = lagrange_collinear(m2);
    let x1 = -m2 / (1.0 + m2);
    let x2 = 1.0 / (1.0 + m2);
    let u = |x: f32, y: f32| restricted_potential([x, y], m2);
    let u1 = u(collinear[0], 0.0);
    let u2 = u(collinear[1], 0.0);
    let u3 = u(collinear[2], 0.0);
    let u4 = u((x1 + x2) * 0.5, 0.5 * 3.0_f32.sqrt());
    [
        u4 + 0.22,
        u4 + 0.08,
        u4,
        (u4 + u3) * 0.5,
        u3,
        (u3 + u2) * 0.5,
        u2,
        (u2 + u1) * 0.5,
        u1,
    ]
}

/// Heliocentric Keplerian ellipse for the comet. `a` grows with bass, and the
/// orbital plane is inclined so the anti-solar tail runs across the canvas.
struct CometOrbit {
    star: [f32; 2],
    a: f32,
    e: f32,
    inc: f32,
    rot: f32,
}

impl CometOrbit {
    fn new(bass: f32, precession: f32) -> Self {
        Self {
            star: [0.60, 0.06],
            a: 0.78 + unit(bass) * 0.16,
            e: 0.55,
            inc: 1.25,
            rot: 0.10 + precession,
        }
    }

    fn mean_anomaly(clock: f32) -> f32 {
        clock * 0.5 + 1.1
    }

    fn orbit(&self) -> KeplerOrbit {
        KeplerOrbit {
            a: self.a,
            e: self.e,
            inc: self.inc,
            arg: self.rot,
        }
    }

    fn focus(&self, mean: f32) -> [f32; 2] {
        let p = self.orbit().point(mean);
        [self.star[0] + p[0], self.star[1] + p[1]]
    }

    fn radius(&self, mean: f32) -> f32 {
        self.orbit().radius(mean)
    }
}

/// Hyperbolic flyby around the assist planet. A stronger attack lowers the
/// periapsis and slows the approach, so the same ship visibly bends more
/// around the planet instead of riding a fixed curve.
struct AssistPath {
    center: [f32; 2],
    e: f32,
    rp: f32,
    p: f32,
    theta: f32,
    v_inf: f32,
    m_max: f32,
}

impl AssistPath {
    fn new(burst: f32) -> Self {
        let burst = unit(burst);
        let mu = 1.0;
        let rp = 0.54 - burst * 0.18;
        let v_inf = 1.15 * (1.0 - burst * 0.10);
        let e = 1.0 + rp * v_inf * v_inf / mu;
        let p = rp * (1.0 + e);
        let span = (-1.0 / e).acos() * 0.80;
        let h_max = 2.0 * (((e - 1.0) / (e + 1.0)).sqrt() * (0.5 * span).tan()).atanh();
        Self {
            center: [0.30, 0.05],
            e,
            rp,
            p,
            theta: -std::f32::consts::FRAC_PI_2,
            v_inf,
            m_max: e * h_max.sinh() - h_max,
        }
    }

    fn hyperbolic_anomaly(mean: f32, e: f32) -> f32 {
        let mut h = mean / e.max(1.01);
        for _ in 0..10 {
            let f = e * h.sinh() - h - mean;
            let df = e * h.cosh() - 1.0;
            h -= f / df.max(1.0e-4);
        }
        h
    }

    /// Points along the flown path, `u` in [0, 1] from entry to exit. The
    /// ship advances in hyperbolic mean anomaly, so it is fastest at
    /// periapsis.
    fn point(&self, u: f32) -> [f32; 2] {
        let mean = (u.clamp(0.0, 1.0) * 2.0 - 1.0) * self.m_max;
        let h = Self::hyperbolic_anomaly(mean, self.e);
        let nu = 2.0 * (((self.e + 1.0) / (self.e - 1.0)).sqrt() * (0.5 * h).tanh()).atan();
        let r = self.p / (1.0 + self.e * nu.cos()).max(0.02);
        let a = self.theta + nu;
        [self.center[0] + r * a.cos(), self.center[1] + r * a.sin()]
    }

    fn periapsis(&self) -> [f32; 2] {
        [
            self.center[0] + self.rp * self.theta.cos(),
            self.center[1] + self.rp * self.theta.sin(),
        ]
    }

    /// Total turn between the incoming and outgoing asymptotes.
    fn bend(&self) -> f32 {
        2.0 * (1.0 / self.e).asin()
    }

    fn energy(&self) -> f32 {
        0.5 * self.v_inf * self.v_inf
    }
}

/// Two-body frame and five Lagrange points in world space. Bass sets the mass
/// ratio and separation; precession rotates the whole diagram. The five points
/// are exact for any mass ratio; the comparable-mass pair only keeps them
/// resolvable at terminal size. L4/L5 linear stability (m2/m1 < 0.04) is not
/// claimed by this diagram.
struct Lagrange {
    m2: f32,
    sep: f32,
    rot: f32,
    x1: f32,
    x2: f32,
    l: [[f32; 2]; 5],
}

impl Lagrange {
    fn new(bass: f32, precession: f32) -> Self {
        let m2 = 0.16 + unit(bass) * 0.18;
        let sep = 0.80 + unit(bass) * 0.18;
        let x1 = -sep * m2 / (1.0 + m2);
        let x2 = sep / (1.0 + m2);
        let collinear = lagrange_collinear(m2);
        let mid = (x1 + x2) * 0.5;
        Self {
            m2,
            sep,
            rot: precession,
            x1,
            x2,
            l: [
                [collinear[0] * sep, 0.0],
                [collinear[1] * sep, 0.0],
                [collinear[2] * sep, 0.0],
                [mid, sep * 0.5 * 3.0_f32.sqrt()],
                [mid, -sep * 0.5 * 3.0_f32.sqrt()],
            ],
        }
    }

    fn potential(&self, p: [f32; 2]) -> f32 {
        restricted_potential([p[0] / self.sep, p[1] / self.sep], self.m2)
    }

    fn to_body(&self, p: [f32; 2]) -> [f32; 2] {
        let (cr, sr) = (self.rot.cos(), self.rot.sin());
        [p[0] * cr + p[1] * sr, -p[0] * sr + p[1] * cr]
    }

    fn to_world(&self, p: [f32; 2]) -> [f32; 2] {
        let (cr, sr) = (self.rot.cos(), self.rot.sin());
        [p[0] * cr - p[1] * sr, p[0] * sr + p[1] * cr]
    }

    fn points_world(&self) -> [[f32; 2]; 5] {
        self.l.map(|p| self.to_world(p))
    }
}

/// Surface endpoints of the tidal bridge, from the moon's near face to the
/// planet's near face.
fn tidal_bridge(planet: [f32; 2], pr: f32, moon: [f32; 2], mr: f32) -> ([f32; 2], [f32; 2]) {
    let mut dir = [planet[0] - moon[0], planet[1] - moon[1]];
    let d = dir[0].hypot(dir[1]).max(0.001);
    dir = [dir[0] / d, dir[1] / d];
    (
        [moon[0] + dir[0] * mr, moon[1] + dir[1] * mr],
        [planet[0] - dir[0] * pr, planet[1] - dir[1] * pr],
    )
}

/// Equal-density masses from the body radii; used for the barycentric split.
fn tidal_masses(planet_radius: f32, moon_radius: f32) -> (f32, f32) {
    (planet_radius.powi(3), moon_radius.powi(3))
}

/// The locked pair. Bass opens the moon's orbit, and the bodies share a
/// flattened barycentric orbit so the diagram is not frozen in place.
fn tidal_bodies(bass: f32, precession: f32) -> ([f32; 2], f32, [f32; 2], f32) {
    let bass = unit(bass);
    let pr = 1.28;
    let mr = 0.62;
    let gap = 0.28 + bass * 0.18;
    let sep = pr + mr + gap;
    let bary = [-0.11, 0.02];
    let (mp, mm) = tidal_masses(pr, mr);
    let planet_share = mm / (mp + mm);
    let moon_share = mp / (mp + mm);
    let (cr, sr) = (precession.cos(), precession.sin());
    let p = [
        bary[0] - cr * sep * planet_share,
        bary[1] - sr * sep * planet_share * 0.30,
    ];
    let m = [
        bary[0] + cr * sep * moon_share,
        bary[1] + sr * sep * moon_share * 0.30,
    ];
    (p, pr, m, mr)
}

struct Canvas<'a> {
    buf: &'a mut Buffer,
    clip: Rect,
    palette: usize,
    theme: &'a Theme,
    scale: f32,
    cx: f32,
    cy: f32,
}

impl<'a> Canvas<'a> {
    fn new(buf: &'a mut Buffer, area: Rect, clip: Rect, palette: usize, theme: &'a Theme) -> Self {
        let height = area.height.max(1) as f32;
        Self {
            buf,
            clip,
            palette,
            theme,
            scale: 2.0 / height,
            cx: area.x as f32 + area.width as f32 * 0.5,
            cy: area.y as f32 + area.height as f32 * 0.5,
        }
    }

    fn plot(&mut self, p: [f32; 2], mark: char, phase: f32, light: f32) {
        if !p[0].is_finite() || !p[1].is_finite() || light <= 0.0 {
            return;
        }
        let col = (self.cx + p[0] / self.scale - 0.5).round() as i32;
        let row = (self.cy + p[1] / self.scale - 0.5).round() as i32;
        if col >= self.clip.x as i32
            && col < self.clip.right() as i32
            && row >= self.clip.y as i32
            && row < self.clip.bottom() as i32
        {
            let cell = &mut self.buf[(col as u16, row as u16)];
            cell.set_char(mark);
            cell.set_fg(self.theme.sample(self.palette, phase, light));
        }
    }

    fn line(&mut self, a: [f32; 2], b: [f32; 2], mark: char, phase: f32, light: f32) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let steps = (dx.abs().max(dy.abs()) / self.scale).ceil() as usize;
        let steps = steps.clamp(1, 512);
        for i in 0..=steps {
            let s = i as f32 / steps as f32;
            self.plot([a[0] + dx * s, a[1] + dy * s], mark, phase, light);
        }
    }

    fn circle(&mut self, c: [f32; 2], r: f32, mark: char, phase: f32, light: f32) {
        let count = ((r * TAU / self.scale).ceil() as usize).clamp(8, 96);
        for i in 0..count {
            let a = i as f32 / count as f32 * TAU;
            self.plot([c[0] + a.cos() * r, c[1] + a.sin() * r], mark, phase, light);
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
        let style = style % NAMES.len();
        self.advance(time, audio);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        self.project(area, visible);
        let t = self.clock;
        match style {
            0 => self.kepler(buf, visible, audio, theme, palette, t),
            1 => self.rings(buf, visible, audio, bands, theme, palette, t),
            2 => self.lagrange(buf, visible, audio, theme, palette, t),
            3 => self.eclipse(buf, visible, audio, theme, palette, t),
            4 => self.accretion(buf, visible, audio, theme, palette, t),
            5 => self.comet(buf, area, visible, audio, theme, palette, t),
            6 => self.assist(buf, area, visible, audio, theme, palette, t),
            7 => self.tidal(buf, area, visible, audio, theme, palette, t),
            8 => self.resonance(buf, area, visible, audio, theme, palette, t),
            _ => self.roche(buf, area, visible, audio, theme, palette, t),
        }
    }

    fn kepler(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let orbits = [
            KeplerOrbit {
                a: 0.62,
                e: 0.16,
                inc: 0.0,
                arg: 0.35,
            },
            KeplerOrbit {
                a: 1.04,
                e: 0.26,
                inc: 0.0,
                arg: 2.15,
            },
            KeplerOrbit {
                a: 1.58,
                e: 0.33,
                inc: 0.0,
                arg: 4.30,
            },
        ];
        let mu = 1.0;
        let mut cs = [0.0_f32; 3];
        let mut sn = [0.0_f32; 3];
        let mut bodies = [[0.0_f32; 2]; 3];
        let mut mean = [0.0_f32; 3];
        let mut semi = [0.0_f32; 3];
        for k in 0..3 {
            mean[k] = t * TAU / orbits[k].period(mu) * 0.6 + k as f32 * 2.1;
            bodies[k] = orbits[k].point(mean[k]);
            cs[k] = orbits[k].arg.cos();
            sn[k] = orbits[k].arg.sin();
            semi[k] = orbits[k].semi_minor();
        }
        let bass = unit(audio.bass);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r2 = x * x + y * y;
                let core = (-r2 * 240.0).exp();
                let halo = 0.30 / (1.0 + r2 * 70.0) * (0.5 + bass * 1.2);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                for k in 0..3 {
                    let xr = x * cs[k] + y * sn[k];
                    let yr = -x * sn[k] + y * cs[k];
                    let d = (xr / orbits[k].a).hypot(yr / semi[k]);
                    let ring = (-((d - 1.0) * 22.0).powi(2)).exp();
                    // The trail spans a fixed interval of mean anomaly, so it
                    // stretches at apoapsis and shortens around periastron.
                    let point_ecc = (yr / semi[k]).atan2(xr / orbits[k].a + orbits[k].e);
                    let point_mean = point_ecc - orbits[k].e * point_ecc.sin();
                    let behind = (mean[k] - point_mean).rem_euclid(TAU);
                    let trail = if behind < 2.0 {
                        (-behind * 1.35).exp()
                    } else {
                        0.0
                    };
                    let line = ring * (0.20 + 0.85 * trail);
                    if line > light && line > 0.05 {
                        mark = '·';
                        light = line * 0.82;
                        phase = 0.50 + k as f32 * 0.09;
                    }
                    let pd2 = (x - bodies[k][0]).powi(2) + (y - bodies[k][1]).powi(2);
                    let head = (-pd2 * 620.0).exp();
                    if head > light && head > 0.05 {
                        mark = '●';
                        light = head;
                        phase = 0.62 + k as f32 * 0.08;
                    }
                }
                if core > light && core > 0.04 {
                    mark = '✦';
                    light = core;
                    phase = 0.10;
                }
                let h = hash(i as i32 * 17 + 5, j as i32 * 23 + 9);
                if h > 0.985 {
                    let twinkle = 0.25 + 0.35 * (t * 1.7 + h * 31.0).sin().abs();
                    if twinkle > light {
                        mark = '·';
                        light = twinkle;
                        phase = 0.55 + h * 0.3;
                    }
                }
                let bg_light = 0.016 + halo * 0.35 + (light * 0.10).min(0.07);
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.56 + y * 0.04,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rings(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        bands: &[f32; 32],
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let treble = unit(audio.treble);
        let flatten = 0.30 + bass * 0.03;
        let tilt = 0.18 + 0.05 * (t * 0.07).sin();
        let ct = tilt.cos();
        let st = tilt.sin();
        let planet = 0.30 + bass * 0.06;
        let moons = [(0.86_f32, 0.024_f32), (1.72, 0.030)];
        let gaps = ring_gaps(&moons, planet);
        let mu = 1.0;
        let mut moon_pos = [[0.0_f32; 2]; 2];
        for (k, &(q, _)) in moons.iter().enumerate() {
            let phase = t * angular_velocity(q, mu) + k as f32 * 2.1;
            let p = orbit_point([0.0, 0.0], q, phase, flatten);
            moon_pos[k] = [p[0] * ct - p[1] * st, p[0] * st + p[1] * ct];
        }
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let xr = x * ct + y * st;
                let yr = -x * st + y * ct;
                let q = (xr * xr + (yr / flatten).powi(2)).sqrt();
                let phi = (yr / flatten).atan2(xr);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                if q < planet {
                    let z = (1.0 - (q / planet).powi(2)).max(0.0).sqrt();
                    let lat = (yr / flatten) / planet;
                    let band = 0.5 + 0.5 * (lat * 16.0 + z * 5.0 + t * 0.12).sin();
                    light = (0.38 + 0.30 * band + 0.18 * z).clamp(0.0, 1.0);
                    mark = if light > 0.62 { '█' } else { '▓' };
                    phase = 0.18 + band * 0.10;
                } else {
                    let win =
                        ((q - 0.40) / 0.06).clamp(0.0, 1.0) * ((2.45 - q) / 0.18).clamp(0.0, 1.0);
                    let band_wave = unit(bands[(phi.rem_euclid(TAU) / TAU * 31.0) as usize]);
                    // Each radius keeps its own Keplerian rate, so azimuthal
                    // texture shears instead of rotating as one rigid disc.
                    let omega = angular_velocity(q.max(0.2), mu);
                    let co = (phi - omega * t).rem_euclid(TAU);
                    let ringlets =
                        0.6 + 0.4 * (q * 34.0 + (q * 8.0).sin() * 2.6 + band_wave * 1.5).sin();
                    let grain = (0.55
                        + 0.25 * (q * 61.0 + co * 3.0).sin()
                        + 0.20 * (q * 19.0 - co * 2.0).sin())
                    .clamp(0.0, 1.0);
                    let mut gap = 1.0;
                    for g in &gaps {
                        let notch = (-((q - g.center) / g.half_width).powi(2)).exp();
                        gap *= 1.0 - g.depth * notch * (0.75 + mid * 0.25);
                    }
                    let dens =
                        (win * ringlets * grain * gap).clamp(0.0, 1.0) * (0.60 + 0.40 * treble);
                    if dens > 0.06 {
                        mark = if dens > 0.55 {
                            '▒'
                        } else if dens > 0.22 {
                            '░'
                        } else {
                            '·'
                        };
                        light = 0.22 + dens * 0.72;
                        phase = 0.42 + dens * 0.16 + q * 0.03;
                    }
                    for (k, m) in moon_pos.iter().enumerate() {
                        let d = (x - m[0]).hypot(y - m[1]);
                        if d < 0.055 {
                            mark = '●';
                            light = 0.90;
                            phase = 0.62 + k as f32 * 0.12;
                        } else if d < 0.10 && light < 0.24 {
                            mark = '·';
                            light = 0.24;
                            phase = 0.60 + k as f32 * 0.12;
                        }
                    }
                }
                let glow = 0.22 / (1.0 + q * 6.0);
                let bg_light = 0.014 + light * 0.12 + glow * 0.10;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.12 + q * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn lagrange(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let lg = Lagrange::new(unit(audio.bass), self.precession);
        let levels = lagrange_levels(lg.m2);
        let trojan = t * 0.65 + self.beat.position() * TAU;
        let trojan_r = 0.12 + self.burst * 0.14;
        let trojan_count = 4 + (self.burst * 8.0) as usize;
        let points = lg.points_world();
        let masses = [lg.to_world([lg.x1, 0.0]), lg.to_world([lg.x2, 0.0])];
        let x1 = -lg.m2 / (1.0 + lg.m2);
        let x2 = 1.0 / (1.0 + lg.m2);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let b = lg.to_body([x, y]);
                let pb = [b[0] / lg.sep, b[1] / lg.sep];
                let r1 = ((pb[0] - x1).powi(2) + pb[1] * pb[1] + 0.001).sqrt();
                let r2 = ((pb[0] - x2).powi(2) + pb[1] * pb[1] + 0.001).sqrt();
                let r = (pb[0] * pb[0] + pb[1] * pb[1]).sqrt();
                let u = lg.potential([x, y]);
                let mut near = f32::MAX;
                for level in levels {
                    near = near.min((u - level).abs());
                }
                let contour = (-(near * 56.0).powi(2)).exp();
                let island = (-(((u - levels[2]) * 50.0).powi(2))).exp();
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                if contour > 0.08 {
                    let h = 1.0e-3;
                    let gx = restricted_potential([pb[0] + h, pb[1]], lg.m2)
                        - restricted_potential([pb[0] - h, pb[1]], lg.m2);
                    let gy = restricted_potential([pb[0], pb[1] + h], lg.m2)
                        - restricted_potential([pb[0], pb[1] - h], lg.m2);
                    mark = if gx.abs() > gy.abs() * 1.5 {
                        '─'
                    } else if gy.abs() > gx.abs() * 1.5 {
                        '│'
                    } else {
                        '·'
                    };
                    light = 0.18 + contour * 0.58;
                    phase = 0.44 + (u * 0.05).rem_euclid(1.0) * 0.20;
                }
                if island > 0.22 {
                    mark = '∘';
                    light = 0.45 + island * 0.35;
                    phase = 0.72;
                }
                for (k, p) in masses.iter().enumerate() {
                    let d = (x - p[0]).hypot(y - p[1]);
                    if d < 0.06 {
                        mark = if k == 0 { '◉' } else { '◍' };
                        light = 0.94;
                        phase = 0.08 + k as f32 * 0.10;
                    }
                }
                for (k, p) in points.iter().enumerate() {
                    let d = (x - p[0]).hypot(y - p[1]);
                    if d < 0.055 {
                        mark = '◇';
                        light = 0.92;
                        phase = 0.68 + k as f32 * 0.03;
                    }
                    if k >= 3 {
                        let ring = (-((d - trojan_r) / 0.022).powi(2)).exp();
                        if ring > light * 0.9 && ring > 0.05 {
                            mark = '·';
                            light = ring * (0.35 + self.burst * 0.55);
                            phase = 0.60;
                        }
                        for n in 0..trojan_count {
                            let a = trojan + n as f32 * TAU / trojan_count as f32 + k as f32;
                            let q = [p[0] + a.cos() * trojan_r, p[1] + a.sin() * trojan_r];
                            if (x - q[0]).hypot(y - q[1]) < 0.045 {
                                mark = '•';
                                light = 0.70 + self.burst * 0.25;
                                phase = 0.64;
                            }
                        }
                    }
                }
                let glow = 0.5 / (1.0 + r1 * 3.0) + 0.35 / (1.0 + r2 * 3.0) + 0.10 / (1.0 + r);
                let bg_light = 0.012 + glow * 0.06 + contour * 0.10;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.55 + u * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn eclipse(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let energy = unit(audio.energy);
        // Apparent (angular) radii. The sun sits at the reference distance;
        // the moon's distance changes with mid, so its angular size crosses
        // the sun's and the eclipse alternates between total and annular.
        let sun_r = 0.56 + bass * 0.03;
        let moon_distance = 0.74 - mid * 0.10;
        let moon_r = angular_radius(0.42, moon_distance);
        let cycle = (t * 0.072 + self.beat.position() * 0.12).fract();
        let travel = 1.08 * (sun_r + moon_r);
        let mx = (cycle * 2.0 - 1.0) * travel;
        let my = 0.10 * (t * 0.17).sin();
        let coverage = occulted_fraction(sun_r, moon_r, mx.hypot(my));
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r = x.hypot(y);
                let dm = (x - mx).hypot(y - my);
                let theta = y.atan2(x);
                let ray = (0.5 + 0.5 * (theta * 11.0 + (theta * 3.0 + t * 0.15).sin() * 2.0).sin())
                    .powi(3);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.008;
                if dm < moon_r {
                    // The moon's night face is dark in front of both the sky
                    // and the photosphere.
                    mark = if r < sun_r { '█' } else { ' ' };
                    light = if r < sun_r { 0.008 } else { 0.0 };
                    phase = 0.0;
                    bg_light = 0.003;
                } else if r < sun_r {
                    // Limb darkening: the centre of the photosphere is the
                    // brightest part of the disc.
                    let limb = (1.0 - (r / sun_r).powi(2)).max(0.0).sqrt();
                    let grain =
                        0.5 + 0.5 * (x * 34.0 + y * 21.0 + (x * 13.0).sin() * 1.7 + t * 0.2).sin();
                    light = (0.46 + 0.54 * limb) * (0.86 + 0.14 * grain);
                    mark = if light > 0.82 {
                        '█'
                    } else if light > 0.62 {
                        '▓'
                    } else {
                        '▒'
                    };
                    phase = 0.10 + grain * 0.07;
                    bg_light += 0.09;
                } else {
                    // The corona is only visible where the photosphere is
                    // hidden; its brightness follows the occulted fraction.
                    let corona = coverage.powi(2)
                        * (-(r - sun_r) * 2.4).exp()
                        * ray
                        * (0.55 + energy * 0.85);
                    if corona > 0.04 {
                        mark = if corona > 0.35 { '│' } else { '·' };
                        light = corona * 0.85;
                        phase = 0.16 + ray * 0.10;
                    }
                    bg_light += corona * 0.13;
                }
                let contact = (-((dm - moon_r) / 0.025).powi(2)).exp()
                    * (-((r - sun_r) / 0.05).powi(2)).exp()
                    * (0.25 + coverage);
                if contact > light && contact > 0.05 && dm > moon_r {
                    mark = '✦';
                    light = contact;
                    phase = 0.30;
                }
                let track = (-((y - my) / 0.012).powi(2)).exp() * 0.07;
                if track > light && track > 0.03 && dm > moon_r {
                    mark = '·';
                    light = track;
                    phase = 0.55;
                }
                let h = hash(i as i32 * 19 + 1, j as i32 * 29 + 3);
                if h > 0.988 && r > sun_r + 0.05 && dm > moon_r {
                    let twinkle = 0.2 + 0.35 * (t * 1.5 + h * 37.0).sin().abs();
                    if twinkle > light {
                        mark = '·';
                        light = twinkle;
                        phase = 0.6 + h * 0.25;
                    }
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.15 + ray * 0.06,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    fn accretion(
        &mut self,
        buf: &mut Buffer,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        let bass = unit(audio.bass);
        let flatten = 0.30 + bass * 0.04;
        let inner = 0.30 + bass * 0.05;
        let mid = unit(audio.mid);
        let treble = unit(audio.treble);
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let yy = y / flatten;
                let r = (x * x + yy * yy).sqrt();
                let phi = yy.atan2(x);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.006;
                if r > inner * 1.01 {
                    // Keplerian shear: the pattern is advected at Omega(r),
                    // so the inner disc laps the outer disc.
                    let co = (phi - angular_velocity(r, 1.0) * t * 0.55).rem_euclid(TAU);
                    let arm = (0.5 + 0.5 * (3.0 * co + 4.0 * (r + 0.01).ln()).cos()).powf(2.5);
                    let radial = (inner / (r + 0.02)).powf(0.95);
                    let doppler = doppler_boost(r, phi, flatten, 0.35);
                    let grain = 0.55 + 0.45 * hash(i as i32 * 5 + 2, j as i32 * 3 + 7);
                    let fade = (-r * 0.45).exp();
                    let e = 3.0
                        * radial
                        * (0.34 + 0.66 * arm)
                        * doppler
                        * grain
                        * (0.70 + 0.45 * mid)
                        * fade;
                    if e > 0.06 {
                        mark = if e > 0.78 {
                            '█'
                        } else if e > 0.52 {
                            '▓'
                        } else if e > 0.30 {
                            '▒'
                        } else if e > 0.14 {
                            '░'
                        } else {
                            '·'
                        };
                        light = e.min(1.0);
                        phase = 0.30 + r * 0.05 + arm * 0.10;
                        bg_light = 0.01 + e * 0.16;
                    }
                }
                let photon = (-((r - inner * 1.015) / 0.018).powi(2)).exp();
                if photon > light * 0.8 && photon > 0.05 {
                    mark = '█';
                    light = photon * (0.75 + unit(audio.kick) * 0.3);
                    phase = 0.18;
                    bg_light = bg_light.max(0.06);
                }
                let jet = (-x * x / 0.0016).exp()
                    * (-y.abs() * 0.55).exp()
                    * (0.30 + treble * 0.8 + 0.12 * (y.abs() * 9.0 - t * 3.0).sin());
                if jet > light * 0.9 && jet > 0.05 {
                    mark = '│';
                    light = jet.min(1.0);
                    phase = 0.52;
                    bg_light = bg_light.max(jet * 0.10);
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.05 + phi.rem_euclid(TAU) * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn comet(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.55, 0.012);
        sprinkle_stars(buf, visible, palette, theme, t, 0.8);
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let orbit = CometOrbit::new(bass, self.precession);
        let anom = CometOrbit::mean_anomaly(t);
        let head = orbit.focus(anom);
        let mut away = [head[0] - orbit.star[0], head[1] - orbit.star[1]];
        let d = away[0].hypot(away[1]).max(0.001);
        away = [away[0] / d, away[1] / d];
        let perp = [-away[1], away[0]];
        // Treble sets the tail length; attacks add a short-lived extension,
        // and outgassing strengthens near perihelion.
        let activity = (0.70 + 0.60 * orbit.a / orbit.radius(anom).max(0.05)).clamp(0.8, 1.9);
        let len = (0.55 + treble * 0.85 + self.burst * 0.30) * activity;
        let mut c = Canvas::new(buf, area, visible, palette, theme);
        let ellipse = orbit.orbit();
        for k in 0..72 {
            let nu0 = k as f32 / 72.0 * TAU;
            let nu1 = (k + 1) as f32 / 72.0 * TAU;
            let p0 = ellipse.point_from_true(nu0);
            let p1 = ellipse.point_from_true(nu1);
            c.line(
                [orbit.star[0] + p0[0], orbit.star[1] + p0[1]],
                [orbit.star[0] + p1[0], orbit.star[1] + p1[1]],
                '·',
                0.50,
                0.15,
            );
        }
        for k in 1..=24 {
            let past = orbit.focus(anom - k as f32 * 0.045);
            let light = (1.0 - k as f32 / 24.0) * 0.42;
            c.plot(past, '·', 0.34, light);
        }
        // Ion tail: straight strands whose reach follows the high end.
        for s in 0..6 {
            let off = (s as f32 - 2.5) * 0.022;
            let mut prev = head;
            for i in 1..=26 {
                let u = i as f32 / 26.0;
                let wiggle = (u * 13.0 - t * 4.0 + s as f32).sin() * 0.016 * u * (0.4 + treble);
                let p = [
                    head[0] + away[0] * u * len + perp[0] * (off * (0.4 + u) + wiggle),
                    head[1] + away[1] * u * len + perp[1] * (off * (0.4 + u) + wiggle),
                ];
                c.line(
                    prev,
                    p,
                    '·',
                    0.55 + s as f32 * 0.015,
                    (1.0 - u) * 0.58 + 0.07,
                );
                prev = p;
            }
        }
        // Dust tail: shorter, wider, and more curved. Attacks fan it out.
        let bend = 0.32 + unit(audio.mid) * 0.35 + self.burst * 0.50;
        for s in 0..9 {
            let off = (s as f32 - 4.0) * 0.026;
            let mut prev = head;
            for i in 1..=20 {
                let u = i as f32 / 20.0;
                let p = [
                    head[0] + away[0] * u * len * 0.70 + perp[0] * (u * u * bend + off),
                    head[1] + away[1] * u * len * 0.70 + perp[1] * (u * u * bend + off),
                ];
                c.line(
                    prev,
                    p,
                    '·',
                    0.24 + s as f32 * 0.012,
                    (1.0 - u) * 0.34 + 0.05,
                );
                prev = p;
            }
        }
        for k in 0..48 {
            let u = (hash(k, 13) * 0.85).powf(0.8);
            let s = (hash(k, 29) - 0.5) * (0.08 + u * 0.14);
            let p = [
                head[0] + away[0] * u * len + perp[0] * s,
                head[1] + away[1] * u * len + perp[1] * s,
            ];
            c.plot(p, '·', 0.46, (1.0 - u) * 0.22 + 0.04);
        }
        c.circle(
            head,
            0.09 + self.burst * 0.07,
            '·',
            0.34,
            0.30 + self.burst * 0.35,
        );
        c.circle(head, 0.05, '∘', 0.40, 0.60);
        if self.burst > 0.04 {
            c.circle(
                head,
                0.14 + self.burst * 0.30,
                '∘',
                0.32,
                0.18 + self.burst * 0.55,
            );
        }
        c.plot(head, '●', 0.55, 0.97);
        c.plot(orbit.star, '✦', 0.12, 0.95);
        c.circle(orbit.star, 0.045, '·', 0.14, 0.60);
        c.circle(orbit.star, 0.085, '·', 0.14, 0.32);
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU + t * 0.06;
            c.line(
                [
                    orbit.star[0] + a.cos() * 0.04,
                    orbit.star[1] + a.sin() * 0.04,
                ],
                [
                    orbit.star[0] + a.cos() * (0.10 + treble * 0.05),
                    orbit.star[1] + a.sin() * (0.10 + treble * 0.05),
                ],
                '·',
                0.14,
                0.42,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn assist(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let planet = [0.30_f32, 0.05];
        let rp = 0.34_f32;
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            let path = AssistPath::new(self.burst);
            let span = (-1.0 / path.e).acos() * 0.80;
            // A fan of test-particle hyperbolas with the same v_inf and
            // different impact parameters shows how the encounter focuses
            // the incoming trajectories.
            for s in -4..=4 {
                let p = (path.p * (1.0 + s as f32 * 0.18)).max(0.30);
                let mut prev = None;
                for k in 0..=48 {
                    let u = k as f32 / 48.0;
                    let nu = (u * 2.0 - 1.0) * span;
                    let r = p / (1.0 + path.e * nu.cos()).max(0.02);
                    let a = path.theta + nu;
                    let q = [planet[0] + r * a.cos(), planet[1] + r * a.sin()];
                    if let Some(prev) = prev {
                        let light = (0.05 + mid * 0.05) * (1.0 + 0.8 / (1.0 + r * r * 3.0));
                        c.line(prev, q, '·', 0.52, light);
                    }
                    prev = Some(q);
                }
            }
            // Equipotential rings around the planet, brightest near the well.
            for (k, radius) in [0.46_f32, 0.58, 0.74, 0.94, 1.20, 1.52].iter().enumerate() {
                c.circle(
                    planet,
                    *radius,
                    '·',
                    0.50,
                    (0.05 + mid * 0.04) * (1.0 - k as f32 * 0.08),
                );
            }
        }
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let dx = x - planet[0];
                let dy = y - planet[1];
                let d = (dx * dx + dy * dy).sqrt();
                if d < rp {
                    let z = (1.0 - (d / rp).powi(2)).max(0.0).sqrt();
                    let light = body_light(dx / rp, dy / rp, z);
                    let mark = if light > 0.68 {
                        '█'
                    } else if light > 0.45 {
                        '▓'
                    } else if light > 0.22 {
                        '▒'
                    } else {
                        '░'
                    };
                    paint(
                        buf,
                        visible.x + i as u16,
                        visible.y + j as u16,
                        mark,
                        0.22 + light * 0.08,
                        light,
                        0.55,
                        0.03,
                        palette,
                        theme,
                    );
                }
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            let ring = 0.62 + bass * 0.22;
            c.circle(planet, ring, '·', 0.45, 0.12 + mid * 0.05);
            c.circle(planet, ring + 0.08, '·', 0.45, 0.08);
            let moon_a = t * 0.9;
            c.plot(
                [
                    planet[0] + moon_a.cos() * ring,
                    planet[1] + moon_a.sin() * ring,
                ],
                '•',
                0.60,
                0.7,
            );
            // The full flyby is faintly visible from the first frame, so the
            // encounter never starts as an empty field around one blob.
            let path = AssistPath::new(self.burst);
            let kinetic = path.energy().min(1.0);
            for k in 0..=96 {
                let u = k as f32 / 96.0;
                c.plot(
                    path.point(u),
                    '·',
                    0.46,
                    0.22 + self.burst * 0.10 + kinetic * 0.04,
                );
            }
            let flown = (self.ship * 96.0) as usize;
            for k in 0..=flown {
                let u = k as f32 / 96.0;
                c.plot(path.point(u), '·', 0.38, 0.26 + u * 0.40);
            }
            let peri = path.periapsis();
            c.plot(peri, '✦', 0.20, 0.32);
            if self.burst > 0.04 {
                let shock = 0.16 + self.burst * (0.40 + path.bend() * 0.25);
                c.circle(peri, shock, '◦', 0.30, 0.15 + self.burst * 0.65);
                c.circle(peri, shock * 0.55, '·', 0.26, 0.10 + self.burst * 0.35);
                c.plot(peri, '✦', 0.20, 0.30 + self.burst * 0.65);
            }
            let ship = path.point(self.ship);
            c.circle(
                ship,
                0.045 + self.burst * 0.05,
                '·',
                0.15,
                0.35 + self.burst * 0.40,
            );
            c.plot(ship, '◆', 0.12, 0.96);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tidal(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.012);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let bass = unit(audio.bass);
        let treble = unit(audio.treble);
        let (planet, pr, moon, mr) = tidal_bodies(bass, self.precession);
        let mut dir = [planet[0] - moon[0], planet[1] - moon[1]];
        let d = dir[0].hypot(dir[1]).max(0.001);
        dir = [dir[0] / d, dir[1] / d];
        let perp = [-dir[1], dir[0]];
        let lib = (0.07 + bass * 0.09) * (t * 0.52 + self.beat.position() * 0.6).sin();
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                let mut bg_light = 0.010;
                let dx = (x - moon[0]) / mr;
                let dy = (y - moon[1]) / mr;
                let lx = dx * dir[0] + dy * dir[1];
                let ly = -dx * dir[1] + dy * dir[0];
                // The tide makes the moon prolate along the planet-moon
                // axis while conserving its volume.
                let bulge = 0.10 + bass * 0.14;
                let along = 1.0 + bulge;
                let across = 1.0 / along.sqrt();
                let meff = (lx / along).hypot(ly / across);
                if meff < 1.0 {
                    let z = (1.0 - meff * meff).max(0.0).sqrt();
                    let lon = lx.atan2(z.max(0.06)) + lib;
                    let lat = (ly / across).clamp(-1.0, 1.0).asin();
                    let ch = hash((lon * 3.2).floor() as i32, (lat * 5.5).floor() as i32);
                    let mut l = (0.10 + 0.90 * (-dx * 0.82 + z * 0.38).max(0.0) + treble * 0.06)
                        .clamp(0.0, 1.0);
                    if ch > 0.82 {
                        l *= 0.55;
                        mark = if ch > 0.94 { '○' } else { '·' };
                    } else {
                        mark = if l > 0.62 {
                            '█'
                        } else if l > 0.38 {
                            '▓'
                        } else if l > 0.16 {
                            '▒'
                        } else {
                            '░'
                        };
                    }
                    light = l;
                    phase = 0.42 + ch * 0.12;
                    bg_light = 0.012 + l * 0.05;
                } else if (x - planet[0]).hypot(y - planet[1]) < pr {
                    let dx = (x - planet[0]) / pr;
                    let dy = (y - planet[1]) / pr;
                    let z = (1.0 - (dx * dx + dy * dy).clamp(0.0, 1.0)).max(0.0).sqrt();
                    let band = 0.5 + 0.5 * (dy * 21.0 + (dx * 5.0 + t * 0.12).sin() * 2.2).sin();
                    let l = (body_light(dx, dy, z) * (0.72 + 0.28 * band)).clamp(0.0, 1.0);
                    mark = if l > 0.66 {
                        '█'
                    } else if l > 0.42 {
                        '▓'
                    } else if l > 0.20 {
                        '▒'
                    } else {
                        '░'
                    };
                    light = l;
                    phase = 0.18 + band * 0.10;
                    bg_light = 0.014 + l * 0.07;
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.55 + dy * 0.03,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            let (mp, mm) = tidal_masses(pr, mr);
            let total = mp + mm;
            let bary = [
                (mp * planet[0] + mm * moon[0]) / total,
                (mp * planet[1] + mm * moon[1]) / total,
            ];
            let planet_orbit = distance(planet, bary);
            let moon_orbit = distance(moon, bary);
            for k in 0..128 {
                let a = k as f32 / 128.0 * TAU;
                let p = [
                    bary[0]
                        + dir[0] * planet_orbit * a.cos()
                        + perp[0] * planet_orbit * 0.30 * a.sin(),
                    bary[1]
                        + dir[1] * planet_orbit * a.cos()
                        + perp[1] * planet_orbit * 0.30 * a.sin(),
                ];
                c.plot(p, '·', 0.34, 0.12 + bass * 0.04);
                let m = [
                    bary[0] + dir[0] * moon_orbit * a.cos() + perp[0] * moon_orbit * 0.30 * a.sin(),
                    bary[1] + dir[1] * moon_orbit * a.cos() + perp[1] * moon_orbit * 0.30 * a.sin(),
                ];
                c.plot(m, '·', 0.40, 0.10 + bass * 0.05);
            }
            c.plot(bary, '·', 0.70, 0.18 + bass * 0.06);
            let (near, far) = tidal_bridge(planet, pr, moon, mr);
            let mid = [(near[0] + far[0]) * 0.5, (near[1] + far[1]) * 0.5];
            let strands = 3 + (self.burst * 3.0) as usize;
            for s in 0..strands {
                let spread = (s as f32 - (strands as f32 - 1.0) * 0.5) * 0.06;
                let bow = 0.18 * (1.0 + bass) + spread;
                let control = [mid[0] + perp[0] * bow, mid[1] + perp[1] * bow];
                let mut prev = near;
                for k in 1..=24 {
                    let u = k as f32 / 24.0;
                    let v = 1.0 - u;
                    let p = [
                        v * v * near[0] + 2.0 * v * u * control[0] + u * u * far[0],
                        v * v * near[1] + 2.0 * v * u * control[1] + u * u * far[1],
                    ];
                    c.line(
                        prev,
                        p,
                        '·',
                        0.34 + s as f32 * 0.03,
                        (1.0 - u) * 0.28 + 0.06 + self.burst * 0.25,
                    );
                    prev = p;
                }
            }
            if self.burst > 0.04 {
                let h = 0.05 + self.burst * 0.22;
                c.line(
                    [mid[0] - perp[0] * h, mid[1] - perp[1] * h],
                    [mid[0] + perp[0] * h, mid[1] + perp[1] * h],
                    '·',
                    0.32,
                    0.20 + self.burst * 0.60,
                );
                c.circle(
                    mid,
                    0.07 + self.burst * 0.20,
                    '·',
                    0.30,
                    0.12 + self.burst * 0.48,
                );
                c.plot(mid, '✦', 0.26, 0.25 + self.burst * 0.65);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resonance(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let beat = unit(audio.beat);
        let radii = resonance_radii(0.34 + bass * 0.02);
        // The attack starts an outward ring that fades as the beat envelope
        // decays, so the reset coincides with a transient instead of a slow
        // coordinate sliding back to zero.
        let pulse_r = (1.0 - beat) * 2.35;
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let r = x.hypot(y);
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase = 0.5_f32;
                for (k, radius) in radii.iter().enumerate() {
                    let w = (-((r - radius) / 0.026).powi(2)).exp();
                    if w > 0.06 {
                        mark = if w > 0.55 { '─' } else { '·' };
                        light = (0.16 + w * 0.45) * (0.6 + mid * 0.4);
                        phase = 0.38 + k as f32 * 0.055;
                    }
                }
                let pulse = (-((r - pulse_r) / 0.030).powi(2)).exp() * (0.25 + beat * 0.7);
                if pulse > light && pulse > 0.05 {
                    mark = '◦';
                    light = pulse;
                    phase = 0.68;
                }
                let dust = 0.05 + 0.05 * (r * 60.0 - t * 0.7).sin().abs();
                let bg_light = 0.008 + light * 0.07 + dust * 0.02;
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase,
                    light,
                    0.50 + r * 0.02,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        let counts = [1usize, 1, 2, 3, 5, 8];
        let mu = 1.0;
        let mut bodies: Vec<Vec<[f32; 2]>> = Vec::with_capacity(radii.len());
        for (k, radius) in radii.iter().enumerate() {
            let speed = angular_velocity(*radius, mu);
            let base = t * speed + k as f32 * 0.9;
            let mut ring = Vec::with_capacity(counts[k]);
            for n in 0..counts[k] {
                let a = base + n as f32 * TAU / counts[k] as f32;
                ring.push([a.cos() * radius, a.sin() * radius]);
            }
            bodies.push(ring);
        }
        let mut c = Canvas::new(buf, area, visible, palette, theme);
        c.plot([0.0, 0.0], '✦', 0.12, 0.85);
        c.circle([0.0, 0.0], 0.05, '·', 0.14, 0.4);
        for (k, ring) in bodies.iter().enumerate() {
            for p in ring {
                c.circle(*p, 0.035, '·', 0.30 + k as f32 * 0.05, 0.35);
                c.plot(*p, '●', 0.40 + k as f32 * 0.06, 0.85);
            }
        }
        for k in 0..bodies.len() - 1 {
            let a = bodies[k][0];
            let b = bodies[k + 1][0];
            let align = a[1].atan2(a[0]).cos() * b[1].atan2(b[0]).cos()
                + a[1].atan2(a[0]).sin() * b[1].atan2(b[0]).sin();
            if align > 0.93 {
                c.line(a, b, '┈', 0.60, 0.30 + align * 0.4);
            }
        }
        for n in 0..48 {
            let a = n as f32 / 48.0 * TAU + t * 0.1;
            c.plot(
                [a.cos() * pulse_r, a.sin() * pulse_r],
                '·',
                0.66,
                0.25 + beat * 0.4,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn roche(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        audio: &Reactivity,
        theme: &Theme,
        palette: usize,
        t: f32,
    ) {
        wash(buf, visible, palette, theme, 0.58, 0.010);
        sprinkle_stars(buf, visible, palette, theme, t, 0.7);
        let bass = unit(audio.bass);
        let energy = unit(audio.energy);
        let impact = unit(audio.transients[1]).max(unit(audio.transients[2]));
        let planet = [-1.55_f32, 0.0_f32];
        let planet_r = 0.95_f32;
        let moon_r = 0.26_f32;
        let density_ratio = 1.75 + bass * 0.20;
        let roche = roche_limit(planet_r, density_ratio, 1.0);
        let flatten: f32 = 0.32;
        let orbit = KeplerOrbit {
            a: roche,
            e: 0.14,
            inc: flatten.acos(),
            arg: 0.0,
        };
        let mean = t * 0.45 + 5.75;
        let rel = orbit.point(mean);
        let moon = [planet[0] + rel[0], planet[1] + rel[1]];
        let distance = orbit.radius(mean);
        let stretch = tidal_stretch(distance, roche);
        let (major, minor) = tidal_axes(moon_r, stretch);
        let axis_len = rel[0].hypot(rel[1]).max(1.0e-3);
        let axis = [rel[0] / axis_len, rel[1] / axis_len];
        let phase = true_anomaly(eccentric_anomaly(mean, orbit.e), orbit.e) + orbit.arg;
        let xs = &self.xs;
        let ys = &self.ys;
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                let mut mark = ' ';
                let mut light = 0.0_f32;
                let mut phase_color = 0.5_f32;
                let mut bg_light = 0.010;
                let dx = x - moon[0];
                let dy = y - moon[1];
                let lx = dx * axis[0] + dy * axis[1];
                let ly = -dx * axis[1] + dy * axis[0];
                let md = (lx / major).hypot(ly / minor);
                if md < 1.0 {
                    let z = (1.0 - md * md).max(0.0).sqrt();
                    let seam = (lx * 18.0 + (ly * 22.0).sin() * 1.6 + t * 0.5).sin();
                    let crack = (-(seam / 0.10).powi(2)).exp() * (stretch - 1.0).clamp(0.0, 1.5);
                    let l = (0.18 + 0.62 * z + crack * 0.3).clamp(0.0, 1.0);
                    mark = if crack > 0.55 {
                        '╱'
                    } else if l > 0.55 {
                        '▓'
                    } else {
                        '▒'
                    };
                    light = l;
                    phase_color = 0.26 + crack * 0.12;
                    bg_light = 0.012 + l * 0.05;
                } else if (x - planet[0]).hypot(y - planet[1]) < planet_r {
                    let dx = (x - planet[0]) / planet_r;
                    let dy = (y - planet[1]) / planet_r;
                    let z = (1.0 - (dx * dx + dy * dy).clamp(0.0, 1.0)).max(0.0).sqrt();
                    let band = 0.5 + 0.5 * (dy * 24.0 + (dx * 6.0 + t * 0.1).sin() * 2.0).sin();
                    let storm = hash((x * 4.0) as i32, (y * 7.0) as i32);
                    let mut l = (body_light(dx, dy, z) * (0.70 + 0.30 * band)).clamp(0.0, 1.0);
                    if storm > 0.93 {
                        l = (l * 1.25).min(1.0);
                    }
                    mark = if l > 0.68 {
                        '█'
                    } else if l > 0.44 {
                        '▓'
                    } else if l > 0.21 {
                        '▒'
                    } else {
                        '░'
                    };
                    light = l;
                    phase_color = 0.20 + band * 0.08;
                    bg_light = 0.014 + l * 0.07;
                }
                paint(
                    buf,
                    visible.x + i as u16,
                    visible.y + j as u16,
                    mark,
                    phase_color,
                    light,
                    0.55 + y * 0.03,
                    bg_light,
                    palette,
                    theme,
                );
            }
        }
        {
            let mut c = Canvas::new(buf, area, visible, palette, theme);
            for k in 0..128 {
                let a = k as f32 / 128.0 * TAU;
                c.plot(
                    orbit_point(planet, roche, a, flatten),
                    '·',
                    0.50,
                    0.05 + 0.03 * energy,
                );
            }
            for k in 0..128 {
                let m = k as f32 / 128.0 * TAU;
                let q = orbit.point(m);
                c.plot([planet[0] + q[0], planet[1] + q[1]], '·', 0.42, 0.045);
            }
            let amount = disrupted_fraction(distance, roche, impact);
            if amount > 0.01 {
                let glow = amount.sqrt();
                for k in 0..110 {
                    let h1 = hash(k, 7);
                    let h2 = hash(k, 19);
                    let h3 = hash(k, 31);
                    let a = roche * (0.90 + 0.18 * h1 + 0.03 * (h2 - 0.5));
                    let drift = phase + (h2 - 0.5) * 1.2 + angular_velocity(a, 1.0) * t * 0.5;
                    let p = orbit_point(planet, a, drift, flatten);
                    let glyph = if h3 > 0.90 {
                        '●'
                    } else if h3 > 0.50 {
                        '•'
                    } else {
                        '·'
                    };
                    c.plot(
                        p,
                        glyph,
                        0.24 + 0.18 * h1,
                        glow * (0.26 + 0.62 * h3) * (0.65 + 0.35 * energy),
                    );
                }
            }
            c.plot(moon, '✶', 0.34, if distance < roche { 0.55 } else { 0.92 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 80, 30)
    }

    fn render_with(style: usize, audio: &Reactivity, frames: usize) -> (Renderer, Buffer) {
        let area = area();
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..frames {
            renderer.draw(
                &mut buf,
                area,
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                audio,
                &Theme::default(),
            );
        }
        (renderer, buf)
    }

    fn render(style: usize, audio: &Reactivity, frames: usize) -> Buffer {
        render_with(style, audio, frames).1
    }

    fn cell_world(col: u16, row: u16) -> [f32; 2] {
        let area = area();
        let scale = 2.0 / area.height as f32;
        let cx = area.x as f32 + area.width as f32 * 0.5;
        let cy = area.y as f32 + area.height as f32 * 0.5;
        [
            (col as f32 + 0.5 - cx) * scale,
            (row as f32 + 0.5 - cy) * scale,
        ]
    }

    fn painted(buf: &Buffer) -> usize {
        buf.content.iter().filter(|c| c.symbol() != " ").count()
    }

    fn count_char(buf: &Buffer, glyph: char) -> usize {
        buf.content
            .iter()
            .filter(|c| c.symbol() == glyph.to_string())
            .count()
    }

    fn count_near(buf: &Buffer, p: [f32; 2], radius: f32) -> usize {
        let mut n = 0;
        for (i, cell) in buf.content.iter().enumerate() {
            if cell.symbol() == " " {
                continue;
            }
            let q = cell_world((i % 80) as u16, (i / 80) as u16);
            if (q[0] - p[0]).hypot(q[1] - p[1]) < radius {
                n += 1;
            }
        }
        n
    }

    fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
        (a[0] - b[0]).hypot(a[1] - b[1])
    }

    fn comet_tail_cells(renderer: &Renderer, buf: &Buffer) -> usize {
        let orbit = CometOrbit::new(0.0, renderer.precession);
        let head = orbit.focus(CometOrbit::mean_anomaly(renderer.clock));
        let mut away = [head[0] - orbit.star[0], head[1] - orbit.star[1]];
        let d = away[0].hypot(away[1]).max(0.001);
        away = [away[0] / d, away[1] / d];
        let perp = [-away[1], away[0]];
        let mut n = 0;
        for (i, cell) in buf.content.iter().enumerate() {
            if cell.symbol() == " " {
                continue;
            }
            let p = cell_world((i % 80) as u16, (i / 80) as u16);
            let dx = p[0] - head[0];
            let dy = p[1] - head[1];
            let along = dx * away[0] + dy * away[1];
            let side = dx * perp[0] + dy * perp[1];
            if (0.35..1.25).contains(&along) && side.abs() < 0.30 {
                n += 1;
            }
        }
        n
    }

    fn signature(buf: &Buffer) -> Vec<(String, ratatui::style::Color, ratatui::style::Color)> {
        buf.content
            .iter()
            .map(|cell| (cell.symbol().to_owned(), cell.fg, cell.bg))
            .collect()
    }

    #[test]
    fn ten_unique_scene_names() {
        assert_eq!(NAMES.len(), 10);
        let names: std::collections::HashSet<_> = NAMES.iter().collect();
        assert_eq!(names.len(), NAMES.len());
    }

    #[test]
    fn every_style_draws_and_styles_differ() {
        let quiet = Reactivity::default();
        let mut images = std::collections::HashSet::new();
        for style in 0..NAMES.len() {
            let image = signature(&render(style, &quiet, 36));
            let marks = image
                .iter()
                .filter(|(mark, fg, bg)| mark != " " || fg != bg)
                .count();
            assert!(
                marks > 80 * 30 / 20,
                "style {style} painted only {marks} cells"
            );
            assert!(images.insert(image), "duplicate style {style}");
        }
        assert_eq!(images.len(), NAMES.len());
    }

    #[test]
    fn audio_changes_every_scene() {
        let quiet = Reactivity::default();
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.35,
            chroma: [0.8; 12],
            transients: [0.9; 4],
            waveform: [0.5; 32],
            novelty: 0.6,
            centroid: 0.5,
            flatness: 0.4,
            ..Reactivity::default()
        };
        for style in 0..NAMES.len() {
            let a = signature(&render(style, &quiet, 36));
            let b = signature(&render(style, &loud, 36));
            assert!(a != b, "audio does not change style {style}");
        }
    }

    #[test]
    fn each_scene_keeps_its_audio_wiring() {
        // Every orbital scene scales its clock by bass and energy; check them
        // one at a time so a dropped input cannot hide behind the other.
        let quiet = Reactivity::default();
        let probes = [
            (
                "bass",
                Reactivity {
                    bass: 0.85,
                    ..Reactivity::default()
                },
            ),
            (
                "energy",
                Reactivity {
                    energy: 0.7,
                    ..Reactivity::default()
                },
            ),
        ];
        for (style, name) in NAMES.iter().enumerate() {
            let base = signature(&render(style, &quiet, 24));
            for (input, audio) in &probes {
                assert!(
                    base != signature(&render(style, audio, 24)),
                    "{name} ignores {input}"
                );
            }
        }
    }

    #[test]
    fn clipped_areas_leave_outside_cells_untouched() {
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(4, 3, 17, 9),
                Rect::new(0, 0, 1, 1),
                Rect::new(0, 0, 0, 0),
                Rect::new(120, 80, 4, 4),
                Rect::new(20, 10, 24, 14),
            ] {
                let mut buf = Buffer::empty(Rect::new(2, 1, 30, 18));
                for cell in &mut buf.content {
                    cell.set_char('X');
                }
                let mut renderer = Renderer::default();
                renderer.draw(
                    &mut buf,
                    area,
                    f32::NAN,
                    style,
                    0,
                    &[f32::NAN; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                let clip = area.intersection(buf.area);
                for y in buf.area.y..buf.area.bottom() {
                    for x in buf.area.x..buf.area.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "X", "style {style}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn same_state_renders_identically() {
        let audio = Reactivity {
            energy: 0.6,
            bass: 0.5,
            mid: 0.4,
            treble: 0.3,
            kick: 0.8,
            onset: 0.7,
            beat_phase: 0.25,
            ..Reactivity::default()
        };
        for style in 0..NAMES.len() {
            let a = signature(&render(style, &audio, 24));
            let b = signature(&render(style, &audio, 24));
            assert_eq!(a, b, "style {style} is not deterministic");
        }
    }

    fn attack(scale: f32) -> Reactivity {
        Reactivity {
            transients: [scale; 4],
            onset: scale,
            ..Reactivity::default()
        }
    }

    #[test]
    fn comet_orbit_radius_and_precession_follow_bass() {
        let quiet = CometOrbit::new(0.0, 0.0);
        let low = CometOrbit::new(0.9, 0.0);
        assert!(low.a > quiet.a + 0.1, "bass must widen the orbit");
        for anomaly in [0.0, 0.8, 1.9, 3.6, 5.1] {
            let r0 = dist(quiet.focus(anomaly), quiet.star);
            let r1 = dist(low.focus(anomaly), low.star);
            assert!(
                r1 > r0,
                "bass must raise the orbital radius at anomaly {anomaly}"
            );
        }
        let turned = CometOrbit::new(0.9, 0.6);
        assert!(
            dist(low.focus(0.0), turned.focus(0.0)) > 0.2,
            "precession must rotate the orbit"
        );
    }

    #[test]
    fn comet_tail_visibly_extends_with_treble() {
        let quiet = Reactivity::default();
        let bright = Reactivity {
            treble: 1.0,
            ..Reactivity::default()
        };
        let (quiet_renderer, quiet_buf) = render_with(5, &quiet, 40);
        let (bright_renderer, bright_buf) = render_with(5, &bright, 40);
        // Treble alone must not move the head, so the length difference is the
        // tail and not a different part of the orbit.
        assert!((quiet_renderer.clock - bright_renderer.clock).abs() < 1.0e-5);
        let quiet_tail = comet_tail_cells(&quiet_renderer, &quiet_buf);
        let bright_tail = comet_tail_cells(&bright_renderer, &bright_buf);
        assert!(quiet_tail > 0, "the comet needs a visible tail in silence");
        assert!(
            bright_tail > quiet_tail + 10,
            "treble should lengthen the tail: {quiet_tail} -> {bright_tail}"
        );
    }

    #[test]
    fn gravity_assist_encounter_bends_and_clears_the_planet() {
        let calm = AssistPath::new(0.0);
        let hit = AssistPath::new(1.0);
        assert!(hit.rp < calm.rp - 0.1, "attack must lower the periapsis");
        assert!(
            hit.bend() > calm.bend() + 0.25,
            "attack must sharpen the flyby"
        );
        assert!(
            calm.rp > 0.34 && hit.rp > 0.34,
            "periapsis must clear the planet"
        );
        for path in [calm, hit] {
            let entry = path.point(0.0);
            let exit = path.point(1.0);
            assert!(
                entry[0] < path.center[0],
                "entry must approach from the left"
            );
            assert!(exit[0] > path.center[0], "exit must leave to the right");
            assert!(entry.iter().chain(exit.iter()).all(|v| v.is_finite()));
        }
    }

    #[test]
    fn gravity_assist_shows_an_encounter_not_one_blob() {
        let (_, quiet) = render_with(6, &Reactivity::default(), 40);
        let (_, loud) = render_with(6, &attack(0.95), 40);
        assert_eq!(
            count_char(&quiet, '◦'),
            0,
            "silence should not draw the shock ring"
        );
        assert!(
            count_char(&loud, '◦') > 12,
            "an attack should ring the periapsis, not just brighten the planet"
        );
        assert_eq!(count_char(&quiet, '◆'), 1, "the ship stays visible");
        assert!(
            painted(&quiet) > 800,
            "the flyby path must fill the field from the first frames"
        );
    }

    #[test]
    fn lagrange_bass_opens_the_pair_and_attacks_bloom_the_trojans() {
        let calm = Lagrange::new(0.0, 0.0);
        let low = Lagrange::new(0.9, 0.0);
        assert!(
            low.sep > calm.sep + 0.15,
            "bass must widen the two-body separation"
        );
        let l4_calm = calm.points_world()[3];
        let l4_low = low.points_world()[3];
        assert!(dist(l4_calm, l4_low) > 0.1, "bass must move the L4 point");
        let (quiet_renderer, quiet) = render_with(2, &Reactivity::default(), 40);
        let (loud_renderer, loud) = render_with(2, &attack(0.95), 40);
        let quiet_l4 = Lagrange::new(0.0, quiet_renderer.precession).points_world()[3];
        let loud_l4 = Lagrange::new(0.0, loud_renderer.precession).points_world()[3];
        assert!(
            count_near(&quiet, quiet_l4, 0.45) > 20,
            "the L4 region must be painted in silence"
        );
        assert!(
            count_char(&loud, '•') > count_char(&quiet, '•') + 4,
            "the transient should expand the trojan bloom"
        );
        assert!(
            count_near(&loud, loud_l4, 0.45) > 20,
            "the L4 region must stay painted"
        );
    }

    #[test]
    fn tidal_lock_opens_the_orbit_and_flares_the_bridge_on_attack() {
        let (p0, _, m0, _) = tidal_bodies(0.0, 0.0);
        let (p1, _, m1, _) = tidal_bodies(0.9, 0.0);
        assert!(
            dist(p1, m1) > dist(p0, m0) + 0.1,
            "bass must open the moon's orbit"
        );
        let (quiet_renderer, quiet) = render_with(7, &Reactivity::default(), 40);
        let (loud_renderer, loud) = render_with(7, &attack(0.95), 40);
        let (p, pr, m, mr) = tidal_bodies(0.0, quiet_renderer.precession);
        let (pl, prl, ml, mrl) = tidal_bodies(0.0, loud_renderer.precession);
        let (near, far) = tidal_bridge(p, pr, m, mr);
        let (near_loud, far_loud) = tidal_bridge(pl, prl, ml, mrl);
        let mid = [(near[0] + far[0]) * 0.5, (near[1] + far[1]) * 0.5];
        let mid_loud = [
            (near_loud[0] + far_loud[0]) * 0.5,
            (near_loud[1] + far_loud[1]) * 0.5,
        ];
        assert!(
            count_near(&loud, mid_loud, 0.3) > count_near(&quiet, mid, 0.3) + 10,
            "the attack should light the tidal bridge between the bodies"
        );
        for body in [p0, p1, m0, m1] {
            assert!(body.iter().all(|v| v.is_finite() && v.abs() < 2.6));
        }
    }

    #[test]
    fn kepler_ellipses_keep_the_star_at_the_focus() {
        let mu = 1.0;
        let orbits = [
            KeplerOrbit {
                a: 0.62,
                e: 0.16,
                inc: 0.0,
                arg: 0.35,
            },
            KeplerOrbit {
                a: 1.04,
                e: 0.26,
                inc: 0.0,
                arg: 2.15,
            },
            KeplerOrbit {
                a: 1.58,
                e: 0.33,
                inc: 0.0,
                arg: 4.30,
            },
        ];
        for orbit in orbits {
            let peri = orbit.point(0.0);
            let apo = orbit.point(std::f32::consts::PI);
            assert!((peri[0].hypot(peri[1]) - orbit.a * (1.0 - orbit.e)).abs() < 1.0e-4);
            assert!((apo[0].hypot(apo[1]) - orbit.a * (1.0 + orbit.e)).abs() < 1.0e-4);
            for nu in [0.4_f32, 1.3, 2.7, 4.5] {
                let p = orbit.point_from_true(nu);
                let r = p[0].hypot(p[1]);
                let expected = orbit.a * (1.0 - orbit.e * orbit.e) / (1.0 + orbit.e * nu.cos());
                assert!((r - expected).abs() < 1.0e-3, "conic radius at nu={nu}");
            }
            let step = 1.0e-3;
            let peri_speed = dist(orbit.point(step), orbit.point(-step)) / (2.0 * step);
            let apo_speed = dist(
                orbit.point(std::f32::consts::PI + step),
                orbit.point(std::f32::consts::PI - step),
            ) / (2.0 * step);
            assert!(peri_speed > apo_speed * 1.3, "periastron must be faster");
        }
        let inner = orbits[0];
        let outer = orbits[2];
        let ratio = inner.period(mu) / outer.period(mu);
        assert!((ratio - (inner.a / outer.a).powf(1.5)).abs() < 1.0e-4);
    }

    #[test]
    fn ring_gaps_follow_moon_radii_and_keplerian_shear() {
        let planet = 0.30;
        let moon = 0.024;
        let own = shepherd_gap(0.86, moon, planet, 1.0, 0.94);
        let inner = shepherd_gap(0.86, moon, planet, 0.5, 0.60);
        assert!((own.center - 0.86).abs() < 1.0e-6);
        assert!((inner.center - 0.86 * 0.5_f32.powf(2.0 / 3.0)).abs() < 1.0e-6);
        assert!((own.half_width / inner.half_width - 1.8 / 1.1).abs() < 1.0e-5);
        let moon_period = orbital_period(0.86, 1.0);
        let ring_period = orbital_period(inner.center, 1.0);
        assert!((moon_period / ring_period - 2.0).abs() < 1.0e-3);
        let gaps = ring_gaps(&[(0.86, moon), (1.72, 0.030)], planet);
        assert_eq!(gaps.len(), 6);
        let shear = angular_velocity(0.5, 1.0) / angular_velocity(1.5, 1.0);
        assert!((shear - 3.0_f32.powf(1.5)).abs() < 1.0e-4);
    }

    #[test]
    fn lagrange_points_solve_the_restricted_problem() {
        for m2 in [0.16_f32, 0.25, 0.34] {
            let l = lagrange_collinear(m2);
            let x1 = -m2 / (1.0 + m2);
            let x2 = 1.0 / (1.0 + m2);
            assert!(l[0] > x1 && l[0] < x2, "L1 lies between the masses");
            assert!(l[1] > x2, "L2 lies beyond the secondary");
            assert!(l[2] < x1, "L3 lies beyond the primary");
            for x in l {
                let h = 1.0e-3;
                let grad = (restricted_potential([x + h, 0.0], m2)
                    - restricted_potential([x - h, 0.0], m2))
                    / (2.0 * h);
                assert!(grad.abs() < 1.0e-2, "collinear point is not an equilibrium");
            }
            let mid = (x1 + x2) * 0.5;
            let y = 0.5 * 3.0_f32.sqrt();
            for py in [y, -y] {
                let d1 = ((mid - x1).powi(2) + py * py).sqrt();
                let d2 = ((mid - x2).powi(2) + py * py).sqrt();
                assert!((d1 - 1.0).abs() < 1.0e-5 && (d2 - 1.0).abs() < 1.0e-5);
                let u4 = restricted_potential([mid, py], m2);
                assert!(lagrange_levels(m2)
                    .iter()
                    .any(|level| (level - u4).abs() < 1.0e-4));
            }
        }
        let shallow = lagrange_collinear(0.10)[0];
        let deep = lagrange_collinear(0.30)[0];
        assert!(
            shallow > deep,
            "a heavier secondary pulls L1 toward the primary"
        );
    }

    #[test]
    fn eclipse_occlusion_and_angular_sizes() {
        assert!((occulted_fraction(1.0, 1.0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!(occulted_fraction(1.0, 1.0, 2.0).abs() < 1.0e-6);
        let partial = occulted_fraction(1.0, 0.5, 1.0);
        assert!(partial > 0.0 && partial < 0.25);
        assert!(occulted_fraction(1.0, 0.5, 0.5) > partial);
        let inside = occulted_fraction(1.0, 0.5, 0.4);
        assert!((inside - 0.25).abs() < 1.0e-6);
        let total = occulted_fraction(0.56, 0.60, 0.02);
        assert!((total - 1.0).abs() < 1.0e-6);
        let annular = occulted_fraction(0.56, 0.42, 0.0);
        assert!((annular - (0.42 / 0.56_f32).powi(2)).abs() < 1.0e-6);
        let near_distance = 0.64;
        let far_distance = 0.78;
        assert!(
            angular_radius(0.42, near_distance) > angular_radius(0.42, far_distance),
            "a closer moon looks larger"
        );
    }

    #[test]
    fn accretion_disc_shears_and_beams_by_keplerian_speed() {
        let mu = 1.0;
        let inner = 0.4;
        let outer = 1.6;
        let shear = angular_velocity(inner, mu) / angular_velocity(outer, mu);
        assert!((shear - (outer / inner).powf(1.5)).abs() < 1.0e-4);
        let inc = 0.3_f32;
        let right = doppler_boost(inner, 0.0, inc, 0.35);
        let left = doppler_boost(inner, std::f32::consts::PI, inc, 0.35);
        assert!(right > left * 1.5, "the approaching limb must be brighter");
        let outer_right = doppler_boost(outer, 0.0, inc, 0.35);
        assert!(
            right > outer_right + 0.15,
            "beaming is strongest near the centre"
        );
        assert!((doppler_boost(inner, 0.0, inc, 0.0) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn comet_uses_kepler_not_anomaly_as_angle() {
        let orbit = CometOrbit::new(0.4, 0.7);
        let mean = 1.2;
        let ecc = eccentric_anomaly(mean, orbit.e);
        let nu = true_anomaly(ecc, orbit.e);
        let expected = orbit.a * (1.0 - orbit.e * orbit.e) / (1.0 + orbit.e * nu.cos());
        assert!((orbit.radius(mean) - expected).abs() < 1.0e-4);
        let old_radius = orbit.a * (1.0 - orbit.e * mean.cos());
        assert!(
            (orbit.radius(mean) - old_radius).abs() > 1.0e-2,
            "mean anomaly must not be used as the eccentric angle"
        );
        let peri = orbit.focus(0.0);
        let apo = orbit.focus(std::f32::consts::PI);
        assert!((dist(peri, orbit.star) - orbit.a * (1.0 - orbit.e)).abs() < 1.0e-4);
        assert!((dist(apo, orbit.star) - orbit.a * (1.0 + orbit.e)).abs() < 1.0e-4);
    }

    #[test]
    fn comet_tail_points_away_from_the_sun() {
        let (renderer, buf) = render_with(5, &Reactivity::default(), 40);
        let orbit = CometOrbit::new(0.0, renderer.precession);
        let head = orbit.focus(CometOrbit::mean_anomaly(renderer.clock));
        let mut away = [head[0] - orbit.star[0], head[1] - orbit.star[1]];
        let d = away[0].hypot(away[1]).max(1.0e-3);
        away = [away[0] / d, away[1] / d];
        let perp = [-away[1], away[0]];
        let mut forward = 0;
        let mut backward = 0;
        for (i, cell) in buf.content.iter().enumerate() {
            if cell.symbol() == " " {
                continue;
            }
            let p = cell_world((i % 80) as u16, (i / 80) as u16);
            let dx = p[0] - head[0];
            let dy = p[1] - head[1];
            let along = dx * away[0] + dy * away[1];
            let side = dx * perp[0] + dy * perp[1];
            let star_clear = (d - 0.20).max(0.30);
            if side.abs() < 0.18 {
                if (0.25..0.95).contains(&along) {
                    forward += 1;
                } else if along < -0.30 && along > -star_clear {
                    backward += 1;
                }
            }
        }
        assert!(
            forward > backward * 3,
            "tail must extend away from the sun: {forward} forward, {backward} backward"
        );
    }

    #[test]
    fn gravity_assist_is_a_consistent_hyperbola() {
        let mu = 1.0;
        for burst in [0.0_f32, 0.5, 1.0] {
            let path = AssistPath::new(burst);
            let e = 1.0 + path.rp * path.v_inf * path.v_inf / mu;
            assert!((path.e - e).abs() < 1.0e-5);
            assert!((path.p - path.rp * (1.0 + path.e)).abs() < 1.0e-5);
            let a = -mu / (path.v_inf * path.v_inf);
            assert!((path.e - (1.0 - path.rp / a)).abs() < 1.0e-5);
            assert!((path.energy() - 0.5 * path.v_inf * path.v_inf).abs() < 1.0e-6);
            assert!((dist(path.point(0.5), path.periapsis())).abs() < 1.0e-4);
            assert!((dist(path.point(0.5), path.center) - path.rp).abs() < 1.0e-4);
            assert!(path.point(0.0)[1] > path.periapsis()[1]);
        }
        let path = AssistPath::new(0.0);
        let step = 1.0e-4;
        let near = dist(path.point(0.5 + step), path.point(0.5 - step)) / (2.0 * step);
        let far = dist(path.point(0.02 + step), path.point(0.02 - step)) / (2.0 * step);
        assert!(
            near > far * 1.2,
            "hyperbolic motion is fastest at periapsis"
        );
    }

    #[test]
    fn tidal_pair_is_barycentric_and_synchronously_locked() {
        for bass in [0.0_f32, 0.5, 1.0] {
            let (p, pr, m, mr) = tidal_bodies(bass, 0.0);
            let (mp, mm) = tidal_masses(pr, mr);
            let total = mp + mm;
            let bary = [-0.11, 0.02];
            let cx = (mp * p[0] + mm * m[0]) / total;
            let cy = (mp * p[1] + mm * m[1]) / total;
            assert!((cx - bary[0]).abs() < 1.0e-5);
            assert!((cy - bary[1]).abs() < 1.0e-5);
            let rp = dist(p, bary);
            let rm = dist(m, bary);
            assert!((rp / rm - mm / mp).abs() < 1.0e-4);
            assert!((dist(m, p) - (pr + mr + 0.28 + 0.18 * bass)).abs() < 1.0e-4);
            let (major, minor) = tidal_axes(mr, 1.8);
            assert!((major * minor * minor - mr.powi(3)).abs() < 1.0e-5);
            assert!(major > mr && minor < mr);
        }
        let (p, pr, m, mr) = tidal_bodies(0.0, 0.8);
        let (near, far) = tidal_bridge(p, pr, m, mr);
        assert!((dist(near, m) - mr).abs() < 1.0e-5);
        assert!((dist(far, p) - pr).abs() < 1.0e-5);
    }

    #[test]
    fn resonance_chain_forms_the_stated_period_ratios() {
        let radii = resonance_radii(0.34);
        let mu = 1.0;
        assert!(radii.windows(2).all(|w| w[1] > w[0]));
        for (k, step) in RESONANCE_STEPS.iter().enumerate() {
            let (p, q) = *step;
            let ratio = orbital_period(radii[k + 1], mu) / orbital_period(radii[k], mu);
            assert!((ratio - p / q).abs() < 1.0e-3, "step {k}: {ratio}");
        }
        let speed_ratio = angular_velocity(radii[0], mu) / angular_velocity(radii[5], mu);
        assert!((speed_ratio - (radii[5] / radii[0]).powf(1.5)).abs() < 1.0e-4);
    }

    #[test]
    fn roche_limit_controls_moon_disruption() {
        let equal = roche_limit(1.0, 1.0, 1.0);
        assert!((equal - 2.44).abs() < 1.0e-4);
        assert!(roche_limit(1.0, 2.0, 1.0) > equal);
        assert!(roche_limit(1.0, 1.0, 2.0) < equal);
        let roche = roche_limit(0.95, 1.8, 1.0);
        assert!(tidal_stretch(roche * 0.9, roche) > 1.0);
        assert!(tidal_stretch(roche * 1.1, roche) < 1.0);
        assert!(disrupted_fraction(roche * 1.1, roche, 1.0).abs() < 1.0e-6);
        assert!(disrupted_fraction(roche * 0.9, roche, 1.0) > 0.0);
        let (major, minor) = tidal_axes(0.26, tidal_stretch(roche * 0.9, roche));
        assert!(major > minor);
        assert!((major * minor * minor - 0.26_f32.powi(3)).abs() < 1.0e-5);
    }

    #[test]
    fn every_scene_fills_a_96x32_frame() {
        let area = Rect::new(0, 0, 96, 32);
        let mut buf = Buffer::empty(area);
        let audio = Reactivity::default();
        for style in 0..NAMES.len() {
            let mut renderer = Renderer::default();
            for frame in 0..24 {
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.4; 32],
                    &audio,
                    &Theme::default(),
                );
            }
            let marks = buf
                .content
                .iter()
                .filter(|cell| cell.symbol() != " " || cell.fg != cell.bg)
                .count();
            assert!(
                marks > 96 * 32 / 24,
                "style {style} painted only {marks} cells"
            );
        }
    }

    #[test]
    fn envelopes_stay_bounded_under_extreme_audio() {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(area());
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        let broken = Reactivity {
            energy: f32::NAN,
            bass: f32::INFINITY,
            transients: [f32::NAN, f32::INFINITY, -3.0, 2.0],
            ..Reactivity::default()
        };
        for frame in 0..600 {
            let audio = if frame % 3 == 0 { &broken } else { &loud };
            let time = if frame % 17 == 0 {
                f32::NAN
            } else {
                frame as f32 / 60.0
            };
            renderer.draw(
                &mut buf,
                area(),
                time,
                frame % NAMES.len(),
                0,
                &[f32::NAN; 32],
                audio,
                &Theme::default(),
            );
            assert!(renderer.burst.is_finite());
            assert!((0.0..=1.0).contains(&renderer.burst));
            assert!(renderer.precession.is_finite());
            assert!((0.0..TAU).contains(&renderer.precession));
            assert!(renderer.ship.is_finite());
            assert!((0.0..1.0).contains(&renderer.ship));
            assert!(renderer.clock.is_finite());
        }
    }
}
