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
/// One slot. Liveness is the GENERATION'S PARITY, not a discriminant.
///
/// `insert` makes an even generation odd and `remove` makes it even again —
/// the arena has always maintained that, and said so — so the `Option` that
/// used to wrap the value was carrying a fact the generation already had. It
/// cost a tag plus its alignment padding on every slot of every arena: a
/// 40-byte `Timer` sat in a 56-byte slot, and now sits in 48.
///
/// `forbid(unsafe_code)` means a free slot still has to hold a real `T`, so
/// it holds `T::default()`. Nothing ever reads it: every path to a value goes
/// through a generation test first, and a free slot's generation is even
/// while every handle ever issued carries an odd one.
struct Slot<T> {
    generation: u32,
    value: T,
}

/// The bit that marks a slot FREE. It is bit 16, which is ABOVE the sixteen a
/// generation arriving from the C ABI can occupy — `Handle::from_raw` takes its
/// generation from `raw >> 16` of a `u32`, so no forged handle can ever carry it.
///
/// That is the whole trick. The old encoding put liveness in the generation's
/// PARITY: free slots even, live slots odd, every issued handle odd. It worked,
/// but it cost `Handle::from_raw` a normalisation at every C entry point — an
/// even generation had to be folded to NULL, because a forged even generation
/// could otherwise match a free slot's own even generation and resolve to the
/// `T::default()` sitting in it. That was 26 `andi` inside the FFI wrappers.
///
/// With the marker out of reach of the ABI, the comparison in `resolve` does the
/// work by itself: a free slot's generation is at least `FREE`, and a handle's is
/// at most `0xFFFF`, so they can never be equal. `from_raw` normalises nothing.
///
/// It also doubles the generation space. Parity spent half of it: 32,767 odd
/// values before a slot's generation repeated. A plain counter gives 65,535.
///
/// REFUTED 2026-09-24: storing the whole ABI word in the slot instead, so
/// `from_raw` keeps what it was handed, takes `srli` -26 and `andi` -6 but
/// costs flash **+108 B** and `mv` **+10** — minting a handle then needs a
/// shift and an `or` at every `from_parts`, which outweighs what the
/// comparison saves. This encoding stays.
const FREE: u32 = 1 << 16;

/// Whether a generation says its slot is live.
const fn live(generation: u32) -> bool {
    generation & FREE == 0
}

/// `N` slots of `T`, addressed by generational [`Handle<K>`].
pub struct Arena<K: Kind, T, const N: usize> {
    slots: [Slot<T>; N],
    len: usize,
    kind: core::marker::PhantomData<K>,
}

impl<K: Kind, T: Default, const N: usize> Default for Arena<K, T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Kind, T: Default, const N: usize> Arena<K, T, N> {
    /// The capacity as a compile-time check: a handle index must fit.
    const CAPACITY_FITS: () = assert!(
        N <= Handle::<K>::MAX_INDEX as usize,
        "arena too large for a u16 handle index"
    );

    /// An empty arena.
    ///
    /// Not `const`: a free slot holds `T::default()` and `Default` is not a
    /// const trait. Nothing built one in a const context — only `size_of`
    /// names the type there, which needs no value.
    #[must_use]
    pub fn new() -> Self {
        let () = Self::CAPACITY_FITS;
        Self {
            slots: core::array::from_fn(|_| Slot {
                // FREE, with the counter at 1. The counter never reaches zero
                // (`try_insert` wraps to 1), because zero is the null handle's
                // generation and `resolve` compares generations directly.
                generation: FREE | 1,
                value: T::default(),
            }),
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
        let Some(index) = self.slots.iter().position(|s| !live(s.generation)) else {
            return Err(value);
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(value);
        };
        // Occupying a slot advances its counter and clears `FREE`. Past
        // `u16::MAX` it wraps to 1, never 0, so a live handle is never null.
        let mut generation = (slot.generation & 0xFFFF).wrapping_add(1);
        if generation > u32::from(u16::MAX) || generation == 0 {
            generation = 1;
        }
        slot.generation = generation;
        slot.value = value;
        // Wrapping: a free slot was found, so the arena is not full and
        // the count is below `N`.
        self.len = self.len.wrapping_add(1);
        // `index < N <= MAX_INDEX`, so the conversion cannot truncate.
        Ok(Handle::from_parts(index as u32, generation))
    }

    /// The object `handle` names.
    #[must_use]
    pub fn get(&self, handle: Handle<K>) -> Option<&T> {
        let slot = self.slots.get(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        Some(&slot.value)
    }

    /// The object `handle` names, mutably.
    #[must_use]
    pub fn get_mut(&mut self, handle: Handle<K>) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        Some(&mut slot.value)
    }

    /// Why a handle that did not resolve did not resolve.
    ///
    /// Only the two error arms call this, so asking costs nothing on a
    /// lookup that succeeds -- which is all but a vanishing few of them.
    /// That is bought by the BRANCH, not by outlining: the call sits inside
    /// the error arm either way. Outlined it was three branchless
    /// instructions behind `mv` + `jal` at 18 sites; `#[inline]` lets each
    /// site produce the code in place.
    #[inline]
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
            .get(handle.index() as usize)
            .ok_or(Error::InvalidHandle)?;
        if slot.generation != handle.generation() {
            return Err(Self::why(handle));
        }
        Ok(&slot.value)
    }

