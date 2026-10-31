use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum View {
    Minimal,
    Canvas,
    Analysis,
}

pub struct Status<'a> {
    pub scene: &'a str,
    pub palette: &'a str,
    pub energy: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
    pub bpm: Option<f32>,
    pub tempo_confidence: f32,
    pub sensitivity: f32,
    pub bands: &'a [f32; 32],
    pub error: Option<&'a str>,
}

const BACKGROUND: Color = Color::Rgb(12, 15, 23);
const MUTED: Color = Color::Rgb(133, 146, 165);
const CYAN: Color = Color::Rgb(104, 224, 225);
const PINK: Color = Color::Rgb(239, 131, 197);

fn base() -> Style {
    Style::default().fg(MUTED).bg(BACKGROUND)
}

fn overlay(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>) {
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).style(base()), area);
}

fn status_line(status: &Status<'_>, width: u16) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!(" {}", status.scene),
        Style::default().fg(Color::White),
    )];
    let mut used = status.scene.chars().count() + 1;
    let palette = format!(" · {}", status.palette);
    if used + palette.chars().count() < width as usize {
        used += palette.chars().count();
        spans.push(Span::styled(palette, Style::default().fg(PINK)));
    }
    if used + 9 <= width as usize {
        spans.push(Span::raw("   m menu"));
    }
    Line::from(spans)
}

fn confident_bpm(status: &Status<'_>) -> Option<f32> {
    status
        .bpm
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0 && status.tempo_confidence >= 0.45)
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

fn envelope(label: &str, value: f32, width: u16, color: Color) -> Line<'static> {
    let cells = width.saturating_sub(8).min(36) as usize;
    let filled = (level(value) * cells as f32).round() as usize;
    Line::from(vec![
        Span::raw(format!(" {label:<5} ")),
        Span::styled("━".repeat(filled), Style::default().fg(color)),
        Span::styled(
            "─".repeat(cells - filled),
            Style::default().fg(Color::Rgb(47, 57, 72)),
        ),
    ])
}

