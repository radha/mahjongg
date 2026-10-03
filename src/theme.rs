//! Tile faces and colour themes (terminal renditions of GNOME's
//! Postmodern / Smooth / Educational tile sets).

use std::sync::OnceLock;

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileTheme {
    Postmodern,
    Smooth,
    Educational,
}

impl TileTheme {
    pub const ALL: [TileTheme; 3] = [
        TileTheme::Postmodern,
        TileTheme::Smooth,
        TileTheme::Educational,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TileTheme::Postmodern => "Postmodern",
            TileTheme::Smooth => "Smooth",
            TileTheme::Educational => "Educational",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TileTheme::Postmodern => "postmodern",
            TileTheme::Smooth => "smooth",
            TileTheme::Educational => "educational",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    System,
    Light,
    Dark,
}

impl Background {
    pub const ALL: [Background; 3] = [Background::System, Background::Light, Background::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Background::System => "Follow System",
            Background::Light => "Light",
            Background::Dark => "Dark",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Background::System => "system",
            Background::Light => "light",
            Background::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

// ----- Colour helpers ------------------------------------------------------

fn truecolor() -> bool {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    *TRUECOLOR.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return true;
        }
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        matches!(
            program.as_str(),
            "iTerm.app" | "WezTerm" | "ghostty" | "vscode" | "Hyper" | "rio" | "WarpTerminal"
        ) || std::env::var("KITTY_WINDOW_ID").is_ok()
    })
}

/// An RGB colour, downgraded to the 256-colour palette on terminals without truecolor.
pub fn rgb(hex: u32) -> Color {
    let (r, g, b) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if truecolor() {
        return Color::Rgb(r, g, b);
    }
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|&i| (LEVELS[i] as i32 - v as i32).abs())
            .unwrap()
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let dist = |c: (u8, u8, u8)| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(c.0, r) + d(c.1, g) + d(c.2, b)
    };
    let gray_level = ((r as u32 + g as u32 + b as u32) / 3).saturating_sub(8) / 10;
    let gray_level = gray_level.min(23) as u8;
    let gray = 8 + gray_level * 10;
    if dist((gray, gray, gray)) < dist(cube) {
        Color::Indexed(232 + gray_level)
    } else {
        Color::Indexed(16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8)
    }
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xff) as f32;
        let y = ((b >> shift) & 0xff) as f32;
        ((x + (y - x) * t).round() as u32) & 0xff
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

// ----- Palettes -------------------------------------------------------------

/// Glyph ink colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Red,
    Green,
    Blue,
    Dark,
    Flower,
    Season,
}

pub struct TilePalette {
    pub face: u32,
    pub side: u32,
    pub highlight: u32,
    pub cursor: u32,
    pub cursor_side: u32,
    red: u32,
    green: u32,
    blue: u32,
    dark: u32,
    flower: u32,
    season: u32,
}

impl TilePalette {
    pub fn for_theme(theme: TileTheme) -> Self {
        match theme {
            TileTheme::Postmodern => TilePalette {
                face: 0xfdfbf6,
                side: 0xa9a292,
                highlight: 0x99c1f1,
                cursor: 0xf8e45c,
                cursor_side: 0xe66100,
                red: 0xc01c28,
                green: 0x26a269,
                blue: 0x1c71d8,
                dark: 0x241f31,
                flower: 0x9141ac,
                season: 0xc64600,
            },
            TileTheme::Smooth => TilePalette {
                face: 0xfff6dc,
                side: 0x4f8a5b,
                highlight: 0xa3d5b0,
                cursor: 0xf6d32d,
                cursor_side: 0xc64600,
                red: 0xb3261e,
                green: 0x2b7a3d,
                blue: 0x1a5fb4,
                dark: 0x3d2b1f,
                flower: 0xa51d6d,
                season: 0xb5530b,
            },
            TileTheme::Educational => TilePalette {
                face: 0xffffff,
                side: 0x8a9bb3,
                highlight: 0x99c1f1,
                cursor: 0xf8e45c,
                cursor_side: 0xe66100,
                red: 0xe01b24,
                green: 0x2ec27e,
                blue: 0x3584e4,
                dark: 0x000000,
                flower: 0xc061cb,
                season: 0xff7800,
            },
        }
    }

