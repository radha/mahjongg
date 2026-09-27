//! Settings, score history and the saved game, stored as small text files
//! under `$XDG_CONFIG_HOME` / `$XDG_DATA_HOME` (defaulting to `~/.config` and
//! `~/.local/share`).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::game::Game;
use crate::map::{Map, Slot};
use crate::theme::{Background, TileTheme};

const APP_DIR: &str = "tui-mahjongg";

fn base_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(var).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(APP_DIR));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback).join(APP_DIR))
}

fn config_path(name: &str) -> Option<PathBuf> {
    base_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join(name))
}

fn data_path(name: &str) -> Option<PathBuf> {
    base_dir("XDG_DATA_HOME", ".local/share").map(|d| d.join(name))
}

fn write_file(path: Option<PathBuf>, contents: &str) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, contents);
}

// ----- Settings -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progression {
    Single,
    Sequential,
    Random,
}

impl Progression {
    pub const ALL: [Progression; 3] = [
        Progression::Single,
        Progression::Sequential,
        Progression::Random,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Progression::Single => "No Progression",
            Progression::Sequential => "Sequential",
            Progression::Random => "Random",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Progression::Single => "single",
            Progression::Sequential => "sequential",
            Progression::Random => "random",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub layout: String,
    pub progression: Progression,
    pub background: Background,
    pub theme: TileTheme,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            layout: "Turtle".into(),
            progression: Progression::Single,
            background: Background::System,
            theme: TileTheme::Postmodern,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut settings = Settings::default();
        let Some(text) = config_path("settings").and_then(|p| fs::read_to_string(p).ok()) else {
            return settings;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "layout" => settings.layout = value.to_string(),
                "progression" => {
                    if let Some(p) = Progression::ALL.into_iter().find(|p| p.key() == value) {
                        settings.progression = p;
                    }
                }
                "background" => {
                    if let Some(b) = Background::from_key(value) {
                        settings.background = b;
                    }
                }
                "theme" => {
                    if let Some(t) = TileTheme::from_key(value) {
                        settings.theme = t;
                    }
                }
                _ => {}
            }
        }
        settings
    }

    pub fn save(&self) {
        let text = format!(
            "layout={}\nprogression={}\nbackground={}\ntheme={}\n",
            self.layout,
            self.progression.key(),
            self.background.key(),
            self.theme.key()
        );
        write_file(config_path("settings"), &text);
    }
}

// ----- Score history ----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// ISO 8601 UTC timestamp.
    pub date: String,
    /// Layout score name (e.g. "easy" for Turtle).
    pub name: String,
    /// Seconds.
    pub duration: u32,
    pub player: String,
}

pub struct History {
    path: Option<PathBuf>,
    pub entries: Vec<HistoryEntry>,
}

impl History {
    pub fn load() -> Self {
        let path = data_path("history");
        let text = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(4, ' ');
                Some(HistoryEntry {
                    date: parts.next()?.to_string(),
                    name: parts.next()?.to_string(),
                    duration: parts.next()?.parse().ok()?,
                    player: parts.next().unwrap_or("").to_string(),
                })
            })
            .collect();
        History { path, entries }
    }

    fn save(&self) {
        let text: String = self
            .entries
            .iter()
            .map(|e| format!("{} {} {} {}\n", e.date, e.name, e.duration, e.player))
            .collect();
        write_file(self.path.clone(), &text);
    }

    pub fn add(&mut self, name: &str, duration: u32) -> HistoryEntry {
        let entry = HistoryEntry {
            date: now_iso8601(),
            name: name.to_string(),
            duration,
            player: player_name(),
        };
        self.entries.push(entry.clone());
        self.save();
        entry
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    /// Entries for a layout, best (shortest) time first.
    pub fn ranked(&self, score_name: &str) -> Vec<HistoryEntry> {
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.name == score_name)
            .cloned()
            .collect();
        entries.sort_by(|a, b| {
            a.duration
                .cmp(&b.duration)
                .then_with(|| a.date.cmp(&b.date))
        });
        entries
    }
}

fn player_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Player".into())
}

fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ----- Saved game -------------------------------------------------------------

pub struct SavedGame {
    pub map: String,
    pub seed: u64,
    pub clock: f64,
    pub current_move: u32,
    pub tiles: Vec<(Slot, i32, bool, u32)>,
}

pub fn write_save(game: &Game) {
    let mut text = format!(
        "map={}\nseed={}\nclock={}\nmove={}\n",
        game.map.name,
        game.seed,
        game.elapsed(),
        game.current_move()
    );
    for t in &game.tiles {
        text.push_str(&format!(
            "tile={} {} {} {} {} {}\n",
            t.slot.x,
            t.slot.y,
            t.slot.layer,
            t.number,
            u8::from(t.visible),
            t.mv
        ));
    }
    write_file(data_path("gamesave"), &text);
}

pub fn delete_save() {
    if let Some(path) = data_path("gamesave") {
        let _ = fs::remove_file(path);
    }
}

pub fn load_save(maps: &[Map]) -> Option<SavedGame> {
    let text = fs::read_to_string(data_path("gamesave")?).ok()?;
    let mut save = SavedGame {
        map: String::new(),
        seed: 0,
        clock: 0.0,
        current_move: 1,
        tiles: vec![],
    };
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "map" => save.map = value.to_string(),
            "seed" => save.seed = value.parse().ok()?,
            "clock" => save.clock = value.parse().ok()?,
            "move" => save.current_move = value.parse().ok()?,
            "tile" => {
                let v: Vec<i64> = value.split(' ').filter_map(|s| s.parse().ok()).collect();
                if v.len() != 6 {
                    return None;
                }
                let slot = Slot {
                    x: v[0] as i32,
                    y: v[1] as i32,
                    layer: v[2] as i32,
                };
                save.tiles.push((slot, v[3] as i32, v[4] != 0, v[5] as u32));
            }
            _ => {}
        }
    }
    let map = maps.iter().find(|m| m.name == save.map)?;
    let valid = save.tiles.len() == map.slots.len()
        && map
            .slots
            .iter()
            .all(|s| save.tiles.iter().any(|(t, ..)| t == s))
        && save.tiles.iter().any(|(_, _, visible, _)| *visible);
    valid.then_some(save)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_format() {
        let d = now_iso8601();
        assert_eq!(d.len(), 20);
        assert!(d.ends_with('Z') && d.as_bytes()[10] == b'T');
        assert!(d.starts_with("20"));
    }
}
