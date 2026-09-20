//! Instruction counts for the foundation, which nothing measured until now.
//!
//! Two structures carry almost every kernel call, and both are here:
//!
//! * **The intrusive lists.** `insert` is ordered by value, so it WALKS -- it
//!   is the delayed list's wake-time order and the one place in the kernel
//!   with a loop whose length is the number of blocked tasks. `insert_end` is
//!   the ready list's O(1) push. The workload drives both, plus removal from
//!   the middle and the round-robin cursor.
//! * **The generational arena.** Every handle the kernel is given is resolved
//!   through it, and a stale handle must be refused rather than resolved to
//!   whatever now occupies the slot -- so the miss path is exercised as
//!   deliberately as the hit path.
//!
//! The insert values are chosen to hit the walk at every depth: ascending
//! fills it from the far end, descending from the near end, and the shuffled
//! order lands in the middle. A single order would measure one arm of a loop
//! whose cost IS its depth.
//!
//! A deterministic counter, not a clock. The verdict counts are the work
//! parity anchors: a change that moves any of them changed behaviour, and a
//! compiler that removed the work moves the checksum.

use rusty_rtos_core::arena::Arena;
use rusty_rtos_core::handle::{Handle, Task};
use rusty_rtos_core::list::Lists;

/// Enough repetitions that process startup is noise in the total.
const REPS: u32 = 3000;

/// Items and lists, sized as a small kernel configuration would.
const ITEMS: usize = 24;
const LISTS: usize = 8;

/// Arena slots.
const SLOTS: usize = 24;

/// Wake-time orders, so the ordered insert's walk is measured at every depth
/// rather than at whichever one a single order happens to produce.
const ORDERS: &[[u64; 8]] = &[
    [1, 2, 3, 4, 5, 6, 7, 8],
    [8, 7, 6, 5, 4, 3, 2, 1],
    [4, 1, 7, 2, 8, 3, 6, 5],
    [5, 5, 5, 5, 5, 5, 5, 5],
    [1, 8, 2, 7, 3, 6, 4, 5],
];

fn main() {
    let mut inserts = 0u64;
    let mut removes = 0u64;
    let mut resolves = 0u64;
    let mut refused = 0u64;
    let mut checksum = 0u64;

    for _ in 0..REPS {
        // ---- the lists -----------------------------------------------------
        let mut lists: Lists<ITEMS, LISTS> = Lists::new();

        for (o, order) in ORDERS.iter().enumerate() {
            let list = (o % LISTS) as u8;

            // Ordered insert: the walk.
            for (i, value) in order.iter().enumerate() {
                if lists.insert(list, i as u16, *value).is_ok() {
                    inserts = inserts.wrapping_add(1);
                }
            }
            checksum = checksum.wrapping_add(lists.len(list).unwrap_or(0) as u64);
            // The head is the smallest value, which is what the delayed list
            // is read for on every tick.
            checksum = checksum.wrapping_add(lists.head_value(list).unwrap_or(0));

            // The round-robin cursor, which is how the ready list is read.
            for _ in 0..4u32 {
                if let Ok(Some(item)) = lists.next_round_robin(list) {
                    checksum = checksum.wrapping_add(u64::from(item));
                }
            }

            // Removal from the middle, not just the ends: the ends are the
            // cheap cases and a list that only ever loses its head measures
            // the unlink it never has to do.
            for i in [3u16, 0, 7, 5, 1, 6, 2, 4] {
                if lists.remove(i).is_ok() {
                    removes = removes.wrapping_add(1);
                }
            }
            checksum = checksum.wrapping_add(u64::from(lists.is_empty(list).unwrap_or(false)));
        }

        // Insert-at-end, the ready list's push, on its own list so the
        // ordered walk above cannot be confused with it.
        let tail = (LISTS - 1) as u8;
        for i in 0..8u16 {
            if lists.insert_end(tail, i).is_ok() {
                inserts = inserts.wrapping_add(1);
            }
        }
        for i in 0..8u16 {
            if lists.remove(i).is_ok() {
                removes = removes.wrapping_add(1);
            }
        }

        // ---- the arena -----------------------------------------------------
        let mut arena: Arena<Task, u64, SLOTS> = Arena::default();
        let mut live: [Option<Handle<Task>>; 8] = [None; 8];

        for (i, slot) in live.iter_mut().enumerate() {
            *slot = arena.insert(i as u64).ok();
        }
        for slot in &live {
            if let Some(handle) = *slot {
                match arena.resolve(handle) {
                    Ok(value) => {
                        resolves = resolves.wrapping_add(1);
                        checksum = checksum.wrapping_add(*value);
                    }
                    Err(_) => refused = refused.wrapping_add(1),
                }
            }
        }

        // Free every other slot, then resolve the STALE handles. A
        // generational arena must refuse them; one that did not would resolve
        // to whatever now occupies the slot, which is the defect the
        // generation exists to prevent.
        for slot in live.iter().step_by(2) {
            if let Some(handle) = *slot {
                if arena.remove(handle).is_some() {
                    removes = removes.wrapping_add(1);
                }
            }
        }
        // Refill, so the slots are occupied again and a stale handle would
        // find a value rather than a hole.
        for i in 0..4u64 {
            let _ = arena.insert(i.wrapping_add(100));
        }
        for slot in live.iter().step_by(2) {
            if let Some(handle) = *slot {
                match arena.resolve(handle) {
                    Ok(value) => {
                        resolves = resolves.wrapping_add(1);
                        checksum = checksum.wrapping_add(*value);
                    }
                    Err(_) => refused = refused.wrapping_add(1),
                }
            }
        }
    }

    println!("checksum {checksum}");
    println!(
        "reps {REPS} inserts {} removes {} resolves {} refused {}",
        inserts / u64::from(REPS),
        removes / u64::from(REPS),
        resolves / u64::from(REPS),
        refused / u64::from(REPS)
    );
}
