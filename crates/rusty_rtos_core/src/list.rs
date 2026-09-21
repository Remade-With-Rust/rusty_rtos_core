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

/// `container` when an item is in no list.
///
/// A sentinel rather than `Option<ListId>`, because `ListId` is `u8` and has
/// no niche: the `Option` costs a discriminant byte AND turns every read of
/// the field into a two-step test. `SIZES_FIT` already asserts
/// `L <= u8::MAX`, so `u8::MAX` itself can never name a list and is free to
/// mean "none".
const NO_LIST: ListId = u8::MAX;

/// How many slots a set of lists needs to hold `items` items and `lists`
/// lists, rounded UP TO A POWER OF TWO.
///
/// The rounding is what makes the list fast, and it is worth saying why
/// rather than leaving it as an arbitrary-looking constraint. Every link
/// this module stores is followed with `link & (N - 1)` rather than a
/// bounds-checked index. LLVM can prove `x & (N - 1) < N` when `N` is a
/// power of two, so the check folds away and the panic becomes unreachable
/// rather than suppressed — no `unsafe`, no `get_unchecked`.
///
/// Measured on `bench/list-cost`, that mask is worth **4.15 instructions per
/// list operation** (20.70 -> 16.55). The alternative that needs no rounding
/// — an exact-size array and `% N` — was measured too and is worse than
/// either: **27.37**, because LLVM lowers a constant modulo to a
/// multiply-shift sequence costing more than the branch it replaces.
///
/// The price is the slack. `slots_for(7, 7)` is 16 rather than 14, which is
/// two `Node`s. Quote it as RAM when the trade is being weighed.
#[must_use]
pub const fn slots_for(items: usize, lists: usize) -> usize {
    items.saturating_add(lists).next_power_of_two()
}

/// One node — and an END MARKER IS ONE TOO.
///
/// That is the whole reason this list is fast, and it is what C FreeRTOS
/// does: `xListEnd` is a `ListItem_t` embedded in `List_t`, so `vListInsert`
/// walks `pxNext` without ever asking whether it has arrived. A marker here
/// is a node at the top of the same array carrying [`ListValue::MAX`], so
/// the ordered walk stops on it by comparing values, which it was doing
/// anyway.
#[derive(Clone, Copy)]
struct Node<V: ListValue> {
    /// `xItemValue`. On a marker this is `V::MAX`, which is `portMAX_DELAY`.
    value: V,
    /// `pxPrevious`.
    prev: u16,
    /// `pxNext`.
    next: u16,
    /// `pxContainer`, or [`NO_LIST`]. A marker belongs to no list.
    container: ListId,
}

impl<V: ListValue> Node<V> {
    const EMPTY: Self = Self {
        value: V::ZERO,
        prev: NONE,
        next: NONE,
        container: NO_LIST,
    };
}

/// What a `List_t` keeps beside its `xListEnd`: the round-robin cursor and
/// the count.
///
/// Deliberately NOT in the node array. These two are touched once per
/// operation; `prev`/`next`/`value` are touched once per link followed.
/// Keeping them apart stops the walk pulling a cursor and a length it has no
/// use for through the cache with every step.
#[derive(Clone, Copy)]
struct Meta {
    /// `pxIndex`, the round-robin cursor. Starts at the list's own marker.
    cursor: u16,
    /// `uxNumberOfItems`.
    len: u16,
}

/// `N` slots — items first, then one end marker per list — over `L` lists.
///
/// **`N` is the SLOT count, not the item count.** It must be a power of two
/// and larger than `L`; [`slots_for`] computes it. The item capacity is
/// [`ListsOf::CAPACITY`], which is `N - L`.
pub struct ListsOf<V: ListValue, const N: usize, const L: usize> {
    /// Items in `0..CAPACITY`, then one marker per list.
    nodes: [Node<V>; N],
    /// One per list, indexed by [`ListId`].
    meta: [Meta; L],
}

/// The kernel's lists: `u64` values, which is what a tick count is.
pub type Lists<const N: usize, const L: usize> = ListsOf<u64, N, L>;

impl<V: ListValue, const N: usize, const L: usize> Default for ListsOf<V, N, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: ListValue, const N: usize, const L: usize> ListsOf<V, N, L> {
    /// The value an empty list reports, and the one a marker carries.
    pub const MAX_VALUE: V = V::MAX;

    /// How many of the `N` slots can hold items. The rest hold markers.
    pub const CAPACITY: usize = N.wrapping_sub(L);

    /// `N - 1`. See [`slots_for`] for why this exists.
    const MASK: usize = N.wrapping_sub(1);

    const SIZES_FIT: () = assert!(
        N.is_power_of_two() && L > 0 && L < N && N <= 0x8000 && L <= u8::MAX as usize,
        "N must be a power of two, greater than L, at most 32768; use `slots_for`"
    );

