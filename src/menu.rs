use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::{audio, config, palette, visuals};

/// A stable identifier for a menu row. The page builder assigns these so
/// selection can survive page rebuilds even when rows change visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

impl RowId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Build a stable row id from a dynamic name. The id is an FNV-1a hash of
    /// `kind` and `name` with the high bit set, so dynamic rows never collide
    /// with the small fixed ids or the `SCENE_BASE`/`TAG_BASE` ranges.
    pub fn named(kind: &str, name: &str) -> Self {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x100_0000_01b3;
        let mut hash = FNV_OFFSET;
        for &byte in kind.as_bytes().iter().chain(b"\x00").chain(name.as_bytes()) {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        Self(hash | (1 << 63))
    }
}

/// Row ids shared between the page builder and the app, which maps ids back
/// to concrete settings and actions.
pub mod ids {
    use super::RowId;

    pub const SCENES: RowId = RowId::new(1);
    pub const AUDIO: RowId = RowId::new(2);
    pub const APPEARANCE: RowId = RowId::new(3);
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
    pub const AUTO_RECONNECT: RowId = RowId::new(29);
    pub const NOW_PLAYING: RowId = RowId::new(18);
    pub const DEVICES_REFRESH: RowId = RowId::new(19);

    pub const PALETTE: RowId = RowId::new(30);
    pub const FOLLOW_THEME: RowId = RowId::new(31);
    pub const FPS: RowId = RowId::new(32);
    pub const COLOR_DEPTH: RowId = RowId::new(33);
    pub const GLYPHS: RowId = RowId::new(34);
    pub const SENSITIVITY: RowId = RowId::new(35);
    pub const AUDIO_STATUS: RowId = RowId::new(36);

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

    pub const FILTER: RowId = RowId::new(11);
    pub const FILTER_MODE: RowId = RowId::new(12);
    pub const FILTER_TAGS: RowId = RowId::new(13);
    pub const FILTER_MATCHES: RowId = RowId::new(14);
    pub const TAGS_CLEAR: RowId = RowId::new(15);

    pub const SEARCH: RowId = RowId::new(16);
    pub const NO_MATCHES: RowId = RowId::new(17);

    pub const TAG_BASE: u64 = 2000;

    pub const fn tag(index: usize) -> RowId {
        RowId::new(TAG_BASE + index as u64)
    }

    pub const PLAYLISTS: RowId = RowId::new(50);
    pub const PLAYLISTS_NEW: RowId = RowId::new(51);
    pub const PLAYLISTS_EMPTY: RowId = RowId::new(52);
    pub const PLAYLIST_NAME: RowId = RowId::new(60);
    pub const PLAYLIST_ACTIVE: RowId = RowId::new(61);
    pub const PLAYLIST_ADD_CURRENT: RowId = RowId::new(62);
    pub const PLAYLIST_REMOVE: RowId = RowId::new(63);
    pub const PLAYLIST_UP: RowId = RowId::new(64);
    pub const PLAYLIST_DOWN: RowId = RowId::new(65);
    pub const PLAYLIST_DELETE: RowId = RowId::new(66);
    pub const CONFIRM_DELETE: RowId = RowId::new(67);
    pub const CONFIRM_CANCEL: RowId = RowId::new(68);
    pub const CONFIRM_PROMPT: RowId = RowId::new(69);

    pub const AUTO_MODE: RowId = RowId::new(70);
    pub const AUTO_TRANSITION: RowId = RowId::new(71);
    pub const AUTO_WITHIN: RowId = RowId::new(72);

    pub fn playlist(name: &str) -> RowId {
        RowId::named("playlist", name)
    }

