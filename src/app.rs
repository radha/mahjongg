//! Application state: the current game plus menus, dialogs, settings and input.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::game::Game;
use crate::map::{Map, Slot, load_maps};
use crate::rng::{Rng, random_seed};
use crate::storage::{self, History, HistoryEntry, Progression, Settings};
use crate::theme::{Background, TileTheme};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    NewGame,
    RestartGame,
    Undo,
    Redo,
    Hint,
    TogglePause,
    Scores,
    OpenMenu,
    MenuActivate(usize),
    MenuBack,
    SetLayout(usize),
    ConfirmLayout(usize),
    SetProgression(Progression),
    SetBackground(Background),
    SetTheme(TileTheme),
    Rules,
    Shortcuts,
    About,
    Quit,
    Reshuffle,
    CloseDialog,
    ScoresLayout(isize),
    ClearScores,
    ConfirmClearScores,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPage {
    Main,
    Layout,
    Progression,
    Appearance,
}

pub struct Menu {
    pub page: MenuPage,
    pub index: usize,
}

pub enum MenuItem {
    Item {
        label: String,
        accel: &'static str,
        action: Action,
    },
    Submenu {
        label: &'static str,
        page: MenuPage,
    },
    Radio {
        label: String,
        checked: bool,
        action: Action,
    },
    Heading(&'static str),
    Separator,
}

impl MenuItem {
    pub fn selectable(&self) -> bool {
        !matches!(self, MenuItem::Heading(_) | MenuItem::Separator)
    }
}

#[derive(Clone, Debug)]
pub enum Dialog {
    NoMoves {
        can_shuffle: bool,
    },
    ChangeLayout(usize),
    Scores {
        layout: usize,
        completed: Option<HistoryEntry>,
    },
    ClearScores {
        layout: usize,
    },
    Rules,
    Shortcuts,
    About,
}

pub struct DialogState {
    pub kind: Dialog,
    pub focus: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Normal,
    Suggested,
    Destructive,
}

pub struct Button {
    pub label: &'static str,
    pub action: Action,
    pub kind: ButtonKind,
}

/// Screen placement of the board, recorded while drawing for mouse hit-testing.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// Terminal cells per half-tile unit.
    pub hw: i32,
    pub hh: i32,
    /// Offset per layer (right, up), which exposes the tile edges below.
    pub shift_x: i32,
    pub shift_y: i32,
    pub ox: i32,
    pub oy: i32,
    pub large: bool,
}

impl Geometry {
    pub fn tile_rect(&self, slot: Slot) -> (i32, i32, i32, i32) {
        (
            self.ox + slot.x * self.hw + slot.layer * self.shift_x,
            self.oy + slot.y * self.hh - slot.layer * self.shift_y,
            2 * self.hw,
            2 * self.hh,
        )
    }
}

pub struct App {
    pub maps: Vec<Map>,
    pub map_index: usize,
    pub game: Game,
    pub settings: Settings,
    pub history: History,
    pub menu: Option<Menu>,
    pub dialog: Option<DialogState>,
    pub restored: bool,
    pub cursor: Option<usize>,
    pub cursor_visible: bool,
    pub regions: Vec<(Rect, Action)>,
    pub geometry: Option<Geometry>,
    pub mouse: Option<(u16, u16)>,
    pub quit: bool,
    auto_paused: bool,
    recorded: bool,
    last_click: Option<(Instant, u16, u16)>,
    rng: Rng,
}

