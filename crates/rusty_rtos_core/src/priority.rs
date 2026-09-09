//! Task priorities, bounded at construction.
//!
//! In C a priority is an `UBaseType_t` the caller promises is below
//! `configMAX_PRIORITIES`, and the kernel clamps or asserts. Here it is a
//! parse-constructor: [`Priority::new`] refuses anything at or above the
//! configured maximum, so every `Priority` a kernel holds is valid and no
//! range check is repeated downstream.

use core::fmt;

use crate::error::{Error, Result};

/// A task priority: `0` is the idle priority (`tskIDLE_PRIORITY`), higher
/// numbers run first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Priority(u8);

impl Priority {
    /// The idle task's priority, `tskIDLE_PRIORITY`.
    pub const IDLE: Self = Self(0);

    /// A priority `raw`, valid iff `raw < max_priorities`
    /// (`Config::MAX_PRIORITIES`).
    ///
    /// # Errors
    /// [`Error::InvalidPriority`] at or above the maximum.
    pub const fn new(raw: u8, max_priorities: u8) -> Result<Self> {
        if raw < max_priorities {
            Ok(Self(raw))
        } else {
            Err(Error::InvalidPriority)
        }
    }

    /// The highest priority a configuration allows, `max_priorities - 1`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when `max_priorities` is zero.
    pub const fn highest(max_priorities: u8) -> Result<Self> {
        match max_priorities.checked_sub(1) {
            Some(p) => Ok(Self(p)),
            None => Err(Error::InvalidArgument),
        }
    }

    /// The priority as a number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Whether this is the idle priority.
    #[must_use]
    pub const fn is_idle(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn bounded_by_the_configuration() {
        assert_eq!(Priority::new(0, 5).unwrap(), Priority::IDLE);
        assert_eq!(Priority::new(4, 5).unwrap().get(), 4);
        assert_eq!(Priority::new(5, 5), Err(Error::InvalidPriority));
        assert_eq!(Priority::new(0, 0), Err(Error::InvalidPriority));
        assert_eq!(Priority::highest(5).unwrap().get(), 4);
        assert_eq!(Priority::highest(0), Err(Error::InvalidArgument));
    }

    #[test]
    fn orders_the_way_the_scheduler_needs() {
        let low = Priority::new(1, 5).unwrap();
        let high = Priority::new(3, 5).unwrap();
        assert!(high > low);
        assert!(Priority::IDLE.is_idle());
        assert!(!low.is_idle());
    }
}