    pub fn playlist_scene(playlist: &str, scene: &str) -> RowId {
        RowId::named("playlist_scene", &format!("{playlist}\x00{scene}"))
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PageId {
    Root,
    Scenes,
    ScenePicker,
    Filter,
    Tags,
    Audio,
    Appearance,
    Controls,
    Playlists,
    Playlist(String),
    ConfirmDelete(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: RowId,
    pub label: String,
    pub kind: RowKind,
    pub value: String,
    /// Optional unit appended to the value in the menu (for example "s").
    pub unit: Option<&'static str>,
    pub visible: bool,
    /// Headers are visible but cannot receive the cursor.
    pub selectable: bool,
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
            unit: None,
            visible: true,
            selectable: true,
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

    fn number_with_unit(
        id: RowId,
        label: &str,
        value: f32,
        spec: NumSpec,
        unit: &'static str,
    ) -> Self {
        Self {
            unit: Some(unit),
            ..Self::number(id, label, value, spec)
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

    fn header(id: RowId, label: &str) -> Self {
        Self {
            selectable: false,
            ..Self::new(id, label, RowKind::ReadOnly, String::new())
        }
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

    pub fn selectable_rows(&self) -> impl Iterator<Item = &Row> {
        self.visible_rows().filter(|row| row.selectable)
    }
}

/// What the app must do in response to a key press. Submenu navigation is
/// applied inside the menu itself and does not surface as an effect.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Quit,
    CloseMenu,
    SelectScene(usize),
    ToggleFavourite(usize),
    ClearTags,
    CreatePlaylist,
    DeletePlaylist(String),
    AddCurrentScene,
    RemoveSelectedScene,
    MoveSelectedScene(isize),
    Back,
    RefreshDevices,
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
    pub unit: Option<&'static str>,
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
    /// The playlist scene row the cursor is on (or was last on), so the
    /// "Remove selected" / "Move up" / "Move down" actions keep operating on
    /// the same scene even though the cursor has moved onto an action row.
    last_scene: Option<RowId>,
    editing: Option<TextEdit>,
    search: String,
}

impl Menu {
    pub fn new() -> Self {
        Self {
            stack: vec![PageId::Root],
            selected: vec![ids::SCENES],
            last_index: vec![0],
            last_scene: None,
            editing: None,
            search: String::new(),
        }
    }

    /// The scene picker's session-local search query (not persisted).
    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn set_search(&mut self, value: String) {
        self.search = value;
    }

    /// The page currently on top of the menu stack.
    pub fn current_page_id(&self) -> Option<&PageId> {
        self.stack.last()
    }

    /// Open a page, starting selection at its first visible row.
    pub fn open_page(&mut self, target: PageId) {
        self.stack.push(target);
        self.selected.push(RowId::new(0));
        self.last_index.push(0);
        self.last_scene = None;
    }

    /// Pop the top page, keeping the root page when already at it.
    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.selected.pop();
            self.last_index.pop();
        }
        self.last_scene = None;
    }

    /// Pop pages until the given page is on top (or the root page is reached).
    pub fn back_to(&mut self, target: PageId) {
        while self.stack.len() > 1 && self.stack.last().is_some_and(|id| id != &target) {
            self.stack.pop();
            self.selected.pop();
            self.last_index.pop();
        }
        self.last_scene = None;
    }

    /// Rename every matching page in the stack (used when a playlist is
    /// renamed, since `PageId::Playlist` and `PageId::ConfirmDelete` embed the
    /// name).
    pub fn rename_page(&mut self, old: PageId, new: PageId) {
        for id in &mut self.stack {
            if *id == old {
                *id = new.clone();
            }
        }
    }

    /// The name of the playlist page currently open, if any.
    pub fn current_playlist(&self) -> Option<&str> {
        match self.stack.last()? {
            PageId::Playlist(name) => Some(name),
            _ => None,
        }
    }

    /// The position of the selected playlist scene, mapping the selected row's
    /// stable id back through the playlist's scene names.
    pub fn selected_playlist_scene(&self, pages: &[Page]) -> Option<usize> {
        let name = self.current_playlist()?;
        let page = find_page(pages, PageId::Playlist(name.to_owned()))?;
        let selected = self.selected.last().copied()?;
        let mut index = 0usize;
        let mut remembered = None;
        for row in page.visible_rows() {
            if row.kind == RowKind::Action && !row.value.is_empty() {
                if row.id == selected {
                    return Some(index);
                }
                if Some(row.id) == self.last_scene {
                    remembered = Some(index);
                }
                index += 1;
            }
        }
        remembered
    }

    /// Re-clamp the selection after pages change, following stable row ids and
    /// falling back to the nearest visible row when the selected row vanished.
    pub fn rebuild(&mut self, pages: &[Page]) {
        // Drop trailing pages that no longer exist (deleted or renamed
        // playlists) so the stack never points at a vanished page.
        while self.stack.len() > 1
            && find_page(pages, self.stack.last().expect("non-empty").clone()).is_none()
        {
            self.stack.pop();
            self.selected.pop();
            self.last_index.pop();
        }
        for depth in 0..self.stack.len() {
            let Some(page) = find_page(pages, self.stack[depth].clone()) else {
                continue;
            };
            let selectable: Vec<&Row> = page.selectable_rows().collect();
            if selectable.is_empty() {
                self.last_index[depth] = 0;
                continue;
            }
            let selected = self.selected[depth];
            let index = selectable
                .iter()
                .position(|row| row.id == selected)
                .unwrap_or_else(|| self.last_index[depth].min(selectable.len() - 1));
            self.selected[depth] = selectable[index].id;
            self.last_index[depth] = index;
        }
        self.sync_last_scene(pages);
    }

    /// Track the playlist scene row the cursor is on so the remove/move
    /// actions keep operating on that scene after the cursor moves off it.
    fn sync_last_scene(&mut self, pages: &[Page]) {
        let Some(name) = self.current_playlist().map(str::to_owned) else {
            self.last_scene = None;
            return;
        };
        let Some(page) = find_page(pages, PageId::Playlist(name)) else {
            self.last_scene = None;
            return;
        };
        let selected = self.selected.last().copied().unwrap_or(RowId::new(0));
        for row in page.visible_rows() {
            if row.kind == RowKind::Action && !row.value.is_empty() && row.id == selected {
                self.last_scene = Some(row.id);
                return;
            }
        }
        // The cursor is on a non-scene row; keep the remembered scene.
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
                    unit: row.unit,
                    kind: row.kind,
                    editing,
                }
            })
            .collect();
        let selected = self
            .selected
            .last()
            .copied()
            .and_then(|id| page.visible_rows().position(|row| row.id == id));
        MenuView {
            path: page.path.clone(),
            selected: selected.unwrap_or(0).min(rows.len().saturating_sub(1)),
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
                        self.back();
                        Effect::None
                    } else {
                        Effect::CloseMenu
                    }
                }
                KeyCode::Char('m') => Effect::CloseMenu,
                KeyCode::Char('q') => Effect::Quit,
                KeyCode::Char('/') => self.start_search_edit(pages),
                KeyCode::Char('f') => self.toggle_favourite(pages),
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
        self.stack
            .last()
            .and_then(|id| find_page(pages, id.clone()))
    }

    fn selected_row<'a>(&self, pages: &'a [Page]) -> Option<&'a Row> {
        let page = self.current_page(pages)?;
        let index = *self.last_index.last()?;
        page.selectable_rows().nth(index)
    }

    fn move_selection(&mut self, delta: isize, pages: &[Page]) {
        let Some(page) = self.current_page(pages) else {
            return;
        };
        let count = page.selectable_rows().count();
        if count == 0 {
            return;
        }
        let depth = self.stack.len() - 1;
        let next = (self.last_index[depth] as isize + delta).rem_euclid(count as isize) as usize;
        let Some(row) = page.selectable_rows().nth(next) else {
            return;
        };
        let id = row.id;
        let is_playlist_scene = self.current_playlist().is_some()
            && row.kind == RowKind::Action
            && !row.value.is_empty();
        self.selected[depth] = id;
        self.last_index[depth] = next;
        if is_playlist_scene {
            self.last_scene = Some(id);
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

    /// `/` begins editing the picker's search row, keeping the current query.
    fn start_search_edit(&mut self, pages: &[Page]) -> Effect {
        if self
            .current_page(pages)
            .is_some_and(|page| page.id != PageId::ScenePicker)
        {
            return Effect::None;
        }
        let Some(page) = self.current_page(pages) else {
            return Effect::None;
        };
        if page.rows.iter().any(|row| row.id == ids::SEARCH) {
            self.editing = Some(TextEdit {
                row: ids::SEARCH,
                buffer: self.search.clone(),
            });
        }
        Effect::None
    }

    /// `f` toggles the favourite flag for the selected scene row.
    fn toggle_favourite(&self, pages: &[Page]) -> Effect {
        if self
            .current_page(pages)
            .is_some_and(|page| page.id != PageId::ScenePicker)
        {
            return Effect::None;
        }
        let Some(row) = self.selected_row(pages) else {
            return Effect::None;
        };
        match scene_index(row.id) {
            Some(index) => Effect::ToggleFavourite(index),
            None => Effect::None,
        }
    }

    fn step_row(&self, direction: isize, pages: &[Page]) -> Effect {
        let Some(row) = self.selected_row(pages) else {
            return Effect::None;
        };
        // On a playlist page, Left/Right reorder the selected scene in place;
        // the row id is keyed by scene name so the cursor stays on the scene.
        if self.current_playlist().is_some() && row.kind == RowKind::Action && !row.value.is_empty()
        {
            return Effect::MoveSelectedScene(direction);
        }
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
        let value = row.value.clone();
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
                if id == ids::DEVICES_REFRESH {
                    Effect::RefreshDevices
                } else if id == ids::TAGS_CLEAR {
                    Effect::ClearTags
                } else if let Some(index) = scene_index(id) {
                    Effect::SelectScene(index)
                } else if id == ids::PLAYLISTS_NEW {
                    Effect::CreatePlaylist
                } else if id == ids::PLAYLIST_ADD_CURRENT {
                    Effect::AddCurrentScene
                } else if id == ids::PLAYLIST_REMOVE {
                    Effect::RemoveSelectedScene
                } else if id == ids::PLAYLIST_UP {
                    Effect::MoveSelectedScene(-1)
                } else if id == ids::PLAYLIST_DOWN {
                    Effect::MoveSelectedScene(1)
                } else if id == ids::CONFIRM_DELETE {
                    match self.current_page(pages).map(|page| &page.id) {
                        Some(PageId::ConfirmDelete(name)) => Effect::DeletePlaylist(name.clone()),
                        _ => Effect::None,
                    }
                } else if id == ids::CONFIRM_CANCEL {
                    Effect::Back
                } else if !value.is_empty() {
                    // Playlist scene rows store their scene name in the value.
                    visuals::SCENES
                        .iter()
                        .position(|scene| *scene == value.as_str())
                        .map_or(Effect::None, Effect::SelectScene)
                } else {
                    Effect::None
                }
            }
            RowKind::Submenu => {
                self.descend(row.submenu.clone(), pages);
                Effect::None
            }
            RowKind::ReadOnly => Effect::None,
        }
    }

    fn descend(&mut self, target: Option<PageId>, pages: &[Page]) {
        let Some(target) = target else {
            return;
        };
        let page = find_page(pages, target.clone());
        let initial = page.and_then(|page| {
            page.initial
                .filter(|id| page.selectable_rows().any(|row| row.id == *id))
                .or_else(|| page.selectable_rows().next().map(|row| row.id))
        });
        let index = page
            .zip(initial)
            .and_then(|(page, id)| page.selectable_rows().position(|row| row.id == id))
            .unwrap_or(0);
        self.stack.push(target);
        self.selected.push(initial.unwrap_or(RowId::new(0)));
        self.last_index.push(index);
    }
}

