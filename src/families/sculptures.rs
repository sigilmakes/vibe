use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::{collections::BTreeMap, f32::consts::TAU};

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    rotation: f32,
    previous_strike: [f32; 4],
    /// One decaying attack envelope per transient band: low, low-mid,
    /// presence, air.
    strike: [f32; 4],
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let time = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1));
        self.last_time = Some(time);
        self.rotation = (self.rotation + dt * (0.24 + unit(audio.mid) * 0.18)).rem_euclid(TAU);
        // Each band keeps its own envelope. Folding them into one maximum let a
        // hat drive the mass that only a kick should move.
        let attacks = [
            unit(audio.kick).max(unit(audio.transients[0])),
            unit(audio.transients[1]),
            unit(audio.transients[2]),
            unit(audio.transients[3]),
        ];
        for (channel, attack) in attacks.into_iter().enumerate() {
            let rise = (attack - self.previous_strike[channel]).max(0.0);
            self.strike[channel] = (self.strike[channel] - dt / 0.18).max(0.0).max(rise);
            self.previous_strike[channel] = attack;
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
        if visible.is_empty() {
            return;
        }
        let [r, g, b] = theme.background;
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)].reset();
                buf[(x, y)].set_char(' ').set_bg(Color::Rgb(r, g, b));
            }
        }
        if style.is_multiple_of(2) {
            self.wire(buf, area, visible, palette, bands, audio, theme);
        } else {
            self.blocks(buf, area, visible, palette, bands, audio, theme);
        }
    }

    // A closed trefoil with four longitudinal rails and transverse ribs. Each
    // transient band owns one part of it, so a low-mid attack cannot stand in
    // for a kick.
    fn sculpture(&self, bands: &[f32; 32], audio: &Reactivity) -> [[[f32; 3]; 4]; 72] {
        let (s, c) = self.rotation.sin_cos();
        let (ts, tc) = (0.48 + self.rotation.sin() * 0.52).sin_cos();
        let [low, body, presence, air] = self.strike;
        std::array::from_fn(|i| {
            let u = i as f32 / 72.0 * TAU;
            let r = 0.53 + 0.18 * (3.0 * u).cos();
            // Low attacks swell the whole knot; nothing above the bass moves it.
            let bulge = 1.0 + unit(audio.bass) * 0.18 + low * 0.12;
            std::array::from_fn(|rail| {
                // Presence attacks fan the four rails apart along the knot.
                let a = rail as f32 * TAU / 4.0 + u + presence * 0.22;
                // Low-mid attacks thicken and deepen the transverse ribs.
                let rib = 0.055
                    + unit(bands[i * 32 / 72]) * 0.035
                    + unit(audio.treble) * 0.018
                    + body * 0.030;
                // Air attacks fray the seams between ribs.
                let fracture = if i % 6 == 0 { air * 0.075 } else { 0.0 };
                let x = (r + rib * a.cos() + fracture) * (2.0 * u).cos() * bulge;
                let y = (r + rib * a.cos() + fracture) * (2.0 * u).sin() * bulge;
                let z = 0.25 * (3.0 * u).sin()
                    + rib * a.sin() * (1.0 + body * 0.5)
                    + low * 0.07 * (5.0 * u).cos();
                let rx = x * c + z * s;
                let rz = z * c - x * s;
                [rx, y * tc - rz * ts, y * ts + rz * tc]
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn wire(
        &self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &Theme,
    ) {
        let vertices = self.sculpture(bands, audio);
        // Project into an abstract unit space first, then fit that box to the
        // whole frame. One terminal row is twice a column, so a screen row uses
        // half the horizontal scale; keeping both equal would stretch the knot
        // vertically and crop its top and bottom.
        let project_raw = |p: [f32; 3]| {
            let perspective = 2.7 / (2.7 - p[2]).max(0.65);
            [p[0] * perspective, p[1] * perspective, p[2]]
        };
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for row in &vertices {
            for vertex in row {
                let p = project_raw(*vertex);
                lo[0] = lo[0].min(p[0]);
                lo[1] = lo[1].min(p[1]);
                hi[0] = hi[0].max(p[0]);
                hi[1] = hi[1].max(p[1]);
            }
        }
        let span = [(hi[0] - lo[0]).max(0.001), (hi[1] - lo[1]).max(0.001)];
        let scale =
            (area.width as f32 * 0.90 / span[0]).min(area.height as f32 * 0.90 * 2.0 / span[1]);
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        // Subpixel coordinates: two across a cell and four down.
        let project = |p: [f32; 3]| {
            let p = project_raw(p);
            [
                area.width as f32 + (p[0] - center[0]) * scale * 2.0,
                area.height as f32 * 2.0 + (p[1] - center[1]) * scale * 2.0,
                p[2],
            ]
        };
        let mut dots = BTreeMap::<(u16, u16), (u8, f32, f32)>::new();
        let mut line = |a: [f32; 3], b: [f32; 3], phase: f32| {
            let a = project(a);
            let b = project(b);
            let steps =
                ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).clamp(1, 2048);
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                let sx = (a[0] + (b[0] - a[0]) * t).round() as i32;
                let sy = (a[1] + (b[1] - a[1]) * t).round() as i32;
                if sx < 0 || sy < 0 || sx >= area.width as i32 * 2 || sy >= area.height as i32 * 4 {
                    continue;
                }
                let x = area.x.saturating_add((sx / 2) as u16);
                let y = area.y.saturating_add((sy / 4) as u16);
                if !visible.contains((x, y).into()) {
                    continue;
                }
                let bit = [[0, 1, 2, 6], [3, 4, 5, 7]][(sx % 2) as usize][(sy % 4) as usize];
                let depth = a[2] + (b[2] - a[2]) * t;
                let entry = dots.entry((x, y)).or_insert((0, -10.0, phase));
                entry.0 |= 1 << bit;
                if depth > entry.1 {
                    entry.1 = depth;
                    entry.2 = phase;
                }
            }
        };
        for i in 0..72 {
            for rail in 0..4 {
                // Air attacks tear small seams without destroying the knot.
                if !(self.strike[3] > 0.4 && i % 12 == 3 && rail % 2 == 0) {
                    line(
                        vertices[i][rail],
                        vertices[(i + 1) % 72][rail],
                        0.52 + rail as f32 * 0.08,
                    );
                }
                if i % 4 == 0 {
                    line(vertices[i][rail], vertices[i][(rail + 1) % 4], 0.12);
                }
            }
        }
        for ((x, y), (mask, depth, phase)) in dots {
            buf[(x, y)]
                .set_char(char::from_u32(0x2800 + mask as u32).unwrap())
                .set_fg(theme.sample(palette, phase, (0.57 + depth * 0.38).clamp(0.18, 0.93)));
        }
    }

    fn composition(&self, bands: &[f32; 32], audio: &Reactivity) -> [[f32; 4]; 7] {
        let bass = unit(audio.bass);
        let mid = unit(audio.mid);
        let high = unit(audio.treble);
        let [low, body, presence, air] = self.strike;
        let split = 0.48 + bass * 0.12 - mid * 0.08 + body * 0.03;
        let lift = low * 0.055;
        let seam = 0.035 + presence * 0.02;
        let cross = 0.39 + mid * 0.15;
        [
            [0.10 - lift, 0.14 + lift, split - 0.10, 0.52 + bass * 0.08],
            [split + seam, 0.14, 0.86 - split - seam, cross - 0.14],
            [
                split + seam + high * 0.035,
                cross + seam,
                0.19 + unit(bands[17]) * 0.08,
                0.29 - mid * 0.08,
            ],
            [0.10, 0.73 + lift, 0.42 + unit(bands[5]) * 0.16, 0.09],
            [0.73 - lift, 0.73 - lift, 0.13 + air * 0.03, 0.13],
            [0.055, 0.14, 0.018, 0.31 + mid * 0.16],
            [0.73, cross + seam, 0.13, 0.045 + air * 0.05],
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn blocks(
        &self,
        buf: &mut Buffer,
        area: Rect,
        visible: Rect,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &Theme,
    ) {
        for (i, [x, y, w, h]) in self.composition(bands, audio).into_iter().enumerate() {
            let left = (x * area.width as f32).round() as u16;
            let top = (y * area.height as f32).round() as u16;
            let right = ((x + w) * area.width as f32).round() as u16;
            let bottom = ((y + h) * area.height as f32).round() as u16;
            let tile = Rect::new(
                area.x.saturating_add(left),
                area.y.saturating_add(top),
                right.saturating_sub(left),
                bottom.saturating_sub(top),
            )
            .intersection(visible);
            let phase = [0.03, 0.61, 0.29, 0.88, 0.15, 0.45, 0.74][i];
            let pigment = theme.sample(palette, phase, [0.86, 0.65, 0.81, 0.7, 0.92, 0.5, 0.58][i]);
            for row in tile.y..tile.bottom() {
                for col in tile.x..tile.right() {
                    let mark = if i == 5 {
                        '▐'
                    } else if i == 6 {
                        '▀'
                    } else {
                        '█'
                    };
                    buf[(col, row)].set_char(mark).set_fg(pigment);
                }
            }
            // A cut-out inside the main mass, rather than an all-over tile grid.
            if i == 0 {
                let cut = Rect::new(
                    area.x.saturating_add(left + (right - left) / 3),
                    area.y.saturating_add(top + (bottom - top) / 3),
                    (right - left) / 3,
                    (bottom - top) / 3,
                )
                .intersection(visible);
                for row in cut.y..cut.bottom() {
                    for col in cut.x..cut.right() {
                        buf[(col, row)].set_char(' ');
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_changes_vertices_and_tile_layout() {
        let renderer = Renderer::default();
        let quiet = Reactivity::default();
        let loud = Reactivity {
            bass: 0.9,
            mid: 0.8,
            treble: 0.9,
            ..quiet
        };
        assert_ne!(
            renderer.sculpture(&[0.0; 32], &quiet),
            renderer.sculpture(&[0.8; 32], &loud)
        );
        assert_ne!(
            renderer.composition(&[0.0; 32], &quiet),
            renderer.composition(&[0.8; 32], &loud)
        );
    }
    #[test]
    fn tiny_offset_and_partial_clipping() {
        for style in 0..2 {
            for area in [
                Rect::new(8, 6, 0, 0),
                Rect::new(8, 6, 1, 1),
                Rect::new(8, 6, 30, 20),
            ] {
                let bounds = Rect::new(5, 3, 12, 9);
                let mut buf = Buffer::empty(bounds);
                for cell in &mut buf.content {
                    cell.set_char('!');
                }
                Renderer::default().draw(
                    &mut buf,
                    area,
                    0.0,
                    style,
                    0,
                    &[0.0; 32],
                    &Reactivity::default(),
                    &Theme::default(),
                );
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        if !area.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "!");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn modes_have_distinct_marks_and_negative_space() {
        let area = Rect::new(0, 0, 100, 32);
        for style in 0..2 {
            let mut buf = Buffer::empty(area);
            Renderer::default().draw(
                &mut buf,
                area,
                0.0,
                style,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &Theme::default(),
            );
            let marks: Vec<char> = buf
                .content
                .iter()
                .filter_map(|cell| cell.symbol().chars().next())
                .collect();
            assert!(marks.iter().filter(|&&c| c == ' ').count() > 1200);
            if style == 0 {
                assert!(marks
                    .iter()
                    .any(|&c| ('\u{2801}'..='\u{28ff}').contains(&c)));
                assert!(!marks.contains(&'█'));
            } else {
                assert!(marks.contains(&'█'));
                assert!(!marks
                    .iter()
                    .any(|&c| ('\u{2801}'..='\u{28ff}').contains(&c)));
            }
        }
    }
    #[test]
    fn wire_sculpture_fills_the_frame_without_cropping() {
        let theme = Theme::default();
        let audio = Reactivity {
            bass: 0.5,
            mid: 0.4,
            treble: 0.4,
            ..Reactivity::default()
        };
        for (width, height) in [(120, 40), (40, 40)] {
            let area = Rect::new(0, 0, width, height);
            let mut renderer = Renderer::default();
            let mut lo = [width, height];
            let mut hi = [0, 0];
            for frame in 0..120 {
                let mut buf = Buffer::empty(area);
                renderer.draw(
                    &mut buf,
                    area,
                    frame as f32 / 60.0,
                    0,
                    0,
                    &[0.4; 32],
                    &audio,
                    &theme,
                );
                for y in 0..height {
                    for x in 0..width {
                        if buf[(x, y)].symbol() != " " {
                            lo[0] = lo[0].min(x);
                            lo[1] = lo[1].min(y);
                            hi[0] = hi[0].max(x);
                            hi[1] = hi[1].max(y);
                        }
                    }
                }
            }
            let used = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1];
            assert!(
                lo[0] >= 1 && lo[1] >= 1 && hi[0] < width && hi[1] < height - 1,
                "wire sculpture must keep a margin at {width}x{height}: lo={lo:?} hi={hi:?}"
            );
            // The knot is round in physical space, so it fills whichever
            // dimension is shorter: the height on wide terminals, the width on
            // tall ones. Filling both would stretch it.
            let fill = (used[0] as f32 / width as f32).max(used[1] as f32 / height as f32);
            assert!(
                fill > 0.85,
                "wire sculpture must fill its limiting dimension at {width}x{height}: {used:?}"
            );
            let physical = used[0] as f32 / (used[1] as f32 * 2.0);
            assert!(
                (0.8..1.25).contains(&physical),
                "wire sculpture must keep its physical aspect at {width}x{height}: {physical}"
            );
        }
    }

    /// One frame after a single attacked band, with everything else quiet.
    fn struck(transients: [f32; 4]) -> Renderer {
        let mut renderer = Renderer::default();
        renderer.advance(0.0, &Reactivity::default());
        renderer.advance(
            1.0 / 60.0,
            &Reactivity {
                transients,
                ..Reactivity::default()
            },
        );
        renderer
    }

    #[test]
    fn strike_envelopes_decay_per_channel() {
        let mut renderer = Renderer::default();
        renderer.advance(0.0, &Reactivity::default());
        renderer.advance(
            0.1,
            &Reactivity {
                kick: 0.9,
                transients: [0.0, 0.6, 0.3, 1.0],
                ..Reactivity::default()
            },
        );
        assert_eq!(renderer.strike, [0.9, 0.6, 0.3, 1.0]);
        renderer.advance(0.2, &Reactivity::default());
        renderer.advance(0.3, &Reactivity::default());
        assert_eq!(renderer.strike, [0.0; 4]);
    }

    #[test]
    fn each_transient_band_moves_its_own_geometry() {
        let quiet = struck([0.0; 4]).sculpture(&[0.4; 32], &Reactivity::default());
        let shapes: Vec<_> = (0..4)
            .map(|channel| {
                let mut transients = [0.0; 4];
                transients[channel] = 1.0;
                struck(transients).sculpture(&[0.4; 32], &Reactivity::default())
            })
            .collect();
        for (channel, shape) in shapes.iter().enumerate() {
            assert_ne!(
                *shape, quiet,
                "transient band {channel} left the sculpture unchanged"
            );
            for (other, rival) in shapes.iter().enumerate().skip(channel + 1) {
                assert_ne!(
                    shape, rival,
                    "transient bands {channel} and {other} share one geometry"
                );
            }
        }
    }

    #[test]
    fn hi_hat_attacks_do_not_move_the_bass_mass() {
        let quiet = struck([0.0; 4]).sculpture(&[0.4; 32], &Reactivity::default());
        let hat = struck([0.0, 0.0, 0.0, 1.0]).sculpture(&[0.4; 32], &Reactivity::default());
        let kick = struck([1.0, 0.0, 0.0, 0.0]).sculpture(&[0.4; 32], &Reactivity::default());
        // Air only frays the seams every sixth sample; the rest of the knot
        // must stay exactly where a quiet frame left it.
        for (i, (air, rest)) in hat.iter().zip(&quiet).enumerate() {
            if !i.is_multiple_of(6) {
                assert_eq!(air, rest, "an air attack displaced vertex {i}");
            }
        }
        assert!(
            kick.iter().zip(&quiet).all(|(loud, rest)| loud != rest),
            "a low attack must swell the whole knot"
        );
    }

    #[test]
    fn tile_layout_answers_each_band_separately() {
        let bands = [0.4; 32];
        let quiet = struck([0.0; 4]).composition(&bands, &Reactivity::default());
        let layouts: Vec<_> = (0..4)
            .map(|channel| {
                let mut transients = [0.0; 4];
                transients[channel] = 1.0;
                struck(transients).composition(&bands, &Reactivity::default())
            })
            .collect();
        for (channel, layout) in layouts.iter().enumerate() {
            assert_ne!(
                *layout, quiet,
                "transient band {channel} left the tiles unchanged"
            );
        }
    }

    #[test]
    fn strike_geometry_is_deterministic_and_bounded() {
        for transients in [[0.0; 4], [1.0; 4], [0.0, 0.0, 0.0, 1.0]] {
            let first = struck(transients).sculpture(&[0.8; 32], &Reactivity::default());
            let second = struck(transients).sculpture(&[0.8; 32], &Reactivity::default());
            assert_eq!(first, second);
            for vertex in first.iter().flatten() {
                for axis in vertex {
                    assert!(
                        axis.is_finite() && axis.abs() < 2.0,
                        "vertex {axis} escaped"
                    );
                }
            }
        }
    }
}
