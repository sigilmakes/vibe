use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::{config, palette, visuals};

/// A stable identifier for a menu row. The page builder assigns these so
/// selection can survive page rebuilds even when rows change visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

impl RowId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Row ids shared between the page builder and the app, which maps ids back
/// to concrete settings and actions.
pub mod ids {
    use super::RowId;

    pub const SCENES: RowId = RowId::new(1);
    pub const AUDIO: RowId = RowId::new(2);
    pub const APPEARANCE: RowId = RowId::new(3);
    pub const ANALYSIS: RowId = RowId::new(4);
    pub const CONTROLS: RowId = RowId::new(5);

    pub const CURRENT_SCENE: RowId = RowId::new(10);

    pub const DEMO: RowId = RowId::new(20);
    pub const SOURCE: RowId = RowId::new(21);
    pub const TARGET: RowId = RowId::new(22);
    pub const FORMAT: RowId = RowId::new(23);
    pub const RATE: RowId = RowId::new(24);
    pub const CHANNELS: RowId = RowId::new(25);
    pub const COMMAND: RowId = RowId::new(26);
    pub const FILE: RowId = RowId::new(27);
    pub const LOOP_FILE: RowId = RowId::new(28);

    pub const PALETTE: RowId = RowId::new(30);
    pub const FOLLOW_THEME: RowId = RowId::new(31);
    pub const FPS: RowId = RowId::new(32);
    pub const COLOR_DEPTH: RowId = RowId::new(33);
    pub const GLYPHS: RowId = RowId::new(34);
    pub const SENSITIVITY: RowId = RowId::new(35);

    pub const CONTROL_M: RowId = RowId::new(40);
    pub const CONTROL_ARROWS: RowId = RowId::new(41);
    pub const CONTROL_ENTER: RowId = RowId::new(42);
    pub const CONTROL_LEFT_RIGHT: RowId = RowId::new(43);
    pub const CONTROL_ESC: RowId = RowId::new(44);
    pub const CONTROL_Q: RowId = RowId::new(45);
    pub const CONTROL_CTRL_C: RowId = RowId::new(46);

    pub const SCENE_BASE: u64 = 1000;

