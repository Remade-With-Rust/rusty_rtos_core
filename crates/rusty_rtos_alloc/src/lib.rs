//! The allocator seam: `rusty_alloc` as the family's global allocator on a
//! hosted target.
//!
//! Every hosted deliverable in Kairos (the demo runner, the C-ABI harness,
//! a Posix-port binary) declares its global allocator through this crate
//! and never names `rusty_alloc` itself. That is the house rule, and it buys
//! one thing: the exact pin and the profile are decided here once.
//!
//! **A library must never declare a global allocator.** A program may define
//! exactly one, so a library that declares it forces the choice on every
//! consumer and makes two such libraries impossible to link together. This
//! crate does not declare one either — it hands out the type, and the
//! deliverable's own `main.rs` does the declaring:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;
//! ```
//!
//! # On a chip: the `small-metal` half
//!
//! The kernel's own heaps are `rusty_rtos_heap` (the `heap_1` / `heap_4` /
//! `heap_5` remakes behind the `Heap` seam). A firmware that wants
//! `rusty_alloc`'s small-metal profile as its Rust global allocator (the
//! `heap_3` seam of K4, or any `alloc` use above the kernel) builds this
//! crate with `--no-default-features --features small-metal` and sets, in
//! its `.cargo/config.toml` or `RUSTFLAGS`:
//!
//! ```text
//! --cfg ra_single_threaded --cfg ra_small_profile
//! ```
//!
//! Both are rusty_alloc's own opt-ins: without the first it refuses to
//! build (its `no_std` profile assumes one thread), without the second it
//! builds and then allocates nothing (32 MiB segments). The firmware then
//! gives the allocator a region once, before the first allocation:
//!
//! ```ignore
//! use rusty_rtos_alloc::small_metal::{Region, good_region_size};
//!
//! #[global_allocator]
//! static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;
//! static HEAP: Region<{ good_region_size(220 * 1024) }> = Region::new();
//!
//! let usable = HEAP.give()?; // three whole 64 KiB segments: 196_608 bytes
//! ```
//!
//! The `?` there needs a nameable error, which is why [`small_metal`]
//! re-exports [`small_metal::PrimError`] and the four `FERR_*` codes it
//! can carry: `PrimError` is a `u32`, so the codes are the difference
//! between "the region was refused" and knowing which way.
//!
//! and builds with `panic = "abort"` (rusty_alloc aborts by panicking
//! without `std`). The seam compiles on `thumbv7em-none-eabihf`,
//! `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf` and
//! `riscv32imafc-unknown-none-elf` under those flags (CI, and `kairos check`
//! through `cfgs` in `KAIROS.toml`); running it on a board is K4's ledger
//! row, not this crate's claim.

#![no_std]
#![forbid(unsafe_code)]

/// The global allocator type. A deliverable writes
/// `#[global_allocator] static A: Alloc = Alloc;` and nothing else.
pub use rusty_alloc_api::RustyAlloc as Alloc;

/// The pinned allocator version, so a binary can print what it is running
/// rather than what its manifest said.
pub use rusty_alloc_api::VERSION;

/// The firmware half: the fixed-region API a `no_std` deliverable hands the
/// allocator its memory through. Present only without `std` and with the
/// `small-metal` feature; see the crate docs for the two `--cfg` flags.
#[cfg(all(feature = "small-metal", not(feature = "std")))]
pub mod small_metal {
    // The whole recipe the crate docs above spell out, and nothing that
    // is not part of it. `PrimError` is what makes the documented
    // `HEAP.give()?` writable at all -- without it a firmware cannot name
    // the error type it is propagating -- and the four `FERR_*` codes are
    // what make it readable, because `PrimError` is a `u32` and a bare
    // integer says nothing about which way the geometry was wrong.
    pub use rusty_alloc::prim::fixed::{
        FERR_GEOMETRY, FERR_MISALIGNED, FERR_REGISTERED, FERR_TOO_SMALL, FIXED_PAGE, MIN_REGION,
        PrimError, REGION_ALIGN, Region, good_region_size, init_region, region_contains,
        region_for, region_stats, usable_bytes,
    };

    // Two things a firmware needs that are NOT in `prim::fixed`, which is
    // why a seam that re-exports the fixed-region API in one `pub use`
    // could not see them. That gap was reported to the allocator's
    // maintainers as "nothing exposes a per-allocation usable size or a
    // slow-path counter"; the answer came back that both already exist,
    // one module over — the same shape as the `PrimError` gap closed in
    // 2.1.0.
    //
    // * `usable_size(p)` answers what a request actually COST in bytes,
    //   which `region_stats()` cannot: it reports over region extents, so
    //   it does not move for a small allocation at all.
    // * `stats()` carries `generic`, `pages_fresh`, `extends` and
    //   `pages_retired` — the counters that say WHICH route an allocation
    //   took and what that route churned, deterministically and with no
    //   clock. On a part where the timing arm needs a quiet core, the
    //   counter arm needs nothing.
    pub use rusty_alloc::alloc::{stats, usable_size};
    pub use rusty_alloc::heap::Stats;
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_pin_is_what_the_manifest_says() {
        // Moving the pin is a conscious act: the manifest and this line change together.
        assert_eq!(super::VERSION, "2.2.0");
    }
}