fn find_page(pages: &[Page], id: PageId) -> Option<&Page> {
    pages.iter().find(|page| page.id == id)
}

fn scene_index(id: RowId) -> Option<usize> {
    let index = id.0.checked_sub(ids::SCENE_BASE)?;
    (index < visuals::SCENES.len() as u64).then_some(index as usize)
}

/// Inverse of `ids::tag`; only tag checkbox ids round-trip here.
pub fn tag_index(id: RowId) -> Option<usize> {
    let index = id.0.checked_sub(ids::TAG_BASE)?;
    (index < visuals::TAGS.len() as u64).then_some(index as usize)
}

const FILTER_MODES: &[&str] = &["All", "Favourites", "Tags"];
const AUTO_MODES: &[&str] = &["Off", "Phrases", "Timer"];

const TRANSITION_SPEC: NumSpec = NumSpec {
    min: 5.0,
    max: 300.0,
    step: 1.0,
    decimals: 0,
};

/// Maps an auto-advance choice's display text to the persisted config string.
pub fn auto_mode_to_config(display: &str) -> &'static str {
    match display {
        "Phrases" => "phrases",
        "Timer" => "timer",
        _ => "off",
    }
}

/// Maps the persisted auto-advance mode to its choice display text.
pub fn auto_mode_display(config_value: &str) -> &'static str {
    match config_value {
        "phrases" => "Phrases",
        "timer" => "Timer",
        _ => "Off",
    }
}

/// Maps an "Advance within" choice's display text to the persisted value.
pub fn within_to_config(display: &str) -> &'static str {
    match display {
        "Favourites" => "favourites",
        "Playlist" => "playlist",
        _ => "all",
    }
}

/// Maps the persisted "Advance within" value to its choice display text.
pub fn within_display(config_value: &str) -> &'static str {
    match config_value {
        "favourites" => "Favourites",
        "playlist" => "Playlist",
        _ => "All",
    }
}

/// Maps a filter choice's display text to the persisted config string.
pub fn filter_mode_to_config(display: &str) -> &'static str {
    match display {
        "Favourites" => "favourites",
        "Tags" => "tags",
        _ => "all",
    }
}

/// Maps the persisted filter string to its choice display text.
pub fn filter_mode_display(config_value: &str) -> &'static str {
    match config_value {
        "favourites" => "Favourites",
        "tags" => "Tags",
        _ => "All",
    }
}

const SOURCES: &[&str] = &["auto", "pipewire", "pulse", "microphone", "command", "file"];
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

/// Session data that the Audio page shows beyond the config.
pub struct AudioView<'a> {
    pub devices: &'a [audio::AudioDevice],
    pub now_playing: Option<&'a str>,
    pub state: &'a audio::AudioState,
}

/// Build all pages from the current config, scene, and session state.
#[cfg(test)]
pub fn build_pages(
    config: &config::Config,
    scene: usize,
    demo: bool,
    active_playlist: &str,
    search: &str,
) -> Vec<Page> {
    build_pages_with_audio(
        config,
        scene,
        demo,
        active_playlist,
        search,
        &AudioView {
            devices: &[],
            now_playing: None,
            state: &audio::AudioState::default(),
        },
    )
}

/// Build all pages with the audio devices, now-playing text, and live capture
/// state used by the Audio page.
pub fn build_pages_with_audio(
    config: &config::Config,
    scene: usize,
    demo: bool,
    active_playlist: &str,
    search: &str,
    audio_view: &AudioView<'_>,
) -> Vec<Page> {
    let scene_name = visuals::SCENES.get(scene).copied().unwrap_or_default();
    let mut pages = vec![
        build_root(),
        build_scenes(
            &config.auto_advance,
            &config.scenes,
            scene_name,
            &config.playlists,
            active_playlist,
        ),
        build_scene_picker(&config.scenes, scene, search),
        build_filter(&config.scenes),
        build_tags(&config.scenes),
        build_audio(
            &config.audio,
            demo,
            audio_view.devices,
            audio_view.now_playing,
            audio_view.state,
        ),
        build_appearance(&config.appearance),
        build_controls(),
    ];
    pages.push(build_playlists(&config.playlists, active_playlist));
    for playlist in &config.playlists {
        pages.push(build_playlist(playlist, scene, active_playlist));
        pages.push(build_confirm_delete(playlist));
    }
    pages
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
            Row::submenu(ids::CONTROLS, "Controls", "", PageId::Controls),
        ],
        initial: None,
    }
}