    pub fn ink(&self, ink: Ink) -> u32 {
        match ink {
            Ink::Red => self.red,
            Ink::Green => self.green,
            Ink::Blue => self.blue,
            Ink::Dark => self.dark,
            Ink::Flower => self.flower,
            Ink::Season => self.season,
        }
    }

    /// Lower layers are shaded slightly darker, which reads as depth.
    pub fn face_for_layer(&self, layer: i32) -> u32 {
        let depth = (4 - layer.clamp(0, 4)) as f32;
        mix(self.face, self.side, depth * 0.07)
    }
}

pub struct UiPalette {
    pub board: Option<u32>,
    pub header: Option<u32>,
    pub header_fg: Option<u32>,
    pub dim_fg: u32,
    pub dialog: Option<u32>,
    pub dialog_fg: Option<u32>,
    pub button: u32,
    pub button_hover: u32,
    pub accent: u32,
    pub destructive: u32,
    pub border: u32,
}

impl UiPalette {
    pub fn for_background(bg: Background) -> Self {
        match bg {
            Background::System => UiPalette {
                board: None,
                header: None,
                header_fg: None,
                dim_fg: 0x8a8a8f,
                dialog: None,
                dialog_fg: None,
                button: 0x5e5c64,
                button_hover: 0x77767b,
                accent: 0x3584e4,
                destructive: 0xe01b24,
                border: 0x77767b,
            },
            Background::Light => UiPalette {
                board: Some(0xfafafb),
                header: Some(0xebebed),
                header_fg: Some(0x2e3436),
                dim_fg: 0x77767b,
                dialog: Some(0xffffff),
                dialog_fg: Some(0x241f31),
                button: 0xdeddda,
                button_hover: 0xc0bfbc,
                accent: 0x3584e4,
                destructive: 0xc01c28,
                border: 0xc0bfbc,
            },
            Background::Dark => UiPalette {
                board: Some(0x222226),
                header: Some(0x2e2e32),
                header_fg: Some(0xffffff),
                dim_fg: 0x9a9996,
                dialog: Some(0x36363a),
                dialog_fg: Some(0xffffff),
                button: 0x4a4a4f,
                button_hover: 0x5e5c64,
                accent: 0x3584e4,
                destructive: 0xe01b24,
                border: 0x5e5c64,
            },
        }
    }

    /// Colour used to fade the board when paused; `None` means terminal default.
    pub fn fade_target(&self) -> u32 {
        self.board.unwrap_or(0x303030)
    }
}

pub fn opt(c: Option<u32>) -> Color {
    c.map(rgb).unwrap_or(Color::Reset)
}

// ----- Faces ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Dots(u8),
    Bamboo(u8),
    Characters(u8),
    Wind(u8),
    /// 0 red, 1 green, 2 white.
    Dragon(u8),
    Season(u8),
    Flower(u8),
    Blank,
}

pub fn face_for(number: Option<u8>) -> Face {
    let Some(number) = number else {
        return Face::Blank;
    };
    let set = number / 4;
    let variant = number % 4;
    match set {
        0..=8 => Face::Dots(set + 1),
        9..=17 => Face::Bamboo(set - 8),
        18..=26 => Face::Characters(set - 17),
        27..=30 => Face::Wind(set - 27),
        31 => Face::Dragon(0),
        32 => Face::Dragon(1),
        33 => Face::Season(variant),
        34 => Face::Dragon(2),
        35 => Face::Flower(variant),
        _ => Face::Blank,
    }
}

