use ratatui::{buffer::Buffer, layout::Rect, style::Color};

const HISTORY: usize = 256;
const SAMPLE_PERIOD: f32 = 1.0 / 30.0;
const FALL: f32 = 2.8;
const PEAK_FALL: f32 = 0.75;
const PEAK_HOLD: f32 = 0.10;
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub struct Renderer {
    last_time: Option<f32>,
    levels: [f32; 32],
    peaks: [f32; 32],
    holds: [f32; 32],
    history: [[f32; 32]; HISTORY],
    head: usize,
    len: usize,
    pending: f32,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            last_time: None,
            levels: [0.0; 32],
            peaks: [0.0; 32],
            holds: [0.0; 32],
            history: [[0.0; 32]; HISTORY],
            head: 0,
            len: 0,
            pending: 0.0,
        }
    }
}

fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

// Inputs are already logarithmic, 40 Hz–16 kHz. Group contiguous bins rather
// than dropping high frequencies when the terminal is narrow.
fn grouped(bands: &[f32; 32], index: usize, count: usize) -> f32 {
    let start = index * 32 / count;
    let end = ((index + 1) * 32 / count).max(start + 1);
    bands[start..end].iter().copied().fold(0.0, f32::max)
}

impl Renderer {
    fn advance(&mut self, time: f32, bands: &[f32; 32]) {
        let time = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        let dt = self
            .last_time
            .map_or(0.0, |last| (time - last).clamp(0.0, 0.1));
        self.last_time = Some(time);
        let input = bands.map(clean);
        for (i, &value) in input.iter().enumerate() {
            self.levels[i] = value.max(self.levels[i] - FALL * dt);
            if value >= self.peaks[i] && value > 0.0 {
                self.peaks[i] = value;
                self.holds[i] = PEAK_HOLD;
            } else {
                let falling = (dt - self.holds[i]).max(0.0);
                self.holds[i] = (self.holds[i] - dt).max(0.0);
                self.peaks[i] = self.levels[i].max(self.peaks[i] - PEAK_FALL * falling);
            }
        }
        self.pending += dt;
        if self.len == 0 {
            self.push(input);
        } else {
            while self.pending >= SAMPLE_PERIOD {
                self.pending -= SAMPLE_PERIOD;
                self.push(input);
            }
            // The newest sample stays live even between history ticks.
            self.history[self.head] = input;
        }
    }

    fn push(&mut self, bands: [f32; 32]) {
        if self.len > 0 {
            self.head = (self.head + 1) % HISTORY;
        }
        self.history[self.head] = bands;
        self.len = (self.len + 1).min(HISTORY);
    }

