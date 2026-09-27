//! Small deterministic PRNG (SplitMix64) so a seed always reproduces the same deal.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `lo..hi` (`hi` exclusive, must be > `lo`).
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        debug_assert!(hi > lo);
        lo + (self.next_u64() % (hi - lo) as u64) as usize
    }
}

pub fn random_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    let mut r = Rng::new(nanos ^ (std::process::id() as u64).rotate_left(32));
    // Keep seeds in a friendly range, like GNOME's int32 seeds.
    r.next_u64() % i32::MAX as u64
}
