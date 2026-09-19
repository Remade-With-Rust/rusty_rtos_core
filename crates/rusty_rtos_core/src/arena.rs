//! A fixed-capacity, generational arena: where every kernel object lives.
//!
//! The C kernel allocates a control block per object and hands out its
//! address. Kairos keeps `N` slots of `T` in a static array, hands out a
//! [`Handle`] (slot + generation), and refuses a stale handle. No heap is
//! involved; `N` comes from the configuration (`Config::MAX_TASKS` and
//! friends), and a create beyond it is [`Error::NoMemory`] — the same answer
//! `pvPortMalloc` returning `NULL` gives in C.
//!
//! `Arena` never panics: every access is bounds- and generation-checked and
//! answers with an `Option` or a `Result`.

use core::fmt;

use crate::error::{Error, Result};
use crate::handle::{Handle, Kind};

/// One slot: its generation (odd while occupied, even while free, never
/// zero after the first use so a live handle never has generation 0).
struct Slot<T> {
    generation: u16,
    value: Option<T>,
}

impl<T> Slot<T> {
    const EMPTY: Self = Self {
        generation: 0,
        value: None,
    };
}

/// `N` slots of `T`, addressed by generational [`Handle<K>`].
pub struct Arena<K: Kind, T, const N: usize> {
    slots: [Slot<T>; N],
    len: usize,
    kind: core::marker::PhantomData<K>,
}

impl<K: Kind, T, const N: usize> Default for Arena<K, T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Kind, T, const N: usize> Arena<K, T, N> {
    /// The capacity as a compile-time check: a handle index must fit.
    const CAPACITY_FITS: () = assert!(
        N <= Handle::<K>::MAX_INDEX as usize,
        "arena too large for a u16 handle index"
    );

    /// An empty arena.
    #[must_use]
    pub const fn new() -> Self {
        let () = Self::CAPACITY_FITS;
        Self {
            slots: [const { Slot::EMPTY }; N],
            len: 0,
            kind: core::marker::PhantomData,
        }
    }

    /// How many objects are live.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no object is live.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The capacity, `N`.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Whether `handle` names a live object.
    #[must_use]
    pub fn contains(&self, handle: Handle<K>) -> bool {
        self.get(handle).is_some()
    }

    /// Place `value` in a free slot and return its handle.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when every slot is occupied; the value is
    /// returned to the caller inside the error's companion so nothing is
    /// lost — see [`Arena::try_insert`].
    pub fn insert(&mut self, value: T) -> Result<Handle<K>> {
        self.try_insert(value).map_err(|_| Error::NoMemory)
    }

    /// Like [`Arena::insert`], but hands the value back when there is no room.
    ///
    /// # Errors
    /// The value itself, when every slot is occupied.
    pub fn try_insert(&mut self, value: T) -> core::result::Result<Handle<K>, T> {
        let Some(index) = self.slots.iter().position(|s| s.value.is_none()) else {
            return Err(value);
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(value);
        };
        // A free slot's generation is even (or 0); occupying it makes it odd.
        // Wrapping past u16::MAX skips 0 so a live handle is never null.
        let mut generation = slot.generation.wrapping_add(1);
        if generation == 0 {
            generation = 1;
        }
        slot.generation = generation;
        slot.value = Some(value);
        // Wrapping: a free slot was found, so the arena is not full and
        // the count is below `N`.
        self.len = self.len.wrapping_add(1);
        // `index < N <= MAX_INDEX`, so the conversion cannot truncate.
        Ok(Handle::from_parts(index as u16, generation))
    }

    /// The object `handle` names.
    #[must_use]
    pub fn get(&self, handle: Handle<K>) -> Option<&T> {
        let slot = self.slots.get(usize::from(handle.index()))?;
        if slot.generation != handle.generation() {
            return None;
        }
        slot.value.as_ref()
    }

    /// The object `handle` names, mutably.
    #[must_use]
    pub fn get_mut(&mut self, handle: Handle<K>) -> Option<&mut T> {
        let slot = self.slots.get_mut(usize::from(handle.index()))?;
        if slot.generation != handle.generation() {
            return None;
        }
        slot.value.as_mut()
    }

    /// Why a handle that did not resolve did not resolve.
    ///
    /// Only the two error arms call this, so asking costs nothing on a
    /// lookup that succeeds -- which is all but a vanishing few of them.
    fn why(handle: Handle<K>) -> Error {
        if handle.is_null() {
            Error::InvalidHandle
        } else {
            Error::Gone
        }
    }

    /// Like [`Arena::get`], but says why: [`Error::InvalidHandle`] for the
    /// null handle or an index beyond the arena, [`Error::Gone`] for a
    /// generation that has been reused or freed.
    ///
    /// # Errors
    /// As above.
    pub fn resolve(&self, handle: Handle<K>) -> Result<&T> {
        let slot = self
            .slots
            .get(usize::from(handle.index()))
            .ok_or(Error::InvalidHandle)?;
        if slot.generation != handle.generation() {
            return Err(Self::why(handle));
        }
        slot.value.as_ref().ok_or_else(|| Self::why(handle))
    }

