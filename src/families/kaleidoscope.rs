//! Kaleidoscope and symmetry: mirrored mandalas, rosettes, prisms, folded
//! light and crystalline repeats. Fields are evaluated in a square world frame
//! with two vertical samples per terminal cell.
use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{PI, TAU};

pub const NAMES: &[&str] = &[
    "Mandala Bloom",
    "Snowflake Vault",
    "Prism Fan",
    "Folded Light",
    "Tiled Reflections",
    "Crystal Repeats",
    "Rosette Wheels",
    "Mirror Corridor",
    "Kaleido Weave",
    "Radial Shatter",
];

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    spin: f32,
    envelopes: [f32; 4],
    attack: f32,
    impact: f32,
    xs: Vec<f32>,
    ys: Vec<[f32; 2]>,
}

struct Params {
    style: usize,
    t: f32,
    spin: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    energy: f32,
    kick: f32,
    beat: f32,
    beat_phase: f32,
    impact: f32,
    centroid: f32,
    flatness: f32,
    chroma_angle: f32,
    chroma_mag: f32,
    bands: [f32; 8],
    wave: [f32; 8],
    folds: usize,
    snow: [[f32; 4]; 6],
    tile: f32,
    hex: f32,
    rosette: [[f32; 3]; 12],
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash2(a: i32, b: i32) -> f32 {
    let mut n = (a as u32).wrapping_mul(374_761_393) ^ (b as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

/// Angle inside one mirrored half-sector, from the mirror axis to the edge.
fn fold_angle(theta: f32, folds: usize) -> f32 {
    let sector = TAU / folds as f32;
    let mut angle = theta.rem_euclid(sector);
    if angle > sector * 0.5 {
        angle = sector - angle;
    }
    angle
}

fn triangle(u: f32) -> f32 {
    let v = (u * 0.5 + 0.5).rem_euclid(1.0);
    4.0 * (v - 0.5).abs() - 1.0
}

/// Repeat a coordinate in mirrored panels, so radial motifs form a frieze.
fn mirror_fold(x: f32) -> f32 {
    let period = 4.0;
    let mut u = x.rem_euclid(period);
    if u > period * 0.5 {
        u = period - u;
    }
    u - period * 0.25
}

fn segment_distance(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len = dx * dx + dy * dy;
    let t = if len > 1e-9 {
        (((px - ax) * dx + (py - ay) * dy) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - ax - dx * t).hypot(py - ay - dy * t)
}

/// Axial rounding for a pointy-topped hexagonal lattice.
fn hex_round(q: f32, r: f32) -> (f32, f32) {
    let s = -q - r;
    let mut rq = q.round();
    let mut rr = r.round();
    let rs = s.round();
    let dq = (rq - q).abs();
    let dr = (rr - r).abs();
    let ds = (rs - s).abs();
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    (rq, rr)
}

fn mandala(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let x = mirror_fold(x);
    let theta = y.atan2(x) + p.spin * 0.8 + p.chroma_angle * 0.25;
    let r = x.hypot(y);
    let sector = TAU / p.folds as f32;
    let u = fold_angle(theta, p.folds) / (sector * 0.5);
    let petal = (u * PI * 0.5).cos();
    let pulse = (p.beat_phase * TAU).sin();
    let rim = 0.62 + 0.16 * petal + p.bass * 0.08 + p.chroma_mag * 0.04 + p.beat * 0.03 * pulse;
    let ring = (-((r - rim) * 10.0).powi(2)).exp();
    let inside = (1.0 - r / (rim + 0.06)).clamp(0.0, 1.0);
    let webbing = ((r * 21.0 - p.t * 2.0 + u * 3.0).sin() * 0.5 + 0.5) * inside;
    let spokes = (-(u * u) * 130.0).exp() * inside;
    let heart = (-r * r * 80.0).exp() * (0.75 + p.impact * 0.5);
    let veil = 0.05 + 0.11 * (u * PI).sin() * inside;
    let v = ring * 0.9 + webbing * 0.5 + spokes * 0.45 + heart + veil;
    let pigment = 0.05 + u * 0.36 + r * 0.2 + p.chroma_angle * 0.04;
    (v, pigment)
}

fn snowflake(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let x = mirror_fold(x);
    let theta = y.atan2(x) + p.spin * 0.25;
    let r = x.hypot(y);
    let sector = TAU / 6.0;
    let angle = fold_angle(theta, 6);
    let u = angle / (sector * 0.5);
    let along = r * angle.cos();
    let across = r * angle.sin();
    let arm = if r < 0.95 {
        (-(across / (0.012 + p.treble * 0.007)).powi(2)).exp()
    } else {
        0.0
    };
    let filament = 0.010 + p.treble * 0.006 + p.centroid * 0.004;
    let mut branch: f32 = 0.0;
    for b in &p.snow {
        let d = segment_distance(along, across, b[0], b[1], b[2], b[3]);
        branch = branch.max((-(d / filament).powi(2)).exp());
    }
    let lobes = 0.05 * (theta * 6.0).cos();
    let outer_r = 0.85 + lobes + p.bass * 0.04;
    let inner_r = 0.53 + lobes * 0.7;
    let rim = (-((r - outer_r) * 30.0).powi(2)).exp();
    let inner = (-((r - inner_r) * 26.0).powi(2)).exp() * 0.4;
    let heart = (-r * r * 120.0).exp() * (1.0 + p.impact * 0.4);
    let lattice = 0.035 + 0.05 * ((theta * 6.0).cos() * 0.5 + 0.5) * (0.5 + p.flatness);
    let v = arm * (0.5 + 0.35 * p.treble) + branch * 0.75 + rim * 0.8 + inner + heart + lattice;
    let pigment = 0.52 + u * 0.16 + p.chroma_angle * 0.04 + along * 0.1;
    (v, pigment)
}

fn prism(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let theta = y.atan2(x) + p.spin * 0.3;
    let r = x.hypot(y);
    let wedges = 12usize;
    let sector = TAU / wedges as f32;
    let pos = theta.rem_euclid(TAU) / sector;
    let index = pos.floor() as usize % wedges;
    let frac = pos.fract();
    let band = p.bands[(index + (p.wave[index % 8] * 3.0) as usize) % 8];
    let pair = if index.is_multiple_of(2) { 1.0 } else { 0.66 };
    let edge = (frac.min(1.0 - frac) * 9.0).min(1.0);
    let inner = (0.28
        + band * 0.55
        + 0.2 * ((r * 11.0 - p.t * 1.7 + index as f32 + p.centroid * 2.0).sin() * 0.5 + 0.5))
        * (0.25 + 0.75 * edge)
        * pair;
    let core = (-r * r * 45.0).exp() * (0.7 + p.kick * 0.5);
    let v = inner * (0.35 + r * 0.55) + core;
    let pigment = index as f32 / wedges as f32 + r * 0.12 + p.chroma_angle * 0.02;
    (v, pigment)
}

fn folded(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let kx = 2.2 + p.mid * 1.8;
    let ky = 1.5 + p.treble * 1.2;
    let fx = triangle(x * kx + p.t * 0.25 + p.chroma_angle * 0.2 + p.beat_phase * 0.4);
    let fy = triangle(y * ky - p.t * 0.21);
    let lx = 0.55 * (p.t * 0.6 + p.chroma_angle).cos() * (0.6 + p.bass * 0.4) + p.wave[3] * 0.12;
    let ly = 0.5 * (p.t * 0.47).sin() * (0.5 + p.mid * 0.6);
    let dx = fx - lx;
    let dy = fy - ly;
    let d = (dx * dx + dy * dy).sqrt();
    let orb = (-d * d * 20.0).exp();
    let caustic = (-((d - 0.26) / 0.055).powi(2)).exp() * 0.5;
    let streak = (-(dy * dy) * 60.0).exp() * 0.18 * (1.0 - d.min(1.0));
    let sheen = 0.05 + 0.05 * (fx * fy).max(0.0);
    let v = orb * (0.8 + 0.4 * p.energy) + caustic + streak + sheen;
    let pigment = 0.1 + fx * 0.2 + fy * 0.15 + p.chroma_angle * 0.04;
    (v, pigment)
}

fn tiles(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let u = x / p.tile;
    let v = y / p.tile;
    let fu = u - u.floor();
    let fv = v - v.floor();
    let mu = (fu * 2.0 - 1.0).abs();
    let mv = (fv * 2.0 - 1.0).abs();
    let ix = u.floor() as i32;
    let iy = v.floor() as i32;
    let seed = hash2(ix, iy);
    let band = p.bands[(ix * 3 + iy * 5).rem_euclid(8) as usize];
    let d = (mu - 0.5).abs().max((mv - 0.5).abs());
    let offset = seed * 4.0 + band * 2.0 + p.wave[(ix + iy).rem_euclid(8) as usize] * 0.8;
    let rings = ((d * 9.0 - p.t * 1.4 + offset).sin() * 0.5 + 0.5).powi(3);
    let glow = (-((d - 0.36) * 10.0).powi(2)).exp();
    let body = 0.08 + 0.18 * band + 0.15 * seed;
    let petal = (mu * PI).sin() * (mv * PI).sin() * (0.25 + 0.25 * band);
    let seam = fu.min(1.0 - fu).min(fv.min(1.0 - fv)) * p.tile;
    let line = (-(seam * seam) * 2500.0).exp() * 0.45;
    let v = body + rings * 0.5 + petal + glow * (0.55 + 0.3 * p.beat) + line;
    let pigment = 0.1 + seed * 0.5 + band * 0.25 + d * 0.2;
    (v, pigment)
}

fn crystal(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let s = p.hex;
    let q = (x * 0.577_350_3 - y * 0.333_333_34) / s;
    let r = y * 0.666_666_7 / s;
    let (cq, cr) = hex_round(q, r);
    let cx = s * 1.732_050_8 * (cq + cr * 0.5);
    let cy = s * 1.5 * cr;
    let dx = x - cx;
    let dy = y - cy;
    // Pointy-topped hexagon norm: one at the boundary, zero at the centre.
    let d = dx
        .abs()
        .max((0.5 * dx + 0.866_025_4 * dy).abs())
        .max((0.5 * dx - 0.866_025_4 * dy).abs())
        / (s * 0.866_025_4);
    let angle = dy.atan2(dx);
    let qi = cq as i32;
    let ri = cr as i32;
    let tint = hash2(qi * 7 + 1, ri * 11 + 3);
    let band = p.bands[(qi * 3 + ri * 5).rem_euclid(8) as usize];
    let facet = ((angle + PI * 0.5) / (TAU / 6.0)).floor() as i32;
    let ftint = hash2(qi * 17 + facet * 5 + 2, ri * 13 + facet * 3 + 1);
    let body = 0.08 + 0.26 * tint + 0.20 * band + 0.18 * ftint + 0.05 * p.flatness;
    // Bright facets radiate from the nucleus to each of the six vertices.
    let vertex = ((angle - PI * 0.5) / (TAU / 6.0)).round() * (TAU / 6.0) + PI * 0.5;
    let mut offset = (angle - vertex).rem_euclid(TAU);
    if offset > PI {
        offset = TAU - offset;
    }
    let arc = offset * d.max(0.001) * s * 0.866_025_4;
    let spoke = (-(arc / 0.010).powi(2)).exp();
    let rim = (-((d - 0.92) * 10.0).powi(2)).exp();
    let core = (-d * d * 5.0).exp() * 0.25;
    let v = body * (0.35 + 0.65 * rim) + spoke * 0.5 * rim + rim * 0.6 + core;
    let pigment =
        0.05 + tint * 0.42 + band * 0.2 + ftint * 0.14 + p.centroid * 0.1 + p.chroma_angle * 0.03;
    (v, pigment)
}

fn rosette(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let x = mirror_fold(x);
    let mut glow = 0.0;
    let mut hue = 0.0;
    let width = 14.0 + p.treble * 8.0;
    for (i, c) in p.rosette.iter().enumerate() {
        let dx = x - c[0];
        let dy = y - c[1];
        let d = (dx * dx + dy * dy).sqrt();
        let w = (-((d - c[2]) * width).powi(2)).exp();
        glow += w;
        hue += w * i as f32 / p.rosette.len() as f32;
    }
    let r = x.hypot(y);
    let heart = (-r * r * 70.0).exp() * (0.65 + p.impact * 0.4);
    let rim = (-((r - 0.66 - p.bass * 0.05) * 12.0).powi(2)).exp() * 0.4;
    let v = glow * 0.65 + heart + rim;
    let pigment = if glow > 0.001 { hue / glow } else { 0.3 } + p.chroma_angle * 0.03;
    (v, pigment)
}

fn corridor(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let r = x.abs().max(y.abs()).max(0.002);
    let spacing = 0.32;
    let k = r.ln() / spacing;
    let inner = k - k.floor();
    let index = k.floor();
    let gap_inner = r * (1.0 - (-inner * spacing).exp());
    let gap_outer = r * (((1.0 - inner) * spacing).exp() - 1.0);
    let depth = gap_inner.min(gap_outer);
    let line = (-(depth / 0.012).powi(2)).exp();
    let angle = index * 0.35 + p.spin * 0.3;
    let (sn, cs) = angle.sin_cos();
    let ring_r = (index * spacing).exp();
    let lx = (x * cs - y * sn) / ring_r;
    let ly = (x * sn + y * cs) / ring_r;
    let mu = lx.abs().min(1.0);
    let mv = ly.abs().min(1.0);
    let grain = (mu * 7.0 + p.t * 0.9 + p.flatness * 2.0).sin() * (mv * 7.0 - p.t * 0.7).cos();
    let parity = if (index as i32) & 1 == 0 { 0.0 } else { 0.16 };
    let wall = 0.10 + 0.10 * grain.abs() + parity;
    let heart = (-r * r * 220.0).exp() * (0.8 + p.impact * 0.5);
    let v = line * (0.85 + 0.2 * p.energy) + wall * (1.0 - line * 0.5) + heart;
    let pigment = index.rem_euclid(8.0) * 0.06 + grain * 0.06 + p.chroma_angle * 0.03;
    (v, pigment)
}

fn shatter(p: &Params, x: f32, y: f32) -> (f32, f32) {
    let theta = y.atan2(x) + p.spin * 0.2;
    let r = x.hypot(y);
    let sectors = 12usize;
    let step = TAU / sectors as f32;
    let wobble = 0.10 * ((r * 5.0 - p.t * 0.8).sin() + 0.5 * (r * 9.0 + p.chroma_angle).cos());
    let pos = (theta + wobble).rem_euclid(TAU) / step;
    let si = pos.floor() as i32;
    let sf = pos - pos.floor();
    let u = (sf * 2.0 - 1.0).abs();
    let rings = 2.5 + p.bass * 2.0;
    let stagger = hash2(si, 5) * 0.8 + p.beat_phase * 0.25;
    let rr = r * rings + stagger;
    let ri = rr.floor();
    let rf = rr - ri;
    let tint = hash2(si * 11 + 3, ri as i32 * 5 + 7);
    let band = p.bands[si.rem_euclid(8) as usize];
    let shard = 0.2 + 0.42 * tint + 0.25 * band + 0.08 * ((r * 3.0 - p.t * 0.6).sin() * 0.5 + 0.5);
    let arc = r * step * 0.5 * (1.0 - u);
    let gap = rf.min(1.0 - rf) / rings;
    let crack_a = (-(arc / 0.018).powi(2)).exp();
    let crack_r = (-(gap / 0.018).powi(2)).exp();
    let crack = crack_a.max(crack_r);
    let heart = (-r * r * 40.0).exp() * 0.3;
    let v = shard * (1.0 - 0.92 * crack) + crack * (0.3 + 0.9 * p.impact) + heart;
    let pigment = 0.1
        + tint * 0.42
        + si.rem_euclid(sectors as i32) as f32 / sectors as f32 * 0.24
        + crack * 0.12
        + p.chroma_angle * 0.03;
    (v, pigment)
}

impl Params {
    fn build(style: usize, renderer: &Renderer, audio: &Reactivity, bands: &[f32; 32]) -> Self {
        let (mut cx, mut cy) = (0.0, 0.0);
        for (i, value) in audio.chroma.iter().enumerate() {
            let angle = i as f32 * TAU / 12.0;
            let weight = unit(*value);
            cx += weight * angle.cos();
            cy += weight * angle.sin();
        }
        let chroma_mag = (cx.hypot(cy) / 3.0).min(1.0);
        let bands8: [f32; 8] =
            std::array::from_fn(|i| (0..4).map(|j| unit(bands[i * 4 + j])).sum::<f32>() * 0.25);
        let wave8: [f32; 8] = std::array::from_fn(|i| unit(audio.waveform[i * 4] * 0.5 + 0.5));
        let folds = 6 + (chroma_mag * 4.0) as usize;
        let mut snow = [[0.0; 4]; 6];
        let len = 0.14 + unit(audio.treble) * 0.11;
        for k in 0..3 {
            let origin = 0.20 + k as f32 * 0.17 + unit(audio.bass) * 0.04;
            snow[k] = [
                origin,
                0.0,
                origin + len * 0.58,
                len * (0.68 + k as f32 * 0.06),
            ];
            snow[k + 3] = [
                origin,
                0.0,
                origin + len * 0.26,
                len * (0.52 + k as f32 * 0.05),
            ];
        }
        let mut rosette = [[0.0; 3]; 12];
        let outer = 0.38 + unit(audio.bass) * 0.10;
        let petal = 0.29 + unit(audio.mid) * 0.045;
        for i in 0..6 {
            let angle = renderer.spin + i as f32 * TAU / 6.0;
            rosette[i] = [outer * angle.cos(), outer * angle.sin(), petal];
            let inner_angle = angle + TAU / 12.0;
            rosette[i + 6] = [0.165 * inner_angle.cos(), 0.165 * inner_angle.sin(), 0.145];
        }
        Self {
            style,
            t: renderer.phase,
            spin: renderer.spin,
            bass: renderer.envelopes[0],
            mid: renderer.envelopes[1],
            treble: renderer.envelopes[2],
            energy: unit(audio.energy),
            kick: unit(audio.kick),
            beat: unit(audio.beat),
            beat_phase: unit(audio.beat_phase),
            impact: renderer.impact,
            centroid: unit(audio.centroid),
            flatness: unit(audio.flatness),
            chroma_angle: cy.atan2(cx),
            chroma_mag,
            bands: bands8,
            wave: wave8,
            folds,
            snow,
            tile: 0.50 + unit(audio.bass) * 0.07,
            hex: 0.24 + unit(audio.bass) * 0.06,
            rosette,
        }
    }
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
        let targets = [
            unit(audio.bass),
            unit(audio.mid),
            unit(audio.treble),
            unit(audio.onset).max(unit(audio.kick)),
        ];
        let blend = 1.0 - (-dt * 9.0).exp();
        for (value, target) in self.envelopes.iter_mut().zip(targets) {
            *value += (target - *value) * blend;
        }
        let attack = unit(
            audio
                .transients
                .iter()
                .copied()
                .fold(unit(audio.onset).max(unit(audio.kick)), f32::max),
        );
        if attack > self.attack + 0.08 {
            self.impact = 1.0;
        }
        self.attack = attack;
        self.impact = (self.impact - dt * 3.0).clamp(0.0, 1.0);
        self.phase = (self.phase
            + dt * (0.55 + self.envelopes[1] * 0.9 + unit(audio.energy) * 0.55))
            .rem_euclid(4096.0);
        self.spin = (self.spin + dt * (0.07 + self.envelopes[0] * 0.32 + unit(audio.beat) * 0.22))
            .rem_euclid(TAU * 64.0);
    }

    fn field(&self, p: &Params, x: f32, y: f32) -> (f32, f32) {
        match p.style {
            0 => mandala(p, x, y),
            1 => snowflake(p, x, y),
            2 => prism(p, x, y),
            3 => folded(p, x, y),
            4 => tiles(p, x, y),
            5 => crystal(p, x, y),
            6 => rosette(p, x, y),
            7 => corridor(p, x, y),
            9 => shatter(p, x, y),
            _ => (0.0, 0.0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn weave(
        &self,
        buf: &mut Buffer,
        clip: Rect,
        area: Rect,
        p: &Params,
        palette: usize,
        theme: &Theme,
    ) {
        let h = area.height as f32;
        let scale = 2.0 / h;
        let frequency = 4.0 + p.mid * 2.5;
        let threshold = 0.72 - p.bass * 0.10;
        let span = (1.0 - threshold).max(0.05);
        for y in clip.y..clip.bottom() {
            let wy = ((y - area.y) as f32 * 2.0 + 1.0) / h - 1.0;
            for x in clip.x..clip.right() {
                let wx = ((x - area.x) as f32 + 0.5 - area.width as f32 * 0.5) * scale;
                let sa = ((wx + wy) * frequency + p.t * 0.55 + p.beat_phase * 0.5).sin();
                let sb = ((wx - wy) * frequency - p.t * 0.42).sin();
                let a = ((sa.abs() - threshold) / span).clamp(0.0, 1.0);
                let b = ((sb.abs() - threshold) / span).clamp(0.0, 1.0);
                let ia = ((wx + wy) * frequency / TAU).floor() as i32;
                let ib = ((wx - wy) * frequency / TAU).floor() as i32;
                let band = p.bands[(ia + ib).rem_euclid(8) as usize];
                let warp = p.wave[(ia - ib).rem_euclid(8) as usize];
                let over_a = (ia + ib) & 1 == 0;
                let (mark, shade, pigment) = if a > 0.04 && b > 0.04 {
                    if over_a != (warp > 0.5) {
                        ('╳', a + b * 0.25, 0.16 + band * 0.2)
                    } else {
                        ('╳', b + a * 0.25, 0.60 + band * 0.2)
                    }
                } else if a > 0.04 {
                    ('╱', a, 0.16 + band * 0.2)
                } else if b > 0.04 {
                    ('╲', b, 0.60 + band * 0.2)
                } else {
                    (' ', 0.0, 0.0)
                };
                if mark != ' ' {
                    let intensity =
                        (shade * (0.5 + band * 0.5) * (0.85 + p.treble * 0.25)).clamp(0.0, 1.0);
                    buf[(x, y)].set_char(mark).set_fg(theme.sample(
                        palette,
                        pigment + p.chroma_angle * 0.03,
                        intensity,
                    ));
                }
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
        self.advance(time, audio);
        let visible = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 || visible.is_empty() {
            return;
        }
        let style = style % NAMES.len();
        let p = Params::build(style, self, audio, bands);
        let background = theme.sample(palette, 0.0, 0.0);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)].reset();
                buf[(x, y)].set_char(' ').set_bg(background);
            }
        }
        if style == 8 {
            self.weave(buf, visible, area, &p, palette, theme);
            return;
        }
        let cols = visible.width as usize;
        let rows = visible.height as usize;
        if self.xs.len() < cols {
            self.xs.resize(cols, 0.0);
        }
        if self.ys.len() < rows {
            self.ys.resize(rows, [0.0; 2]);
        }
        let h = area.height as f32;
        let scale = 2.0 / h;
        let half_width = area.width as f32 * 0.5;
        for (i, slot) in self.xs.iter_mut().take(cols).enumerate() {
            let x = (visible.x + i as u16) as f32;
            *slot = (x + 0.5 - area.x as f32 - half_width) * scale;
        }
        for (j, slot) in self.ys.iter_mut().take(rows).enumerate() {
            let y = (visible.y + j as u16) as f32 - area.y as f32;
            *slot = [(y * 2.0 + 0.5) / h - 1.0, (y * 2.0 + 1.5) / h - 1.0];
        }
        for j in 0..rows {
            let [top_y, bottom_y] = self.ys[j];
            let row = visible.y + j as u16;
            for i in 0..cols {
                let x = self.xs[i];
                let (vt, pt) = self.field(&p, x, top_y);
                let (vb, pb) = self.field(&p, x, bottom_y);
                let top = theme.sample(palette, pt, vt);
                let bottom = theme.sample(palette, pb, vb);
                buf[(visible.x + i as u16, row)]
                    .set_char('▀')
                    .set_fg(top)
                    .set_bg(bottom);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(style: usize, audio: Reactivity, area: Rect, frames: usize) -> Buffer {
        let mut renderer = Renderer::default();
        let mut buffer = Buffer::empty(area);
        for i in 0..frames {
            renderer.draw(
                &mut buffer,
                area,
                i as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                &audio,
                &Theme::default(),
            );
        }
        buffer
    }

    fn signature(buffer: &Buffer) -> Vec<(String, ratatui::style::Color, ratatui::style::Color)> {
        buffer
            .content
            .iter()
            .map(|cell| (cell.symbol().to_string(), cell.fg, cell.bg))
            .collect()
    }

    fn drawn_cells(buffer: &Buffer) -> usize {
        let background = Theme::default().sample(0, 0.0, 0.0);
        buffer
            .content
            .iter()
            .filter(|cell| cell.bg != background || (cell.symbol() != " " && cell.fg != background))
            .count()
    }

    fn loud() -> Reactivity {
        Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.5,
            waveform: std::array::from_fn(|i| (i as f32 * 0.7).sin()),
            transients: [1.0; 4],
            novelty: 1.0,
            chroma: [1.0; 12],
            centroid: 0.8,
            flatness: 0.6,
            ..Reactivity::default()
        }
    }

    #[test]
    fn names_are_ten_and_unique() {
        assert_eq!(NAMES.len(), 10);
        for (i, name) in NAMES.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(
                NAMES.iter().skip(i + 1).all(|other| other != name),
                "duplicate scene name {name}"
            );
        }
    }

    #[test]
    fn every_style_draws_across_the_area() {
        let area = Rect::new(0, 0, 80, 30);
        for (style, name) in NAMES.iter().enumerate() {
            let buffer = render(style, loud(), area, 30);
            let cells = drawn_cells(&buffer);
            assert!(
                cells > 120,
                "style {style} ({name}) filled only {cells} cells"
            );
        }
    }

    #[test]
    fn styles_are_visibly_distinct() {
        let area = Rect::new(0, 0, 80, 30);
        let masks: Vec<_> = (0..NAMES.len())
            .map(|style| signature(&render(style, loud(), area, 30)))
            .collect();
        for i in 0..masks.len() {
            for j in i + 1..masks.len() {
                let changed = masks[i]
                    .iter()
                    .zip(&masks[j])
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    changed > area.area() as usize / 4,
                    "styles {i} and {j} share too many cells ({changed} differ)"
                );
            }
        }
    }

    #[test]
    fn every_style_reacts_to_audio() {
        let area = Rect::new(0, 0, 80, 30);
        for (style, name) in NAMES.iter().enumerate() {
            let quiet = signature(&render(style, Reactivity::default(), area, 30));
            let loud = signature(&render(style, loud(), area, 30));
            let changed = quiet.iter().zip(&loud).filter(|(a, b)| a != b).count();
            assert!(
                changed > 15,
                "style {style} ({name}) changed only {changed} cells with audio"
            );
        }
    }

    #[test]
    fn same_state_is_deterministic() {
        let area = Rect::new(0, 0, 60, 20);
        for style in 0..NAMES.len() {
            let first = render(style, loud(), area, 20);
            let second = render(style, loud(), area, 20);
            assert_eq!(first, second, "style {style} is not deterministic");
        }
    }

    #[test]
    fn clips_to_area_and_survives_hostile_input() {
        let outer = Rect::new(5, 4, 41, 21);
        let audio = Reactivity {
            bass: f32::NAN,
            mid: f32::INFINITY,
            treble: -7.0,
            energy: f32::NAN,
            beat_phase: f32::INFINITY,
            waveform: [f32::NAN; 32],
            chroma: [f32::INFINITY; 12],
            ..Default::default()
        };
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(9, 8, 22, 11),
                Rect::new(0, 0, 15, 12),
                Rect::new(60, 40, 8, 8),
                Rect::new(5, 4, 0, 0),
                Rect::new(20, 12, 1, 1),
            ] {
                let mut buffer = Buffer::empty(outer);
                for cell in &mut buffer.content {
                    cell.set_char('!');
                }
                let mut renderer = Renderer::default();
                for (i, time) in [0.0, f32::NAN, f32::INFINITY, 1e9].iter().enumerate() {
                    renderer.draw(
                        &mut buffer,
                        area,
                        *time,
                        style,
                        0,
                        &[f32::NAN; 32],
                        &audio,
                        &Theme::default(),
                    );
                    let visible = area.intersection(outer);
                    for y in outer.y..outer.bottom() {
                        for x in outer.x..outer.right() {
                            if !visible.contains((x, y).into()) {
                                assert_eq!(
                                    buffer[(x, y)].symbol(),
                                    "!",
                                    "style {style} painted outside its area at frame {i}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
