use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};

pub const NAMES: &[&str] = &[
    "Life Pulse",
    "Brians Brain",
    "Cyclic Cells",
    "Forest Fire",
    "Excitable Tissue",
    "Sandpile",
    "Rule 110",
    "Rule 30",
    "Turmite Walk",
    "Crystal Growth",
];

const W: usize = 96;
const H: usize = 54;
const LEN: usize = W * H;
const SOURCES: [(usize, usize); 4] = [(24, 27), (48, 18), (72, 27), (48, 36)];

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Default)]
pub struct Renderer {
    cells: Vec<u16>,
    next: Vec<u16>,
    glow: Vec<u8>,
    mode: Option<usize>,
    last_time: Option<f32>,
    accumulator: f32,
    tick: u64,
    rng: u32,
    ant: (usize, usize, usize),
    walkers: Vec<(usize, usize)>,
    attacks: [f32; 4],
    pending: [u8; 4],
}

impl Renderer {
    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn reset(&mut self, mode: usize) {
        self.cells = vec![0; LEN];
        self.next = vec![0; LEN];
        self.glow = vec![0; LEN];
        self.mode = Some(mode);
        self.rng = 0x39a2_74b1;
        self.tick = 0;
        self.accumulator = 0.0;
        self.attacks = [0.0; 4];
        self.pending = [0; 4];
        self.ant = (W / 2, H / 2, 0);
        self.walkers.clear();
        match mode {
            0 | 1 => {
                // Separated gliders / firing clusters leave room for their wakes.
                for &(x, y) in &SOURCES {
                    for (dx, dy) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
                        self.cells[(y + dy) * W + x + dx] = 1;
                    }
                }
                if mode == 0 {
                    for y in [10, 39] {
                        for x in 36..39 {
                            self.cells[y * W + x] = 1;
                        }
                    }
                }
            }
            2 => {
                for y in 0..H {
                    for x in 0..W {
                        let angle = (y as f32 - 27.0).atan2((x as f32 - 48.0) * 0.65);
                        let noise = (self.random() % 3) as u16;
                        self.cells[y * W + x] =
                            (((angle + std::f32::consts::PI) * 8.0 / std::f32::consts::TAU) as u16
                                + noise)
                                % 8;
                    }
                }
            }
            3 => {
                for y in 6..H - 6 {
                    for x in 10..W - 10 {
                        if self.random() % 100 < 66 {
                            self.cells[y * W + x] = 1;
                        }
                    }
                }
                self.cells[27 * W + 48] = 2;
            }
            4 => {
                for y in 17..37 {
                    self.cells[y * W + 47] = 1;
                }
                for y in 27..39 {
                    self.cells[y * W + 48] = 5;
                }
            }
            5 => {
                // An unstable mound is already avalanching on the first frame.
                for y in 15_usize..40 {
                    for x in 32_usize..65 {
                        let d = x.abs_diff(48) + y.abs_diff(27);
                        if d < 21 {
                            self.cells[y * W + x] = (24 - d) as u16;
                        }
                    }
                }
            }
            6 | 7 => {
                self.cells[(H - 1) * W + W / 2] = 1;
            }
            8 => {}
            _ => {
                self.cells[27 * W + 48] = 1;
                for d in 1..5 {
                    for (x, y) in [(48 + d, 27), (48 - d, 27), (48, 27 + d), (48, 27 - d)] {
                        self.cells[y * W + x] = 1;
                    }
                }
                for _ in 0..128 {
                    let x = self.random() as usize % W;
                    let y = self.random() as usize % H;
                    self.walkers.push((x, y));
                }
            }
        }
        for _ in 0..32 {
            self.step(mode, &[0.0; 4]);
        }
    }

    // Periodic boundaries for local automata; sand and elementary rules have
    // absorbing edges. DLA walkers reflect and the ant wraps without teleporting.
    fn neighbors(&self, x: usize, y: usize, state: u16, diagonal: bool) -> u8 {
        let mut n = 0;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if (dx == 0 && dy == 0) || (!diagonal && dx != 0 && dy != 0) {
                    continue;
                }
                let xx = (x as isize + dx).rem_euclid(W as isize) as usize;
                let yy = (y as isize + dy).rem_euclid(H as isize) as usize;
                n += u8::from(self.cells[yy * W + xx] == state);
            }
        }
        n
    }

    fn inject(&mut self, mode: usize, channel: usize, amount: u8) {
        let (x, y) = SOURCES[channel];
        match mode {
            0 | 1 => {
                for (dx, dy) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
                    self.cells[(y + dy) * W + x + dx] = 1;
                }
            }
            2 => {
                let successor = (self.cells[y * W + x] + 1) % 8;
                for dy in 0..3 {
                    for dx in 0..3 {
                        self.cells[(y + dy) * W + x + dx] = successor;
                    }
                }
            }
            3 => {
                // The forest is sparse, so a fixed 3x3 stamp usually lands on
                // ash. Walk outward from this band's source until standing
                // trees are found, then ignite up to `amount` of them.
                let mut lit = 0usize;
                'rings: for radius in 0..=12i32 {
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            if radius > 0 && dx.abs() != radius && dy.abs() != radius {
                                continue;
                            }
                            let x = (SOURCES[channel].0 as i32 + dx).rem_euclid(W as i32) as usize;
                            let y = (SOURCES[channel].1 as i32 + dy).rem_euclid(H as i32) as usize;
                            let i = y * W + x;
                            if self.cells[i] == 1 {
                                self.cells[i] = 2;
                                lit += 1;
                                if lit >= usize::from(amount.max(1)) {
                                    break 'rings;
                                }
                            }
                        }
                    }
                }
            }
            4 => {
                for dy in 0..5 {
                    let i = (y + dy - 2) * W + x;
                    if self.cells[i] == 0 {
                        self.cells[i] = 1;
                    }
                }
            }
            5 => {
                let i = y * W + x;
                self.cells[i] = self.cells[i].saturating_add(amount as u16 * 4).min(2048);
            }
            6 | 7 => {
                self.cells[(H - 1) * W + x] ^= 1;
            }
            8 => {
                // Alter the substrate just ahead of the walker, not its position.
                let (x, y, _) = self.ant;
                self.cells[y * W + (x + 1) % W] ^= 1;
            }
            _ => {
                // Increase local sticking sites on the existing aggregate only.
                for y in 1..H - 1 {
                    for x in 1..W - 1 {
                        let i = y * W + x;
                        if self.cells[i] == 1 && (x + y + channel).is_multiple_of(11) {
                            self.glow[i] = 255;
                        }
                    }
                }
                // Additional walkers start at a fixed reservoir boundary.
                if self.walkers.len() < 160 {
                    self.walkers.push((x, 1));
                }
            }
        }
    }

    fn step(&mut self, mode: usize, drive: &[f32; 4]) {
        self.tick = self.tick.wrapping_add(1);
        for v in &mut self.glow {
            *v = v.saturating_sub(8);
        }
        for (channel, level) in drive.iter().enumerate() {
            let amount = self.pending[channel];
            if amount > 0 {
                self.inject(mode, channel, amount);
                self.pending[channel] = 0;
            } else if *level > 0.2 && self.tick.is_multiple_of(9) {
                self.inject(mode, channel, 1);
            }
        }
        // Quiet pacemakers are local, deterministic sources, not reseeded grids.
        if mode <= 1 && self.tick.is_multiple_of(61) {
            self.inject(mode, 1, 1);
        }
        if mode == 4 && self.tick.is_multiple_of(29) {
            self.inject(mode, 1, 1);
        }
        if mode == 3 && self.tick.is_multiple_of(47) {
            self.inject(mode, 0, 1);
        }
        if mode == 5 {
            let i = 27 * W + 48;
            self.cells[i] = self.cells[i].saturating_add(6).min(2048);
        }
        if mode == 8 {
            for _ in 0..96 {
                let (x, y, dir) = self.ant;
                let i = y * W + x;
                let turn = if self.cells[i] == 0 { 1 } else { 3 };
                let dir = (dir + turn) % 4;
                self.cells[i] ^= 1;
                self.glow[i] = 255;
                let (dx, dy) = [(0, -1), (1, 0), (0, 1), (-1, 0)][dir];
                self.ant = (
                    ((x as isize + dx).rem_euclid(W as isize)) as usize,
                    ((y as isize + dy).rem_euclid(H as isize)) as usize,
                    dir,
                );
            }
            return;
        }
        if mode == 9 {
            // Diffusion-limited aggregation with slow dissolution: each deposit
            // has a finite residence time. Keep a small nucleus; older material
            // dissolves locally so a finite terminal never fills and freezes.
            for y in 0..H {
                for x in 0..W {
                    let i = y * W + x;
                    if self.cells[i] == 0 {
                        continue;
                    }
                    let nucleus = x.abs_diff(48) + y.abs_diff(27) <= 1;
                    self.next[i] = self.next[i].saturating_add(1);
                    if !nucleus && self.next[i] > 320 + ((x * 17 + y * 31) % 280) as u16 {
                        self.cells[i] = 0;
                        self.next[i] = 0;
                    }
                }
            }
            // Walkers move one lattice bond and stick only next to a deposit.
            for k in 0..self.walkers.len() {
                let (mut x, mut y) = self.walkers[k];
                for _ in 0..24 {
                    let direction = self.random() % 4;
                    match direction {
                        0 => x = (x + 1).min(W - 2),
                        1 => x = x.saturating_sub(1).max(1),
                        2 => y = (y + 1).min(H - 2),
                        _ => y = y.saturating_sub(1).max(1),
                    }
                    if self.cells[y * W + x] == 0 && self.neighbors(x, y, 1, false) > 0 {
                        self.cells[y * W + x] = 1;
                        self.next[y * W + x] = 0;
                        self.glow[y * W + x] = 255;
                        let r = self.random() as usize;
                        // Recycled particles enter from the outer reservoir.
                        (x, y) = match r % 4 {
                            0 => (1, 1 + r / 4 % (H - 2)),
                            1 => (W - 2, 1 + r / 4 % (H - 2)),
                            2 => (1 + r / 4 % (W - 2), 1),
                            _ => (1 + r / 4 % (W - 2), H - 2),
                        };
                    }
                }
                self.walkers[k] = (x, y);
            }
            return;
        }
        self.next.fill(0);
        if mode == 6 || mode == 7 {
            // Space-time scroll: bottom is the evolving 1-D generation.
            self.next[..(H - 1) * W].copy_from_slice(&self.cells[W..]);
            let rule = if mode == 6 { 110_u8 } else { 30_u8 };
            for x in 1..W - 1 {
                let i = (H - 1) * W + x;
                let code = self.cells[i - 1] * 4 + self.cells[i] * 2 + self.cells[i + 1];
                self.next[i] = ((rule >> code) & 1) as u16;
            }
        } else {
            for y in 0..H {
                for x in 0..W {
                    let i = y * W + x;
                    let v = self.cells[i];
                    self.next[i] = match mode {
                        0 => {
                            let n = self.neighbors(x, y, 1, true);
                            u16::from(n == 3 || (v == 1 && n == 2))
                        }
                        1 => match v {
                            0 => u16::from(self.neighbors(x, y, 1, true) == 2),
                            1 => 2,
                            _ => 0,
                        },
                        2 => {
                            let successor = (v + 1) % 8;
                            if self.neighbors(x, y, successor, false) >= 1 {
                                successor
                            } else {
                                v
                            }
                        }
                        3 => match v {
                            0 => {
                                // Slow seed rain; sparse trees stay visible between burns.
                                u16::from(self.random().is_multiple_of(900))
                            }
                            1 => {
                                if self.neighbors(x, y, 2, true) > 0 {
                                    2
                                } else {
                                    1
                                }
                            }
                            2..=9 => v + 1,
                            _ => 0,
                        },
                        4 => match v {
                            0 => u16::from(self.neighbors(x, y, 1, false) > 0),
                            1..=8 => v + 1,
                            _ => 0,
                        },
                        5 => {
                            // Parallel Abelian toppling; grains crossing an edge leave.
                            let mut grains = v % 4;
                            if x > 0 {
                                grains += self.cells[i - 1] / 4;
                            }
                            if x + 1 < W {
                                grains += self.cells[i + 1] / 4;
                            }
                            if y > 0 {
                                grains += self.cells[i - W] / 4;
                            }
                            if y + 1 < H {
                                grains += self.cells[i + W] / 4;
                            }
                            grains.min(2048)
                        }
                        _ => 0,
                    };
                    if self.next[i] != v {
                        self.glow[i] = 255;
                    }
                }
            }
        }
        std::mem::swap(&mut self.cells, &mut self.next);
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
        if self.mode != Some(mode) {
            self.reset(mode);
            self.last_time = None;
        }
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        let mut drive = [0.0; 4];
        for channel in 0..4 {
            let level = unit(audio.transients[channel]).max(match channel {
                0 => unit(audio.kick),
                1 => unit(audio.onset),
                2 => unit(audio.novelty),
                _ => unit(audio.onset) * unit(audio.treble),
            });
            if level - self.attacks[channel] > 0.1 && level > 0.18 {
                self.pending[channel] = self.pending[channel]
                    .saturating_add((level * 8.0) as u8 + 1)
                    .min(16);
            }
            self.attacks[channel] = level;
            drive[channel] = (0..8).map(|i| unit(bands[channel * 8 + i])).sum::<f32>() / 8.0;
        }
        self.accumulator = (self.accumulator + dt * (10.0 + unit(audio.energy) * 8.0)).min(2.0);
        for _ in 0..2 {
            if self.accumulator < 1.0 {
                break;
            }
            self.accumulator -= 1.0;
            self.step(mode, &drive);
        }
        if area.width == 0 || area.height == 0 {
            return;
        }
        let visible = area.intersection(buf.area);
        let background = theme.sample(palette, 0.0, 0.0);
        for row in visible.y..visible.bottom() {
            for col in visible.x..visible.right() {
                let x = (col - area.x) as usize * W / area.width as usize;
                let y = (row - area.y) as usize * H / area.height as usize;
                let i = y * W + x;
                let v = self.cells[i];
                let wake = self.glow[i] as f32 / 255.0;
                let (glyph, phase, light) = match mode {
                    0 => {
                        if v == 1 {
                            ('●', 0.48 + y as f32 * 0.003, 0.8)
                        } else {
                            ('·', 0.65, wake * 0.22)
                        }
                    }
                    1 => match v {
                        1 => ('✦', 0.8, 0.95),
                        2 => ('∘', 0.58, 0.36),
                        _ => (' ', 0.0, 0.0),
                    },
                    2 => {
                        // Draw only one oriented frontier per domain, not a grid.
                        let edge = self.cells[y * W + (x + 1) % W] == (v + 1) % 8;
                        if edge {
                            ('╱', v as f32 / 8.0, 0.55 + wake * 0.25)
                        } else {
                            (' ', 0.0, 0.0)
                        }
                    }
                    3 => match v {
                        1 => ('♣', 0.38, 0.22),
                        2 => ('▲', 0.04, 0.95),
                        3..=5 => ('˙', 0.12, 0.3),
                        _ => (' ', 0.0, 0.0),
                    },
                    4 => match v {
                        1 => ('━', 0.75, 0.9),
                        2..=4 => ('─', 0.54, (5 - v) as f32 * 0.10),
                        _ => (' ', 0.0, 0.0),
                    },
                    5 => match v {
                        0 => (' ', 0.0, 0.0),
                        1 => ('·', 0.08, 0.28),
                        2 => ('⁙', 0.14, 0.4),
                        3 => ('▪', 0.24, 0.55),
                        _ => ('◆', 0.02, 0.9),
                    },
                    6 => {
                        if v == 1 {
                            (
                                '▰',
                                0.72 - y as f32 * 0.005,
                                0.3 + y as f32 / H as f32 * 0.5,
                            )
                        } else {
                            (' ', 0.0, 0.0)
                        }
                    }
                    7 => {
                        if v == 1 {
                            ('▴', 0.05 + x as f32 * 0.003, 0.55)
                        } else {
                            (' ', 0.0, 0.0)
                        }
                    }
                    8 => {
                        if (x, y) == (self.ant.0, self.ant.1) {
                            ('◆', 0.12, 1.0)
                        } else if v == 1 {
                            ('▪', 0.4 + wake * 0.3, 0.24 + wake * 0.6)
                        } else {
                            ('·', 0.6, wake * 0.18)
                        }
                    }
                    _ => {
                        if v == 1 {
                            let n = self.neighbors(x, y, 1, false);
                            (
                                if n <= 1 {
                                    '✧'
                                } else if n == 2 {
                                    '┼'
                                } else {
                                    '◆'
                                },
                                0.55 + wake * 0.2,
                                0.45 + wake * 0.5,
                            )
                        } else {
                            (' ', 0.0, 0.0)
                        }
                    }
                };
                let cell = &mut buf[(col, row)];
                cell.reset();
                cell.set_char(if light > 0.02 { glyph } else { ' ' })
                    .set_bg(background)
                    .set_fg(theme.sample(palette, phase, light));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(mode: usize) -> Renderer {
        Renderer {
            cells: vec![0; LEN],
            next: vec![0; LEN],
            glow: vec![0; LEN],
            mode: Some(mode),
            rng: 43219,
            ..Renderer::default()
        }
    }

    #[test]
    fn local_rules_are_not_variations_of_one_rule() {
        let mut life = blank(0);
        for x in 30..33 {
            life.cells[27 * W + x] = 1;
        }
        life.step(0, &[0.0; 4]);
        assert_eq!((0..LEN).filter(|&i| life.cells[i] == 1).count(), 3);
        assert_eq!(life.cells[26 * W + 31], 1);
        assert_eq!(life.cells[27 * W + 30], 0);
        let mut brain = blank(1);
        brain.cells[27 * W + 30] = 1;
        brain.cells[27 * W + 32] = 1;
        brain.step(1, &[0.0; 4]);
        assert_eq!(brain.cells[27 * W + 31], 1);
        assert_eq!(brain.cells[27 * W + 30], 2);
        brain.step(1, &[0.0; 4]);
        assert_eq!(brain.cells[27 * W + 30], 0);
        let mut cyclic = blank(2);
        cyclic.cells[27 * W + 31] = 1;
        cyclic.step(2, &[0.0; 4]);
        assert_eq!(cyclic.cells[27 * W + 30], 1);
        assert_eq!(cyclic.cells[26 * W + 30], 0); // Cardinal, not diagonal.
        let mut fire = blank(3);
        fire.cells[27 * W + 30] = 2;
        fire.cells[27 * W + 31] = 1;
        fire.step(3, &[0.0; 4]);
        assert_eq!(fire.cells[27 * W + 31], 2);
        assert_eq!(fire.cells[27 * W + 30], 3);
        let mut tissue = blank(4);
        tissue.cells[27 * W + 30] = 1;
        tissue.cells[27 * W + 31] = 4;
        tissue.step(4, &[0.0; 4]);
        assert_eq!(tissue.cells[27 * W + 31], 5); // Refractory cannot fire.
        assert_eq!(tissue.cells[26 * W + 30], 1);
    }

    #[test]
    fn forest_fire_ignition_finds_sparse_trees() {
        let mut fire = blank(3);
        // One tree inside the search radius, but outside the old fixed 3x3 stamp.
        let (sx, sy) = SOURCES[0];
        let tree = sy * W + sx + 5;
        fire.cells[tree] = 1;
        fire.inject(3, 0, 1);
        assert_eq!(fire.cells[tree], 2, "band strike missed a standing tree");
    }

    #[test]
    fn sand_topples_atomically_and_loses_grains_at_edges() {
        let mut sand = blank(5);
        sand.cells[12 * W + 12] = 8;
        sand.step(5, &[0.0; 4]);
        assert_eq!(sand.cells[12 * W + 12], 0);
        assert_eq!(sand.cells[12 * W + 11], 2);
        assert_eq!(sand.cells[11 * W + 12], 2);
        assert_eq!(sand.cells.iter().map(|&v| v as usize).sum::<usize>(), 14); // 6 driver grains.
        let mut edge = blank(5);
        edge.cells[0] = 4;
        edge.step(5, &[0.0; 4]);
        assert_eq!(edge.cells.iter().map(|&v| v as usize).sum::<usize>(), 8);
    }

    #[test]
    fn elementary_truth_tables_and_history() {
        for (mode, expected) in [(6, [1, 1, 0]), (7, [1, 1, 1])] {
            let mut r = blank(mode);
            r.cells[(H - 1) * W + 48] = 1;
            r.step(mode, &[0.0; 4]);
            assert_eq!(&r.cells[(H - 1) * W + 47..(H - 1) * W + 50], &expected);
            assert_eq!(r.cells[(H - 2) * W + 48], 1);
        }
    }

    #[test]
    fn turmite_and_crystal_preserve_their_geometry() {
        let mut ant = blank(8);
        ant.ant = (48, 27, 0);
        ant.step(8, &[0.0; 4]);
        assert_ne!(ant.ant, (48, 27, 0));
        assert!(ant.cells.iter().all(|&v| v <= 1));
        let mut crystal = Renderer::default();
        crystal.reset(9);
        let before = crystal.cells.clone();
        for _ in 0..40 {
            crystal.step(9, &[0.0; 4]);
        }
        assert!(crystal.cells.iter().zip(&before).all(|(&a, &b)| a >= b));
        assert!(crystal.cells.iter().sum::<u16>() > before.iter().sum::<u16>());
        for y in 0..H {
            for x in 0..W {
                if crystal.cells[y * W + x] == 1 {
                    assert!(crystal.neighbors(x, y, 1, false) > 0);
                }
            }
        }
    }

    #[test]
    fn clipping_offsets_disjoint_and_tiny() {
        let theme = Theme::default();
        for mode in 0..NAMES.len() {
            for area in [
                Rect::new(0, 0, 0, 0),
                Rect::new(8, 6, 1, 1),
                Rect::new(0, 0, 20, 20),
                Rect::new(10, 8, 30, 14),
                Rect::new(90, 90, 12, 12),
                Rect::new(0, 0, 1, 1),
            ] {
                let mut buf = Buffer::empty(Rect::new(7, 5, 15, 11));
                for cell in &mut buf.content {
                    cell.set_char('Z');
                }
                let original = buf.clone();
                let mut r = Renderer::default();
                r.draw(
                    &mut buf,
                    area,
                    0.0,
                    mode,
                    0,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &theme,
                );
                let clip = area.intersection(buf.area);
                for y in buf.area.y..buf.area.bottom() {
                    for x in buf.area.x..buf.area.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)], original[(x, y)]);
                        }
                    }
                }
            }
        }
    }

    fn mask(buf: &Buffer) -> Vec<String> {
        buf.content.iter().map(|c| c.symbol().to_owned()).collect()
    }

    #[test]
    fn distinct_scenes_and_structural_audio() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 96, 54);
        let mut signatures = Vec::new();
        for mode in 0..NAMES.len() {
            let mut quiet = Renderer::default();
            let mut loud = Renderer::default();
            let mut a = Buffer::empty(area);
            let mut b = a.clone();
            for frame in 0..90 {
                quiet.draw(
                    &mut a,
                    area,
                    frame as f32 / 30.0,
                    mode,
                    0,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &theme,
                );
                let pulse = if frame % 6 < 2 { 1.0 } else { 0.0 };
                let audio = Reactivity {
                    energy: 0.8,
                    transients: [pulse; 4],
                    ..Reactivity::default()
                };
                loud.draw(
                    &mut b,
                    area,
                    frame as f32 / 30.0,
                    mode,
                    0,
                    &[0.8; 32],
                    &audio,
                    &theme,
                );
            }
            assert_ne!(
                mask(&a),
                mask(&b),
                "audio must alter structure in mode {mode}"
            );
            assert!(a.content.iter().any(|c| c.symbol() != " "), "quiet {mode}");
            assert!(!signatures.contains(&mask(&a)), "duplicate {mode}");
            signatures.push(mask(&a));
        }
    }

    #[test]
    fn lazy_default_and_bounded_state_under_bad_inputs() {
        let theme = Theme::default();
        let mut r = Renderer::default();
        assert!(r.cells.is_empty() && r.walkers.is_empty());
        for mode in 0..NAMES.len() {
            let mut buf = Buffer::empty(Rect::new(2, 4, 2, 1));
            let audio = Reactivity {
                energy: f32::INFINITY,
                onset: f32::NAN,
                transients: [f32::NAN; 4],
                ..Reactivity::default()
            };
            for time in [0.0, f32::NAN, -1e9, 1e9, f32::INFINITY, 0.0] {
                r.draw(
                    &mut buf,
                    Rect::new(0, 0, 20, 10),
                    time,
                    mode,
                    usize::MAX,
                    &[f32::NAN; 32],
                    &audio,
                    &theme,
                );
                assert_eq!(r.cells.len(), LEN);
                assert_eq!(r.next.len(), LEN);
                assert!(r.cells.iter().all(|&v| v <= 2048));
                assert!(r.accumulator.is_finite() && r.accumulator <= 2.0);
                assert!(r.ant.0 < W && r.ant.1 < H);
                assert!(r.walkers.len() <= 160);
            }
        }
    }

    #[test]
    #[ignore = "manual release-mode frame timing at 160x50"]
    fn frame_budget() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 160, 50);
        for (mode, name) in NAMES.iter().enumerate() {
            let mut r = Renderer::default();
            let mut buf = Buffer::empty(area);
            let initial = std::time::Instant::now();
            r.draw(
                &mut buf,
                area,
                0.0,
                mode,
                0,
                &[0.4; 32],
                &Reactivity::default(),
                &theme,
            );
            let initial_ms = initial.elapsed().as_secs_f64() * 1000.0;
            let start = std::time::Instant::now();
            for frame in 1..121 {
                r.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    mode,
                    0,
                    &[0.4; 32],
                    &Reactivity::default(),
                    &theme,
                );
            }
            eprintln!(
                "{}: {:.3} ms initial, {:.3} ms/frame",
                name,
                initial_ms,
                start.elapsed().as_secs_f64() * 1000.0 / 120.0
            );
        }
    }
}