/// A glyph placed at `(col, row)` inside the tile face.
pub type Glyph = (i32, i32, &'static str, Ink);

const NUMERALS: [&str; 9] = ["一", "二", "三", "四", "五", "六", "七", "八", "九"];
const FULLWIDTH: [&str; 9] = ["１", "２", "３", "４", "５", "６", "７", "８", "９"];
const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
const WINDS: [&str; 4] = ["東", "南", "西", "北"];
const WIND_LETTERS: [&str; 4] = ["E", "S", "W", "N"];
const DRAGONS: [&str; 3] = ["中", "發", "白"];
const DRAGON_LETTERS: [&str; 3] = ["R", "G", "W"];
const DRAGON_INKS: [Ink; 3] = [Ink::Red, Ink::Green, Ink::Blue];
const SEASONS: [&str; 4] = ["春", "夏", "秋", "冬"];
const FLOWERS: [&str; 4] = ["梅", "蘭", "菊", "竹"];

/// Pip positions on a 5x3 grid (cols 0/2/4, rows 0..2), as in real tiles.
fn pips(n: u8) -> &'static [(i32, i32)] {
    match n {
        1 => &[(2, 1)],
        2 => &[(2, 0), (2, 2)],
        3 => &[(0, 0), (2, 1), (4, 2)],
        4 => &[(0, 0), (4, 0), (0, 2), (4, 2)],
        5 => &[(0, 0), (4, 0), (2, 1), (0, 2), (4, 2)],
        6 => &[(0, 0), (4, 0), (0, 1), (4, 1), (0, 2), (4, 2)],
        7 => &[(0, 0), (2, 0), (4, 0), (0, 1), (4, 1), (0, 2), (4, 2)],
        8 => &[
            (0, 0),
            (2, 0),
            (4, 0),
            (0, 1),
            (4, 1),
            (0, 2),
            (2, 2),
            (4, 2),
        ],
        _ => &[
            (0, 0),
            (2, 0),
            (4, 0),
            (0, 1),
            (2, 1),
            (4, 1),
            (0, 2),
            (2, 2),
            (4, 2),
        ],
    }
}

fn dot_ink(n: u8, index: usize, row: i32) -> Ink {
    match n {
        1 | 5 if index == pips(n).len() / 2 => Ink::Red,
        3 => [Ink::Blue, Ink::Red, Ink::Green][index],
        6 | 7 if row == 0 => Ink::Green,
        6 | 7 => Ink::Red,
        9 => [Ink::Blue, Ink::Red, Ink::Green][row as usize],
        2 | 4 if row == 0 => Ink::Blue,
        2 | 4 => Ink::Green,
        _ => Ink::Blue,
    }
}

fn bamboo_ink(n: u8, index: usize, row: i32) -> Ink {
    match n {
        5 | 9 if row == 1 && pips(n)[index].0 == 2 => Ink::Red,
        7 if row == 0 && pips(n)[index].0 == 2 => Ink::Red,
        _ => Ink::Green,
    }
}

/// Glyphs for the large (5x3 content) tile face.
pub fn large_glyphs(face: Face, theme: TileTheme) -> Vec<Glyph> {
    let n_idx = |n: u8| (n - 1) as usize;
    let mut g: Vec<Glyph> = Vec::new();
    let educational = theme == TileTheme::Educational;
    match face {
        Face::Blank => {}
        Face::Dots(n) | Face::Bamboo(n) if educational => {
            let (glyph, ink) = if matches!(face, Face::Dots(_)) {
                ("筒", Ink::Blue)
            } else {
                ("索", Ink::Green)
            };
            g.push((1, 1, FULLWIDTH[n_idx(n)], ink));
            g.push((1, 2, glyph, ink));
        }
        Face::Dots(n) => {
            for (i, &(c, r)) in pips(n).iter().enumerate() {
                let sym = if n == 1 { "◉" } else { "●" };
                g.push((c, r, sym, dot_ink(n, i, r)));
            }
        }
        Face::Bamboo(1) => {
            g.push((1, 1, "鳥", Ink::Green));
        }
        Face::Bamboo(n) => {
            for (i, &(c, r)) in pips(n).iter().enumerate() {
                g.push((c, r, "▮", bamboo_ink(n, i, r)));
            }
        }
        Face::Characters(n) => {
            if educational {
                g.push((1, 1, FULLWIDTH[n_idx(n)], Ink::Dark));
            } else {
                g.push((1, 1, NUMERALS[n_idx(n)], Ink::Dark));
            }
            g.push((1, 2, "萬", Ink::Red));
        }
        Face::Wind(w) => {
            g.push((1, 1, WINDS[w as usize], Ink::Dark));
            if educational {
                g.push((4, 0, WIND_LETTERS[w as usize], Ink::Blue));
            }
        }
        Face::Dragon(d) => {
            g.push((1, 1, DRAGONS[d as usize], DRAGON_INKS[d as usize]));
            if educational {
                g.push((4, 0, DRAGON_LETTERS[d as usize], DRAGON_INKS[d as usize]));
            }
        }
        Face::Season(s) => {
            g.push((1, 1, SEASONS[s as usize], Ink::Season));
            g.push((4, 0, DIGITS[s as usize], Ink::Season));
        }
        Face::Flower(f) => {
            g.push((1, 1, FLOWERS[f as usize], Ink::Flower));
            g.push((4, 0, DIGITS[f as usize], Ink::Flower));
        }
    }
    if educational {
        match face {
            Face::Dots(n) | Face::Bamboo(n) | Face::Characters(n) => {
                let ink = match face {
                    Face::Dots(_) => Ink::Blue,
                    Face::Bamboo(_) => Ink::Green,
                    _ => Ink::Red,
                };
                g.push((4, 0, DIGITS[n_idx(n)], ink));
            }
            _ => {}
        }
    }
    g
}

