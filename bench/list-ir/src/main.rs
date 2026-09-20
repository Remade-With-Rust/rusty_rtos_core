//! Instruction counts for the intrusive lists alone.
//!
//! `core-ir` counts the lists and the generational arena together. That is the
//! right shape for a gate over the foundation and the wrong shape for deciding
//! whether an edit to `list.rs` paid: about half of that number is the arena,
//! so a real 4% list win arrives as 2% and cannot be told from the ~0.01%
//! codegen wobble a refactor produces. This instrument drops the arena and
//! changes nothing else. `core-ir` stays the gate; this one is the microscope.
//!
//! **What it has to exercise, and why each one is here.**
//!
//! * The **sorted insert walk** at every depth. Its cost IS its depth, so a
//!   single wake-time order measures one arm of the loop. Ascending fills from
//!   the far end, descending from the near end, shuffled lands in the middle,
//!   and all-equal is the FreeRTOS tie rule (`vListInsert` puts a later equal
//!   value AFTER the ones already there).
//! * The **`portMAX_DELAY` fast path**, which does not walk at all. It is a
//!   separate branch of `insert_inner`, and a change can move one branch
//!   without the other -- the generic's first cut read as a 2.29% win that
//!   turned out to be the whole workload taking this branch by accident.
//! * **`insert_end`**, the ready list's O(1) push, which reaches
//!   `link_between` by a different route and never walks.
//! * **Removal from the middle.** A list that only ever loses its head
//!   measures the unlink it does not have to do.
//! * The **round-robin cursor** and **`iter`**, which are how the ready list
//!   and the delayed list are READ. A write-only workload prices half the
//!   structure.
//! * The **refusal paths**: a bad item, an item already in a list, an item in
//!   no list. They are branches inside the hot functions, and a workload that
//!   only ever succeeds leaves them uncounted while still paying for them.
//!
//! A deterministic counter, not a clock. The verdict counts below the checksum
//! are the work-parity anchors -- but note what the generic taught this file:
//! **an anchor proves the ANSWER, not the WORK.** A change that reaches the
//! same list by a cheaper-looking route moves the instruction count and leaves
//! every anchor exactly where it was. Read the count and the anchors together.

use rusty_rtos_core::list::ListsOf;

/// The sort key. `u64` by default, `u32` under `--features narrow`.
///
/// A `TickWidth::Bits32` kernel has no 64-bit tick to sort by, and every
/// Kairos target is a 32-bit machine where the wider key costs two
/// instructions per compare and two loads per read.
#[cfg(not(any(feature = "narrow", feature = "tiny")))]
type Key = u64;
#[cfg(all(feature = "narrow", not(feature = "tiny")))]
type Key = u32;
#[cfg(feature = "tiny")]
type Key = u16;

/// The workload's literals, at whichever width this arm is built for.
///
/// The orders are stored as `u32` so all three arms drive the SAME list with
/// the SAME values -- an arm that used different numbers would not be a price
/// for the width, it would be a different experiment.
#[inline]
#[allow(clippy::cast_possible_truncation)]
const fn key(v: u32) -> Key {
    v as Key
}

/// Items and lists, sized as a small kernel configuration would.
const ITEMS: usize = 24;
const LISTS: usize = 8;

/// Enough repetitions that process startup is noise in the total.
const REPS: u32 = 2_000;

type Lists = ListsOf<Key, ITEMS, LISTS>;

/// Wake-time orders. Stored at `u32` so the same literals feed both key
/// widths -- the workload must be identical across the arms, or the arms are
/// measuring two different things and their difference is not a price.
const ORDERS: &[[u32; 8]] = &[
    [1, 2, 3, 4, 5, 6, 7, 8],
    [8, 7, 6, 5, 4, 3, 2, 1],
    [4, 1, 7, 2, 8, 3, 6, 5],
    [5, 5, 5, 5, 5, 5, 5, 5],
    [1, 8, 2, 7, 3, 6, 4, 5],
];

