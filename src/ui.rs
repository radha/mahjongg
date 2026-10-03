//! Rendering: GNOME-style header bar, the 3D tile board, popover menu and dialogs.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::app::{Action, App, ButtonKind, Dialog, Geometry, MenuItem, MenuPage};
use crate::game::Game;
use crate::map::Map;
use crate::theme::{
    Ink, TilePalette, UiPalette, face_for, large_glyphs, mix, opt, rgb, small_glyphs,
};

// ----- Low-level cell helpers ----------------------------------------------

/// Write one grapheme, keeping double-width glyphs consistent when tiles overlap.
fn put(buf: &mut Buffer, clip: Rect, x: i32, y: i32, sym: &str, style: Style) {
    let (left, top) = (clip.x as i32, clip.y as i32);
    let (right, bottom) = (left + clip.width as i32, top + clip.height as i32);
    let w = sym.width().max(1) as i32;
    if x < left || y < top || x + w > right || y >= bottom {
        return;
    }
    // Overwriting the tail of a wide glyph: blank the glyph itself.
    if x > left {
        let prev = &mut buf[(x as u16 - 1, y as u16)];
        if prev.symbol().width() > 1 {
            prev.set_symbol(" ");
        }
    }
    buf[(x as u16, y as u16)].set_symbol(sym).set_style(style);
    if w == 2 {
        buf[(x as u16 + 1, y as u16)]
            .set_symbol(" ")
            .set_style(style);
    }
}

fn text(buf: &mut Buffer, clip: Rect, x: i32, y: i32, s: &str, style: Style) -> i32 {
    let mut cx = x;
    let mut tmp = [0u8; 4];
    for ch in s.chars() {
        let sym = ch.encode_utf8(&mut tmp);
        put(buf, clip, cx, y, sym, style);
        cx += sym.width().max(1) as i32;
    }
    cx
}

fn centered_text(buf: &mut Buffer, area: Rect, y: u16, s: &str, style: Style) {
    let x = area.x as i32 + (area.width as i32 - s.width() as i32) / 2;
    text(buf, area, x, y as i32, s, style);
}

fn fill(buf: &mut Buffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn hovered(app: &App, r: Rect) -> bool {
    app.mouse
        .is_some_and(|(x, y)| r.contains(ratatui::layout::Position { x, y }))
}

// ----- Entry point ------------------------------------------------------------

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.regions.clear();
    let area = frame.area();
    let ui = UiPalette::for_background(app.settings.background);
    let tp = TilePalette::for_theme(app.settings.theme);
    let buf = frame.buffer_mut();

    fill(
        buf,
        area,
        Style::default().bg(opt(ui.board)).fg(opt(ui.dialog_fg)),
    );
    let header_h = 3.min(area.height);
    let header = Rect {
        height: header_h,
        ..area
    };
    let board = Rect {
        y: area.y + header_h,
        height: area.height - header_h,
        ..area
    };

    draw_board(buf, board, app, &ui, &tp);
    draw_header(buf, header, app, &ui);
    if app.pause_overlay_visible() {
        draw_pause_overlay(buf, board, app, &ui);
    }
    // Overlays are modal: only their own controls take clicks.
    if app.overlay_open() {
        app.regions.clear();
    }
    if app.menu.is_some() {
        draw_menu(buf, area, app, &ui);
    }
    if app.dialog.is_some() {
        draw_dialog(buf, area, app, &ui);
    }
}

// ----- Header bar -------------------------------------------------------------

fn format_clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h:02}∶{m:02}∶{s:02}")
    } else {
        format!("{m:02}∶{s:02}")
    }
}

#[allow(clippy::too_many_arguments)]
fn header_button(
    buf: &mut Buffer,
    app: &mut App,
    ui: &UiPalette,
    x: i32,
    y: u16,
    label: &str,
    enabled: bool,
    action: Action,
) -> i32 {
    let w = label.width() as u16;
    let rect = Rect::new(x.max(0) as u16, y, w, 1);
    let mut style = Style::default().bg(opt(ui.header)).fg(opt(ui.header_fg));
    if !enabled {
        style = style.fg(rgb(ui.dim_fg)).add_modifier(Modifier::DIM);
    } else if !app.overlay_open() && hovered(app, rect) {
        style = style.bg(rgb(ui.button_hover));
    }
    let area = buf.area;
    text(buf, area, x, y as i32, label, style);
    if enabled {
        app.regions.push((rect, action));
    }
    x + w as i32
}