    /// `L` empty lists over `CAPACITY` unlinked items.
    #[must_use]
    // A const fn cannot use `get_mut`, and both loops are bounded by `L`,
    // which `SIZES_FIT` has just asserted is below `N`.
    #[allow(clippy::indexing_slicing)]
    pub const fn new() -> Self {
        let () = Self::SIZES_FIT;
        let mut nodes = [const { Node::EMPTY }; N];
        let mut meta = [Meta {
            cursor: NONE,
            len: 0,
        }; L];
        let mut l = 0;
        while l < L {
            let at = Self::CAPACITY + l;
            let marker = at as u16;
            nodes[at] = Node {
                // `vListInitialise`: `xListEnd.xItemValue = portMAX_DELAY`.
                // This is what makes the ordered walk stop here without
                // anyone testing for it.
                value: V::MAX,
                prev: marker,
                next: marker,
                container: NO_LIST,
            };
            // `pxIndex = &xListEnd`.
            meta[l] = Meta {
                cursor: marker,
                len: 0,
            };
            l = l.wrapping_add(1);
        }
        Self { nodes, meta }
    }

    /// The slot holding `list`'s end marker.
    ///
    /// Only valid once the caller has checked `list` names a list — every
    /// public entry point does that through [`ListsOf::list_meta`] first.
    const fn end_of(list: ListId) -> u16 {
        (Self::CAPACITY + list as usize) as u16
    }

    /// Whether a link names a marker rather than an item.
    ///
    /// Still needed where a marker has to become `None` on the public
    /// surface — `head`, `next`, the iterator. NOT needed to follow a link,
    /// which is the change that pays.
    const fn is_end(link: u16) -> bool {
        link as usize >= Self::CAPACITY
    }

    /// Follow an internal link.
    ///
    /// Every link this module stores is a slot index below `N`, so the mask
    /// changes nothing — and because LLVM can prove `x & (N - 1) < N`, the
    /// bounds check folds away. See [`slots_for`].
    #[inline]
    // The mask is the bound: `x & (N - 1)` cannot reach `N`.
    #[allow(clippy::indexing_slicing)]
    fn at(&self, link: u16) -> &Node<V> {
        &self.nodes[link as usize & Self::MASK]
    }

    /// As [`ListsOf::at`], mutably.
    #[inline]
    #[allow(clippy::indexing_slicing)]
    fn at_mut(&mut self, link: u16) -> &mut Node<V> {
        &mut self.nodes[link as usize & Self::MASK]
    }

    /// A CALLER's item handle, checked once.
    ///
    /// This is the only place an `ItemId` is validated, and everything after
    /// it follows stored links, which are this module's own invariant. A
    /// marker's slot is deliberately out of range here: an `ItemId` naming
    /// one would let a caller unlink a list's own end.
    fn item(&self, item: ItemId) -> Result<&Node<V>> {
        if usize::from(item) >= Self::CAPACITY {
            return Err(Error::InvalidArgument);
        }
        Ok(self.at(item))
    }

    /// As [`ListsOf::item`], mutably.
    fn item_mut(&mut self, item: ItemId) -> Result<&mut Node<V>> {
        if usize::from(item) >= Self::CAPACITY {
            return Err(Error::InvalidArgument);
        }
        Ok(self.at_mut(item))
    }

    /// A caller's list handle, checked once.
    fn list_meta(&self, list: ListId) -> Result<&Meta> {
        self.meta
            .get(usize::from(list))
            .ok_or(Error::InvalidArgument)
    }

    /// As [`ListsOf::list_meta`], mutably.
    fn list_meta_mut(&mut self, list: ListId) -> Result<&mut Meta> {
        self.meta
            .get_mut(usize::from(list))
            .ok_or(Error::InvalidArgument)
    }

    /// `xItemValue`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `item` names no item.
    pub fn value(&self, item: ItemId) -> Result<V> {
        Ok(self.item(item)?.value)
    }

    /// `listSET_LIST_ITEM_VALUE`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `item` names no item.
    pub fn set_value(&mut self, item: ItemId, value: V) -> Result<()> {
        self.item_mut(item)?.value = value;
        Ok(())
    }

    /// `listLIST_ITEM_CONTAINER`: which list holds this item, if any.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `item` names no item.
    pub fn container(&self, item: ItemId) -> Result<Option<ListId>> {
        // The `Option` stays on the PUBLIC surface -- it is what the C's
        // `listLIST_ITEM_CONTAINER` means and what the kernel matches on.
        // Only the stored form is a sentinel.
        let c = self.item(item)?.container;
        Ok(if c == NO_LIST { None } else { Some(c) })
    }

    /// `vListInsert`: ordered by `value`, after every item that equals it.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if either handle names nothing,
    /// [`Error::Busy`] if the item is already in a list.
    pub fn insert(&mut self, list: ListId, item: ItemId, value: V) -> Result<()> {
        self.insert_inner(list, item, Some(value))
    }

    /// `vListInsert` sorting by the value the item already carries.
    ///
    /// # Errors
    ///
    /// As [`ListsOf::insert`].
    pub fn insert_keeping_value(&mut self, list: ListId, item: ItemId) -> Result<()> {
        self.insert_inner(list, item, None)
    }