/// Glyphs for the compact (3x1 content) tile face.
pub fn small_glyphs(face: Face, theme: TileTheme) -> Vec<Glyph> {
    let educational = theme == TileTheme::Educational;
    let idx = |n: u8| (n - 1) as usize;
    match face {
        Face::Blank => vec![],
        Face::Dots(n) => vec![
            (0, 0, DIGITS[idx(n)], Ink::Blue),
            (1, 0, if educational { "筒" } else { "●" }, Ink::Blue),
        ],
        Face::Bamboo(n) => vec![
            (0, 0, DIGITS[idx(n)], Ink::Green),
            (1, 0, if educational { "索" } else { "▮" }, Ink::Green),
        ],
        Face::Characters(n) => vec![(0, 0, DIGITS[idx(n)], Ink::Dark), (1, 0, "萬", Ink::Red)],
        Face::Wind(w) if educational => vec![
            (0, 0, WIND_LETTERS[w as usize], Ink::Blue),
            (1, 0, WINDS[w as usize], Ink::Dark),
        ],
        Face::Wind(w) => vec![(1, 0, WINDS[w as usize], Ink::Dark)],
        Face::Dragon(d) => vec![(1, 0, DRAGONS[d as usize], DRAGON_INKS[d as usize])],
        Face::Season(s) => vec![
            (0, 0, DIGITS[s as usize], Ink::Season),
            (1, 0, SEASONS[s as usize], Ink::Season),
        ],
        Face::Flower(f) => vec![
            (0, 0, DIGITS[f as usize], Ink::Flower),
            (1, 0, FLOWERS[f as usize], Ink::Flower),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn all_faces_fit_inside_tile() {
        for number in 0..144 {
            let face = face_for(Some(number));
            assert_ne!(face, Face::Blank);
            for theme in TileTheme::ALL {
                for (c, r, s, _) in large_glyphs(face, theme) {
                    assert!(
                        c >= 0 && c + s.width() as i32 <= 5 && (0..3).contains(&r),
                        "{face:?} {s}"
                    );
                }
                for (c, r, s, _) in small_glyphs(face, theme) {
                    assert!(
                        c >= 0 && c + s.width() as i32 <= 3 && r == 0,
                        "{face:?} {s}"
                    );
                }
            }
        }
    }

    #[test]
    fn faces_are_distinct_per_set() {
        let mut seen = std::collections::HashSet::new();
        for set in 0..36 {
            seen.insert(format!(
                "{:?}",
                large_glyphs(face_for(Some(set * 4)), TileTheme::Postmodern)
            ));
        }
        assert_eq!(seen.len(), 36);
    }
}
