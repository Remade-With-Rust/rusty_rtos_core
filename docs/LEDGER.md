# rusty_rtos_core — the ledger

Every number this package claims, with the run that produced it. A row
without a method is not a number. Counters before clocks; an external oracle
before a self-metric; the method line names the machine, the pinning, the arm
order and the null-arm floor for anything timed.

## The build fact (2026-09-09, K0)

| gate | result | method |
|---|---|---|
| `cargo test --workspace` on the host | 34 tests pass, 0 fail (31 unit, 2 in `tests/no_panic.rs`, 1 in `rusty_rtos_alloc`; 1 doc-test ignored by design) | Windows 11 host, `rustc 1.98.0` from `rust-toolchain.toml` |
| `cargo check -p rusty_rtos_core --no-default-features` and `--features alloc` on `thumbv7em-none-eabihf`, `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `riscv32imafc-unknown-none-elf` | all 8 rungs pass | `kairos check rusty_rtos_core --fmt --clippy --test --deny` (the fleet gate), exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` under the workspace lint policy | clean | same run; `unwrap`/`expect`/`panic`/`todo`/`unimplemented` = deny, `indexing_slicing` + `arithmetic_side_effects` = warn under `-D warnings` |
| `cargo fmt --all -- --check` | clean | same run |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok | same run; `deny.toml` bans `*-sys`, `ring`, `aws-lc-sys`; one unmatched licence allowance (`Zlib`) is a warning, not a failure |
| `cargo audit` | 0 advisories | 17 locked crates (the dev/`std` closure; the `no_std` core itself has none); advisory-db of 2026-09-09, 1243 advisories |
| `cargo +nightly miri test -p rusty_rtos_core --lib` | 31 unit tests pass under Miri (77 s) | miri 0.1.0 (2026-09-08) on the Windows host; the two `tests/no_panic.rs` sweeps are ignored under Miri only (`#[cfg_attr(miri, ignore)]`), since a 50k-iteration sweep takes hours in the interpreter; a full `cargo miri test` was stopped at 15 minutes before that attribute existed |
| `tests/no_panic.rs` | 50,000 random constructions/conversions and 200 rounds × 500 random arena/list operations, no panic, invariants hold each step | LCG-seeded (fixed seed, reproducible), asserts arena occupancy, handle generation monotonicity, list ordering and cursor validity |

No speed number, no size number: nothing here has been timed or sized. Nothing
has run on a chip. The first oracle number belongs to `rusty_rtos_kernel`
(K1): a `dynamic` trace identical to `oracle/traces/dynamic.trace` in the
umbrella (24,403 lines, reproducible; umbrella `docs/LEDGER.md`).
