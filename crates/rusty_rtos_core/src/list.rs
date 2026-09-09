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
        link >= END_BASE && link != NONE
    }

    fn list_of_end(link: u16) -> Option<ListId> {
        if Self::is_end(link) {
            u8::try_from(link.wrapping_sub(END_BASE)).ok()
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

    /// `(prev, next, value)` of any node, item or end marker.
    fn links(&self, link: u16) -> Result<(u16, u16, u64)> {
        if let Some(list) = Self::list_of_end(link) {
            let e = self.end(list)?;
            Ok((e.prev, e.next, Self::MAX_VALUE))
        } else {
            let n = self.item(link)?;
            Ok((n.prev, n.next, n.value))
        }
    }

    fn set_next(&mut self, link: u16, next: u16) -> Result<()> {
        if let Some(list) = Self::list_of_end(link) {
            self.end_mut(list)?.next = next;
        } else {
            self.item_mut(link)?.next = next;
        }
        Ok(())
    }

    fn set_prev(&mut self, link: u16, prev: u16) -> Result<()> {
        if let Some(list) = Self::list_of_end(link) {
            self.end_mut(list)?.prev = prev;
        } else {
            self.item_mut(link)?.prev = prev;
        }
        Ok(())
    }

    /// Link `item` between `before` and `before.next`, in `list`.
    fn link_after(&mut self, list: ListId, item: ItemId, before: u16) -> Result<()> {
        let (_, after, _) = self.links(before)?;
        {
            let n = self.item_mut(item)?;
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
        self.end(list)?;
        if self.item(item)?.container.is_some() {
            return Err(Error::Busy);
        }
        self.item_mut(item)?.value = value;
        let end = Self::end_of(list);
        let before = if value == Self::MAX_VALUE {
            self.end(list)?.prev
        } else {
            // Walk from the end marker while the NEXT node's value is <= ours,
            // which stops at the marker (MAX_VALUE) at the latest.
            let mut iter = end;
            let mut guard = 0usize;
            loop {
                let (_, next, _) = self.links(iter)?;
                let (_, _, next_value) = self.links(next)?;
                if next_value > value {
                    break;
                }
                iter = next;
                guard = guard.saturating_add(1);
                if guard > N {
                    return Err(Error::InvalidArgument);
                }
            }
            iter
        };
        self.link_after(list, item, before)
    }

    /// `vListInsertEnd`: put `item` in `list` immediately before the cursor,
    /// so a round-robin walk reaches it last. The item's value is kept.
    ///
    /// # Errors
    /// As [`Lists::insert`].
    pub fn insert_end(&mut self, list: ListId, item: ItemId) -> Result<()> {
        let cursor = self.end(list)?.cursor;
        if self.item(item)?.container.is_some() {
            return Err(Error::Busy);
        }
        let (before, _, _) = self.links(cursor)?;
        self.link_after(list, item, before)
    }

    /// `uxListRemove`: take `item` out of whichever list holds it and return
    /// how many items that list still has. If the cursor sat on the item, it
    /// moves to the previous node, as in C.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad item; [`Error::NotActive`] if the
    /// item is in no list.
    pub fn remove(&mut self, item: ItemId) -> Result<usize> {
        let (prev, next, _) = self.links(item)?;
        let Some(list) = self.item(item)?.container else {
            return Err(Error::NotActive);
        };
        self.set_next(prev, next)?;
        self.set_prev(next, prev)?;
        {
            let e = self.end_mut(list)?;
            if e.cursor == item {
                e.cursor = prev;
            }
            e.len = e.len.saturating_sub(1);
        }
        let n = self.item_mut(item)?;
        n.container = None;
        n.prev = NONE;
        n.next = NONE;
        Ok(usize::from(self.end(list)?.len))
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

    /// `listGET_OWNER_OF_NEXT_ENTRY`: advance the cursor past the end marker
    /// and return the item it lands on, or `None` when the list is empty.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a list outside `0..L`.
    pub fn next_round_robin(&mut self, list: ListId) -> Result<Option<ItemId>> {
        let end = Self::end_of(list);
        let e = self.end(list)?;
        if e.len == 0 {
            return Ok(None);
        }
        let (_, mut next, _) = self.links(e.cursor)?;
        if next == end {
            let (_, first, _) = self.links(end)?;
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
        if self.item(item)?.container.is_none() {
            return Err(Error::NotActive);
        }
        let (_, next, _) = self.links(item)?;
        Ok((!Self::is_end(next)).then_some(next))
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
