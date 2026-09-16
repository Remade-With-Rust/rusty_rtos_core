//! `list.c` remade over indices: the scheduler's ready, delayed, suspended
//! and event lists without a pointer.
//!
//! FreeRTOS threads a `ListItem_t` through every control block: a doubly
//! linked, circular list with an end marker whose value is `portMAX_DELAY`,
//! a per-list cursor (`pxIndex`) that `vListInsertEnd` and the round-robin
//! walk use, and a `pxContainer` back-pointer so `uxListRemove` needs no
//! list argument. Every one of those is here, over `u16` indices:
//!
//! * `vListInsert`     → [`Lists::insert`] — sorted by value, after equal values;
//!   a value of [`Lists::MAX_VALUE`] goes last (the C special case).
//! * `vListInsertEnd`  → [`Lists::insert_end`] — immediately before the cursor.
//! * `uxListRemove`    → [`Lists::remove`] — returns the remaining count and
//!   moves the cursor back if it sat on the removed item.
//! * `listGET_OWNER_OF_NEXT_ENTRY` → [`Lists::next_round_robin`].
//! * `listGET_HEAD_ENTRY` / `listGET_ITEM_VALUE_OF_HEAD_ENTRY` → [`Lists::head`],
//!   [`Lists::head_value`].
//! * `listLIST_ITEM_CONTAINER` → [`Lists::container`].
//!
//! `N` items (one per state item or event item the kernel owns) can each be
//! in at most one of `L` lists at a time, exactly as in C. Nothing here
//! panics: a wrong index or a double insert is an [`Error`], where the C
//! kernel would corrupt the list.

use crate::error::{Error, Result};

/// One of the `L` lists, `0..L`.
pub type ListId = u8;

/// One of the `N` items, `0..N`.
pub type ItemId = u16;

const NONE: u16 = u16::MAX;
const END_BASE: u16 = 0x8000;

#[derive(Clone, Copy)]
struct Node {
    prev: u16,
    next: u16,
    value: u64,
    container: Option<ListId>,
}

impl Node {
    const EMPTY: Self = Self {
        prev: NONE,
        next: NONE,
        value: 0,
        container: None,
    };
}

#[derive(Clone, Copy)]
struct End {
    /// The end marker's own links (a circular list is never empty of nodes).
    prev: u16,
    next: u16,
    /// `pxIndex`: the cursor `vListInsertEnd` inserts before and the
    /// round-robin walk advances.
    cursor: u16,
    len: u16,
}

/// `N` list items shared by `L` lists.
pub struct Lists<const N: usize, const L: usize> {
    items: [Node; N],
    ends: [End; L],
}

impl<const N: usize, const L: usize> Default for Lists<N, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const L: usize> Lists<N, L> {
    /// The end marker's value, `portMAX_DELAY`: an item inserted with this
    /// value goes last, after every other item of the same value.
    pub const MAX_VALUE: u64 = u64::MAX;

    const SIZES_FIT: () = assert!(
        N < END_BASE as usize && L <= u8::MAX as usize,
        "at most 32767 items and 255 lists"
    );

    /// `L` empty lists over `N` unlinked items.
    #[must_use]
    // The one index in this crate: a const fn cannot use `get_mut`, and the
    // loop condition `l < L` bounds it.
    #[allow(clippy::indexing_slicing)]
    pub const fn new() -> Self {
        let () = Self::SIZES_FIT;
        let mut ends = [End {
            prev: NONE,
            next: NONE,
            cursor: NONE,
            len: 0,
        }; L];
        let mut l = 0;
        while l < L {
            let e = END_BASE.wrapping_add(l as u16);
            ends[l] = End {
                prev: e,
                next: e,
                cursor: e,
                len: 0,
            };
            l = l.wrapping_add(1);
        }
        Self {
            items: [const { Node::EMPTY }; N],
            ends,
        }
    }

    const fn end_of(list: ListId) -> u16 {
        END_BASE.wrapping_add(list as u16)
    }

    const fn is_end(link: u16) -> bool {
        link >= END_BASE
    }