    fn insert_inner(&mut self, list: ListId, item: ItemId, keep: Option<V>) -> Result<()> {
        // The list is checked HERE, before `end_of` turns it into a slot
        // index. `end_of` does no checking of its own, and a masked index
        // built from a bad list would name some other list's marker.
        let _ = self.list_meta(list)?;
        // `None` means "sort by the value the item already carries", which
        // is what every event-list insert wants; `link_between` then skips
        // the write, because there is nothing to change.
        let value = match keep {
            Some(v) => v,
            None => self.item(item)?.value,
        };
        let end = Self::end_of(list);
        let (before, after) = if value == Self::MAX_VALUE {
            (self.at(end).prev, end)
        } else {
            // Walk while the NEXT node's value is <= ours. The marker
            // carries `MAX_VALUE`, so this stops there at the latest and
            // NOTHING in the loop tests for the end of the list -- the
            // comparison the sort needs is the same one that terminates it.
            let mut before = end;
            let mut after = self.at(end).next;
            // A step counter, IN DEBUG BUILDS ONLY.
            //
            // The walk cannot outrun a marker carrying `MAX_VALUE`, so in a
            // list this module has not corrupted the counter is dead code --
            // and it measured **3.27 instructions per list operation**, which
            // was the whole difference between 23.07 and 19.80. C FreeRTOS
            // has no such counter either: `vListInsert` with a damaged
            // `xListEnd.xItemValue` loops forever, exactly as this would.
            //
            // But "loops forever" is a bad way to learn you have broken the
            // invariant. Poisoning `new()` to give the marker `ZERO` does not
            // fail the suite, it HANGS it -- measured, exit 124 under a
            // timeout. So the counter stays where the tests run and leaves
            // where the kernel ships, which is strictly better than the
            // oracle rather than merely equal to it.
            #[cfg(debug_assertions)]
            let mut guard = 0usize;
            loop {
                let node = self.at(after);
                let (following, after_value) = (node.next, node.value);
                if after_value > value {
                    break;
                }
                before = after;
                after = following;
                #[cfg(debug_assertions)]
                {
                    guard = guard.wrapping_add(1);
                    if guard > N {
                        return Err(Error::InvalidArgument);
                    }
                }
            }
            (before, after)
        };
        self.link_between(list, item, before, after, keep)
    }

    /// `vListInsertEnd`: in front of the round-robin cursor.
    ///
    /// # Errors
    ///
    /// As [`ListsOf::insert`].
    pub fn insert_end(&mut self, list: ListId, item: ItemId) -> Result<()> {
        // ONE read of the cursor, and the node before it comes straight from
        // the cursor's own `prev` -- which is now a plain node read whether
        // the cursor is an item or the marker, because both are nodes.
        let cursor = self.list_meta(list)?.cursor;
        let before = self.at(cursor).prev;
        self.link_between(list, item, before, cursor, None)
    }

