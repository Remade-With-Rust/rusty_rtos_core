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

//! # What has been tried here, measured (2026-09-19)
//!
//! ## The third pass: the tail fast path, and why it is not here
//!
//! A sorted list's LAST item carries its largest value, one read away. If the
//! arriving value is at least as large, the walk was always going to run the
//! whole list and stop at the marker -- so one comparison replaces `len` of
//! them. It subsumes the `portMAX_DELAY` branch exactly, and `>=` keeps
//! `vListInsert`'s rule that a later equal value goes AFTER the ones already
//! there. It is the right shape for a delayed list, whose wake times are
//! `now + something` with `now` only increasing, so they arrive ASCENDING.
//!
//! On `bench/list-ir` it read **-11.8%** (x86-64) and **-7.7%** (i686).
//! At the KERNEL level it is a LOSS, and the loss is a function of depth:
//!
//! | delayed-list depth | kdelay-ir head | with the fast path |
//! |---|---|---|
//! | 4 (what this kernel reaches) | 2,280,288 | 2,389,308 (**+4.78%**) |
//! | 17 | 2,662,691 | 2,684,102 (**+0.80%**) |
//!
//! The penalty shrinks with depth exactly as it should, and it does not
//! reach zero by 17; extrapolated, break-even is near depth 20. The kernel's
//! geometry caps tasks at 24 and its own workload parks 4. **So it does not
//! pay at any depth this kernel can reach**, and a 12% stage-level win is a
//! 5% system-level loss. That is the "one probe must measure the level above
//! the change" rule earning its keep.
//!
//! Why it costs anything at all: the fast path is only correct on a SORTED
//! list, `insert_end` (`vListInsertEnd`) does not keep one sorted, and the
//! kernel mixes both on the pending-ready list. The guard is a flag on
//! `End`, and maintaining it taxes EVERY insert and remove -- which on a
//! kernel is overwhelmingly ready-list traffic that never walks anything.
//! Measured, the guard's *test* is free (identical counts with and without
//! it); the cost is entirely its upkeep, and moving the flag into a spare
//! bit of `len` to shrink `End` was worse again.
//!
//! **If you come back to this**, the conditions are recorded: a system whose
//! delayed or timer list routinely holds ~20 entries, `bench/kdelay-ir`
//! (`--features deep` for depth 17) to price it, and the four ordering tests
//! below, which pin the contract the fast path has to preserve and which the
//! first cut of it broke while measuring -18% and passing everything.
//!
//! ## The third pass, continued: three wins and three more refutations
//!
//! The wins are all the same shape, and it is not the shape anyone would
//! have guessed: **collapse two lookups of the same `End` into one**, where
//! the two sit NEXT TO each other.
//!
//! * `insert_end` read `ends[list]` for the cursor and then sent that cursor
//!   through a general `prev` helper, which reads `ends[list]` again for a
//!   field the first read already had. -1.15% ksched-ir, -1.19% i686.
//! * `link_between` wrote the marker through `set_next`/`set_prev`, each of
//!   which looks `ends[list]` up, and then took it a third time to bump the
//!   length. -3.80% ksched-ir, -3.47% kdelay-ir, -3.70% x86-64.
//! * `iter` built itself from `head()` AND `len()`, which are both `end()`,
//!   and then walked through `next()`, which re-checks that the item is in a
//!   list and wraps the answer in a `Result<Option<_>>` for `.ok().flatten()`
//!   to unwrap again. -5.41% x86-64, -5.35% i686, and BIT-IDENTICAL on both
//!   kernel instruments, because `iter` has one caller up there and it is a
//!   diagnostic. A win for the crate, not for the kernel; that identity is
//!   also the proof the change leaked nowhere else.
//!
//! The refutations are the reason to distrust every sentence above:
//!
//! | tried | verdict |
//! |---|---|
//! | the SAME fold in `remove` | +6.18% x86-64, **+10.21% i686**, +2.25% kdelay-ir |
//! | `head` drops `e.len > 0`, which is provably redundant | **+3.71%** x86-64, +3.48% i686 |
//! | the sorted flag in `len`'s spare bit rather than a `bool` on `End` | +1,026,042 on the arm it was meant to fix |
//!
//! **`remove` is the one to remember.** It is the identical edit to the
//! `link_between` win -- same five lines, same reasoning -- and it loses by
//! 10% on the machine the product ships on. It was measured, refuted,
//! re-measured after `link_between` landed and moved its baseline, and lost
//! by MORE. The difference is DISTANCE: in `link_between` the folded writes
//! are adjacent to the length bump and collapse into one access, while in
//! `remove` the block that clears the item sits between them and the two
//! flags stay live across it. That is not visible on the page, and the
//! prediction drawn from one of them was wrong about the other -- in both
//! directions, on consecutive attempts.
//!
//! **`head` is the one that should end the arguing.** `e.len > 0` and
//! `!is_end(e.next)` cannot disagree -- a list is empty exactly when the
//! marker is its own `next` -- so one of them is free to delete. Deleting it
//! cost 3.7%, because `iter` calls `head` AND `len`, and the test being
//! removed was what let LLVM share work between them.
//!
//! So: every local argument about cost in this file has been wrong at least
//! once, in both directions, including the ones with a proof attached.
//! Measure on all four instruments -- list-ir at both widths, kdelay-ir,
//! ksched-ir -- and believe those.
//!
//! ## Where the fifth pass stopped, and why
//!
//! Two more, after the gated pair landed:
//!
//! | tried | verdict |
//! |---|---|
//! | the SAME gated marker test, in `next_round_robin` | **+1.94% i686** |
//! | `Iter::remaining` as a `u16` rather than a `usize` | +1.12% x86-64, +2.45% i686 |
//!
//! **The first one is this file in one line.** `is_marker_of` is the helper
//! that WON -1.78% on i686 in `link_between`. The identical call, in the
//! neighbouring function, on the same machine, loses 1.94%. Nothing on the
//! page distinguishes them.
//!
//! That is the third pair of neighbours here where one edit flips sign --
//! after `link_between`/`remove` for the marker fold, and the walk for any
//! restructuring at all. The file has no transferable structure left: every
//! remaining edit is a coin flip that costs a four-arm sweep to resolve.
//! Nine wins against twenty-two refutations across five passes is where it
//! stopped, and the next instruction is not in here -- on the blocking
//! workload `kernel.rs` is the larger consumer, and until 2026-09-19 no
//! instrument had ever made a task block.
//!
//! ## The fifth pass: one win, six refutations, and a wall
//!
//! The win is the same move as the third pass's three: **delete a call whose
//! body carries its own branch and its own `Result`.**
//!
//! * `remove` reached the end marker through `set_next`/`set_prev`, which
//!   subtract `END_BASE`, wrap the difference in an `Option<usize>`, and
//!   index `ends` with it. `remove` already knows the list, so `end_mut`
//!   names the same struct with no arithmetic. -2.04% x86-64, -1.72%
//!   ksched-ir, -1.06% kdelay-ir, -0.75% i686. Both helpers then died.
//!
//! | tried | verdict |
//! |---|---|
//! | one `ends[list]` read serving BOTH arms of `insert_inner` | flat |
//! | the same deletion in `next_and_value`, via a `list` parameter | -2.03% x86-64 / **+7.43% i686** |
//! | ...the same, inlined so no parameter is passed | **+38.6% x86-64** / +7.43% i686 |
//! | `head_value` reading straight through instead of via `head` + `value` | -0.12% x86-64 / **+2.09% i686** |
//! | `insert_inner` passing `Some(value)` to fold the `Option` at both sites | flat |
//!
//! **Two walls came out of this, and they are the useful part.**
//!
//! **The sorted-insert walk is codegen-fragile. Do not restructure its
//! body.** Three separate attempts -- breaking on `is_end` instead of on a
//! fetched `MAX_VALUE`, the same copying fields out instead of borrowing,
//! and inlining `next_and_value` into it -- cost **+26.9%, +26.9% and
//! +38.6%** on x86-64. In every one of them the per-step instruction
//! sequence is provably identical to what it replaced: one compare, one
//! bounds check, one read, one compare. What moved was loop shape. This is
//! the most obvious place in the file to optimise and the most expensive
//! place to try.
//!
//! **The i686 arm is the binding constraint, and it is not a formality.**
//! Three consecutive changes that the host accepted or liked -- two of them
//! deletions -- were rejected by the 32-bit arm at +7.43%, +7.43% and
//! +2.09%. Every Kairos target is 32-bit. A host-only harness would have
//! shipped all three.
//!
//! The parameter was NOT the cause of the first two, which is worth saying
//! because it was the obvious explanation and it was wrong: the inlined
//! version passes no parameter and reads +7.43% on i686 to the instruction.
//! What costs there is `end(list)` standing in for `end_index`.
//!
//! ## The fourth pass: six refutations and no wins
//!
//! Driven off a census this time rather than off reading, and it still went
//! 0 for 6. Recorded in full because five of the six are ideas that look
//! obviously correct on the page.
//!
//! | tried | verdict |
//! |---|---|
//! | split the `self` borrow so `next_round_robin` takes ONE `ends[list]` | flat (-82 on a floor of 82) |
//! | `.get(i).ok_or(..)?` -> explicit range test + index, in all four accessors | flat (-0.06%) |
//! | `take_head_before`: fuse `is_empty`+`head`+`value`+`remove` for the tick | **+0.14%** kdelay-ir, +0.12% ksched-ir |
//! | the insert walk stops at the marker structurally, not via `MAX_VALUE` | **+26.9%** list-ir / **-2.57%** kdelay-ir |
//! | ...the same, copying fields out instead of holding a `&Node` | bit-identical to the above |
//!
//! **Why they all failed, which is one reason.** `&mut self` is `noalias`,
//! so LLVM has ALREADY shared the reads across these small accessors. Every
//! one of those changes tried to remove a read the compiler had removed
//! before it got there, and each added real structure -- a match, an enum
//! payload, a second loop exit -- that it had not.
//!
//! That also explains the three that DID win in the third pass. None of them
//! removed a read. `insert_end` deleted a call to `prev_of`, `link_between`
//! deleted two calls to `set_next`/`set_prev`, and `Iter` deleted a call to
//! `next()`. Each removed a FUNCTION BODY with its own branch and its own
//! `Result`, which is structure the compiler is not free to invent away.
//!
//! **The census led here and was still not enough.** It put 10.4% of
//! kdelay-ir in `core/src/slice/index.rs` -- larger than any line of this
//! file -- and that 10.4% is inlined bounds-check code ATTRIBUTED to that
//! file, not cost anybody can remove: writing the test by hand produced the
//! same object code. A census attributes cost to a line; it does not say the
//! cost is removable. This file's own second pass says that, and it was
//! walked into again anyway.
//!
//! **And the walk result is the one to read twice.** Breaking out of the
//! sorted-insert loop on `is_end(after)` instead of on a `MAX_VALUE` fetched
//! through `next_and_value` is STRICTLY LESS WORK -- it skips a read of
//! `ends[list]` and a `(u16, V)` tuple on the last step of every walk that
//! reaches the end -- and it costs **27%** here while SAVING 2.6% on the
//! kernel. Reproduced bit-for-bit across four runs and two spellings. The
//! per-step instruction sequence is identical in both forms, so what moved
//! is loop shape, not work. It is rejected because a 27% regression on the
//! crate's own instrument disqualifies a library change whatever the kernel
//! thinks -- but the kernel half of that trade is real and is the one thing
//! in four passes that made the delayed list cheaper.
//!
//! ## The second pass: one win, six refutations
//!
//! The `container` sentinel below was the win. The six are recorded because
//! every one of them is the obvious next idea:
//!
//! | tried | core-ir | ksched-ir |
//! |---|---|---|
//! | specialise the walk's first step (the end marker is known) | flat | flat |
//! | count the insert guard DOWN to zero instead of up to `N` | flat | flat |
//! | fuse `head_value` so it skips `head`'s `Option<ItemId>` | flat | flat |
//! | put the ITEM arm first in the four link accessors | flat | flat |
//! | `#[inline(always)]` on the four leaf accessors | flat | flat |
//! | one node access in `remove` instead of two | flat | **+6,709** |
//!
//! The census invites most of these: `if link >= END_BASE` carries 9.58% of
//! `core-ir` and `if guard > N` 2.50%. **Neither is removable cost.** Those
//! are the traversal and the loop, attributed to the first line of an
//! inlined body -- and reordering the branch or making the test free moves
//! nothing, which is how you can tell.
//!
//! What that leaves: LLVM has already done every local optimisation here, so
//! only a REPRESENTATION or an ALGORITHMIC change moves this file. One
//! representation change did. The algorithmic one above moved the list
//! instrument a lot and the kernel the wrong way.
//!
//! ## The narrow key, which was supposed to be the next win and is not
//!
//! `ListsOf<V, N, L>` makes the sort key a parameter -- `TickWidth` offers
//! 16, 32 and 64 bits, and every Kairos target is a 32-bit machine where a
//! 64-bit compare is two instructions. The null arm proves the parameter
//! itself is free: at `V = u64` the count moves 58 instructions on i686 and
//! 0.013% on x86-64.
//!
//! The width is NOT free, and not in the direction anyone predicted:
//!
//! | key | `Node` size | x86-64 | i686 |
//! |---|---|---|---|
//! | `u16` | 8 | -1.41% | -5.21% |
//! | `u64` | 16 | base | base |
//! | `u32` | 12 | **+4.30%** | **+5.65%** |
//!
//! The obvious choice for a 32-bit kernel is the worst of the three. Two
//! mechanisms were proposed and both were refuted by their own predictions:
//! padding `Node<u32>` to 16 bytes read +3.5% on x86-64 and -3.15% on i686,
//! OPPOSITE SIGNS, and a `u16` node forced to 16 bytes came out worse than a
//! `u64` one at the identical stride. The ordering is measured; its
//! mechanism is not known, and `bench/list-ir/run.sh` says so rather than
//! guessing a third time.
//!
//! So a `Bits16` configuration should name `ListsOf<u16, N, L>` and take the
//! 5.2%. A `Bits32` one should stay at the `u64` default until somebody
//! explains the middle row.

