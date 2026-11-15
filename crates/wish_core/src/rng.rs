//! Deterministic random numbers.
//!
//! The engine never uses the operating system's randomness or a library
//! whose sequence may change between versions. [`Rng`] is xoshiro256++,
//! written out here and checked against reference values, seeded through
//! SplitMix64. The same seed gives the same sequence on every platform and
//! in every release.

/// SplitMix64: turns any 64-bit seed into a well-mixed sequence. Used only to
/// expand a seed into an [`Rng`] state.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A deterministic random number generator (xoshiro256++).
///
/// Fast, small (32 bytes), and fully determined by its seed. Not suitable
/// for cryptography.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    /// A generator whose whole sequence is determined by `seed`.
    pub fn from_seed(seed: u64) -> Self {
        let mut mixer = seed;
        let state = [
            splitmix64(&mut mixer),
            splitmix64(&mut mixer),
            splitmix64(&mut mixer),
            splitmix64(&mut mixer),
        ];
        Self::from_state(state)
    }

    /// A generator continuing from a saved state (see [`Rng::state`]). An
    /// all-zero state would only ever produce zeros, so it is replaced by
    /// the state of seed 0.
    pub fn from_state(state: [u64; 4]) -> Self {
        if state == [0; 4] {
            Self::from_seed(0)
        } else {
            Self { state }
        }
    }

    /// The generator's state, to save and restore it.
    pub fn state(&self) -> [u64; 4] {
        self.state
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// The next 32 random bits (the high half of the next 64).
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A number in `[0, 1)`, with 24 random bits.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// A number in `[0, 1)`, with 53 random bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A uniformly distributed number in `0..bound`, without bias. Returns 0
    /// when `bound` is 0.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        // Lemire's method: multiply, and reject the few low products that
        // would make some results more likely than others.
        let mut product = u64::from(self.next_u32()) * u64::from(bound);
        let mut low = product as u32;
        if low < bound {
            let threshold = bound.wrapping_neg() % bound;
            while low < threshold {
                product = u64::from(self.next_u32()) * u64::from(bound);
                low = product as u32;
            }
        }
        (product >> 32) as u32
    }

    /// A uniformly distributed integer in `low..high`. Returns `low` when the
    /// range is empty.
    pub fn range_i32(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        let span = high.abs_diff(low);
        low.wrapping_add_unsigned(self.below(span))
    }

    /// A number in `low..high` (`high` excluded). Returns `low` when the
    /// range is empty.
    pub fn range_f32(&mut self, low: f32, high: f32) -> f32 {
        if high > low {
            let value = low + (high - low) * self.next_f32();
            // Rounding can land exactly on `high`; keep the bound exclusive.
            if value < high { value } else { low }
        } else {
            low
        }
    }

    /// `true` with probability `probability` (clamped to `0..=1`).
    pub fn chance(&mut self, probability: f64) -> bool {
        self.next_f64() < probability
    }

    /// A random element of `items`, or `None` if it is empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let len = u32::try_from(items.len()).unwrap_or(u32::MAX);
        items.get(self.below(len) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published first output of SplitMix64 with seed 0.
    #[test]
    fn splitmix64_matches_the_reference() {
        let mut state = 0;
        assert_eq!(splitmix64(&mut state), 0xE220_A839_7B1D_CDAF);
    }

    /// xoshiro256++ from state [1, 2, 3, 4]. The first two values were also
    /// worked out by hand from the algorithm's definition.
    #[test]
    fn xoshiro256plusplus_matches_the_reference() {
        let mut rng = Rng::from_state([1, 2, 3, 4]);
        assert_eq!(rng.next_u64(), 41_943_041);
        assert_eq!(rng.next_u64(), 58_720_359);
    }

    /// Recorded on 2026-11-15; locks the seeding and every derived method.
    /// If this fails, every recorded replay and test seed changes meaning.
    #[test]
    fn recorded_sequence_never_changes() {
        let mut rng = Rng::from_seed(7);
        let values = [
            rng.next_u64(),
            rng.next_u64(),
            u64::from(rng.below(100)),
            u64::from(rng.below(1_000_000)),
        ];
        assert_eq!(
            values,
            [
                1_021_219_803_524_665_661,
                3_174_977_118_032_272_916,
                71,
                427_209
            ]
        );
    }

    #[test]
    fn same_seed_same_sequence_different_seed_different_sequence() {
        let sequence = |seed| {
            let mut rng = Rng::from_seed(seed);
            (0..16).map(|_| rng.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(sequence(42), sequence(42));
        assert_ne!(sequence(42), sequence(43));
    }

    #[test]
    fn state_round_trips() {
        let mut rng = Rng::from_seed(9);
        rng.next_u64();
        let mut copy = Rng::from_state(rng.state());
        assert_eq!(copy.next_u64(), rng.next_u64());
        assert_ne!(Rng::from_state([0; 4]).state(), [0; 4]);
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut rng = Rng::from_seed(1);
        for _ in 0..10_000 {
            let unit = rng.next_f32();
            assert!((0.0..1.0).contains(&unit));
            let wide = rng.next_f64();
            assert!((0.0..1.0).contains(&wide));
            assert!(rng.below(7) < 7);
            let integer = rng.range_i32(-3, 4);
            assert!((-3..4).contains(&integer));
            let float = rng.range_f32(-1.5, 2.5);
            assert!((-1.5..2.5).contains(&float));
        }
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.range_i32(5, 5), 5);
        assert_eq!(rng.range_i32(i32::MIN, i32::MIN + 1), i32::MIN);
        assert_eq!(rng.range_f32(2.0, 1.0), 2.0);
        assert!(rng.pick::<u8>(&[]).is_none());
        assert!(!rng.chance(0.0));
        assert!(rng.chance(1.0));
    }

    #[test]
    fn below_is_roughly_uniform() {
        let mut rng = Rng::from_seed(3);
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[rng.below(6) as usize] += 1;
        }
        for count in counts {
            assert!((9_000..11_000).contains(&count), "{counts:?}");
        }
    }
}