    /// Like [`Arena::resolve`], mutably.
    ///
    /// # Errors
    /// As [`Arena::resolve`].
    pub fn resolve_mut(&mut self, handle: Handle<K>) -> Result<&mut T> {
        let slot = self
            .slots
            .get_mut(handle.index() as usize)
            .ok_or(Error::InvalidHandle)?;
        if slot.generation != handle.generation() {
            return Err(Self::why(handle));
        }
        Ok(&mut slot.value)
    }

    /// Remove the object `handle` names, invalidating the handle and every
    /// copy of it.
    #[must_use]
    /// Free the slot `handle` names without handing the value back.
    ///
    /// Same state change as [`Arena::remove`] -- `FREE` set, `len`
    /// decremented, the old handle dead -- but it does not MOVE the value out,
    /// and that is the whole point. `remove` answers `Option<T>`, which for a
    /// `Tcb` is over a hundred bytes returned THROUGH MEMORY: the caller gets a
    /// stack buffer, `mem::take` copies the value into it, and `T::default()`
    /// is written over the slot with a `memset` CALL. Every arena removal in
    /// this kernel is spelled `let _ = ...remove(x)` -- not one reads the value
    /// -- so all of that filled a buffer that is dropped on the next line.
    ///
    /// Leaving the old value in the slot is sound because nothing can reach it:
    /// the slot's generation now carries [`FREE`], which no handle can equal,
    /// and `try_insert` overwrites the value before handing out a new handle.
    /// It is the same argument [`Arena::remove`]'s own doc makes about the
    /// generation bump, applied to the value as well.
    ///
    /// `Drop` is the one thing that cannot be left to chance, so it is not:
    /// `needs_drop` is a `const fn`, so for a `T` that owns something this
    /// still writes the default and runs the old value's destructor exactly
    /// where `remove` ran it, and for plain data the whole branch folds away.
    ///
    /// Returns whether a live object was there.
    pub fn discard(&mut self, handle: Handle<K>) -> bool {
        let Some(slot) = self.slots.get_mut(handle.index() as usize) else {
            return false;
        };
        if slot.generation != handle.generation() {
            return false;
        }
        if core::mem::needs_drop::<T>() {
            slot.value = T::default();
        }
        slot.generation |= FREE;
        // Wrapping: the generation matched, so a live value was here and the
        // count is at least one.
        self.len = self.len.wrapping_sub(1);
        true
    }

    /// Take the value out and free its slot, if `handle` still names it.
    ///
    /// `None` for a handle that is stale, forged or already removed -- and the
    /// slot's generation moves on, so no copy of this handle resolves again.
    pub fn remove(&mut self, handle: Handle<K>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        let value = core::mem::take(&mut slot.value);
        // Freeing sets `FREE` and leaves the counter alone; the next
        // `try_insert` advances it. The handle just invalidated compared equal
        // to the bare counter, and now the slot holds `FREE | counter`, which no
        // handle can equal — so it is dead the instant this store lands, and
        // dead again under a different counter when the slot is reused.
        slot.generation |= FREE;
        // Wrapping: `take` above answered `Some`, so a live value was
        // here and the count is at least one.
        self.len = self.len.wrapping_sub(1);
        Some(value)
    }

    /// Every live `(handle, object)` in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<K>, &T)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| live(s.generation))
            .map(|(i, s)| (Handle::from_parts(i as u32, s.generation), &s.value))
    }

    /// Every live `(handle, object)` in slot order, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<K>, &mut T)> + '_ {
        self.slots
            .iter_mut()
            .enumerate()
            .filter(|(_, s)| live(s.generation))
            .map(|(i, s)| (Handle::from_parts(i as u32, s.generation), &mut s.value))
    }

    /// The handle of the object at slot `index`, if live.
    #[must_use]
    pub fn handle_at(&self, index: u16) -> Option<Handle<K>> {
        let slot = self.slots.get(usize::from(index))?;
        live(slot.generation).then(|| Handle::from_parts(index as u32, slot.generation))
    }
}

