use crate::palette::Theme;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

pub struct Status<'a> {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
    pub beat: f32,
    pub bpm: Option<f32>,
    pub tempo_confidence: f32,
    pub sensitivity: f32,
    pub bands: &'a [f32; 32],
    pub error: Option<&'a str>,
    pub source: Option<&'a str>,
    pub silent_for: f32,
}

fn color([r, g, b]: [u8; 3]) -> Color {
    Color::Rgb(r, g, b)
}

fn base(theme: &Theme) -> Style {
    Style::default()
        .fg(color(theme.foreground))
        .bg(color(theme.background))
}

fn overlay(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>, theme: &Theme) {
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).style(base(theme)), area);
}

fn confident_bpm(status: &Status<'_>) -> Option<f32> {
    status.bpm.filter(|bpm| bpm.is_finite() && *bpm > 0.0)
}

fn tempo(status: &Status<'_>) -> String {
    match confident_bpm(status) {
        Some(bpm) => format!("{bpm:.0} BPM"),
        None => "tempo · learning".to_owned(),
    }
}

fn level(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn envelope(label: &str, value: f32, width: u16, accent: Color, theme: &Theme) -> Line<'static> {
    let cells = width.saturating_sub(8).min(36) as usize;
    let filled = (level(value) * cells as f32).round() as usize;
    Line::from(vec![
        Span::raw(format!(" {label:<5} ")),
        Span::styled("━".repeat(filled), Style::default().fg(accent)),
        Span::styled(
            "─".repeat(cells - filled),
            Style::default().fg(color(theme.ansi[8])),
        ),
    ])
}

