//! The Heap seam: the only way the kernel allocates.
//!
//! `pvPortMalloc` / `vPortFree` and the two size probes, as a trait. Static
//! allocation (the arena, and caller-owned stacks) needs no heap at all;
//! [`NoHeap`] is the implementation a static-only build carries, and it
//! answers every allocation with `None`, which is exactly what
//! `pvPortMalloc` returning `NULL` means.
//!
//! The trait passes `NonNull<u8>` and `Layout` through; constructing and
//! carrying a pointer is safe Rust, and the one place that dereferences one
//! is the heap implementation's own fenced block in `rusty_rtos_heap`.

use core::alloc::Layout;
use core::ptr::NonNull;

/// What a heap provides to the kernel.
pub trait Heap {
    /// `pvPortMalloc`: `None` when the request cannot be met (the kernel
    /// turns that into [`crate::Error::NoMemory`] and calls the
    /// malloc-failed hook).
    fn alloc(&self, layout: Layout) -> Option<NonNull<u8>>;

    /// `vPortFree`. `layout` is what [`Heap::alloc`] was given.
    fn free(&self, ptr: NonNull<u8>, layout: Layout);

    /// `xPortGetFreeHeapSize`.
    fn free_bytes(&self) -> usize;

    /// `xPortGetMinimumEverFreeHeapSize`.
    fn minimum_ever_free_bytes(&self) -> usize;
}

/// A heap that has nothing: the static-allocation-only build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoHeap;

impl Heap for NoHeap {
    fn alloc(&self, _layout: Layout) -> Option<NonNull<u8>> {
        None
    }

    fn free(&self, _ptr: NonNull<u8>, _layout: Layout) {}

    fn free_bytes(&self) -> usize {
        0
    }

    fn minimum_ever_free_bytes(&self) -> usize {
        0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn no_heap_refuses_everything() {
        let h = NoHeap;
        assert!(h.alloc(Layout::new::<u64>()).is_none());
        assert_eq!(h.free_bytes(), 0);
        assert_eq!(h.minimum_ever_free_bytes(), 0);
    }
}
