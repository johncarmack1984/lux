//! The DMX universe buffer and its overlay semantics.
//!
//! One byte per slot, exactly [`UNIVERSE_SIZE`] slots. Writes *overlay*: an
//! incoming frame touches only the slots it carries, leaving higher slots
//! untouched — which is what lets a 6-byte RGBAW write (a color pick, a
//! remote frame) coexist with raw faders on slots 7..=512. These are the same
//! semantics as the desktop's `LuxBuffer`; the node uses this plain form.

/// A full DMX512 universe is 512 one-byte slots.
pub const UNIVERSE_SIZE: usize = 512;

/// A plain universe buffer with overlay semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Universe {
    slots: [u8; UNIVERSE_SIZE],
}

impl Default for Universe {
    fn default() -> Self {
        Self {
            slots: [0; UNIVERSE_SIZE],
        }
    }
}

impl Universe {
    /// Overlay `incoming` onto the leading slots; higher slots keep their
    /// values. Oversized input is truncated at the universe boundary.
    pub fn overlay(&mut self, incoming: &[u8]) {
        let n = incoming.len().min(UNIVERSE_SIZE);
        self.slots[..n].copy_from_slice(&incoming[..n]);
    }

    /// Set one slot; `slot` is the 1-based DMX slot number.
    pub fn set_slot(&mut self, slot: u16, value: u8) -> Result<(), String> {
        let index = usize::from(slot);
        if !(1..=UNIVERSE_SIZE).contains(&index) {
            return Err(format!(
                "slot {slot} out of range (expected 1..={UNIVERSE_SIZE})"
            ));
        }
        self.slots[index - 1] = value;
        Ok(())
    }

    /// The current slot values, slot 1 first.
    pub fn slots(&self) -> &[u8; UNIVERSE_SIZE] {
        &self.slots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_touches_only_the_leading_slots() {
        let mut u = Universe::default();
        u.set_slot(10, 200).unwrap();
        u.overlay(&[1, 2, 3]);
        assert_eq!(&u.slots()[..3], &[1, 2, 3]);
        assert_eq!(u.slots()[9], 200); // untouched by the overlay

        // Oversized input truncates instead of panicking.
        u.overlay(&[7u8; 600]);
        assert!(u.slots().iter().all(|&v| v == 7));
    }

    #[test]
    fn set_slot_is_one_based_and_bounded() {
        let mut u = Universe::default();
        u.set_slot(1, 9).unwrap();
        u.set_slot(512, 9).unwrap();
        assert_eq!(u.slots()[0], 9);
        assert_eq!(u.slots()[511], 9);
        assert!(u.set_slot(0, 9).is_err());
        assert!(u.set_slot(513, 9).is_err());
    }
}
