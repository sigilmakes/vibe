use crate::menu::{MenuRowView, MenuView, RowKind};
use crate::palette::Theme;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

/// Raw analyser values for the analysis overlay. `sensitivity` is the
/// separate visual response multiplier, not applied to these readings.
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

/// Draw an opaque, centered menu with a title path, label/value rows, and
/// scrolling. The selected row is inverted; submenu and editing rows carry
/// trailing markers. No footer hint line is drawn.
pub fn menu(frame: &mut Frame, menu: &MenuView, theme: &Theme) {
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    let width = screen.width.min(44);
    let height = screen
        .height
        .min(menu.rows.len().saturating_add(1).min(u16::MAX as usize) as u16);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let capacity = height.saturating_sub(1) as usize;
    let selected = menu.selected.min(menu.rows.len().saturating_sub(1));
    let first = selected.saturating_sub(capacity.saturating_sub(1));
    let mut lines = vec![Line::styled(
        title_line(&menu.path, first, capacity, menu.rows.len(), width),
        Style::default()
            .fg(color(theme.ansi[12]))
            .add_modifier(Modifier::BOLD),
    )];
    for (index, row) in menu.rows.iter().enumerate().skip(first).take(capacity) {
        lines.push(row_line(row, index == selected, width, theme));
    }
    overlay(frame, area, lines, theme);
}

/// Title line for the menu popup. When the row list is taller than the popup,
/// the visible range is right-aligned so a long catalogue still shows where the
/// cursor sits and that more rows exist below.
fn title_line(path: &[String], first: usize, capacity: usize, total: usize, width: u16) -> String {
    let lead = format!(" {}", path.join(" / "));
    if total <= capacity {
        return clip(lead, width as usize);
    }
    let counter = format!("{}–{}/{total}", first + 1, (first + capacity).min(total));
    let used = lead.chars().count() + counter.chars().count();
    let pad = (width as usize).saturating_sub(used).max(1);
    clip(
        format!("{lead}{}{counter}", " ".repeat(pad)),
        width as usize,
    )
}

fn row_line(row: &MenuRowView, active: bool, width: u16, theme: &Theme) -> Line<'static> {
    let marker = if active { "›" } else { " " };
    let mut text = format!(" {marker} {}", row.label);
    let value = row_value(row);
    if !value.is_empty() {
        let used = text.chars().count();
        let pad = (width as usize).saturating_sub(used + 1 + value.chars().count());
        text.push_str(&" ".repeat(pad));
        text.push(' ');
        text.push_str(&value);
    }
    let text = clip(text, width as usize);
    let style = if active {
        Style::default()
            .fg(color(theme.background))
            .bg(color(theme.foreground))
            .add_modifier(Modifier::BOLD)
    } else {
        base(theme)
    };
    Line::styled(text, style)
}

fn row_value(row: &MenuRowView) -> String {
    match row.kind {
        RowKind::Checkbox => {
            if row.value == "on" {
                "[x]".to_owned()
            } else {
                "[ ]".to_owned()
            }
        }
        RowKind::Submenu => {
            if row.value.is_empty() {
                "›".to_owned()
            } else {
                format!("{} ›", row.value)
            }
        }
        RowKind::Text | RowKind::Number if row.editing => format!("{}▌", row.value),
        RowKind::Action => String::new(),
        _ => match row.unit {
            Some(unit) if !row.value.is_empty() => format!("{} {unit}", row.value),
            _ => row.value.clone(),
        },
    }
}

fn clip(text: String, width: usize) -> String {
    text.chars().take(width).collect()
}