use crate::error::{Error, Result};

/// One of the `L` lists, `0..L`.
pub type ListId = u8;

/// One of the `N` items, `0..N`.
pub type ItemId = u16;

/// What a list sorts by: a tick, at the width the configuration uses.
///
/// `TickWidth` offers 16, 32 and 64 bits, and a list built for a 32-bit
/// kernel has no reason to carry a 64-bit key. On a 32-bit machine -- which
/// every Kairos target is -- a `u64` compare is two instructions and a `u64`
/// load is two loads, on every step of the sorted insert walk. Measured on
/// `core-ir` at i686: **-5.12%** for the narrow key alone.
///
/// Sealed by construction: the only implementors are here.
pub trait ListValue: Copy + Ord + Default + core::fmt::Debug {
    /// `portMAX_DELAY` at this width: sorts last, and is the end marker's
    /// own value.
    const MAX: Self;
    /// The value an item carries before it has ever been inserted.
    ///
    /// Spelled out rather than taken from `Default`, because `EMPTY` is a
    /// `const` and `Default::default()` is not callable in one -- and an
    /// item's untouched value is observable through `Lists::value`, so
    /// substituting `MAX` here would be a behaviour change wearing the
    /// clothes of a refactor.
    const ZERO: Self;
}

impl ListValue for u64 {
    const MAX: Self = u64::MAX;
    const ZERO: Self = 0;
}

