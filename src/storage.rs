//! Settings, score history and the saved game, stored as small text files
//! under `$XDG_CONFIG_HOME` / `$XDG_DATA_HOME` (defaulting to `~/.config` and
//! `~/.local/share`).

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::game::{Game, TileState};
use crate::map::{Map, Slot};
use crate::theme::{Background, TileTheme};

const APP_DIR: &str = "tui-mahjongg";

#[cfg(not(test))]
fn base_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(var).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(APP_DIR));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback).join(APP_DIR))
}

/// Tests never touch the real config: each test thread gets its own empty directory.
#[cfg(test)]
fn base_dir(_var: &str, _fallback: &str) -> Option<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static DIR: PathBuf = std::env::temp_dir().join(format!(
            "{APP_DIR}-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    }
    Some(DIR.with(PathBuf::clone))
}

fn config_path(name: &str) -> Option<PathBuf> {
    base_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join(name))
}

fn data_path(name: &str) -> Option<PathBuf> {
    base_dir("XDG_DATA_HOME", ".local/share").map(|d| d.join(name))
}

/// Write via a temporary file and rename, so a crash never leaves a truncated file.
fn write_file(path: Option<PathBuf>, contents: &str) -> io::Result<()> {
    let Some(path) = path else { return Ok(()) };
    let write = || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, contents)?;
        fs::rename(&tmp, &path)
    };
    write().map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
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

    pub fn save(&self) -> io::Result<()> {
        let text = format!(
            "layout={}\nprogression={}\nbackground={}\ntheme={}\n",
            self.layout,
            self.progression.key(),
            self.background.key(),
            self.theme.key()
        );
        write_file(config_path("settings"), &text)
    }
}

// ----- Score history ----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// ISO 8601 local timestamp with UTC offset.
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
    /// Lines this version can't parse, kept so saving never drops them.
    unparsed: Vec<String>,
}

impl History {
    pub fn load() -> Self {
        let path = data_path("history");
        let text = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        Self::parse(path, &text)
    }

    fn parse(path: Option<PathBuf>, text: &str) -> Self {
        let mut entries = vec![];
        let mut unparsed = vec![];
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let mut parts = line.splitn(4, ' ');
            let entry = (|| {
                Some(HistoryEntry {
                    date: parts.next()?.to_string(),
                    name: parts.next()?.to_string(),
                    duration: parts.next()?.parse().ok()?,
                    player: parts.next().unwrap_or("").to_string(),
                })
            })();
            match entry {
                Some(e) => entries.push(e),
                None => unparsed.push(line.to_string()),
            }
        }
        History {
            path,
            entries,
            unparsed,
        }
    }

    fn to_text(&self) -> String {
        let mut text = String::new();
        for line in &self.unparsed {
            text.push_str(line);
            text.push('\n');
        }
        for e in &self.entries {
            text.push_str(&format!(
                "{} {} {} {}\n",
                e.date, e.name, e.duration, e.player
            ));
        }
        text
    }

    pub fn save(&self) -> io::Result<()> {
        write_file(self.path.clone(), &self.to_text())
    }

    /// Record a finished game. Call [`History::save`] afterwards.
    pub fn add(&mut self, name: &str, duration: u32) -> HistoryEntry {
        let entry = HistoryEntry {
            date: now_iso8601(),
            name: name.to_string(),
            duration,
            player: player_name(),
        };
        self.entries.push(entry.clone());
        entry
    }

    /// Remove every score. Call [`History::save`] afterwards.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.unparsed.clear();
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

/// Seconds east of UTC for the local time zone at `secs` since the epoch.
#[cfg(unix)]
fn utc_offset(secs: i64) -> i64 {
    let time = secs as libc::time_t;
    // SAFETY: `tm` is plain data, and localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

#[cfg(not(unix))]
fn utc_offset(_secs: i64) -> i64 {
    0
}

/// The current local time, e.g. `2026-10-03T16:16:00+05:30`.
fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_iso8601(secs, utc_offset(secs))
}

