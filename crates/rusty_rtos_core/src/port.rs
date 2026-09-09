//! The Port seam: the only way the kernel touches a CPU.
//!
//! A FreeRTOS port is `portmacro.h` plus `port.c`: critical sections, the
//! yield, the tick source, the first-task start and the context switch.
//! Here the *logic* of those is a trait the kernel is generic over; the asm
//! lives in `rusty_rtos_port-<arch>`, the one place in the family with a
//! fenced `unsafe` block. The deterministic sim port implements this trait
//! with plain function calls, which is what makes the oracle trace
//! comparable (umbrella `ORACLES.md`, "the sim contract").
//!
//! K0 fixes the seam's shape for what K1 needs on the sim; the stack
//! initialisation and first-task start arrive with the first real port and
//! are recorded in the decision log when they do.

use crate::isr::Woken;

/// What a port provides to the kernel.
pub trait Port {
    /// `portYIELD()`: request a context switch at the next opportunity.
    /// From task context only; an ISR uses [`Port::yield_from_isr`].
    fn yield_now(&self);

    /// `portYIELD_FROM_ISR(x)`: switch on the way out of the interrupt if a
    /// higher-priority task was woken.
    fn yield_from_isr(&self, woken: Woken);

    /// `portENTER_CRITICAL()`: mask interrupts, counting nesting.
    fn enter_critical(&self);

    /// `portEXIT_CRITICAL()`: unmask when the nesting count reaches zero.
    fn exit_critical(&self);

    /// `portSET_INTERRUPT_MASK_FROM_ISR()`: mask, returning the previous
    /// mask for [`Port::clear_interrupt_mask_from_isr`].
    fn set_interrupt_mask_from_isr(&self) -> u32;

    /// `portCLEAR_INTERRUPT_MASK_FROM_ISR(x)`.
    fn clear_interrupt_mask_from_isr(&self, saved: u32);

    /// Whether the caller is in interrupt context (`xPortIsInsideInterrupt`
    /// where the port has it; the sim knows exactly).
    fn in_isr(&self) -> bool;

    /// `portGET_CORE_ID()`; `0` on a single core.
    fn core_id(&self) -> u8 {
        0
    }

    /// The port's idle behaviour: wait for the next interrupt, or on the
    /// sim, deliver the next tick per the sim contract.
    fn idle(&self) {}

    /// `portSUPPRESS_TICKS_AND_SLEEP(x)` for tickless idle; the default is
    /// the C default, a no-op that keeps the tick running.
    fn suppress_ticks_and_sleep(&self, _expected_idle_ticks: u64) {}
}
