//! Mutex access policy: recover from poisoning instead of panicking.
//!
//! A poisoned lock means some other thread panicked while holding the guard.
//! The default `.lock().unwrap()` response turns that one dead thread into a
//! cascade — every later touch of the same state panics too — which in a
//! lighting app means the output dies mid-show over an already-handled fault.
//! Lux's shared state (the render buffer, setup store, session tokens, channel
//! metadata) is plain data that is never left half-valid across an unwind
//! boundary in a way worse than "slightly stale", so recovering the inner
//! value and logging is strictly better than crashing.
//!
//! This trait is the only sanctioned way to take a `std::sync::Mutex` in this
//! crate — `clippy::unwrap_used` is denied workspace-wide, so a bare
//! `.lock().unwrap()` fails CI.

use std::sync::{Mutex, MutexGuard};

pub trait LockPolicy<T> {
    /// Lock, recovering the data if a panicked thread poisoned it.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockPolicy<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            log::warn!("a lock was poisoned by a panicked thread; recovering its state");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_a_poisoned_lock() {
        let lock = std::sync::Arc::new(Mutex::new(7u8));
        let poisoner = lock.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock_or_recover();
            panic!("poison the lock");
        })
        .join();
        assert!(lock.is_poisoned());
        assert_eq!(*lock.lock_or_recover(), 7);
    }
}
