# rusty_rtos_core

[![Remade With Rust](https://img.shields.io/badge/Remade%20With-Rust-000?logo=rust&logoColor=fff)](https://github.com/remade-with-rust)
[![By Mata Network](https://img.shields.io/badge/by-Mata%20Network-5b2be0)](https://www.mata.network)
[![crates.io](https://img.shields.io/crates/v/rusty_rtos_core.svg)](https://crates.io/crates/rusty_rtos_core)
[![docs.rs](https://docs.rs/rusty_rtos_core/badge.svg)](https://docs.rs/rusty_rtos_core)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The shared vocabulary of **Kairos** — FreeRTOS remade in Rust. Ticks,
priorities, generational handles, `list.c` remade over indices, `FreeRTOSConfig.h`
as a trait, and the four seams every other package is generic over. No C, no FFI,
no pointers, no allocator, no architecture. `#![forbid(unsafe_code)]`, `no_std`,
zero dependencies, MIT OR Apache-2.0.

- **The types**: `Tick<W>` at 16/32/64 bits with the wrapping and overflow
  rules `xTickCount` follows; `Priority` validated against the config;
  `Handle<K>` — a generational index (`u16` slot + `u16` generation), **never a
  pointer** — and `Arena<K, T, N>`, which answers a stale handle with
  `Error::Gone` instead of touching freed memory; `Lists<N, L>`, `list.c` remade
  over indices with sentinel and cursor semantics intact; one `Copy` `Error`
  whose `pd_code()` maps to the C return codes.
- **The seams**: `Config` (`FreeRTOSConfig.h` as a trait with associated
  consts), plus `Port`, `Heap`, `Hooks` and `Trace` — the last with one `Event`
  per oracle trace macro, 42 of them, which is what makes a line-by-line diff
  against the C kernel possible at all.

**Known gaps.** This is vocabulary, not behaviour: no scheduler, no queue, no
timer, no port, no heap. Those are the sibling crates. `MAX_PRIORITIES`,
arena sizes and list capacities are compile-time constants, so a system's
geometry is declared rather than allocated.

- This package's plan: [docs/plans/rusty_rtos_core.md](https://github.com/Remade-With-Rust/rusty_rtos_core/blob/main/docs/plans/rusty_rtos_core.md)
- Every number: [docs/LEDGER.md](https://github.com/Remade-With-Rust/rusty_rtos_core/blob/main/docs/LEDGER.md)
- The family plan: Kairos [`docs/plans/rtos-mission.md`](https://github.com/Remade-With-Rust/kairos/blob/main/docs/plans/rtos-mission.md)

**Claims discipline:** this README makes no performance or capability claim that
is not backed by a test, a benchmark ledger entry, or a kill test recorded in
the plan. "Scaffold" means scaffold. "Sim only" means the sim port; "builds, not
flashed" means no chip has run it.

## Conformance

This crate is not diffed against the C kernel directly — it has no behaviour
of its own to diff. It is proven *through* the kernel that uses it: the Kairos
conformance corpus produces traces **byte-identical to the C kernel's** for
100,000 ticks a scenario, and the ready lists, delayed lists and handle
arena those traces exercise are all in this crate.

| | |
|---|---|
| corpus scenarios byte-identical to the C kernel | **19** |
| architectures the corpus runs on | host, ARMv7-M, RV32, Xtensa LX7 |
| of those, on silicon rather than an emulator | Xtensa LX7 (XIAO ESP32-S3), 18/18 |
| `unsafe` blocks in this crate | **0**, enforced by `#![forbid(unsafe_code)]` |

```sh
kairos conform --all --ticks 100000      # from the Kairos umbrella
```

**What the list is proven on separately.** `Lists` carries its own rotation
tests, because "a ready task is never chosen" has exactly two causes — it is
not in the list the scheduler looks at, or the cursor is not moving — and
nothing else tells them apart. They cover rotation across other lists being
used, emptied and refilled in between.

**Still open:** nothing in this crate is timed on a part; the timing rows are
the kernel's and the port's.

## Using it

```rust
use rusty_rtos_core::config::Config;
use rusty_rtos_core::handle::{Handle, TaskKind};
use rusty_rtos_core::tick::{Bits32, Tick};

// `FreeRTOSConfig.h`, as a trait. The geometry is declared, not allocated.
#[derive(Debug, Clone, Copy, Default)]
struct MyConfig;

impl Config for MyConfig {
    type Tick = Bits32;
    const TICK_RATE_HZ: u32 = 1000;
    const MAX_PRIORITIES: u8 = 5;
    const MINIMAL_STACK_SIZE: usize = 128;
    const MAX_TASK_NAME_LEN: usize = 16;
    const TIMER_TASK_PRIORITY: u8 = 2;
    const TIMER_TASK_STACK_DEPTH: usize = 256;
    const TIMER_QUEUE_LENGTH: usize = 10;
    const NOTIFICATION_ARRAY_ENTRIES: usize = 3;
}

fn main() {
    // A tick wraps the way `xTickCount` wraps, at the width you chose.
    let t: Tick<Bits32> = Tick::from_raw(u32::MAX as u64);
    assert_eq!(t.wrapping_add(2).raw(), 1);

    // A handle is an index plus a generation, so a stale one is detectable
    // rather than dangling. This is the property that removes a whole class
    // of use-after-free from the kernel above.
    let h: Handle<TaskKind> = Handle::new(3, 1);
    assert_eq!(h.index(), 3);
    assert_ne!(h, Handle::new(3, 2)); // same slot, older generation
}
```

## Performance

The one row this crate owns is its list, measured against the `list.c` it
remakes.

| arm | instructions per list operation | vs C |
|---|---:|---:|
| FreeRTOS `list.c` | 22.32 | 1.00× |
| `rusty_rtos_core::list` | **34.22** | **1.533×** |

It was 46.45 and 2.081×. What closed the gap was reading each node a call
touches **once**: `next_and_value` and `prev_of` replace a `links` accessor
that returned more than any caller needed, `link_between` takes the `after` its
callers already hold instead of re-reading it, and the sorted walk reads one
node per step instead of two.

Method: callgrind, three run lengths, the cost taken as the **slope** so
fixed setup cannot flatter it — and both arms print checksum
`7de9075f4deb23e5`, which is how you know they did the same work. It cost the
corpus nothing: 18 scenarios still identical to the C kernel at 100,000 ticks,
every arm's ticks, yields, exits and lines equal to the digit.

```sh
bench/list-cost/run.sh      # from the Kairos umbrella
```

## Portability

`no_std` everywhere, with an `alloc` rung and a `std` rung above it. The crate
knows nothing about a CPU, so "portable" here means it compiles and its tests
pass, not that a scheduler ran.

| target | builds | corpus runs above it |
|---|---|---|
| host (x86-64 Windows, Linux) | ✅ | ✅ |
| `thumbv7m-none-eabi` (Cortex-M3) | ✅ | ✅ |
| `riscv32imac-unknown-none-elf` | ✅ | ✅ |
| `xtensa-esp32s3-none-elf` | ✅ | ✅ on silicon |

Default features are `std`; `--no-default-features` is pure `core`.

## Layout

```text
crates/rusty_rtos_core   no_std (+ alloc); forbid(unsafe); no dependencies; the crate every package uses
crates/rusty_rtos_alloc  the family's allocator seam: rusty_alloc-api at an exact pin, declared only by deliverables
firmware/                per-chip example projects, excluded from the workspace
docs/plans/              this package's plan and its hardening audit
docs/LEDGER.md           every number, with its method line
```

## Build

```sh
cargo test --workspace                                   # host: the tests
cargo check -p rusty_rtos_core --no-default-features \
  --target thumbv7em-none-eabihf                         # Cortex-M4F class, no alloc
cargo check -p rusty_rtos_core --no-default-features --features alloc \
  --target riscv32imac-unknown-none-elf                  # ESP32-C6 class, with alloc
```

CI holds the core to `thumbv7em-none-eabihf`, `thumbv8m.main-none-eabihf`,
`riscv32imac-unknown-none-elf` and `riscv32imafc-unknown-none-elf`, with and
without `alloc`, plus `cargo deny check`. Firmware examples (Xtensa needs the
esp toolchain; Cortex-M and RISC-V work on stable) are built from their own
directories under `firmware/`.

## Part of Remade With Rust

This crate is part of **[Kairos](https://github.com/Remade-With-Rust/kairos)** —
FreeRTOS remade in memory-safe Rust, as independent packages that expose the API
a FreeRTOS developer already knows and prove every scheduling decision against
the C kernel's own trace. `rusty_rtos_core` is the bottom of that stack: every other crate depends on it and it depends on nothing.

The family:
[`rusty_rtos_core`](https://crates.io/crates/rusty_rtos_core) (the shared
vocabulary),
[`rusty_rtos_kernel`](https://crates.io/crates/rusty_rtos_kernel) (the
scheduler),
[`rusty_rtos_port`](https://crates.io/crates/rusty_rtos_port) (the architecture
seam),
[`rusty_rtos_heap`](https://crates.io/crates/rusty_rtos_heap) (the allocators),
[`rusty_rtos-capi`](https://github.com/Remade-With-Rust/rusty_rtos-capi) (the C
ABI) and
[`rusty_rtos_demo`](https://github.com/Remade-With-Rust/rusty_rtos_demo) (the
conformance corpus). Also check out the rest of
**[github.com/remade-with-rust](https://github.com/remade-with-rust)**.

## About Mata Network

<!-- ORG BOILERPLATE — keep identical across repos -->

[Mata Network](https://www.mata.network) builds sovereign, self-hostable
infrastructure. **Remade With Rust** is our open-source home for the
permissively-licensed building blocks that work depends on.

<!-- /ORG BOILERPLATE -->

## License

MIT OR Apache-2.0, at your option. FreeRTOS is MIT-licensed by Amazon.com,
Inc. or its affiliates; this crate remakes its API and behaviour from the
published sources and links no FreeRTOS code.

---

<!-- HARDENING-TABLE:BEGIN generated by use-protection-please — edit docs/plans/use-protection-please.md, not this block -->
## Hardening status

**Tier** critical-path · **Audited** 2026-09-16 (v0.1.0 release pass) · **v1.0.0 gates** 10/17 · [Full checklist](https://github.com/Remade-With-Rust/rusty_rtos_core/blob/main/docs/plans/use-protection-please.md)

`██████████░░░░░░░░░░` **50%** &nbsp;·&nbsp; 18 Completed · 0 Scheduled · 18 Incomplete · 19 N/A

| Phase | ✅ Completed | 🗓 Scheduled | ⬜ Incomplete | · N/A |
|---|--:|--:|--:|--:|
| 0 — Threat modeling | 0 | 0 | 2 | 0 |
| 1 — Toolchain | 2 | 0 | 2 | 0 |
| 2 — Supply chain | 7 | 0 | 1 | 0 |
| 3 — Code level | 6 | 0 | 1 | 0 |
| 4 — Static analysis | 0 | 0 | 1 | 0 |
| 5 — Dynamic analysis | 1 | 0 | 2 | 0 |
| 6 — Fuzzing and properties | 1 | 0 | 3 | 0 |
| 7 — Formal verification | 0 | 0 | 1 | 0 |
| 8 — Build and binary | 0 | 0 | 1 | 1 |
| 9 — Runtime privilege | 0 | 0 | 0 | 1 |
| 10 — Cryptography | 0 | 0 | 0 | 3 |
| 11 — CI/CD, release, and operations | 1 | 0 | 4 | 0 |
| 12 — Compliance controls | 0 | 0 | 0 | 14 |
| **Total** | **18** | **0** | **18** | **19** |

Gates waived for 0.x are listed with their reasons in the plan's "v0.1.0 release decision" section — an Incomplete gate not listed there is an omission, not a decision.

**Architect** — [Tim Almond](https://github.com/Ttimmahlax) — accountable for this unit's security design; rendered
<!-- HARDENING-TABLE:END -->
