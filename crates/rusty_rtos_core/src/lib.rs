#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
//! `rusty_rtos_core` — the shared vocabulary of the Kairos family.
//!
//! Kairos is FreeRTOS remade in memory-safe Rust. This crate is Layer 0:
//! the handful of `no_std` types every other package speaks at its boundary,
//! so a task handle, a tick count, a priority or an error crosses from the
//! kernel to a port to a library with no conversion and no copy.
//!
//! The laws it encodes (mission plan §2.5):
//!
//! 1. **Handles are indices, never pointers.** Every kernel object lives in
//!    an [`arena::Arena`]; a [`handle::Handle`] is a generational index and a
//!    stale one is [`Error::Gone`], not a use-after-free.
//! 2. **The list is index-linked.** [`list::Lists`] remakes FreeRTOS's
//!    `list.c` — `vListInsert`, `vListInsertEnd`, `uxListRemove`, the
//!    round-robin cursor — over indices, so the scheduler core needs no
//!    pointer and no `unsafe`.
//! 3. **Time is a value.** [`tick::Tick`] is a monotonic count whose width
//!    is a type parameter mirroring `configTICK_TYPE_WIDTH_IN_BITS`.
//! 4. **One `Copy` error.** [`Error`] has no `String`; it maps onto the C
//!    kernel's `pdPASS` / `pdFAIL` / `errQUEUE_*` codes for the C ABI.
//! 5. **The config is a type.** [`config::Config`] is `FreeRTOSConfig.h` as
//!    associated constants; [`config::DefaultConfig`] mirrors the upstream
//!    template configuration value for value.
//! 6. **Four seams, traits only.** [`port::Port`], [`heap::Heap`],
//!    [`trace::Trace`] and [`hooks::Hooks`] are the only way the kernel
//!    touches a CPU, memory, an observer or the application.
//! 7. **The ISR side is a type.** [`isr::Isr`] is the proof a `*FromISR`
//!    variant demands; the wrong variant does not compile.
//!
//! `forbid(unsafe)`. No allocator, no arch, no product type. Feature ladder
//! `std` ⊃ `alloc` ⊃ core-only; CI holds the bare rungs on Cortex-M and
//! RISC-V on every push.

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod arena;
pub mod config;
pub mod error;
pub mod handle;
pub mod heap;
pub mod hooks;
pub mod isr;
pub mod list;
pub mod port;
pub mod priority;
pub mod tick;
pub mod time;
pub mod trace;

pub use error::{Error, Result};

/// Crate version, for manifests and logs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The version of every byte format this family emits (the trace line
/// format, the C ABI struct layouts). Readers refuse a version they do not
/// know; the bump discipline is the mission plan's §2.5.
pub const FORMAT_VERSION: u16 = 1;

/// The names a kernel, a port or a firmware wants in scope.
pub mod prelude {
    pub use crate::arena::Arena;
    pub use crate::config::{Config, DefaultConfig};
    pub use crate::error::{Error, Result};
    pub use crate::handle::{
        EventGroupHandle, Handle, QueueHandle, StreamBufferHandle, TaskHandle, TimerHandle,
    };
    pub use crate::heap::{Heap, NoHeap};
    pub use crate::hooks::{Hooks, NoHooks};
    pub use crate::isr::{Isr, Woken};
    pub use crate::list::{ListId, Lists};
    pub use crate::port::Port;
    pub use crate::priority::Priority;
    pub use crate::tick::{Bits16, Bits32, Bits64, Tick, TickWidth};
    pub use crate::time::{Duration, Timeout};
    pub use crate::trace::{Event, NoTrace, Trace};
}