impl App {
    pub fn new() -> Self {
        let maps = load_maps();
        let settings = Settings::load();
        let history = History::load();
        let map_index = maps
            .iter()
            .position(|m| m.name == settings.layout)
            .unwrap_or(0);
        let game = Game::new(maps[map_index].clone());
        let mut app = App {
            maps,
            map_index,
            game,
            settings,
            history,
            menu: None,
            dialog: None,
            restored: false,
            cursor: None,
            cursor_visible: false,
            regions: vec![],
            geometry: None,
            mouse: None,
            quit: false,
            auto_paused: false,
            recorded: false,
            last_click: None,
            rng: Rng::new(random_seed()),
        };

        if let Some(save) = storage::load_save(&app.maps) {
            let index = app
                .maps
                .iter()
                .position(|m| m.name == save.map)
                .unwrap_or(0);
            app.map_index = index;
            app.game = Game::new(app.maps[index].clone());
            app.game
                .restore(save.seed, save.current_move, save.clock, &save.tiles);
            app.restored = app.game.is_valid_deal();
        }
        if !app.restored {
            let rotate = app.settings.progression == Progression::Random;
            app.new_game(rotate);
        }
        app
    }

    /// Save the game in progress (GNOME resumes it next launch).
    pub fn shutdown(&mut self) {
        if self.game.started() && !self.game.inspecting() && self.game.can_move() {
            storage::write_save(&self.game);
        } else {
            storage::delete_save();
            if self.game.inspecting() {
                let next = self.next_map_index();
                self.settings.layout = self.maps[next].name.clone();
            }
        }
        self.settings.save();
    }

    fn next_map_index(&mut self) -> usize {
        let current = self
            .maps
            .iter()
            .position(|m| m.name == self.settings.layout)
            .unwrap_or(0);
        match self.settings.progression {
            Progression::Single => current,
            Progression::Sequential => (current + 1) % self.maps.len(),
            Progression::Random => self.rng.range(0, self.maps.len()),
        }
    }

    pub fn new_game(&mut self, rotate: bool) {
        let index = if rotate {
            self.next_map_index()
        } else {
            self.maps
                .iter()
                .position(|m| m.name == self.settings.layout)
                .unwrap_or(0)
        };
        self.map_index = index;
        self.settings.layout = self.maps[index].name.clone();
        self.settings.save();
        self.game = Game::new(self.maps[index].clone());
        self.game.generate(None);
        storage::delete_save();
        self.reset_session();
    }

    fn restart_game(&mut self) {
        self.game.restart();
        storage::delete_save();
        self.reset_session();
    }

    fn reset_session(&mut self) {
        self.restored = false;
        self.recorded = false;
        self.auto_paused = false;
        self.menu = None;
        self.dialog = None;
        if self.cursor.is_some_and(|c| c >= self.game.tiles.len()) {
            self.cursor = None;
        }
        self.fix_cursor();
    }

    // ----- Overlays ------------------------------------------------------

    /// GNOME pauses the game while a menu or dialog is open.
    fn auto_pause(&mut self) {
        if !self.game.paused() {
            self.game.set_paused(true);
            self.auto_paused = self.game.paused();
        }
    }

    fn close_overlays(&mut self) {
        self.menu = None;
        self.dialog = None;
        if self.auto_paused {
            self.auto_paused = false;
            self.game.set_paused(false);
        }
    }

    fn open_dialog(&mut self, kind: Dialog) {
        if self.dialog.is_none() && self.menu.is_none() {
            self.auto_pause();
        }
        self.menu = None;
        let focus = Self::buttons_for(&kind, self)
            .iter()
            .position(|b| b.kind == ButtonKind::Suggested)
            .unwrap_or(0);
        self.dialog = Some(DialogState { kind, focus });
    }

    fn open_menu(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        if self.menu.is_some() {
            self.close_overlays();
            return;
        }
        self.auto_pause();
        self.menu = Some(Menu {
            page: MenuPage::Main,
            index: 0,
        });
    }

    pub fn pause_overlay_visible(&self) -> bool {
        self.game.paused() && self.menu.is_none() && self.dialog.is_none()
    }

