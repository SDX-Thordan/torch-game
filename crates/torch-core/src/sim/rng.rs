//! Deterministic PCG32 RNG — the integer-basis randomness the whole sim is
//! built on (§27: "PCG32 RNG with integer basis-point probabilities").
//!
//! `Math.random`-style float RNGs are forbidden in the core: determinism across
//! platforms requires an exact integer algorithm. This is the seed primitive the
//! economy, traffic, and combat systems will all draw from.

const PCG_DEFAULT_INC: u64 = 1442695040888963407;
const PCG_MULT: u64 = 6364136223846793005;

/// Minimal, reproducible PCG32 generator.
#[derive(Clone, Debug)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Seed a new generator. Identical seeds always produce identical streams.
    pub fn new(seed: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: PCG_DEFAULT_INC,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    /// Next 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniformly distributed integer in `[0, bound)` (bound > 0), bias-free via
    /// rejection sampling — fully deterministic.
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0);
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    /// True with probability `bp` basis points (0..=10000). Integer-only, so it
    /// behaves identically on every platform (§27).
    pub fn chance_bp(&mut self, bp: u32) -> bool {
        self.below(10_000) < bp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Pcg32::new(42);
        let mut b = Pcg32::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Pcg32::new(1);
        let mut b = Pcg32::new(2);
        let differ = (0..32).any(|_| a.next_u32() != b.next_u32());
        assert!(differ);
    }

    #[test]
    fn below_respects_bound() {
        let mut rng = Pcg32::new(7);
        for _ in 0..10_000 {
            assert!(rng.below(6) < 6);
        }
    }

    #[test]
    fn chance_bp_bounds() {
        let mut rng = Pcg32::new(9);
        assert!(!rng.chance_bp(0)); // never
        let mut all = true;
        let mut rng2 = Pcg32::new(9);
        for _ in 0..100 {
            if !rng2.chance_bp(10_000) {
                all = false;
            }
        }
        assert!(all); // always
    }
}