    /// Link `item` between two slots and bump the length.
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
        // UNCONDITIONAL, both of them. A neighbour is an item or this list's
        // marker and there is no longer any difference between those: both
        // are nodes in one array. The four marker tests this used to make,
        // and the two bounds-checked lookups behind them, are gone.
        self.at_mut(before).next = item;
        self.at_mut(after).prev = item;
        let m = self.list_meta_mut(list)?;
        m.len = m.len.wrapping_add(1);
        Ok(())
    }

    /// `uxListRemove`: unlink an item and answer how many are left.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `item` names no item,
    /// [`Error::NotActive`] if it is in no list.
    pub fn remove(&mut self, item: ItemId) -> Result<usize> {
        // One read takes all three fields, and checks the handle once.
        let (prev, next, container) = {
            let n = self.item(item)?;
            (n.prev, n.next, n.container)
        };
        if container == NO_LIST {
            return Err(Error::NotActive);
        }
        // Both unconditional, for the same reason as `link_between`.
        self.at_mut(next).prev = prev;
        self.at_mut(prev).next = next;
        {
            // `item` was validated above, so this needs no second check.
            //
            // ONLY `container`, which is what `uxListRemove` clears too. The
            // two `NONE` writes that used to be here were hygiene, not
            // safety: nothing can follow a removed node's links, because
            // every path to them goes through a `container != NO_LIST` test
            // first. They cost 2 stores per removal and the C makes neither.
            self.at_mut(item).container = NO_LIST;
        }
        // `container` came out of a linked node, so it names a real list.
        let m = self.list_meta_mut(container)?;
        if m.cursor == item {
            m.cursor = prev;
        }
        // Wrapping: this is reached only after the item was found in this
        // list and unlinked from it, so the length is at least one.
        m.len = m.len.wrapping_sub(1);
        Ok(usize::from(m.len))
    }

    /// `listLIST_IS_EMPTY`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn is_empty(&self, list: ListId) -> Result<bool> {
        Ok(self.list_meta(list)?.len == 0)
    }

    /// `listCURRENT_LIST_LENGTH`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn len(&self, list: ListId) -> Result<usize> {
        Ok(usize::from(self.list_meta(list)?.len))
    }

    /// The first item, or `None` when the list is empty.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn head(&self, list: ListId) -> Result<Option<ItemId>> {
        let len = self.list_meta(list)?.len;
        let first = self.at(Self::end_of(list)).next;
        Ok((len > 0 && !Self::is_end(first)).then_some(first))
    }

    /// The first item's value, or [`ListsOf::MAX_VALUE`] when empty.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn head_value(&self, list: ListId) -> Result<V> {
        // No empty-list branch: when the list is empty the marker's own
        // `next` is the marker, and a marker carries `MAX_VALUE`. The answer
        // falls out of the data instead of being special-cased.
        let _ = self.list_meta(list)?;
        let first = self.at(Self::end_of(list)).next;
        Ok(self.at(first).value)
    }

    /// The last item's value, or [`ListsOf::MAX_VALUE`] when empty.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn tail_value(&self, list: ListId) -> Result<V> {
        let _ = self.list_meta(list)?;
        let last = self.at(Self::end_of(list)).prev;
        Ok(self.at(last).value)
    }

    /// Whether the list is in ascending value order.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list, or if the walk
    /// runs longer than the arena can hold.
    pub fn is_sorted(&self, list: ListId) -> Result<bool> {
        let _ = self.list_meta(list)?;
        let mut node = self.at(Self::end_of(list)).next;
        let mut previous: Option<V> = None;
        let mut guard = 0usize;
        while !Self::is_end(node) {
            let n = self.at(node);
            let (following, value) = (n.next, n.value);
            if previous.is_some_and(|p| value < p) {
                return Ok(false);
            }
            previous = Some(value);
            node = following;
            guard = guard.wrapping_add(1);
            if guard > N {
                return Err(Error::InvalidArgument);
            }
        }
        Ok(true)
    }

    /// `vListInsert`, taking the append shortcut when the value belongs at
    /// the end — which, for a delayed list fed a rising tick, it usually
    /// does.
    ///
    /// # Errors
    ///
    /// As [`ListsOf::insert`].
    pub fn insert_sorted(&mut self, list: ListId, item: ItemId, value: V) -> Result<()> {
        let _ = self.list_meta(list)?;
        let end = Self::end_of(list);
        let tail = self.at(end).prev;
        // An empty list answers `MAX_VALUE` here without a branch, because
        // `tail` is then the marker and a marker carries `MAX_VALUE`.
        let tail_value = self.at(tail).value;
        // `>=`, not `>`: `vListInsert` puts a later equal value AFTER the
        // ones already there, which is exactly where appending puts it.
        if value >= tail_value {
            // A TRUE append: linked between the tail and the end marker.
            //
            // NOT `insert_end`, which links before the CURSOR and so appends
            // only while the cursor is still sitting at the marker.
            return self.link_between(list, item, tail, end, Some(value));
        }
        self.insert(list, item, value)
    }

    /// `pxIndex`: where the round robin is sitting.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn cursor_of(&self, list: ListId) -> Result<ItemId> {
        Ok(self.list_meta(list)?.cursor)
    }

    /// `listGET_OWNER_OF_NEXT_ENTRY`: advance the round robin and answer
    /// whose turn it now is.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `list` names no list.
    pub fn next_round_robin(&mut self, list: ListId) -> Result<Option<ItemId>> {
        let (cursor, len) = {
            let m = self.list_meta(list)?;
            (m.cursor, m.len)
        };
        if len == 0 {
            return Ok(None);
        }
        let end = Self::end_of(list);
        // No "is the cursor the marker" branch: the marker's `next` IS the
        // first item, so one read serves both cases.
        let mut next = self.at(cursor).next;
        if next == end {
            next = self.at(next).next;
        }
        self.list_meta_mut(list)?.cursor = next;
        Ok((!Self::is_end(next)).then_some(next))
    }

    /// The item after this one, or `None` at the end of the list.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `item` names no item,
    /// [`Error::NotActive`] if it is in no list.
    pub fn next(&self, item: ItemId) -> Result<Option<ItemId>> {
        let n = self.item(item)?;
        if n.container == NO_LIST {
            return Err(Error::NotActive);
        }
        Ok((!Self::is_end(n.next)).then_some(n.next))
    }

    /// Every item in the list, in order.
    pub fn iter(&self, list: ListId) -> Iter<'_, V, N, L> {
        let (at, remaining) = match self.list_meta(list) {
            Ok(m) => (self.at(Self::end_of(list)).next, usize::from(m.len)),
            Err(_) => (NONE, 0),
        };
        Iter {
            lists: self,
            at,
            remaining,
        }
    }
}

/// The iterator [`ListsOf::iter`] returns.
pub struct Iter<'a, V: ListValue, const N: usize, const L: usize> {
    lists: &'a ListsOf<V, N, L>,
    at: u16,
    remaining: usize,
}