    /// The `ends` subscript for an end-marker link, or `None` for an item.
    ///
    /// [`NONE`] needs no special case: it is `0x7fff` above `END_BASE` and
    /// `L <= u8::MAX`, so the bounds check on `ends` refuses it anyway —
    /// one comparison doing what the old `link != NONE` did with two.
    const fn end_index(link: u16) -> Option<usize> {
        if link >= END_BASE {
            Some(link.wrapping_sub(END_BASE) as usize)
        } else {
            None
        }
    }

    fn end(&self, list: ListId) -> Result<&End> {
        self.ends
            .get(usize::from(list))
            .ok_or(Error::InvalidArgument)
    }

    fn end_mut(&mut self, list: ListId) -> Result<&mut End> {
        self.ends
            .get_mut(usize::from(list))
            .ok_or(Error::InvalidArgument)
    }

    fn item(&self, item: ItemId) -> Result<&Node> {
        self.items
            .get(usize::from(item))
            .ok_or(Error::InvalidArgument)
    }

    fn item_mut(&mut self, item: ItemId) -> Result<&mut Node> {
        self.items
            .get_mut(usize::from(item))
            .ok_or(Error::InvalidArgument)
    }

    /// `(next, value)` of any node: what the sorted walk reads per step.
    ///
    /// Every one of these helpers takes exactly the fields its caller needs
    /// from **one** array read. The version this replaced returned all three
    /// fields as a tuple, so a caller wanting two of them from two nodes
    /// paid four bounds checks and four end-marker tests for two reads.
    /// That re-reading was most of what put this list at 2.08x `list.c`.
    fn next_and_value(&self, link: u16) -> Result<(u16, u64)> {
        if let Some(i) = Self::end_index(link) {
            let e = self.ends.get(i).ok_or(Error::InvalidArgument)?;
            Ok((e.next, Self::MAX_VALUE))
        } else {
            let n = self.item(link)?;
            Ok((n.next, n.value))
        }
    }

    /// `prev` of any node.
    fn prev_of(&self, link: u16) -> Result<u16> {
        if let Some(i) = Self::end_index(link) {
            Ok(self.ends.get(i).ok_or(Error::InvalidArgument)?.prev)
        } else {
            Ok(self.item(link)?.prev)
        }
    }

    fn set_next(&mut self, link: u16, next: u16) -> Result<()> {
        if let Some(i) = Self::end_index(link) {
            self.ends.get_mut(i).ok_or(Error::InvalidArgument)?.next = next;
        } else {
            self.item_mut(link)?.next = next;
        }
        Ok(())
    }

    fn set_prev(&mut self, link: u16, prev: u16) -> Result<()> {
        if let Some(i) = Self::end_index(link) {
            self.ends.get_mut(i).ok_or(Error::InvalidArgument)?.prev = prev;
        } else {
            self.item_mut(link)?.prev = prev;
        }
        Ok(())
    }

    /// Link `item` between `before` and `after`, in `list`.
    ///
    /// The caller passes `after` because it always already knows it:
    /// [`Lists::insert`] has just walked to it, and [`Lists::insert_end`]
    /// inserts before the cursor. Reading `before.next` here again was a
    /// whole node read per insert for a value the caller had in hand.
    fn link_between(
        &mut self,
        list: ListId,
        item: ItemId,
        before: u16,
        after: u16,
        value: Option<u64>,
    ) -> Result<()> {
        {
            let n = self.item_mut(item)?;
            // The `Busy` check rides the read that is happening anyway, and
            // happens before anything is written — so a refused insert
            // still leaves both lists exactly as it found them.
            if n.container.is_some() {
                return Err(Error::Busy);
            }
            if let Some(value) = value {
                n.value = value;
            }
            n.prev = before;
            n.next = after;
            n.container = Some(list);
        }
        self.set_next(before, item)?;
        self.set_prev(after, item)?;
        let e = self.end_mut(list)?;
        e.len = e.len.saturating_add(1);
        Ok(())
    }

