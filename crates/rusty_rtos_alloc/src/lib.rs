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
//! # On a chip
//!
//! A firmware's heap is a different question: the kernel's own heaps are
//! `rusty_rtos_heap` (the `heap_1` / `heap_4` / `heap_5` remakes behind the
//! `Heap` seam), and a firmware that wants `rusty_alloc`'s small-metal
//! profile as its Rust global allocator takes the Janus `rusty_esp_alloc`
//! seam (which carries the fixed `Region` and the single-context invariant)
//! or its equivalent for the board. This crate is the hosted half only.

#![no_std]
#![forbid(unsafe_code)]

/// The global allocator type. A deliverable writes
/// `#[global_allocator] static A: Alloc = Alloc;` and nothing else.
pub use rusty_alloc_api::RustyAlloc as Alloc;

/// The pinned allocator version, so a binary can print what it is running
/// rather than what its manifest said.
pub use rusty_alloc_api::VERSION;

#[cfg(test)]
mod tests {
    #[test]
    fn the_pin_is_what_the_manifest_says() {
        // Moving the pin is a conscious act: the manifest and this line change together.
        assert_eq!(super::VERSION, "2.0.4");
    }
}
