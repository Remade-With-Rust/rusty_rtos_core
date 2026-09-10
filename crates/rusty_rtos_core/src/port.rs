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

    // ----------------------------------------------------- the tick source --
    //
    // On silicon the tick is an interrupt: the port's handler calls the
    // kernel's tick entry directly and every default below is right. On a
    // simulator there is no interrupt, so the port *raises* a tick and the
    // kernel takes it at the same point the hardware one would have fired
    // (`ORACLES.md`, sim contract v1). Keeping that on this seam rather
    // than in a second trait is what lets the kernel be generic over one
    // bound, and mirrors the C port, whose `port.c` owns exactly these
    // counters and this handler.

    /// Take a tick the port has raised, if any. `true` at most once per
    /// raise. A hardware port raises none.
    fn take_pending_tick(&self) -> bool {
        false
    }

    /// Count one delivered tick, for the counters a scenario reports
    /// (`ulKairosTicks` on the C side).
    fn count_tick(&self) {}

    /// Count one `portYIELD()` (`ulKairosYields` on the C side).
    fn count_yield(&self) {}

    /// Outermost critical-section exits so far (`ulKairosExits` on the C
    /// side). On a simulator this is the clock; on silicon it is zero and
    /// nobody asks.
    fn exits(&self) -> u64 {
        0
    }

    /// `portSET_INTERRUPT_MASK_FROM_ISR()`: mask interrupts from inside
    /// one, returning the previous mask to restore.
    ///
    /// It is a *different* seam from [`Port::enter_critical`] because the C
    /// makes it one, and on the Posix port both halves are literally empty:
    /// signals are already blocked inside a handler. So a `FromISR` call
    /// touches neither the nesting count nor the exit count there, and this
    /// default says exactly that. A silicon port raises BASEPRI (or its
    /// equivalent) here and still must not count the exit.
    fn enter_critical_from_isr(&self) -> u32 {
        0
    }

    /// `portCLEAR_INTERRUPT_MASK_FROM_ISR( x )`.
    fn exit_critical_from_isr(&self, _mask: u32) {}

    /// Tell the port the kernel is inside its tick entry, where the C
    /// handler runs with interrupts already masked and adjusts the nesting
    /// count by hand rather than through `portEXIT_CRITICAL`.
    fn set_in_tick_entry(&self, _yes: bool) {}

    /// `xPortStartScheduler` has run: tasks are executing now.
    fn scheduler_started(&self) {}

    // ----------------------------------------------- the tail of a switch --
    //
    // Critical nesting is *per task*, even where a port keeps it in one
    // variable: the Posix port saves it in `prvSwitchThread` and restores it
    // when the task is resumed, and hands a brand-new task a zero
    // (`uxCriticalNesting = 0` in `prvWaitForStart`). A kernel with no
    // stacks has to say that out loud, and it has to say more than that.
    //
    // A thread stops at the switch. A stackless kernel's call does not: the
    // frame the scheduler switched away from is still on the machine and it
    // runs to its end — the sections it had open, and any section it opens
    // and closes afterwards. On the C port none of that has happened yet.
    // All of it happens when the task runs again.
    //
    // So the port stops charging the tail's exits to the clock and counts
    // them instead, and the kernel replays the count when the task is
    // switched back in. Counting rather than discarding is the whole point:
    // a tail is not always just the sections that were open. The timeout
    // path of `xQueueReceive` calls `prvIsQueueEmpty` *after* the
    // `xTaskResumeAll` that may have switched the caller away, and that
    // section is one more exit the C charges to the resumed task.

    /// The switch has happened, and what the caller does from here is the
    /// tail of a frame the C port would not have run yet: stop counting its
    /// exits as sim time and tally them.
    fn begin_unwind(&self) {}

    /// The tail is over. Returns how many outermost exits it made — what
    /// the switched-out task owes the clock when it next runs.
    fn end_unwind(&self) -> u32 {
        0
    }
}
