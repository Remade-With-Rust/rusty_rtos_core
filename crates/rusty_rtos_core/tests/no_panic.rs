//! The robustness gate: nothing in this crate panics on any input a caller
//! can construct. Random inputs from an LCG (the same corpus on every
//! machine) drive every constructor, every conversion and a long random
//! operation sequence over the list and the arena, each under
//! `catch_unwind` so a failure names the site and prints the input.
//!
//! The list is the one that matters: the C `list.c` corrupts silently on a
//! double insert or a remove of an unlinked item; ours must answer with an
//! `Error` for every sequence.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use rusty_rtos_core::arena::Arena;
use rusty_rtos_core::config::{Config, DefaultConfig, PosixDemoConfig};
use rusty_rtos_core::handle::{Handle, Task, TaskHandle};
use rusty_rtos_core::list::Lists;
use rusty_rtos_core::priority::Priority;
use rusty_rtos_core::tick::{Bits16, Bits32, Bits64, Tick};
use rusty_rtos_core::time::{Duration, Timeout};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// A value that is often near a boundary: 0, 1, MAX, MAX-1, or random.
    fn edgy(&mut self) -> u64 {
        match self.below(6) {
            0 => 0,
            1 => 1,
            2 => u64::MAX,
            3 => u64::MAX - 1,
            4 => u64::from(u32::MAX),
            _ => self.next(),
        }
    }
}

fn check<R>(name: &str, input: &str, f: impl FnOnce() -> R) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        panic!("{name} panicked on {input}");
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "a 50k-iteration sweep; hours under Miri, milliseconds natively (the unit tests carry the Miri gate)"
)]
fn constructors_and_conversions_never_panic() {
    let mut rng = Lcg(0x5eed_0001);
    for _ in 0..50_000 {
        let raw = (rng.edgy() & 0xFF) as u8;
        let max = (rng.edgy() & 0xFF) as u8;
        check("Priority::new", &format!("{raw} {max}"), || {
            let _ = Priority::new(raw, max);
            let _ = Priority::highest(max);
        });
        let h = rng.edgy() as u32;
        check("Handle::from_raw", &format!("{h:#x}"), || {
            let handle = TaskHandle::from_raw(h);
            let _ = handle.non_null();
            assert_eq!(TaskHandle::from_raw(handle.to_raw()), handle);
        });
        let n = rng.edgy();
        let m = rng.edgy();
        check("Tick arithmetic", &format!("{n} {m}"), || {
            for t in [Tick::<Bits16>::new(n), Tick::new(m)] {
                let _ = t.wrapping_add(m);
                let _ = t.wrapping_sub(m);
                let _ = t.checked_add(m);
                let _ = t.since(Tick::new(m));
                let _ = t.overflows_by(m);
            }
            let _ = Tick::<Bits32>::new(n).checked_add(m);
            let _ = Tick::<Bits64>::new(n).checked_add(m);
        });
        check("Duration / Timeout", &format!("{n} {m}"), || {
            let d = match m % 3 {
                0 => Duration::millis(n),
                1 => Duration::micros(n),
                _ => Duration::secs(n),
            };
            let _ = d.to_ticks::<DefaultConfig>();
            let _ = d.to_ticks::<PosixDemoConfig>();
            let _ = Timeout::from_ticks::<DefaultConfig>(n).to_ticks::<DefaultConfig>();
            let _ = Timeout::after::<PosixDemoConfig>(d);
            let _ = DefaultConfig::ms_to_ticks(n);
            let _ = DefaultConfig::ticks_to_ms(n);
        });
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "200 x 500 random operations; hours under Miri, milliseconds natively (the unit tests carry the Miri gate)"
)]
fn random_list_and_arena_operation_sequences_never_panic() {
    const ITEMS: usize = 12;
    const LISTS: usize = 3;
    let mut rng = Lcg(0x5eed_0002);
    for round in 0..200 {
        let mut lists = Lists::<ITEMS, LISTS>::new();
        let mut arena = Arena::<Task, u64, ITEMS>::new();
        let mut handles: Vec<TaskHandle> = Vec::new();
        for step in 0..500 {
            let op = rng.below(12);
            // Sometimes out of range on purpose.
            let item = (rng.below(ITEMS as u64 + 3)) as u16;
            let list = (rng.below(LISTS as u64 + 2)) as u8;
            let value = rng.edgy();
            let input =
                format!("round {round} step {step} op {op} item {item} list {list} value {value}");
            check("Lists", &input, || match op {
                0 | 1 => {
                    let _ = lists.insert(list, item, value);
                }
                2 | 3 => {
                    let _ = lists.insert_end(list, item);
                }
                4 | 5 => {
                    let _ = lists.remove(item);
                }
                6 => {
                    let _ = lists.next_round_robin(list);
                }
                7 => {
                    let _ = lists.set_value(item, value);
                    let _ = lists.value(item);
                    let _ = lists.container(item);
                }
                8 => {
                    let _ = lists.head(list);
                    let _ = lists.head_value(list);
                    let _ = lists.len(list);
                    let _ = lists.is_empty(list);
                    let _ = lists.next(item);
                    let _: usize = lists.iter(list).count();
                }
                9 => {
                    if let Ok(h) = arena.insert(value) {
                        handles.push(h);
                    }
                }
                10 => {
                    let h = if handles.is_empty() || rng.below(4) == 0 {
                        Handle::from_raw(rng.edgy() as u32)
                    } else {
                        handles[rng.below(handles.len() as u64) as usize]
                    };
                    let _ = arena.get(h);
                    let _ = arena.resolve(h);
                    let _ = arena.contains(h);
                    let _ = arena.handle_at(item);
                }
                _ => {
                    if !handles.is_empty() {
                        let i = rng.below(handles.len() as u64) as usize;
                        let h = handles.swap_remove(i);
                        let _ = arena.remove(h);
                        // A second remove of the same handle must be a clean None.
                        assert!(arena.remove(h).is_none());
                    }
                }
            });
            // The list invariant: every list's length equals what an
            // iteration finds, and every contained item names its container.
            for l in 0..LISTS as u8 {
                let len = lists.len(l).unwrap();
                let walked = lists.iter(l).count();
                assert_eq!(len, walked, "{input}: length {len} but walked {walked}");
                for it in lists.iter(l) {
                    assert_eq!(lists.container(it).unwrap(), Some(l), "{input}");
                }
            }
        }
    }
}