    pub fn menu_items(&self, page: MenuPage) -> Vec<MenuItem> {
        use MenuItem::*;
        match page {
            MenuPage::Main => vec![
                Item {
                    label: "New Game".into(),
                    accel: "Ctrl+N",
                    action: Action::NewGame,
                },
                Item {
                    label: "Restart Game".into(),
                    accel: "Ctrl+R",
                    action: Action::RestartGame,
                },
                Item {
                    label: "Scores".into(),
                    accel: "S",
                    action: Action::Scores,
                },
                Separator,
                Submenu {
                    label: "Layout",
                    page: MenuPage::Layout,
                },
                Submenu {
                    label: "Layout Progression",
                    page: MenuPage::Progression,
                },
                Submenu {
                    label: "Appearance",
                    page: MenuPage::Appearance,
                },
                Separator,
                Item {
                    label: "Game Rules".into(),
                    accel: "F1",
                    action: Action::Rules,
                },
                Item {
                    label: "Keyboard Shortcuts".into(),
                    accel: "?",
                    action: Action::Shortcuts,
                },
                Item {
                    label: "About Mahjongg".into(),
                    accel: "",
                    action: Action::About,
                },
                Separator,
                Item {
                    label: "Quit".into(),
                    accel: "Ctrl+Q",
                    action: Action::Quit,
                },
            ],
            MenuPage::Layout => self
                .maps
                .iter()
                .enumerate()
                .map(|(i, m)| Radio {
                    label: m.name.clone(),
                    checked: m.name == self.settings.layout,
                    action: Action::SetLayout(i),
                })
                .collect(),
            MenuPage::Progression => Progression::ALL
                .into_iter()
                .map(|p| Radio {
                    label: p.name().into(),
                    checked: p == self.settings.progression,
                    action: Action::SetProgression(p),
                })
                .collect(),
            MenuPage::Appearance => {
                let mut items = vec![Heading("Background")];
                items.extend(Background::ALL.into_iter().map(|b| Radio {
                    label: b.name().into(),
                    checked: b == self.settings.background,
                    action: Action::SetBackground(b),
                }));
                items.push(Separator);
                items.push(Heading("Theme"));
                items.extend(TileTheme::ALL.into_iter().map(|t| Radio {
                    label: t.name().into(),
                    checked: t == self.settings.theme,
                    action: Action::SetTheme(t),
                }));
                items
            }
        }
    }