fn filter_summary(scenes: &config::ScenesConfig) -> String {
    match scenes.filter.as_str() {
        "favourites" => "Favourites".to_owned(),
        "tags" => format!("Tags ({})", scenes.filter_tags.len()),
        _ => "All".to_owned(),
    }
}

fn playlist_summary(playlists: &[config::Playlist], active: &str) -> String {
    if !active.is_empty() {
        return format!("active: {active}");
    }
    match playlists.len() {
        0 => "none".to_owned(),
        1 => "1 playlist".to_owned(),
        count => format!("{count} playlists"),
    }
}

fn build_scenes(
    auto: &config::AutoAdvanceConfig,
    scenes: &config::ScenesConfig,
    scene_name: &str,
    playlists: &[config::Playlist],
    active: &str,
) -> Page {
    let mut rows = vec![
        Row::submenu(
            ids::CURRENT_SCENE,
            "Visualisation",
            scene_name,
            PageId::ScenePicker,
        ),
        Row::submenu(
            ids::FILTER,
            "Filter…",
            &filter_summary(scenes),
            PageId::Filter,
        ),
        Row::submenu(
            ids::PLAYLISTS,
            "Playlists…",
            &playlist_summary(playlists, active),
            PageId::Playlists,
        ),
        Row::choice(
            ids::AUTO_MODE,
            "Auto-advance",
            AUTO_MODES,
            auto_mode_display(&auto.mode),
        ),
    ];
    if auto.mode != "off" {
        let timing = if auto.mode == "timer" {
            "Interval"
        } else {
            "Fallback after"
        };
        rows.push(Row::number_with_unit(
            ids::AUTO_TRANSITION,
            timing,
            auto.transition_secs,
            TRANSITION_SPEC,
            "s",
        ));
        let mut scopes = vec!["All"];
        if !scenes.favourites.is_empty() {
            scopes.push("Favourites");
        }
        if !active.is_empty() {
            scopes.push("Playlist");
        }
        if scopes.len() > 1 {
            rows.push(Row::choice(
                ids::AUTO_WITHIN,
                "Advance within",
                &scopes,
                within_display(&auto.within),
            ));
        }
    }
    Page {
        id: PageId::Scenes,
        title: "Scenes".into(),
        path: vec!["Vibe".into(), "Scenes".into()],
        rows,
        initial: None,
    }
}

fn build_playlists(playlists: &[config::Playlist], active: &str) -> Page {
    let mut rows = vec![Row::action(ids::PLAYLISTS_NEW, "New playlist")];
    if playlists.is_empty() {
        rows.push(Row::readonly(ids::PLAYLISTS_EMPTY, "No playlists yet", ""));
    } else {
        for playlist in playlists {
            let label = if playlist.name == active {
                format!("* {}", playlist.name)
            } else {
                playlist.name.clone()
            };
            rows.push(Row::submenu(
                ids::playlist(&playlist.name),
                &label,
                &format!("{} scenes", playlist.scenes.len()),
                PageId::Playlist(playlist.name.clone()),
            ));
        }
    }
    Page {
        id: PageId::Playlists,
        title: "Playlists".into(),
        path: vec!["Vibe".into(), "Scenes".into(), "Playlists".into()],
        rows,
        initial: None,
    }
}

fn build_playlist(playlist: &config::Playlist, scene: usize, active: &str) -> Page {
    let mut rows = vec![
        Row::text(ids::PLAYLIST_NAME, "Name", &playlist.name),
        Row::checkbox(ids::PLAYLIST_ACTIVE, "Active", playlist.name == active),
    ];
    for (index, scene_name) in playlist.scenes.iter().enumerate() {
        let mut row = Row::action(
            ids::playlist_scene(&playlist.name, scene_name),
            &format!("{}. {scene_name}", index + 1),
        );
        row.value = scene_name.clone();
        rows.push(row);
    }
    let current = visuals::SCENES.get(scene).copied().unwrap_or_default();
    if !playlist
        .scenes
        .iter()
        .any(|name| name.eq_ignore_ascii_case(current))
    {
        rows.push(Row::action(ids::PLAYLIST_ADD_CURRENT, "Add current scene"));
    }
    if !playlist.scenes.is_empty() {
        rows.push(Row::action(ids::PLAYLIST_REMOVE, "Remove selected"));
    }
    if playlist.scenes.len() > 1 {
        rows.push(Row::action(ids::PLAYLIST_UP, "Move up"));
        rows.push(Row::action(ids::PLAYLIST_DOWN, "Move down"));
    }
    rows.push(Row::submenu(
        ids::PLAYLIST_DELETE,
        "Delete playlist…",
        "",
        PageId::ConfirmDelete(playlist.name.clone()),
    ));
    Page {
        id: PageId::Playlist(playlist.name.clone()),
        title: playlist.name.clone(),
        path: vec![
            "Vibe".into(),
            "Scenes".into(),
            "Playlists".into(),
            playlist.name.clone(),
        ],
        rows,
        initial: Some(ids::PLAYLIST_NAME),
    }
}

fn build_confirm_delete(playlist: &config::Playlist) -> Page {
    Page {
        id: PageId::ConfirmDelete(playlist.name.clone()),
        title: "Delete".into(),
        path: vec![
            "Vibe".into(),
            "Scenes".into(),
            "Playlists".into(),
            playlist.name.clone(),
            "Delete".into(),
        ],
        rows: vec![
            Row::header(
                ids::CONFIRM_PROMPT,
                &format!("Delete \"{}\"?", playlist.name),
            ),
            Row::action(ids::CONFIRM_DELETE, "Delete"),
            Row::action(ids::CONFIRM_CANCEL, "Cancel"),
        ],
        initial: Some(ids::CONFIRM_CANCEL),
    }
}

/// Whether a scene passes the configured filter (mode and tags only).
fn scene_filtered(scenes: &config::ScenesConfig, scene: usize) -> bool {
    let name = visuals::SCENES[scene];
    match scenes.filter.as_str() {
        "favourites" => scenes
            .favourites
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name)),
        "tags" => {
            scenes.filter_tags.is_empty()
                || visuals::tags(scene).iter().any(|tag| {
                    scenes
                        .filter_tags
                        .iter()
                        .any(|f| f.eq_ignore_ascii_case(tag))
                })
        }
        _ => true,
    }
}

/// Whether a scene passes the filter and the picker's search query.
fn scene_matches(scenes: &config::ScenesConfig, scene: usize, search: &str) -> bool {
    let name = visuals::SCENES[scene];
    let search_ok = search.is_empty() || name.to_lowercase().contains(&search.to_lowercase());
    search_ok && scene_filtered(scenes, scene)
}