impl<V: ListValue, const N: usize, const L: usize> Iterator for Iter<'_, V, N, L> {
    type Item = ItemId;

    fn next(&mut self) -> Option<ItemId> {
        if self.remaining == 0 || ListsOf::<V, N, L>::is_end(self.at) {
            return None;
        }
        let item = self.at;
        // Wrapping: the guard above returned on zero.
        self.remaining = self.remaining.wrapping_sub(1);
        self.at = self.lists.at(item).next;
        Some(item)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;

    fn order(l: &Lists<16, 2>, list: ListId) -> Vec<ItemId> {
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
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<16, 2>::new();
        assert!(l.is_empty(0).unwrap());
        assert_eq!(l.head_value(0).unwrap(), Lists::<16, 2>::MAX_VALUE);
        l.insert(0, 3, 30).unwrap();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 4, 20).unwrap(); // equal value: after item 2
        l.insert(0, 5, Lists::<16, 2>::MAX_VALUE).unwrap();
        l.insert(0, 6, Lists::<16, 2>::MAX_VALUE).unwrap(); // MAX after MAX
        assert_eq!(order(&l, 0), [1, 2, 4, 3, 5, 6]);
        assert_eq!(l.head(0).unwrap(), Some(1));
        assert_eq!(l.head_value(0).unwrap(), 10);
        assert_eq!(l.len(0).unwrap(), 6);
        assert_eq!(l.container(4).unwrap(), Some(0));
        assert_eq!(l.container(7).unwrap(), None);
    }

    #[test]
    fn insert_end_and_the_round_robin_cursor() {
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<16, 2>::new();
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
        type L8 = Lists<8, 1>;
        // The first id PAST the items, written from the constant rather
        // than as a literal. A literal is exactly what went stale when N
        // changed from the item count to the slot count: the 4 that had
        // been out of range silently became a valid item.
        const PAST_END: ItemId = L8::CAPACITY as ItemId;
        let mut l = L8::new();
        assert_eq!(l.insert(1, 0, 0), Err(Error::InvalidArgument));
        assert_eq!(l.insert(0, PAST_END, 0), Err(Error::InvalidArgument));
        assert_eq!(l.remove(PAST_END + 2), Err(Error::InvalidArgument));
        assert_eq!(l.head(3), Err(Error::InvalidArgument));
        assert_eq!(l.next(0), Err(Error::NotActive));
        assert_eq!(l.set_value(PAST_END, 1), Err(Error::InvalidArgument));
    }

    /// The end marker's `MAX_VALUE` is what makes the ordered walk finite,
    /// so it is pinned rather than argued.
    ///
    /// `insert_inner` used to carry a step counter that returned
    /// `InvalidArgument` if the walk ran longer than the arena. It was dead
    /// code — the walk cannot outrun a marker carrying the largest value
    /// there is — and it cost **3.27 instructions per list operation**,
    /// measured, which was the difference between 23.07 and 19.80. It is
    /// gone, and these three properties are why that is safe:
    ///
    /// 1. a marker's value cannot be written, because every path to a
    ///    `value` write goes through `item_mut`, which refuses any id at or
    ///    above `CAPACITY`;
    /// 2. the walk only runs for values strictly below `MAX_VALUE`, because
    ///    `MAX_VALUE` itself takes the append branch above it;
    /// 3. so `after_value > value` is true at the marker at the latest, and
    ///    the loop breaks within `len + 1` steps.
    ///
    /// Break any of the three and this test fails. That is the whole point
    /// of it: the guard was removed on the strength of an invariant, so the
    /// invariant is now the thing under test.
    #[test]
    fn the_marker_is_what_terminates_the_ordered_walk() {
        type L = Lists<16, 2>;
        const MARKER: ItemId = L::CAPACITY as ItemId;

        // (1) No caller can reach a marker's value.
        let mut l = L::new();
        assert_eq!(l.set_value(MARKER, 0), Err(Error::InvalidArgument));
        assert_eq!(l.set_value(MARKER + 1, 0), Err(Error::InvalidArgument));
        assert_eq!(l.value(MARKER), Err(Error::InvalidArgument));
        // Nor unlink one, which would leave a list with no terminator.
        assert_eq!(l.remove(MARKER), Err(Error::InvalidArgument));

        // (2) MAX_VALUE never enters the walk; it appends.
        l.insert(0, 0, 5).unwrap();
        l.insert(0, 1, L::MAX_VALUE).unwrap();
        l.insert(0, 2, 7).unwrap();
        assert_eq!(order(&l, 0), [0, 2, 1], "MAX_VALUE must sort to the end");

        // (3) A FULL list still terminates, walked from both ends. If the
        // marker ever stopped breaking the loop this would hang rather than
        // fail, so it is the case worth having.
        let mut full = L::new();
        for item in 0..MARKER {
            // Descending, so every insert walks the whole list to the front
            // — the longest walk the structure can produce.
            full.insert(0, item, u64::from(MARKER - item)).unwrap();
        }
        assert_eq!(full.len(0).unwrap(), usize::from(MARKER));
        assert!(full.is_sorted(0).unwrap());
        assert_eq!(full.head_value(0).unwrap(), 1);
        assert_eq!(full.tail_value(0).unwrap(), u64::from(MARKER));
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
        let mut l = Lists::<16, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 20).unwrap(); // equal to the tail: after it
        l.insert(0, 4, 21).unwrap(); // above the tail: last
        l.insert(0, 5, 5).unwrap(); // below the head: first
        assert_eq!(order(&l, 0), [5, 1, 2, 3, 4]);
        l.insert(0, 6, Lists::<16, 2>::MAX_VALUE).unwrap();
        l.insert(0, 7, Lists::<16, 2>::MAX_VALUE).unwrap();
        assert_eq!(order(&l, 0), [5, 1, 2, 3, 4, 6, 7]);
    }

    /// The first item of an empty list, at both extremes.
    ///
    /// `portMAX_DELAY` and an ordinary value take different arms of
    /// `insert_inner`, and an empty list is where those arms can disagree
    /// about which one runs.
    #[test]
    fn the_first_item_of_an_empty_list_at_both_extremes() {
        let mut l = Lists::<16, 2>::new();
        l.insert(0, 0, Lists::<16, 2>::MAX_VALUE).unwrap();
        assert_eq!(order(&l, 0), [0]);
        assert_eq!(l.head(0).unwrap(), Some(0));
        assert_eq!(l.head_value(0).unwrap(), Lists::<16, 2>::MAX_VALUE);
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
        let mut l = Lists::<16, 2>::new();
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

    /// `insert_sorted` appends when the value belongs at the end and walks
    /// when it does not, and the list is sorted either way.
    #[test]
    fn insert_sorted_appends_or_walks_and_stays_sorted() {
        let mut l = Lists::<16, 2>::new();
        l.insert_sorted(0, 1, 10).unwrap();
        l.insert_sorted(0, 2, 20).unwrap();
        // Belongs at the end: the append path.
        l.insert_sorted(0, 3, 30).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3]);
        // Belongs in the middle: the walk.
        l.insert_sorted(0, 4, 15).unwrap();
        assert_eq!(order(&l, 0), [1, 4, 2, 3]);
        assert!(l.is_sorted(0).unwrap());
        // And the value the append wrote is the value the item carries --
        // `insert_end` keeps the item's own, so a missed `set_value` here
        // would leave a zero at the tail and only show up as a wake order.
        assert_eq!(l.value(3).unwrap(), 30);
    }

    /// The tie rule, which is the promise that is silent when broken.
    ///
    /// `vListInsert` puts a later EQUAL value after the ones already there.
    /// A `>` instead of a `>=` in the append test sends equals down the
    /// walk, where they land before their equals -- a different wake order
    /// for tasks that share a tick, and nothing else to see.
    #[test]
    fn insert_sorted_puts_a_later_equal_value_after_its_equals() {
        let mut l = Lists::<16, 2>::new();
        l.insert_sorted(0, 1, 10).unwrap();
        l.insert_sorted(0, 2, 20).unwrap();
        l.insert_sorted(0, 3, 20).unwrap();
        l.insert_sorted(0, 4, 20).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3, 4]);

        // The same three through the plain walk, which is the behaviour the
        // append has to agree with.
        let mut w = Lists::<16, 2>::new();
        w.insert(0, 1, 10).unwrap();
        w.insert(0, 2, 20).unwrap();
        w.insert(0, 3, 20).unwrap();
        w.insert(0, 4, 20).unwrap();
        assert_eq!(order(&w, 0), order(&l, 0));
    }

    /// A moved cursor cannot move where `insert_sorted` appends.
    ///
    /// This is the guard on the mechanism, not on a caller. `insert_end`
    /// links before the CURSOR, so an append built on it lands at the tail
    /// only while the cursor is still at the marker -- which used to be a
    /// fact the CALLER had to keep in mind, and the one most likely to be
    /// forgotten, because nothing in the name says "before the cursor".
    /// `insert_sorted` links between the tail and the marker instead, so
    /// there is nothing left to remember.
    ///
    /// If anyone ever rewrites that append back onto `insert_end`, this is
    /// the test that fails.
    #[test]
    fn insert_sorted_appends_at_the_tail_even_after_the_cursor_has_moved() {
        let mut l = Lists::<16, 2>::new();
        l.insert_sorted(0, 1, 10).unwrap();
        l.insert_sorted(0, 2, 20).unwrap();
        l.insert_sorted(0, 3, 30).unwrap();

        // One round-robin step: the cursor leaves the marker and sits on
        // item 1, so `insert_end` would now insert BEFORE item 1 -- at the
        // head of a list whose values are all smaller.
        assert_eq!(l.next_round_robin(0).unwrap(), Some(1));
        l.insert_sorted(0, 4, 40).unwrap();

        // Built on `insert_end` this would read [4, 1, 2, 3]: item 4 linked
        // before the cursor, at the head of a list whose values are all
        // smaller. A true append puts it where its value belongs.
        assert_eq!(order(&l, 0), [1, 2, 3, 4]);
        assert!(l.is_sorted(0).unwrap());
    }

    /// The one promise `insert_sorted` still asks a caller to keep, and
    /// what it costs when it is not kept.
    ///
    /// Pinned as a HAZARD, not as desired behaviour: an unsorted list has
    /// no meaningful tail, so the append puts the value after a smaller
    /// one and the list stays wrong. `is_sorted` is how a caller finds out
    /// before trusting it -- which is the whole of what the list can offer
    /// without the `sorted` flag that `tail_value` records as a net loss.
    #[test]
    fn insert_sorted_on_an_unsorted_list_misplaces_and_is_sorted_says_so() {
        let mut l = Lists::<16, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 30).unwrap();

        // `insert_end` with a small value leaves 10, 20, 30, 5.
        l.set_value(4, 5).unwrap();
        l.insert_end(0, 4).unwrap();
        assert!(!l.is_sorted(0).unwrap(), "the list is unsorted now");

        // 25 against a tail of 5 reads as "after everything", so it is
        // appended -- where the walk would have put it between 20 and 30.
        l.insert_sorted(0, 5, 25).unwrap();
        assert_eq!(order(&l, 0), [1, 2, 3, 4, 5]);
        assert_eq!(order(&l, 0).len(), 5);
        assert!(!l.is_sorted(0).unwrap());
    }

    /// `is_sorted` on the lists the kernel actually keeps sorted.
    #[test]
    fn is_sorted_answers_for_empty_single_and_equal_lists() {
        let mut l = Lists::<16, 2>::new();
        assert!(l.is_sorted(0).unwrap(), "an empty list is sorted");
        l.insert_sorted(0, 1, 7).unwrap();
        assert!(l.is_sorted(0).unwrap(), "one item is sorted");
        l.insert_sorted(0, 2, 7).unwrap();
        assert!(l.is_sorted(0).unwrap(), "equal values are non-decreasing");
        assert!(l.is_sorted(1).unwrap(), "an untouched list is sorted");
        assert!(l.is_sorted(2).is_err(), "a list outside 0..L is an error");
    }

    /// Moving a LINKED item's value reorders the list under it.
    ///
    /// The kernel always removes before it re-values, but the API does not
    /// require that, so anything that trusts the order must not trust it
    /// across this call.
    #[test]
    fn set_value_on_a_linked_item_leaves_the_list_unsorted() {
        let mut l = Lists::<16, 2>::new();
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
        let mut l = Lists::<8, 1>::new();
        l.insert(0, 0, 1).unwrap();
        l.insert(0, 1, 2).unwrap();
        l.remove(0).unwrap();
        l.set_value(0, 9).unwrap();
        l.insert(0, 0, l.value(0).unwrap()).unwrap();
        assert_eq!(l.iter(0).collect::<Vec<_>>(), [1, 0]);
    }

    /// `is_sorted` walks under the same corruption guard `insert` does, and
    /// the guard must not fire on a list that is merely FULL.
    ///
    /// `cargo mutants` found this: `guard > N` survived being mutated to
    /// `>=` and to `==`, because no test built a list long enough to reach
    /// the guard at all, so every version of it behaved the same. A guard
    /// nothing ever approaches is a guard nothing is testing.
    #[test]
    fn is_sorted_does_not_false_alarm_on_a_completely_full_list() {
        // Eight items is N for this geometry: the walk touches every one.
        let mut l = Lists::<16, 2>::new();
        for i in 0..8_u16 {
            l.insert_sorted(0, i, u64::from(i) * 10).unwrap();
        }
        assert_eq!(order(&l, 0).len(), 8, "every item is linked");
        assert!(
            l.is_sorted(0).unwrap(),
            "a full list is sorted, not corrupt -- the guard must not trip              on the last item"
        );
    }

    /// `tail_value` is `MAX_VALUE` on an empty list and the last item's
    /// value otherwise. The empty answer is the one that matters: it is
    /// what makes `insert_sorted` take the walk rather than appending into
    /// nothing, and it is NOT the type's default.
    #[test]
    fn tail_value_is_max_when_empty_and_the_last_value_when_not() {
        let mut l = Lists::<16, 2>::new();
        assert_eq!(
            l.tail_value(0),
            Ok(Lists::<16, 2>::MAX_VALUE),
            "empty answers MAX, which is the end marker's own value"
        );
        assert_ne!(
            Lists::<16, 2>::MAX_VALUE,
            u64::default(),
            "and MAX is not the default, or the test above proves nothing"
        );

        l.insert_sorted(0, 1, 10).unwrap();
        l.insert_sorted(0, 2, 20).unwrap();
        assert_eq!(l.tail_value(0), Ok(20), "the LAST value, not the first");
    }

    /// `is_empty` tracks the list it is asked about and not some other one.
    #[test]
    fn is_empty_follows_inserts_and_removes_on_that_list_alone() {
        let mut l = Lists::<16, 2>::new();
        assert_eq!(l.is_empty(0), Ok(true));
        assert_eq!(l.is_empty(1), Ok(true));

        l.insert_sorted(0, 1, 10).unwrap();
        assert_eq!(l.is_empty(0), Ok(false));
        assert_eq!(l.is_empty(1), Ok(true), "list 1 is untouched");

        l.remove(1).unwrap();
        assert_eq!(l.is_empty(0), Ok(true), "and empty again after the remove");
    }

    /// `cursor_of` is the diagnostic that answers "did the round robin
    /// move", so it has to report the marker before any lap and the item
    /// it landed on afterwards.
    #[test]
    fn cursor_of_reports_the_marker_then_the_item_it_lands_on() {
        let mut l = Lists::<16, 2>::new();
        l.insert_sorted(0, 1, 10).unwrap();
        l.insert_sorted(0, 2, 20).unwrap();

        let start = l.cursor_of(0).unwrap();
        assert_eq!(l.next_round_robin(0).unwrap(), Some(1));
        let after = l.cursor_of(0).unwrap();
        assert_ne!(after, start, "one lap moved the cursor off the marker");
        assert_eq!(after, 1, "and onto the item the lap returned");
    }

    /// The longest walk `insert` can be made to take, which is a full list
    /// entered in ascending order.
    ///
    /// # Why this does NOT kill `insert_inner`'s guard, and `is_sorted`'s
    /// test does kill its own
    ///
    /// Both carry `if guard > N { return Err(..) }`. `cargo mutants`
    /// replaces the `>` with `>=` and with `==`; those die in `is_sorted`
    /// and survive here, and the difference is how far each walk goes.
    ///
    /// `is_sorted` visits every linked item, so on a FULL list its guard
    /// reaches exactly `N` -- which is the one value `>` and `>=` disagree
    /// about, and why `is_sorted_does_not_false_alarm_on_a_completely_full_list`
    /// kills them.
    ///
    /// `insert_inner` stops at the node BEFORE the marker, so its guard
    /// reaches the list's length as it was BEFORE the insert: at most
    /// `N - 1`, because the `N`th item is the one being inserted. `N - 1`
    /// is below both thresholds, so no constructible input tells the two
    /// apart. **They are equivalent mutants**, and the only thing that
    /// could distinguish them is a cyclic list, which `link_between` does
    /// not let a caller build.
    ///
    /// Written down because the first version of this test claimed to kill
    /// them and did not even take the walk: it inserted DESCENDING values,
    /// which land at the head, so the loop broke at the first node and the
    /// guard never moved at all.
    #[test]
    fn insert_takes_its_longest_walk_on_a_full_list_entered_in_order() {
        let mut l = Lists::<16, 2>::new();
        // ASCENDING: each new value belongs after everything already
        // linked, so `insert` walks the whole list before placing it.
        for i in 0..8_u16 {
            l.insert(0, i, u64::from(i).wrapping_add(1).wrapping_mul(10))
                .expect("a full list is not a corrupt one");
        }
        assert_eq!(order(&l, 0), [0, 1, 2, 3, 4, 5, 6, 7]);
        assert!(l.is_sorted(0).unwrap());
    }

    /// `insert_keeping_value` sorts by the value the item ALREADY carries,
    /// which is what every event-list insert wants. Nothing tested it, so
    /// replacing its whole body with `Ok(())` -- linking nothing at all --
    /// went unnoticed.
    #[test]
    fn insert_keeping_value_links_the_item_and_sorts_by_what_it_holds() {
        let mut l = Lists::<16, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 30).unwrap();

        // Give item 3 its value first, then link it WITHOUT passing one.
        l.set_value(3, 20).unwrap();
        l.insert_keeping_value(0, 3).unwrap();

        assert_eq!(
            order(&l, 0),
            [1, 3, 2],
            "it linked the item, and sorted it by the 20 it was carrying"
        );
        assert_eq!(l.value(3).unwrap(), 20, "and did not overwrite that value");
    }

    /// `next` walks one link and stops AT the end marker rather than
    /// returning it -- the `!` in `(!is_end(next)).then_some(next)`. With
    /// the `!` deleted the answers invert: `None` in the middle of a list
    /// and `Some(marker)` at its tail, so a walker would both stop early
    /// and then run off the end.
    #[test]
    fn next_yields_the_successor_and_stops_at_the_tail() {
        let mut l = Lists::<16, 2>::new();
        l.insert(0, 1, 10).unwrap();
        l.insert(0, 2, 20).unwrap();
        l.insert(0, 3, 30).unwrap();

        assert_eq!(l.next(1), Ok(Some(2)), "the middle of a list has a next");
        assert_eq!(l.next(2), Ok(Some(3)));
        assert_eq!(
            l.next(3),
            Ok(None),
            "and the LAST item has none -- not the marker"
        );
    }
}
