//! The arena and the lists against a reference model, with libFuzzer
//! choosing the operations.
//!
//! `tests/no_panic.rs` already shows that no sequence PANICS. This checks
//! that no sequence CORRUPTS -- the failure the C `list.c` is known for,
//! where a double insert or a remove of an unlinked item silently breaks
//! the list. After every operation:
//!
//! - every item's container is what the model says it is, i.e. exactly the
//!   inserts and removes that answered `Ok` took effect, and no other;
//! - every list's length equals what an iteration walks, every item walked
//!   names that list as its container, and `remove` answers the model's
//!   remaining count;
//! - an item's value is the one last stored;
//! - every live arena handle resolves to exactly the model's value, and a
//!   removed or invented handle never resolves.

#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;

use libfuzzer_sys::fuzz_target;
use rusty_rtos_core::arena::Arena;
use rusty_rtos_core::handle::{Handle, Task, TaskHandle};
use rusty_rtos_core::list::Lists;

const SLOTS: usize = 16;
const LISTS: usize = 3;

/// The input, a byte at a time; zeros once it runs out.
struct Input<'a>(&'a [u8]);

impl Input<'_> {
    fn byte(&mut self) -> u8 {
        let (b, rest) = self.0.split_first().map_or((0, &[][..]), |(b, r)| (*b, r));
        self.0 = rest;
        b
    }

    fn u64(&mut self) -> u64 {
        let mut w = [0_u8; 8];
        for b in &mut w {
            *b = self.byte();
        }
        u64::from_le_bytes(w)
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input(data);
    let mut lists = Lists::<SLOTS, LISTS>::new();
    let mut arena = Arena::<Task, u64, SLOTS>::new();

    // The model.
    let mut container: HashMap<u16, u8> = HashMap::new();
    let mut values: HashMap<u16, u64> = HashMap::new();
    let mut live: Vec<(TaskHandle, u64)> = Vec::new();
    let mut dead: Vec<TaskHandle> = Vec::new();

    let mut steps = 0;
    while !input.done() && steps < 4_000 {
        steps += 1;
        // Out of range on purpose, some of the time.
        let item = u16::from(input.byte() % (SLOTS as u8 + 3));
        let list = input.byte() % (LISTS as u8 + 2);
        match input.byte() % 10 {
            0 | 1 => {
                let value = input.u64();
                if lists.insert(list, item, value).is_ok() {
                    assert!(
                        !container.contains_key(&item),
                        "insert of an item already in list {:?}",
                        container.get(&item)
                    );
                    container.insert(item, list);
                    values.insert(item, value);
                }
            }
            2 | 3 => {
                if lists.insert_end(list, item).is_ok() {
                    assert!(
                        !container.contains_key(&item),
                        "insert_end of an item already in a list"
                    );
                    container.insert(item, list);
                }
            }
            4 | 5 => match lists.remove(item) {
                Ok(left) => {
                    let was = container
                        .remove(&item)
                        .expect("remove succeeded on an item the model says is in no list");
                    let expected = container.values().filter(|&&l| l == was).count();
                    assert_eq!(
                        left, expected,
                        "remove answered {left} left in list {was}, model says {expected}"
                    );
                }
                Err(_) => assert!(
                    !container.contains_key(&item),
                    "remove refused an item that is in a list"
                ),
            },
            6 => {
                let value = input.u64();
                if lists.set_value(item, value).is_ok() {
                    values.insert(item, value);
                }
            }
            7 => {
                let _ = lists.next_round_robin(list);
            }
            8 => {
                let value = input.u64();
                match arena.insert(value) {
                    Ok(h) => {
                        assert!(
                            live.iter().all(|(l, _)| *l != h),
                            "the arena handed out a live handle twice"
                        );
                        live.push((h, value));
                    }
                    Err(_) => assert_eq!(
                        live.len(),
                        SLOTS,
                        "the arena refused an insert with room left"
                    ),
                }
            }
            _ => {
                let pick = input.byte();
                if !live.is_empty() && pick % 4 != 0 {
                    let (h, v) = live.swap_remove(usize::from(pick) % live.len());
                    assert_eq!(
                        arena.remove(h),
                        Some(v),
                        "remove returned something other than what was stored"
                    );
                    assert!(
                        arena.remove(h).is_none(),
                        "a second remove of the same handle succeeded"
                    );
                    dead.push(h);
                } else {
                    let forged: TaskHandle = Handle::from_raw(input.u64() as u32);
                    if !live.iter().any(|(l, _)| *l == forged) {
                        assert!(
                            arena.resolve(forged).is_err(),
                            "an invented handle resolved"
                        );
                    }
                }
            }
        }

        // The lists against the model.
        for l in 0..LISTS as u8 {
            let len = lists.len(l).unwrap();
            let walked: Vec<u16> = lists.iter(l).collect();
            assert_eq!(
                len,
                walked.len(),
                "list {l}: length {len}, walked {}",
                walked.len()
            );
            assert_eq!(
                len,
                container.values().filter(|&&c| c == l).count(),
                "list {l}: length disagrees with the model"
            );
            for it in walked {
                assert_eq!(
                    lists.container(it).unwrap(),
                    Some(l),
                    "item {it} walked in list {l} names another container"
                );
                assert_eq!(
                    container.get(&it),
                    Some(&l),
                    "item {it} is in list {l}, the model says otherwise"
                );
            }
        }
        for (&it, &v) in &values {
            if let Ok(stored) = lists.value(it) {
                assert_eq!(stored, v, "item {it}: value {stored}, last stored {v}");
            }
        }
        // The arena against the model.
        for (h, v) in &live {
            assert_eq!(
                arena.resolve(*h).ok(),
                Some(v),
                "a live handle did not resolve to its value"
            );
        }
        for h in &dead {
            if !live.iter().any(|(l, _)| l == h) {
                assert!(
                    arena.resolve(*h).is_err(),
                    "a removed handle still resolves"
                );
            }
        }
        assert_eq!(
            arena.len(),
            live.len(),
            "arena length disagrees with the model"
        );
    }
});
