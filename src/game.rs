//! Game rules and state, following GNOME Mahjongg's `game.vala`.
//!
//! Tiles are numbered 0..144; `number / 4` is the face, so tiles with the same
//! face match (the four seasons share a face, as do the four flowers).

use std::time::{Duration, Instant};

use crate::map::{Map, Slot};
use crate::rng::{Rng, random_seed};

const HINT_BLINK: Duration = Duration::from_millis(250);
const HINT_BLINKS: u32 = 5;
const SHAKE_DURATION: Duration = Duration::from_millis(250);
const AUTOPLAY_INTERVAL: Duration = Duration::from_millis(500);
/// Upper bound on backtracking steps before giving up on one random attempt.
const SEARCH_BUDGET: u32 = 50_000;

pub type Match = (usize, usize);

#[derive(Clone, Debug)]
pub struct Tile {
    pub number: i32,
    pub visible: bool,
    /// Move on which this tile was removed (0 = never / cleared redo history).
    pub mv: u32,
    pub slot: Slot,
    left: Vec<usize>,
    right: Vec<usize>,
    above: Vec<usize>,
}

impl Tile {
    pub fn face(&self) -> i32 {
        self.number / 4
    }
}

pub struct Game {
    pub map: Map,
    pub tiles: Vec<Tile>,
    pub seed: u64,
    rng: Rng,
    selected: Option<usize>,
    hint_match: Option<Match>,
    hint_matches: Vec<Match>,
    hint_index: usize,
    hint_started: Option<Instant>,
    clock_elapsed: f64,
    clock_started: bool,
    clock_running: Option<Instant>,
    paused: bool,
    inspecting: bool,
    current_move: u32,
    shake: Option<(usize, Instant)>,
    autoplay: Option<Instant>,
    /// Removal order found while generating the deal (a guaranteed solution).
    pub solution: Vec<Match>,
}

impl Game {
    pub fn new(map: Map) -> Self {
        let mut tiles: Vec<Tile> = map
            .slots
            .iter()
            .map(|&slot| Tile {
                number: -1,
                visible: true,
                mv: 0,
                slot,
                left: vec![],
                right: vec![],
                above: vec![],
            })
            .collect();

        for i in 0..tiles.len() {
            let s = tiles[i].slot;
            for j in 0..tiles.len() {
                if i == j {
                    continue;
                }
                let t = tiles[j].slot;
                if t.y < s.y - 1 || t.y > s.y + 1 {
                    continue;
                }
                if t.layer == s.layer {
                    if t.x == s.x - 2 {
                        tiles[i].left.push(j);
                    }
                    if t.x == s.x + 2 {
                        tiles[i].right.push(j);
                    }
                } else if t.layer > s.layer && t.x >= s.x - 1 && t.x <= s.x + 1 {
                    tiles[i].above.push(j);
                }
            }
        }

        Game {
            map,
            tiles,
            seed: 0,
            rng: Rng::new(0),
            selected: None,
            hint_match: None,
            hint_matches: vec![],
            hint_index: 0,
            hint_started: None,
            clock_elapsed: 0.0,
            clock_started: false,
            clock_running: None,
            paused: false,
            inspecting: false,
            current_move: 1,
            shake: None,
            autoplay: None,
            solution: vec![],
        }
    }

    /// Deal a new solvable board. Works backwards: repeatedly remove a random
    /// pair of free blank tiles until the board is empty (backtracking on dead
    /// ends), then assign matching faces to each removed pair.
    pub fn generate(&mut self, seed: Option<u64>) {
        self.seed = seed.unwrap_or_else(random_seed);
        self.paused = false;
        self.reset_clock();
        self.set_selected(None);
        self.set_hint(None);
        self.current_move = 1;
        self.inspecting = false;
        self.shake = None;
        self.autoplay = None;
        for t in &mut self.tiles {
            t.number = -1;
            t.visible = true;
            t.mv = 0;
        }

        let n_pairs = self.tiles.len() / 2;
        let pair_numbers: Vec<i32> = (0..n_pairs as i32).map(|i| i * 2).collect();
        self.rng = Rng::new(self.seed);
        let pair_numbers = self.shuffle_pair_numbers(pair_numbers);
        self.choose_tile_pairs(&pair_numbers);

        for t in &mut self.tiles {
            t.visible = true;
            t.mv = 0;
        }
    }

    pub fn restart(&mut self) {
        self.generate(Some(self.seed));
    }

