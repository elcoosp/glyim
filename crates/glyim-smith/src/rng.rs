//! Deterministic, dependency-free RNG for program generation.
//!
//! Plan CO-5 (`docs/roadmaps/glyim-master-plan.md` §5.3): the generator is a
//! **pure function of its seed** — no clock, no system entropy, no external
//! `rand` dependency (supply-chain surface stays empty). The core is
//! xoshiro256\*\* seeded through `SplitMix64`, which is the standard
//! avalanche-safe way to expand a single `u64` seed into the four-word
//! state; both algorithms are small enough to keep the whole generator
//! auditable and reproducible across machines (constitution C6).

/// `xoshiro256**` generator seeded through `SplitMix64`.
///
/// Deterministic by construction: `SmithRng::new(s)` followed by the same
/// call sequence produces the same stream on every machine and every run
/// (no threading through the interpreter, no `Instant::now`, no OS entropy).
pub struct SmithRng {
    state: [u64; 4],
}

impl SmithRng {
    /// Creates a generator from a single 64-bit seed.
    ///
    /// The seed is expanded with `SplitMix64` (Steele et al.), which
    /// guarantees that nearby seeds produce well-scrambled initial states —
    /// the property the corpus's `(family, seed)` identity already relies on.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut mix = seed;
        let mut state = [0u64; 4];
        for slot in &mut state {
            mix = mix.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = mix;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *slot = z ^ (z >> 31);
        }
        Self { state }
    }

    /// Draws the next 64-bit value (xoshiro256\*\*).
    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }

    /// Draws a value in `0..n` (`n` must be non-zero).
    ///
    /// The modulo is fine for generation: the biases are far below the
    /// structural diversity the profiles synthesize.
    #[must_use]
    pub fn below(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0, "below() requires a non-zero bound");
        self.next_u64() % n
    }

    /// Draws a value in `lo..=hi` inclusive.
    ///
    /// `i32` throughout: the generator's value domain is Glyim's `i32`
    /// scalar range, and every caller treats the draw as an `i32` constant.
    #[must_use]
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        debug_assert!(lo <= hi, "range() bounds are inverted");
        lo + self.below((hi - lo + 1) as u64) as i32
    }

    /// Draws one element out of a non-empty slice.
    #[must_use]
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        debug_assert!(!items.is_empty(), "choose() requires a non-empty slice");
        &items[self.below(items.len() as u64) as usize]
    }

    /// Returns `true` with `percent`% probability.
    #[must_use]
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

#[cfg(test)]
mod tests {
    use super::SmithRng;

    #[test]
    fn same_seed_same_stream() {
        let mut a = SmithRng::new(42);
        let mut b = SmithRng::new(42);
        for _ in 0..1_000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_different_stream() {
        let mut a = SmithRng::new(42);
        let mut b = SmithRng::new(43);
        let equal = (0..1_000).filter(|_| a.next_u64() == b.next_u64()).count();
        assert!(equal < 10, "distinct seeds produced near-identical streams");
    }

    #[test]
    fn bounded_draws_stay_in_bounds() {
        let mut rng = SmithRng::new(7);
        for _ in 0..10_000 {
            assert!(rng.below(5) < 5);
            assert!((2..=10).contains(&rng.range(2, 10)));
        }
    }
}