#[allow(clippy::cast_possible_truncation)]
fn main() {
    let mut inserts = 0u64;
    let mut removes = 0u64;
    let mut walked = 0u64;
    let mut refused = 0u64;
    let mut checksum = 0u64;

    for _ in 0..REPS {
        let mut lists = Lists::new();

        for (o, order) in ORDERS.iter().enumerate() {
            let list = (o % LISTS) as u8;

            for (i, value) in order.iter().enumerate() {
                if lists.insert(list, i as u16, key(*value)).is_ok() {
                    inserts = inserts.wrapping_add(1);
                }
            }
            // An item already in a list must be refused WITHOUT touching
            // either list. The check rides a read that happens anyway, which
            // does not make it free to get wrong.
            if lists.insert(list, 0, key(3)).is_err() {
                refused = refused.wrapping_add(1);
            }
            // And an item outside `0..N`.
            if lists.insert(list, ITEMS as u16, key(3)).is_err() {
                refused = refused.wrapping_add(1);
            }

            checksum = checksum.wrapping_add(lists.len(list).unwrap_or(0) as u64);
            // The head is the smallest value: what the delayed list is read
            // for on every single tick.
            let head = lists.head_value(list).unwrap_or(key(0));
            checksum = checksum.wrapping_add(u64::from(head));

            // The whole-list walk, which is how a scan for expired timers
            // reads it.
            for item in lists.iter(list) {
                checksum = checksum.wrapping_add(u64::from(item));
                walked = walked.wrapping_add(1);
            }

            // The round-robin cursor, which is how the ready list is read.
            for _ in 0..4u32 {
                if let Ok(Some(item)) = lists.next_round_robin(list) {
                    checksum = checksum.wrapping_add(u64::from(item));
                }
            }

            // Re-sort in place: the shape of a task whose wake time moves.
            // `set_value` then `insert_keeping_value` is the path that reads
            // the item's own value rather than being handed one.
            for i in [2u16, 6] {
                if lists.remove(i).is_ok() {
                    removes = removes.wrapping_add(1);
                }
                let _ = lists.set_value(i, key(9));
                if lists.insert_keeping_value(list, i).is_ok() {
                    inserts = inserts.wrapping_add(1);
                }
            }

            // From the middle outward, not from the head.
            for i in [3u16, 0, 7, 5, 1, 6, 2, 4] {
                if lists.remove(i).is_ok() {
                    removes = removes.wrapping_add(1);
                }
            }
            // Removing an item that is in no list.
            if lists.remove(0).is_err() {
                refused = refused.wrapping_add(1);
            }
            checksum = checksum.wrapping_add(u64::from(lists.is_empty(list).unwrap_or(false)));
        }

        // ---- the portMAX_DELAY fast path, which does not walk --------------
        //
        // Four items that sort last, after eight that sort by value. This is
        // the branch a blocked-forever task takes, and it is the branch the
        // generic's first cut sent the whole workload down by accident.
        let deep = 0u8;
        for i in 0..8u16 {
            if lists.insert(deep, i, key(u32::from(i) + 1)).is_ok() {
                inserts = inserts.wrapping_add(1);
            }
        }
        for i in 8..12u16 {
            if lists.insert(deep, i, Lists::MAX_VALUE).is_ok() {
                inserts = inserts.wrapping_add(1);
            }
        }
        let head = lists.head_value(deep).unwrap_or(key(0));
        checksum = checksum.wrapping_add(u64::from(head));
        for i in 0..12u16 {
            if lists.remove(i).is_ok() {
                removes = removes.wrapping_add(1);
            }
        }

        // ---- insert_end, the ready list's push -----------------------------
        let tail = (LISTS - 1) as u8;
        for i in 0..8u16 {
            if lists.insert_end(tail, i).is_ok() {
                inserts = inserts.wrapping_add(1);
            }
        }
        for _ in 0..8u32 {
            if let Ok(Some(item)) = lists.next_round_robin(tail) {
                checksum = checksum.wrapping_add(u64::from(item));
            }
        }
        for i in 0..8u16 {
            if lists.remove(i).is_ok() {
                removes = removes.wrapping_add(1);
            }
        }
    }

    println!("checksum {checksum}");
    println!(
        "reps {REPS} inserts {} removes {} walked {} refused {}",
        inserts / u64::from(REPS),
        removes / u64::from(REPS),
        walked / u64::from(REPS),
        refused / u64::from(REPS)
    );
}
