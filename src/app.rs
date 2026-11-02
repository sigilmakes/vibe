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
    pub active_playlist: String,
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
    active_playlist: String,
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
            active_playlist: options.active_playlist,
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
        app.prune_playlists();
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
                menu::Effect::CreatePlaylist => self.create_playlist(),
                menu::Effect::DeletePlaylist(name) => self.delete_playlist(&name),
                menu::Effect::AddCurrentScene => self.add_current_scene(),
                menu::Effect::RemoveSelectedScene => self.remove_selected_scene(),
                menu::Effect::MoveSelectedScene(delta) => self.move_selected_scene(delta),
                menu::Effect::Back => {
                    if let Some(menu) = &mut self.menu {
                        menu.back();
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
        self.pages = menu::build_pages(
            &self.config,
            self.scene,
            self.demo,
            &self.active_playlist,
            search,
        );
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
            ids::PLAYLIST_NAME => self.rename_playlist(value),
            ids::PLAYLIST_ACTIVE => self.set_playlist_active(value),
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

    fn current_playlist_name(&self) -> Option<String> {
        self.menu
            .as_ref()
            .and_then(menu::Menu::current_playlist)
            .map(str::to_owned)
    }

    fn selected_playlist_scene(&self) -> Option<usize> {
        self.menu
            .as_ref()
            .and_then(|menu| menu.selected_playlist_scene(&self.pages))
    }

    fn unique_playlist_name(&self) -> String {
        let mut index = 1;
        loop {
            let candidate = format!("Playlist {index}");
            if !self
                .config
                .playlists
                .iter()
                .any(|playlist| playlist.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
            index += 1;
        }
    }

    fn create_playlist(&mut self) {
        let name = self.unique_playlist_name();
        self.config.playlists.push(config::Playlist {
            name: name.clone(),
            scenes: Vec::new(),
        });
        self.mark_config_dirty();
        self.rebuild_pages();
        if let Some(menu) = &mut self.menu {
            menu.open_page(menu::PageId::Playlist(name));
            menu.rebuild(&self.pages);
        }
    }

    fn delete_playlist(&mut self, name: &str) {
        self.config
            .playlists
            .retain(|playlist| playlist.name != name);
        if self.active_playlist == name {
            self.active_playlist.clear();
        }
        self.mark_config_dirty();
        self.save_state();
        self.rebuild_pages();
        if let Some(menu) = &mut self.menu {
            menu.back_to(menu::PageId::Playlists);
        }
    }

    fn rename_playlist(&mut self, value: &str) {
        let trimmed = value.trim();
        let Some(old) = self.current_playlist_name() else {
            return;
        };
        if trimmed.is_empty() {
            return;
        }
        if self
            .config
            .playlists
            .iter()
            .any(|playlist| playlist.name != old && playlist.name.eq_ignore_ascii_case(trimmed))
        {
            return;
        }
        let Some(playlist) = self
            .config
            .playlists
            .iter_mut()
            .find(|playlist| playlist.name == old)
        else {
            return;
        };
        if playlist.name == trimmed {
            return;
        }
        playlist.name = trimmed.to_owned();
        if self.active_playlist == old {
            self.active_playlist = trimmed.to_owned();
        }
        if let Some(menu) = &mut self.menu {
            menu.rename_page(
                menu::PageId::Playlist(old.clone()),
                menu::PageId::Playlist(trimmed.to_owned()),
            );
        }
        self.mark_config_dirty();
    }

    fn set_playlist_active(&mut self, value: &str) {
        let Some(name) = self.current_playlist_name() else {
            return;
        };
        if value == "on" {
            self.active_playlist = name;
        } else if self.active_playlist == name {
            self.active_playlist.clear();
        }
        self.save_state();
    }

    fn add_current_scene(&mut self) {
        let scene_name = visuals::SCENES[self.scene].to_owned();
        let Some(name) = self.current_playlist_name() else {
            return;
        };
        let Some(playlist) = self
            .config
            .playlists
            .iter_mut()
            .find(|playlist| playlist.name == name)
        else {
            return;
        };
        if !playlist.scenes.iter().any(|scene| scene == &scene_name) {
            playlist.scenes.push(scene_name);
            self.mark_config_dirty();
        }
    }

    fn remove_selected_scene(&mut self) {
        let Some(name) = self.current_playlist_name() else {
            return;
        };
        let Some(index) = self.selected_playlist_scene() else {
            return;
        };
        let Some(playlist) = self
            .config
            .playlists
            .iter_mut()
            .find(|playlist| playlist.name == name)
        else {
            return;
        };
        if index < playlist.scenes.len() {
            playlist.scenes.remove(index);
            self.mark_config_dirty();
        }
    }

    fn move_selected_scene(&mut self, delta: isize) {
        let Some(name) = self.current_playlist_name() else {
            return;
        };
        let Some(index) = self.selected_playlist_scene() else {
            return;
        };
        let Some(playlist) = self
            .config
            .playlists
            .iter_mut()
            .find(|playlist| playlist.name == name)
        else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 || target >= playlist.scenes.len() as isize {
            return;
        }
        playlist.scenes.swap(index, target as usize);
        self.mark_config_dirty();
    }

    fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_touched = Some(Instant::now());
    }

    /// Canonicalise scene selection stored in the config. Unknown scenes and
    /// tags are dropped, duplicates are collapsed, and an unknown filter mode
    /// falls back to All.
    fn prune_scenes(&mut self) {
        let before = self.config.scenes.clone();
        let mut favourites = Vec::new();
        for name in &before.favourites {
            let Some(canonical) = visuals::SCENES
                .iter()
                .find(|scene| scene.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            if !favourites
                .iter()
                .any(|known: &String| known.eq_ignore_ascii_case(canonical))
            {
                favourites.push((*canonical).to_owned());
            }
        }
        let mut tags = Vec::new();
        for tag in &before.filter_tags {
            let Some(canonical) = visuals::TAGS
                .iter()
                .find(|known| known.eq_ignore_ascii_case(tag))
            else {
                continue;
            };
            if !tags
                .iter()
                .any(|known: &String| known.eq_ignore_ascii_case(canonical))
            {
                tags.push((*canonical).to_owned());
            }
        }
        self.config.scenes.favourites = favourites;
        self.config.scenes.filter_tags = tags;
        self.config.scenes.filter = match before.filter.as_str() {
            "favourites" => "favourites",
            "tags" => "tags",
            _ => "all",
        }
        .to_owned();
        if self.config.scenes != before {
            self.mark_config_dirty();
        }
    }

    /// Canonicalise playlist data: trim names, drop empty and duplicate names,
    /// drop empty/duplicate/unknown scene names, and resolve the active
    /// playlist against the surviving names.
    fn prune_playlists(&mut self) {
        let before = self.config.playlists.clone();
        let mut playlists: Vec<config::Playlist> = Vec::new();
        for playlist in &before {
            let name = playlist.name.trim();
            if name.is_empty() {
                continue;
            }
            if playlists
                .iter()
                .any(|known| known.name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            let mut scenes = Vec::new();
            for scene in &playlist.scenes {
                let scene = scene.trim();
                if scene.is_empty() {
                    continue;
                }
                let Some(canonical) = visuals::SCENES
                    .iter()
                    .find(|known| known.eq_ignore_ascii_case(scene))
                else {
                    continue;
                };
                if !scenes
                    .iter()
                    .any(|known: &String| known.eq_ignore_ascii_case(canonical))
                {
                    scenes.push((*canonical).to_owned());
                }
            }
            playlists.push(config::Playlist {
                name: name.to_owned(),
                scenes,
            });
        }
        self.config.playlists = playlists;

        if !self.active_playlist.is_empty() {
            match self
                .config
                .playlists
                .iter()
                .find(|playlist| playlist.name.eq_ignore_ascii_case(&self.active_playlist))
            {
                Some(playlist) => {
                    let canonical = playlist.name.clone();
                    if canonical != self.active_playlist {
                        self.active_playlist = canonical;
                    }
                }
                None => self.active_playlist.clear(),
            }
        }

        if self.config.playlists != before {
            self.mark_config_dirty();
        }
    }

    fn flush_config(&mut self) {
        self.prune_scenes();
        self.prune_playlists();
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
            active_playlist: self.active_playlist.clone(),
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
            active_playlist: String::new(),
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
    fn favourites_and_search_are_driven_from_the_scene_picker() {
        let mut app = demo_app();
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Enter));
        let current = visuals::SCENES[app.scene].to_owned();
        app.handle_key(press(KeyCode::Char('f')));
        assert!(app
            .config
            .scenes
            .favourites
            .iter()
            .any(|name| name == &current));

        app.handle_key(press(KeyCode::Char('/')));
        for ch in "jul".chars() {
            app.handle_key(press(KeyCode::Char(ch)));
        }
        app.handle_key(press(KeyCode::Enter));
        let search_selected = {
            let view = app.view(0.0);
            let menu = view.menu.as_ref().unwrap();
            assert!(menu
                .rows
                .iter()
                .any(|row| row.label == "Search" && row.value == "jul"));
            menu.rows[menu.selected].label == "Search"
        };
        if search_selected {
            app.handle_key(press(KeyCode::Down));
        }
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(visuals::SCENES[app.scene], "Julia");
    }

    #[test]
    fn invalid_scene_selection_is_canonicalised_on_startup() {
        let mut options = demo_options();
        options.config.scenes.filter = "nonsense".into();
        options.config.scenes.favourites =
            vec!["julia".into(), "JULIA".into(), "missing scene".into()];
        options.config.scenes.filter_tags =
            vec!["CALM".into(), "calm".into(), "missing tag".into()];
        let app = App::new(options);
        assert_eq!(app.config.scenes.filter, "all");
        assert_eq!(app.config.scenes.favourites, vec!["Julia"]);
        assert_eq!(app.config.scenes.filter_tags, vec!["calm"]);
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
            active_playlist: String::new(),
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

    fn open_playlists(app: &mut App) {
        app.handle_key(press(KeyCode::Char('m')));
        app.handle_key(press(KeyCode::Enter)); // Scenes
        app.handle_key(press(KeyCode::Down)); // Filter…
        app.handle_key(press(KeyCode::Down)); // Playlists…
        app.handle_key(press(KeyCode::Enter)); // Playlists page
    }

    #[test]
    fn creating_a_playlist_pushes_a_unique_named_empty_playlist() {
        let mut app = demo_app();
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Enter)); // New playlist
        assert_eq!(app.config.playlists.len(), 1);
        assert_eq!(app.config.playlists[0].name, "Playlist 1");
        assert!(app.config.playlists[0].scenes.is_empty());
        let view = app.view(0.0);
        let menu = view.menu.as_ref().unwrap();
        assert_eq!(menu.path.last().map(String::as_str), Some("Playlist 1"));

        app.handle_key(press(KeyCode::Esc)); // back to Playlists
        app.handle_key(press(KeyCode::Enter)); // New playlist again
        assert_eq!(app.config.playlists[1].name, "Playlist 2");
    }

    #[test]
    fn renaming_a_playlist_updates_its_name_and_the_active_reference() {
        let mut options = demo_options();
        options.config.playlists = vec![config::Playlist {
            name: "A".into(),
            scenes: vec![],
        }];
        options.active_playlist = "A".into();
        let mut app = App::new(options);
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Down)); // A
        app.handle_key(press(KeyCode::Enter)); // A page
        app.handle_key(press(KeyCode::Enter)); // begin editing Name
        app.handle_key(press(KeyCode::Backspace)); // clear "A"
        for ch in "Chill".chars() {
            app.handle_key(press(KeyCode::Char(ch)));
        }
        app.handle_key(press(KeyCode::Enter)); // commit
        assert_eq!(app.config.playlists[0].name, "Chill");
        assert_eq!(app.active_playlist, "Chill");
    }

    #[test]
    fn renaming_to_an_existing_name_is_rejected() {
        let mut options = demo_options();
        options.config.playlists = vec![
            config::Playlist {
                name: "A".into(),
                scenes: vec![],
            },
            config::Playlist {
                name: "B".into(),
                scenes: vec![],
            },
        ];
        let mut app = App::new(options);
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Down)); // A
        app.handle_key(press(KeyCode::Enter)); // A page
        app.handle_key(press(KeyCode::Enter)); // edit Name
        app.handle_key(press(KeyCode::Backspace)); // clear "A"
        app.handle_key(press(KeyCode::Char('B')));
        app.handle_key(press(KeyCode::Enter)); // commit -> rejected
        assert_eq!(app.config.playlists[0].name, "A");
        assert_eq!(app.config.playlists[1].name, "B");
    }

    #[test]
    fn toggling_active_sets_and_clears_the_active_playlist() {
        let mut options = demo_options();
        options.config.playlists = vec![config::Playlist {
            name: "A".into(),
            scenes: vec![],
        }];
        let mut app = App::new(options);
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Down)); // A
        app.handle_key(press(KeyCode::Enter)); // A page
        app.handle_key(press(KeyCode::Down)); // Active
        app.handle_key(press(KeyCode::Enter)); // set active
        assert_eq!(app.active_playlist, "A");
        app.handle_key(press(KeyCode::Enter)); // clear active
        assert_eq!(app.active_playlist, "");
    }

    #[test]
    fn adding_the_current_scene_appends_it_once() {
        let mut app = demo_app();
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Enter)); // New playlist
        app.handle_key(press(KeyCode::Down)); // Active
        app.handle_key(press(KeyCode::Down)); // Add current scene
        app.handle_key(press(KeyCode::Enter)); // add
        assert_eq!(app.config.playlists[0].scenes, vec![visuals::SCENES[0]]);
        let view = app.view(0.0);
        let menu = view.menu.as_ref().unwrap();
        assert!(!menu.rows.iter().any(|row| row.label == "Add current scene"));
    }

    #[test]
    fn removing_the_selected_scene_removes_it() {
        let mut options = demo_options();
        options.config.playlists = vec![config::Playlist {
            name: "A".into(),
            scenes: vec!["Julia".into()],
        }];
        let mut app = App::new(options);
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Down)); // A
        app.handle_key(press(KeyCode::Enter)); // A page
        app.handle_key(press(KeyCode::Down)); // Active
        app.handle_key(press(KeyCode::Down)); // 1. Julia
        app.handle_key(press(KeyCode::Down)); // Add current scene
        app.handle_key(press(KeyCode::Down)); // Remove selected
        app.handle_key(press(KeyCode::Enter)); // remove
        assert!(app.config.playlists[0].scenes.is_empty());
    }

    #[test]
    fn moving_a_scene_reorders_and_selection_follows() {
        let mut options = demo_options();
        options.config.playlists = vec![config::Playlist {
            name: "A".into(),
            scenes: vec!["Julia".into(), "Waveform Scope".into()],
        }];
        let mut app = App::new(options);
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Down)); // A
        app.handle_key(press(KeyCode::Enter)); // A page
        app.handle_key(press(KeyCode::Down)); // Active
        app.handle_key(press(KeyCode::Down)); // 1. Julia
        app.handle_key(press(KeyCode::Right)); // move down
        assert_eq!(
            app.config.playlists[0].scenes,
            vec!["Waveform Scope", "Julia"]
        );
        assert_eq!(app.selected_playlist_scene(), Some(1));
        let view = app.view(0.0);
        let menu = view.menu.as_ref().unwrap();
        assert_eq!(menu.rows[menu.selected].label, "2. Julia");
    }

    #[test]
    fn playlist_edits_persist_to_config_and_state() {
        let dir = temp_dir("playlist-persist");
        let config_path = dir.join("config.toml");
        let state_path = dir.join("state.toml");
        let mut app = App::new(Options {
            scene: 0,
            config: config::Config::default(),
            demo: true,
            config_path: config_path.clone(),
            state_path: Some(state_path.clone()),
            active_playlist: String::new(),
        });
        open_playlists(&mut app);
        app.handle_key(press(KeyCode::Enter)); // New playlist
        app.handle_key(press(KeyCode::Down)); // Active
        app.handle_key(press(KeyCode::Down)); // Add current scene
        app.handle_key(press(KeyCode::Enter)); // add current scene
        app.handle_key(press(KeyCode::Up)); // back to Active
        app.handle_key(press(KeyCode::Enter)); // set active
        app.flush();

        let (loaded, _) = config::load(&config_path);
        assert_eq!(loaded.playlists, app.config.playlists);
        let (state, _) = state::load(&state_path, None);
        assert_eq!(state.active_playlist, "Playlist 1");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn playlist_data_is_normalised_on_startup() {
        let mut options = demo_options();
        options.config.playlists = vec![
            config::Playlist {
                name: "  Chill  ".into(),
                scenes: vec!["julia".into(), "JULIA".into(), "missing scene".into()],
            },
            config::Playlist {
                name: "chill".into(),
                scenes: vec![],
            },
            config::Playlist {
                name: "  ".into(),
                scenes: vec![],
            },
        ];
        options.active_playlist = "chill".into();
        let app = App::new(options);
        assert_eq!(app.config.playlists.len(), 1);
        assert_eq!(app.config.playlists[0].name, "Chill");
        assert_eq!(app.config.playlists[0].scenes, vec!["Julia"]);
        assert_eq!(app.active_playlist, "Chill");

        // A missing active playlist is cleared on startup.
        let mut options = demo_options();
        options.config.playlists = vec![config::Playlist {
            name: "Chill".into(),
            scenes: vec![],
        }];
        options.active_playlist = "Missing".into();
        let app = App::new(options);
        assert_eq!(app.active_playlist, "");
    }
}
