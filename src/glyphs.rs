use std::f32::consts::PI;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{palette::Theme, visuals::Reactivity};

const HISTORY_LIMIT: usize = 32;
const MARK_LIFETIME: f32 = 1.15;
const BRAILLE: [[u8; 4]; 2] = [[1, 2, 4, 64], [8, 16, 32, 128]];

#[derive(Clone, Copy)]
struct Mark {
    age: f32,
    strength: f32,
    channel: usize,
    column: usize,
}

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    phase: f32,
    envelopes: [f32; 4],
    previous: [f32; 4],
    cooldown: [f32; 4],
    column: usize,
    marks: Vec<Mark>,
}

fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        let levels = [audio.bass, audio.mid, audio.treble, audio.energy];
        let smoothing = 1.0 - (-dt * 7.0).exp();
        for (envelope, level) in self.envelopes.iter_mut().zip(levels) {
            *envelope += (unit(level) - *envelope) * smoothing;
        }
        // Fractional 0.8/1.3/1.7 oscillators close after ten turns.
        self.phase = (self.phase + dt * (0.65 + 0.9 * self.envelopes[1])) % (20.0 * PI);
        for mark in &mut self.marks {
            mark.age += dt;
        }
        self.marks.retain(|mark| mark.age < MARK_LIFETIME);
        let mut advanced = false;
        for channel in 0..4 {
            self.cooldown[channel] = (self.cooldown[channel] - dt).max(0.0);
            let fallback = match channel {
                0 => audio.kick,
                1 => audio.onset * 0.8,
                2 => audio.novelty,
                _ => audio.onset * audio.treble,
            };
            let attack = unit(audio.transients[channel]).max(unit(fallback));
            if attack > 0.16
                && attack - self.previous[channel] > 0.09
                && self.cooldown[channel] == 0.0
            {
                if !advanced {
                    self.column = (self.column + 1) % 8;
                    advanced = true;
                }
                if self.marks.len() == HISTORY_LIMIT {
                    self.marks.remove(0);
                }
                self.marks.push(Mark {
                    age: 0.0,
                    strength: attack,
                    channel,
                    column: self.column,
                });
                self.cooldown[channel] = 0.055;
            }
            self.previous[channel] = attack;
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
        let clip = area.intersection(buf.area);
        if clip.is_empty() {
            return;
        }
        let bg = Color::Rgb(
            theme.background[0],
            theme.background[1],
            theme.background[2],
        );
        for y in clip.y..clip.bottom() {
            for x in clip.x..clip.right() {
                buf[(x, y)].set_char(' ').set_bg(bg);
            }
        }
        if style.is_multiple_of(2) {
            self.percussion(buf, area, clip, palette, theme);
        } else {
            self.lace(buf, area, clip, palette, bands, theme);
        }
    }

    fn percussion(&self, buf: &mut Buffer, area: Rect, clip: Rect, palette: usize, theme: &Theme) {
        // Attacks share rhythmic anchors, but compose across the page instead of
        // occupying fixed voice rows. Age drives expansion and recoil, not noise.
        const HEIGHTS: [f32; 8] = [0.52, 0.36, 0.63, 0.43, 0.58, 0.32, 0.65, 0.46];
        const PIGMENTS: [f32; 4] = [0.16, 0.56, 0.82, 0.34];
        for mark in &self.marks {
            let life = mark.age / MARK_LIFETIME;
            let fade = (1.0 - life).powf(1.5);
            if fade < 0.055 {
                continue;
            }
            let release = 1.0 - (-mark.age * 9.0).exp();
            let direction = if mark.column.is_multiple_of(2) { 1 } else { -1 };
            let x = ((0.16 + mark.column as f32 / 7.0 * 0.68) * area.width as f32) as i32;
            let voice_offset = [0.0, -0.11, 0.12, -0.19][mark.channel];
            let y = ((HEIGHTS[mark.column] + voice_offset) * area.height as f32) as i32;
            let width = (area.width as f32 * (0.025 + mark.strength * 0.055)).max(2.0);
            let height = (area.height as f32 * (0.035 + mark.strength * 0.055)).max(1.0);
            let color = theme.sample(
                palette,
                PIGMENTS[mark.channel],
                (0.65 + 0.35 * mark.strength) * fade,
            );
            let mut put = |dx: i32, dy: i32, glyph: char| {
                let px = area.x as i32 + x + dx;
                let py = area.y as i32 + y + dy;
                if px >= clip.x as i32
                    && px < clip.right() as i32
                    && py >= clip.y as i32
                    && py < clip.bottom() as i32
                {
                    buf[(px as u16, py as u16)].set_char(glyph).set_fg(color);
                }
            };
            match mark.channel {
                0 => {
                    // A heavy bracket stamp opens outward; its broken edges keep
                    // the center and the surrounding page dark.
                    let w = (width * (0.55 + release * 1.2)) as i32;
                    let h = (height * (0.55 + release * 1.0)).max(1.0) as i32;
                    for dx in 1..=w.min(4) {
                        put(-w + dx, -h, '━');
                        put(w - dx, h, '━');
                    }
                    put(-w, -h, '┏');
                    put(w, -h, '┓');
                    put(-w, h, '┗');
                    put(w, h, '┛');
                    for dy in 0..h.min(3) {
                        put(-w, -h + 1 + dy, '┃');
                        put(w, h - 1 - dy, '┃');
                    }
                    if mark.age < 0.12 {
                        put(0, 0, '▪');
                        put(-2, 0, '╸');
                        put(2, 0, '╺');
                    }
                }
                1 => {
                    // Snare: a sheared rule with a traveling cut and a short
                    // parallel echo, like a compositor's angular strike-through.
                    let reach = (width * (0.8 + release * 0.9)) as i32;
                    let sweep = (release * width * 1.6) as i32 * direction;
                    for dx in -reach..=reach {
                        let dy = (dx as f32 / width * 0.7).round() as i32 * direction;
                        if (dx - sweep).abs() > 1 {
                            put(dx, dy, '━');
                        }
                    }
                    put(sweep, -1, '╱');
                    put(sweep, 0, '╋');
                    put(sweep, 1, '╱');
                    for dx in 0..reach / 2 {
                        put(-direction * dx, 2, '─');
                    }
                }
                2 => {
                    // Three ordered hatching strokes splay apart on the attack.
                    let spread = (release * width * 0.65) as i32;
                    let length = (height * (0.8 + mark.strength)).max(2.0) as i32;
                    for ray in -1i32..=1 {
                        for step in 0..length {
                            let dx = direction * (step + spread + ray * (2 + spread));
                            let dy = -step + ray;
                            put(dx, dy, if direction > 0 { '╱' } else { '╲' });
                        }
                    }
                }
                _ => {
                    // A sparse, symmetric punctuation fan, never random debris.
                    let spread = (width * (0.3 + release * 1.2)) as i32;
                    put(0, 0, '⁝');
                    for ray in 1..=3 {
                        let dx = (spread * ray / 3).max(ray);
                        let dy = -((release * height) as i32 + ray % 2);
                        put(dx, dy, if ray == 3 { '˙' } else { '·' });
                        put(-dx, dy, if ray == 3 { '˙' } else { '·' });
                    }
                }
            }
        }
    }

    fn lace(
        &self,
        buf: &mut Buffer,
        area: Rect,
        clip: Rect,
        palette: usize,
        bands: &[f32; 32],
        theme: &Theme,
    ) {
        let [bass, mid, treble, energy] = self.envelopes;
        let low = bands[..10].iter().map(|v| unit(*v)).sum::<f32>() / 10.0;
        let high = bands[20..].iter().map(|v| unit(*v)).sum::<f32>() / 12.0;
        // Keep the plate's nodal skeleton, but let its intersections breathe
        // and shear. Recent attacks send bounded local ripples through it.
        let mix = 0.22 + mid * 0.52 + low * 0.2 + self.phase.sin() * 0.09;
        let amplitude = 0.08 + bass * 0.14;
        let threshold = 0.035 + energy * 0.025;
        let angle = self.phase.sin() * (0.07 + mid * 0.08);
        let (sin, cos) = angle.sin_cos();
        let pulses: [Option<(f32, f32, f32, f32)>; 4] = std::array::from_fn(|channel| {
            self.marks
                .iter()
                .rev()
                .find(|mark| mark.channel == channel && mark.age < 0.65)
                .map(|mark| {
                    let angle = mark.column as f32 * PI / 4.0 + channel as f32 * 0.55;
                    (
                        angle.cos() * 0.35,
                        angle.sin() * 0.35,
                        mark.age,
                        mark.strength,
                    )
                })
        });
        for y in clip.y..clip.bottom() {
            for x in clip.x..clip.right() {
                let mut dots = 0u8;
                let mut brightness = 0.0f32;
                for (dx, column) in BRAILLE.iter().enumerate() {
                    for (dy, bit) in column.iter().enumerate() {
                        let u = ((x - area.x) as f32 + (dx as f32 + 0.5) / 2.0) / area.width as f32
                            * 2.0
                            - 1.0;
                        let v = ((y - area.y) as f32 + (dy as f32 + 0.5) / 4.0)
                            / area.height as f32
                            * 2.0
                            - 1.0;
                        let radius = (u * u + v * v).sqrt();
                        if radius > 0.91 {
                            continue;
                        }
                        let ru = u * cos - v * sin;
                        let rv = u * sin + v * cos;
                        let mut ripple = 0.0;
                        let mut glint = 0.0f32;
                        for &(cx, cy, age, strength) in pulses.iter().flatten() {
                            let distance = (ru - cx).hypot(rv - cy);
                            let front = distance - (0.10 + age * 1.5);
                            let envelope = (-front * front * 110.0).exp()
                                * (1.0 - age / 0.65).max(0.0)
                                * strength;
                            ripple += envelope * (front * 21.0 + age * 9.0).cos() * 0.075;
                            glint = glint.max(envelope);
                        }
                        let a = PI
                            * (ru
                                + amplitude * (rv * PI + self.phase).sin()
                                + ripple
                                + (self.phase * 1.7 + rv * 4.0).sin() * 0.018);
                        let b = PI
                            * (rv + amplitude * (ru * PI - self.phase * 0.8).sin() - ripple * 0.7
                                + (self.phase * 1.3 - ru * 4.0).cos() * 0.018);
                        let field = (a * 3.0).sin() * (b * 2.0).sin()
                            - mix * (a * 2.0).sin() * (b * 3.0).sin()
                            + (treble + high) * 0.18 * (a * 5.0).sin() * (b * 4.0).sin();
                        let edge = ((0.91 - radius) * 15.0).clamp(0.0, 1.0);
                        if field.abs() < threshold * edge {
                            dots |= bit;
                            brightness = brightness
                                .max((0.36 + energy * 0.45 + glint * 0.18).min(1.0) * edge);
                        }
                    }
                }
                if dots != 0 {
                    let glyph = char::from_u32(0x2800 + dots as u32).unwrap();
                    buf[(x, y)].set_char(glyph).set_fg(theme.sample(
                        palette,
                        0.56 + mid * 0.04,
                        brightness,
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(renderer: &mut Renderer, style: usize, audio: &Reactivity, time: f32) -> Buffer {
        let area = Rect::new(0, 0, 80, 30);
        let mut buf = Buffer::empty(area);
        renderer.draw(
            &mut buf,
            area,
            time,
            style,
            0,
            &[0.0; 32],
            audio,
            &Theme::default(),
        );
        buf
    }

    fn symbols(buf: &Buffer) -> String {
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn lace_fractional_oscillators_do_not_jump_after_one_turn() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        let mut renderer = Renderer {
            phase: 2.0 * PI - 0.0001,
            last_time: Some(0.0),
            ..Default::default()
        };
        renderer.draw(
            &mut buffer,
            area,
            0.0,
            1,
            0,
            &[0.0; 32],
            &Reactivity::default(),
            &Theme::default(),
        );
        let before = buffer.clone();
        renderer.draw(
            &mut buffer,
            area,
            0.001,
            1,
            0,
            &[0.0; 32],
            &Reactivity::default(),
            &Theme::default(),
        );
        let changed = before
            .content
            .iter()
            .zip(&buffer.content)
            .filter(|(a, b)| a.symbol() != b.symbol())
            .count();
        assert!(
            changed < area.area() as usize / 10,
            "one-turn wrap changed {changed} cells"
        );
    }

    #[test]
    fn attacks_create_voice_specific_shapes_and_leave_history() {
        let mut renderer = Renderer::default();
        let quiet = Reactivity::default();
        let empty = render(&mut renderer, 0, &quiet, 0.0);
        let hit = Reactivity {
            transients: [1.0, 0.8, 0.9, 0.7],
            ..quiet
        };
        let punch = render(&mut renderer, 0, &hit, 0.02);
        let text = symbols(&punch);
        assert_ne!(symbols(&empty), text);
        assert!(
            text.chars().any(|c| ['┏', '┓', '┗', '┛'].contains(&c))
                && text.contains('╋')
                && (text.contains('╱') || text.contains('╲'))
                && text.contains('⁝')
        );
        assert_eq!(renderer.marks.len(), 4);
        assert!(renderer
            .marks
            .iter()
            .all(|mark| mark.column == renderer.column));
        assert!(symbols(&render(&mut renderer, 0, &quiet, 0.1)).contains('╋'));
        for frame in 2..20 {
            render(&mut renderer, 0, &quiet, frame as f32 * 0.1);
        }
        assert!(renderer.marks.is_empty());
    }

    #[test]
    fn held_attack_does_not_fill_score_and_history_is_bounded() {
        let mut renderer = Renderer::default();
        let hit = Reactivity {
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        for frame in 0..30 {
            renderer.advance(frame as f32 * 0.01, &hit);
        }
        assert_eq!(renderer.marks.len(), 4);
        for frame in 30..2000 {
            let audio = if frame % 2 == 0 {
                hit
            } else {
                Reactivity::default()
            };
            renderer.advance(frame as f32 * 0.04, &audio);
            assert!(renderer.marks.len() <= HISTORY_LIMIT);
        }
    }

    #[test]
    fn percussion_choreography_expands_and_strength_sets_scale() {
        let mut renderer = Renderer::default();
        renderer.marks.push(Mark {
            age: 0.0,
            strength: 1.0,
            channel: 0,
            column: 3,
        });
        let first = render(&mut renderer, 0, &Reactivity::default(), 0.0);
        renderer.marks[0].age = 0.24;
        let expanded = render(&mut renderer, 0, &Reactivity::default(), 0.0);
        assert_ne!(symbols(&first), symbols(&expanded));
        let span = |buf: &Buffer| {
            let xs: Vec<usize> = buf
                .content
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.symbol() != " ")
                .map(|(index, _)| index % buf.area.width as usize)
                .collect();
            xs.iter().max().unwrap() - xs.iter().min().unwrap()
        };
        assert!(span(&expanded) > span(&first));
        renderer.marks[0].strength = 0.2;
        let soft = render(&mut renderer, 0, &Reactivity::default(), 0.0);
        assert!(span(&soft) < span(&expanded));
    }

    #[test]
    fn lace_audio_changes_node_locations_not_only_color() {
        let mut quiet = Renderer::default();
        let mut loud = Renderer::default();
        let sound = Reactivity {
            bass: 0.9,
            mid: 0.7,
            treble: 0.8,
            energy: 0.8,
            ..Reactivity::default()
        };
        for frame in 0..30 {
            quiet.advance(frame as f32 / 60.0, &Reactivity::default());
            loud.advance(frame as f32 / 60.0, &sound);
        }
        assert_ne!(
            symbols(&render(&mut quiet, 1, &Reactivity::default(), 0.5)),
            symbols(&render(&mut loud, 1, &sound, 0.5))
        );
    }

    #[test]
    fn lace_moves_at_fixed_audio_and_attacks_immediately_bend_branches() {
        let quiet = Reactivity::default();
        let mut moving = Renderer::default();
        let first = render(&mut moving, 1, &quiet, 0.0);
        render(&mut moving, 1, &quiet, 0.1);
        let later = render(&mut moving, 1, &quiet, 0.2);
        let changed = |a: &Buffer, b: &Buffer| {
            a.content
                .iter()
                .zip(&b.content)
                .filter(|(a, b)| a.symbol() != b.symbol())
                .count()
        };
        assert!(changed(&first, &later) > 30);
        let unstruck = render(&mut Renderer::default(), 1, &quiet, 0.0);
        let hit = Reactivity {
            transients: [1.0; 4],
            ..quiet
        };
        let struck = render(&mut Renderer::default(), 1, &hit, 0.0);
        assert!(changed(&unstruck, &struck) > 8);
        let visible = later
            .content
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .count();
        assert!(
            visible < later.content.len() / 2,
            "lace must retain negative space"
        );
    }

    #[test]
    fn styles_have_distinct_glyph_languages() {
        let hit = Reactivity {
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        let score = symbols(&render(&mut Renderer::default(), 0, &hit, 0.0));
        let lace = symbols(&render(&mut Renderer::default(), 1, &hit, 0.0));
        assert!(score.contains('┃'));
        assert!(!score
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
        assert!(lace.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
        assert!(!lace.contains('┃'));
    }

    #[test]
    fn clips_offsets_empty_and_tiny_areas_without_touching_surroundings() {
        let hit = Reactivity {
            transients: [1.0; 4],
            ..Reactivity::default()
        };
        for style in 0..2 {
            for area in [
                Rect::new(7, 9, 0, 0),
                Rect::new(8, 10, 1, 1),
                Rect::new(9, 11, 80, 30),
                Rect::new(0, 0, 2, 2),
            ] {
                let mut buf = Buffer::empty(Rect::new(7, 9, 6, 5));
                for cell in &mut buf.content {
                    cell.set_char('x');
                }
                Renderer::default().draw(
                    &mut buf,
                    area,
                    0.0,
                    style,
                    0,
                    &[0.0; 32],
                    &hit,
                    &Theme::default(),
                );
                for y in 9..14 {
                    for x in 7..13 {
                        if !area.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "x");
                        }
                    }
                }
            }
        }
    }
}