    /// Like [`Arena::resolve`], mutably.
    ///
    /// # Errors
    /// As [`Arena::resolve`].
    pub fn resolve_mut(&mut self, handle: Handle<K>) -> Result<&mut T> {
        let slot = self
            .slots
            .get_mut(usize::from(handle.index()))
            .ok_or(Error::InvalidHandle)?;
        if slot.generation != handle.generation() {
            return Err(Self::why(handle));
        }
        slot.value.as_mut().ok_or_else(|| Self::why(handle))
    }

    /// Remove the object `handle` names, invalidating the handle and every
    /// copy of it.
    #[must_use]
    pub fn remove(&mut self, handle: Handle<K>) -> Option<T> {
        let slot = self.slots.get_mut(usize::from(handle.index()))?;
        if slot.generation != handle.generation() {
            return None;
        }
        let value = slot.value.take()?;
        // Freeing bumps the generation again (odd -> even), so the old
        // handle can never match a future occupant.
        slot.generation = slot.generation.wrapping_add(1);
        // Wrapping: `take` above answered `Some`, so a live value was
        // here and the count is at least one.
        self.len = self.len.wrapping_sub(1);
        Some(value)
    }

    /// Every live `(handle, object)` in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<K>, &T)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.value
                .as_ref()
                .map(|v| (Handle::from_parts(i as u16, s.generation), v))
        })
    }

    /// Every live `(handle, object)` in slot order, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<K>, &mut T)> + '_ {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            let generation = s.generation;
            s.value
                .as_mut()
                .map(|v| (Handle::from_parts(i as u16, generation), v))
        })
    }

    /// The handle of the object at slot `index`, if live.
    #[must_use]
    pub fn handle_at(&self, index: u16) -> Option<Handle<K>> {
        let slot = self.slots.get(usize::from(index))?;
        slot.value.as_ref()?;
        Some(Handle::from_parts(index, slot.generation))
    }
}

impl<K: Kind, T: fmt::Debug, const N: usize> fmt::Debug for Arena<K, T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::handle::Task;

    type Tasks = Arena<Task, u32, 3>;

    #[test]
    fn insert_get_remove_and_the_stale_handle() {
        let mut a = Tasks::new();
        assert!(a.is_empty());
        let h = a.insert(10).unwrap();
        assert!(!h.is_null());
        assert_eq!(a.get(h), Some(&10));
        assert_eq!(a.len(), 1);
        *a.get_mut(h).unwrap() = 11;
        assert_eq!(a.resolve(h), Ok(&11));
        assert_eq!(a.remove(h), Some(11));
        assert_eq!(a.get(h), None);
        assert_eq!(a.resolve(h), Err(Error::Gone));
        assert_eq!(a.remove(h), None);
        // The slot is reused with a new generation; the old handle stays dead.
        let h2 = a.insert(12).unwrap();
        assert_eq!(h2.index(), h.index());
        assert_ne!(h2.generation(), h.generation());
        assert_eq!(a.get(h), None);
        assert_eq!(a.get(h2), Some(&12));
    }

    #[test]
    fn full_is_no_memory_and_nothing_is_lost() {
        let mut a = Tasks::new();
        for i in 0..3 {
            a.insert(i).unwrap();
        }
        assert_eq!(a.insert(99), Err(Error::NoMemory));
        assert_eq!(a.try_insert(99), Err(99));
        assert_eq!(a.len(), 3);
        assert_eq!(a.capacity(), 3);
    }

    #[test]
    fn null_and_out_of_range_are_invalid_not_gone() {
        let a = Tasks::new();
        assert_eq!(a.resolve(Handle::NULL), Err(Error::InvalidHandle));
        assert_eq!(
            a.resolve(Handle::from_parts(7, 1)),
            Err(Error::InvalidHandle)
        );
        assert_eq!(a.resolve(Handle::from_parts(1, 1)), Err(Error::Gone));
        assert!(!a.contains(Handle::from_parts(0, 1)));
    }

    #[test]
    fn generations_never_mint_a_null_handle() {
        let mut a = Arena::<Task, u8, 1>::new();
        let mut last = 0u16;
        for _ in 0..70_000u32 {
            let h = a.insert(0).unwrap();
            assert!(!h.is_null());
            assert_ne!(h.generation(), last);
            last = h.generation();
            a.remove(h).unwrap();
        }
    }

    #[test]
    fn iteration_is_in_slot_order_with_live_handles() {
        let mut a = Tasks::new();
        let h0 = a.insert(0).unwrap();
        let h1 = a.insert(1).unwrap();
        let h2 = a.insert(2).unwrap();
        a.remove(h1).unwrap();
        let seen: alloc_free::V = a.iter().map(|(h, v)| (h, *v)).collect();
        assert_eq!(seen.as_slice(), &[(h0, 0), (h2, 2)]);
        assert_eq!(a.handle_at(1), None);
        assert_eq!(a.handle_at(2), Some(h2));
    }

    mod alloc_free {
        extern crate std;
        pub type V = std::vec::Vec<(super::Handle<super::Task>, u32)>;
    }
}