fn draw_header(buf: &mut Buffer, area: Rect, app: &mut App, ui: &UiPalette) {
    if area.height == 0 {
        return;
    }
    let bar = Rect {
        height: area.height.min(2),
        ..area
    };
    let base = Style::default().bg(opt(ui.header)).fg(opt(ui.header_fg));
    fill(buf, bar, base);
    if area.height >= 3 {
        let y = area.y + 2;
        for x in area.left()..area.right() {
            match ui.header {
                Some(h) => buf[(x, y)]
                    .set_symbol("▀")
                    .set_style(Style::default().fg(rgb(h)).bg(opt(ui.board))),
                None => buf[(x, y)]
                    .set_symbol("─")
                    .set_style(Style::default().fg(rgb(ui.border))),
            };
        }
    }

    let game = &app.game;
    let paused = game.paused();
    let can_undo = !paused && game.can_undo();
    let can_redo = !paused && game.can_redo();
    let moves_left = game.moves_left();
    let can_hint = app.can_hint();
    let can_pause = game.started() && !game.inspecting() && !game.complete();
    let clock = format_clock(game.elapsed().as_secs());
    let subtitle = if game.inspecting() {
        "Finished · Ctrl+N for a new game".to_string()
    } else {
        format!("Moves Left: {moves_left:2}")
    };

    let y = area.y;
    let mut x = area.x as i32 + 1;
    x = header_button(buf, app, ui, x, y, " ↶ Undo ", can_undo, Action::Undo) + 1;
    header_button(buf, app, ui, x, y, " ↷ Redo ", can_redo, Action::Redo);

    let right_labels = [
        (" ? Hint ", can_hint, Action::Hint),
        (
            if paused {
                " ▶ Resume "
            } else {
                " ‖ Pause "
            },
            can_pause,
            Action::TogglePause,
        ),
        (" ≡ ", true, Action::OpenMenu),
    ];
    let total: i32 = right_labels
        .iter()
        .map(|(l, ..)| l.width() as i32 + 1)
        .sum();
    let mut x = area.right() as i32 - total;
    for (label, enabled, action) in right_labels {
        let menu_open = action == Action::OpenMenu && app.menu.is_some();
        x = header_button(buf, app, ui, x, y, label, enabled, action) + 1;
        if menu_open {
            let w = label.width() as i32;
            let r = Rect::new((x - w - 1) as u16, y, w as u16, 1);
            buf.set_style(r, Style::default().bg(rgb(ui.button_hover)));
        }
    }

    centered_text(buf, bar, y, &clock, base.add_modifier(Modifier::BOLD));
    if bar.height > 1 {
        centered_text(buf, bar, y + 1, &subtitle, base.fg(rgb(ui.dim_fg)));
    }
}

// ----- Board -------------------------------------------------------------------

/// Pick the largest tile size that fits the layout in `area`.
pub fn compute_geometry(map: &Map, area: Rect) -> Result<Geometry, (u16, u16)> {
    let mut needed = (0, 0);
    for large in [true, false] {
        let mut g = Geometry {
            hw: if large { 3 } else { 2 },
            hh: if large { 2 } else { 1 },
            shift_x: 1,
            shift_y: if large { 1 } else { 0 },
            ox: 0,
            oy: 0,
            large,
        };
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &slot in &map.slots {
            let (x, y, w, h) = g.tile_rect(slot);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + w);
            max_y = max_y.max(y + h);
        }
        let (bw, bh) = (max_x - min_x, max_y - min_y);
        needed = (bw as u16 + 2, bh as u16);
        if bw + 2 <= area.width as i32 && bh <= area.height as i32 {
            g.ox = area.x as i32 + (area.width as i32 - bw) / 2 - min_x;
            g.oy = area.y as i32 + (area.height as i32 - bh) / 2 - min_y;
            return Ok(g);
        }
    }
    Err(needed)
}

