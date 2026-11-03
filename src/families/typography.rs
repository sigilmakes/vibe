use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect};

use crate::{palette::Theme, visuals::Reactivity};

pub const NAMES: &[&str] = &[
    "Glyph Ribbons",
    "Terminal Weave",
    "Braille Halftone",
    "Type Orbit",
    "Glyph Spiral",
    "Scanline Etching",
    "Broken Score",
    "Block Mosaic",
    "Type Helix",
    "Crosshatch Current",
];

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    levels: [f32; 4],
    attacks: [f32; 4],
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn wave(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

// All marks are single-column characters. Coordinates remain local to the
// requested area even when only a small part of that area is in the buffer.
struct Page<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    clip: Rect,
    theme: &'a Theme,
    palette: usize,
}

impl Page<'_> {
    fn mark(&mut self, x: f32, y: f32, glyph: char, pigment: f32, light: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let x = x.round() as i32 + self.area.x as i32;
        let y = y.round() as i32 + self.area.y as i32;
        if x < self.clip.x as i32
            || x >= self.clip.right() as i32
            || y < self.clip.y as i32
            || y >= self.clip.bottom() as i32
        {
            return;
        }
        self.buf[(x as u16, y as u16)]
            .set_char(glyph)
            .set_fg(self.theme.sample(self.palette, pigment, light));
    }
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.08));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        let blend = 1.0 - (-dt * 9.0).exp();
        for (dst, src) in
            self.levels
                .iter_mut()
                .zip([audio.bass, audio.mid, audio.treble, audio.energy])
        {
            *dst += (unit(src) - *dst) * blend;
        }
        for (i, attack) in self.attacks.iter_mut().enumerate() {
            *attack = (*attack * (-dt * 7.0).exp()).max(unit(audio.transients[i]));
        }
        // The helix also uses 0.3 * phase; ten turns close both periods.
        self.phase = (self.phase + dt * (0.22 + self.levels[1] * 0.55)).rem_euclid(10.0 * TAU);
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
        let clip = area.intersection(buf.area);
        if clip.is_empty() {
            return;
        }
        let bg = theme.sample(palette, 0.0, 0.0);
        for y in clip.y..clip.bottom() {
            for x in clip.x..clip.right() {
                buf[(x, y)].set_char(' ').set_bg(bg).set_fg(bg);
            }
        }
        let mut page = Page {
            buf,
            area,
            clip,
            theme,
            palette,
        };
        let w = area.width as f32;
        let h = area.height as f32;
        let t = self.phase;
        let [bass, mid, treble, energy] = self.levels;
        match style % NAMES.len() {
            0 => {
                // Separate calligraphic strips; the glyph direction follows the curve.
                for ribbon in 0..4 {
                    for x in 0..area.width {
                        let u = x as f32 / w;
                        if !(0.07..0.93).contains(&u) {
                            continue;
                        }
                        let a = u * TAU * (1.0 + ribbon as f32 * 0.17) + t + ribbon as f32;
                        let y = h * (0.19 + ribbon as f32 * 0.20)
                            + a.sin() * h * (0.045 + bass * 0.04)
                            + wave(audio.waveform[x as usize * 31 / area.width.max(1) as usize])
                                * h
                                * 0.045;
                        let glyph = if a.cos() > 0.4 {
                            '/'
                        } else if a.cos() < -0.4 {
                            '\\'
                        } else {
                            '~'
                        };
                        for edge in 0..2 {
                            page.mark(
                                x as f32,
                                y + edge as f32,
                                glyph,
                                ribbon as f32 * 0.19 + u * 0.12,
                                0.44 + energy * 0.4,
                            );
                        }
                    }
                }
            }
            1 => {
                // Woven short warp and weft bundles, with alternating overpasses.
                for y in 0..area.height {
                    for x in 0..area.width {
                        let tx = x / 9;
                        let ty = y / 5;
                        if x % 9 == 8 || y % 5 == 4 {
                            continue;
                        }
                        let over = (tx + ty).is_multiple_of(2);
                        let shift = (t.sin() * 1.8 + bass * 2.0) as u16;
                        let warp = (u32::from(x) + u32::from(shift)).is_multiple_of(3);
                        let weft = y.is_multiple_of(2);
                        let glyph = match (over, warp, weft) {
                            (true, true, _) | (false, true, false) => '│',
                            (_, _, true) => '═',
                            _ => continue,
                        };
                        page.mark(
                            x as f32,
                            y as f32,
                            glyph,
                            tx as f32 * 0.07 + ty as f32 * 0.11,
                            0.35 + mid * 0.48,
                        );
                    }
                }
            }
            2 => {
                // Evaluate each of the eight real braille subpixels against an
                // undulating ink silhouette, not a random braille character set.
                const DOTS: [[u32; 4]; 2] = [[1, 2, 4, 64], [8, 16, 32, 128]];
                for y in 0..area.height {
                    for x in 0..area.width {
                        let mut bits = 0;
                        for (dx, column) in DOTS.iter().enumerate() {
                            for (dy, bit) in column.iter().enumerate() {
                                let u = (x as f32 + dx as f32 * 0.5) / w;
                                let v = (y as f32 + dy as f32 * 0.25) / h;
                                let center = 0.5 + (u * TAU * 1.6 + t).sin() * 0.18;
                                let radius =
                                    0.07 + bass * 0.09 + unit(bands[(u * 31.0) as usize]) * 0.10;
                                let threshold =
                                    ((x as usize * 3 + y as usize * 5 + dx + dy) % 8) as f32 / 8.0;
                                let density = (1.0 - (v - center).abs() / radius).max(0.0);
                                if density > threshold {
                                    bits |= bit;
                                }
                            }
                        }
                        if bits != 0 {
                            page.mark(
                                x as f32,
                                y as f32,
                                char::from_u32(0x2800 + bits).unwrap(),
                                x as f32 / w * 0.6,
                                0.6 + treble * 0.3,
                            );
                        }
                    }
                }
            }
            3 => {
                // Three eccentric rings of spaced letterforms and tiny satellites.
                const LETTERS: &[u8] = b"oxcvu";
                for ring in 0..3 {
                    let count = 24 + ring * 12;
                    for i in 0..count {
                        let a = i as f32 / count as f32 * TAU
                            + t * if ring % 2 == 0 { 1.0 } else { -1.0 };
                        let r = 0.14 + ring as f32 * 0.12 + bass * 0.025;
                        let skew = (a * 3.0 + t).sin() * self.attacks[ring] * 0.03;
                        let x = w * (0.5 + a.cos() * (r + skew));
                        let y = h * (0.5 + a.sin() * (r + skew));
                        page.mark(
                            x,
                            y,
                            LETTERS[(i + ring) % LETTERS.len()] as char,
                            ring as f32 * 0.28 + unit(audio.chroma[i % 12]) * 0.1,
                            0.55 + energy * 0.35,
                        );
                        if i % 6 == 0 {
                            page.mark(x + 2.0, y, ':', 0.7, 0.4 + treble * 0.4);
                        }
                    }
                }
            }
            4 => {
                // One continuous, breathing typographic spiral with a hollow core.
                let count = (area.width as usize * 5).clamp(80, 900);
                for i in 0..count {
                    let u = i as f32 / count as f32;
                    let a = u * TAU * 3.6 - t;
                    let r = 0.065 + u * 0.38;
                    let scallop = 1.0 + (a * 5.0).sin() * (0.035 + mid * 0.06);
                    let glyph = [')', '}', '>', '.', '('][(i / 7) % 5];
                    page.mark(
                        w * (0.5 + a.cos() * r * scallop),
                        h * (0.5 + a.sin() * r * scallop),
                        glyph,
                        u * 0.8,
                        0.35 + u * 0.35 + energy * 0.2,
                    );
                }
            }
            5 => {
                // Horizontal engraved contours with broken strokes and displaced edges.
                for row in 0..12 {
                    for x in 0..area.width {
                        let u = x as f32 / w;
                        let ridge = (u * TAU * 1.1 + t + row as f32 * 0.14).sin();
                        let center = 0.50 + ridge * 0.19;
                        let length = (1.0 - ((row as f32 - 5.5) / 6.0).powi(2)).max(0.0);
                        if (u - center).abs() > 0.39 * length || (x as usize + row * 3) % 19 < 3 {
                            continue;
                        }
                        let notch = unit(bands[row * 2]) * ((u * TAU * 4.0).sin().max(0.0));
                        let y = h * (0.13 + row as f32 * 0.062 + ridge * 0.024 - notch * 0.035);
                        page.mark(
                            x as f32,
                            y,
                            if notch > 0.3 { '=' } else { '_' },
                            row as f32 * 0.06,
                            0.40 + treble * 0.45,
                        );
                    }
                }
            }
            6 => {
                // Broken Score: faint staff and bar lines, then noteheads with
                // stems that grow on attacks and beams that follow the melody.
                let staff_top = 0.26;
                let staff_gap = 0.065;
                let staff_bottom = staff_top + staff_gap * 4.0;
                let left = 1.0;
                let right = w - 2.0;
                for line in 0..5 {
                    let y = h * (staff_top + line as f32 * staff_gap);
                    let mut x = left;
                    while x <= right {
                        page.mark(x, y, '─', 0.55, 0.13 + mid * 0.06);
                        x += 1.0;
                    }
                }
                for bar in [0.08_f32, 0.30, 0.52, 0.74, 0.96] {
                    let x = w * bar;
                    let mut y = h * staff_top;
                    while y <= h * staff_bottom {
                        page.mark(x, y, '│', 0.55, 0.12 + mid * 0.05);
                        y += 1.0;
                    }
                }
                // Precompute notes so beams can sit behind the noteheads.
                type Note = Option<(f32, f32, usize, f32)>;
                let mut notes: [[Note; 5]; 12] = [[None; 5]; 12];
                for (group, row) in notes.iter_mut().enumerate() {
                    let u = 0.11 + group as f32 * 0.065;
                    let x = w * u + (t * 0.35 + group as f32).sin() * 0.8;
                    for (register, note) in row.iter_mut().enumerate() {
                        if (group * 7 + register * 11) % 5 == 0 {
                            continue;
                        }
                        let line = h * (staff_top + register as f32 * staff_gap);
                        let sway = wave(audio.waveform[group * 2]) * h * 0.025;
                        let hit = self.attacks[register % 4];
                        *note = Some((x, line + sway, 2 + (hit * 3.0).round() as usize, hit));
                    }
                }
                for pair in notes.windows(2) {
                    for (previous, current) in pair[0].iter().zip(&pair[1]) {
                        let (Some((px, py, pstem, _)), Some((x, y, stem, hit))) =
                            (*previous, *current)
                        else {
                            continue;
                        };
                        let (x0, y0) = (px, py - pstem as f32);
                        let (x1, y1) = (x, y - stem as f32);
                        let dy = y1 - y0;
                        let glyph = if dy.abs() < 0.5 {
                            '─'
                        } else if dy > 0.0 {
                            '╲'
                        } else {
                            '╱'
                        };
                        let mut bx = x0 + 1.0;
                        while bx < x1 {
                            let by = y0 + dy * (bx - x0) / (x1 - x0).max(1.0);
                            page.mark(bx, by, glyph, 0.5, 0.30 + hit * 0.25);
                            bx += 1.0;
                        }
                    }
                }
                for row in &notes {
                    for (register, note) in row.iter().enumerate() {
                        let Some((x, y, stem, hit)) = *note else {
                            continue;
                        };
                        for step in 1..=stem {
                            page.mark(
                                x,
                                y - step as f32,
                                '│',
                                register as f32 * 0.12,
                                0.38 + hit * 0.30,
                            );
                        }
                    }
                }
                for (group, row) in notes.iter().enumerate() {
                    for (register, note) in row.iter().enumerate() {
                        let Some((x, y, _, hit)) = *note else {
                            continue;
                        };
                        page.mark(
                            x,
                            y,
                            if (group * 7 + register * 11) % 2 == 0 {
                                '●'
                            } else {
                                '○'
                            },
                            register as f32 * 0.12 + group as f32 * 0.02,
                            0.55 + hit * 0.35,
                        );
                    }
                }
            }
            7 => {
                // Nested offset block islands, not a rectangular equalizer.
                const BLOCKS: [char; 4] = ['░', '▒', '▓', '█'];
                let islands: [(f32, f32, f32, f32); 7] = std::array::from_fn(|island| {
                    let a = island as f32 * 2.399;
                    (
                        0.5 + a.cos() * 0.28,
                        0.5 + a.sin() * 0.27,
                        0.075 + unit(bands[island * 4]) * 0.045,
                        0.09 + self.attacks[island % 4] * 0.035,
                    )
                });
                for y in 0..area.height {
                    for x in 0..area.width {
                        let u = x as f32 / w;
                        let v = y as f32 / h;
                        let mut ink = None;
                        for (island, &(cx, cy, sx, sy)) in islands.iter().enumerate() {
                            let d = ((u - cx).abs() / sx).max((v - cy).abs() / sy);
                            if d < 1.0 {
                                let layer = ((1.0 - d) * 4.0) as usize;
                                ink = Some((BLOCKS[layer.min(3)], island as f32 * 0.13));
                            }
                        }
                        if let Some((ch, color)) = ink {
                            page.mark(x as f32, y as f32, ch, color, 0.5 + energy * 0.4);
                        }
                    }
                }
            }
            8 => {
                // Two vertical strands exchange foreground depth and carry short
                // letterform rungs only at the front half of each turn.
                for y in 0..area.height {
                    let v = y as f32 / h;
                    if !(0.06..0.94).contains(&v) {
                        continue;
                    }
                    let a = v * TAU * 2.2 + t;
                    let spread = w * (0.19 + bass * 0.045);
                    let center = w * 0.5 + (v * TAU + t * 0.3).sin() * w * 0.07;
                    let left = center + a.sin() * spread;
                    let right = center - a.sin() * spread;
                    page.mark(
                        left,
                        y as f32,
                        if a.cos() > 0.0 { 'S' } else { ':' },
                        v * 0.5,
                        0.42 + (a.cos() + 1.0) * 0.2,
                    );
                    page.mark(
                        right,
                        y as f32,
                        if a.cos() < 0.0 { 'Z' } else { ':' },
                        0.55 + v * 0.3,
                        0.42 + (1.0 - a.cos()) * 0.2,
                    );
                    if y % 3 == 0 && a.cos().abs() > 0.35 {
                        let lo = left.min(right) as i32;
                        let hi = left.max(right) as i32;
                        for x in (lo..hi).step_by(3) {
                            page.mark(
                                x as f32,
                                y as f32,
                                if y % 2 == 0 { '-' } else { '=' },
                                v,
                                0.28 + mid * 0.28,
                            );
                        }
                    }
                }
            }
            _ => {
                // Two curving hatching currents intersect through a central lens.
                for y in 0..area.height {
                    for x in 0..area.width {
                        let u = x as f32 / w - 0.5;
                        let v = y as f32 / h - 0.5;
                        if u * u / 0.22 + v * v / 0.19 > 1.0 {
                            continue;
                        }
                        let bend = (u * 7.0 + t).sin() * (2.0 + bass * 3.0);
                        let a = (x as f32 * 0.5 + y as f32 + bend).rem_euclid(6.0);
                        let b =
                            (x as f32 * 0.5 - y as f32 + (v * 8.0 - t).sin() * 3.0).rem_euclid(7.0);
                        let first = a < 0.9 + unit(audio.flatness) * 0.3;
                        let second = b < 0.9;
                        let glyph = match (first, second) {
                            (true, true) => '×',
                            (true, false) => '/',
                            (false, true) => '\\',
                            _ => continue,
                        };
                        page.mark(
                            x as f32,
                            y as f32,
                            glyph,
                            0.35 + u * 0.5 + unit(audio.centroid) * 0.2,
                            0.4 + treble * 0.45,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(style: usize, audio: &Reactivity, bands: &[f32; 32]) -> Buffer {
        let area = Rect::new(0, 0, 80, 28);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..12 {
            renderer.draw(
                &mut buf,
                area,
                frame as f32 / 60.0,
                style,
                0,
                bands,
                audio,
                &Theme::default(),
            );
        }
        buf
    }

    #[test]
    fn quiet_compositions_are_nonempty_and_pairwise_distinct() {
        let images: Vec<_> = (0..NAMES.len())
            .map(|style| image(style, &Reactivity::default(), &[0.0; 32]))
            .collect();
        for (i, img) in images.iter().enumerate() {
            assert!(
                img.content.iter().any(|cell| cell.symbol() != " "),
                "{}",
                NAMES[i]
            );
            for other in &images[..i] {
                assert_ne!(img, other);
            }
        }
        assert!(images[2].content.iter().any(|cell| cell
            .symbol()
            .chars()
            .any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch))));
    }

    #[test]
    fn audio_changes_character_geometry_in_every_mode() {
        let loud = Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            waveform: [0.8; 32],
            transients: [1.0; 4],
            flatness: 1.0,
            ..Reactivity::default()
        };
        for (style, name) in NAMES.iter().enumerate() {
            let quiet = image(style, &Reactivity::default(), &[0.0; 32]);
            let active = image(style, &loud, &[1.0; 32]);
            let symbols = |buf: Buffer| {
                buf.content
                    .iter()
                    .map(|c| c.symbol().to_owned())
                    .collect::<Vec<_>>()
            };
            assert_ne!(symbols(quiet), symbols(active), "{}", name);
        }
    }

    #[test]
    fn clips_offset_tiny_and_disjoint_areas_and_survives_nonfinite_input() {
        let bounds = Rect::new(7, 4, 19, 11);
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(0, 0, 14, 9),
                Rect::new(10, 6, 1, 1),
                Rect::new(80, 90, 5, 8),
                Rect::new(7, 4, 0, 0),
            ] {
                let mut buf = Buffer::empty(bounds);
                for cell in &mut buf.content {
                    cell.set_char('?');
                }
                let original = buf.clone();
                let mut renderer = Renderer::default();
                let audio = Reactivity {
                    bass: f32::NAN,
                    mid: f32::INFINITY,
                    waveform: [f32::NAN; 32],
                    ..Reactivity::default()
                };
                for time in [0.0, f32::NAN, 10000.0, -3.0] {
                    renderer.draw(
                        &mut buf,
                        area,
                        time,
                        style,
                        0,
                        &[f32::NAN; 32],
                        &audio,
                        &Theme::default(),
                    );
                }
                let clip = area.intersection(bounds);
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        if !clip.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)], original[(x, y)]);
                        }
                    }
                }
                assert!(renderer.phase.is_finite());
                assert!(renderer.levels.iter().all(|v| v.is_finite()));
                assert!(renderer.phase < 0.2);
            }
        }
    }

    #[test]
    fn helix_is_continuous_across_phase_wrap() {
        let area = Rect::new(0, 0, 120, 40);
        let render = |phase| {
            let mut renderer = Renderer {
                phase,
                last_time: Some(0.0),
                ..Renderer::default()
            };
            let mut buf = Buffer::empty(area);
            renderer.draw(
                &mut buf,
                area,
                0.0,
                8,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            buf
        };
        let before = render(10.0 * TAU - 0.0001);
        let after = render(0.0001);
        let changed = before
            .content
            .iter()
            .zip(&after.content)
            .filter(|(a, b)| a.symbol() != b.symbol())
            .count();
        assert!(changed <= 12, "phase wrap displaced {changed} helix cells");
    }

    fn painted(audio: &Reactivity) -> Buffer {
        let area = Rect::new(0, 0, 80, 28);
        let mut buf = Buffer::empty(area);
        let mut renderer = Renderer::default();
        for frame in 0..60 {
            renderer.draw(
                &mut buf,
                area,
                frame as f32 / 60.0,
                6,
                0,
                &[0.0; 32],
                audio,
                &Theme::default(),
            );
        }
        buf
    }

    fn glyph_counts(buf: &Buffer) -> std::collections::HashMap<char, usize> {
        let mut counts = std::collections::HashMap::new();
        for cell in &buf.content {
            if let Some(ch) = cell.symbol().chars().next() {
                if ch != ' ' {
                    *counts.entry(ch).or_default() += 1;
                }
            }
        }
        counts
    }

    fn symbols(buf: &Buffer) -> Vec<String> {
        buf.content
            .iter()
            .map(|cell| cell.symbol().to_owned())
            .collect()
    }

    #[test]
    fn broken_score_draws_staff_stems_and_beams() {
        let quiet = Reactivity::default();
        let buf = painted(&quiet);
        let counts = glyph_counts(&buf);
        let ink: usize = counts.values().sum();
        let coverage = ink as f32 / (80 * 28) as f32;
        assert!(
            (0.15..0.50).contains(&coverage),
            "score ink coverage {coverage}"
        );
        assert!(
            counts.get(&'─').copied().unwrap_or(0) > 200,
            "staff and beams are too thin"
        );
        assert!(
            counts.get(&'│').copied().unwrap_or(0) > 50,
            "stems are too short"
        );
        let heads = counts.get(&'●').copied().unwrap_or(0) + counts.get(&'○').copied().unwrap_or(0);
        assert!(heads >= 40, "only {heads} noteheads");
        for punctuation in [';', ':', ',', '!'] {
            assert_eq!(
                counts.get(&punctuation),
                None,
                "score still uses {punctuation}"
            );
        }
        let hit = Reactivity {
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        let struck = painted(&hit);
        let struck_counts = glyph_counts(&struck);
        assert!(
            struck_counts.get(&'│').copied().unwrap_or(0) > counts.get(&'│').copied().unwrap_or(0),
            "attacks do not lengthen stems"
        );
        let moved = symbols(&buf)
            .iter()
            .zip(symbols(&struck))
            .filter(|(a, b)| a.as_str() != b.as_str())
            .count();
        assert!(moved > 100, "attacks moved only {moved} score cells");
        assert_eq!(symbols(&buf), symbols(&painted(&quiet)));
    }

    #[test]
    #[ignore = "release timing probe"]
    fn release_frame_budget() {
        let area = Rect::new(0, 0, 160, 50);
        let mut buf = Buffer::empty(area);
        let theme = Theme::default();
        let audio = Reactivity::default();
        for (style, name) in NAMES.iter().enumerate() {
            let mut renderer = Renderer::default();
            let start = std::time::Instant::now();
            for frame in 0..100 {
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.4; 32],
                    &audio,
                    &theme,
                );
            }
            let ms = start.elapsed().as_secs_f64() * 10.0;
            eprintln!("{}: {:.3} ms/frame", name, ms);
            assert!(ms < 16.0);
        }
    }
}
