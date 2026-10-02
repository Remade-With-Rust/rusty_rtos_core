//! The list differential (hardening gate H-29): `ListsOf` against FreeRTOS's
//! own `list.c`, step for step.
//!
//! `oracle/list/driver.c` builds the pinned FreeRTOS-Kernel V11.3.1 `list.c`,
//! unmodified, and drives it through a 50,000-step random script of
//! `vListInsert`, `vListInsertEnd`, `uxListRemove` and
//! `listGET_OWNER_OF_NEXT_ENTRY`. After each step it prints the touched list:
//! its items in order with their values, its length, and where its
//! round-robin index points. `oracle/list/list.trace` is that output,
//! committed.
//!
//! This test runs the SAME script, from the same xorshift, and must print the
//! same lines. The list is the data structure every ready list, delayed list
//! and event list in the kernel is built from, so an ordering or cursor
//! difference here is a scheduling difference everywhere.
//!
//! Regenerate the trace (WSL, the oracle fetched by `kairos oracle fetch`):
//! `cd oracle/list && sh run.sh`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a differential test: it asserts by panicking, and indexes small tables"
)]

use core::fmt::Write as _;

use rusty_rtos_core::list::{ItemId, ListId, ListValue, ListsOf};

const TRACE: &str = include_str!("../../../oracle/list/list.trace");

const STEPS: u32 = 50_000;
const LISTS: usize = 4;
const ITEMS: usize = 24;

/// Capacity 32 (a power of two, as `ListsOf` requires); the script uses 24.
type L = ListsOf<u32, 32, LISTS>;

/// `driver.c`'s xorshift, bit for bit.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

fn print_list(out: &mut String, lists: &L, step: u32, op: &str, r: i64, l: ListId) {
    let len = lists.len(l).unwrap();
    let _ = write!(out, "{step} {op} r={r} L{l} len={len} [");
    for item in lists.iter(l) {
        let _ = write!(out, " {item}:{}", lists.value(item).unwrap());
    }
    let cursor = lists.cursor_of(l).unwrap();
    if usize::from(cursor) >= ITEMS {
        let _ = writeln!(out, " ] idx=end");
    } else {
        let _ = writeln!(out, " ] idx={cursor}");
    }
}

fn run() -> String {
    let mut rng = Rng(0x9e37_79b9);
    let mut lists = L::new();
    let mut out = String::new();
    for step in 1..=STEPS {
        let kind = rng.next() % 100;
        let l = (rng.next() % LISTS as u32) as ListId;
        let i = (rng.next() % ITEMS as u32) as ItemId;
        let arg = rng.next();
        let owner = lists.container(i).unwrap();

        if kind < 35 {
            if owner.is_some() {
                continue;
            }
            let v = if arg % 16 == 0 { u32::MAX } else { arg % 6 };
            lists.insert(l, i, v).unwrap();
            print_list(&mut out, &lists, step, &format!("insert {i} {v}"), 0, l);
        } else if kind < 60 {
            if owner.is_some() {
                continue;
            }
            lists.insert_end(l, i).unwrap();
            print_list(&mut out, &lists, step, &format!("insert_end {i}"), 0, l);
        } else if kind < 85 {
            let Some(from) = owner else {
                continue;
            };
            let r = lists.remove(i).unwrap() as i64;
            print_list(&mut out, &lists, step, &format!("remove {i}"), r, from);
        } else {
            if lists.is_empty(l).unwrap() {
                continue;
            }
            let got = lists
                .next_round_robin(l)
                .unwrap()
                .expect("a non-empty list");
            print_list(&mut out, &lists, step, "next", i64::from(got), l);
        }
    }
    out
}

#[test]
fn lists_order_and_rotate_exactly_as_freertos_list_c_does() {
    // `portMAX_DELAY` is the end marker's own value on both sides.
    assert_eq!(<u32 as ListValue>::MAX, u32::MAX);
    let ours = run();
    let theirs = TRACE.replace("\r\n", "\n");
    let mut ours_lines = ours.lines();
    for (n, want) in theirs.lines().enumerate() {
        let got = ours_lines.next().unwrap_or("<missing>");
        assert_eq!(got, want, "list divergence at trace line {n}");
    }
    assert!(ours_lines.next().is_none(), "Rust printed extra lines");
    // The script must reach the cases that matter.
    for (what, at_least) in [
        ("insert ", 5_000),
        ("insert_end", 3_000),
        ("remove", 5_000),
        ("next", 2_000),
        ("idx=end", 500),
    ] {
        let n = theirs.lines().filter(|l| l.contains(what)).count();
        assert!(n > at_least, "only {n} lines with {what:?}");
    }
    let max = theirs.lines().filter(|l| l.contains(":4294967295")).count();
    assert!(max > 1_000, "portMAX_DELAY appears on only {max} lines");
}
