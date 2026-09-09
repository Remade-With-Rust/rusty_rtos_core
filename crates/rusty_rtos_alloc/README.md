# rusty_rtos_alloc

The Kairos allocator seam: `rusty_alloc` (pure-Rust mimalloc remake, pinned
exactly) as the global allocator of a hosted deliverable. The seam hands out
the type; the deliverable's `main.rs` declares it. No library in the family
ever declares a global allocator.

```rust
#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;
```

Part of Kairos (Remade With Rust). Plan: `docs/plans/rusty_rtos_core.md` in the repo.