#[allow(clippy::too_many_arguments)]
fn draw_tile(buf: &mut Buffer, clip: Rect, x: i32, y: i32, w: i32, h: i32, face: u32, side: u32) {
    let body = Style::default().bg(rgb(face));
    let edge = Style::default().fg(rgb(face)).bg(rgb(side));
    for r in 0..h {
        for c in 0..w {
            let (sym, style) = match (c == 0, r == h - 1) {
                (true, true) => ("▝", edge),
                (true, false) => ("▐", edge),
                (false, true) => ("▀", edge),
                (false, false) => (" ", body),
            };
            put(buf, clip, x + c, y + r, sym, style);
        }
    }
}

fn draw_board(buf: &mut Buffer, area: Rect, app: &mut App, ui: &UiPalette, tp: &TilePalette) {
    if area.height == 0 {
        app.geometry = None;
        return;
    }
    let geom = match compute_geometry(&app.game.map, area) {
        Ok(g) => g,
        Err((w, h)) => {
            app.geometry = None;
            let style = Style::default().bg(opt(ui.board)).fg(opt(ui.dialog_fg));
            let mid = area.y + area.height / 2;
            centered_text(
                buf,
                area,
                mid.saturating_sub(1),
                "Terminal too small",
                style.add_modifier(Modifier::BOLD),
            );
            let msg = format!(
                "Need {}×{} for this layout (have {}×{})",
                w,
                h + 3,
                area.width,
                area.height + 3
            );
            centered_text(buf, area, mid, &msg, style.fg(rgb(ui.dim_fg)));
            return;
        }
    };
    app.geometry = Some(geom);

    let game: &Game = &app.game;
    let paused = game.paused();
    let fade = ui.fade_target();
    let theme = app.settings.theme;
    for (i, tile) in game.tiles.iter().enumerate() {
        if !tile.visible {
            continue;
        }
        let (x, y, w, h) = geom.tile_rect(tile.slot);
        let x = x + game.shake_offset(i);
        let mut face = tp.face_for_layer(tile.slot.layer);
        let mut side = tp.side;
        if paused {
            face = mix(face, fade, 0.55);
            side = mix(side, fade, 0.55);
        } else {
            if app.cursor_visible && app.cursor == Some(i) {
                face = tp.cursor;
                side = tp.cursor_side;
            }
            if game.highlighted(i) {
                face = tp.highlight;
            }
        }
        draw_tile(buf, area, x, y, w, h, face, side);
        if paused {
            continue;
        }
        let face_kind = face_for(tile.number);
        let glyphs = if geom.large {
            large_glyphs(face_kind, theme)
        } else {
            small_glyphs(face_kind, theme)
        };
        for (c, r, sym, ink) in glyphs {
            let color = if ink == Ink::Dark || !game.highlighted(i) {
                tp.ink(ink)
            } else {
                mix(tp.ink(ink), 0x000000, 0.15)
            };
            put(
                buf,
                area,
                x + 1 + c,
                y + r,
                sym,
                Style::default().fg(rgb(color)).bg(rgb(face)),
            );
        }
    }
}

// ----- Buttons, frames ------------------------------------------------------------

fn button_style(ui: &UiPalette, kind: ButtonKind, focused: bool, hover: bool) -> Style {
    let base = Style::default().fg(opt(ui.dialog_fg));
    let bg = if hover { ui.button_hover } else { ui.button };
    match (kind, focused) {
        (ButtonKind::Destructive, true) => base
            .bg(rgb(ui.destructive))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        (_, true) => base
            .bg(rgb(ui.accent))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        (ButtonKind::Destructive, false) => base.bg(rgb(bg)).fg(rgb(ui.destructive)),
        (ButtonKind::Suggested, false) => base
            .bg(rgb(bg))
            .fg(rgb(ui.accent))
            .add_modifier(Modifier::BOLD),
        (ButtonKind::Normal, false) => base.bg(rgb(bg)),
    }
}

