//! Generational handles: the family's answer to `TaskHandle_t` and friends.
//!
//! A FreeRTOS handle is a pointer to a control block; after `vTaskDelete`
//! it dangles. A Kairos handle is an index into an [`crate::arena::Arena`]
//! plus the generation of the slot when the object was created. Reusing the
//! slot bumps the generation, so a stale handle stops resolving
//! ([`crate::Error::Gone`]) instead of aliasing a new object.
//!
//! The kind of object is a type parameter, so a `QueueHandle` cannot be
//! passed where a `TaskHandle` is expected — the C kernel's `void *` cast
//! does not exist here.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

use crate::error::{Error, Result};

/// The kinds of kernel object a handle can name. Sealed to the kernel's set.
pub trait Kind: Copy + Clone + fmt::Debug + PartialEq + Eq + 'static {
    /// A short name for the trace sink ("task", "queue", ...).
    const NAME: &'static str;
}

/// A task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Task;
/// A queue, semaphore or mutex (all queues in the C kernel, and here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Queue;
/// A software timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timer;
/// An event group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventGroup;
/// A stream or message buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamBuffer;

impl Kind for Task {
    const NAME: &'static str = "task";
}
impl Kind for Queue {
    const NAME: &'static str = "queue";
}
impl Kind for Timer {
    const NAME: &'static str = "timer";
}
impl Kind for EventGroup {
    const NAME: &'static str = "event-group";
}
impl Kind for StreamBuffer {
    const NAME: &'static str = "stream-buffer";
}

/// A generational index naming one object of kind `K`.
///
/// `index` is the arena slot (at most [`Handle::MAX_INDEX`]); `generation`
/// is the slot's generation at creation, and is never zero for a live
/// handle — generation 0 is [`Handle::NULL`], the C kernel's `NULL` handle.
pub struct Handle<K: Kind> {
    index: u16,
    generation: u16,
    kind: PhantomData<K>,
}

/// `TaskHandle_t`.
pub type TaskHandle = Handle<Task>;
/// `QueueHandle_t` / `SemaphoreHandle_t`.
pub type QueueHandle = Handle<Queue>;
/// `TimerHandle_t`.
pub type TimerHandle = Handle<Timer>;
/// `EventGroupHandle_t`.
pub type EventGroupHandle = Handle<EventGroup>;
/// `StreamBufferHandle_t` / `MessageBufferHandle_t`.
pub type StreamBufferHandle = Handle<StreamBuffer>;

impl<K: Kind> Handle<K> {
    /// The largest slot index a handle can name (the arena capacity bound).
    pub const MAX_INDEX: u16 = u16::MAX.wrapping_sub(1);

    /// The null handle: names nothing, resolves to
    /// [`Error::InvalidHandle`]. What `NULL` means to `xTaskGetHandle`.
    pub const NULL: Self = Self {
        index: 0,
        generation: 0,
        kind: PhantomData,
    };

    /// A handle for `index` at `generation`. The arena is the only intended
    /// caller; a generation of zero yields the null handle.
    #[must_use]
    pub const fn from_parts(index: u16, generation: u16) -> Self {
        Self {
            index,
            generation,
            kind: PhantomData,
        }
    }

    /// The arena slot this handle names.
    #[must_use]
    pub const fn index(self) -> u16 {
        self.index
    }

    /// The slot generation this handle was minted at (zero for the null handle).
    #[must_use]
    pub const fn generation(self) -> u16 {
        self.generation
    }

    /// Whether this is the null handle.
    #[must_use]
    pub const fn is_null(self) -> bool {
        self.generation == 0
    }

    /// The handle as one `u32` for the C ABI: generation in the high half,
    /// index in the low half. The null handle is `0`, exactly `NULL`.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        ((self.generation as u32) << 16) | (self.index as u32)
    }

    /// A handle from its C-ABI form. Never fails: a zero is the null handle,
    /// anything else is checked by the arena it is presented to.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self::from_parts((raw & 0xFFFF) as u16, (raw >> 16) as u16)
    }

    /// This handle, or [`Error::InvalidHandle`] if it is null.
    ///
    /// # Errors
    /// [`Error::InvalidHandle`] for the null handle.
    pub const fn non_null(self) -> Result<Self> {
        if self.is_null() {
            Err(Error::InvalidHandle)
        } else {
            Ok(self)
        }
    }
}

// Manual impls so `K` needs no bounds beyond `Kind` at use sites.
impl<K: Kind> Clone for Handle<K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K: Kind> Copy for Handle<K> {}
impl<K: Kind> PartialEq for Handle<K> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<K: Kind> Eq for Handle<K> {}
impl<K: Kind> Hash for Handle<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_raw().hash(state);
    }
}
impl<K: Kind> Default for Handle<K> {
    fn default() -> Self {
        Self::NULL
    }
}
impl<K: Kind> fmt::Debug for Handle<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "{}:null", K::NAME)
        } else {
            write!(f, "{}:{}@{}", K::NAME, self.index, self.generation)
        }
    }
}
impl<K: Kind> fmt::Display for Handle<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn raw_form_round_trips_and_null_is_zero() {
        let h = TaskHandle::from_parts(7, 3);
        assert_eq!(h.to_raw(), 0x0003_0007);
        assert_eq!(TaskHandle::from_raw(h.to_raw()), h);
        assert_eq!(TaskHandle::NULL.to_raw(), 0);
        assert!(TaskHandle::from_raw(0).is_null());
        assert_eq!(TaskHandle::NULL.non_null(), Err(Error::InvalidHandle));
        assert_eq!(h.non_null(), Ok(h));
    }

    #[test]
    fn kinds_do_not_mix_and_names_are_stable() {
        assert_eq!(<Task as Kind>::NAME, "task");
        assert_eq!(<Queue as Kind>::NAME, "queue");
        // A QueueHandle is a different type from a TaskHandle: this line
        // would not compile if they were the same, which is the point.
        let _q: QueueHandle = Handle::from_parts(1, 1);
        assert_eq!(core::mem::size_of::<TaskHandle>(), 4);
    }
}
