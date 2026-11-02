use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{audio, palette, ui, visuals};

/// Starting state for an interactive session.
pub struct Options {
    pub scene: usize,
    pub palette: usize,
    pub sensitivity: f32,
    pub demo: bool,
    pub capture: audio::CaptureConfig,
}

/// Whether the frame loop should keep running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// One frame's visual input.
///
/// Audio-derived values are already scaled by the current sensitivity.
/// `status()` projects the diagnostic subset used by the UI overlays.
pub struct View<'a> {
    pub scene: usize,
    pub palette: usize,
    pub bands: &'a [f32; 32],
    pub reactivity: &'a visuals::Reactivity,
    pub analysis: bool,
    pub help: bool,
    pub menu: Option<MenuView>,
    audio: &'a audio::AudioState,
    sensitivity: f32,
    source: &'a str,
    capture_error: Option<&'a str>,
    demo: bool,
}

impl View<'_> {
    pub fn status(&self) -> ui::Status<'_> {
        ui::Status {
            sensitivity: self.sensitivity,
            bass: self.audio.bass,
            mid: self.audio.mid,
            treble: self.audio.treble,
            onset: self.audio.onset,
            kick: self.audio.kick,
            beat: self.audio.beat,
            bpm: self.audio.bpm,
            tempo_confidence: self.audio.tempo_confidence,
            bands: self.bands,
            error: if self.demo {
                None
            } else {
                self.capture_error.or(self.audio.error.as_deref())
            },
            source: if self.demo { None } else { Some(self.source) },
            silent_for: if self.demo {
                0.0
            } else {
                self.audio.silent_for
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MenuView {
    pub title: &'static str,
    pub items: &'static [&'static str],
    pub selected: usize,
}

#[derive(Clone, Copy)]
enum MenuKind {
    Root,
    Scenes,
    Palettes,
}

struct Menu {
    kind: MenuKind,
    selected: usize,
}

impl Menu {
    fn items(&self) -> &'static [&'static str] {
        match self.kind {
            MenuKind::Root => &["Visualisation", "Palette", "Analysis", "Controls"],
            MenuKind::Scenes => visuals::SCENES,
            MenuKind::Palettes => palette::PALETTES,
        }
    }

    fn title(&self) -> &'static str {
        match self.kind {
            MenuKind::Root => "Vibe",
            MenuKind::Scenes => "Visualisation",
            MenuKind::Palettes => "Palette",
        }
    }

    fn step(&mut self, delta: isize) {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.items().len() as isize) as usize;
    }

    fn view(&self) -> MenuView {
        MenuView {
            title: self.title(),
            items: self.items(),
            selected: self.selected,
        }
    }
}

/// Interactive session state: visuals, overlays, sensitivity, and audio.
///
/// The CLI resolves names and indices before constructing an `App`. A live
/// capture that fails to start is recorded and reported through the frame
/// status rather than failing construction, so an unavailable sink degrades
/// to an error overlay instead of ending the session.
pub struct App {
    scene: usize,
    palette: usize,
    analysis: bool,
    help: bool,
    menu: Option<Menu>,
    sensitivity: f32,
    demo: bool,
    capture: Option<audio::Capture>,
    capture_error: Option<String>,
    capture_config: audio::CaptureConfig,
    audio: audio::AudioState,
    bands: [f32; 32],
    reactivity: visuals::Reactivity,
    source: String,
}

impl App {
    /// Start a session, opening live audio unless `demo` is set.
    pub fn new(options: Options) -> Self {
        let mut app = Self {
            scene: options.scene,
            palette: options.palette,
            analysis: false,
            help: false,
            menu: None,
            sensitivity: options.sensitivity,
            demo: options.demo,
            capture: None,
            capture_error: None,
            capture_config: options.capture,
            audio: audio::AudioState::default(),
            bands: [0.0; 32],
            reactivity: visuals::Reactivity::default(),
            source: String::new(),
        };
        if !app.demo {
            app.connect();
        }
        app
    }