/// Draw a row of buttons centred in `area` at row `y`, registering click regions.
fn button_row(
    buf: &mut Buffer,
    app: &mut App,
    ui: &UiPalette,
    area: Rect,
    y: u16,
    buttons: &[(String, ButtonKind, bool, Action)],
) {
    let gap = 2;
    let total: i32 = buttons
        .iter()
        .map(|(l, ..)| l.width() as i32 + 4)
        .sum::<i32>()
        + gap * (buttons.len() as i32 - 1).max(0);
    let mut x = area.x as i32 + (area.width as i32 - total) / 2;
    for (label, kind, focused, action) in buttons {
        let w = label.width() as i32 + 4;
        let rect = Rect::new(x.max(0) as u16, y, w as u16, 1);
        let style = button_style(ui, *kind, *focused, hovered(app, rect));
        text(buf, area, x, y as i32, &format!("  {label}  "), style);
        app.regions.push((rect, action.clone()));
        x += w + gap;
    }
}

/// Clear a dialog box with rounded border; returns the inner area.
fn frame_box(buf: &mut Buffer, rect: Rect, ui: &UiPalette) -> Rect {
    let bg = Style::default().bg(opt(ui.dialog)).fg(opt(ui.dialog_fg));
    fill(buf, rect, bg);
    let border = bg.fg(rgb(ui.border));
    let (l, r, t, b) = (rect.left(), rect.right() - 1, rect.top(), rect.bottom() - 1);
    for x in l..=r {
        buf[(x, t)].set_symbol("─").set_style(border);
        buf[(x, b)].set_symbol("─").set_style(border);
    }
    for y in t..=b {
        buf[(l, y)].set_symbol("│").set_style(border);
        buf[(r, y)].set_symbol("│").set_style(border);
    }
    buf[(l, t)].set_symbol("╭");
    buf[(r, t)].set_symbol("╮");
    buf[(l, b)].set_symbol("╰");
    buf[(r, b)].set_symbol("╯");
    Rect::new(
        rect.x + 2,
        rect.y + 1,
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(2),
    )
}

// ----- Pause overlay -----------------------------------------------------------------

fn draw_pause_overlay(buf: &mut Buffer, area: Rect, app: &mut App, ui: &UiPalette) {
    let rect = centered(area, 44, 7);
    if rect.width < 20 || rect.height < 5 {
        return;
    }
    let inner = frame_box(buf, rect, ui);
    let bg = Style::default().bg(opt(ui.dialog)).fg(opt(ui.dialog_fg));
    centered_text(
        buf,
        inner,
        inner.y + 1,
        "Paused",
        bg.add_modifier(Modifier::BOLD),
    );
    let second = if app.restored {
        (
            "Restart Game".to_string(),
            ButtonKind::Normal,
            false,
            Action::RestartGame,
        )
    } else {
        ("Quit".to_string(), ButtonKind::Normal, false, Action::Quit)
    };
    let buttons = [
        (
            "Resume Game".to_string(),
            ButtonKind::Suggested,
            true,
            Action::TogglePause,
        ),
        second,
    ];
    button_row(buf, app, ui, inner, inner.y + 3, &buttons);
}

// ----- Menu popover ---------------------------------------------------------------------

