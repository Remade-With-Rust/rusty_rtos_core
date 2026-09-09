//! One `Copy` error for the whole family.
//!
//! FreeRTOS answers with `pdPASS` / `pdFAIL` and a handful of `err*` codes;
//! Kairos answers with a `Result` whose error is this enum. It carries no
//! `String`, so it crosses `no_std` boundaries and package boundaries without
//! conversion chains; packages wrap it in their own enums for detail and the
//! C ABI maps it back with [`Error::pd_code`].

use core::fmt;

/// `Result` with the family's error.
pub type Result<T> = core::result::Result<T, Error>;

/// Why a kernel operation did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The wait ran out before the condition held (`pdFALSE` from a blocking
    /// call, `errQUEUE_EMPTY` / `errQUEUE_FULL` in the queue API).
    Timeout,
    /// A queue, buffer or set has no room for the item.
    Full,
    /// A queue or buffer has nothing to take.
    Empty,
    /// No slot, heap block or stack could be found
    /// (`errCOULD_NOT_ALLOCATE_REQUIRED_MEMORY`).
    NoMemory,
    /// The handle names an object that no longer exists (a generation
    /// mismatch). In C this is a use-after-free; here it is a value.
    Gone,
    /// The handle never named an object (out of range or the null handle).
    InvalidHandle,
    /// A priority at or above `Config::MAX_PRIORITIES`.
    InvalidPriority,
    /// A size, count or index outside what the configuration allows.
    InvalidArgument,
    /// The call is not allowed from an interrupt (use the `*_from_isr`
    /// variant with an [`crate::isr::Isr`] token).
    InIsr,
    /// The call is only allowed from an interrupt.
    NotInIsr,
    /// The scheduler is suspended and the call would block
    /// (`errQUEUE_BLOCKED`).
    SchedulerSuspended,
    /// The object is in use in a way that refuses the operation (a mutex held
    /// by another task, an item already in a list).
    Busy,
    /// The object is not in the state the operation needs (a timer that is
    /// not active, an item in no list).
    NotActive,
    /// The kernel was built without the subsystem (a cargo feature is off).
    Unsupported,
}

impl Error {
    /// The C kernel's return code for this error, for the C ABI.
    ///
    /// `pdPASS` is 1 and `pdFAIL` is 0; the `err*` codes are negative where
    /// upstream defines them so. Errors upstream reports only through
    /// `configASSERT` (a stale handle, a bad priority) map to `pdFAIL`.
    #[must_use]
    pub const fn pd_code(self) -> i32 {
        match self {
            Self::NoMemory => -1,
            Self::SchedulerSuspended => -4,
            _ => 0,
        }
    }

    /// A short, stable name for logs and the trace sink.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Full => "full",
            Self::Empty => "empty",
            Self::NoMemory => "no-memory",
            Self::Gone => "gone",
            Self::InvalidHandle => "invalid-handle",
            Self::InvalidPriority => "invalid-priority",
            Self::InvalidArgument => "invalid-argument",
            Self::InIsr => "in-isr",
            Self::NotInIsr => "not-in-isr",
            Self::SchedulerSuspended => "scheduler-suspended",
            Self::Busy => "busy",
            Self::NotActive => "not-active",
            Self::Unsupported => "unsupported",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl core::error::Error for Error {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_c_kernel() {
        assert_eq!(Error::NoMemory.pd_code(), -1);
        assert_eq!(Error::SchedulerSuspended.pd_code(), -4);
        assert_eq!(Error::Timeout.pd_code(), 0);
        assert_eq!(Error::Gone.pd_code(), 0);
    }

    #[test]
    fn names_are_unique() {
        let all = [
            Error::Timeout,
            Error::Full,
            Error::Empty,
            Error::NoMemory,
            Error::Gone,
            Error::InvalidHandle,
            Error::InvalidPriority,
            Error::InvalidArgument,
            Error::InIsr,
            Error::NotInIsr,
            Error::SchedulerSuspended,
            Error::Busy,
            Error::NotActive,
            Error::Unsupported,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i.wrapping_add(1)) {
                assert_ne!(a.name(), b.name());
            }
        }
        assert_eq!(core::mem::size_of::<Error>(), 1);
    }
}