fn build_scene_picker(scenes: &config::ScenesConfig, scene: usize, search: &str) -> Page {
    let filtered: Vec<usize> = (0..visuals::SCENES.len())
        .filter(|&index| scene_matches(scenes, index, search))
        .collect();
    let mut rows = vec![Row::text(ids::SEARCH, "Search", search)];
    if filtered.is_empty() {
        rows.push(Row::readonly(ids::NO_MATCHES, "No scenes match", ""));
    } else {
        for &index in &filtered {
            let name = visuals::SCENES[index];
            let mut label = String::new();
            if index == scene {
                label.push_str("> ");
            }
            if scenes
                .favourites
                .iter()
                .any(|f| f.eq_ignore_ascii_case(name))
            {
                label.push_str("* ");
            }
            label.push_str(name);
            rows.push(Row::action(ids::scene(index), &label));
        }
    }
    let initial = if filtered.contains(&scene) {
        Some(ids::scene(scene))
    } else {
        filtered.first().map(|&index| ids::scene(index))
    };
    Page {
        id: PageId::ScenePicker,
        title: "Visualisation".into(),
        path: vec!["Vibe".into(), "Scenes".into(), "Visualisation".into()],
        rows,
        initial,
    }
}

fn build_filter(scenes: &config::ScenesConfig) -> Page {
    let mut rows = vec![Row::choice(
        ids::FILTER_MODE,
        "Mode",
        FILTER_MODES,
        filter_mode_display(&scenes.filter),
    )];
    if scenes.filter == "tags" {
        rows.push(Row::submenu(
            ids::FILTER_TAGS,
            "Tags",
            &scenes.filter_tags.len().to_string(),
            PageId::Tags,
        ));
    }
    let matches = (0..visuals::SCENES.len())
        .filter(|&scene| scene_filtered(scenes, scene))
        .count();
    rows.push(Row::readonly(
        ids::FILTER_MATCHES,
        "Matches",
        &matches.to_string(),
    ));
    Page {
        id: PageId::Filter,
        title: "Filter".into(),
        path: vec!["Vibe".into(), "Scenes".into(), "Filter".into()],
        rows,
        initial: None,
    }
}

fn build_tags(scenes: &config::ScenesConfig) -> Page {
    let mut rows: Vec<Row> = visuals::TAGS
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let on = scenes
                .filter_tags
                .iter()
                .any(|selected| selected.eq_ignore_ascii_case(tag));
            Row::checkbox(ids::tag(index), tag, on)
        })
        .collect();
    let mut clear = Row::action(ids::TAGS_CLEAR, "Clear tags");
    clear.visible = !scenes.filter_tags.is_empty();
    rows.push(clear);
    Page {
        id: PageId::Tags,
        title: "Tags".into(),
        path: vec![
            "Vibe".into(),
            "Scenes".into(),
            "Filter".into(),
            "Tags".into(),
        ],
        rows,
        initial: None,
    }
}

/// The kind of device a source targets: a sink for output monitors, a source
/// for the microphone, and nothing for command/file inputs.
pub fn target_kind(source: &str) -> Option<&'static str> {
    match source {
        "auto" | "pipewire" | "pulse" => Some("sink"),
        "microphone" => Some("source"),
        _ => None,
    }
}

fn connection_label(state: &audio::AudioState) -> String {
    if let Some(error) = state.error.as_deref().filter(|error| !error.is_empty()) {
        format!("error: {error}")
    } else if state.connected {
        "connected".to_owned()
    } else {
        "waiting for samples".to_owned()
    }
}