fn draw_menu(buf: &mut Buffer, area: Rect, app: &mut App, ui: &UiPalette) {
    let Some(menu) = &app.menu else { return };
    let (page, index) = (menu.page, menu.index);
    let items = app.menu_items(page);
    let width: u16 = 34;
    let title_rows = if page == MenuPage::Main { 0 } else { 2 };
    let height = items.len() as u16 + title_rows + 2;
    let x = area.right().saturating_sub(width + 1).max(area.x);
    let rect = Rect::new(
        x,
        area.y + 1,
        width.min(area.width),
        height.min(area.height.saturating_sub(1)),
    );
    if rect.height < 3 || rect.width < 4 {
        return;
    }
    let bg = Style::default().bg(opt(ui.dialog)).fg(opt(ui.dialog_fg));
    let inner = frame_box(buf, rect, ui);
    let inner = Rect {
        x: inner.x - 1,
        width: inner.width + 2,
        ..inner
    };
    let mut y = inner.y;

    if page != MenuPage::Main {
        let r = Rect::new(inner.x, y, inner.width, 1);
        let style = if hovered(app, r) {
            bg.bg(rgb(ui.button_hover))
        } else {
            bg
        };
        fill(buf, r, style);
        text(buf, inner, inner.x as i32 + 1, y as i32, "‹", style);
        centered_text(
            buf,
            r,
            y,
            App::menu_title(page),
            style.add_modifier(Modifier::BOLD),
        );
        app.regions.push((r, Action::MenuBack));
        y += 1;
        for x in inner.left()..inner.right() {
            buf[(x, y)].set_symbol("─").set_style(bg.fg(rgb(ui.border)));
        }
        y += 1;
    }

    for (i, item) in items.iter().enumerate() {
        if y >= inner.bottom() {
            break;
        }
        let r = Rect::new(inner.x, y, inner.width, 1);
        let active = i == index || (item.selectable() && hovered(app, r));
        let style = if active && item.selectable() {
            bg.bg(rgb(ui.button_hover))
        } else {
            bg
        };
        fill(buf, r, style);
        let left = inner.x as i32 + 1;
        let right = inner.right() as i32 - 1;
        match item {
            MenuItem::Item { label, accel, .. } => {
                text(buf, inner, left, y as i32, label, style);
                text(
                    buf,
                    inner,
                    right - accel.width() as i32,
                    y as i32,
                    accel,
                    style.fg(rgb(ui.dim_fg)),
                );
            }
            MenuItem::Submenu { label, .. } => {
                text(buf, inner, left, y as i32, label, style);
                text(buf, inner, right - 1, y as i32, "›", style);
            }
            MenuItem::Radio { label, checked, .. } => {
                let mark = if *checked { "●" } else { "○" };
                let mark_style = if *checked {
                    style.fg(rgb(ui.accent))
                } else {
                    style.fg(rgb(ui.dim_fg))
                };
                text(buf, inner, left, y as i32, mark, mark_style);
                text(buf, inner, left + 2, y as i32, label, style);
            }
            MenuItem::Heading(label) => {
                text(
                    buf,
                    inner,
                    left,
                    y as i32,
                    label,
                    style.fg(rgb(ui.dim_fg)).add_modifier(Modifier::BOLD),
                );
            }
            MenuItem::Separator => {
                for x in inner.left()..inner.right() {
                    buf[(x, y)].set_symbol("─").set_style(bg.fg(rgb(ui.border)));
                }
            }
        }
        if item.selectable() {
            app.regions.push((r, Action::MenuActivate(i)));
        }
        y += 1;
    }
}

// ----- Dialogs ---------------------------------------------------------------------------

