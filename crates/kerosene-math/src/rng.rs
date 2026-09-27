// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A small, seedable random number generator.
//!
//! The same seed gives the same numbers on every platform and in every
//! build, which is the point: a replay, a saved game and a bug report all
//! need "random" to happen the same way twice. SplitMix64 -- one `u64` of
//! state, fast, and good enough for gameplay; not for cryptography.

/// A seedable generator. Its whole state is one `u64`
/// ([`state`](Rng::state)), which is what a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator that will produce the same numbers for the same seed.
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// A seed from a piece of text: the same text, the same seed, everywhere.
    pub fn seed_from(text: &str) -> u64 {
        // FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// The whole state, to save and give back to [`Rng::new`].
    pub fn state(&self) -> u64 {
        self.state
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number in `[lo, hi)`; `lo` when the range is empty.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        if hi <= lo {
            return lo;
        }
        lo + (hi - lo) * self.next_f32()
    }

    /// A whole number in `[lo, hi]`, both ends included; `lo` when `hi`
    /// is below it.
    pub fn range_int(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as i64
    }

    /// `true` with probability `p` (0 to 1).
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// One of `items`, or `None` when there are none.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let i = self.range_int(0, items.len() as i64 - 1) as usize;
        items.get(i)
    }
}

impl Default for Rng {
    fn default() -> Rng {
        Rng::new(0x4b45_524f_5345_4e45) // "KEROSENE"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        // And a known value, so a change to the algorithm is noticed: saved
        // games and replays depend on it.
        assert_eq!(Rng::new(0).next_u64(), 0xe220_a839_7b1d_cdaf);
    }

    #[test]
    fn a_saved_state_carries_on_where_it_left_off() {
        let mut a = Rng::new(7);
        a.next_u64();
        let mut b = Rng::new(a.state());
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn ranges_stay_in_range() {
        let mut r = Rng::new(1);
        for _ in 0..1000 {
            let f = r.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&f));
            let i = r.range_int(1, 6);
            assert!((1..=6).contains(&i));
        }
        assert_eq!(r.range(5.0, 5.0), 5.0);
        assert_eq!(r.range_int(3, 1), 3);
        assert_eq!(r.pick::<u8>(&[]), None);
        assert!([1, 2, 3].contains(r.pick(&[1, 2, 3]).unwrap()));
    }

    #[test]
    fn a_text_seed_is_stable() {
        assert_eq!(Rng::seed_from("kero"), Rng::seed_from("kero"));
        assert_ne!(Rng::seed_from("kero"), Rng::seed_from("kerp"));
    }
}