impl ListValue for u32 {
    const MAX: Self = u32::MAX;
    const ZERO: Self = 0;
}

impl ListValue for u16 {
    const MAX: Self = u16::MAX;
    const ZERO: Self = 0;
}

const NONE: u16 = u16::MAX;
const END_BASE: u16 = 0x8000;

/// `container` when an item is in no list.
///
/// A sentinel rather than `Option<ListId>`, because `ListId` is `u8` and has
/// no niche: the `Option` costs a discriminant byte AND turns every read of
/// the field into a two-step test. `SIZES_FIT` already asserts
/// `L <= u8::MAX`, so `u8::MAX` itself can never name a list and is free to
/// mean "none".
const NO_LIST: ListId = u8::MAX;

#[derive(Clone, Copy)]
struct Node<V: ListValue> {
    prev: u16,
    next: u16,
    value: V,
    container: ListId,
}

impl<V: ListValue> Node<V> {
    const EMPTY: Self = Self {
        prev: NONE,
        next: NONE,
        value: V::ZERO,
        container: NO_LIST,
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

/// `N` list items shared by `L` lists, sorted by a key of width `V`.
pub struct ListsOf<V: ListValue, const N: usize, const L: usize> {
    items: [Node<V>; N],
    ends: [End; L],
}

/// `N` list items shared by `L` lists, keyed by a 64-bit tick.
///
/// The default, and what every existing caller means by `Lists<N, L>`. A
/// configuration whose `TickWidth` is 16 or 32 bits can name
/// [`ListsOf`] with a narrower key instead and pay for the width it uses.
pub type Lists<const N: usize, const L: usize> = ListsOf<u64, N, L>;

impl<V: ListValue, const N: usize, const L: usize> Default for ListsOf<V, N, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: ListValue, const N: usize, const L: usize> ListsOf<V, N, L> {
    /// The end marker's value, `portMAX_DELAY`: an item inserted with this
    /// value goes last, after every other item of the same value.
    pub const MAX_VALUE: V = V::MAX;

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
    #[cfg(target_pointer_width = "32")]
    const fn end_index(link: u16) -> Option<usize> {
        if link >= END_BASE {
            Some(link.wrapping_sub(END_BASE) as usize)
        } else {
            None
        }
    }

    /// Is `link` the end marker of `list`?
    ///
    /// Two spellings, chosen by pointer width, because they are the same
    /// question and they do NOT measure the same. A link handed to one of
    /// these call sites belongs to `list`, so "is it a marker" and "is it
    /// THIS list's marker" cannot disagree -- `is_end` is a compare against
    /// a constant, `== end_of(list)` has to build `END_BASE + list` first.
    ///
    /// The cheap-looking one is cheaper only on 32-bit. Measured on
    /// `bench/sweep.sh`, spelling every site `is_end`:
    ///
    /// | arm | delta |
    /// |---|---|
    /// | list-ir i686 | **-1.78%** |
    /// | ksched-ir | -0.55% |
    /// | kdelay-ir | +0.03% |
    /// | list-ir x86-64 | **+3.81%** |
    ///
    /// Every Kairos target is 32-bit, and the x86-64 arm is a host this
    /// crate does not ship to -- but `rusty_rtos_core` is a general `no_std`
    /// crate, so a 3.81% regression there is somebody's real cost. Each
    /// width gets the form it measures better with.
    ///
    /// The price is a second path. It is one `const fn` with no state, both
    /// arms are exercised by the same tests on whichever host runs them, and
    /// the numbers above are here so the next person can re-take them rather
    /// than re-derive the reasoning.
    #[cfg(target_pointer_width = "32")]
    #[inline]
    const fn is_marker_of(link: u16, _list: ListId) -> bool {
        Self::is_end(link)
    }

    /// See the 32-bit twin.
    #[cfg(not(target_pointer_width = "32"))]
    #[inline]
    const fn is_marker_of(link: u16, list: ListId) -> bool {
        link == Self::end_of(list)
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

    fn item(&self, item: ItemId) -> Result<&Node<V>> {
        self.items
            .get(usize::from(item))
            .ok_or(Error::InvalidArgument)
    }

    fn item_mut(&mut self, item: ItemId) -> Result<&mut Node<V>> {
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
    fn next_and_value(&self, link: u16, list: ListId) -> Result<(u16, V)> {
        // Two ways to reach the marker, gated the same way and for the same
        // reason as [`ListsOf::is_marker_of`] -- and this pair leans the
        // OTHER way, which is why the gate is not a 32-bit favour.
        //
        // The walk never leaves the list it was handed, so `end(list)`
        // names the marker directly, where `end_index` subtracts `END_BASE`
        // and wraps the difference in an `Option<usize>` to name the same
        // struct. Spelling it `end(list)` everywhere measured:
        //
        // | arm | delta |
        // |---|---|
        // | list-ir x86-64 | **-2.03%** |
        // | kdelay-ir | **-1.83%** |
        // | ksched-ir | 0 |
        // | list-ir i686 | **+7.43%** |
        //
        // The parameter is not what costs on i686: an inlined version that
        // passes nothing reads the same +7.43% to the instruction, and is
        // +38.6% on x86-64 besides. It is `end(list)` itself.
        #[cfg(target_pointer_width = "32")]
        {
            let _ = list;
            if let Some(i) = Self::end_index(link) {
                let e = self.ends.get(i).ok_or(Error::InvalidArgument)?;
                return Ok((e.next, Self::MAX_VALUE));
            }
        }
        #[cfg(not(target_pointer_width = "32"))]
        {
            if Self::is_end(link) {
                return Ok((self.end(list)?.next, Self::MAX_VALUE));
            }
        }
        let n = self.item(link)?;
        Ok((n.next, n.value))
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
        value: Option<V>,
    ) -> Result<()> {
        {
            let n = self.item_mut(item)?;
            // The `Busy` check rides the read that is happening anyway, and
            // happens before anything is written — so a refused insert
            // still leaves both lists exactly as it found them.
            if n.container != NO_LIST {
                return Err(Error::Busy);
            }
            if let Some(value) = value {
                n.value = value;
            }
            n.prev = before;
            n.next = after;
            n.container = list;
        }
        // The neighbours are items of THIS list or this list's end marker --
        // never another list's. So whenever one of them is the marker, the
        // struct `set_next`/`set_prev` would look up is the very one the
        // length bump below already has to take.
        //
        // Both sides, because both happen: `before` is the marker when the
        // item sorts to the head, `after` is the marker when it sorts to the
        // tail or carries `portMAX_DELAY` or arrives through `insert_end`
        // with the cursor unmoved, and BOTH are when the list was empty.
        // Two comparisons replace up to two bounds-checked lookups and two
        // marker tests, and the writes ride a lookup that was happening
        // anyway.
        let before_is_end = Self::is_marker_of(before, list);
        let after_is_end = Self::is_marker_of(after, list);
        if !before_is_end {
            self.item_mut(before)?.next = item;
        }
        if !after_is_end {
            self.item_mut(after)?.prev = item;
        }
        let e = self.end_mut(list)?;
        if before_is_end {
            e.next = item;
        }
        if after_is_end {
            e.prev = item;
        }
        e.len = e.len.wrapping_add(1);
        Ok(())
    }

    /// The value an item sorts by (`xItemValue`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn value(&self, item: ItemId) -> Result<V> {
        Ok(self.item(item)?.value)
    }

    /// Set an item's value (`listSET_LIST_ITEM_VALUE`). Allowed while the
    /// item is in a list, exactly as in C — the caller re-inserts to re-sort.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn set_value(&mut self, item: ItemId, value: V) -> Result<()> {
        self.item_mut(item)?.value = value;
        Ok(())
    }

    /// The list `item` is in, if any (`listLIST_ITEM_CONTAINER`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an item outside `0..N`.
    pub fn container(&self, item: ItemId) -> Result<Option<ListId>> {
        // The `Option` stays on the PUBLIC surface -- it is what the C's
        // `listLIST_ITEM_CONTAINER` means and what the kernel matches on.
        // Only the stored form is a sentinel.
        let c = self.item(item)?.container;
        Ok(if c == NO_LIST { None } else { Some(c) })
    }

    /// `vListInsert`: put `item` in `list` sorted ascending by `value`,
    /// after every item with an equal value; [`Lists::MAX_VALUE`] goes last.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad list or item; [`Error::Busy`] if
    /// the item is already in a list (C would corrupt both lists).
    pub fn insert(&mut self, list: ListId, item: ItemId, value: V) -> Result<()> {
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

    fn insert_inner(&mut self, list: ListId, item: ItemId, keep: Option<V>) -> Result<()> {
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
            let mut after = self.next_and_value(end, list)?.0;
            let mut guard = 0usize;
            loop {
                let (following, after_value) = self.next_and_value(after, list)?;
                if after_value > value {
                    break;
                }
                before = after;
                after = following;
                guard = guard.wrapping_add(1);
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
        //
        // ONE end read, not two. When the cursor is the marker -- which it is
        // for a ready list nothing has walked, and again after every lap --
        // the node before the cursor IS the marker's own `prev`, a field of
        // the struct this line already holds. The old shape read
        // `ends[list]` for the cursor and then sent that cursor through a
        // general `prev` helper, which tests it for marker-ness and reads
        // `ends[list]` a second time. That helper had no other caller and is
        // gone with it.
        let (cursor, tail) = {
            let e = self.end(list)?;
            (e.cursor, e.prev)
        };
        let before = if Self::is_marker_of(cursor, list) {
            tail
        } else {
            self.item(cursor)?.prev
        };
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
        if container == NO_LIST {
            return Err(Error::NotActive);
        }
        let list = container;
        // `set_next`/`set_prev` locate the end marker by SUBTRACTING
        // `END_BASE` from the link and indexing `ends` with the difference,
        // through an `Option` and a `Result`. Here the list is already in
        // hand: an item's neighbour is another item or THIS list's marker,
        // never another list's, so `end_mut(list)` reaches the same struct
        // without the arithmetic and without the option.
        //
        // The writes stay exactly where they were. Moving them DOWN into the
        // length bump is a different change and it loses -- +6.18% x86-64,
        // +10.21% i686, measured twice, once on a moved baseline.
        if Self::is_end(prev) {
            self.end_mut(list)?.next = next;
        } else {
            self.item_mut(prev)?.next = next;
        }
        if Self::is_end(next) {
            self.end_mut(list)?.prev = prev;
        } else {
            self.item_mut(next)?.prev = prev;
        }
        {
            let n = self.item_mut(item)?;
            n.container = NO_LIST;
            n.prev = NONE;
            n.next = NONE;
        }
        // The length this answers is the one just decremented, so the
        // trailing re-read of the end marker is gone.
        let e = self.end_mut(list)?;
        if e.cursor == item {
            e.cursor = prev;
        }
        // Wrapping: this is reached only after the item was found in
        // this list and unlinked from it, so the length is at least one.
        e.len = e.len.wrapping_sub(1);
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
    pub fn head_value(&self, list: ListId) -> Result<V> {
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
        if n.container == NO_LIST {
            return Err(Error::NotActive);
        }
        Ok((!Self::is_end(n.next)).then_some(n.next))
    }

    /// The items of `list` from the head, in list order.
    pub fn iter(&self, list: ListId) -> Iter<'_, V, N, L> {
        // ONE end-marker read. `head()` and `len()` are both `end()`, so
        // building the iterator used to look the same list up twice for two
        // fields of the same struct.
        let (at, remaining) = match self.end(list) {
            Ok(e) => (e.next, usize::from(e.len)),
            Err(_) => (NONE, 0),
        };
        Iter {
            lists: self,
            at,
            remaining,
        }
    }
}

/// The items of one list, head first.
///
/// `at` is a RAW LINK, not an `Option<ItemId>`, because the list already
/// carries its own terminator: the end marker. Walking `Option<ItemId>` meant
/// every step went through [`ListsOf::next`], which re-checks that the item is
/// in a list -- a question this walk answered when it started -- and then
/// wraps the answer in a `Result<Option<_>>` for `.ok().flatten()` to
/// immediately unwrap again. The marker does that job with one comparison.
pub struct Iter<'a, V: ListValue, const N: usize, const L: usize> {
    lists: &'a ListsOf<V, N, L>,
    /// The next link to visit; an end marker (or [`NONE`]) stops the walk.
    at: u16,
    /// The anti-cycle guard, NOT the terminator. A list that pointed at
    /// itself would otherwise never reach a marker.
    remaining: usize,
}

impl<V: ListValue, const N: usize, const L: usize> Iterator for Iter<'_, V, N, L> {
    type Item = ItemId;

    fn next(&mut self) -> Option<ItemId> {
        if self.remaining == 0 || self.at >= END_BASE {
            return None;
        }
        let item = self.at;
        // Wrapping: the guard above returned on zero.
        self.remaining = self.remaining.wrapping_sub(1);
        // A link this module wrote out of range is a corrupt list; stop the
        // walk rather than pretend the rest of it is meaningful.
        self.at = match self.lists.item(item) {
            Ok(n) => n.next,
            Err(_) => NONE,
        };
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

    /// A value EQUAL to the tail's goes after it.
    ///
    /// The ascending test above pins ties in the MIDDLE of a list. This pins
    /// a tie against the LAST item, which is the case a tail fast path
    /// decides with a single operator -- `>` instead of `>=` there silently
    /// reverses two tasks that blocked with the same wake time, and no other
    /// test in this file would notice.
    #[test]
    fn a_value_equal_to_the_tail_goes_after_it() {
        let mut l = Lists::<8, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 20).unwrap(); // equal to the tail: after it
        l.insert(0, 4, 21).unwrap(); // above the tail: last
        l.insert(0, 5, 5).unwrap(); // below the head: first
        assert_eq!(order(&l, 0), [5, 1, 2, 3, 4]);
        l.insert(0, 6, Lists::<8, 2>::MAX_VALUE).unwrap();
        l.insert(0, 7, Lists::<8, 2>::MAX_VALUE).unwrap();
        assert_eq!(order(&l, 0), [5, 1, 2, 3, 4, 6, 7]);
    }

    /// The first item of an empty list, at both extremes.
    ///
    /// `portMAX_DELAY` and an ordinary value take different arms of
    /// `insert_inner`, and an empty list is where those arms can disagree
    /// about which one runs.
    #[test]
    fn the_first_item_of_an_empty_list_at_both_extremes() {
        let mut l = Lists::<8, 2>::new();
        l.insert(0, 0, Lists::<8, 2>::MAX_VALUE).unwrap();
        assert_eq!(order(&l, 0), [0]);
        assert_eq!(l.head(0).unwrap(), Some(0));
        assert_eq!(l.head_value(0).unwrap(), Lists::<8, 2>::MAX_VALUE);
        assert_eq!(l.len(0).unwrap(), 1);

        l.insert(1, 1, 7).unwrap();
        assert_eq!(order(&l, 1), [1]);
        assert_eq!(l.head_value(1).unwrap(), 7);
        // Emptied and refilled: the marker must be its own `prev` again.
        l.remove(1).unwrap();
        assert!(l.is_empty(1).unwrap());
        l.insert(1, 2, 3).unwrap();
        l.insert(1, 3, 1).unwrap();
        assert_eq!(order(&l, 1), [3, 2]);
    }

    /// `insert_end` leaves a list UNSORTED, and a later `insert` must still
    /// give `vListInsert`'s answer on it.
    ///
    /// **This is the test this file did not have, and the gap was not
    /// hypothetical.** A tail fast path -- conclude "after everything" from
    /// one comparison against the last item -- measured -18% on `list-ir`
    /// and passed all 81 tests and the conformance differential while being
    /// WRONG here, because a wrong order is still a consistent order and
    /// nothing was looking at a list that had taken both calls.
    ///
    /// The kernel reaches this: a task made ready while the scheduler is
    /// suspended joins the pending-ready list sorted in one path and at the
    /// end in another.
    #[test]
    fn insert_end_leaves_a_list_unsorted_and_insert_still_walks_it() {
        let mut l = Lists::<8, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 30).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3]);

