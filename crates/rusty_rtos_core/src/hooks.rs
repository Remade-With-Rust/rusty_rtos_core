//! The application hooks as a trait.
//!
//! `vApplicationIdleHook`, `vApplicationTickHook`,
//! `vApplicationMallocFailedHook`, `vApplicationStackOverflowHook`,
//! `vApplicationDaemonTaskStartupHook` and the sleep-processing macros are
//! link-time symbols in C, switched on by `configUSE_*_HOOK`. Here they are
//! methods with no-op defaults on a type the kernel is generic over, so a
//! firmware implements the ones it wants and forgets none.

use crate::handle::TaskHandle;

/// The application's hooks. Every method has a no-op default.
pub trait Hooks {
    /// `vApplicationIdleHook`: called by the idle task on every pass. Must
    /// not block. On the sim port this is also where a tick is delivered
    /// (umbrella `ORACLES.md`, the sim contract).
    fn idle(&self) {}

    /// `vApplicationPassiveIdleHook` (SMP): the passive idle tasks' hook.
    fn passive_idle(&self) {}

    /// `vApplicationTickHook`: called from the tick interrupt. Must not block.
    fn tick(&self) {}

    /// `vApplicationMallocFailedHook`: an allocation through the [`crate::heap::Heap`]
    /// seam returned `None`.
    fn malloc_failed(&self) {}

    /// `vApplicationStackOverflowHook`: the named task overflowed its stack.
    fn stack_overflow(&self, _task: TaskHandle, _name: &str) {}

    /// `vApplicationDaemonTaskStartupHook`: the timer daemon task's first run.
    fn daemon_startup(&self) {}

    /// `configPRE_SLEEP_PROCESSING(x)`: before a tickless sleep of the given
    /// expected length; may shorten it by returning a smaller value.
    fn pre_sleep(&self, expected_idle_ticks: u64) -> u64 {
        expected_idle_ticks
    }

    /// `configPOST_SLEEP_PROCESSING(x)`: after a tickless sleep.
    fn post_sleep(&self, _expected_idle_ticks: u64) {}

    /// `configPRE_SUPPRESS_TICKS_AND_SLEEP_PROCESSING(x)`: may veto a
    /// tickless sleep by returning `0`.
    fn pre_suppress_ticks(&self, expected_idle_ticks: u64) -> u64 {
        expected_idle_ticks
    }
}

/// No hooks at all: every method is the default no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoHooks;

impl Hooks for NoHooks {}

/// `vApplicationTickHook`, as something that can actually reach the kernel.
///
/// [`Hooks::tick`] takes `&self` and so can only touch state of its own.
/// That is enough for a hook that counts, and not enough for the ones the
/// FreeRTOS standard demos install: they call the `FromISR` half of the
/// API — `xQueueOverwriteFromISR`, `xQueueSendFromISR`,
/// `xEventGroupSetBitsFromISR`, `xSemaphoreGiveFromISR` — from inside the
/// tick interrupt, which is the only place several of them are exercised
/// at all.
///
/// So this seam hands the kernel to the hook. It is `Copy` and takes
/// `self` by value because the kernel holds it: the kernel copies it out,
/// runs it, and stores what comes back, which is what lets the hook borrow
/// the kernel mutably without borrowing itself twice. A hook that reads
/// `kernel`'s own copy of itself sees the value from before this call —
/// there is no reason to, and the borrow checker is not the thing stopping
/// you.
///
/// `K` is the kernel type. The kernel is generic over the hook and the
/// hook is generic over the kernel; that is not circular, because both are
/// resolved to one concrete pair at the point a firmware names them.
pub trait TickHook<K>: Copy {
    /// Run one tick's worth of interrupt-context work.
    ///
    /// Called from inside the kernel's tick entry, at exactly the point
    /// `xTaskIncrementTick` calls `vApplicationTickHook` — including the
    /// second call site, where the scheduler is suspended and the tick is
    /// only being pended. Must not block.
    fn tick(self, kernel: &mut K) -> Self;
}

/// `configUSE_TICK_HOOK 0`: no tick hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoTickHook;

impl<K> TickHook<K> for NoTickHook {
    fn tick(self, _kernel: &mut K) -> Self {
        self
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn defaults_pass_the_sleep_length_through() {
        let h = NoHooks;
        assert_eq!(h.pre_sleep(17), 17);
        assert_eq!(h.pre_suppress_ticks(17), 17);
        h.idle();
        h.tick();
        h.malloc_failed();
        h.stack_overflow(TaskHandle::NULL, "x");
        h.daemon_startup();
        h.post_sleep(1);
        h.passive_idle();
    }

    #[test]
    fn the_null_tick_hook_returns_itself_and_touches_nothing() {
        let mut kernel = 0_u32;
        let hook = NoTickHook;
        assert_eq!(TickHook::tick(hook, &mut kernel), NoTickHook);
        assert_eq!(kernel, 0);
    }
}