    /// Restore a saved game (tile numbers/visibility/moves, clock). Starts paused.
    pub fn restore(
        &mut self,
        seed: u64,
        current_move: u32,
        clock: f64,
        tiles: &[(Slot, i32, bool, u32)],
    ) {
        self.seed = seed;
        self.rng = Rng::new(seed);
        self.current_move = current_move.max(1);
        for tile in &mut self.tiles {
            if let Some(&(_, number, visible, mv)) = tiles.iter().find(|(s, ..)| *s == tile.slot) {
                tile.number = number;
                tile.visible = visible;
                tile.mv = mv;
            }
        }
        self.clock_started = true;
        self.clock_running = None;
        self.clock_elapsed = clock;
        self.paused = true;
    }

    pub fn is_valid_deal(&self) -> bool {
        let mut counts = [0; 36];
        for t in &self.tiles {
            if !(0..144).contains(&t.number) {
                return false;
            }
            counts[t.face() as usize] += 1;
        }
        counts.iter().all(|c| c % 2 == 0)
    }

    // ----- Queries -------------------------------------------------------

    pub fn selectable(&self, i: usize) -> bool {
        let t = &self.tiles[i];
        if !t.visible {
            return false;
        }
        let left_blocked = t.left.iter().any(|&j| self.tiles[j].visible);
        let right_blocked = t.right.iter().any(|&j| self.tiles[j].visible);
        if left_blocked && right_blocked {
            return false;
        }
        !self.covered(i)
    }

    /// True if any visible tile sits on top of this one.
    pub fn covered(&self, i: usize) -> bool {
        self.tiles[i].above.iter().any(|&j| self.tiles[j].visible)
    }

    pub fn matches(&self, a: usize, b: usize) -> bool {
        self.tiles[a].face() == self.tiles[b].face()
    }

    pub fn moves_left(&self) -> usize {
        self.find_matches(true).len()
    }

    pub fn complete(&self) -> bool {
        self.tiles.iter().all(|t| !t.visible)
    }

    pub fn can_move(&self) -> bool {
        self.moves_left() != 0
    }

    pub fn can_shuffle(&self) -> bool {
        (0..self.tiles.len())
            .filter(|&i| self.selectable(i))
            .take(2)
            .count()
            == 2
    }

    pub fn can_undo(&self) -> bool {
        self.current_move > 1
    }

    pub fn can_redo(&self) -> bool {
        self.tiles.iter().any(|t| t.mv >= self.current_move)
    }

    pub fn all_tiles_unblocked(&self) -> bool {
        (0..self.tiles.len()).all(|i| !self.tiles[i].visible || self.selectable(i))
    }

    #[cfg(test)]
    pub fn tiles_left(&self) -> usize {
        self.tiles.iter().filter(|t| t.visible).count()
    }

    pub fn current_move(&self) -> u32 {
        self.current_move
    }

    pub fn started(&self) -> bool {
        self.clock_started
    }