    pub fn menu_title(page: MenuPage) -> &'static str {
        match page {
            MenuPage::Main => "",
            MenuPage::Layout => "Layout",
            MenuPage::Progression => "Layout Progression",
            MenuPage::Appearance => "Appearance",
        }
    }

    pub fn buttons(&self) -> Vec<Button> {
        match &self.dialog {
            Some(d) => Self::buttons_for(&d.kind, self),
            None => vec![],
        }
    }

    fn buttons_for(kind: &Dialog, app: &App) -> Vec<Button> {
        use ButtonKind::*;
        let b = |label, action, kind| Button {
            label,
            action,
            kind,
        };
        match kind {
            Dialog::NoMoves { can_shuffle } => {
                let mut v = vec![
                    b("Quit", Action::Quit, Destructive),
                    b("New Game", Action::NewGame, Normal),
                ];
                if *can_shuffle {
                    v.push(b("Reshuffle", Action::Reshuffle, Normal));
                }
                v.push(b("Continue", Action::CloseDialog, Suggested));
                v
            }
            Dialog::ChangeLayout(i) => vec![
                b("Cancel", Action::CloseDialog, Suggested),
                b("Change Layout", Action::ConfirmLayout(*i), Destructive),
            ],
            Dialog::Scores {
                completed: Some(_), ..
            } => {
                vec![
                    b("Quit", Action::Quit, Normal),
                    b("New Game", Action::NewGame, Suggested),
                ]
            }
            Dialog::Scores { .. } => {
                let mut v = vec![];
                if !app.history.entries.is_empty() {
                    v.push(b("Clear Scores", Action::ClearScores, Destructive));
                }
                v.push(b("Close", Action::CloseDialog, Suggested));
                v
            }
            Dialog::ClearScores { .. } => vec![
                b("Cancel", Action::CloseDialog, Suggested),
                b("Clear All", Action::ConfirmClearScores, Destructive),
            ],
            Dialog::Rules | Dialog::Shortcuts | Dialog::About => {
                vec![b("Close", Action::CloseDialog, Suggested)]
            }
        }
    }

    // ----- Actions -------------------------------------------------------

    pub fn perform(&mut self, action: Action) {
        match action {
            Action::NewGame => {
                self.close_overlays();
                self.new_game(true);
            }
            Action::RestartGame => {
                self.close_overlays();
                self.restart_game();
            }
            Action::Undo => {
                if !self.game.paused() && self.game.can_undo() {
                    self.game.undo();
                    self.fix_cursor();
                    self.after_move();
                }
            }
            Action::Redo => {
                if !self.game.paused() && self.game.can_redo() {
                    self.game.redo();
                    self.fix_cursor();
                    self.after_move();
                }
            }
            Action::Hint => {
                if !self.game.paused() && self.game.moves_left() > 0 {
                    self.game.show_hint();
                }
            }
            Action::TogglePause => {
                if self.menu.is_none() && self.dialog.is_none() {
                    let paused = self.game.paused();
                    self.game.set_paused(!paused);
                    self.restored = false;
                }
            }
            Action::Scores => {
                let layout = self.map_index;
                self.open_dialog(Dialog::Scores {
                    layout,
                    completed: None,
                });
            }
            Action::OpenMenu => self.open_menu(),
            Action::MenuActivate(index) => self.menu_activate(index),
            Action::MenuBack => {
                if let Some(menu) = &mut self.menu {
                    let back_to = match menu.page {
                        MenuPage::Layout => 4,
                        MenuPage::Progression => 5,
                        MenuPage::Appearance => 6,
                        MenuPage::Main => 0,
                    };
                    menu.page = MenuPage::Main;
                    menu.index = back_to;
                }
            }
            Action::SetLayout(i) => {
                self.menu = None;
                if self.maps[i].name == self.settings.layout {
                    self.close_overlays();
                } else if self.game.started() && !self.game.inspecting() {
                    self.open_dialog(Dialog::ChangeLayout(i));
                } else {
                    self.close_overlays();
                    self.apply_layout(i);
                }
            }
            Action::ConfirmLayout(i) => {
                self.close_overlays();
                self.apply_layout(i);
            }
            Action::SetProgression(p) => {
                self.settings.progression = p;
                self.settings.save();
                self.close_overlays();
            }
            Action::SetBackground(b) => {
                self.settings.background = b;
                self.settings.save();
                self.close_overlays();
            }
            Action::SetTheme(t) => {
                self.settings.theme = t;
                self.settings.save();
                self.close_overlays();
            }
            Action::Rules => self.open_dialog(Dialog::Rules),
            Action::Shortcuts => self.open_dialog(Dialog::Shortcuts),
            Action::About => self.open_dialog(Dialog::About),
            Action::Quit => {
                self.close_overlays();
                self.quit = true;
            }
            Action::Reshuffle => {
                self.close_overlays();
                self.game.shuffle_remaining();
                self.after_move();
            }
            Action::CloseDialog => self.close_overlays(),
            Action::ScoresLayout(delta) => {
                if let Some(DialogState {
                    kind:
                        Dialog::Scores {
                            layout,
                            completed: None,
                        },
                    ..
                }) = &mut self.dialog
                {
                    let n = self.maps.len() as isize;
                    *layout = (*layout as isize + delta).rem_euclid(n) as usize;
                }
            }
            Action::ClearScores => {
                let layout = self.map_index;
                self.dialog = None;
                self.open_dialog(Dialog::ClearScores { layout });
            }
            Action::ConfirmClearScores => {
                self.history.clear();
                let layout = match &self.dialog {
                    Some(DialogState {
                        kind: Dialog::ClearScores { layout },
                        ..
                    }) => *layout,
                    _ => self.map_index,
                };
                self.dialog = None;
                self.open_dialog(Dialog::Scores {
                    layout,
                    completed: None,
                });
            }
        }
    }

    fn apply_layout(&mut self, index: usize) {
        self.settings.layout = self.maps[index].name.clone();
        self.new_game(false);
    }

    fn menu_activate(&mut self, index: usize) {
        let Some(menu) = &self.menu else { return };
        let items = self.menu_items(menu.page);
        let Some(item) = items.into_iter().nth(index) else {
            return;
        };
        match item {
            MenuItem::Item { action, .. } | MenuItem::Radio { action, .. } => {
                // Menu actions that open dialogs keep the auto-pause; others close it.
                match action {
                    Action::Scores
                    | Action::Rules
                    | Action::Shortcuts
                    | Action::About
                    | Action::SetLayout(_) => self.perform(action),
                    _ => {
                        self.close_overlays();
                        self.perform(action);
                    }
                }
            }
            MenuItem::Submenu { page, .. } => {
                // Start on the checked radio item, like GTK's popover submenus.
                let items = self.menu_items(page);
                let index = items
                    .iter()
                    .position(|i| matches!(i, MenuItem::Radio { checked: true, .. }))
                    .or_else(|| items.iter().position(|i| i.selectable()))
                    .unwrap_or(0);
                self.menu = Some(Menu { page, index });
            }
            _ => {}
        }
    }

    /// Mirrors GNOME's `moved` handler: record a win, or offer help when stuck.
    fn after_move(&mut self) {
        if self.game.complete() {
            if !self.recorded {
                self.recorded = true;
                let entry = self
                    .history
                    .add(&self.game.map.score_name, self.game.elapsed() as u32);
                storage::delete_save();
                let layout = self.map_index;
                self.open_dialog(Dialog::Scores {
                    layout,
                    completed: Some(entry),
                });
            }
            return;
        }
        if self.game.inspecting() {
            return;
        }
        if !self.game.can_move() {
            let can_shuffle = self.game.can_shuffle();
            self.open_dialog(Dialog::NoMoves { can_shuffle });
        }
    }

    /// Advance animations/autoplay. Call every frame.
    pub fn tick(&mut self) {
        if self.game.tick() {
            self.fix_cursor();
            self.after_move();
        }
    }

    // ----- Board interaction ---------------------------------------------

    /// GNOME's click handler.
    fn click_tile(&mut self, tile: Option<usize>, double: bool) {
        if self.game.inspecting() || self.game.autoplaying() {
            return;
        }
        if self.game.paused() {
            self.game.set_paused(false);
            self.restored = false;
            return;
        }
        if double && tile.is_none() && self.game.all_tiles_unblocked() {
            self.game.autoplay_end_game();
        }
        let Some(tile) = tile else { return };
        if !self.game.selectable(tile) {
            self.game.shake_tile(tile);
            return;
        }
        match self.game.selected() {
            None => self.game.set_selected(Some(tile)),
            Some(sel) if sel == tile => self.game.set_selected(None),
            Some(sel) if self.game.matches(sel, tile) => {
                self.game.remove_pair(sel, tile);
                self.fix_cursor();
                self.after_move();
            }
            Some(_) => self.game.set_selected(Some(tile)),
        }
    }

    pub fn tile_at(&self, col: u16, row: u16) -> Option<usize> {
        let geom = self.geometry?;
        let (col, row) = (col as i32, row as i32);
        let mut hit = None;
        for (i, t) in self.game.tiles.iter().enumerate() {
            if !t.visible {
                continue;
            }
            let (x, y, w, h) = geom.tile_rect(t.slot);
            let x = x + self.game.shake_offset(i);
            if col >= x && col < x + w && row >= y && row < y + h {
                hit = Some(i);
            }
        }
        hit
    }

    /// Tiles the keyboard cursor may rest on: visible with nothing on top.
    fn uncovered(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.game.tiles.len()).filter(|&i| self.game.tiles[i].visible && !self.game.covered(i))
    }

    /// Keep the cursor on a visible, uncovered tile near where it was.
    fn fix_cursor(&mut self) {
        let origin = match self.cursor {
            Some(c) if self.game.tiles[c].visible && !self.game.covered(c) => return,
            Some(c) => self.game.tiles[c].slot,
            None => Slot {
                x: 0,
                y: 0,
                layer: 0,
            },
        };
        self.cursor = self.uncovered().min_by_key(|&i| {
            let s = self.game.tiles[i].slot;
            (s.x - origin.x).pow(2) + (s.y - origin.y).pow(2)
        });
    }

    fn move_cursor(&mut self, dx: i32, dy: i32) {
        if !self.cursor_visible {
            self.cursor_visible = true;
            self.fix_cursor();
            return;
        }
        self.fix_cursor();
        let Some(cur) = self.cursor else { return };
        let from = self.game.tiles[cur].slot;
        let best = self
            .uncovered()
            .filter(|&i| i != cur)
            .filter_map(|i| {
                let s = self.game.tiles[i].slot;
                let (ddx, ddy) = (s.x - from.x, s.y - from.y);
                let along = ddx * dx + ddy * dy;
                let across = (ddx * dy - ddy * dx).abs();
                (along > 0 && across <= along * 2).then_some((i, along * 2 + across * 3))
            })
            .min_by_key(|&(_, score)| score);
        if let Some((i, _)) = best {
            self.cursor = Some(i);
        }
    }

    /// Cycle the cursor through free (selectable) tiles in reading order.
    fn cycle_cursor(&mut self, forward: bool) {
        self.cursor_visible = true;
        let mut free: Vec<usize> = (0..self.game.tiles.len())
            .filter(|&i| self.game.selectable(i))
            .collect();
        if free.is_empty() {
            return;
        }
        free.sort_by_key(|&i| {
            let s = self.game.tiles[i].slot;
            (s.y, s.x, s.layer)
        });
        let key = |i: usize| {
            let s = self.game.tiles[i].slot;
            (s.y, s.x, s.layer)
        };
        let next = match self.cursor {
            None => free[0],
            Some(c) => {
                let k = key(c);
                if forward {
                    *free.iter().find(|&&i| key(i) > k).unwrap_or(&free[0])
                } else {
                    *free
                        .iter()
                        .rev()
                        .find(|&&i| key(i) < k)
                        .unwrap_or(free.last().unwrap())
                }
            }
        };
        self.cursor = Some(next);
    }

    // ----- Input ---------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.menu.is_some() {
            self.menu_key(key);
            return;
        }
        if self.dialog.is_some() {
            self.dialog_key(key);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let action = match key.code {
            KeyCode::Char('q') if ctrl => Some(Action::Quit),
            KeyCode::Char('w') if ctrl => Some(Action::Quit),
            KeyCode::Char('n') if ctrl => Some(Action::NewGame),
            KeyCode::Char('r') if ctrl => Some(Action::RestartGame),
            KeyCode::Char('p') if ctrl => Some(Action::TogglePause),
            KeyCode::Char('h') if ctrl => Some(Action::Hint),
            KeyCode::Char('z') | KeyCode::Char('Z') if ctrl && shift => Some(Action::Redo),
            KeyCode::Char('z') if ctrl => Some(Action::Undo),
            KeyCode::Char('y') if ctrl => Some(Action::Redo),
            _ if ctrl => None,
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('n') => Some(Action::NewGame),
            KeyCode::Char('p') | KeyCode::Esc | KeyCode::Pause => Some(Action::TogglePause),
            KeyCode::Char('h') | KeyCode::Backspace => Some(Action::Hint),
            KeyCode::Char('u') | KeyCode::Char('z') => Some(Action::Undo),
            KeyCode::Char('U') | KeyCode::Char('Z') | KeyCode::Char('y') => Some(Action::Redo),
            KeyCode::Char('s') => Some(Action::Scores),
            KeyCode::Char('m') | KeyCode::F(10) => Some(Action::OpenMenu),
            KeyCode::F(1) => Some(Action::Rules),
            KeyCode::Char('?') => Some(Action::Shortcuts),
            _ => None,
        };
        if let Some(action) = action {
            self.perform(action);
            return;
        }

        if self.game.paused() {
            if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                self.perform(Action::TogglePause);
            }
            return;
        }
        match key.code {
            KeyCode::Left => self.move_cursor(-1, 0),
            KeyCode::Right => self.move_cursor(1, 0),
            KeyCode::Up => self.move_cursor(0, -1),
            KeyCode::Down => self.move_cursor(0, 1),
            KeyCode::Tab => self.cycle_cursor(true),
            KeyCode::BackTab => self.cycle_cursor(false),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if !self.cursor_visible {
                    self.cursor_visible = true;
                    self.fix_cursor();
                } else {
                    self.fix_cursor();
                    self.click_tile(self.cursor, false);
                }
            }
            KeyCode::Char('a') if !self.game.inspecting() => self.game.autoplay_end_game(),
            _ => {}
        }
    }

    fn menu_key(&mut self, key: KeyEvent) {
        let Some(menu) = &self.menu else { return };
        let page = menu.page;
        let items = self.menu_items(page);
        let step = |from: usize, forward: bool| -> usize {
            let n = items.len();
            let mut i = from;
            for _ in 0..n {
                i = if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                };
                if items[i].selectable() {
                    return i;
                }
            }
            from
        };
        let index = menu.index;
        match key.code {
            KeyCode::Esc | KeyCode::F(10) | KeyCode::Char('m') => {
                if page == MenuPage::Main || key.code != KeyCode::Esc {
                    self.close_overlays();
                } else {
                    self.perform(Action::MenuBack);
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.menu.as_mut().unwrap().index = step(index, false)
            }
            KeyCode::Down | KeyCode::Tab => self.menu.as_mut().unwrap().index = step(index, true),
            KeyCode::Left | KeyCode::Backspace if page != MenuPage::Main => {
                self.perform(Action::MenuBack)
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.menu_activate(index),
            KeyCode::Right if matches!(items.get(index), Some(MenuItem::Submenu { .. })) => {
                self.menu_activate(index)
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.perform(Action::Quit)
            }
            _ => {}
        }
    }

    fn dialog_key(&mut self, key: KeyEvent) {
        let buttons = self.buttons();
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let n = buttons.len().max(1);
        match key.code {
            KeyCode::Left | KeyCode::BackTab
                if !matches!(
                    dialog.kind,
                    Dialog::Scores {
                        completed: None,
                        ..
                    }
                ) || key.code == KeyCode::BackTab =>
            {
                dialog.focus = (dialog.focus + n - 1) % n
            }
            KeyCode::Right | KeyCode::Tab
                if !matches!(
                    dialog.kind,
                    Dialog::Scores {
                        completed: None,
                        ..
                    }
                ) || key.code == KeyCode::Tab =>
            {
                dialog.focus = (dialog.focus + 1) % n
            }
            KeyCode::Left => self.perform(Action::ScoresLayout(-1)),
            KeyCode::Right => self.perform(Action::ScoresLayout(1)),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(b) = buttons.into_iter().nth(dialog.focus) {
                    self.perform(b.action);
                }
            }
            KeyCode::Esc => self.perform(Action::CloseDialog),
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.perform(Action::Quit)
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        self.mouse = Some((ev.column, ev.row));
        let MouseEventKind::Down(MouseButton::Left) = ev.kind else {
            return;
        };

        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(t, c, r)| now - t < DOUBLE_CLICK && c == ev.column && r == ev.row);
        self.last_click = Some((now, ev.column, ev.row));

        let pos = ratatui::layout::Position {
            x: ev.column,
            y: ev.row,
        };
        if let Some((_, action)) = self.regions.iter().rev().find(|(r, _)| r.contains(pos)) {
            let action = action.clone();
            self.perform(action);
            return;
        }
        if self.menu.is_some() {
            self.close_overlays();
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        self.cursor_visible = false;
        let tile = self.tile_at(ev.column, ev.row);
        if tile.is_some() {
            self.cursor = tile;
        }
        self.click_tile(tile, double);
    }
}