fn spectrum(status: &Status<'_>, width: u16) -> Line<'static> {
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
            CYAN
        } else if i * 3 < count * 2 {
            Color::Rgb(168, 167, 244)
        } else {
            PINK
        };
        spans.push(Span::styled(
            glyphs[(value * 7.0).round() as usize].to_string(),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

fn help_popup(frame: &mut Frame, status: &Status<'_>) {
    let screen = frame.area();
    let width = screen.width.min(38);
    let height = screen.height.min(13);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let lines = vec![
        Line::styled(
            " Controls",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Line::raw(" m       menu"),
        Line::raw(" s/p/v   scene / palette / view menus"),
        Line::raw(" ↑/↓ j/k select · Enter confirm"),
        Line::raw(" Esc     dismiss menu"),
        Line::raw(" ←/→ space  next scene"),
        Line::raw(" a       auto scenes"),
        Line::raw(" d       demo"),
        Line::raw(" r       reconnect"),
        Line::raw(" +/-     sensitivity"),
        Line::raw(" q       quit"),
        Line::raw(" ?       close help"),
        Line::styled(
            format!(" Sensitivity {:.1}×", status.sensitivity),
            Style::default().fg(PINK),
        ),
    ];
    overlay(frame, area, lines);
}

/// Draw a compact, opaque menu above the canvas and any status overlay.
pub fn menu(frame: &mut Frame, title: &str, items: &[&str], selected: usize) {
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
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
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
                .fg(BACKGROUND)
                .bg(CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            base()
        };
        lines.push(Line::styled(text, style));
    }
    overlay(frame, area, lines);
}

pub fn draw(frame: &mut Frame, view: View, help: bool, status: &Status<'_>) {
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    let panel_height = match view {
        View::Canvas => 0,
        View::Minimal => 1,
        View::Analysis => screen.height.min(6),
    };
    if panel_height > 0 {
        let area = Rect::new(
            screen.x,
            screen.bottom() - panel_height,
            screen.width,
            panel_height,
        );
        let mut lines = Vec::new();
        if view == View::Analysis {
            // Preserve the status line when there is not room for every meter.
            let detail = vec![
                spectrum(status, screen.width),
                envelope("low", status.bass, screen.width, CYAN),
                envelope("mid", status.mid, screen.width, Color::Rgb(168, 167, 244)),
                envelope("high", status.treble, screen.width, PINK),
                Line::from(vec![
                    Span::styled(
                        if level(status.kick) > 0.25 {
                            " ● "
                        } else {
                            " · "
                        },
                        Style::default().fg(CYAN),
                    ),
                    Span::raw("kick "),
                    Span::styled(
                        if level(status.onset) > 0.25 {
                            "● "
                        } else {
                            "· "
                        },
                        Style::default().fg(PINK),
                    ),
                    Span::raw("onset  "),
                    Span::styled(tempo(status), Style::default().fg(CYAN)),
                    Span::raw(if level(status.energy) > 0.02 {
                        "  signal"
                    } else {
                        "  listening"
                    }),
                    Span::raw(format!("  gain {:.1}×", status.sensitivity)),
                    Span::raw(format!(
                        "  confidence {:.0}%",
                        level(status.tempo_confidence) * 100.0
                    )),
                ]),
            ];
            lines.extend(
                detail
                    .into_iter()
                    .take(panel_height.saturating_sub(1) as usize),
            );
        }
        lines.push(status_line(status, screen.width));
        overlay(frame, area, lines);
    }
    if let Some(error) = status.error {
        let y = screen
            .bottom()
            .saturating_sub(panel_height + 1)
            .max(screen.y);
        let area = Rect::new(screen.x, y, screen.width, 1);
        let message = error.lines().next().unwrap_or("Audio unavailable");
        overlay(
            frame,
            area,
            vec![Line::styled(
                format!(" Audio: {message}"),
                Style::default().fg(PINK),
            )],
        );
    }
    if help {
        help_popup(frame, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(
        width: u16,
        height: u16,
        view: View,
        help: bool,
        error: Option<&str>,
    ) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let bands = [0.5; 32];
        let status = Status {
            scene: "Aurora",
            palette: "Lagoon",
            energy: 0.6,
            bass: 0.7,
            mid: 0.4,
            treble: 0.2,
            onset: 0.8,
            kick: 0.7,
            bpm: Some(120.0),
            tempo_confidence: 0.8,
            sensitivity: 1.0,
            bands: &bands,
            error,
        };
        terminal
            .draw(|frame| {
                for y in 0..height {
                    for x in 0..width {
                        frame.buffer_mut()[(x, y)].set_symbol("▀");
                    }
                }
                draw(frame, view, help, &status);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn minimal_is_one_clean_status_line() {
        let buffer = render(100, 30, View::Minimal, false, None);
        let status = row(&buffer, 29);
        assert!(status.contains("Aurora"));
        assert!(status.contains("Lagoon"));
        assert!(status.contains("m menu"));
        for unwanted in ["LIVE", "auto", "manual", "BPM", "━", "─", "▁", "█"] {
            assert!(!status.contains(unwanted));
        }
        assert!(!row(&buffer, 29).contains('▀'));
        assert!(row(&buffer, 0).chars().all(|c| c == '▀'));
    }

    #[test]
    fn analysis_has_meters_and_clears_every_panel_cell() {
        let buffer = render(100, 30, View::Analysis, false, None);
        for y in 24..30 {
            assert!(!row(&buffer, y).contains('▀'));
        }
        assert!(row(&buffer, 25).contains("low"));
        assert!(row(&buffer, 28).contains("onset"));
        assert!(row(&buffer, 28).contains("kick"));
        assert!(row(&buffer, 28).contains("120 BPM"));
        assert!(row(&buffer, 29).contains("Aurora"));
    }

    #[test]
    fn canvas_is_untouched_except_for_error() {
        let buffer = render(80, 24, View::Canvas, false, None);
        assert!(buffer.content.iter().all(|cell| cell.symbol() == "▀"));
        let buffer = render(80, 24, View::Canvas, false, Some("Disconnected"));
        assert!(row(&buffer, 23).contains("Disconnected"));
        assert!(!row(&buffer, 23).contains('▀'));
    }

    #[test]
    fn help_is_opaque_and_lists_controls() {
        let buffer = render(80, 25, View::Canvas, true, None);
        for y in 6..19 {
            for x in 21..59 {
                assert_ne!(buffer[(x, y)].symbol(), "▀");
            }
        }
        let text = (0..25)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        for control in ["←/→ space", "palette", "sensitivity", "reconnect", "quit"] {
            assert!(text.contains(control));
        }
    }

    #[test]
    fn small_and_empty_terminals_do_not_panic() {
        for (width, height) in [(0, 0), (1, 1), (8, 2), (20, 7), (160, 50)] {
            for view in [View::Minimal, View::Canvas, View::Analysis] {
                render(width, height, view, true, Some("Disconnected"));
            }
        }
        let buffer = render(14, 2, View::Minimal, false, None);
        assert!(row(&buffer, 1).contains("Aurora"));
        assert!(!row(&buffer, 1).contains("help"));
    }

    #[test]
    fn unconfident_tempo_is_not_presented_as_a_measurement() {
        let bands = [0.0; 32];
        let mut status = Status {
            scene: "Aurora",
            palette: "Lagoon",
            energy: 0.0,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            onset: 0.0,
            kick: 0.0,
            bpm: Some(120.0),
            tempo_confidence: 0.1,
            sensitivity: 1.0,
            bands: &bands,
            error: None,
        };
        assert_eq!(tempo(&status), "tempo · learning");
        status.bpm = None;
        status.tempo_confidence = 1.0;
        assert_eq!(tempo(&status), "tempo · learning");
    }

    #[test]
    fn menu_is_opaque_and_scrolls_to_selection() {
        for (width, height) in [(0, 0), (1, 1), (8, 3), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    for cell in &mut frame.buffer_mut().content {
                        cell.set_symbol("▀");
                    }
                    menu(frame, "View", &["Minimal", "Canvas", "Analysis"], 2);
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            if width == 80 {
                for y in 9..14 {
                    for x in 23..57 {
                        assert_ne!(buffer[(x, y)].symbol(), "▀");
                    }
                }
                assert!(row(buffer, 12).contains("› Analysis"));
                assert_eq!(buffer[(24, 12)].bg, CYAN);
            }
            if width == 8 {
                assert!(row(buffer, 1).contains("› Ana"));
            }
        }
    }
}