    /// The value an item sorts by (`xItemValue`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn value(&self, item: ItemId) -> Result<u64> {
        Ok(self.item(item)?.value)
    }

    /// Set an item's value (`listSET_LIST_ITEM_VALUE`). Allowed while the
    /// item is in a list, exactly as in C — the caller re-inserts to re-sort.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn set_value(&mut self, item: ItemId, value: u64) -> Result<()> {
        self.item_mut(item)?.value = value;
        Ok(())
    }

    /// The list `item` is in, if any (`listLIST_ITEM_CONTAINER`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn container(&self, item: ItemId) -> Result<Option<ListId>> {
        Ok(self.item(item)?.container)
    }

    /// `vListInsert`: put `item` in `list` sorted ascending by `value`,
    /// after every item with an equal value; [`Lists::MAX_VALUE`] goes last.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad list or item; [`Error::Busy`] if
    /// the item is already in a list (C would corrupt both lists).
    pub fn insert(&mut self, list: ListId, item: ItemId, value: u64) -> Result<()> {
        self.insert_inner(list, item, Some(value))
    }

    /// `vListInsert` sorting by the value the item already carries.
    ///
    /// The event lists want exactly this: a task's event item keeps the
    /// value its call set, and the caller used to read that value out and
    /// hand it straight back, so the item was read once to be written with
    /// what it already held.
    ///
    /// # Errors
    /// As [`Lists::insert`].
    pub fn insert_keeping_value(&mut self, list: ListId, item: ItemId) -> Result<()> {
        self.insert_inner(list, item, None)
    }

    fn insert_inner(&mut self, list: ListId, item: ItemId, keep: Option<u64>) -> Result<()> {
        // `None` means "sort by the value the item already carries", which
        // is what every event-list insert wants; `link_between` then skips
        // the write, because there is nothing to change.
        let value = match keep {
            Some(v) => v,
            None => self.item(item)?.value,
        };
        let end = Self::end_of(list);
        let (before, after) = if value == Self::MAX_VALUE {
            (self.end(list)?.prev, end)
        } else {
            // Walk from the end marker while the NEXT node's value is <= ours,
            // which stops at the marker (MAX_VALUE) at the latest.
            //
            // One node read per step, not two: the node whose value decides
            // the step is also the node whose `next` is the following step's,
            // so `next_and_value` takes both at once. `after` falls out of
            // the walk, which is why nothing re-reads `before.next` after it.
            let mut before = end;
            let mut after = self.next_and_value(end)?.0;
            let mut guard = 0usize;
            loop {
                let (following, after_value) = self.next_and_value(after)?;
                if after_value > value {
                    break;
                }
                before = after;
                after = following;
                guard = guard.saturating_add(1);
                if guard > N {
                    return Err(Error::InvalidArgument);
                }
            }
            (before, after)
        };
        self.link_between(list, item, before, after, keep)
    }

    /// `vListInsertEnd`: put `item` in `list` immediately before the cursor,
    /// so a round-robin walk reaches it last. The item's value is kept.
    ///
    /// # Errors
    /// As [`Lists::insert`].
    pub fn insert_end(&mut self, list: ListId, item: ItemId) -> Result<()> {
        // The cursor is the node we insert before, so it *is* `after` and
        // nothing has to read `before.next` to find it again.
        let cursor = self.end(list)?.cursor;
        let before = self.prev_of(cursor)?;
        self.link_between(list, item, before, cursor, None)
    }

    /// `uxListRemove`: take `item` out of whichever list holds it and return
    /// how many items that list still has. If the cursor sat on the item, it
    /// moves to the previous node, as in C.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad item; [`Error::NotActive`] if the
    /// item is in no list.
    pub fn remove(&mut self, item: ItemId) -> Result<usize> {
        // One read takes all three fields. An `ItemId` is never an end
        // marker, so this goes straight to `items` rather than through the
        // end-marker test the general helpers have to make.
        let (prev, next, container) = {
            let n = self.item(item)?;
            (n.prev, n.next, n.container)
        };
        let Some(list) = container else {
            return Err(Error::NotActive);
        };
        self.set_next(prev, next)?;
        self.set_prev(next, prev)?;
        {
            let n = self.item_mut(item)?;
            n.container = None;
            n.prev = NONE;
            n.next = NONE;
        }
        // The length this answers is the one just decremented, so the
        // trailing re-read of the end marker is gone.
        let e = self.end_mut(list)?;
        if e.cursor == item {
            e.cursor = prev;
        }
        e.len = e.len.saturating_sub(1);
        Ok(usize::from(e.len))
    }

    /// `listLIST_IS_EMPTY`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn is_empty(&self, list: ListId) -> Result<bool> {
        Ok(self.end(list)?.len == 0)
    }

    /// `listCURRENT_LIST_LENGTH`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn len(&self, list: ListId) -> Result<usize> {
        Ok(usize::from(self.end(list)?.len))
    }

    /// `listGET_HEAD_ENTRY`: the first item, or `None` when empty.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn head(&self, list: ListId) -> Result<Option<ItemId>> {
        let e = self.end(list)?;
        Ok((e.len > 0 && !Self::is_end(e.next)).then_some(e.next))
    }

    /// `listGET_ITEM_VALUE_OF_HEAD_ENTRY`: the first item's value, or
    /// [`Lists::MAX_VALUE`] (the end marker's) when empty — exactly the C
    /// idiom the delayed-list wake-time check relies on.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn head_value(&self, list: ListId) -> Result<u64> {
        match self.head(list)? {
            Some(item) => self.value(item),
            None => Ok(Self::MAX_VALUE),
        }
    }

    /// Where the round-robin cursor (`pxIndex`) currently sits.
    ///
    /// A diagnostic. When a ready task is never chosen, the question is
    /// always "did the cursor move", and nothing else can answer it.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list that does not exist.
    pub fn cursor_of(&self, list: ListId) -> Result<ItemId> {
        Ok(self.end(list)?.cursor)
    }

    /// `listGET_OWNER_OF_NEXT_ENTRY`: advance the cursor past the end marker
    /// and return the item it lands on, or `None` when the list is empty.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn next_round_robin(&mut self, list: ListId) -> Result<Option<ItemId>> {
        let end = Self::end_of(list);
        // The node after the end marker is the list's first item, which is
        // the `ends[list].next` this read already took — so wrapping costs
        // no second read, which is what it used to cost on every lap.
        let (cursor, len, first) = {
            let e = self.end(list)?;
            (e.cursor, e.len, e.next)
        };
        if len == 0 {
            return Ok(None);
        }
        let mut next = if cursor == end {
            first
        } else {
            self.item(cursor)?.next
        };
        if next == end {
            next = first;
        }
        self.end_mut(list)?.cursor = next;
        Ok((!Self::is_end(next)).then_some(next))
    }

    /// The item after `item` in its list, or `None` at the end.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad item; [`Error::NotActive`] if
    /// the item is in no list.
    pub fn next(&self, item: ItemId) -> Result<Option<ItemId>> {
        let n = self.item(item)?;
        if n.container.is_none() {
            return Err(Error::NotActive);
        }
        Ok((!Self::is_end(n.next)).then_some(n.next))
    }

    /// The items of `list` from the head, in list order.
    pub fn iter(&self, list: ListId) -> Iter<'_, N, L> {
        Iter {
            lists: self,
            at: self.head(list).ok().flatten(),
            remaining: self.len(list).unwrap_or(0),
        }
    }
}