fn spectrum(status: &Status<'_>, width: u16, theme: &Theme) -> Line<'static> {
    let count = width.saturating_sub(2).min(32) as usize;
    let glyphs = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut spans = vec![Span::raw(" ")];
    for i in 0..count {
        let start = i * 32 / count;
        let end = (i + 1) * 32 / count;
        let value = status.bands[start..end]
            .iter()
            .copied()
            .map(level)
            .fold(0.0, f32::max);
        let color = if i * 3 < count {
            color(theme.ansi[4])
        } else if i * 3 < count * 2 {
            color(theme.ansi[2])
        } else {
            color(theme.ansi[3])
        };
        spans.push(Span::styled(
            glyphs[(value * 7.0).round() as usize].to_string(),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

fn centered(screen: Rect, width: u16, height: u16) -> Rect {
    let width = screen.width.min(width);
    let height = screen.height.min(height);
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}

fn help_popup(frame: &mut Frame, theme: &Theme) {
    let area = centered(frame.area(), 38, 9);
    let lines = vec![
        Line::styled(
            " Controls",
            Style::default()
                .fg(color(theme.ansi[12]))
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(" m     opens menu"),
        Line::raw(" Esc   closes menu / popup"),
        Line::raw(" ↑/↓ Enter navigate / select menus"),
        Line::raw(" ←/→   visualisation   p  palette"),
        Line::raw(" +/-   sensitivity"),
        Line::raw(" d     demo"),
        Line::raw(" r     reconnect"),
        Line::raw(" q     quit"),
    ];
    overlay(frame, area, lines, theme);
}

fn analysis_popup(frame: &mut Frame, status: &Status<'_>, theme: &Theme) {
    let area = centered(frame.area(), 44, 10);
    let lines = vec![
        Line::styled(
            " Analysis",
            Style::default()
                .fg(color(theme.ansi[12]))
                .add_modifier(Modifier::BOLD),
        ),
        spectrum(status, area.width, theme),
        envelope("bass", status.bass, area.width, color(theme.ansi[4]), theme),
        envelope("mid", status.mid, area.width, color(theme.ansi[2]), theme),
        envelope(
            "high",
            status.treble,
            area.width,
            color(theme.ansi[3]),
            theme,
        ),
        envelope(
            "onset",
            status.onset,
            area.width,
            color(theme.ansi[11]),
            theme,
        ),
        envelope(
            "kick",
            status.kick,
            area.width,
            color(theme.ansi[12]),
            theme,
        ),
        envelope(
            "beat",
            status.beat,
            area.width,
            color(theme.ansi[10]),
            theme,
        ),
        Line::raw(format!(" {}", tempo(status))),
        Line::raw(format!(
            " Confidence {:.2} · response {:.1}×",
            level(status.tempo_confidence),
            status.sensitivity
        )),
    ];
    overlay(frame, area, lines, theme);
}

/// Draw a compact, opaque menu above the canvas.
pub fn menu(frame: &mut Frame, title: &str, items: &[&str], selected: usize, theme: &Theme) {
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    let width = screen.width.min(34);
    let height = screen
        .height
        .min(items.len().saturating_add(2).min(u16::MAX as usize) as u16);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let capacity = height.saturating_sub(2) as usize;
    let selected = selected.min(items.len().saturating_sub(1));
    let first = selected.saturating_sub(capacity.saturating_sub(1));
    let mut lines = vec![Line::styled(
        format!(" {title}"),
        Style::default()
            .fg(color(theme.ansi[12]))
            .add_modifier(Modifier::BOLD),
    )];
    for (index, item) in items.iter().enumerate().skip(first).take(capacity) {
        let active = index == selected;
        let marker = if active { "›" } else { " " };
        let text = format!(
            " {marker} {item:<padding$}",
            padding = width.saturating_sub(3) as usize
        );
        let style = if active {
            Style::default()
                .fg(color(theme.background))
                .bg(color(theme.foreground))
                .add_modifier(Modifier::BOLD)
        } else {
            base(theme)
        };
        lines.push(Line::styled(text, style));
    }
    overlay(frame, area, lines, theme);
}

/// Leave the canvas untouched unless an explicit popup or error is present.
pub fn draw(frame: &mut Frame, analysis: bool, help: bool, status: &Status<'_>, theme: &Theme) {
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    if let Some(error) = status.error {
        let message = error.lines().next().unwrap_or("Audio unavailable");
        let width = message.chars().count().saturating_add(8).min(60) as u16;
        let area = centered(screen, width, 1);
        overlay(
            frame,
            area,
            vec![Line::styled(
                format!(" Audio: {message}"),
                Style::default().fg(color(theme.ansi[1])),
            )],
            theme,
        );
    } else if status.silent_for > 3.0 {
        let source = status.source.unwrap_or("selected source");
        let message = format!("{source} is silent · --list-audio");
        let width = message.chars().count().saturating_add(8).min(60) as u16;
        let area = centered(screen, width, 1);
        overlay(
            frame,
            area,
            vec![Line::styled(
                format!(" Audio: {message}"),
                Style::default().fg(color(theme.ansi[3])),
            )],
            theme,
        );
    }
    if analysis {
        analysis_popup(frame, status, theme);
    }
    if help {
        help_popup(frame, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    fn status(bands: &[f32; 32]) -> Status<'_> {
        Status {
            bass: 0.7,
            mid: 0.4,
            treble: 0.2,
            onset: 0.8,
            kick: 0.7,
            beat: 0.6,
            bpm: Some(120.0),
            tempo_confidence: 0.8,
            sensitivity: 1.0,
            bands,
            error: None,
            source: None,
            silent_for: 0.0,
        }
    }

    fn render(width: u16, height: u16, analysis: bool, help: bool, error: Option<&str>) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let bands = [0.5; 32];
        let mut status = status(&bands);
        status.error = error;
        terminal
            .draw(|frame| {
                for cell in &mut frame.buffer_mut().content {
                    cell.set_symbol("▀");
                }
                draw(frame, analysis, help, &status, &Theme::default());
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| row(buffer, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_popup_only(buffer: &Buffer, area: Rect) {
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                if area.contains((x, y).into()) {
                    assert_ne!(cell.symbol(), "▀");
                    assert_ne!(cell.bg, Color::Reset);
                } else {
                    assert_eq!(cell.symbol(), "▀");
                }
            }
        }
    }

    #[test]
    fn default_leaves_the_full_canvas_untouched() {
        for (width, height) in [(1, 1), (20, 7), (80, 24), (160, 50)] {
            let buffer = render(width, height, false, false, None);
            assert!(buffer.content.iter().all(|cell| cell.symbol() == "▀"));
        }
    }

    #[test]
    fn analysis_is_a_centered_opaque_popup_only_when_requested() {
        let buffer = render(100, 30, true, false, None);
        assert_popup_only(&buffer, Rect::new(28, 10, 44, 10));
        let text = text(&buffer);
        for measurement in [
            "bass",
            "mid",
            "high",
            "onset",
            "kick",
            "beat",
            "120 BPM",
            "Confidence 0.80",
        ] {
            assert!(text.contains(measurement));
        }
        for unwanted in ["Aurora", "Lagoon", "LIVE", "m menu"] {
            assert!(!text.contains(unwanted));
        }
    }

    #[test]
    fn errors_do_not_create_a_persistent_status_line() {
        let buffer = render(80, 24, false, false, Some("Disconnected"));
        assert_popup_only(&buffer, Rect::new(30, 11, 20, 1));
        assert!(text(&buffer).contains("Audio: Disconnected"));
    }

    #[test]
    fn silent_capture_shows_a_source_hint() {
        let bands = [0.0; 32];
        let mut status = status(&bands);
        status.source = Some("Dummy Output");
        status.silent_for = 4.0;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, false, false, &status, &Theme::default()))
            .unwrap();
        let text = text(terminal.backend().buffer());
        assert!(text.contains("Dummy Output is silent"));
        assert!(text.contains("--list-audio"));
    }

    #[test]
    fn help_is_opaque_and_describes_only_current_controls() {
        let buffer = render(80, 25, false, true, None);
        assert_popup_only(&buffer, Rect::new(21, 8, 38, 9));
        let text = text(&buffer);
        for control in [
            "m     opens menu",
            "Esc   closes",
            "↑/↓ Enter",
            "←/→   visualisation",
            "+/-",
            "d     demo",
            "r     reconnect",
            "q     quit",
        ] {
            assert!(text.contains(control));
        }
        for obsolete in [
            "show", "hide", "toggle", "s/p/v", "space", "?", "LIVE", "Scene",
        ] {
            assert!(!text.contains(obsolete));
        }
    }

    #[test]
    fn small_and_empty_terminals_clip_popups_without_touching_other_cells() {
        for (width, height) in [(0, 0), (1, 1), (8, 2), (20, 7), (160, 50)] {
            for (analysis, help, w, h) in [(true, false, 44, 10), (false, true, 38, 9)] {
                let buffer = render(width, height, analysis, help, None);
                assert_popup_only(&buffer, centered(buffer.area, w, h));
            }
        }
        let buffer = render(20, 7, false, true, None);
        assert!(row(&buffer, 1).contains("opens menu"));
        assert!(row(&buffer, 2).contains("closes"));
    }

    #[test]
    fn tempo_display_uses_the_tracker_decision() {
        let bands = [0.0; 32];
        let mut status = status(&bands);
        status.tempo_confidence = 0.35;
        assert_eq!(tempo(&status), "120 BPM");
        status.bpm = None;
        assert_eq!(tempo(&status), "tempo · learning");
    }

    #[test]
    fn menu_is_opaque_borderless_and_scrolls_to_selection() {
        for (width, height) in [(0, 0), (1, 1), (8, 3), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    for cell in &mut frame.buffer_mut().content {
                        cell.set_symbol("▀");
                    }
                    menu(
                        frame,
                        "Menu",
                        &["First", "Second", "Third", "Controls"],
                        3,
                        &Theme::default(),
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_popup_only(buffer, centered(buffer.area, 34, 6));
            if width == 80 {
                assert!(row(buffer, 13).contains("› Controls"));
                assert_eq!(buffer[(24, 13)].bg, color(Theme::default().foreground));
                assert_eq!(buffer[(24, 13)].fg, color(Theme::default().background));
                let text = text(buffer);
                assert!(!text.contains('─'));
                assert!(!text.contains('│'));
            }
            if width == 8 {
                assert!(row(buffer, 1).contains("› Con"));
            }
        }
    }
}
