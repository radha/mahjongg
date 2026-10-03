//! Application state: the current game plus menus, dialogs, settings and input.

use std::io;
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
    CancelClearScores,
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
    /// First error hit while saving settings, scores or the game; reported on exit.
    pub storage_error: Option<String>,
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
            storage_error: None,
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
            if app.restored {
                app.settings.layout = save.map;
            }
        }
        if !app.restored {
            let rotate = app.settings.progression == Progression::Random;
            app.new_game(rotate);
        }
        app
    }

    /// Save the game in progress (GNOME resumes it next launch).
    pub fn shutdown(&mut self) {
        self.persist();
        if self.game.inspecting() {
            let next = self.next_map_index();
            self.settings.layout = self.maps[next].name.clone();
        }
        self.save_settings();
    }

    /// Keep the save file in step with the game, so nothing is lost if the
    /// terminal is closed or the process is killed.
    fn persist(&mut self) {
        let result = if self.game.started() && !self.game.inspecting() && self.game.can_move() {
            storage::write_save(&self.game)
        } else {
            storage::delete_save()
        };
        self.record(result);
    }

    fn save_settings(&mut self) {
        let result = self.settings.save();
        self.record(result);
    }

    fn save_history(&mut self) {
        let result = self.history.save();
        self.record(result);
    }

    fn record(&mut self, result: io::Result<()>) {
        if let Err(e) = result {
            self.storage_error.get_or_insert(e.to_string());
        }
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
        self.save_settings();
        self.game = Game::new(self.maps[index].clone());
        self.game.generate(None);
        self.persist();
        self.reset_session();
    }

    fn restart_game(&mut self) {
        self.game.restart();
        self.persist();
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
        let focus = self
            .buttons_for(&kind)
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

    /// Hint is available during play; with no moves left it reopens the
    /// "No Moves Left" choices instead.
    pub fn can_hint(&self) -> bool {
        !self.game.paused() && !self.game.inspecting() && !self.game.autoplaying()
    }

    /// Menus and dialogs are modal.
    pub fn overlay_open(&self) -> bool {
        self.menu.is_some() || self.dialog.is_some()
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
            Some(d) => self.buttons_for(&d.kind),
            None => vec![],
        }
    }

    fn buttons_for(&self, kind: &Dialog) -> Vec<Button> {
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
                if !self.history.entries.is_empty() {
                    v.push(b("Clear Scores", Action::ClearScores, Destructive));
                }
                v.push(b("Close", Action::CloseDialog, Suggested));
                v
            }
            Dialog::ClearScores { .. } => vec![
                b("Cancel", Action::CancelClearScores, Suggested),
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
                if !self.can_hint() {
                    return;
                }
                if self.game.can_move() {
                    self.game.show_hint();
                } else {
                    // Stuck: offer the reshuffle / new game choices again.
                    let can_shuffle = self.game.can_shuffle();
                    self.open_dialog(Dialog::NoMoves { can_shuffle });
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
                let Some(page) = self.menu.as_ref().map(|m| m.page) else {
                    return;
                };
                // Return to the submenu entry we came from.
                let index = self
                    .menu_items(MenuPage::Main)
                    .iter()
                    .position(|i| matches!(i, MenuItem::Submenu { page: p, .. } if *p == page))
                    .unwrap_or(0);
                self.menu = Some(Menu {
                    page: MenuPage::Main,
                    index,
                });
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
                self.save_settings();
                self.close_overlays();
            }
            Action::SetBackground(b) => {
                self.settings.background = b;
                self.save_settings();
                self.close_overlays();
            }
            Action::SetTheme(t) => {
                self.settings.theme = t;
                self.save_settings();
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
                // Remember which layout's table was showing, to return to it.
                let layout = match &self.dialog {
                    Some(DialogState {
                        kind: Dialog::Scores { layout, .. },
                        ..
                    }) => *layout,
                    _ => self.map_index,
                };
                self.open_dialog(Dialog::ClearScores { layout });
            }
            Action::ConfirmClearScores => {
                self.history.clear();
                self.save_history();
                self.back_to_scores();
            }
            Action::CancelClearScores => self.back_to_scores(),
        }
    }

    /// Leave the "Clear All Scores?" confirmation for the scores table it came from.
    fn back_to_scores(&mut self) {
        let layout = match &self.dialog {
            Some(DialogState {
                kind: Dialog::ClearScores { layout },
                ..
            }) => *layout,
            _ => self.map_index,
        };
        self.open_dialog(Dialog::Scores {
            layout,
            completed: None,
        });
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
        self.persist();
        if self.game.complete() {
            if !self.recorded {
                self.recorded = true;
                let secs = u32::try_from(self.game.elapsed().as_secs()).unwrap_or(u32::MAX);
                let entry = self.history.add(&self.game.map.score_name, secs);
                self.save_history();
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
        let key = |i: usize| {
            let s = self.game.tiles[i].slot;
            (s.y, s.x, s.layer)
        };
        let mut free: Vec<usize> = (0..self.game.tiles.len())
            .filter(|&i| self.game.selectable(i))
            .collect();
        if free.is_empty() {
            return;
        }
        free.sort_by_key(|&i| key(i));
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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Quitting works everywhere, including menus and dialogs. The game is saved.
        if ctrl && matches!(key.code, KeyCode::Char('q' | 'c')) {
            self.perform(Action::Quit);
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

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let action = match key.code {
            KeyCode::Char('n') if ctrl => Some(Action::NewGame),
            KeyCode::Char('r') if ctrl => Some(Action::RestartGame),
            KeyCode::Char('p') if ctrl => Some(Action::TogglePause),
            KeyCode::Char('h') if ctrl => Some(Action::Hint),
            KeyCode::Char('z') | KeyCode::Char('Z') if ctrl && shift => Some(Action::Redo),
            KeyCode::Char('z') if ctrl => Some(Action::Undo),
            KeyCode::Char('y') if ctrl => Some(Action::Redo),
            _ if ctrl => None,
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('p') | KeyCode::Esc | KeyCode::Pause => Some(Action::TogglePause),
            KeyCode::Char('u') => Some(Action::Undo),
            KeyCode::Char('U') => Some(Action::Redo),
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
            KeyCode::Left | KeyCode::Char('h') => self.move_cursor(-1, 0),
            KeyCode::Right | KeyCode::Char('l') => self.move_cursor(1, 0),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(0, -1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(0, 1),
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
            KeyCode::Esc => {
                if let Dialog::ClearScores { .. } = dialog.kind {
                    self.perform(Action::CancelClearScores);
                } else {
                    self.perform(Action::CloseDialog);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn draw(app: &mut App) {
        let mut term = Terminal::new(TestBackend::new(110, 42)).unwrap();
        term.draw(|f| crate::ui::draw(f, app)).unwrap();
    }

    fn click(app: &mut App, r: Rect) {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn region(app: &App, action: &Action) -> Option<Rect> {
        app.regions
            .iter()
            .find(|(_, a)| a == action)
            .map(|(r, _)| *r)
    }

    fn play_first_move(app: &mut App) {
        let (a, b) = app.game.solution[0];
        app.click_tile(Some(a), false);
        app.click_tile(Some(b), false);
        assert_eq!(app.game.current_move(), 2);
    }

    #[test]
    fn header_ignores_clicks_while_a_dialog_is_open() {
        let mut app = App::new();
        draw(&mut app);
        let hint = region(&app, &Action::Hint).unwrap();
        key(&mut app, KeyCode::F(1));
        draw(&mut app);
        assert!(region(&app, &Action::Hint).is_none());
        click(&mut app, hint);
        assert!(!app.game.started(), "no hint penalty through the dialog");
        assert!(app.dialog.is_some());
    }

    #[test]
    fn ctrl_c_and_ctrl_q_quit_from_anywhere() {
        for open in [None, Some(KeyCode::Char('m')), Some(KeyCode::F(1))] {
            for c in ['c', 'q'] {
                let mut app = App::new();
                if let Some(code) = open {
                    key(&mut app, code);
                }
                ctrl(&mut app, c);
                assert!(app.quit, "{open:?} ctrl+{c}");
            }
        }
    }

    #[test]
    fn bare_n_does_not_discard_the_game() {
        let mut app = App::new();
        let seed = app.game.seed;
        key(&mut app, KeyCode::Char('n'));
        assert_eq!(app.game.seed, seed);
        ctrl(&mut app, 'n');
        assert_ne!(app.game.seed, seed);
    }

    #[test]
    fn clear_scores_returns_to_the_browsed_layout() {
        let mut app = App::new();
        app.history.add("easy", 100);
        key(&mut app, KeyCode::Char('s'));
        key(&mut app, KeyCode::Right);
        key(&mut app, KeyCode::Right);
        let browsed = (app.map_index + 2) % app.maps.len();
        let showing = |app: &App| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Dialog::Scores { layout, .. }) => Some(*layout),
            _ => None,
        };
        assert_eq!(showing(&app), Some(browsed));

        app.perform(Action::ClearScores);
        key(&mut app, KeyCode::Esc);
        assert_eq!(showing(&app), Some(browsed), "cancel goes back");
        assert_eq!(app.history.entries.len(), 1);

        app.perform(Action::ClearScores);
        app.perform(Action::ConfirmClearScores);
        assert_eq!(showing(&app), Some(browsed));
        assert!(app.history.entries.is_empty());
    }

    #[test]
    fn hint_when_stuck_offers_reshuffle() {
        let mut app = App::new();
        play_first_move(&mut app);
        for (i, t) in app.game.tiles.iter_mut().enumerate() {
            t.visible = i < 2;
        }
        app.game.tiles[0].number = Some(0);
        app.game.tiles[1].number = Some(4);
        assert!(!app.game.can_move());
        app.perform(Action::Hint);
        assert!(matches!(
            app.dialog.as_ref().map(|d| &d.kind),
            Some(Dialog::NoMoves { can_shuffle: true })
        ));
    }

    #[test]
    fn every_move_is_saved_and_restored_with_its_layout() {
        let mut app = App::new();
        app.apply_layout(3);
        assert!(
            storage::load_save(&app.maps).is_none(),
            "nothing to resume yet"
        );
        play_first_move(&mut app);
        assert!(storage::load_save(&app.maps).is_some());

        // Settings pointing elsewhere must not override the saved game's layout.
        app.settings.layout = app.maps[0].name.clone();
        app.save_settings();
        let resumed = App::new();
        assert!(resumed.restored && resumed.game.paused());
        assert_eq!(resumed.map_index, 3);
        assert_eq!(resumed.settings.layout, resumed.maps[3].name);
    }

    #[test]
    fn menu_back_returns_to_the_submenu_entry() {
        let mut app = App::new();
        key(&mut app, KeyCode::Char('m'));
        let items = app.menu_items(MenuPage::Main);
        let appearance = items
            .iter()
            .position(|i| {
                matches!(
                    i,
                    MenuItem::Submenu {
                        page: MenuPage::Appearance,
                        ..
                    }
                )
            })
            .unwrap();
        app.perform(Action::MenuActivate(appearance));
        assert_eq!(app.menu.as_ref().unwrap().page, MenuPage::Appearance);
        key(&mut app, KeyCode::Esc);
        let menu = app.menu.as_ref().unwrap();
        assert_eq!((menu.page, menu.index), (MenuPage::Main, appearance));
    }

    #[test]
    fn autoplay_waits_while_paused_and_the_win_is_not_left_paused() {
        let mut app = App::new();
        for (a, b) in app.game.solution.clone() {
            if app.game.all_tiles_unblocked() {
                break;
            }
            app.game.remove_pair(a, b);
        }
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Esc);
        assert!(app.game.paused() && app.game.autoplaying());
        let left = app.game.tiles.iter().filter(|t| t.visible).count();
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(250));
            app.tick();
        }
        assert_eq!(app.game.tiles.iter().filter(|t| t.visible).count(), left);

        key(&mut app, KeyCode::Esc);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !app.game.complete() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.game.complete());
        assert!(matches!(
            app.dialog.as_ref().map(|d| &d.kind),
            Some(Dialog::Scores {
                completed: Some(_),
                ..
            })
        ));
        key(&mut app, KeyCode::Esc);
        assert!(!app.game.paused() && !app.pause_overlay_visible());
    }
}