    pub const fn scene(index: usize) -> RowId {
        RowId::new(SCENE_BASE + index as u64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Toggle,
    Choice,
    Number,
    Text,
    Action,
    Submenu,
    Checkbox,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumSpec {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub decimals: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageId {
    Root,
    Scenes,
    ScenePicker,
    Audio,
    Appearance,
    Controls,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: RowId,
    pub label: String,
    pub kind: RowKind,
    pub value: String,
    pub visible: bool,
    pub choices: Vec<String>,
    pub num: Option<NumSpec>,
    pub submenu: Option<PageId>,
}

impl Row {
    fn new(id: RowId, label: &str, kind: RowKind, value: String) -> Self {
        Self {
            id,
            label: label.to_owned(),
            kind,
            value,
            visible: true,
            choices: Vec::new(),
            num: None,
            submenu: None,
        }
    }

    fn toggle(id: RowId, label: &str, on: bool) -> Self {
        Self::new(id, label, RowKind::Toggle, bool_value(on))
    }

    fn checkbox(id: RowId, label: &str, on: bool) -> Self {
        Self::new(id, label, RowKind::Checkbox, bool_value(on))
    }

    fn choice(id: RowId, label: &str, choices: &[&str], current: &str) -> Self {
        let value = if choices.iter().any(|c| c == &current) {
            current.to_owned()
        } else {
            choices.first().copied().unwrap_or("").to_owned()
        };
        Self {
            choices: choices.iter().map(|c| c.to_string()).collect(),
            ..Self::new(id, label, RowKind::Choice, value)
        }
    }

    fn number(id: RowId, label: &str, value: f32, spec: NumSpec) -> Self {
        let value = format_number(value.clamp(spec.min, spec.max), spec);
        Self {
            num: Some(spec),
            ..Self::new(id, label, RowKind::Number, value)
        }
    }

    fn text(id: RowId, label: &str, value: &str) -> Self {
        Self::new(id, label, RowKind::Text, value.to_owned())
    }

    fn action(id: RowId, label: &str) -> Self {
        Self::new(id, label, RowKind::Action, String::new())
    }

    fn submenu(id: RowId, label: &str, value: &str, target: PageId) -> Self {
        Self {
            submenu: Some(target),
            ..Self::new(id, label, RowKind::Submenu, value.to_owned())
        }
    }

    fn readonly(id: RowId, label: &str, value: &str) -> Self {
        Self::new(id, label, RowKind::ReadOnly, value.to_owned())
    }
}

fn bool_value(on: bool) -> String {
    if on { "on" } else { "off" }.to_owned()
}

fn format_number(value: f32, spec: NumSpec) -> String {
    let factor = 10_f32.powi(spec.decimals as i32);
    let rounded = (value * factor).round() / factor;
    if spec.decimals == 0 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: PageId,
    pub title: String,
    pub path: Vec<String>,
    pub rows: Vec<Row>,
    /// Preferred selection when the page is opened fresh.
    pub initial: Option<RowId>,
}

impl Page {
    pub fn visible_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|row| row.visible)
    }
}

/// What the app must do in response to a key press. Submenu navigation is
/// applied inside the menu itself and does not surface as an effect.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Quit,
    CloseMenu,
    OpenAnalysis,
    SelectScene(usize),
    Changed { id: RowId, value: String },
}

/// Render-ready snapshot of the open menu.
#[derive(Clone, Debug)]
pub struct MenuView {
    pub path: Vec<String>,
    pub rows: Vec<MenuRowView>,
    pub selected: usize,
}

#[derive(Clone, Debug)]
pub struct MenuRowView {
    pub label: String,
    pub value: String,
    pub kind: RowKind,
    pub editing: bool,
}

struct TextEdit {
    row: RowId,
    buffer: String,
}

/// Menu navigation state: a page stack plus per-page selection, and the
/// optional in-progress text edit. Pages are rebuilt by the app and passed
/// back in on every key press; selection follows stable row ids.
pub struct Menu {
    stack: Vec<PageId>,
    selected: Vec<RowId>,
    last_index: Vec<usize>,
    editing: Option<TextEdit>,
}

impl Menu {
    pub fn new() -> Self {
        Self {
            stack: vec![PageId::Root],
            selected: vec![ids::SCENES],
            last_index: vec![0],
            editing: None,
        }
    }

    /// Re-clamp the selection after pages change, following stable row ids and
    /// falling back to the nearest visible row when the selected row vanished.
    pub fn rebuild(&mut self, pages: &[Page]) {
        for depth in 0..self.stack.len() {
            let Some(page) = find_page(pages, self.stack[depth]) else {
                continue;
            };
            let visible: Vec<&Row> = page.visible_rows().collect();
            if visible.is_empty() {
                self.last_index[depth] = 0;
                continue;
            }
            let selected = self.selected[depth];
            let index = visible
                .iter()
                .position(|row| row.id == selected)
                .unwrap_or_else(|| self.last_index[depth].min(visible.len() - 1));
            self.selected[depth] = visible[index].id;
            self.last_index[depth] = index;
        }
    }

