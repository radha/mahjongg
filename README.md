# Mahjongg (TUI)

A terminal clone of [GNOME Mahjongg](https://gitlab.gnome.org/GNOME/gnome-mahjongg),
the tile-matching solitaire, written in Rust with [Ratatui](https://ratatui.rs).

```sh
cargo run --release
```

## What matches GNOME Mahjongg

- **All 10 official layouts** (Turtle, The Ziggurat, Four Bridges, Cloud, Tic-Tac-Toe,
  Red Dragon, Overpass, Pyramid's Walls, Confounding Cross, Taipei), loaded from GNOME's
  own `mahjongg.map`.
- **Rules and deal generation are ported from `game.vala`.** Every deal is solvable. A tile is
  free when nothing is on top of it and its left or right side is open. Seasons match
  seasons and flowers match flowers.
- **Header bar**: Undo / Redo, a clock with "Moves Left" underneath, Hint, Pause and the main menu.
- **Penalties**: a hint adds 30 seconds and a reshuffle adds 60, the same as GNOME.
  Undo and redo are free.
- **Pause** hides the tile faces. Opening a menu or dialog pauses the game automatically.
- **"No Moves Left" dialog** offers Quit / New Game / Reshuffle / Continue.
- **Scores**: best times per layout, shown in a table after each win.
- **Menu**: New Game, Restart Game, Scores, Layout, Layout Progression (none / sequential /
  random), Appearance (background Follow System / Light / Dark, and the
  Postmodern / Smooth / Educational tile themes), Game Rules, Keyboard Shortcuts, About.
- **Unfinished games** are saved when you quit and restored (paused) on the next launch.
- **Clicking a blocked tile** makes it shake. Double-clicking the background when every tile
  is free auto-finishes the game.

## Controls

The game is fully playable with the mouse. On the keyboard:

| Keys | Action |
|---|---|
| Arrow keys | Move the tile cursor |
| Tab / Shift+Tab | Jump between free tiles |
| Enter / Space | Select / match the tile under the cursor |
| Ctrl+Z, U | Undo |
| Ctrl+Y, Shift+U | Redo |
| Ctrl+H, H | Hint |
| Esc, P, Ctrl+P | Pause / resume |
| Ctrl+N, N | New game |
| Ctrl+R | Restart game |
| S | Scores |
| F10, M | Main menu |
| F1 | Game rules |
| ? | Keyboard shortcuts |
| A | Auto-finish (when every tile is free) |
| Ctrl+Q, Q | Quit (the game is saved) |

## Terminal requirements

- The board is drawn at the largest size that fits. Large tiles need about 100×40 for
  Turtle. Compact tiles fit in 80×24.
- A font with CJK glyphs (萬 東 中 …) and block elements. Almost every modern terminal
  font falls back to one automatically.
- Truecolor is used when the terminal supports it (`COLORTERM=truecolor`, iTerm2, WezTerm,
  Kitty, Ghostty…). Otherwise colours are mapped to the 256-colour palette.

## Files

- Settings: `$XDG_CONFIG_HOME/tui-mahjongg/settings` (default `~/.config`)
- Scores and saved game: `$XDG_DATA_HOME/tui-mahjongg/` (default `~/.local/share`)

## License

GPL-3.0-or-later. The layout data in `data/mahjongg.map` comes from GNOME Mahjongg
(© Mahjongg Contributors, GPL-3.0-or-later), and the game logic is a port of GNOME's.