fn format_duration(secs: u32) -> String {
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

const RULES: &[(&str, &str)] = &[
    (
        "Clear the board by matching pairs of identical tiles",
        "Any season matches any season; any flower matches any flower",
    ),
    (
        "Only uncovered tiles with a free long edge can be selected",
        "A tile is free when nothing is on top and its left or right side is open",
    ),
    ("Rounds are scored based on completion time", ""),
    ("You can pause the game", "Tile faces will be hidden"),
    ("You can undo or redo a move", "No time penalty is added"),
    (
        "You can use hints to reveal matching tiles",
        "Adds a 30-second time penalty",
    ),
    (
        "You can shuffle tiles when no moves are left",
        "Adds a 60-second time penalty",
    ),
];

const SHORTCUTS: &[(&str, &str)] = &[
    ("Mouse click", "Select / match a tile"),
    (
        "Double-click background",
        "Finish the game when all tiles are free",
    ),
    ("Arrow keys / H J K L", "Move the tile cursor"),
    ("Tab / Shift+Tab", "Jump between free tiles"),
    ("Enter / Space", "Select the tile under the cursor"),
    ("A", "Auto-finish when all tiles are free"),
    ("Ctrl+N", "New game"),
    ("Ctrl+R", "Restart game"),
    ("Ctrl+Z / U", "Undo"),
    ("Ctrl+Y / Shift+U", "Redo"),
    ("Ctrl+H", "Hint (or reshuffle when stuck)"),
    ("Esc / P / Ctrl+P", "Pause / resume"),
    ("S", "Scores"),
    ("F10 / M", "Main menu"),
    ("F1", "Game rules"),
    ("Ctrl+Q / Ctrl+C / Q", "Quit (the game is saved)"),
];

fn draw_dialog(buf: &mut Buffer, area: Rect, app: &mut App, ui: &UiPalette) {
    let Some(dialog) = &app.dialog else { return };
    let kind = dialog.kind.clone();
    let focus = dialog.focus;
    let bg = Style::default().bg(opt(ui.dialog)).fg(opt(ui.dialog_fg));
    let bold = bg.add_modifier(Modifier::BOLD);
    let dim = bg.fg(rgb(ui.dim_fg));

    let (w, h) = match &kind {
        Dialog::NoMoves { .. } => (62, 9),
        Dialog::ChangeLayout(_) | Dialog::ClearScores { .. } => (48, 9),
        Dialog::Scores { .. } => (64, 22),
        Dialog::Rules => (78, 5 + RULES.len() as u16 * 2 + 3),
        Dialog::Shortcuts => (74, 5 + SHORTCUTS.len() as u16 + 3),
        Dialog::About => (60, 14),
    };
    let rect = centered(area, w, h);
    if rect.width < 10 || rect.height < 5 {
        return;
    }
    let inner = frame_box(buf, rect, ui);
    let mut y = inner.y + 1;
    let line = |buf: &mut Buffer, y: &mut u16, s: &str, style: Style| {
        centered_text(buf, inner, *y, s, style);
        *y += 1;
    };

    match &kind {
        Dialog::NoMoves { can_shuffle } => {
            line(buf, &mut y, "No Moves Left", bold);
            y += 1;
            let body = if *can_shuffle {
                "You can undo your moves and try to find a solution,\nor reshuffle the remaining tiles."
            } else {
                "You can undo your moves and try to find a solution,\nor start a new game."
            };
            for l in body.lines() {
                line(buf, &mut y, l, bg);
            }
        }
        Dialog::ChangeLayout(_) => {
            line(buf, &mut y, "Change Layout?", bold);
            y += 1;
            line(buf, &mut y, "This will end your current game.", bg);
        }
        Dialog::ClearScores { .. } => {
            line(buf, &mut y, "Clear All Scores?", bold);
            y += 1;
            line(
                buf,
                &mut y,
                "This will remove every score for every layout.",
                bg,
            );
        }
        Dialog::Scores { layout, completed } => {
            let map = &app.maps[*layout];
            match completed {
                Some(entry) => {
                    line(buf, &mut y, "Congratulations!", bold);
                    let msg = format!(
                        "You cleared {} in {}",
                        map.name,
                        format_duration(entry.duration)
                    );
                    line(buf, &mut y, &msg, dim);
                }
                None => {
                    line(buf, &mut y, "Scores", bold);
                    let label = format!("‹  {}  ›", map.name);
                    let lw = label.width() as u16;
                    let lx = inner.x + (inner.width.saturating_sub(lw)) / 2;
                    centered_text(buf, inner, y, &label, bg.fg(rgb(ui.accent)));
                    app.regions
                        .push((Rect::new(lx, y, 3, 1), Action::ScoresLayout(-1)));
                    app.regions.push((
                        Rect::new(lx + lw.saturating_sub(3), y, 3, 1),
                        Action::ScoresLayout(1),
                    ));
                    y += 1;
                }
            }
            y += 1;
            let ranked = app.history.ranked(&map.score_name);
            if ranked.is_empty() {
                y += 3;
                line(
                    buf,
                    &mut y,
                    "No scores yet",
                    dim.add_modifier(Modifier::BOLD),
                );
                line(
                    buf,
                    &mut y,
                    "Play some games and your times will show up here.",
                    dim,
                );
            } else {
                let cols = [
                    inner.x as i32 + 2,
                    inner.x as i32 + 10,
                    inner.x as i32 + 24,
                    inner.x as i32 + 48,
                ];
                let header = ["Rank", "Date", "Player", "Time"];
                for (c, h) in cols.iter().zip(header) {
                    text(
                        buf,
                        inner,
                        *c,
                        y as i32,
                        h,
                        dim.add_modifier(Modifier::BOLD),
                    );
                }
                y += 1;
                let max_rows = (inner.bottom().saturating_sub(y + 2)) as usize;
                let highlight = completed
                    .as_ref()
                    .and_then(|c| ranked.iter().position(|e| e == c));
                let start = match highlight {
                    Some(h) if h >= max_rows => h + 1 - max_rows,
                    _ => 0,
                };
                for (rank, entry) in ranked.iter().enumerate().skip(start).take(max_rows) {
                    let is_new = Some(rank) == highlight;
                    let style = if is_new {
                        bg.bg(rgb(ui.accent))
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        bg
                    };
                    if is_new {
                        fill(buf, Rect::new(inner.x, y, inner.width, 1), style);
                    }
                    let date = entry.date.get(..10).unwrap_or(&entry.date);
                    let player: String = entry.player.chars().take(22).collect();
                    text(
                        buf,
                        inner,
                        cols[0],
                        y as i32,
                        &(rank + 1).to_string(),
                        style,
                    );
                    text(buf, inner, cols[1], y as i32, date, style);
                    text(buf, inner, cols[2], y as i32, &player, style);
                    text(
                        buf,
                        inner,
                        cols[3],
                        y as i32,
                        &format_duration(entry.duration),
                        style,
                    );
                    y += 1;
                }
            }
        }
        Dialog::Rules => {
            line(buf, &mut y, "Game Rules", bold);
            y += 1;
            for (rule, detail) in RULES {
                text(
                    buf,
                    inner,
                    inner.x as i32 + 2,
                    y as i32,
                    "•",
                    bg.fg(rgb(ui.accent)),
                );
                text(buf, inner, inner.x as i32 + 4, y as i32, rule, bg);
                y += 1;
                if !detail.is_empty() {
                    text(buf, inner, inner.x as i32 + 4, y as i32, detail, dim);
                }
                y += 1;
            }
        }
        Dialog::Shortcuts => {
            line(buf, &mut y, "Keyboard Shortcuts", bold);
            y += 1;
            for (keys, what) in SHORTCUTS {
                text(
                    buf,
                    inner,
                    inner.x as i32 + 2,
                    y as i32,
                    keys,
                    bg.fg(rgb(ui.accent)),
                );
                text(buf, inner, inner.x as i32 + 28, y as i32, what, bg);
                y += 1;
            }
        }
        Dialog::About => {
            line(buf, &mut y, "Mahjongg", bold);
            line(
                buf,
                &mut y,
                concat!("Version ", env!("CARGO_PKG_VERSION")),
                dim,
            );
            y += 1;
            line(buf, &mut y, "Match tiles and clear the board", bg);
            line(buf, &mut y, "A terminal rendition of GNOME Mahjongg,", bg);
            line(buf, &mut y, "built with Rust and Ratatui.", bg);
            y += 1;
            line(
                buf,
                &mut y,
                "Layouts from GNOME Mahjongg by Rexford Newbould,",
                dim,
            );
            line(buf, &mut y, "Krzysztof Foltman and Sapphire Becker.", dim);
            line(buf, &mut y, "License: GPL-3.0-or-later", dim);
        }
    }

    let buttons: Vec<_> = app
        .buttons()
        .into_iter()
        .enumerate()
        .map(|(i, b)| (b.label.to_string(), b.kind, i == focus, b.action))
        .collect();
    button_row(buf, app, ui, inner, inner.bottom() - 1, &buttons);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::load_maps;

    #[test]
    fn every_layout_fits_a_standard_terminal_in_compact_mode() {
        for map in load_maps() {
            let g = compute_geometry(&map, Rect::new(0, 3, 80, 21));
            assert!(g.is_ok(), "{} needs {:?}", map.name, g.err());
        }
    }

    #[test]
    fn tiny_terminals_never_panic() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let overlays = [
            None,
            Some(KeyCode::Char('m')),
            Some(KeyCode::Char('s')),
            Some(KeyCode::F(1)),
        ];
        for open in overlays {
            let mut app = App::new();
            let (a, b) = app.game.solution[0];
            app.game.remove_pair(a, b);
            if let Some(code) = open {
                app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            for w in 0..24 {
                for h in 0..12 {
                    let mut term =
                        ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
                    term.draw(|f| draw(f, &mut app)).unwrap();
                }
            }
        }
    }

    #[test]
    fn large_mode_used_when_room() {
        let maps = load_maps();
        let g = compute_geometry(&maps[0], Rect::new(0, 3, 200, 60)).unwrap();
        assert!(g.large);
    }
}

/// Renders frames to HTML for visual inspection:
/// `SNAPSHOT_DIR=/tmp/x cargo test snapshot -- --ignored`
#[cfg(test)]
mod snapshot {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn css(c: Color, default: &str) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::White => "#ffffff".into(),
            Color::Black => "#000000".into(),
            _ => default.into(),
        }
    }

    fn to_html(buf: &Buffer) -> String {
        let mut html = String::from(
            "<html><head><meta charset='utf-8'></head><body style='margin:0;background:#1e1e1e'><pre style='font:16px/1.15 Menlo,monospace;margin:0;color:#ddd'>",
        );
        for y in 0..buf.area.height {
            let mut skip = 0;
            for x in 0..buf.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let cell = &buf[(x, y)];
                let mut fg = css(cell.fg, "#dddddd");
                let mut bg = css(cell.bg, "#1e1e1e");
                if cell.modifier.contains(Modifier::REVERSED) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let sym = cell.symbol();
                let w = sym.width();
                skip = w.saturating_sub(1);
                let bold = if cell.modifier.contains(Modifier::BOLD) {
                    "font-weight:bold;"
                } else {
                    ""
                };
                let esc = sym.replace('&', "&amp;").replace('<', "&lt;");
                html.push_str(&format!(
                    "<span style='color:{fg};background:{bg};{bold}display:inline-block;width:{}ch'>{esc}</span>",
                    w.max(1)
                ));
            }
            html.push('\n');
        }
        html.push_str("</pre></body></html>");
        html
    }

    fn shot(app: &mut App, w: u16, h: u16, name: &str) {
        let dir = std::env::var("SNAPSHOT_DIR").unwrap();
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        std::fs::write(
            format!("{dir}/{name}.html"),
            to_html(term.backend().buffer()),
        )
        .unwrap();
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    #[ignore = "writes HTML snapshots to $SNAPSHOT_DIR"]
    fn snapshot() {
        let mut app = App::new();
        app.game.generate(Some(12345));
        shot(&mut app, 110, 42, "large");
        shot(&mut app, 80, 24, "small");

        // Select a tile via keyboard, and show a hint.
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Tab);
        shot(&mut app, 110, 42, "selected");

        for &(a, b) in app.game.solution.clone().iter().take(30) {
            app.game.remove_pair(a, b);
        }
        shot(&mut app, 110, 42, "midgame");
        app.settings.theme = crate::theme::TileTheme::Educational;
        app.settings.background = crate::theme::Background::Dark;
        shot(&mut app, 110, 42, "educational-dark");
        app.settings.theme = crate::theme::TileTheme::Smooth;
        app.settings.background = crate::theme::Background::Light;
        shot(&mut app, 110, 42, "smooth-light");

        key(&mut app, KeyCode::Char('m'));
        shot(&mut app, 110, 42, "menu");
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        shot(&mut app, 110, 42, "menu-layout");
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('p'));
        shot(&mut app, 110, 42, "paused");
        key(&mut app, KeyCode::Char('p'));
        key(&mut app, KeyCode::Char('?'));
        shot(&mut app, 80, 24, "shortcuts");
        key(&mut app, KeyCode::Esc);

        for (i, map) in app.maps.clone().iter().enumerate() {
            app.map_index = i;
            app.game = crate::game::Game::new(map.clone());
            app.game.generate(Some(99));
            shot(&mut app, 110, 42, &format!("layout-{i}"));
        }
        let solution = app.game.solution.clone();
        for (a, b) in solution {
            app.game.remove_pair(a, b);
        }
        app.history.add(&app.game.map.score_name, 312);
        app.history.save().unwrap();
        app.perform(crate::app::Action::Scores);
        shot(&mut app, 110, 42, "scores");
    }
}