/// The items of one list, head first.
pub struct Iter<'a, const N: usize, const L: usize> {
    lists: &'a Lists<N, L>,
    at: Option<ItemId>,
    remaining: usize,
}

impl<const N: usize, const L: usize> Iterator for Iter<'_, N, L> {
    type Item = ItemId;

    fn next(&mut self) -> Option<ItemId> {
        if self.remaining == 0 {
            return None;
        }
        let item = self.at?;
        self.remaining = self.remaining.saturating_sub(1);
        self.at = self.lists.next(item).ok().flatten();
        Some(item)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;

    fn order(l: &Lists<8, 2>, list: ListId) -> Vec<ItemId> {
        l.iter(list).collect()
    }

    /// `listGET_OWNER_OF_NEXT_ENTRY` on a two-item list must ALTERNATE.
    ///
    /// This is the whole of round-robin scheduling between equal
    /// priorities, and two items is the case where getting it wrong is
    /// invisible in a total and fatal in practice: a cursor that advances
    /// twice per call, or not at all, returns the same item for ever and
    /// one of the two tasks never runs again.
    #[test]
    fn round_robin_alternates_between_two_items() {
        let mut l = Lists::<8, 2>::new();
        l.insert_end(0, 1).unwrap();
        l.insert_end(0, 2).unwrap();
        assert_eq!(l.len(0).unwrap(), 2);

        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(l.next_round_robin(0).unwrap());
        }
        let mut it = seen.iter();
        let a = it.next().copied().unwrap();
        let b = it.next().copied().unwrap();
        assert_ne!(a, b, "two calls in a row returned the same item: {seen:?}");
        let want = [a, b, a, b, a, b];
        assert!(
            seen.iter().eq(want.iter()),
            "the rotation did not alternate: {seen:?}"
        );
    }

    /// The rotation must survive OTHER lists being used in between.
    ///
    /// This is the scheduler's real shape: a busy priority with two ready
    /// tasks, and a higher priority whose task keeps blocking and waking.
    /// Between two selections from the busy list, another list is rotated,
    /// emptied and refilled -- and the busy list's cursor has to be
    /// exactly where it was left.
    #[test]
    fn round_robin_survives_another_list_being_used_between_calls() {
        let mut l = Lists::<8, 2>::new();
        // list 0: the busy priority, two tasks that never leave.
        l.insert_end(0, 1).unwrap();
        l.insert_end(0, 2).unwrap();
        // list 1: the higher priority, one task that comes and goes.
        l.insert_end(1, 3).unwrap();

        let mut seen = Vec::new();
        for _ in 0..6 {
            // the higher priority runs...
            assert_eq!(l.next_round_robin(1).unwrap(), Some(3));
            // ...blocks...
            l.remove(3).unwrap();
            // ...so the scheduler comes back to the busy list.
            seen.push(l.next_round_robin(0).unwrap().unwrap());
            // ...and later it wakes again.
            l.insert_end(1, 3).unwrap();
        }
        let a = seen.first().copied().unwrap();
        assert!(
            seen.iter().any(|&x| x != a),
            "the busy list stopped rotating once another list was used: {seen:?}"
        );
    }

    /// Three items rotate in order and come back round.
    #[test]
    fn round_robin_visits_every_item_in_turn() {
        let mut l = Lists::<8, 2>::new();
        for item in 1..=3 {
            l.insert_end(0, item).unwrap();
        }
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(l.next_round_robin(0).unwrap().unwrap());
        }
        assert_eq!(seen.len(), 6);
        let first = seen.first().copied().unwrap();
        assert_eq!(
            seen.get(3).copied(),
            Some(first),
            "it did not come back round after three"
        );
        let mut lap: Vec<ItemId> = seen.iter().take(3).copied().collect();
        lap.sort_unstable();
        assert!(
            lap.iter().eq([1, 2, 3].iter()),
            "not every item was visited: {seen:?}"
        );
    }

    #[test]
    fn insert_sorts_ascending_after_equal_values_like_v_list_insert() {
        let mut l = Lists::<8, 2>::new();
        assert!(l.is_empty(0).unwrap());
        assert_eq!(l.head_value(0).unwrap(), Lists::<8, 2>::MAX_VALUE);
        l.insert(0, 3, 30).unwrap();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 4, 20).unwrap(); // equal value: after item 2
        l.insert(0, 5, Lists::<8, 2>::MAX_VALUE).unwrap();
        l.insert(0, 6, Lists::<8, 2>::MAX_VALUE).unwrap(); // MAX after MAX
        assert_eq!(order(&l, 0), [1, 2, 4, 3, 5, 6]);
        assert_eq!(l.head(0).unwrap(), Some(1));
        assert_eq!(l.head_value(0).unwrap(), 10);
        assert_eq!(l.len(0).unwrap(), 6);
        assert_eq!(l.container(4).unwrap(), Some(0));
        assert_eq!(l.container(7).unwrap(), None);
    }

    #[test]
    fn insert_end_and_the_round_robin_cursor() {
        let mut l = Lists::<8, 2>::new();
        assert_eq!(l.next_round_robin(0).unwrap(), None);
        l.insert_end(0, 1).unwrap();
        l.insert_end(0, 2).unwrap();
        l.insert_end(0, 3).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3]);
        // The C idiom: the walk returns 1, 2, 3, 1, ...
        assert_eq!(l.next_round_robin(0).unwrap(), Some(1));
        assert_eq!(l.next_round_robin(0).unwrap(), Some(2));
        // vListInsertEnd inserts BEFORE the cursor (which sits on 2), so 4 is
        // reached only after the rest of the round.
        l.insert_end(0, 4).unwrap();
        assert_eq!(order(&l, 0), [1, 4, 2, 3]);
        assert_eq!(l.next_round_robin(0).unwrap(), Some(3));
        assert_eq!(l.next_round_robin(0).unwrap(), Some(1));
        assert_eq!(l.next_round_robin(0).unwrap(), Some(4));
    }

    #[test]
    fn remove_returns_the_count_and_moves_the_cursor_back() {
        let mut l = Lists::<8, 2>::new();
        for i in 1..=3 {
            l.insert_end(0, i).unwrap();
        }
        assert_eq!(l.next_round_robin(0).unwrap(), Some(1));
        assert_eq!(l.next_round_robin(0).unwrap(), Some(2));
        // The cursor sits on 2; removing 2 moves it to 1, so the next walk
        // step is 3, not a skipped item.
        assert_eq!(l.remove(2).unwrap(), 2);
        assert_eq!(l.container(2).unwrap(), None);
        assert_eq!(l.next_round_robin(0).unwrap(), Some(3));
        assert_eq!(l.remove(1).unwrap(), 1);
        assert_eq!(l.remove(3).unwrap(), 0);
        assert!(l.is_empty(0).unwrap());
        assert_eq!(l.next_round_robin(0).unwrap(), None);
        assert_eq!(l.remove(3), Err(Error::NotActive));
    }

    #[test]
    fn an_item_is_in_one_list_at_a_time_and_moves_between_lists() {
        let mut l = Lists::<8, 2>::new();
        l.insert(0, 1, 5).unwrap();
        assert_eq!(l.insert(1, 1, 5), Err(Error::Busy));
        assert_eq!(l.insert_end(1, 1), Err(Error::Busy));
        l.remove(1).unwrap();
        l.insert(1, 1, 7).unwrap();
        assert_eq!(l.container(1).unwrap(), Some(1));
        assert_eq!(order(&l, 0), []);
        assert_eq!(order(&l, 1), [1]);
        assert_eq!(l.value(1).unwrap(), 7);
    }

    #[test]
    fn bad_arguments_are_errors_not_panics() {
        let mut l = Lists::<4, 1>::new();
        assert_eq!(l.insert(1, 0, 0), Err(Error::InvalidArgument));
        assert_eq!(l.insert(0, 4, 0), Err(Error::InvalidArgument));
        assert_eq!(l.remove(9), Err(Error::InvalidArgument));
        assert_eq!(l.head(3), Err(Error::InvalidArgument));
        assert_eq!(l.next(0), Err(Error::NotActive));
        assert_eq!(l.set_value(7, 1), Err(Error::InvalidArgument));
    }

    #[test]
    fn set_value_then_reinsert_resorts() {
        let mut l = Lists::<4, 1>::new();
        l.insert(0, 0, 1).unwrap();
        l.insert(0, 1, 2).unwrap();
        l.remove(0).unwrap();
        l.set_value(0, 9).unwrap();
        l.insert(0, 0, l.value(0).unwrap()).unwrap();
        assert_eq!(l.iter(0).collect::<Vec<_>>(), [1, 0]);
    }
}