fn build_audio(
    audio: &config::AudioConfig,
    demo: bool,
    devices: &[audio::AudioDevice],
    now_playing: Option<&str>,
    state: &audio::AudioState,
) -> Page {
    let mut rows = vec![Row::toggle(ids::DEMO, "Demo mode", demo)];
    if !demo {
        rows.push(Row::readonly(
            ids::AUDIO_STATUS,
            "Status",
            &connection_label(state),
        ));
        if let Some(now_playing) = now_playing.filter(|text| !text.is_empty()) {
            rows.push(Row::readonly(ids::NOW_PLAYING, "Now playing", now_playing));
        }
        rows.push(Row::choice(ids::SOURCE, "Source", SOURCES, &audio.source));
        if let Some(kind) = target_kind(&audio.source) {
            let mut targets = vec!["System default"];
            for device in devices.iter().filter(|device| device.kind == kind) {
                let name = device.name.as_str();
                if !targets.contains(&name) {
                    targets.push(name);
                }
            }
            let target = if audio.target.is_empty() {
                "System default"
            } else {
                audio.target.as_str()
            };
            if target != "System default" && !targets.contains(&target) {
                targets.push(target);
            }
            rows.push(Row::choice(ids::TARGET, "Target", &targets, target));
            rows.push(Row::action(ids::DEVICES_REFRESH, "Refresh devices"));
        }
        rows.push(Row::choice(ids::FORMAT, "Format", FORMATS, &audio.format));
        rows.push(Row::number(ids::RATE, "Rate", audio.rate as f32, RATE_SPEC));
        rows.push(Row::number(
            ids::CHANNELS,
            "Channels",
            audio.channels as f32,
            CHANNELS_SPEC,
        ));
        if target_kind(&audio.source).is_some() {
            rows.push(Row::toggle(
                ids::AUTO_RECONNECT,
                "Reconnect",
                audio.auto_reconnect,
            ));
        }
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
        build_pages(&config::Config::default(), 0, demo, "", "")
    }

    fn title(menu: &Menu, pages: &[Page]) -> String {
        menu.view(pages).path.last().cloned().unwrap_or_default()
    }

    fn editing(menu: &Menu, pages: &[Page]) -> bool {
        menu.view(pages).rows.iter().any(|row| row.editing)
    }

    #[test]
    fn root_page_lists_the_four_top_level_entries() {
        let pages = default_pages(false);
        let root = find_page(&pages, PageId::Root).unwrap();
        let labels: Vec<&str> = root.visible_rows().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["Scenes", "Audio", "Appearance", "Controls"]);
    }

    #[test]
    fn scene_picker_lists_every_scene_with_search_and_starts_on_current() {
        let pages = build_pages(&config::Config::default(), 7, false, "", "");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        assert_eq!(picker.rows.len(), visuals::SCENES.len() + 1);
        assert_eq!(picker.rows[0].id, ids::SEARCH);
        assert_eq!(picker.initial, Some(ids::scene(7)));
    }

    #[test]
    fn scene_picker_filters_by_mode_favourites_tags_and_search() {
        // Favourites mode keeps only favourited scenes.
        let mut config = config::Config::default();
        config.scenes.filter = "favourites".into();
        config.scenes.favourites = vec!["Julia".into(), "Missing Scene".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        let labels: Vec<&str> = picker
            .visible_rows()
            .filter(|row| row.id != ids::SEARCH)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, ["* Julia"]);

        // Tags mode with no tags behaves as All.
        config.scenes.filter = "tags".into();
        config.scenes.filter_tags.clear();
        config.scenes.favourites.clear();
        let pages = build_pages(&config, 0, false, "", "");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        assert_eq!(picker.rows.len(), visuals::SCENES.len() + 1);

        // Tags mode matches any selected tag; search narrows case-insensitively.
        config.scenes.filter_tags = vec!["fractal".into()];
        let pages = build_pages(&config, 0, false, "", "julia");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        let labels: Vec<&str> = picker
            .visible_rows()
            .filter(|row| row.id != ids::SEARCH)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, ["Julia"]);

        // A search with no matches shows the read-only placeholder row.
        let pages = build_pages(&config, 0, false, "", "zzzz");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        assert!(picker.visible_rows().any(|row| row.id == ids::NO_MATCHES));
    }

    #[test]
    fn every_scene_is_listed_and_findable_by_name() {
        let config = config::Config::default();
        let pages = build_pages(&config, 0, false, "", "");
        let picker = find_page(&pages, PageId::ScenePicker).unwrap();
        let listed: Vec<usize> = picker
            .visible_rows()
            .filter_map(|row| scene_index(row.id))
            .collect();
        assert_eq!(listed, (0..visuals::SCENES.len()).collect::<Vec<_>>());

        for (index, name) in visuals::SCENES.iter().enumerate() {
            for query in [(*name).to_owned(), name.to_lowercase()] {
                let pages = build_pages(&config, 0, false, "", &query);
                let picker = find_page(&pages, PageId::ScenePicker).unwrap();
                let found: Vec<usize> = picker
                    .visible_rows()
                    .filter_map(|row| scene_index(row.id))
                    .collect();
                assert!(found.contains(&index), "{name} not found by {query:?}");
            }
        }
    }

    #[test]
    fn every_tag_matches_scenes_and_every_scene_carries_a_tag() {
        let mut config = config::Config::default();
        config.scenes.filter = "tags".into();
        let mut covered = vec![false; visuals::SCENES.len()];
        for tag in visuals::TAGS {
            config.scenes.filter_tags = vec![(*tag).to_owned()];
            let pages = build_pages(&config, 0, false, "", "");
            let picker = find_page(&pages, PageId::ScenePicker).unwrap();
            let found: Vec<usize> = picker
                .visible_rows()
                .filter_map(|row| scene_index(row.id))
                .collect();
            assert!(!found.is_empty(), "tag {tag} matches no scene");
            for index in found {
                covered[index] = true;
            }
        }
        let uncovered: Vec<&str> = covered
            .iter()
            .enumerate()
            .filter(|(_, seen)| !**seen)
            .map(|(index, _)| visuals::SCENES[index])
            .collect();
        assert!(uncovered.is_empty(), "no tag filter reaches {uncovered:?}");
    }

    #[test]
    fn filter_page_reports_mode_tag_count_and_matches() {
        let mut config = config::Config::default();
        config.scenes.filter = "tags".into();
        config.scenes.filter_tags = vec!["fractal".into(), "particles".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let filter = find_page(&pages, PageId::Filter).unwrap();
        let tags = filter
            .visible_rows()
            .find(|row| row.id == ids::FILTER_TAGS)
            .unwrap();
        assert_eq!(tags.value, "2");
        let matches = filter
            .visible_rows()
            .find(|row| row.id == ids::FILTER_MATCHES)
            .unwrap();
        let expected = (0..visuals::SCENES.len())
            .filter(|&scene| scene_filtered(&config.scenes, scene))
            .count();
        assert_eq!(matches.value, expected.to_string());

        // Tags submenu is hidden outside tags mode.
        config.scenes.filter = "all".into();
        let pages = build_pages(&config, 0, false, "", "");
        let filter = find_page(&pages, PageId::Filter).unwrap();
        assert!(!filter.visible_rows().any(|row| row.id == ids::FILTER_TAGS));
    }

    #[test]
    fn tags_page_has_a_checkbox_per_tag_and_clear_when_selected() {
        let mut config = config::Config::default();
        let pages = build_pages(&config, 0, false, "", "");
        let tags = find_page(&pages, PageId::Tags).unwrap();
        assert_eq!(
            tags.visible_rows().count(),
            visuals::TAGS.len(),
            "Clear tags is hidden until a tag is selected"
        );
        assert!(tags.visible_rows().all(|row| row.kind == RowKind::Checkbox));

        config.scenes.filter_tags = vec!["calm".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let tags = find_page(&pages, PageId::Tags).unwrap();
        let clear = tags.visible_rows().find(|row| row.id == ids::TAGS_CLEAR);
        assert!(clear.is_some());
        assert_eq!(clear.unwrap().kind, RowKind::Action);
    }

    #[test]
    fn audio_page_hides_rows_that_do_not_apply_to_the_source() {
        let mut config = config::Config::default();
        config.audio.source = "command".into();
        let pages = build_pages(&config, 0, false, "", "");
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let visible: Vec<RowId> = audio.visible_rows().map(|row| row.id).collect();
        assert!(visible.contains(&ids::SOURCE));
        assert!(visible.contains(&ids::COMMAND));
        assert!(!visible.contains(&ids::TARGET));
        assert!(!visible.contains(&ids::FILE));
        assert!(!visible.contains(&ids::LOOP_FILE));

        let mut config = config::Config::default();
        config.audio.source = "file".into();
        let pages = build_pages(&config, 0, false, "", "");
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
        let pages = build_pages(&config, 0, false, "", "");
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
        let pages = build_pages(&config, 0, false, "", "");
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
        let pages = build_pages(&config, 0, false, "", "");
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        for _ in 0..6 {
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
        let pages = build_pages(&config, 0, false, "", "");
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Enter), &pages);
        for _ in 0..6 {
            menu.handle_key(press(KeyCode::Down), &pages);
        }
        assert_eq!(menu.view(&pages).rows[6].label, "Command");

        config.audio.source = "file".into();
        let pages = build_pages(&config, 0, false, "", "");
        menu.rebuild(&pages);
        let view = menu.view(&pages);
        assert_eq!(title(&menu, &pages), "Audio");
        assert!(view.selected < view.rows.len());
        assert_eq!(view.rows[view.selected].label, "File");
    }

    fn config_with_playlists() -> config::Config {
        let mut config = config::Config::default();
        config.playlists = vec![
            config::Playlist {
                name: "Chill".into(),
                scenes: vec!["Julia".into(), "Waveform Scope".into()],
            },
            config::Playlist {
                name: "Empty".into(),
                scenes: vec![],
            },
        ];
        config
    }

    #[test]
    fn playlists_page_lists_playlists_and_the_empty_state() {
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "Chill", "");
        let playlists = find_page(&pages, PageId::Playlists).unwrap();
        let labels: Vec<&str> = playlists
            .visible_rows()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, ["New playlist", "* Chill", "Empty"]);
        let values: Vec<&str> = playlists
            .visible_rows()
            .map(|row| row.value.as_str())
            .collect();
        assert_eq!(values[1], "2 scenes");
        assert_eq!(values[2], "0 scenes");

        let empty = build_pages(&config::Config::default(), 0, false, "", "");
        let playlists = find_page(&empty, PageId::Playlists).unwrap();
        let labels: Vec<&str> = playlists
            .visible_rows()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, ["New playlist", "No playlists yet"]);
    }

    #[test]
    fn playlist_page_has_name_active_scenes_and_actions() {
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "Chill", "");
        let page = find_page(&pages, PageId::Playlist("Chill".into())).unwrap();
        let labels: Vec<&str> = page.visible_rows().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Name",
                "Active",
                "1. Julia",
                "2. Waveform Scope",
                "Add current scene",
                "Remove selected",
                "Move up",
                "Move down",
                "Delete playlist…",
            ]
        );
        let active = page
            .visible_rows()
            .find(|row| row.id == ids::PLAYLIST_ACTIVE)
            .unwrap();
        assert_eq!(active.value, "on");
        let scene_rows: Vec<&Row> = page
            .visible_rows()
            .filter(|row| row.kind == RowKind::Action && !row.value.is_empty())
            .collect();
        assert_eq!(scene_rows.len(), 2);
        assert_eq!(scene_rows[0].id, ids::playlist_scene("Chill", "Julia"));
        assert_eq!(scene_rows[1].value, "Waveform Scope");
    }

    #[test]
    fn confirm_delete_page_prompts_and_offers_delete_and_cancel() {
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "", "");
        let page = find_page(&pages, PageId::ConfirmDelete("Chill".into())).unwrap();
        let labels: Vec<&str> = page.visible_rows().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["Delete \"Chill\"?", "Delete", "Cancel"]);
        assert!(page.visible_rows().any(|row| row.id == ids::CONFIRM_DELETE));
        assert!(page.visible_rows().any(|row| row.id == ids::CONFIRM_CANCEL));
        assert!(
            !page
                .visible_rows()
                .find(|row| row.id == ids::CONFIRM_PROMPT)
                .unwrap()
                .selectable
        );

        let mut menu = Menu::new();
        menu.rebuild(&pages);
        menu.descend(Some(PageId::ConfirmDelete("Chill".into())), &pages);
        let view = menu.view(&pages);
        assert_eq!(view.rows[view.selected].label, "Cancel");
        menu.handle_key(press(KeyCode::Up), &pages);
        let view = menu.view(&pages);
        assert_eq!(view.rows[view.selected].label, "Delete");
    }

    #[test]
    fn playlist_scene_selection_maps_back_to_index_and_follows_reorder() {
        let mut menu = Menu::new();
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "Chill", "");
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Enter), &pages); // Scenes
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Down), &pages); // Playlists…
        menu.handle_key(press(KeyCode::Enter), &pages); // Playlists
        menu.handle_key(press(KeyCode::Down), &pages); // * Chill
        menu.handle_key(press(KeyCode::Enter), &pages); // Chill page
        assert_eq!(menu.current_playlist(), Some("Chill"));
        menu.handle_key(press(KeyCode::Down), &pages); // Active
        menu.handle_key(press(KeyCode::Down), &pages); // 1. Julia
        assert_eq!(menu.selected_playlist_scene(&pages), Some(0));
        assert_eq!(
            menu.handle_key(press(KeyCode::Right), &pages),
            Effect::MoveSelectedScene(1)
        );
        // The row id is keyed by scene name, so the selection still points at
        // Julia even though the app has not reordered the page here.
        assert_eq!(menu.selected_playlist_scene(&pages), Some(0));
    }

    #[test]
    fn rename_page_updates_every_page_in_the_stack() {
        let mut menu = Menu::new();
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "", "");
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Enter), &pages); // Scenes
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Down), &pages); // Playlists…
        menu.handle_key(press(KeyCode::Enter), &pages); // Playlists
        menu.handle_key(press(KeyCode::Down), &pages); // Chill
        menu.handle_key(press(KeyCode::Enter), &pages); // Chill page
        menu.rename_page(
            PageId::Playlist("Chill".into()),
            PageId::Playlist("Relax".into()),
        );
        assert_eq!(menu.current_playlist(), Some("Relax"));
    }

    #[test]
    fn back_to_and_open_page_manage_the_stack() {
        let mut menu = Menu::new();
        let config = config_with_playlists();
        let pages = build_pages(&config, 0, false, "", "");
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Enter), &pages); // Scenes
        menu.handle_key(press(KeyCode::Down), &pages);
        menu.handle_key(press(KeyCode::Down), &pages); // Playlists…
        menu.handle_key(press(KeyCode::Enter), &pages); // Playlists
        menu.handle_key(press(KeyCode::Down), &pages); // Chill
        menu.handle_key(press(KeyCode::Enter), &pages); // Chill page
        menu.back_to(PageId::Playlists);
        assert_eq!(menu.stack.last().cloned(), Some(PageId::Playlists));

        menu.open_page(PageId::Playlist("Empty".into()));
        assert_eq!(
            menu.stack.last().cloned(),
            Some(PageId::Playlist("Empty".into()))
        );
    }

    #[test]
    fn auto_advance_details_are_hidden_while_off_and_named_by_mode() {
        let config = config::Config::default();
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        let labels: Vec<&str> = scenes
            .visible_rows()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(
            labels,
            ["Visualisation", "Filter…", "Playlists…", "Auto-advance"]
        );
        assert_eq!(
            scenes
                .visible_rows()
                .find(|row| row.id == ids::AUTO_MODE)
                .unwrap()
                .value,
            "Off"
        );

        let mut config = config::Config::default();
        config.auto_advance.mode = "timer".into();
        config.auto_advance.transition_secs = 20.0;
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        let labels: Vec<&str> = scenes
            .visible_rows()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Visualisation",
                "Filter…",
                "Playlists…",
                "Auto-advance",
                "Interval",
            ]
        );
        let transition = scenes
            .visible_rows()
            .find(|row| row.id == ids::AUTO_TRANSITION)
            .unwrap();
        assert_eq!(transition.value, "20");
        assert_eq!(transition.unit, Some("s"));
        assert!(!scenes.visible_rows().any(|row| row.id == ids::AUTO_WITHIN));

        // Phrases use the same value as a fallback, not an interval.
        config.auto_advance.mode = "phrases".into();
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        let labels: Vec<&str> = scenes
            .visible_rows()
            .map(|row| row.label.as_str())
            .collect();
        assert!(labels.contains(&"Fallback after"));
        assert!(!labels.contains(&"Interval"));

        // The scope row appears once there is somewhere else to advance within.
        config.scenes.favourites = vec!["Julia".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        assert!(scenes.visible_rows().any(|row| row.id == ids::AUTO_WITHIN));
    }

    #[test]
    fn auto_advance_scopes_only_offer_available_targets() {
        let mut config = config::Config::default();
        config.auto_advance.mode = "timer".into();
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        assert!(!scenes.visible_rows().any(|row| row.id == ids::AUTO_WITHIN));

        config.scenes.favourites = vec!["Julia".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        let within = scenes
            .visible_rows()
            .find(|row| row.id == ids::AUTO_WITHIN)
            .unwrap();
        assert_eq!(within.choices, ["All", "Favourites"]);

        config.playlists = vec![config::Playlist {
            name: "Chill".into(),
            scenes: vec![],
        }];
        let pages = build_pages(&config, 0, false, "Chill", "");
        let scenes = find_page(&pages, PageId::Scenes).unwrap();
        let within = scenes
            .visible_rows()
            .find(|row| row.id == ids::AUTO_WITHIN)
            .unwrap();
        assert_eq!(within.choices, ["All", "Favourites", "Playlist"]);
    }

    #[test]
    fn advance_within_cycles_when_a_scope_exists() {
        let mut config = config::Config::default();
        config.auto_advance.mode = "timer".into();
        config.scenes.favourites = vec!["Julia".into()];
        let pages = build_pages(&config, 0, false, "", "");
        let mut menu = Menu::new();
        menu.rebuild(&pages);
        menu.handle_key(press(KeyCode::Enter), &pages); // Scenes
        for _ in 0..5 {
            menu.handle_key(press(KeyCode::Down), &pages); // Advance within
        }
        assert_eq!(
            menu.handle_key(press(KeyCode::Right), &pages),
            Effect::Changed {
                id: ids::AUTO_WITHIN,
                value: "Favourites".into()
            }
        );
    }

    fn audio_pages(config: &config::Config, state: &audio::AudioState) -> Vec<Page> {
        build_pages_with_audio(
            config,
            0,
            false,
            "",
            "",
            &AudioView {
                devices: &[],
                now_playing: None,
                state,
            },
        )
    }

    fn audio_device(kind: &'static str, name: &str) -> audio::AudioDevice {
        audio::AudioDevice {
            kind,
            name: name.into(),
            description: name.into(),
            serial: None,
            channels: None,
            rate: None,
            default: false,
        }
    }

    #[test]
    fn audio_page_offers_microphone_targets_and_now_playing() {
        let devices = vec![
            audio_device("sink", "sink.analog"),
            audio_device("source", "source.mic"),
        ];
        let mut config = config::Config::default();
        config.audio.target = "sink.analog".into();
        let state = audio::AudioState::default();
        let pages = build_pages_with_audio(
            &config,
            0,
            false,
            "",
            "",
            &AudioView {
                devices: &devices,
                now_playing: Some("A - B"),
                state: &state,
            },
        );
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let source = audio
            .visible_rows()
            .find(|row| row.id == ids::SOURCE)
            .unwrap();
        assert!(source.choices.iter().any(|choice| choice == "microphone"));
        let target = audio
            .visible_rows()
            .find(|row| row.id == ids::TARGET)
            .unwrap();
        assert_eq!(target.kind, RowKind::Choice);
        assert_eq!(target.choices, ["System default", "sink.analog"]);
        assert_eq!(target.value, "sink.analog");
        assert_eq!(
            audio
                .visible_rows()
                .find(|row| row.id == ids::NOW_PLAYING)
                .unwrap()
                .value,
            "A - B"
        );
        assert!(audio
            .visible_rows()
            .any(|row| row.id == ids::DEVICES_REFRESH));
        assert_eq!(
            audio
                .visible_rows()
                .find(|row| row.id == ids::AUTO_RECONNECT)
                .unwrap()
                .value,
            "on"
        );

        config.audio.target.clear();
        config.audio.source = "microphone".into();
        let pages = build_pages_with_audio(
            &config,
            0,
            false,
            "",
            "",
            &AudioView {
                devices: &devices,
                now_playing: None,
                state: &state,
            },
        );
        let audio = find_page(&pages, PageId::Audio).unwrap();
        let target = audio
            .visible_rows()
            .find(|row| row.id == ids::TARGET)
            .unwrap();
        assert_eq!(target.choices, ["System default", "source.mic"]);
        assert_eq!(target.value, "System default");
        assert!(!audio.visible_rows().any(|row| row.id == ids::NOW_PLAYING));
    }

    #[test]
    fn audio_page_reports_capture_state_in_a_row() {
        let config = config::Config::default();
        let mut state = audio::AudioState::default();
        let pages = audio_pages(&config, &state);
        let audio_page = find_page(&pages, PageId::Audio).unwrap();
        let status = audio_page
            .visible_rows()
            .find(|row| row.id == ids::AUDIO_STATUS)
            .unwrap();
        assert_eq!(status.kind, RowKind::ReadOnly);
        assert_eq!(status.value, "waiting for samples");

        state.connected = true;
        let pages = audio_pages(&config, &state);
        let audio_page = find_page(&pages, PageId::Audio).unwrap();
        let status = audio_page
            .visible_rows()
            .find(|row| row.id == ids::AUDIO_STATUS)
            .unwrap();
        assert_eq!(status.value, "connected");

        state.error = Some("pipewire exited".into());
        let pages = audio_pages(&config, &state);
        let audio_page = find_page(&pages, PageId::Audio).unwrap();
        let status = audio_page
            .visible_rows()
            .find(|row| row.id == ids::AUDIO_STATUS)
            .unwrap();
        assert_eq!(status.value, "error: pipewire exited");
    }

    #[test]
    fn command_and_file_sources_hide_reconnect_and_targets() {
        for source in ["command", "file"] {
            let mut config = config::Config::default();
            config.audio.source = source.into();
            let pages = audio_pages(&config, &audio::AudioState::default());
            let audio = find_page(&pages, PageId::Audio).unwrap();
            let visible: Vec<RowId> = audio.visible_rows().map(|row| row.id).collect();
            assert!(!visible.contains(&ids::TARGET));
            assert!(!visible.contains(&ids::DEVICES_REFRESH));
            assert!(!visible.contains(&ids::AUTO_RECONNECT));
            assert!(!visible.contains(&ids::NOW_PLAYING));
        }
    }
}