    /// Sample audio and derive the values for one rendered frame.
    pub fn view(&mut self, time: f32) -> View<'_> {
        self.audio = if self.demo {
            demo_audio(time)
        } else {
            self.capture
                .as_ref()
                .map(audio::Capture::snapshot)
                .unwrap_or_default()
        };
        let gain = self.sensitivity;
        self.bands = self.audio.bands.map(|band| (band * gain).clamp(0.0, 1.0));
        self.reactivity = reactivity(&self.audio, gain);
        self.source = self.audio.source.describe();
        View {
            scene: self.scene,
            palette: self.palette,
            bands: &self.bands,
            reactivity: &self.reactivity,
            analysis: self.analysis,
            help: self.help,
            menu: self.menu.as_ref().map(Menu::view),
            audio: &self.audio,
            sensitivity: gain,
            source: &self.source,
            capture_error: self.capture_error.as_deref(),
            demo: self.demo,
        }
    }

    /// Apply a key press. Only press events are considered; key releases and
    /// repeats are ignored. A popup owns the keyboard until it closes, apart
    /// from quit shortcuts, which always work.
    pub fn handle_key(&mut self, key: KeyEvent) -> Flow {
        if key.kind != KeyEventKind::Press {
            return Flow::Continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Flow::Quit;
        }
        if let Some(mut active) = self.menu.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Char('q') => return Flow::Quit,
                KeyCode::Up | KeyCode::Char('k') => {
                    active.step(-1);
                    self.menu = Some(active);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    active.step(1);
                    self.menu = Some(active);
                }
                KeyCode::Enter => match active.kind {
                    MenuKind::Root => match active.selected {
                        0 => {
                            self.menu = Some(Menu {
                                kind: MenuKind::Scenes,
                                selected: self.scene,
                            });
                        }
                        1 => {
                            self.menu = Some(Menu {
                                kind: MenuKind::Palettes,
                                selected: self.palette,
                            });
                        }
                        2 => self.analysis = true,
                        _ => self.help = true,
                    },
                    MenuKind::Scenes => self.scene = active.selected,
                    MenuKind::Palettes => self.palette = active.selected,
                },
                _ => self.menu = Some(active),
            }
            return Flow::Continue;
        }
        if self.analysis {
            match key.code {
                KeyCode::Esc => self.analysis = false,
                KeyCode::Char('q') => return Flow::Quit,
                _ => {}
            }
            return Flow::Continue;
        }
        if self.help {
            match key.code {
                KeyCode::Esc => self.help = false,
                KeyCode::Char('q') => return Flow::Quit,
                _ => {}
            }
            return Flow::Continue;
        }
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                self.scene = if key.code == KeyCode::Left {
                    (self.scene + visuals::SCENES.len() - 1) % visuals::SCENES.len()
                } else {
                    (self.scene + 1) % visuals::SCENES.len()
                };
            }
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Char('m') => {
                self.menu = Some(Menu {
                    kind: MenuKind::Root,
                    selected: 0,
                })
            }
            KeyCode::Char('p') => {
                self.menu = Some(Menu {
                    kind: MenuKind::Palettes,
                    selected: self.palette,
                })
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.sensitivity = (self.sensitivity + 0.1).min(3.0)
            }
            KeyCode::Char('-') => self.sensitivity = (self.sensitivity - 0.1).max(0.25),
            KeyCode::Char('r') => {
                self.demo = false;
                self.connect();
            }
            KeyCode::Char('d') => {
                self.demo = !self.demo;
                if self.demo {
                    self.capture = None;
                } else {
                    self.connect();
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    /// The selected visualisation, for persistence when the session ends.
    pub fn scene(&self) -> usize {
        self.scene
    }

    fn connect(&mut self) {
        self.capture = None;
        match audio::Capture::start(self.capture_config.clone()) {
            Ok(capture) => {
                self.capture = Some(capture);
                self.capture_error = None;
            }
            Err(error) => self.capture_error = Some(error.to_string()),
        }
    }
}

fn demo_audio(t: f32) -> audio::AudioState {
    let phase = (t * 2.0).fract();
    let kick = (-phase * 18.0).exp();
    let hat = (-((t * 8.0).fract()) * 20.0).exp();
    let mut state = audio::AudioState::default();
    for (i, band) in state.bands.iter_mut().enumerate() {
        let f = i as f32;
        *band = if i < 9 {
            kick * (0.9 - f * 0.04)
        } else {
            ((t * 1.3 + f * 0.6).sin() * 0.5 + 0.5) * 0.35 + hat * 0.25
        };
    }
    state.energy = 0.2 + kick * 0.65;
    state.peak = state.energy;
    state.rms = state.energy * 0.1;
    state.bass = kick;
    state.mid = 0.25 + (t * 1.3).sin() * 0.15;
    state.treble = hat * 0.7;
    state.onset = kick.max(hat * 0.4);
    state.transients = [kick, hat * 0.6, hat, hat * 0.8];
    state.novelty = kick.max(hat);
    state.chroma = [
        1.0 / 3.0,
        0.0,
        0.0,
        0.0,
        1.0 / 3.0,
        0.0,
        0.0,
        1.0 / 3.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ];
    state.centroid = 0.25 + hat * 0.45;
    state.flatness = hat * 0.5;
    state.kick = kick;
    state.beat = kick;
    state.beat_phase = phase;
    state.waveform =
        std::array::from_fn(|i| (i as f32 * 0.4 + t * 12.0).sin() * (0.3 + kick * 0.6));
    state.bpm = Some(120.0);
    state.tempo_confidence = 1.0;
    state.connected = true;
    state
}

fn reactivity(state: &audio::AudioState, gain: f32) -> visuals::Reactivity {
    let scale = |value: f32| (value * gain).clamp(0.0, 1.0);
    visuals::Reactivity {
        energy: scale(state.energy),
        bass: scale(state.bass),
        mid: scale(state.mid),
        treble: scale(state.treble),
        onset: scale(state.onset),
        transients: state.transients.map(scale),
        novelty: scale(state.novelty),
        chroma: state.chroma,
        centroid: state.centroid,
        flatness: state.flatness,
        kick: scale(state.kick),
        beat: scale(state.beat),
        beat_phase: state.beat_phase,
        waveform: state.waveform.map(|x| (x * gain).clamp(-1.0, 1.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn unreachable_capture() -> audio::CaptureConfig {
        audio::CaptureConfig {
            backend: audio::Backend::File(PathBuf::from("/nonexistent/vibe-audio.pcm")),
            target: None,
            format: audio::PcmFormat::default(),
        }
    }

    fn demo_options() -> Options {
        Options {
            scene: 0,
            palette: 0,
            sensitivity: 1.0,
            demo: true,
            capture: unreachable_capture(),
        }
    }

    fn demo_app() -> App {
        App::new(demo_options())
    }

    #[test]
    fn demo_frame_renders_on_small_and_large_terminals() {
        let mut app = demo_app();
        let view = app.view(4.2);
        let theme = palette::Theme::default();
        for (width, height) in [(1, 1), (80, 24), (160, 50)] {
            let area = ratatui::layout::Rect::new(0, 0, width, height);
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            visuals::Renderer::default().draw(
                &mut buffer,
                area,
                4.2,
                view.scene,
                view.palette,
                view.bands,
                view.reactivity,
                &theme,
            );
            assert!(buffer.content.iter().any(|cell| cell.symbol() != " "));
        }
    }

    #[test]
    fn arrows_walk_the_scene_catalog_with_wraparound() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Left));
        assert_eq!(app.scene(), visuals::SCENES.len() - 1);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene(), 0);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene(), 1);
    }

    #[test]
    fn menu_navigation_wraps_and_selection_applies_to_the_scene() {
        let mut app = demo_app();
        assert_eq!(app.handle_key(press(KeyCode::Char('m'))), Flow::Continue);
        let menu = app.view(0.0).menu.unwrap();
        assert_eq!(menu.title, "Vibe");
        assert_eq!(menu.items.len(), 4);

        app.handle_key(press(KeyCode::Up));
        assert_eq!(app.view(0.0).menu.unwrap().selected, 3);
        app.handle_key(press(KeyCode::Down));
        assert_eq!(app.view(0.0).menu.unwrap().selected, 0);

        app.handle_key(press(KeyCode::Enter));
        let menu = app.view(0.0).menu.unwrap();
        assert_eq!(menu.title, "Visualisation");
        assert_eq!(menu.selected, 0);
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.scene(), 1);
        assert!(app.view(0.0).menu.is_none());
    }

    #[test]
    fn palette_menu_starts_on_the_active_palette() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('p')));
        {
            let view = app.view(0.0);
            let menu = view.menu.unwrap();
            assert_eq!(menu.title, "Palette");
            assert_eq!(menu.selected, view.palette);
        }

        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.view(0.0).palette, 1);
    }

    #[test]
    fn open_popups_consume_global_shortcuts_until_closed() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('?')));
        assert!(app.view(0.0).help);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene(), 0);
        app.handle_key(press(KeyCode::Esc));
        assert!(!app.view(0.0).help);

        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene(), 1);
    }

    #[test]
    fn menu_analysis_entry_opens_the_overlay_and_escape_closes_it() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        {
            let view = app.view(0.0);
            assert!(view.analysis);
            assert!(view.menu.is_none());
        }

        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene(), 0);
        app.handle_key(press(KeyCode::Esc));
        assert!(!app.view(0.0).analysis);
    }

    #[test]
    fn q_and_ctrl_c_quit_from_any_overlay() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('?')));
        assert_eq!(app.handle_key(press(KeyCode::Char('q'))), Flow::Quit);

        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_c), Flow::Quit);
    }

    #[test]
    fn sensitivity_stays_within_cli_bounds_and_does_not_change_tempo() {
        let mut app = demo_app();
        for _ in 0..40 {
            app.handle_key(press(KeyCode::Char('+')));
        }
        {
            let view = app.view(0.0);
            let status = view.status();
            assert!((status.sensitivity - 3.0).abs() < 1e-5);
            assert_eq!(status.bpm, Some(120.0));
            assert!(view.reactivity.energy <= 1.0);
            assert!(view.reactivity.kick <= 1.0);
        }

        for _ in 0..40 {
            app.handle_key(press(KeyCode::Char('-')));
        }
        assert!((app.view(0.0).status().sensitivity - 0.25).abs() < 1e-5);
    }

    #[test]
    fn capture_start_failure_is_reported_and_demo_toggle_recovers() {
        let mut options = demo_options();
        options.demo = false;
        let mut app = App::new(options);
        let view = app.view(1.0);
        assert!(view.status().error.is_some());
        assert_eq!(view.reactivity.energy, 0.0);

        app.handle_key(press(KeyCode::Char('d')));
        let view = app.view(1.0);
        assert!(view.status().error.is_none());
        assert!(view.status().source.is_none());
        assert_eq!(view.status().bpm, Some(120.0));

        app.handle_key(press(KeyCode::Char('d')));
        assert!(app.view(1.0).status().error.is_some());
    }
}
