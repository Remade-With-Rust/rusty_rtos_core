# rusty_rtos_core — package plan

**One sentence:** The shared vocabulary of the Kairos family (FreeRTOS remade in Rust): ticks, priorities, generational handles, one Copy error, the Config trait, and the Port / Heap / Trace / Hooks seams. No pointers, no allocator, no arch. forbid(unsafe).

Family plan: Kairos `docs/plans/rtos-mission.md` (umbrella repo) — its §2.1
names what this package remakes, wraps and never touches; its §2.5 lists the
types this package owns; its §6 carries the phase this package's kill test
belongs to. This file obeys that one.

Written 2026-09-09. Status: **K0 — real types, no oracle comparison yet.**
The types below exist, are tested on the host, check on four bare-metal
targets with and without `alloc`, and pass the fleet gate. Nothing has been
diffed against the C kernel (that is `rusty_rtos_kernel`'s K1 job) and
nothing has run on a chip.

---

## 1. What it is, what it is not

**Is:** the vocabulary every other `rusty_rtos_*` package speaks: the value
types (`Tick`, `Priority`, `Duration`, `Timeout`), the handle scheme
(`Handle<K>`, `Arena`), the list algorithm the C kernel's `list.c` is built
on (`Lists`), the configuration (`Config` with `DefaultConfig` and
`PosixDemoConfig`), the one error, and the four seams the kernel is generic
over (`Port`, `Heap`, `Trace`, `Hooks`) plus the ISR token (`Isr`, `Woken`).

**Is not:** a scheduler, a queue, a timer, a port, a heap or a binding. It
knows no CPU, allocates nothing, and links no FreeRTOS code. Its only
dependency is `core` (and `alloc` behind the feature).

## 2. The laws this package encodes

1. `no_std` (+ `alloc`), `forbid(unsafe_code)`, arch-agnostic; the lint
   policy denies `unwrap`/`expect`/`panic`/`todo`/`unimplemented` and holds
   `indexing_slicing` and `arithmetic_side_effects` under `-D warnings`, so
   every slice access is a `get()` and every sum is checked or wrapping by
   name.
2. **Handles are generational indices, never pointers** (mission plan
   §2.5): `Handle<K>` is a `u16` index plus a `u16` generation, `NULL` is
   generation 0, and an `Arena` refuses a stale handle with `Error::Gone`
   instead of dereferencing freed memory. The C kernel's use-after-delete is
   not representable here.
3. **`list.c` remade over indices**: `Lists<N, L>` keeps the C semantics
   FreeRTOS relies on (a sentinel `END` item with `MAX_VALUE`, `vListInsert`
   ordering with the end-item tie rule, `vListInsertEnd` before the cursor,
   the cursor stepping back on removal, round-robin `next`) so the kernel's
   trace can match the oracle line for line.
4. **`FreeRTOSConfig.h` is a type**: `Config` carries every geometry knob as
   an associated const with `FreeRTOS.h`'s own defaults (`DefaultConfig` =
   `examples/template_configuration`, `PosixDemoConfig` = `Demo/Posix_GCC`),
   `validate()` refuses what `FreeRTOS.h`'s `#error` lines refuse, and the
   umbrella's `docs/CONFIG-MAP.md` says which of the 94 knobs is a const, a
   feature, a hook, a port matter, or never.
5. **One trace vocabulary**: `trace::Event` names the 42 macros of the
   oracle contract with the C macro's own suffix (`Event::name()`), so the
   kernel's sink and the C harness print the same word.
6. Every claim has a kill test or a ledger row; the README copies this plan
   and never upgrades it.

## 3. The surface as built (2026-09-09)

| module | what | tested by |
|---|---|---|
| `tick` | `Tick<W: TickWidth>` (`Bits16`/`Bits32`/`Bits64`): masked `u64`, `wrapping_add/sub`, `checked_add`, `since`, `overflows_by` | unit tests at each width; `tests/no_panic.rs` (50k random constructions and conversions) |
| `priority` | `Priority::new(raw, max) -> Result`, `highest`, `IDLE` | unit tests; no-panic sweep |
| `time` | `Duration` (ms/µs/s) with `to_ticks::<C>()` (floor, as `pdMS_TO_TICKS`), `Timeout::{None, Ticks, Forever}` | unit tests |
| `error` | one `Copy` `Error` (14 variants) with `pd_code()` matching the C return codes (`errCOULD_NOT_ALLOCATE_REQUIRED_MEMORY` = -1, `errQUEUE_BLOCKED`/scheduler suspended = -4) | `codes_match_the_c_kernel`, `names_are_unique` |
| `handle` | `Handle<K: Kind>` for Task/Queue/Timer/EventGroup/StreamBuffer; `to_raw`/`from_raw` (`u32`) | unit tests |
| `arena` | `Arena<K, T, const N>` with generational slots: `insert`, `try_insert`, `get`, `get_mut`, `resolve`, `remove`, `iter`, `handle_at` | invariants under 200 × 500 random ops (`tests/no_panic.rs`) |
| `list` | `Lists<const N, const L>`: `insert`, `insert_end`, `remove`, `next_round_robin`, `head`, `head_value`, `container`, `set_value`, `iter`, sentinel encoding `END_BASE = 0x8000` | ordering, tie rule, cursor semantics, `bad_arguments_are_errors_not_panics`; random-ops invariants |
| `config` | `Config` trait (+ Kairos-only `MAX_TASKS`, `MAX_QUEUES`, `MAX_TIMERS`, `MAX_EVENT_GROUPS`, `MAX_STREAM_BUFFERS`), `validate()`, `ms_to_ticks`, `ticks_to_ms`; `DefaultConfig`, `PosixDemoConfig` (64-bit ticks: the Posix port types `TickType_t` as `unsigned long`) | `validate_refuses_what_freertos_h_errors_on`, const assertions on the two profiles |
| `isr` | `Isr` token (`!Send`), `Woken::{NO, YES}`, `needed`, `or` | unit tests |
| `port`, `heap`, `hooks`, `trace` | the four seams: `Port` (yield, critical sections, interrupt mask, `in_isr`, `core_id`, idle, tickless), `Heap` (`Layout` in, `Option<NonNull<u8>>` out, free-bytes counters), `Hooks` (defaulted), `Trace` (`event(tick, Event)`); `NoHeap`, `NoHooks`, `NoTrace`, `CountTrace` | `Event::name()` coverage; the seams are exercised by the kernel (K1) |
| `FORMAT_VERSION` | `1`: the trace/sim contract version (`ORACLES.md`) | — |