impl<K: Kind, T: fmt::Debug + Default, const N: usize> fmt::Debug for Arena<K, T, N> {
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

    /// The invariant that replaced `Handle::from_raw`'s parity normalisation.
    ///
    /// A free slot is marked with a bit ABOVE the sixteen `from_raw` can
    /// produce, so no handle a C caller can forge may resolve to one. This is
    /// checked EXHAUSTIVELY over the whole forgeable generation space, because
    /// the old encoding's defence was an explicit fold and this one is a layout
    /// fact — if the fact ever stops holding, the fold is not there to catch it.
    ///
    /// It sweeps WHICH slot is live as well, which the first version of this
    /// test did not. That version pinned slot 1 as the live one, and a later
    /// experiment moved the free marker into the index half — where a collision
    /// is only reachable when the FREE slot sits at the index the marker names.
    /// Poisoning that encoding did not fail this test. A test that fixes the
    /// arrangement can only refute the arrangements it fixed.
    #[test]
    fn no_forgeable_handle_resolves_to_a_free_slot() {
        const N: u32 = 3;
        for live_at in 0..N {
            let mut a = Arena::<Task, u8, 3>::new();
            // Fill, then free everything except `live_at`, so every slot has
            // been occupied once and the survivor varies across the sweep.
            let handles: [Handle<Task>; 3] = [
                a.insert(1).unwrap(),
                a.insert(2).unwrap(),
                a.insert(3).unwrap(),
            ];
            for (i, h) in handles.iter().enumerate() {
                if i as u32 != live_at {
                    a.remove(*h).unwrap();
                }
            }
            let live = *handles.get(live_at as usize).unwrap();
            for generation in 0..=u32::from(u16::MAX) {
                for index in 0..N {
                    let forged = Handle::<Task>::from_raw((generation << 16) | index);
                    if forged == live {
                        assert!(a.resolve(forged).is_ok());
                        continue;
                    }
                    assert!(
                        a.resolve(forged).is_err(),
                        "live_at {live_at}: forged generation {generation}                          resolved at slot {index}"
                    );
                    assert!(a.get(forged).is_none());
                }
            }
        }
    }

    #[test]
    fn generations_never_mint_a_null_handle() {
        let mut a = Arena::<Task, u8, 1>::new();
        let mut last = 0u32;
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

    /// `is_empty` and `contains` are the two cheapest questions an arena
    /// answers, and `cargo mutants` found both replaceable by a constant
    /// without any test noticing.
    #[test]
    fn is_empty_and_contains_track_what_is_actually_in_there() {
        let mut a: Arena<Task, u8, 4> = Arena::new();
        assert!(a.is_empty(), "a new arena is empty");

        let h = a.insert(7).unwrap();
        assert!(!a.is_empty(), "and is not, once something is in it");
        assert!(a.contains(h), "the handle it just minted is in there");

        a.remove(h).unwrap();
        assert!(a.is_empty());
        assert!(!a.contains(h), "and the handle is not, after the remove");
    }

    /// `resolve_mut` checks the GENERATION, not just the slot. A stale
    /// handle whose slot has been reused must not hand out the new
    /// occupant -- that is the whole reason a handle carries a generation,
    /// and the check survived mutation until this test existed.
    #[test]
    fn resolve_mut_refuses_a_stale_handle_whose_slot_was_reused() {
        let mut a: Arena<Task, u8, 4> = Arena::new();
        let first = a.insert(1).unwrap();
        a.remove(first).unwrap();
        let second = a.insert(2).unwrap();

        assert_eq!(
            first.index(),
            second.index(),
            "the slot was reused, which is what makes this a real test"
        );
        assert!(
            a.resolve_mut(first).is_err(),
            "the STALE handle must not reach the new occupant"
        );
        assert_eq!(a.resolve_mut(second).map(|v| *v), Ok(2));
    }

    /// `iter_mut` has to reach every live entry, and be able to change it.
    #[test]
    fn iter_mut_reaches_and_can_change_every_live_entry() {
        let mut a: Arena<Task, u8, 4> = Arena::new();
        let h1 = a.insert(1).unwrap();
        let h2 = a.insert(2).unwrap();

        let mut seen = 0_usize;
        for (_, value) in a.iter_mut() {
            *value = value.saturating_add(10);
            seen = seen.saturating_add(1);
        }
        assert_eq!(seen, 2, "both entries, not an empty iterator");
        assert_eq!(a.resolve(h1).copied(), Ok(11));
        assert_eq!(a.resolve(h2).copied(), Ok(12));
    }
}