    fn past(&self, age: usize) -> &[f32; 32] {
        &self.history[(self.head + HISTORY - age) % HISTORY]
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
        _audio: &crate::visuals::Reactivity,
        theme: &crate::palette::Theme,
    ) {
        self.advance(time, bands);
        let visible = area.intersection(buf.area);
        let background = Color::Rgb(
            theme.background[0],
            theme.background[1],
            theme.background[2],
        );
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                buf[(x, y)].reset();
                buf[(x, y)]
                    .set_char(' ')
                    .set_bg(background)
                    .set_fg(background);
            }
        }
        if area.width == 0 || area.height == 0 {
            return;
        }
        if style % 2 == 1 {
            let count = usize::from(area.width).min(32);
            for y in visible.y..visible.bottom() {
                // Newest spectrum is at the bottom; two time slices per cell.
                let lower_age = usize::from(area.bottom() - 1 - y) * 2;
                for x in visible.x..visible.right() {
                    let band = usize::from(x - area.x) * count / usize::from(area.width);
                    let color = |age| {
                        let value = if age < self.len {
                            grouped(self.past(age), band, count)
                        } else {
                            0.0
                        };
                        theme.sample(palette, 0.43 + band as f32 / count as f32 * 0.42, value)
                    };
                    let upper = color(lower_age + 1);
                    let lower = color(lower_age);
                    if upper != background || lower != background {
                        buf[(x, y)].set_char('▀').set_fg(upper).set_bg(lower);
                    }
                }
            }
        } else {
            let width = usize::from(area.width);
            let count = ((width + 1) / 3).clamp(1, 32);
            for band in 0..count {
                let left = band * width / count;
                let right = (band + 1) * width / count;
                let end = right - usize::from(count > 1);
                let height = grouped(&self.levels, band, count) * area.height as f32;
                let peak = grouped(&self.peaks, band, count) * area.height as f32;
                for y in visible.y..visible.bottom() {
                    let bottom = f32::from(area.bottom() - 1 - y);
                    let eighths = ((height - bottom).clamp(0.0, 1.0) * 8.0).round() as usize;
                    let cap = peak > 0.0
                        && bottom <= (peak - 0.125).max(0.0)
                        && (peak - 0.125).max(0.0) < bottom + 1.0;
                    let phase = 0.43 + bottom / area.height as f32 * 0.40;
                    for offset in left..end {
                        let x = area.x + offset as u16;
                        if x < visible.x || x >= visible.right() {
                            continue;
                        }
                        if eighths > 0 {
                            buf[(x, y)]
                                .set_char(BLOCKS[eighths])
                                .set_fg(theme.sample(palette, phase, 0.86));
                        } else if cap {
                            buf[(x, y)]
                                .set_char('▔')
                                .set_fg(theme.sample(palette, phase, 1.0));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(
        renderer: &mut Renderer,
        area: Rect,
        time: f32,
        style: usize,
        bands: &[f32; 32],
    ) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 20));
        renderer.draw(
            &mut buf,
            area,
            time,
            style,
            0,
            bands,
            &crate::visuals::Reactivity::default(),
            &crate::palette::Theme::default(),
        );
        buf
    }

    #[test]
    fn immediate_attack_quick_release_and_independent_peak() {
        let mut renderer = Renderer::default();
        renderer.advance(0.0, &[1.0; 32]);
        assert_eq!(renderer.levels, [1.0; 32]);
        renderer.advance(0.05, &[0.0; 32]);
        assert!(renderer.levels[0] < 0.9);
        assert_eq!(renderer.peaks[0], 1.0);
        renderer.advance(0.15, &[0.0; 32]);
        assert!(renderer.peaks[0] < 1.0);
        assert!(renderer.peaks[0] > renderer.levels[0]);
        renderer.advance(50.0, &[0.0; 32]);
        assert!(renderer.peaks[0] > 0.8, "stalls must not erase peaks");
        for i in 1..30 {
            renderer.advance(50.0 + i as f32 * 0.1, &[0.0; 32]);
        }
        assert_eq!(renderer.levels, [0.0; 32]);
        assert_eq!(renderer.peaks, [0.0; 32]);
    }

    #[test]
    fn all_frequency_regions_survive_narrow_aggregation() {
        for band in 0..32 {
            let mut bands = [0.0; 32];
            bands[band] = 1.0;
            assert!((0..3).any(|i| grouped(&bands, i, 3) == 1.0));
            let buf = render(
                &mut Renderer::default(),
                Rect::new(7, 9, 1, 1),
                0.0,
                0,
                &bands,
            );
            assert_eq!(buf[(7, 9)].symbol(), "█");
            assert_eq!(buf[(6, 9)].symbol(), " ");
        }
    }

    #[test]
    fn clips_to_buffer_and_handles_empty_offset_areas() {
        for style in 0..2 {
            for area in [
                Rect::new(99, 19, 10, 10),
                Rect::new(120, 30, 2, 2),
                Rect::new(9, 9, 0, 1),
                Rect::new(9, 9, 1, 0),
            ] {
                render(&mut Renderer::default(), area, 0.0, style, &[1.0; 32]);
            }
            // A partially visible target must paint only its intersection.
            let outer = Rect::new(0, 0, 20, 10);
            let target = Rect::new(14, 6, 20, 10);
            let mut buf = Buffer::empty(outer);
            for cell in &mut buf.content {
                cell.set_char('!');
            }
            Renderer::default().draw(
                &mut buf,
                target,
                0.0,
                style,
                0,
                &[1.0; 32],
                &crate::visuals::Reactivity::default(),
                &crate::palette::Theme::default(),
            );
            let visible = target.intersection(outer);
            for y in outer.y..outer.bottom() {
                for x in outer.x..outer.right() {
                    if !visible.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), "!", "style {style} painted outside");
                    }
                }
            }
        }
    }

    #[test]
    fn waterfall_preserves_frequency_and_scrolls_with_bounded_history() {
        let mut renderer = Renderer::default();
        let mut low = [0.0; 32];
        low[0] = 1.0;
        let mut high = [0.0; 32];
        high[31] = 1.0;
        renderer.advance(0.0, &low);
        renderer.advance(0.04, &high);
        assert_eq!(renderer.past(0), &high);
        assert_eq!(renderer.past(1), &low);
        let area = Rect::new(0, 0, 32, 4);
        let buf = render(&mut renderer, area, 0.08, 1, &high);
        assert_ne!(buf[(31, 3)].bg, Color::Rgb(0, 0, 0));
        assert_eq!(buf[(15, 3)].bg, Color::Rgb(0, 0, 0));
        assert_ne!(buf[(0, 2)].bg, Color::Rgb(0, 0, 0));
        for i in 1..1000 {
            renderer.advance(0.08 + i as f32 * 0.04, &high);
        }
        assert_eq!(renderer.len, HISTORY);
        assert!(renderer.head < HISTORY);
        assert!(renderer.history.iter().all(|row| row == &high));
    }

    #[test]
    fn silence_is_exact_theme_background() {
        let theme = crate::palette::Theme {
            background: [17, 23, 29],
            ..Default::default()
        };
        for style in 0..2 {
            let area = Rect::new(3, 4, 9, 5);
            let mut buf = Buffer::empty(area);
            Renderer::default().draw(
                &mut buf,
                area,
                0.0,
                style,
                0,
                &[0.0; 32],
                &crate::visuals::Reactivity::default(),
                &theme,
            );
            assert!(buf
                .content
                .iter()
                .all(|cell| cell.symbol() == " " && cell.bg == Color::Rgb(17, 23, 29)));
        }
    }
}