    pub fn view(&self, pages: &[Page]) -> MenuView {
        let Some(page) = self.current_page(pages) else {
            return MenuView {
                path: Vec::new(),
                rows: Vec::new(),
                selected: 0,
            };
        };
        let editing_row = self.editing.as_ref().map(|edit| edit.row);
        let rows: Vec<MenuRowView> = page
            .visible_rows()
            .map(|row| {
                let editing = Some(row.id) == editing_row;
                let value = if editing {
                    self.editing
                        .as_ref()
                        .map(|edit| edit.buffer.clone())
                        .unwrap_or_default()
                } else {
                    row.value.clone()
                };
                MenuRowView {
                    label: row.label.clone(),
                    value,
                    kind: row.kind,
                    editing,
                }
            })
            .collect();
        MenuView {
            path: page.path.clone(),
            selected: self
                .last_index
                .last()
                .copied()
                .unwrap_or(0)
                .min(rows.len().saturating_sub(1)),
            rows,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, pages: &[Page]) -> Effect {
        if key.kind != KeyEventKind::Press {
            return Effect::None;
        }
        if let Some(edit) = &mut self.editing {
            match key.code {
                KeyCode::Esc => {
                    self.editing = None;
                    Effect::None
                }
                KeyCode::Enter => {
                    let row = edit.row;
                    let value = edit.buffer.clone();
                    self.editing = None;
                    Effect::Changed { id: row, value }
                }
                KeyCode::Backspace => {
                    edit.buffer.pop();
                    Effect::None
                }
                KeyCode::Char(ch) => {
                    edit.buffer.push(ch);
                    Effect::None
                }
                _ => Effect::None,
            }
        } else {
            match key.code {
                KeyCode::Esc => {
                    if self.stack.len() > 1 {
                        self.stack.pop();
                        self.selected.pop();
                        self.last_index.pop();
                        Effect::None
                    } else {
                        Effect::CloseMenu
                    }
                }
                KeyCode::Char('m') => Effect::CloseMenu,
                KeyCode::Char('q') => Effect::Quit,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_selection(-1, pages);
                    Effect::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_selection(1, pages);
                    Effect::None
                }
                KeyCode::Left => self.arrow(-1, pages),
                KeyCode::Right => self.arrow(1, pages),
                KeyCode::Enter => self.activate(pages),
                _ => Effect::None,
            }
        }
    }

    fn current_page<'a>(&self, pages: &'a [Page]) -> Option<&'a Page> {
        self.stack.last().and_then(|id| find_page(pages, *id))
    }

    fn selected_row<'a>(&self, pages: &'a [Page]) -> Option<&'a Row> {
        let page = self.current_page(pages)?;
        let index = *self.last_index.last()?;
        page.visible_rows().nth(index)
    }

    fn move_selection(&mut self, delta: isize, pages: &[Page]) {
        let Some(page) = self.current_page(pages) else {
            return;
        };
        let count = page.visible_rows().count();
        if count == 0 {
            return;
        }
        let depth = self.stack.len() - 1;
        let next = (self.last_index[depth] as isize + delta).rem_euclid(count as isize) as usize;
        if let Some(row) = page.visible_rows().nth(next) {
            self.selected[depth] = row.id;
            self.last_index[depth] = next;
        }
    }

    /// Left and Right open and leave submenus; on value rows they step the
    /// value as before.
    fn arrow(&mut self, direction: isize, pages: &[Page]) -> Effect {
        let submenu = self
            .selected_row(pages)
            .and_then(|row| (row.kind == RowKind::Submenu).then_some(row.submenu.clone()));
        if let Some(target) = submenu {
            if direction < 0 {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    self.selected.pop();
                    self.last_index.pop();
                }
            } else {
                self.descend(target, pages);
            }
            Effect::None
        } else {
            self.step_row(direction, pages)
        }
    }

    fn step_row(&self, direction: isize, pages: &[Page]) -> Effect {
        let Some(row) = self.selected_row(pages) else {
            return Effect::None;
        };
        match row.kind {
            RowKind::Choice => {
                if row.choices.is_empty() {
                    return Effect::None;
                }
                let current = row
                    .choices
                    .iter()
                    .position(|choice| choice == &row.value)
                    .unwrap_or(0);
                let next =
                    (current as isize + direction).rem_euclid(row.choices.len() as isize) as usize;
                Effect::Changed {
                    id: row.id,
                    value: row.choices[next].clone(),
                }
            }
            RowKind::Toggle | RowKind::Checkbox => {
                let on = row.value != "on";
                Effect::Changed {
                    id: row.id,
                    value: bool_value(on),
                }
            }
            RowKind::Number => {
                let Some(spec) = row.num else {
                    return Effect::None;
                };
                let current: f32 = row.value.parse().unwrap_or(spec.min);
                let next = (current + direction as f32 * spec.step).clamp(spec.min, spec.max);
                Effect::Changed {
                    id: row.id,
                    value: format_number(next, spec),
                }
            }
            _ => Effect::None,
        }
    }

    fn activate(&mut self, pages: &[Page]) -> Effect {
        let Some(row) = self.selected_row(pages) else {
            return Effect::None;
        };
        let id = row.id;
        match row.kind {
            RowKind::Toggle | RowKind::Checkbox => {
                let on = row.value != "on";
                Effect::Changed {
                    id,
                    value: bool_value(on),
                }
            }
            RowKind::Choice => self.step_row(1, pages),
            RowKind::Number | RowKind::Text => {
                self.editing = Some(TextEdit {
                    row: id,
                    buffer: row.value.clone(),
                });
                Effect::None
            }
            RowKind::Action => {
                if id == ids::ANALYSIS {
                    Effect::OpenAnalysis
                } else if let Some(index) = scene_index(id) {
                    Effect::SelectScene(index)
                } else {
                    Effect::None
                }
            }
            RowKind::Submenu => {
                self.descend(row.submenu, pages);
                Effect::None
            }
            RowKind::ReadOnly => Effect::None,
        }
    }

    fn descend(&mut self, target: Option<PageId>, pages: &[Page]) {
        let Some(target) = target else {
            return;
        };
        let page = find_page(pages, target);
        let initial = page
            .and_then(|page| page.initial)
            .or_else(|| page.and_then(|page| page.visible_rows().next().map(|row| row.id)));
        self.stack.push(target);
        self.selected.push(initial.unwrap_or(RowId::new(0)));
        self.last_index.push(0);
    }
}

