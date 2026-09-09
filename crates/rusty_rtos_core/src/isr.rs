//! The interrupt side as a type.
//!
//! FreeRTOS has a `*FromISR` twin of most API calls, and calling the wrong
//! one is a classic silent bug. In Kairos the ISR variants take an [`Isr`]
//! token, which only an interrupt entry point constructs, and they hand back
//! a [`Woken`] — the `pxHigherPriorityTaskWoken` flag — that the port's
//! `yield_from_isr` consumes. The wrong variant does not compile.

/// Proof that the caller runs in interrupt context.
///
/// A port's interrupt entry constructs it (once per interrupt) and passes it
/// down; task code never holds one. It is deliberately not `Copy`-free —
/// an ISR may call several kernel APIs — but it is `!Send` so it cannot be
/// stashed for a task to misuse.
#[derive(Debug, Clone, Copy)]
pub struct Isr {
    _not_send: core::marker::PhantomData<*const ()>,
}

impl Isr {
    /// The token. Only an interrupt entry point should call this; the port
    /// crates are the intended callers and document their entry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _not_send: core::marker::PhantomData,
        }
    }
}

impl Default for Isr {
    fn default() -> Self {
        Self::new()
    }
}

/// `pxHigherPriorityTaskWoken`: whether an ISR-side call unblocked a task
/// that should run before the interrupted one. Accumulates across calls in
/// one interrupt and is consumed by the port's `yield_from_isr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[must_use = "a woken flag that is dropped never yields; pass it to the port"]
pub struct Woken(bool);

impl Woken {
    /// Nothing woken yet.
    pub const NO: Self = Self(false);
    /// A higher-priority task was woken.
    pub const YES: Self = Self(true);

    /// Whether a yield is needed.
    #[must_use]
    pub const fn needed(self) -> bool {
        self.0
    }

    /// Fold another call's result in (`||` on the C flag).
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 || other.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn woken_accumulates() {
        assert!(!Woken::NO.needed());
        assert!(Woken::NO.or(Woken::YES).needed());
        assert!(Woken::YES.or(Woken::NO).needed());
        assert!(!Woken::NO.or(Woken::NO).needed());
        assert_eq!(Woken::default(), Woken::NO);
        let _token = Isr::new();
    }
}
