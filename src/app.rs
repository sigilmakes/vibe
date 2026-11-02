use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{audio, config, menu, palette, state, ui, visuals};

/// Starting state for an interactive session. The CLI loads and seeds the
/// config and resolves the scene before constructing an `App`.
pub struct Options {
    pub scene: usize,
    pub config: config::Config,
    pub demo: bool,
    pub config_path: PathBuf,
    pub state_path: Option<PathBuf>,
}

/// Whether the frame loop should keep running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// One frame's visual input. Settings are resolved from the in-memory config
/// so menu edits take effect on the very next frame.
pub struct View<'a> {
    pub scene: usize,
    pub palette: usize,
    pub bands: &'a [f32; 32],
    pub reactivity: &'a visuals::Reactivity,
    pub analysis: bool,
    pub menu: Option<menu::MenuView>,
    pub fps: u32,
    pub color_depth: palette::ColorDepth,
    pub glyphs: palette::GlyphMode,
    pub follow_theme: bool,
    audio: &'a audio::AudioState,
    sensitivity: f32,
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
        }
    }
}

const CONFIG_DEBOUNCE: Duration = Duration::from_millis(250);

/// Interactive session state: the scene, the in-memory config, the analysis
/// overlay, and live audio. Menu edits change the config in place, mark it
/// dirty, and are debounced to disk; the state file is written when the scene
/// changes and on exit.
pub struct App {
    scene: usize,
    config: config::Config,
    config_dirty: bool,
    config_touched: Option<Instant>,
    save_errors: Vec<String>,
    config_path: PathBuf,
    state_path: Option<PathBuf>,
    analysis: bool,
    menu: Option<menu::Menu>,
    demo: bool,
    capture: Option<audio::Capture>,
    audio: audio::AudioState,
    bands: [f32; 32],
    reactivity: visuals::Reactivity,
    pages: Vec<menu::Page>,
}

impl App {
    pub fn new(options: Options) -> Self {
        let mut app = Self {
            scene: options.scene,
            config: options.config,
            config_dirty: false,
            config_touched: None,
            save_errors: Vec::new(),
            config_path: options.config_path,
            state_path: options.state_path,
            analysis: false,
            menu: None,
            demo: options.demo,
            capture: None,
            audio: audio::AudioState::default(),
            bands: [0.0; 32],
            reactivity: visuals::Reactivity::default(),
            pages: Vec::new(),
        };
        app.prune_scenes();
        app.rebuild_pages();
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
        let gain = self.config.appearance.sensitivity;
        self.bands = self.audio.bands.map(|band| (band * gain).clamp(0.0, 1.0));
        self.reactivity = reactivity(&self.audio, gain);
        let menu = self.menu.as_ref().map(|menu| menu.view(&self.pages));
        View {
            scene: self.scene,
            palette: self.palette_index(),
            bands: &self.bands,
            reactivity: &self.reactivity,
            analysis: self.analysis,
            menu,
            fps: self.config.appearance.fps,
            color_depth: self.color_depth(),
            glyphs: self.glyphs(),
            follow_theme: self.config.appearance.follow_theme,
            audio: &self.audio,
            sensitivity: gain,
        }
    }