    pub fn inspecting(&self) -> bool {
        self.inspecting
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn elapsed(&self) -> f64 {
        if !self.clock_started {
            return 0.0;
        }
        self.clock_elapsed
            + self
                .clock_running
                .map_or(0.0, |s| s.elapsed().as_secs_f64())
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn autoplaying(&self) -> bool {
        self.autoplay.is_some()
    }

    /// Whether the tile is drawn highlighted (selected, or a blinking hint).
    pub fn highlighted(&self, i: usize) -> bool {
        if self.selected == Some(i) {
            return true;
        }
        match (self.hint_match, self.hint_started) {
            (Some((a, b)), Some(start)) if a == i || b == i => {
                let phase = start.elapsed().as_millis() / HINT_BLINK.as_millis();
                phase < HINT_BLINKS as u128 && phase.is_multiple_of(2)
            }
            _ => false,
        }
    }

    /// Horizontal shake offset (in terminal columns) for a tile clicked while blocked.
    pub fn shake_offset(&self, i: usize) -> i32 {
        match self.shake {
            Some((t, start)) if t == i && start.elapsed() < SHAKE_DURATION => {
                let s = start.elapsed().as_secs_f64();
                (std::f64::consts::TAU * 6.0 * s).sin().round() as i32
            }
            _ => 0,
        }
    }

    pub fn animating(&self) -> bool {
        self.hint_started.is_some() || self.shake.is_some() || self.autoplay.is_some()
    }

    // ----- Actions -------------------------------------------------------

    pub fn set_selected(&mut self, tile: Option<usize>) {
        self.selected = tile;
        // Hint matches depend on the selected tile.
        self.hint_matches.clear();
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        if !self.started() || self.inspecting || self.complete() {
            return;
        }
        self.paused = paused;
        if paused {
            self.stop_clock();
        } else {
            self.continue_clock();
        }
        self.set_selected(None);
        self.set_hint(None);
    }

    pub fn shake_tile(&mut self, i: usize) {
        self.shake = Some((i, Instant::now()));
    }

    pub fn remove_pair(&mut self, a: usize, b: usize) -> bool {
        if a == b || !self.tiles[a].visible || !self.tiles[b].visible || !self.matches(a, b) {
            return false;
        }
        self.set_selected(None);
        self.set_hint(None);

        // Making a move discards the redo queue.
        let current = self.current_move;
        for t in &mut self.tiles {
            if t.mv >= current {
                t.mv = 0;
            }
        }
        for i in [a, b] {
            self.tiles[i].visible = false;
            self.tiles[i].mv = current;
        }
        self.current_move += 1;

        if self.complete() {
            self.stop_clock();
            self.inspecting = true;
            self.autoplay = None;
        } else {
            self.start_clock();
        }
        true
    }

    pub fn undo(&mut self) {
        if !self.can_undo() {
            return;
        }
        self.set_selected(None);
        self.set_hint(None);
        self.current_move -= 1;
        let current = self.current_move;
        for t in &mut self.tiles {
            if t.mv == current {
                t.visible = true;
            }
        }
    }

    pub fn redo(&mut self) {
        if !self.can_redo() {
            return;
        }
        self.set_selected(None);
        self.set_hint(None);
        let current = self.current_move;
        for t in &mut self.tiles {
            if t.mv == current {
                t.visible = false;
            }
        }
        self.current_move += 1;
    }

    /// Reassign faces of the remaining tiles so the rest of the board is
    /// solvable. Costs a 60 second penalty.
    pub fn shuffle_remaining(&mut self) {
        if !self.can_shuffle() {
            return;
        }
        self.set_selected(None);
        self.set_hint(None);
        self.current_move = 1;

        let mut removed_faces = vec![];
        let mut to_shuffle = vec![];
        for (i, t) in self.tiles.iter_mut().enumerate() {
            t.mv = 0;
            if t.visible {
                to_shuffle.push(i);
            } else if !removed_faces.contains(&t.face()) {
                removed_faces.push(t.face());
            }
        }

        let mut pair_numbers = vec![];
        for &i in &to_shuffle {
            let t = &mut self.tiles[i];
            let face = t.face();
            let mut pair_number = t.number - (t.number % 2);
            t.number = -1;
            // If one pair of this face was already removed, the two survivors
            // may come from different original pairs; merge them into one.
            if removed_faces.contains(&face) {
                pair_number = face * 4;
            }
            if !pair_numbers.contains(&pair_number) {
                pair_numbers.push(pair_number);
            }
        }

        let pair_numbers = self.shuffle_pair_numbers(pair_numbers);
        self.choose_tile_pairs(&pair_numbers);
        for &i in &to_shuffle {
            self.tiles[i].visible = true;
        }

        self.start_clock();
        self.clock_elapsed += 60.0;
    }

    pub fn next_hint(&mut self) -> Option<Match> {
        if self.hint_matches.is_empty() {
            self.hint_matches = self.find_matches_for_tile(self.selected, true);
            if self.hint_matches.is_empty() {
                self.hint_matches = self.find_matches(true);
            }
            if self.hint_matches.is_empty() {
                return None;
            }
            self.hint_index = self.rng.range(0, self.hint_matches.len());
        }
        self.hint_index += 1;
        if self.hint_index >= self.hint_matches.len() {
            self.hint_index = 0;
        }
        Some(self.hint_matches[self.hint_index])
    }

    /// Blink a matching pair. Costs a 30 second penalty.
    pub fn show_hint(&mut self) {
        let hint = self.next_hint();
        self.set_hint(hint);
    }

    /// Automatically clear the board once every tile is free.
    pub fn autoplay_end_game(&mut self) {
        if !self.all_tiles_unblocked() || self.autoplay.is_some() || self.complete() {
            return;
        }
        self.autoplay = Some(Instant::now() - AUTOPLAY_INTERVAL);
    }

    /// Advance timers. Returns true if a pair was removed by autoplay.
    pub fn tick(&mut self) -> bool {
        if let Some(start) = self.hint_started
            && start.elapsed() >= HINT_BLINK * HINT_BLINKS
        {
            self.hint_started = None;
        }
        if let Some((_, start)) = self.shake
            && start.elapsed() >= SHAKE_DURATION
        {
            self.shake = None;
        }
        if let Some(last) = self.autoplay
            && last.elapsed() >= AUTOPLAY_INTERVAL
        {
            match self.next_hint() {
                Some((a, b)) => {
                    self.autoplay = Some(Instant::now());
                    self.remove_pair(a, b);
                    return true;
                }
                None => self.autoplay = None,
            }
        }
        false
    }

    // ----- Internals -----------------------------------------------------

    fn set_hint(&mut self, hint: Option<Match>) {
        self.hint_started = None;
        let Some(m) = hint else {
            self.hint_match = None;
            self.hint_matches.clear();
            return;
        };
        self.hint_match = Some(m);
        self.hint_started = Some(Instant::now());
        if self.inspecting {
            return;
        }
        self.start_clock();
        self.clock_elapsed += 30.0;
    }

    fn shuffle_pair_numbers(&mut self, mut numbers: Vec<i32>) -> Vec<i32> {
        for i in 0..numbers.len() {
            let n = self.rng.range(i, numbers.len());
            numbers.swap(i, n);
        }
        numbers
    }

    fn choose_tile_pairs(&mut self, pair_numbers: &[i32]) {
        self.solution.clear();
        for _attempt in 0..20 {
            let mut budget = SEARCH_BUDGET;
            let mut path = vec![];
            if self.search_pairs(pair_numbers, 0, true, &mut budget, &mut path) {
                self.solution = path;
                return;
            }
        }
        // Could not find a playable order (e.g. a tall stack with nothing
        // beside it); assign faces to any pairs, like GNOME's fallback.
        let mut budget = u32::MAX;
        let mut path = vec![];
        self.search_pairs(pair_numbers, 0, false, &mut budget, &mut path);
    }

    fn search_pairs(
        &mut self,
        pair_numbers: &[i32],
        depth: usize,
        check_selectable: bool,
        budget: &mut u32,
        path: &mut Vec<Match>,
    ) -> bool {
        if depth == pair_numbers.len() {
            return true;
        }
        if *budget == 0 {
            return false;
        }
        *budget -= 1;

        let matches = self.find_matches(check_selectable);
        if matches.is_empty() {
            return false;
        }
        let n = self.rng.range(0, matches.len());
        for k in 0..matches.len() {
            let (a, b) = matches[(n + k) % matches.len()];
            self.tiles[a].visible = false;
            self.tiles[b].visible = false;
            path.push((a, b));
            if self.search_pairs(pair_numbers, depth + 1, check_selectable, budget, path) {
                self.tiles[a].number = pair_numbers[depth];
                self.tiles[b].number = pair_numbers[depth] + 1;
                return true;
            }
            path.pop();
            self.tiles[a].visible = true;
            self.tiles[b].visible = true;
            if *budget == 0 {
                return false;
            }
        }
        false
    }

    fn find_matches(&self, check_selectable: bool) -> Vec<Match> {
        let candidates: Vec<usize> = (0..self.tiles.len())
            .filter(|&i| self.tiles[i].visible && (!check_selectable || self.selectable(i)))
            .collect();
        let mut matches = vec![];
        for (k, &a) in candidates.iter().enumerate() {
            for &b in &candidates[k + 1..] {
                if self.matches(a, b) {
                    matches.push((a, b));
                }
            }
        }
        matches
    }

    fn find_matches_for_tile(&self, tile: Option<usize>, check_selectable: bool) -> Vec<Match> {
        let Some(tile) = tile else { return vec![] };
        if !self.tiles[tile].visible || (check_selectable && !self.selectable(tile)) {
            return vec![];
        }
        (0..self.tiles.len())
            .filter(|&t| {
                t != tile
                    && self.tiles[t].visible
                    && self.matches(t, tile)
                    && (!check_selectable || self.selectable(t))
            })
            .map(|t| (t, tile))
            .collect()
    }

    fn start_clock(&mut self) {
        if self.clock_started {
            return;
        }
        self.clock_started = true;
        self.clock_running = Some(Instant::now());
    }

    fn stop_clock(&mut self) {
        if let Some(start) = self.clock_running.take() {
            self.clock_elapsed += start.elapsed().as_secs_f64();
        }
    }

    fn continue_clock(&mut self) {
        self.clock_started = true;
        if self.clock_running.is_none() {
            self.clock_running = Some(Instant::now());
        }
    }

    fn reset_clock(&mut self) {
        self.clock_started = false;
        self.clock_running = None;
        self.clock_elapsed = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::load_maps;

    fn new_game(map_index: usize, seed: u64) -> Game {
        let mut game = Game::new(load_maps()[map_index].clone());
        game.generate(Some(seed));
        game
    }

    #[test]
    fn every_layout_deals_a_solvable_board() {
        for map_index in 0..load_maps().len() {
            for seed in 0..5 {
                let mut game = new_game(map_index, seed);
                assert!(game.is_valid_deal(), "map {map_index} seed {seed}");
                let mut counts = [0; 36];
                for t in &game.tiles {
                    counts[t.face() as usize] += 1;
                }
                assert!(counts.iter().all(|&c| c == 4));

                // Replaying the generator's removal order must win the game.
                let solution = game.solution.clone();
                assert_eq!(solution.len(), 72, "map {map_index} seed {seed}");
                for (a, b) in solution {
                    assert!(game.selectable(a) && game.selectable(b));
                    assert!(game.remove_pair(a, b));
                }
                assert!(game.complete());
                assert!(game.inspecting());
            }
        }
    }

    #[test]
    fn same_seed_same_deal() {
        let a = new_game(0, 42);
        let b = new_game(0, 42);
        let numbers = |g: &Game| g.tiles.iter().map(|t| t.number).collect::<Vec<_>>();
        assert_eq!(numbers(&a), numbers(&b));
    }

    #[test]
    fn turtle_start_blocking_rules() {
        let game = new_game(0, 1);
        let find = |x, y, layer| {
            game.tiles
                .iter()
                .position(|t| t.slot == Slot { x, y, layer })
                .unwrap()
        };
        // Row ends are free, middles are blocked on both sides.
        assert!(game.selectable(find(2, 0, 0)));
        assert!(!game.selectable(find(4, 0, 0)));
        // Tile under the 6x6 block is covered.
        assert!(!game.selectable(find(8, 2, 0)));
        // The left end tile, and the top of the pyramid.
        assert!(game.selectable(find(0, 7, 0)));
        assert!(game.selectable(find(13, 7, 4)));
        // Tiles under the top tile are covered by it.
        assert!(!game.selectable(find(12, 6, 3)));
    }

    #[test]
    fn undo_redo_and_redo_queue_cleared_by_move() {
        let mut game = new_game(0, 7);
        let (a, b) = game.solution[0];
        let (c, d) = game.solution[1];
        assert!(game.remove_pair(a, b));
        assert!(game.remove_pair(c, d));
        assert_eq!(game.tiles_left(), 140);
        game.undo();
        assert_eq!(game.tiles_left(), 142);
        assert!(game.can_redo());
        game.redo();
        assert_eq!(game.tiles_left(), 140);
        game.undo();
        game.undo();
        assert!(!game.can_undo());
        assert!(game.remove_pair(a, b));
        assert!(!game.can_redo(), "a new move clears redo");
    }

    #[test]
    fn shuffle_keeps_board_solvable_and_adds_penalty() {
        let mut game = new_game(0, 3);
        for &(a, b) in game.solution.clone().iter().take(20) {
            game.remove_pair(a, b);
        }
        let before = game.elapsed();
        game.shuffle_remaining();
        assert!(game.elapsed() >= before + 60.0);
        assert!(game.is_valid_deal());
        assert_eq!(game.tiles_left(), 104);
        for (a, b) in game.solution.clone() {
            assert!(game.remove_pair(a, b));
        }
        assert!(game.complete());
    }

    #[test]
    fn hint_adds_penalty_and_starts_clock() {
        let mut game = new_game(0, 5);
        assert!(!game.started());
        game.show_hint();
        assert!(game.started());
        assert!(game.elapsed() >= 30.0);
        let (a, b) = game.hint_match.unwrap();
        assert!(game.matches(a, b) && game.selectable(a) && game.selectable(b));
    }

    #[test]
    fn moves_left_counts_free_matching_pairs() {
        let game = new_game(0, 11);
        let mut expected = 0;
        for a in 0..144 {
            for b in a + 1..144 {
                if game.selectable(a) && game.selectable(b) && game.matches(a, b) {
                    expected += 1;
                }
            }
        }
        assert_eq!(game.moves_left(), expected);
        assert!(expected > 0);
    }
}
