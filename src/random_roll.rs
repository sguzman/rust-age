//! Fallout 1 CE's shuffled Park-Miller generator and d% roll rules.
//!
//! Source: pinned src/game/roll.cc. The initial state is the result of CE's
//! seed_generator, not roll_init (which additionally samples 100,000 rolls
//! for its chi-squared diagnostic). All time inputs are explicit; no OS RNG,
//! gameplay objects, scripts or proprietary assets are involved.
use std::io;

const SHUFFLE_SIZE: usize = 32;
const MODULUS: i32 = 0x7FFF_FFFF;
pub const GAME_TIME_TICKS_PER_DAY: i32 = 24 * 60 * 60 * 10;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RollOutcome {
    CriticalFailure = 0,
    Failure = 1,
    Success = 2,
    CriticalSuccess = 3,
}

/// Owned equivalent of CE roll.cc idum, iy and iv[32]. The first iv[0]
/// starts at zero, because CE's 40-to-1 seed loop never writes index zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FalloutRng {
    idum: i32,
    iy: i32,
    iv: [i32; SHUFFLE_SIZE],
}

impl FalloutRng {
    /// Deterministic CE seed_generator, without roll_set_seed(-1)'s external
    /// platform-random seed or roll_init's subsequent diagnostic draws.
    pub fn from_seed(seed: i32) -> Self {
        let mut rng = Self {
            idum: 0,
            iy: 0,
            iv: [0; SHUFFLE_SIZE],
        };
        rng.reseed_exact(seed);
        rng
    }

    /// CE leaves iv[0] unchanged across reseeds. Preserve that subtle state.
    pub fn reseed_exact(&mut self, seed: i32) {
        let mut num = seed.max(1);
        for index in (1..=40).rev() {
            num = 16807 * (num % 127773) - 2836 * (num / 127773);
            if num < 0 {
                num &= MODULUS;
            }
            if index < SHUFFLE_SIZE {
                self.iv[index] = num;
            }
        }
        self.iy = self.iv[0];
        self.idum = num;
    }

    /// CE ran1(max): original Bays-Durham shuffled generator.
    fn ran1(&mut self, max: i32) -> io::Result<i32> {
        if max <= 0 {
            return Err(invalid("CE RNG modulus must be positive"));
        }
        let mut next = 16807 * (self.idum % 127773) - 2836 * (self.idum / 127773);
        if next < 0 {
            next += MODULUS;
        }
        if next < 0 {
            next += MODULUS;
        }
        let index = (self.iy & 0x1F) as usize;
        let value = self.iv[index];
        self.iv[index] = next;
        self.iy = value;
        self.idum = next;
        Ok(value % max)
    }

    /// CE roll_random(min,max). Reversed bounds intentionally consume one
    /// RNG draw then return min, due to CE's final bounds check.
    pub fn between(&mut self, min: i32, max: i32) -> io::Result<i32> {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        let span = high
            .checked_sub(low)
            .and_then(|distance| distance.checked_add(1))
            .ok_or_else(|| invalid("CE RNG interval overflows signed i32"))?;
        let value = low
            .checked_add(self.ran1(span)?)
            .ok_or_else(|| invalid("CE RNG output overflows signed i32"))?;
        if value < min || value > max {
            Ok(min)
        } else {
            Ok(value)
        }
    }

    /// CE roll_check_critical with explicit game ticks instead of global time.
    /// The extra critical roll begins only after one in-game day.
    pub fn check_critical(
        &mut self,
        delta: i32,
        critical_success_modifier: i32,
        game_ticks: i32,
    ) -> io::Result<RollOutcome> {
        let past_first_day = game_ticks / GAME_TIME_TICKS_PER_DAY >= 1;
        if delta < 0 {
            if past_first_day {
                let failure_margin = delta
                    .checked_neg()
                    .ok_or_else(|| invalid("CE critical failure margin overflows"))?;
                if self.between(1, 100)? <= failure_margin / 10 {
                    return Ok(RollOutcome::CriticalFailure);
                }
            }
            Ok(RollOutcome::Failure)
        } else {
            if past_first_day {
                let chance = (delta / 10)
                    .checked_add(critical_success_modifier)
                    .ok_or_else(|| invalid("CE critical chance overflows"))?;
                if self.between(1, 100)? <= chance {
                    return Ok(RollOutcome::CriticalSuccess);
                }
            }
            Ok(RollOutcome::Success)
        }
    }

    /// CE roll_check: one original d% roll followed by conditional critical.
    /// Returns both the resulting outcome and CE's signed success margin.
    pub fn check(
        &mut self,
        difficulty: i32,
        critical_success_modifier: i32,
        game_ticks: i32,
    ) -> io::Result<(RollOutcome, i32)> {
        let delta = difficulty
            .checked_sub(self.between(1, 100)?)
            .ok_or_else(|| invalid("CE roll margin overflows"))?;
        Ok((
            self.check_critical(delta, critical_success_modifier, game_ticks)?,
            delta,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{FalloutRng, RollOutcome, GAME_TIME_TICKS_PER_DAY};

    #[test]
    fn initial_shuffle_slot_and_seed_clamp_are_deterministic() {
        let mut first = FalloutRng::from_seed(1);
        let mut second = FalloutRng::from_seed(1);
        let mut zero = FalloutRng::from_seed(0);
        assert_eq!(first.between(1, 100).unwrap(), 1);
        assert_eq!(second.between(1, 100).unwrap(), 1);
        assert_eq!(zero.between(1, 100).unwrap(), 1);
        for _ in 0..128 {
            let a = first.between(1, 100).unwrap();
            assert_eq!(a, second.between(1, 100).unwrap());
            assert_eq!(a, zero.between(1, 100).unwrap());
        }
    }

    #[test]
    fn reversed_bounds_preserve_original_ce_fallback() {
        let mut reversed = FalloutRng::from_seed(42);
        let mut normal = FalloutRng::from_seed(42);
        assert_eq!(reversed.between(10, 1).unwrap(), 10);
        let _ = normal.between(1, 10).unwrap();
        assert_eq!(
            reversed.between(-10, 10).unwrap(),
            normal.between(-10, 10).unwrap()
        );
        assert_eq!(reversed.between(5, 5).unwrap(), 5);
        assert!(reversed.between(i32::MIN, i32::MAX).is_err());
    }

    #[test]
    fn day_threshold_changes_critical_sampling_not_prior_rolls() {
        let mut before_day = FalloutRng::from_seed(19);
        let mut after_day = FalloutRng::from_seed(19);
        assert_eq!(
            before_day
                .check_critical(-15, 0, GAME_TIME_TICKS_PER_DAY - 1)
                .unwrap(),
            RollOutcome::Failure
        );
        assert_eq!(before_day.between(1, 100).unwrap(), 1);
        assert!(
            after_day
                .check_critical(-15, 0, GAME_TIME_TICKS_PER_DAY)
                .is_ok()
        );
        // After a day has passed the critical check consumes exactly the
        // draw made explicitly in the pre-day case, leaving equal RNG state.
        assert_eq!(before_day, after_day);
    }

    #[test]
    fn roll_reports_delta_and_rejects_nonrepresentable_ranges() {
        let mut rng = FalloutRng::from_seed(1);
        let (outcome, delta) = rng.check(50, 0, 0).unwrap();
        assert_eq!(outcome, RollOutcome::Success);
        assert_eq!(delta, 49);
        assert!(rng.between(i32::MIN, i32::MAX).is_err());
    }
}