        // `vListInsertEnd` puts item 4 before the cursor, which is still the
        // marker, so it lands at the tail carrying a value of 5. The list is
        // now 10, 20, 30, 5 -- and the tail is no longer the largest.
        l.set_value(4, 5).unwrap();
        l.insert_end(0, 4).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3, 4]);

        // `vListInsert` of 25 walks from the marker and stops at the first
        // node whose NEXT value exceeds 25: after item 2, before item 3. A
        // tail fast path would compare 25 against the tail's 5, conclude
        // "after everything", and put it last. That is the divergence.
        l.insert(0, 5, 25).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 5, 3, 4]);
    }

    /// Moving a LINKED item's value reorders the list under it.
    ///
    /// The kernel always removes before it re-values, but the API does not
    /// require that, so anything that trusts the order must not trust it
    /// across this call.
    #[test]
    fn set_value_on_a_linked_item_leaves_the_list_unsorted() {
        let mut l = Lists::<8, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 30).unwrap();
        // The tail's value drops below the head's, in place: 10, 20, 1.
        l.set_value(3, 1).unwrap();
        // The walk stops after item 1, because item 2's 20 exceeds 15.
        l.insert(0, 4, 15).unwrap();
        assert_eq!(order(&l, 0), [1, 4, 2, 3]);
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