`rusty_rtos_alloc` (a second crate in this workspace) is the family's
allocator seam: `pub use rusty_alloc_api::RustyAlloc as Alloc;` at the exact
pin `=2.0.4`, declared only by deliverables (firmware, tools), never by a
library.

## 4. Roadmap

| Milestone | Adds | Driven by | Kill test |
|---|---|---|---|
| **K0** (done 2026-09-09) | the vocabulary above; the two config profiles; the 42-event trace vocabulary | K0 | a clean clone builds alone; `kairos check --fmt --clippy --test --deny` passes on 8 bare-metal rungs |
| K1 | whatever `rusty_rtos_kernel`'s scheduler proves missing (a `TaskState` enum, notification-index types, `TickType` overflow helpers) — added only with the kernel's failing test | K1 | `dynamic` trace identical to the oracle's |
| K2 | queue/timer/event-group/stream-buffer value types the kernel's second milestone needs; Kani harnesses for the CBMC `list.c` proofs | K2 | Kani green on the ported proof list |
| K3 | `Port` additions for SMP (`core_id` in use; per-core critical sections) | K3 | the SMP kill test in the family plan |

## 5. Deliberately absent

- **Pointers of any kind in the public surface.** No `*mut`, no `NonNull`
  outside the `Heap` seam; handles and arenas instead.
- **An allocator.** `alloc` is a feature for `Vec`/`Box` in tests and in
  `std` consumers; the kernel's arenas are `const N` arrays.
- **`configUSE_TRACE_FACILITY` / `configGENERATE_RUN_TIME_STATS` tables.**
  Tracing is the `Trace` seam; run-time stats are a K6 concern.
- **`taskENTER_CRITICAL` as a free function.** Critical sections are the
  `Port` seam's, with `Isr`-typed ISR variants.
- **The AWS IoT libraries.** Mission plan §2.1: cloud-of-record by design.

## 6. Risks

| Risk | Mitigation |
|---|---|
| The index-based `Lists` diverges from `list.c` in a corner (`MAX_VALUE` tie, cursor on removal) and the kernel trace never matches | the semantics are asserted in unit tests copied from the C comments; the K1 kill test is a line-for-line diff against the oracle, so any divergence surfaces as the first mismatch line |
| `Config` consts drift from `FreeRTOS.h` defaults across kernel versions | `DefaultConfig` cites the template file; `ORACLES.md` pins the kernel; re-pinning re-runs `validate()` and the const assertions |
| The `u16` index/generation halves of `Handle` are too small for a big system | `MAX_*` consts are per-`Config`; 65,535 objects of a kind and 65,535 reuses per slot is far beyond any FreeRTOS deployment; widening is a `FORMAT_VERSION` bump |
| `PosixDemoConfig` (64-bit ticks) and `DefaultConfig` (32-bit) make the kernel's tests target-shaped | the kernel is generic over `Config`; the sim runs `PosixDemoConfig` because the oracle does, firmware runs 32-bit |

## 7. Decision log

| Date | Decision |
|---|---|
| 2026-09-09 | Stamped from the Kairos template; obeys the family plan. |
| 2026-09-09 | The `-core` crate convention is folded: this package *is* the core, so the workspace holds `rusty_rtos_core` (the crate every package depends on) and `rusty_rtos_alloc` (the allocator seam) with no facade layer. |
| 2026-09-09 | `PosixDemoConfig::Tick = Bits64`: the Posix port's `portmacro.h` types `TickType_t` as `unsigned long`, and the oracle trace prints `portMAX_DELAY` as 2^64-1; matching the oracle beats matching the demo's `configUSE_16_BIT_TICKS = 0` reading. |
| 2026-09-09 | `Event` gains `EventGroupCreate` and `StreamBufferCreate` (42 events): objects are named by creation ordinal in the trace, so the creation lines are what assign the names. |
| 2026-09-09 | `ticks_to_ms` uses `checked_div(...).unwrap_or(0)`: a `TICK_RATE_HZ` of 0 is refused by `validate()`, and the arithmetic lint policy still wants the division total. |