fn find_page(pages: &[Page], id: PageId) -> Option<&Page> {
    pages.iter().find(|page| page.id == id)
}

fn scene_index(id: RowId) -> Option<usize> {
    let index = id.0.checked_sub(ids::SCENE_BASE)?;
    (index < visuals::SCENES.len() as u64).then_some(index as usize)
}

const SOURCES: &[&str] = &["auto", "pipewire", "pulse", "command", "file"];
const FORMATS: &[&str] = &["f32le", "s16le", "s32le"];
const COLOR_DEPTHS: &[&str] = &["auto", "truecolor", "ansi256", "ansi16", "mono"];
const GLYPHS: &[&str] = &["auto", "unicode", "ascii"];

const FPS_SPEC: NumSpec = NumSpec {
    min: 1.0,
    max: 240.0,
    step: 1.0,
    decimals: 0,
};
const RATE_SPEC: NumSpec = NumSpec {
    min: 8_000.0,
    max: 192_000.0,
    step: 1_000.0,
    decimals: 0,
};
const CHANNELS_SPEC: NumSpec = NumSpec {
    min: 1.0,
    max: 32.0,
    step: 1.0,
    decimals: 0,
};
const SENSITIVITY_SPEC: NumSpec = NumSpec {
    min: 0.25,
    max: 3.0,
    step: 0.1,
    decimals: 2,
};

fn is_pipewire_or_pulse_or_auto(source: &str) -> bool {
    matches!(source, "auto" | "pipewire" | "pulse")
}

/// Build all pages from the current config, scene, and session state.
pub fn build_pages(config: &config::Config, scene: usize, demo: bool) -> Vec<Page> {
    let scene_name = visuals::SCENES.get(scene).copied().unwrap_or_default();
    vec![
        build_root(),
        build_scenes(scene_name),
        build_scene_picker(scene),
        build_audio(&config.audio, demo),
        build_appearance(&config.appearance),
        build_controls(),
    ]
}