    /// Apply a key press. Only press events are considered. The menu owns the
    /// keyboard while open; quit shortcuts always work.
    pub fn handle_key(&mut self, key: KeyEvent) -> Flow {
        if key.kind != KeyEventKind::Press {
            return Flow::Continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Flow::Quit;
        }
        if self.menu.is_some() {
            let effect = self
                .menu
                .as_mut()
                .expect("menu checked above")
                .handle_key(key, &self.pages);
            match effect {
                menu::Effect::Quit => return Flow::Quit,
                menu::Effect::CloseMenu => {
                    self.menu = None;
                    self.flush_config();
                }
                menu::Effect::OpenAnalysis => {
                    self.menu = None;
                    self.analysis = true;
                }
                menu::Effect::SelectScene(scene) => {
                    self.menu = None;
                    self.set_scene(scene);
                }
                menu::Effect::ToggleFavourite(index) => self.toggle_favourite(index),
                menu::Effect::ClearTags => {
                    if !self.config.scenes.filter_tags.is_empty() {
                        self.config.scenes.filter_tags.clear();
                        self.mark_config_dirty();
                    }
                }
                menu::Effect::Changed { id, value } => self.apply_change(id, &value),
                menu::Effect::None => {}
            }
            self.rebuild_pages();
            return Flow::Continue;
        }
        if self.analysis {
            match key.code {
                KeyCode::Esc => self.analysis = false,
                KeyCode::Char('q') => return Flow::Quit,
                KeyCode::Char('m') => {
                    self.analysis = false;
                    self.menu = Some(menu::Menu::new());
                    self.rebuild_pages();
                }
                _ => {}
            }
            return Flow::Continue;
        }
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                let next = if key.code == KeyCode::Left {
                    (self.scene + visuals::SCENES.len() - 1) % visuals::SCENES.len()
                } else {
                    (self.scene + 1) % visuals::SCENES.len()
                };
                self.set_scene(next);
            }
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Char('m') => {
                self.menu = Some(menu::Menu::new());
                self.rebuild_pages();
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Debounced config write; call once per frame.
    pub fn maybe_persist(&mut self, now: Instant) {
        if self.config_dirty {
            let due = self
                .config_touched
                .is_none_or(|touched| now.duration_since(touched) >= CONFIG_DEBOUNCE);
            if due {
                self.flush_config();
            }
        }
    }

    /// Flush the config and state before exit; errors are reported afterwards.
    pub fn flush(&mut self) {
        self.flush_config();
        self.save_state();
    }

    /// Errors accumulated while saving, reported once after terminal restore.
    pub fn take_save_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.save_errors)
    }

    fn rebuild_pages(&mut self) {
        let search = self
            .menu
            .as_ref()
            .map(menu::Menu::search)
            .unwrap_or_default();
        self.pages = menu::build_pages(&self.config, self.scene, self.demo, search);
        if let Some(menu) = &mut self.menu {
            menu.rebuild(&self.pages);
        }
    }

    fn palette_index(&self) -> usize {
        palette::PALETTES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(&self.config.appearance.palette))
            .unwrap_or(0)
    }

    fn color_depth(&self) -> palette::ColorDepth {
        use clap::ValueEnum;
        palette::ColorDepth::from_str(&self.config.appearance.color_depth, true)
            .unwrap_or(palette::ColorDepth::Auto)
    }

    fn glyphs(&self) -> palette::GlyphMode {
        use clap::ValueEnum;
        palette::GlyphMode::from_str(&self.config.appearance.glyphs, true)
            .unwrap_or(palette::GlyphMode::Auto)
    }

    fn apply_change(&mut self, id: menu::RowId, value: &str) {
        use menu::ids;
        if let Some(index) = menu::tag_index(id) {
            let tag = visuals::TAGS[index].to_owned();
            let on = value == "on";
            let present = self
                .config
                .scenes
                .filter_tags
                .iter()
                .any(|selected| selected == &tag);
            if on && !present {
                self.config.scenes.filter_tags.push(tag);
                self.mark_config_dirty();
            } else if !on && present {
                self.config
                    .scenes
                    .filter_tags
                    .retain(|selected| selected != &tag);
                self.mark_config_dirty();
            }
            return;
        }
        match id {
            ids::DEMO => {
                let on = value == "on";
                if on != self.demo {
                    self.demo = on;
                    if on {
                        self.capture = None;
                    } else {
                        self.connect();
                    }
                }
            }
            ids::SOURCE => {
                if self.config.audio.source != value {
                    self.config.audio.source = value.to_owned();
                    self.mark_config_dirty();
                    self.connect();
                }
            }
            ids::TARGET => {
                if self.config.audio.target != value {
                    self.config.audio.target = value.to_owned();
                    self.mark_config_dirty();
                    self.connect();
                }
            }
            ids::FORMAT => {
                if self.config.audio.format != value {
                    self.config.audio.format = value.to_owned();
                    self.mark_config_dirty();
                    self.connect();
                }
            }
            ids::RATE => {
                if let Ok(rate) = value.parse::<u32>() {
                    if (8_000..=192_000).contains(&rate) && rate != self.config.audio.rate {
                        self.config.audio.rate = rate;
                        self.mark_config_dirty();
                        self.connect();
                    }
                }
            }
            ids::CHANNELS => {
                if let Ok(channels) = value.parse::<u16>() {
                    if (1..=32).contains(&channels) && channels != self.config.audio.channels {
                        self.config.audio.channels = channels;
                        self.mark_config_dirty();
                        self.connect();
                    }
                }
            }
            ids::COMMAND => {
                if self.config.audio.command != value {
                    self.config.audio.command = value.to_owned();
                    self.mark_config_dirty();
                    self.connect();
                }
            }
            ids::FILE => {
                if self.config.audio.file != value {
                    self.config.audio.file = value.to_owned();
                    self.mark_config_dirty();
                    self.connect();
                }
            }
            ids::LOOP_FILE => {
                let on = value == "on";
                if self.config.audio.loop_file != on {
                    self.config.audio.loop_file = on;
                    self.mark_config_dirty();
                }
            }
            ids::PALETTE => {
                if self.config.appearance.palette != value {
                    self.config.appearance.palette = value.to_owned();
                    self.mark_config_dirty();
                }
            }
            ids::FOLLOW_THEME => {
                let on = value == "on";
                if self.config.appearance.follow_theme != on {
                    self.config.appearance.follow_theme = on;
                    self.mark_config_dirty();
                }
            }
            ids::FPS => {
                if let Ok(fps) = value.parse::<u32>() {
                    if (1..=240).contains(&fps) && fps != self.config.appearance.fps {
                        self.config.appearance.fps = fps;
                        self.mark_config_dirty();
                    }
                }
            }
            ids::COLOR_DEPTH => {
                if self.config.appearance.color_depth != value {
                    self.config.appearance.color_depth = value.to_owned();
                    self.mark_config_dirty();
                }
            }
            ids::GLYPHS => {
                if self.config.appearance.glyphs != value {
                    self.config.appearance.glyphs = value.to_owned();
                    self.mark_config_dirty();
                }
            }
            ids::SENSITIVITY => {
                if let Ok(sensitivity) = value.parse::<f32>() {
                    if (0.25..=3.0).contains(&sensitivity)
                        && sensitivity != self.config.appearance.sensitivity
                    {
                        self.config.appearance.sensitivity = sensitivity;
                        self.mark_config_dirty();
                    }
                }
            }
            ids::FILTER_MODE => {
                let mode = menu::filter_mode_to_config(value);
                if self.config.scenes.filter != mode {
                    self.config.scenes.filter = mode.to_owned();
                    self.mark_config_dirty();
                }
            }
            ids::SEARCH => {
                if let Some(menu) = &mut self.menu {
                    menu.set_search(value.to_owned());
                }
            }
            _ => {}
        }
    }

    fn toggle_favourite(&mut self, index: usize) {
        let name = visuals::SCENES[index];
        if let Some(position) = self
            .config
            .scenes
            .favourites
            .iter()
            .position(|f| f.eq_ignore_ascii_case(name))
        {
            self.config.scenes.favourites.remove(position);
        } else {
            self.config.scenes.favourites.push(name.to_owned());
        }
        self.mark_config_dirty();
    }

    fn set_scene(&mut self, scene: usize) {
        self.scene = scene;
        self.save_state();
    }

    fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_touched = Some(Instant::now());
    }

    /// Drop favourites and filter tags that no longer resolve to a known scene
    /// or tag, so old or hand-edited configs become clean again.
    fn prune_scenes(&mut self) {
        let favourites = self.config.scenes.favourites.len();
        self.config.scenes.favourites.retain(|name| {
            visuals::SCENES
                .iter()
                .any(|scene| scene.eq_ignore_ascii_case(name))
        });
        let tags = self.config.scenes.filter_tags.len();
        self.config.scenes.filter_tags.retain(|tag| {
            visuals::TAGS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(tag))
        });
        if self.config.scenes.favourites.len() != favourites
            || self.config.scenes.filter_tags.len() != tags
        {
            self.mark_config_dirty();
        }
    }

    fn flush_config(&mut self) {
        self.prune_scenes();
        if !self.config_dirty {
            return;
        }
        if let Err(error) = config::save(&self.config_path, &self.config) {
            self.save_errors.push(format!(
                "Could not save config to {}: {error}",
                self.config_path.display()
            ));
        }
        self.config_dirty = false;
        self.config_touched = None;
    }

    fn save_state(&mut self) {
        let Some(path) = &self.state_path else {
            return;
        };
        let state = state::State {
            last_scene: visuals::SCENES[self.scene].to_owned(),
            active_playlist: String::new(),
        };
        if let Err(error) = state::save(path, &state) {
            self.save_errors.push(format!(
                "Could not save state to {}: {error}",
                path.display()
            ));
        }
    }

    fn connect(&mut self) {
        self.capture = None;
        if let Ok(capture) = audio::Capture::start(audio::capture_config(&self.config.audio)) {
            self.capture = Some(capture);
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

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vibe-app-{tag}-{}-{unique}", std::process::id()))
    }

    fn demo_options() -> Options {
        Options {
            scene: 0,
            config: config::Config::default(),
            demo: true,
            config_path: std::env::temp_dir().join("vibe-unused-config.toml"),
            state_path: None,
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
        assert_eq!(app.scene, visuals::SCENES.len() - 1);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene, 0);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.scene, 1);
    }

    #[test]
    fn analysis_is_reached_from_the_root_menu() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        let view = app.view(0.0);
        assert!(view.menu.is_some());
        assert_eq!(
            view.menu.as_ref().unwrap().path.last().map(String::as_str),
            Some("Vibe")
        );

        for _ in 0..3 {
            app.handle_key(press(KeyCode::Down));
        }
        app.handle_key(press(KeyCode::Enter));
        let view = app.view(0.0);
        assert!(view.analysis);
        assert!(view.menu.is_none());

        app.handle_key(press(KeyCode::Esc));
        assert!(!app.view(0.0).analysis);
    }

    #[test]
    fn menu_opens_over_analysis() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        for _ in 0..3 {
            app.handle_key(press(KeyCode::Down));
        }
        app.handle_key(press(KeyCode::Enter));
        assert!(app.view(0.0).analysis);
        app.handle_key(press(KeyCode::Char('m')));
        let view = app.view(0.0);
        assert!(!view.analysis);
        assert!(view.menu.is_some());
    }

    #[test]
    fn scene_picker_selects_a_scene_and_closes_the_menu() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.scene, 1);
        assert!(app.view(0.0).menu.is_none());
    }

    #[test]
    fn menu_edits_update_config_in_place() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.config.appearance.palette, "Phosphor");
    }

    #[test]
    fn demo_toggle_hides_audio_rows() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        let view = app.view(0.0);
        let rows = &view.menu.as_ref().unwrap().rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Demo mode");
        assert_eq!(rows[0].value, "on");
    }

    #[test]
    fn config_and_state_are_written_on_flush() {
        let dir = temp_dir("flush");
        let config_path = dir.join("config.toml");
        let state_path = dir.join("state.toml");
        let mut app = App::new(Options {
            scene: 0,
            config: config::Config::default(),
            demo: true,
            config_path: config_path.clone(),
            state_path: Some(state_path.clone()),
        });

        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Down));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Right));
        app.flush();

        let (loaded, _) = config::load(&config_path);
        assert_eq!(loaded.appearance.palette, "Phosphor");
        let (state, _) = state::load(&state_path, None);
        assert_eq!(state.last_scene, visuals::SCENES[0]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn q_and_ctrl_c_quit_from_the_menu() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        assert_eq!(app.handle_key(press(KeyCode::Char('q'))), Flow::Quit);

        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_c), Flow::Quit);
    }
}