/// Leave the canvas untouched unless the analysis popup is open.
pub fn draw(frame: &mut Frame, analysis: bool, status: &Status<'_>, theme: &Theme) {
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    if analysis {
        analysis_popup(frame, status, theme);
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
        }
    }

    fn render(width: u16, height: u16, analysis: bool) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let bands = [0.5; 32];
        let status = status(&bands);
        terminal
            .draw(|frame| {
                for cell in &mut frame.buffer_mut().content {
                    cell.set_symbol("▀");
                }
                draw(frame, analysis, &status, &Theme::default());
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
            let buffer = render(width, height, false);
            assert!(buffer.content.iter().all(|cell| cell.symbol() == "▀"));
        }
    }

    #[test]
    fn analysis_is_a_centered_opaque_popup_only_when_requested() {
        let buffer = render(100, 30, true);
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
    fn small_and_empty_terminals_clip_popups_without_touching_other_cells() {
        for (width, height) in [(0, 0), (1, 1), (8, 2), (20, 7), (160, 50)] {
            let buffer = render(width, height, true);
            assert_popup_only(&buffer, centered(buffer.area, 44, 10));
        }
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
    fn menu_number_rows_render_their_unit() {
        let view = MenuView {
            path: vec!["Scenes".into()],
            rows: vec![MenuRowView {
                label: "Transition time".into(),
                value: "20".into(),
                unit: Some("s"),
                kind: RowKind::Number,
                editing: false,
            }],
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| menu(frame, &view, &Theme::default()))
            .unwrap();
        assert!(text(terminal.backend().buffer()).contains("Transition time"));
        assert!(text(terminal.backend().buffer()).contains("20 s"));
    }

    fn menu_rows(labels: &[&str]) -> Vec<MenuRowView> {
        labels
            .iter()
            .map(|label| MenuRowView {
                label: (*label).to_owned(),
                value: String::new(),
                unit: None,
                kind: RowKind::Action,
                editing: false,
            })
            .collect()
    }

    fn render_menu(rows: Vec<MenuRowView>, selected: usize, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let view = MenuView {
            path: vec!["Vibe".into(), "Scenes".into(), "Visualisation".into()],
            rows,
            selected,
        };
        terminal
            .draw(|frame| menu(frame, &view, &Theme::default()))
            .unwrap();
        text(terminal.backend().buffer())
    }

    #[test]
    fn long_lists_show_the_visible_range_and_scroll_to_selection() {
        let labels: Vec<String> = (0..201).map(|index| format!("Scene {index}")).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let top = render_menu(menu_rows(&labels), 0, 80, 24);
        assert!(top.contains("1–23/201"), "{top}");
        assert!(top.contains("Scene 0"));
        assert!(!top.contains("Scene 200"));

        let bottom = render_menu(menu_rows(&labels), 200, 80, 24);
        assert!(bottom.contains("179–201/201"), "{bottom}");
        assert!(bottom.contains("› Scene 200"));
        assert!(!bottom.contains("Scene 0"));

        // A list that fits the popup has no range to report.
        let short = render_menu(menu_rows(&["One", "Two", "Three"]), 1, 80, 24);
        assert!(!short.contains('–'), "{short}");
    }

    #[test]
    fn menu_is_opaque_borderless_and_scrolls_to_selection() {
        let rows = vec![
            MenuRowView {
                label: "First".into(),
                value: "a".into(),
                unit: None,
                kind: RowKind::Choice,
                editing: false,
            },
            MenuRowView {
                label: "Second".into(),
                value: "b".into(),
                unit: None,
                kind: RowKind::Choice,
                editing: false,
            },
            MenuRowView {
                label: "Third".into(),
                value: "c".into(),
                unit: None,
                kind: RowKind::Choice,
                editing: false,
            },
            MenuRowView {
                label: "Controls".into(),
                value: "d".into(),
                unit: None,
                kind: RowKind::Submenu,
                editing: false,
            },
        ];
        for (width, height) in [(0, 0), (1, 1), (8, 3), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let view = MenuView {
                path: vec!["Menu".into()],
                rows: rows.clone(),
                selected: 3,
            };
            terminal
                .draw(|frame| {
                    for cell in &mut frame.buffer_mut().content {
                        cell.set_symbol("▀");
                    }
                    menu(frame, &view, &Theme::default());
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_popup_only(buffer, centered(buffer.area, 44, 5));
            if width == 80 {
                assert!(row(buffer, 13).contains("› Controls"));
                assert_eq!(buffer[(24, 13)].bg, color(Theme::default().foreground));
                assert_eq!(buffer[(24, 13)].fg, color(Theme::default().background));
                let text = text(buffer);
                assert!(!text.contains('─'));
                assert!(!text.contains('│'));
            }
            if width == 8 {
                assert!(row(buffer, 2).contains('›'));
            }
        }
    }
}