fn build_root() -> Page {
    Page {
        id: PageId::Root,
        title: "Vibe".into(),
        path: vec!["Vibe".into()],
        rows: vec![
            Row::submenu(ids::SCENES, "Scenes", "", PageId::Scenes),
            Row::submenu(ids::AUDIO, "Audio", "", PageId::Audio),
            Row::submenu(ids::APPEARANCE, "Appearance", "", PageId::Appearance),
            Row::action(ids::ANALYSIS, "Analysis"),
            Row::submenu(ids::CONTROLS, "Controls", "", PageId::Controls),
        ],
        initial: None,
    }
}

fn build_scenes(scene_name: &str) -> Page {
    Page {
        id: PageId::Scenes,
        title: "Scenes".into(),
        path: vec!["Vibe".into(), "Scenes".into()],
        rows: vec![Row::submenu(
            ids::CURRENT_SCENE,
            "Visualisation",
            scene_name,
            PageId::ScenePicker,
        )],
        initial: None,
    }
}

fn build_scene_picker(scene: usize) -> Page {
    let rows = visuals::SCENES
        .iter()
        .enumerate()
        .map(|(index, name)| Row::action(ids::scene(index), name))
        .collect();
    Page {
        id: PageId::ScenePicker,
        title: "Visualisation".into(),
        path: vec!["Vibe".into(), "Scenes".into(), "Visualisation".into()],
        rows,
        initial: Some(ids::scene(scene)),
    }
}

fn build_audio(audio: &config::AudioConfig, demo: bool) -> Page {
    let mut rows = vec![Row::toggle(ids::DEMO, "Demo mode", demo)];
    if !demo {
        rows.push(Row::choice(ids::SOURCE, "Source", SOURCES, &audio.source));
        if is_pipewire_or_pulse_or_auto(&audio.source) {
            rows.push(Row::text(ids::TARGET, "Target", &audio.target));
        }
        rows.push(Row::choice(ids::FORMAT, "Format", FORMATS, &audio.format));
        rows.push(Row::number(ids::RATE, "Rate", audio.rate as f32, RATE_SPEC));
        rows.push(Row::number(
            ids::CHANNELS,
            "Channels",
            audio.channels as f32,
            CHANNELS_SPEC,
        ));
        if audio.source == "command" {
            rows.push(Row::text(ids::COMMAND, "Command", &audio.command));
        }
        if audio.source == "file" {
            rows.push(Row::text(ids::FILE, "File", &audio.file));
            rows.push(Row::toggle(ids::LOOP_FILE, "Loop file", audio.loop_file));
        }
    }
    Page {
        id: PageId::Audio,
        title: "Audio".into(),
        path: vec!["Vibe".into(), "Audio".into()],
        rows,
        initial: None,
    }
}

fn build_appearance(appearance: &config::AppearanceConfig) -> Page {
    Page {
        id: PageId::Appearance,
        title: "Appearance".into(),
        path: vec!["Vibe".into(), "Appearance".into()],
        rows: vec![
            Row::choice(
                ids::PALETTE,
                "Palette",
                palette::PALETTES,
                &appearance.palette,
            ),
            Row::checkbox(ids::FOLLOW_THEME, "Follow theme", appearance.follow_theme),
            Row::number(ids::FPS, "FPS", appearance.fps as f32, FPS_SPEC),
            Row::choice(
                ids::COLOR_DEPTH,
                "Colour depth",
                COLOR_DEPTHS,
                &appearance.color_depth,
            ),
            Row::choice(ids::GLYPHS, "Glyphs", GLYPHS, &appearance.glyphs),
            Row::number(
                ids::SENSITIVITY,
                "Sensitivity",
                appearance.sensitivity,
                SENSITIVITY_SPEC,
            ),
        ],
        initial: None,
    }
}

