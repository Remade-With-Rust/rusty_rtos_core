# rusty_rtos_alloc

The Kairos allocator seam: `rusty_alloc` (pure-Rust mimalloc remake, pinned
exactly) as the global allocator of a hosted deliverable. The seam hands out
the type; the deliverable's `main.rs` declares it. No library in the family
ever declares a global allocator.

```rust
#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;
```

## On a chip: `small-metal`

```toml
rusty_rtos_alloc = { version = "0.1", default-features = false, features = ["small-metal"] }
```

```text
RUSTFLAGS="--cfg ra_single_threaded --cfg ra_small_profile"   # rusty_alloc's own opt-ins
```

```rust
use rusty_rtos_alloc::small_metal::{Region, good_region_size};
static HEAP: Region<{ good_region_size(220 * 1024) }> = Region::new();
let usable = HEAP.give()?; // hand the region over once, before the first allocation
```

Build with `panic = "abort"`. Checked on `thumbv7em-none-eabihf`,
`thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf` and
`riscv32imafc-unknown-none-elf` (CI and `kairos check`); nothing here has run
on a board yet — that is K4's ledger row.

Part of Kairos (Remade With Rust). Plan: `docs/plans/rusty_rtos_core.md` in the repo.
