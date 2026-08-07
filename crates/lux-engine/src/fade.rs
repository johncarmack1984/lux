//! Crossfades: the "recall a look over N seconds" primitive.
//!
//! A [`Crossfade`] is the pure half of a scene recall — it holds where every
//! slot started, where it is going, and how long it has to get there, and
//! answers one question: *what should these slots read at `elapsed_ms`?* It
//! neither renders nor sleeps, so the same math drives the desktop's ticker
//! today and a headless `lux-node` recall later.
//!
//! Two properties the callers rely on:
//!
//! - **Sparse.** A fade owns only the slots it was given, and `at` returns only
//!   those. Every other slot in the universe is untouched, which is what lets a
//!   scene captured over a six-fixture patch coexist with raw faders — the same
//!   overlay discipline as [`crate::universe`].
//! - **It lands exactly.** The last tick returns the target values verbatim, so
//!   a fade can never leave a channel one bit short of the look that was saved.
//!
//! The arithmetic is integer throughout (rounding half away from zero). That is
//! not incidental: the workspace lints forbid float comparison and the sloppy
//! numeric casts a float path invites, and exact integers make the tests below
//! assertions rather than approximations.

/// A linear crossfade from a captured universe snapshot to a sparse target.
#[derive(Debug, Clone)]
pub struct Crossfade {
    steps: Vec<Step>,
    duration_ms: u32,
}

/// One slot's journey: 1-based DMX slot, where it started, where it ends.
#[derive(Debug, Clone, Copy)]
struct Step {
    ch: u16,
    from: u8,
    to: u8,
}

impl Crossfade {
    /// Build a fade from the live universe `from` (slot 1 first) to the sparse
    /// `to` levels over `duration_ms`.
    ///
    /// A target slot outside the snapshot is dropped rather than rejected: a
    /// scene saved against an older, wider patch must degrade to "fades what it
    /// still can", never to a failed recall.
    pub fn new(from: &[u8], to: &[(u16, u8)], duration_ms: u32) -> Self {
        let steps = to
            .iter()
            .filter_map(|&(ch, target)| {
                let index = usize::from(ch).checked_sub(1)?;
                Some(Step {
                    ch,
                    from: *from.get(index)?,
                    to: target,
                })
            })
            .collect();
        Crossfade { steps, duration_ms }
    }

    /// How long this fade runs, in milliseconds.
    pub fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    /// Whether the fade has reached its target by `elapsed_ms`.
    pub fn is_done(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= u64::from(self.duration_ms)
    }

    /// The sparse overlay to apply at `elapsed_ms` — one entry per slot the
    /// fade owns, in the order it was given them. At or past the duration this
    /// is exactly the target.
    pub fn at(&self, elapsed_ms: u64) -> Vec<(u16, u8)> {
        let done = self.is_done(elapsed_ms);
        self.steps
            .iter()
            .map(|step| {
                let value = if done {
                    step.to
                } else {
                    interpolate(step.from, step.to, elapsed_ms, self.duration_ms)
                };
                (step.ch, value)
            })
            .collect()
    }
}

/// `from + (to - from) * elapsed / duration`, rounded half away from zero and
/// clamped to a byte. Only called with `elapsed < duration` (so `duration > 0`).
fn interpolate(from: u8, to: u8, elapsed: u64, duration: u32) -> u8 {
    let delta = i64::from(to) - i64::from(from);
    // `elapsed < duration <= u32::MAX`, so both conversions are lossless; the
    // fallbacks only exist because the lint floor forbids `unwrap`.
    let elapsed = i64::try_from(elapsed).unwrap_or(i64::from(duration));
    let duration = i64::from(duration);
    let travelled = div_round(delta.saturating_mul(elapsed), duration);
    let value = (i64::from(from) + travelled).clamp(0, 255);
    u8::try_from(value).unwrap_or(to)
}

/// Integer division rounding half away from zero, so a fade's midpoint is the
/// midpoint in both directions rather than biased toward the floor.
fn div_round(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    let half = denominator / 2;
    if numerator >= 0 {
        (numerator + half) / denominator
    } else {
        (numerator - half) / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A universe snapshot with the given leading slots.
    fn universe(leading: &[u8]) -> Vec<u8> {
        let mut slots = vec![0u8; crate::universe::UNIVERSE_SIZE];
        slots[..leading.len()].copy_from_slice(leading);
        slots
    }

    #[test]
    fn fades_linearly_between_the_endpoints() {
        let fade = Crossfade::new(&universe(&[0, 255]), &[(1, 200), (2, 0)], 1000);

        assert_eq!(fade.at(0), vec![(1, 0), (2, 255)]);
        // Halfway: slot 1 has travelled 100 of 200, slot 2 128 of 255 (its
        // half-unit rounds away from where it started).
        assert_eq!(fade.at(500), vec![(1, 100), (2, 127)]);
        assert_eq!(fade.at(1000), vec![(1, 200), (2, 0)]);
        // Past the end it holds the target rather than overshooting.
        assert_eq!(fade.at(99_999), vec![(1, 200), (2, 0)]);
    }

    #[test]
    fn lands_exactly_on_the_target() {
        // A duration that doesn't divide the delta evenly is where a naive
        // implementation leaves a channel one bit short forever.
        let fade = Crossfade::new(&universe(&[0]), &[(1, 255)], 700);
        assert!(!fade.is_done(699));
        assert!(fade.is_done(700));
        assert_eq!(fade.at(700), vec![(1, 255)]);
    }

    #[test]
    fn a_zero_length_fade_snaps() {
        let fade = Crossfade::new(&universe(&[10]), &[(1, 200)], 0);
        assert!(fade.is_done(0));
        assert_eq!(fade.at(0), vec![(1, 200)]);
    }

    #[test]
    fn owns_only_its_own_slots() {
        // Slot 5 is not in the target, so nothing in the overlay mentions it —
        // the caller's buffer keeps whatever a fader left there.
        let fade = Crossfade::new(&universe(&[1, 2, 3, 4, 5]), &[(2, 100)], 1000);
        assert_eq!(fade.at(1000), vec![(2, 100)]);
    }

    #[test]
    fn drops_slots_the_snapshot_does_not_cover() {
        // Slot 0 doesn't exist (DMX is 1-based) and 513 is past the universe: a
        // scene carrying either still recalls the slots that are real.
        let fade = Crossfade::new(&universe(&[7]), &[(0, 9), (1, 9), (513, 9)], 0);
        assert_eq!(fade.at(0), vec![(1, 9)]);
    }

    #[test]
    fn rounding_is_symmetric_up_and_down() {
        // A half-unit of travel rounds away from where the slot started, so a
        // rise and the matching fall cover the same distance at the same tick
        // (2 of 3, in opposite directions) instead of one of them stalling.
        let up = Crossfade::new(&universe(&[0]), &[(1, 3)], 2);
        let down = Crossfade::new(&universe(&[3]), &[(1, 0)], 2);
        assert_eq!(up.at(1), vec![(1, 2)]); // 0 + 1.5 → 2
        assert_eq!(down.at(1), vec![(1, 1)]); // 3 - 1.5 → 1
    }
}