fn format_iso8601(utc_secs: i64, offset: i64) -> String {
    let secs = utc_secs + offset;
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
    let zone = if offset == 0 {
        "Z".to_string()
    } else {
        let sign = if offset < 0 { '-' } else { '+' };
        let m = offset.abs() / 60;
        format!("{sign}{:02}:{:02}", m / 60, m % 60)
    };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{zone}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ----- Saved game -------------------------------------------------------------

pub struct SavedGame {
    pub map: String,
    pub seed: u64,
    pub clock: Duration,
    pub current_move: u32,
    pub tiles: Vec<TileState>,
}

/// Only dealt games are saved, so every tile has a number.
pub fn write_save(game: &Game) -> io::Result<()> {
    let mut text = format!(
        "map={}\nseed={}\nclock={}\nmove={}\n",
        game.map.name,
        game.seed,
        game.elapsed().as_secs_f64(),
        game.current_move()
    );
    for t in &game.tiles {
        let Some(number) = t.number else {
            return Ok(());
        };
        text.push_str(&format!(
            "tile={} {} {} {} {} {}\n",
            t.slot.x,
            t.slot.y,
            t.slot.layer,
            number,
            u8::from(t.visible),
            t.mv
        ));
    }
    write_file(data_path("gamesave"), &text)
}

pub fn delete_save() -> io::Result<()> {
    let Some(path) = data_path("gamesave") else {
        return Ok(());
    };
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
        }
        _ => Ok(()),
    }
}

pub fn load_save(maps: &[Map]) -> Option<SavedGame> {
    parse_save(&fs::read_to_string(data_path("gamesave")?).ok()?, maps)
}

fn parse_save(text: &str, maps: &[Map]) -> Option<SavedGame> {
    let mut save = SavedGame {
        map: String::new(),
        seed: 0,
        clock: Duration::ZERO,
        current_move: 1,
        tiles: vec![],
    };
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "map" => save.map = value.to_string(),
            "seed" => save.seed = value.parse().ok()?,
            // Rejects negative, NaN and infinite times.
            "clock" => save.clock = Duration::try_from_secs_f64(value.parse().ok()?).ok()?,
            "move" => save.current_move = value.parse().ok()?,
            "tile" => {
                let mut v = value.split(' ');
                let mut next = || v.next();
                save.tiles.push(TileState {
                    slot: Slot {
                        x: next()?.parse().ok()?,
                        y: next()?.parse().ok()?,
                        layer: next()?.parse().ok()?,
                    },
                    number: next()?.parse().ok()?,
                    visible: next()?.parse::<u8>().ok()? != 0,
                    mv: next()?.parse().ok()?,
                });
            }
            _ => {}
        }
    }
    let map = maps.iter().find(|m| m.name == save.map)?;
    let valid = save.tiles.len() == map.slots.len()
        && map
            .slots
            .iter()
            .all(|s| save.tiles.iter().any(|t| t.slot == *s))
        && save.tiles.iter().any(|t| t.visible);
    valid.then_some(save)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::map::load_maps;

    #[test]
    fn iso_date_format() {
        let d = now_iso8601();
        assert!(d.len() == 20 || d.len() == 25, "{d}");
        assert!(d.as_bytes()[10] == b'T' && d.starts_with("20"));
        assert_eq!(format_iso8601(0, 0), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_iso8601(1_790_000_000, 19_800),
            "2026-09-21T19:43:20+05:30"
        );
        assert_eq!(format_iso8601(0, -3_600), "1969-12-31T23:00:00-01:00");
    }

    #[test]
    fn save_round_trip() {
        let maps = load_maps();
        let mut game = Game::new(maps[2].clone());
        game.generate(Some(9));
        let (a, b) = game.solution[0];
        game.remove_pair(a, b);
        write_save(&game).unwrap();

        let save = load_save(&maps).unwrap();
        let mut restored = Game::new(maps[2].clone());
        restored.restore(save.seed, save.current_move, save.clock, &save.tiles);
        assert!(restored.is_valid_deal() && restored.paused());
        assert_eq!(restored.current_move(), 2);
        for (x, y) in game.tiles.iter().zip(&restored.tiles) {
            assert_eq!((x.number, x.visible, x.mv), (y.number, y.visible, y.mv));
        }

        delete_save().unwrap();
        assert!(load_save(&maps).is_none());
        delete_save().unwrap();
    }

    #[test]
    fn corrupt_clock_rejected() {
        let maps = load_maps();
        let mut game = Game::new(maps[0].clone());
        game.generate(Some(1));
        let (a, b) = game.solution[0];
        game.remove_pair(a, b);
        write_save(&game).unwrap();
        let text = fs::read_to_string(data_path("gamesave").unwrap()).unwrap();
        assert!(parse_save(&text, &maps).is_some());
        for bad in ["NaN", "-5", "inf"] {
            let edited: String = text
                .lines()
                .map(|l| {
                    if l.starts_with("clock=") {
                        format!("clock={bad}\n")
                    } else {
                        format!("{l}\n")
                    }
                })
                .collect();
            assert!(parse_save(&edited, &maps).is_none(), "{bad}");
        }
    }

    #[test]
    fn history_keeps_unparsed_lines() {
        let text = "# from a newer version\n2026-01-01T00:00:00Z easy 300 Ann Lee\n";
        let h = History::parse(None, text);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].player, "Ann Lee");
        assert_eq!(h.to_text(), text);
    }
}
