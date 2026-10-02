//! `ListsOf::move_to_end` IS `uxListRemove` then `vListInsertEnd` into the
//! same list -- proved by driving two list sets through the same random
//! script, one with the pair and one with the fused operation, and requiring
//! every list's order, values, length and round-robin cursor to agree after
//! every step. The pair itself is what `tests/list_differential.rs` proves
//! against FreeRTOS's own `list.c`, so this is the second half of the chain.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "a property test: it asserts by panicking"
)]

use rusty_rtos_core::list::{ItemId, ListId, ListsOf};

const LISTS: usize = 4;
const ITEMS: u16 = 24;
type L = ListsOf<u32, 32, LISTS>;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

/// One list as the test compares it: items with values, length, cursor.
type ListState = (Vec<(ItemId, u32)>, usize, ItemId);

fn state(lists: &L) -> Vec<ListState> {
    (0..LISTS as ListId)
        .map(|l| {
            let order = lists
                .iter(l)
                .map(|i| (i, lists.value(i).unwrap()))
                .collect();
            (order, lists.len(l).unwrap(), lists.cursor_of(l).unwrap())
        })
        .collect()
}

#[test]
fn move_to_end_is_remove_then_insert_end() {
    let mut moved = 0_u32;
    let mut refused = 0_u32;
    let mut cursor_on_item = 0_u32;
    let mut bad_list = 0_u32;
    for seed in 1..=32_u32 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9) | 1);
        let (mut pair, mut fused) = (L::new(), L::new());
        for step in 0..5_000 {
            let l = (rng.next() % LISTS as u32) as ListId;
            let i = (rng.next() % u32::from(ITEMS)) as ItemId;
            let arg = rng.next();
            match rng.next() % 6 {
                // A list id that names no list -- `NO_LIST` (u8::MAX), the
                // container of every free item, among them -- is refused
                // with nothing written, whatever the item.
                5 => {
                    let bad = if arg % 2 == 0 { LISTS as ListId } else { u8::MAX };
                    assert!(fused.move_to_end(bad, i).is_err(), "seed {seed} step {step}");
                    bad_list += 1;
                }
                0 if pair.container(i).unwrap().is_none() => {
                    let v = arg % 4;
                    pair.insert(l, i, v).unwrap();
                    fused.insert(l, i, v).unwrap();
                }
                1 if pair.container(i).unwrap().is_none() => {
                    pair.insert_end(l, i).unwrap();
                    fused.insert_end(l, i).unwrap();
                }
                2 if pair.container(i).unwrap().is_some() => {
                    pair.remove(i).unwrap();
                    fused.remove(i).unwrap();
                }
                3 if !pair.is_empty(l).unwrap() => {
                    pair.next_round_robin(l).unwrap();
                    fused.next_round_robin(l).unwrap();
                }
                _ => {
                    // The operation under test, against the pair it fuses.
                    if pair.cursor_of(l).unwrap() == i {
                        cursor_on_item += 1;
                    }
                    let in_list = pair.container(i).unwrap() == Some(l);
                    if in_list {
                        pair.remove(i).unwrap();
                        pair.insert_end(l, i).unwrap();
                        moved += 1;
                    } else {
                        refused += 1;
                    }
                    assert_eq!(
                        fused.move_to_end(l, i).unwrap(),
                        in_list,
                        "seed {seed} step {step}: the answer"
                    );
                }
            }
            assert_eq!(state(&pair), state(&fused), "seed {seed} step {step}");
        }
    }
    assert!(moved > 10_000, "only {moved} moves");
    assert!(refused > 10_000, "only {refused} refusals");
    assert!(bad_list > 10_000, "only {bad_list} bad list ids");
    assert!(
        cursor_on_item > 1_000,
        "only {cursor_on_item} moves with the cursor ON the item"
    );
}
