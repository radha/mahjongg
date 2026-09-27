//! Layout ("map") loading. The layouts are GNOME Mahjongg's own `mahjongg.map`,
//! parsed the same way GNOME does: coordinates are doubled into half-tile units.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Horizontal position in half-tile units.
    pub x: i32,
    /// Vertical position in half-tile units.
    pub y: i32,
    pub layer: i32,
}

#[derive(Clone, Debug)]
pub struct Map {
    pub name: String,
    pub score_name: String,
    /// Sorted lowest layer first, then diagonally from top-right to bottom-left
    /// (GNOME's drawing order).
    pub slots: Vec<Slot>,
}

const MAP_DATA: &str = include_str!("../data/mahjongg.map");

pub fn load_maps() -> Vec<Map> {
    parse_maps(MAP_DATA)
}

pub fn parse_maps(data: &str) -> Vec<Map> {
    let mut maps = Vec::new();
    let mut current: Option<Map> = None;
    let mut layer_z = 0;
    let mut rest = data;

    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(end) => {
                    rest = &rest[end + 3..];
                    continue;
                }
                None => break,
            }
        }
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];

        if let Some(name) = tag.strip_prefix('/') {
            match name.trim().to_ascii_lowercase().as_str() {
                "map" => {
                    if let Some(mut map) = current.take() {
                        let n = map.slots.len();
                        if n > 0 && n <= 144 && n % 2 == 0 {
                            map.slots.sort_by(compare_slots);
                            maps.push(map);
                        }
                    }
                }
                "layer" => layer_z = 0,
                _ => {}
            }
            continue;
        }

        let tag = tag.trim_end_matches('/').trim();
        let (name, attr_text) = match tag.find(char::is_whitespace) {
            Some(i) => (&tag[..i], &tag[i..]),
            None => (tag, ""),
        };
        let attrs = parse_attrs(attr_text);
        let get = |key: &str| -> Option<f64> {
            attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .and_then(|(_, v)| v.trim().parse().ok())
        };
        let get_str = |key: &str| -> String {
            attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let half = |key: &str| (get(key).unwrap_or(0.0) * 2.0) as i32;

        let name = name.to_ascii_lowercase();
        if name == "map" {
            current = Some(Map {
                name: get_str("name"),
                score_name: get_str("scorename"),
                slots: Vec::new(),
            });
            continue;
        }
        let Some(map) = current.as_mut() else {
            continue;
        };
        let z = get("z").map(|z| z as i32).unwrap_or(layer_z);
        match name.as_str() {
            "layer" => layer_z = get("z").unwrap_or(0.0) as i32,
            "row" => {
                let (x2, y) = (half("right"), half("y"));
                let mut x = half("left");
                while x <= x2 {
                    map.slots.push(Slot { x, y, layer: z });
                    x += 2;
                }
            }
            "column" => {
                let (x, y2) = (half("x"), half("bottom"));
                let mut y = half("top");
                while y <= y2 {
                    map.slots.push(Slot { x, y, layer: z });
                    y += 2;
                }
            }
            "block" => {
                let (x2, y1, y2) = (half("right"), half("top"), half("bottom"));
                let mut x = half("left");
                while x <= x2 {
                    let mut y = y1;
                    while y <= y2 {
                        map.slots.push(Slot { x, y, layer: z });
                        y += 2;
                    }
                    x += 2;
                }
            }
            "tile" => map.slots.push(Slot {
                x: half("x"),
                y: half("y"),
                layer: z,
            }),
            _ => {}
        }
    }
    maps
}

fn parse_attrs(text: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = text;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(quote) else {
            break;
        };
        attrs.push((key, after[1..1 + close].to_string()));
        rest = &after[close + 2..];
    }
    attrs
}

/// Lowest layer first; within a layer, diagonally from top-right to bottom-left.
fn compare_slots(a: &Slot, b: &Slot) -> Ordering {
    a.layer
        .cmp(&b.layer)
        .then_with(|| (b.x - b.y).cmp(&(a.x - a.y)))
        .then_with(|| a.x.cmp(&b.x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn loads_all_gnome_layouts() {
        let maps = load_maps();
        let names: Vec<_> = maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Turtle",
                "The Ziggurat",
                "Four Bridges",
                "Cloud",
                "Tic-Tac-Toe",
                "Red Dragon",
                "Overpass",
                "Pyramid's Walls",
                "Confounding Cross",
                "Taipei"
            ]
        );
        for map in &maps {
            assert_eq!(map.slots.len(), 144, "{}", map.name);
            let unique: HashSet<_> = map.slots.iter().collect();
            assert_eq!(unique.len(), 144, "duplicate slots in {}", map.name);
        }
    }

    #[test]
    fn turtle_matches_classic_shape() {
        let maps = load_maps();
        let turtle = &maps[0];
        let per_layer = |l| turtle.slots.iter().filter(|s| s.layer == l).count();
        assert_eq!(
            (0..5).map(per_layer).collect::<Vec<_>>(),
            [87, 36, 16, 4, 1]
        );
        assert!(turtle.slots.contains(&Slot {
            x: 13,
            y: 7,
            layer: 4
        }));
    }
}