fn build_controls() -> Page {
    Page {
        id: PageId::Controls,
        title: "Controls".into(),
        path: vec!["Vibe".into(), "Controls".into()],
        rows: vec![
            Row::readonly(ids::CONTROL_M, "m", "open / close menu"),
            Row::readonly(ids::CONTROL_ARROWS, "↑ / ↓", "move"),
            Row::readonly(ids::CONTROL_ENTER, "Enter", "select / edit"),
            Row::readonly(
                ids::CONTROL_LEFT_RIGHT,
                "← / →",
                "back / open; change value",
            ),
            Row::readonly(ids::CONTROL_ESC, "Esc", "back / close"),
            Row::readonly(ids::CONTROL_Q, "q", "quit"),
            Row::readonly(ids::CONTROL_CTRL_C, "Ctrl-C", "quit"),
        ],
        initial: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn default_pages(demo: bool) -> Vec<Page> {
        build_pages(&config::Config::default(), 0, demo)
    }

    fn title(menu: &Menu, pages: &[Page]) -> String {
        menu.view(pages).path.last().cloned().unwrap_or_default()
    }

    fn editing(menu: &Menu, pages: &[Page]) -> bool {
        menu.view(pages).rows.iter().any(|row| row.editing)
    }

    #[test]
    fn root_page_lists_the_five_top_level_entries() {
        let pages = default_pages(false);
        let root = find_page(&pages, PageId::Root).unwrap();
        let labels: Vec<&str> = root.visible_rows().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Scenes", "Audio", "Appearance", "Analysis", "Controls"]
        );
    }

    #[test]
    fn scene_picker_has_one_row_per_scene_and_starts_on_current() {
        let pages = build_pages(&config::Config::default(), 7, false);
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        assert_eq!(picker.rows.len(), visuals::SCENES.len());
        assert_eq!(picker.initial, Some(ids::scene(7)));
    }

    #[test]
    fn audio_page_hides_rows_that_do_not_apply_to_the_source() {
        let mut config = config::Config::default();
        config.audio.source = "command".into();
        let pages = build_pages(&config, 0, false);
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let visible: Vec<RowId> = audio.visible_rows().map(|row| row.id).collect();
        assert!(visible.contains(&ids::SOURCE));
        assert!(visible.contains(&ids::COMMAND));
        assert!(!visible.contains(&ids::TARGET));
        assert!(!visible.contains(&ids::FILE));
        assert!(!visible.contains(&ids::LOOP_FILE));

        let mut config = config::Config::default();
        config.audio.source = "file".into();
        let pages = build_pages(&config, 0, false);
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let visible: Vec<RowId> = audio.visible_rows().map(|row| row.id).collect();
        assert!(visible.contains(&ids::FILE));
        assert!(visible.contains(&ids::LOOP_FILE));
        assert!(!visible.contains(&ids::COMMAND));
    }

    #[test]
    fn demo_mode_shows_only_the_demo_row() {
        let pages = default_pages(true);
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let visible: Vec<RowId> = audio.visible_rows().map(|row| row.id).collect();
        assert_eq!(visible, vec![ids::DEMO]);
    }

    #[test]
    fn enter_descends_and_escape_returns() {
        let mut menu = Menu::new();
        let pages = default_pages(false);
        menu.rebuild(&pages);
        assert_eq!(title(&menu, &pages), "Vibe");

        menu.handle_key(press(KeyCode::Down), &pages);
        assert_eq!(menu.handle_key(press(KeyCode::Enter), &pages), Effect::None);
        assert_eq!(title(&menu, &pages), "Audio");

        assert_eq!(menu.handle_key(press(KeyCode::Esc), &pages), Effect::None);
        assert_eq!(title(&menu, &pages), "Vibe");
        assert_eq!(
            menu.handle_key(press(KeyCode::Esc), &pages),
            Effect::CloseMenu
        );
    }

    #[test]
    fn left_and_right_open_and_leave_submenus_and_still_step_values() {
        let mut menu = Menu::new();
        let pages = default_pages(false);
        menu.rebuild(&pages);

        // Right opens the selected submenu; Left returns.
        assert_eq!(menu.handle_key(press(KeyCode::Right), &pages), Effect::None);
        assert_eq!(title(&menu, &pages), "Scenes");
        assert_eq!(menu.handle_key(press(KeyCode::Left), &pages), Effect::None);
        assert_eq!(title(&menu, &pages), "Vibe");

        // Value rows keep using Left and Right to step.
        menu.handle_key(press(KeyCode::Down), &pages); // Audio
        menu.handle_key(press(KeyCode::Right), &pages);
        assert_eq!(title(&menu, &pages), "Audio");
        assert_eq!(
            menu.handle_key(press(KeyCode::Right), &pages),
            Effect::Changed {
                id: ids::DEMO,
                value: "on".into()
            }
        );
    }

    #[test]
    fn choice_rows_cycle_and_report_changed() {
        let mut menu = Menu::new();
        let mut config = config::Config::default();
        let pages = build_pages(&config, 0, false);
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        assert_eq!(title(&menu, &pages), "Appearance");
        assert_eq!(
            menu.handle_key(press(KeyCode::Right), &pages),
            Effect::Changed {
                id: ids::PALETTE,
                value: "Phosphor".into()
            }
        );

        // The app applies the change and rebuilds before the next key.
        config.appearance.palette = "Phosphor".into();
        let pages = build_pages(&config, 0, false);
        menu.rebuild(&pages);
        assert_eq!(
            menu.handle_key(press(KeyCode::Left), &pages),
            Effect::Changed {
                id: ids::PALETTE,
                value: "Terminal".into()
            }
        );
    }

    #[test]
    fn toggle_and_number_rows_step_in_place() {
        let mut menu = Menu::new();
        let pages = default_pages(false);
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        // Demo mode is the first Audio row and starts off.
        assert_eq!(
            menu.handle_key(press(KeyCode::Enter), &pages),
            Effect::Changed {
                id: ids::DEMO,
                value: "on".into()
            }
        );

        // Appearance: Sensitivity is the last row; Right steps by 0.1.
        menu.handle_key(press(KeyCode::Esc), &pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        for _ in 0..5 {
            menu.handle_key(press(KeyCode::Down), &pages);
        }
        assert_eq!(
            menu.handle_key(press(KeyCode::Right), &pages),
            Effect::Changed {
                id: ids::SENSITIVITY,
                value: "1.1".into()
            }
        );
    }

    #[test]
    fn text_edit_appends_commits_and_cancels() {
        let mut menu = Menu::new();
        let mut config = config::Config::default();
        config.audio.source = "command".into();
        let pages = build_pages(&config, 0, false);
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        for _ in 0..5 {
            menu.handle_key(press(KeyCode::Down), &pages);
        }
        assert_eq!(menu.handle_key(press(KeyCode::Enter), &pages), Effect::None);
        assert!(editing(&menu, &pages));
        menu.handle_key(press(KeyCode::Char('x')), &pages);
        assert_eq!(
            menu.handle_key(press(KeyCode::Enter), &pages),
            Effect::Changed {
                id: ids::COMMAND,
                value: "x".into()
            }
        );
        assert!(!editing(&menu, &pages));

        // Cancelling leaves the value untouched.
        menu.handle_key(press(KeyCode::Enter), &pages);
        menu.handle_key(press(KeyCode::Char('y')), &pages);
        assert_eq!(menu.handle_key(press(KeyCode::Esc), &pages), Effect::None);
        assert!(!editing(&menu, &pages));
    }

    #[test]
    fn selection_follows_stable_ids_after_a_row_disappears() {
        let mut menu = Menu::new();
        let mut config = config::Config::default();
        config.audio.source = "command".into();
        let pages = build_pages(&config, 0, false);
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        for _ in 0..5 {
            menu.handle_key(press(KeyCode::Down), &pages);
        }
        assert_eq!(menu.view(&pages).rows[5].label, "Command");

        config.audio.source = "file".into();
        let pages = build_pages(&config, 0, false);
        menu.rebuild(&pages);
        let view = menu.view(&pages);
        assert_eq!(title(&menu, &pages), "Audio");
        assert!(view.selected < view.rows.len());
        assert_eq!(view.rows[view.selected].label, "File");
    }
}
