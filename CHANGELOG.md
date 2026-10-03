# Changelog

Security-relevant changes are called out under **Security** (hardening gate
H-38). Versions follow SemVer; in 0.x a minor bump may break the API.

## Unreleased

### Added
- `TickHook::wants_tick(&self) -> bool`, provided, default `true`: a hook
  may say its `tick` would hand `self` back untouched, and the kernel then
  skips the call and the copy of the hook out and back that the by-value
  signature costs every tick. `NoTickHook` answers `false`. Adding a
  provided method is not a breaking change. In the demo's sim, whose tick
  hook is an enum the size of its largest interrupt half (448 bytes), it
  took the scenarios with none -5.5M instructions of ~115M (BlockQ,
  GenQTest), at +2 a tick for the scenarios that have one.

## 0.2.4 — 2026-10-02

### Added
- `ListsOf::move_to_end(list, item)`: `uxListRemove` then `vListInsertEnd`
  into the SAME list, as one operation -- what the SMP scheduler does to the
  running task on every switch. The item is read once and the length never
  moves. Answers `Ok(false)` and changes nothing if the item is not in that
  list; `Err` for a list or item id that names nothing, checked BEFORE
  anything is written. Proved against the pair it fuses by
  `tests/move_to_end.rs` (32 seeds x 5,000 steps: order, values, length and
  round-robin cursor after every step, including bad list ids). In the
  two-core kernel it took `select_for_core` -1,494,024 instructions on the
  smp-ir bench (semtest -636,806).

### Changed
- The list iterator ends on its count alone. A list's length is the number
  of items between its marker's links -- this crate's own invariant, held
  against `list.c` by `tests/list_differential.rs` -- so testing for the
  marker as well paid a compare and a branch per item. Two-core corpus:
  semtest -212,931, BlockQ -187,484 instructions. No one-core path uses the
  iterator; the one-core kernel is unchanged to the instruction.

## 0.2.3 — 2026-10-01

### Fixed
- `rusty_rtos_alloc` pins `rusty_alloc` / `rusty_alloc-api` `=2.2.2`. 2.2.1
  did not compile on macOS (`mincore`'s out-vector type differs on Apple), so
  this crate's `alloc` feature, and CI's macOS host job, could not build
  there. 2.2.2 also fixes `range_is_reserved` on macOS and a `blockmap`
  abort under multi-threaded segment adoption.

## 0.2.2 — 2026-10-01

### Security
- `cargo vet` coverage (`supply-chain/`): 14 of 15 dependencies certified;
  `portable-atomic` exempted pending the owner's decision.
- `fuzz/lists_arena`: a coverage-guided fuzz target checking the arena and
  the lists against a reference model after every operation.
- A threat model (`docs/threat-model.md`) with a residual-risk register.
- CI: every action pinned to a commit SHA, `permissions: contents: read`,
  `cargo vet --locked`, the unsafe census, the hardening-table check and a
  fuzz regression per push; fuzzing, AddressSanitizer and `cargo careful`
  nightly.

### Fixed
- `generations_never_mint_a_null_handle` had no `#[test]` and had never run;
  it runs, and passes.
- CI was red at 0.2.1 (clippy `-D warnings`, `cargo fmt --check`); green.
