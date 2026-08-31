// SPDX-License-Identifier: MPL-2.0
//! A fixed-width bit set, sized once and reused.
//!
//! Visibility is fundamentally "which of these N things can see which other N
//! things", and N is in the thousands, so the inner loops here are bitwise
//! operations on words rather than anything cleverer.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
    bits: usize,
}

impl BitSet {
    pub fn new(bits: usize) -> Self {
        BitSet { words: vec![0; bits.div_ceil(64)], bits }
    }

    pub fn len(&self) -> usize { self.bits }
    // Present because `len` without `is_empty` is a lint; vis code never asks.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool { self.bits == 0 }

    #[inline]
    pub fn set(&mut self, i: usize) {
        if i < self.bits { self.words[i >> 6] |= 1u64 << (i & 63); }
    }

    #[inline]
    pub fn test(&self, i: usize) -> bool {
        i < self.bits && self.words[i >> 6] & (1u64 << (i & 63)) != 0
    }

    /// Intersect with `other`, reporting whether the result has any bit that
    /// `already` does not -- the "did we learn anything new" test that stops
    /// the flow recursion.
    pub fn intersect_into(&mut self, a: &BitSet, b: &BitSet, already: &BitSet) -> bool {
        let mut more = 0u64;
        for i in 0..self.words.len() {
            let w = a.words[i] & b.words[i];
            self.words[i] = w;
            more |= w & !already.words[i];
        }
        more != 0
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Whether every bit set here is also set in `other`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_subset_of(&self, other: &BitSet) -> bool {
        self.words.iter().zip(&other.words).all(|(a, b)| a & !b == 0)
    }

    /// Set every bit up to the declared width.
    ///
    /// The padding past `bits` is cleared afterwards, so `count` stays honest
    /// rather than reporting the whole last word.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn fill(&mut self) {
        self.words.fill(u64::MAX);
        let extra = self.words.len() * 64 - self.bits;
        if extra > 0 && !self.words.is_empty() {
            let last = self.words.len() - 1;
            self.words[last] >>= extra;
        }
    }

    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.bits).filter(move |&i| self.test(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_test() {
        let mut b = BitSet::new(100);
        b.set(0);
        b.set(63);
        b.set(64);
        b.set(99);
        assert!(b.test(0) && b.test(63) && b.test(64) && b.test(99));
        assert!(!b.test(1) && !b.test(65));
        assert_eq!(b.count(), 4);
    }

    #[test]
    fn out_of_range_is_ignored_not_a_panic() {
        let mut b = BitSet::new(10);
        b.set(1000);
        assert!(!b.test(1000));
        assert_eq!(b.count(), 0);
    }

    #[test]
    fn fill_respects_the_declared_width() {
        // The trap: filling whole words would count padding bits as set.
        let mut b = BitSet::new(70);
        b.fill();
        assert_eq!(b.count(), 70);
        assert!(b.test(69));
        assert!(!b.test(70));
    }

    #[test]
    fn intersect_reports_whether_anything_is_new() {
        let bits = 128;
        let (mut a, mut b, mut seen) = (BitSet::new(bits), BitSet::new(bits), BitSet::new(bits));
        a.set(5); a.set(10); a.set(100);
        b.set(5); b.set(100);
        seen.set(5);

        let mut out = BitSet::new(bits);
        let more = out.intersect_into(&a, &b, &seen);
        assert!(more, "bit 100 is new");
        assert!(out.test(5) && out.test(100) && !out.test(10));

        seen.set(100);
        let more = out.intersect_into(&a, &b, &seen);
        assert!(!more, "everything in the intersection is already seen");
    }

    #[test]
    fn subset_relation() {
        let mut a = BitSet::new(64);
        let mut b = BitSet::new(64);
        a.set(1); a.set(2);
        b.set(1); b.set(2); b.set(3);
        assert!(a.is_subset_of(&b));
        assert!(!b.is_subset_of(&a));
    }
}
