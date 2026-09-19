//! The Trace seam: the oracle instrument.
//!
//! FreeRTOS fires a `trace*` macro at every scheduling and IPC decision.
//! Kairos fires [`Trace::event`] with an [`Event`] whose [`Event::name`] is
//! the macro's suffix (`TASK_SWITCHED_IN`, `QUEUE_SEND`, ...), so a sink can
//! print exactly the line the instrumented C kernel prints and a diff of the
//! two traces is the conformance gate (umbrella `ORACLES.md`, "trace line
//! format"). The default sink is [`NoTrace`], a no-op the compiler removes.
//!
//! The event set is the 42 macros K1 and K2 gate on. The ~470 `traceENTER_*`
//! / `traceRETURN_*` pairs are deliberately not part of the contract.

use crate::handle::{EventGroupHandle, QueueHandle, StreamBufferHandle, TaskHandle, TimerHandle};
use crate::priority::Priority;

/// One kernel event, with what the corresponding C macro receives.
///
/// Field names are the C macro's own parameter names in snake case
/// (`task` / `name` for a TCB, `queue`, `timer`, `group`, `buffer`), which
/// is why they carry no doc of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum Event<'a> {
    /// `traceSTARTING_SCHEDULER`.
    StartingScheduler,
    /// `traceTASK_CREATE`.
    TaskCreate {
        task: TaskHandle,
        name: &'a str,
        priority: Priority,
    },
    /// `traceTASK_CREATE_FAILED`.
    TaskCreateFailed,
    /// `traceTASK_DELETE`.
    TaskDelete { task: TaskHandle, name: &'a str },
    /// `traceTASK_SWITCHED_IN`.
    TaskSwitchedIn { task: TaskHandle, name: &'a str },
    /// `traceTASK_SWITCHED_OUT`.
    TaskSwitchedOut { task: TaskHandle, name: &'a str },
    /// `traceTASK_DELAY`.
    TaskDelay {
        task: TaskHandle,
        name: &'a str,
        ticks: u64,
    },
    /// `traceTASK_DELAY_UNTIL`.
    TaskDelayUntil {
        task: TaskHandle,
        name: &'a str,
        wake_at: u64,
    },
    /// `traceTASK_SUSPEND`.
    TaskSuspend { task: TaskHandle, name: &'a str },
    /// `traceTASK_RESUME`.
    TaskResume { task: TaskHandle, name: &'a str },
    /// `traceTASK_RESUME_FROM_ISR`.
    TaskResumeFromIsr { task: TaskHandle, name: &'a str },
    /// `traceTASK_PRIORITY_SET`.
    TaskPrioritySet {
        task: TaskHandle,
        name: &'a str,
        priority: Priority,
    },
    /// `traceTASK_PRIORITY_INHERIT`.
    TaskPriorityInherit {
        task: TaskHandle,
        name: &'a str,
        priority: Priority,
    },
    /// `traceTASK_PRIORITY_DISINHERIT`.
    TaskPriorityDisinherit {
        task: TaskHandle,
        name: &'a str,
        priority: Priority,
    },
    /// `traceTASK_INCREMENT_TICK`.
    TaskIncrementTick { tick: u64 },
    /// `traceMOVED_TASK_TO_READY_STATE`.
    MovedTaskToReadyState { task: TaskHandle, name: &'a str },
    /// `traceMOVED_TASK_TO_DELAYED_LIST`.
    MovedTaskToDelayedList { task: TaskHandle, name: &'a str },
    /// `traceMOVED_TASK_TO_OVERFLOW_DELAYED_LIST`.
    MovedTaskToOverflowDelayedList { task: TaskHandle, name: &'a str },
    /// `traceTASK_NOTIFY`.
    TaskNotify {
        task: TaskHandle,
        name: &'a str,
        index: usize,
    },
    /// `traceTASK_NOTIFY_WAIT`.
    TaskNotifyWait {
        task: TaskHandle,
        name: &'a str,
        index: usize,
    },
    /// `traceTASK_NOTIFY_WAIT_BLOCK`.
    TaskNotifyWaitBlock {
        task: TaskHandle,
        name: &'a str,
        index: usize,
    },
    /// `traceTASK_NOTIFY_TAKE`.
    TaskNotifyTake {
        task: TaskHandle,
        name: &'a str,
        index: usize,
    },
    /// `traceTASK_NOTIFY_TAKE_BLOCK`.
    TaskNotifyTakeBlock {
        task: TaskHandle,
        name: &'a str,
        index: usize,
    },
    /// `traceQUEUE_CREATE`.
    QueueCreate {
        queue: QueueHandle,
        name: &'a str,
        length: usize,
    },
    /// `traceQUEUE_SEND`.
    QueueSend { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_SEND_FAILED`.
    QueueSendFailed { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_SEND_FROM_ISR`.
    QueueSendFromIsr { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_RECEIVE`.
    QueueReceive { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_RECEIVE_FAILED`.
    QueueReceiveFailed { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_RECEIVE_FROM_ISR`.
    QueueReceiveFromIsr { queue: QueueHandle, name: &'a str },
    /// `traceQUEUE_PEEK`.
    QueuePeek { queue: QueueHandle, name: &'a str },
    /// `traceBLOCKING_ON_QUEUE_SEND`.
    BlockingOnQueueSend { queue: QueueHandle, name: &'a str },
    /// `traceBLOCKING_ON_QUEUE_RECEIVE`.
    BlockingOnQueueReceive { queue: QueueHandle, name: &'a str },
    /// `traceBLOCKING_ON_QUEUE_PEEK`.
    BlockingOnQueuePeek { queue: QueueHandle, name: &'a str },
    /// `traceTIMER_CREATE`.
    TimerCreate { timer: TimerHandle, name: &'a str },
    /// `traceTIMER_COMMAND_SEND`.
    TimerCommandSend {
        timer: TimerHandle,
        name: &'a str,
        command: i32,
        value: u64,
    },
    /// `traceTIMER_EXPIRED`.
    TimerExpired { timer: TimerHandle, name: &'a str },
    /// `traceEVENT_GROUP_CREATE`.
    EventGroupCreate { group: EventGroupHandle },
    /// `traceEVENT_GROUP_SET_BITS`.
    EventGroupSetBits { group: EventGroupHandle, bits: u32 },
    /// `traceEVENT_GROUP_WAIT_BITS_BLOCK`.
    EventGroupWaitBitsBlock { group: EventGroupHandle, bits: u32 },
    /// `traceEVENT_GROUP_WAIT_BITS_END`.
    EventGroupWaitBitsEnd {
        group: EventGroupHandle,
        bits: u32,
        timed_out: bool,
    },
    /// `traceSTREAM_BUFFER_CREATE`.
    StreamBufferCreate {
        buffer: StreamBufferHandle,
        is_message_buffer: bool,
    },
    /// `traceSTREAM_BUFFER_SEND`.
    StreamBufferSend {
        buffer: StreamBufferHandle,
        bytes: usize,
    },
    /// `traceSTREAM_BUFFER_RECEIVE`.
    StreamBufferReceive {
        buffer: StreamBufferHandle,
        bytes: usize,
    },
    /// `traceLOW_POWER_IDLE_BEGIN`.
    LowPowerIdleBegin,
    /// `traceLOW_POWER_IDLE_END`.
    LowPowerIdleEnd,
}

impl Event<'_> {
    /// The C macro's suffix: the second field of a trace line.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::StartingScheduler => "STARTING_SCHEDULER",
            Self::TaskCreate { .. } => "TASK_CREATE",
            Self::TaskCreateFailed => "TASK_CREATE_FAILED",
            Self::TaskDelete { .. } => "TASK_DELETE",
            Self::TaskSwitchedIn { .. } => "TASK_SWITCHED_IN",
            Self::TaskSwitchedOut { .. } => "TASK_SWITCHED_OUT",
            Self::TaskDelay { .. } => "TASK_DELAY",
            Self::TaskDelayUntil { .. } => "TASK_DELAY_UNTIL",
            Self::TaskSuspend { .. } => "TASK_SUSPEND",
            Self::TaskResume { .. } => "TASK_RESUME",
            Self::TaskResumeFromIsr { .. } => "TASK_RESUME_FROM_ISR",
            Self::TaskPrioritySet { .. } => "TASK_PRIORITY_SET",
            Self::TaskPriorityInherit { .. } => "TASK_PRIORITY_INHERIT",
            Self::TaskPriorityDisinherit { .. } => "TASK_PRIORITY_DISINHERIT",
            Self::TaskIncrementTick { .. } => "TASK_INCREMENT_TICK",
            Self::MovedTaskToReadyState { .. } => "MOVED_TASK_TO_READY_STATE",
            Self::MovedTaskToDelayedList { .. } => "MOVED_TASK_TO_DELAYED_LIST",
            Self::MovedTaskToOverflowDelayedList { .. } => "MOVED_TASK_TO_OVERFLOW_DELAYED_LIST",
            Self::TaskNotify { .. } => "TASK_NOTIFY",
            Self::TaskNotifyWait { .. } => "TASK_NOTIFY_WAIT",
            Self::TaskNotifyWaitBlock { .. } => "TASK_NOTIFY_WAIT_BLOCK",
            Self::TaskNotifyTake { .. } => "TASK_NOTIFY_TAKE",
            Self::TaskNotifyTakeBlock { .. } => "TASK_NOTIFY_TAKE_BLOCK",
            Self::QueueCreate { .. } => "QUEUE_CREATE",
            Self::QueueSend { .. } => "QUEUE_SEND",
            Self::QueueSendFailed { .. } => "QUEUE_SEND_FAILED",
            Self::QueueSendFromIsr { .. } => "QUEUE_SEND_FROM_ISR",
            Self::QueueReceive { .. } => "QUEUE_RECEIVE",
            Self::QueueReceiveFailed { .. } => "QUEUE_RECEIVE_FAILED",
            Self::QueueReceiveFromIsr { .. } => "QUEUE_RECEIVE_FROM_ISR",
            Self::QueuePeek { .. } => "QUEUE_PEEK",
            Self::BlockingOnQueueSend { .. } => "BLOCKING_ON_QUEUE_SEND",
            Self::BlockingOnQueueReceive { .. } => "BLOCKING_ON_QUEUE_RECEIVE",
            Self::BlockingOnQueuePeek { .. } => "BLOCKING_ON_QUEUE_PEEK",
            Self::TimerCreate { .. } => "TIMER_CREATE",
            Self::TimerCommandSend { .. } => "TIMER_COMMAND_SEND",
            Self::TimerExpired { .. } => "TIMER_EXPIRED",
            Self::EventGroupCreate { .. } => "EVENT_GROUP_CREATE",
            Self::EventGroupSetBits { .. } => "EVENT_GROUP_SET_BITS",
            Self::EventGroupWaitBitsBlock { .. } => "EVENT_GROUP_WAIT_BITS_BLOCK",
            Self::EventGroupWaitBitsEnd { .. } => "EVENT_GROUP_WAIT_BITS_END",
            Self::StreamBufferCreate { .. } => "STREAM_BUFFER_CREATE",
            Self::StreamBufferSend { .. } => "STREAM_BUFFER_SEND",
            Self::StreamBufferReceive { .. } => "STREAM_BUFFER_RECEIVE",
            Self::LowPowerIdleBegin => "LOW_POWER_IDLE_BEGIN",
            Self::LowPowerIdleEnd => "LOW_POWER_IDLE_END",
        }
    }

    /// The name the C side prints in the third field, when the event has one.
    #[must_use]
    pub const fn subject(&self) -> Option<&str> {
        match self {
            Self::TaskCreate { name, .. }
            | Self::TaskDelete { name, .. }
            | Self::TaskSwitchedIn { name, .. }
            | Self::TaskSwitchedOut { name, .. }
            | Self::TaskDelay { name, .. }
            | Self::TaskDelayUntil { name, .. }
            | Self::TaskSuspend { name, .. }
            | Self::TaskResume { name, .. }
            | Self::TaskResumeFromIsr { name, .. }
            | Self::TaskPrioritySet { name, .. }
            | Self::TaskPriorityInherit { name, .. }
            | Self::TaskPriorityDisinherit { name, .. }
            | Self::MovedTaskToReadyState { name, .. }
            | Self::MovedTaskToDelayedList { name, .. }
            | Self::MovedTaskToOverflowDelayedList { name, .. }
            | Self::TaskNotify { name, .. }
            | Self::TaskNotifyWait { name, .. }
            | Self::TaskNotifyWaitBlock { name, .. }
            | Self::TaskNotifyTake { name, .. }
            | Self::TaskNotifyTakeBlock { name, .. }
            | Self::QueueCreate { name, .. }
            | Self::QueueSend { name, .. }
            | Self::QueueSendFailed { name, .. }
            | Self::QueueSendFromIsr { name, .. }
            | Self::QueueReceive { name, .. }
            | Self::QueueReceiveFailed { name, .. }
            | Self::QueueReceiveFromIsr { name, .. }
            | Self::QueuePeek { name, .. }
            | Self::BlockingOnQueueSend { name, .. }
            | Self::BlockingOnQueueReceive { name, .. }
            | Self::BlockingOnQueuePeek { name, .. }
            | Self::TimerCreate { name, .. }
            | Self::TimerCommandSend { name, .. }
            | Self::TimerExpired { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// A trace sink. The kernel calls [`Trace::event`] at every decision the C
/// kernel traces; the sink decides what to do with it.
pub trait Trace {
    /// The port's outermost critical-section exit count, as it stands at
    /// the moment of the next [`Trace::event`].
    ///
    /// On a simulator that number *is* the clock (`ORACLES.md`, sim
    /// contract v1), so a sink that is being diffed against the C oracle
    /// can print it as an extra column and compare like for like. A sink
    /// that does not care ignores it, and the call compiles away.
    fn note_exits(&mut self, _exits: u64) {}

    /// Whether this sink reads the `name` an event carries.
    ///
    /// Building one costs a UTF-8 validation -- the bytes were checked when
    /// the name was set, and a `&str` has to have them checked again -- which
    /// a sink that only counts, or drops, events never looks at. Saying so
    /// lets the kernel skip it.
    ///
    /// Defaulted to `true`, so a sink that says nothing behaves exactly as it
    /// did and every existing implementation keeps compiling.
    const WANTS_NAMES: bool = true;

    /// An event, at the kernel's current tick count.
    fn event(&mut self, tick: u64, event: Event<'_>);
}

/// The no-op sink: every event is dropped; the optimiser removes the calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoTrace;

impl Trace for NoTrace {
    // Nothing is read, so nothing needs building.
    const WANTS_NAMES: bool = false;

    fn event(&mut self, _tick: u64, _event: Event<'_>) {}
}

/// The events a tickless sleep is allowed to change.
///
/// Suppressing ticks does not change WHEN a task runs -- the port winds the
/// tick count forward over the sleep, so every task still wakes at the tick
/// it would have woken at. What it changes is that the per-tick heartbeat
/// stops firing while nothing is due, and that the idle path announces the
/// sleep it took.
///
/// Those three events, and only those three, are what [`Scheduling`] drops.
#[must_use]
pub const fn is_suppressible(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::TaskIncrementTick { .. } | Event::LowPowerIdleBegin | Event::LowPowerIdleEnd
    )
}

/// A [`Trace`] that passes on only the events a schedule is made of.
///
/// # What this is for
///
/// Turning tickless idle on changes a trace, so a raw byte-diff against the C
/// kernel's cannot survive it -- 15.2% of the conformance corpus is
/// `TASK_INCREMENT_TICK`. But it does not change the SCHEDULE: a task still
/// runs at the tick it would have run at, because the port winds the count
/// forward across the sleep.
///
/// This is the projection that says so. Wrap any sink in it and the events
/// that reach the sink are exactly the ones a correct tickless port must not
/// disturb -- so two runs that differ only in whether they slept produce the
/// same output, and a run that reordered, dropped or added a scheduling
/// decision does not.
///
/// It is a claim about what is being proved, so it is worth being narrow:
/// [`is_suppressible`] names the three events dropped, and every other event
/// -- every switch, every list move, every queue and event-group and timer
/// operation, with its tick stamp -- goes through untouched.
///
/// ```
/// use rusty_rtos_core::trace::{CountTrace, Event, Scheduling, Trace};
///
/// let mut projected = Scheduling::new(CountTrace::default());
/// projected.event(7, Event::TaskIncrementTick { tick: 7 });
/// projected.event(7, Event::StartingScheduler);
/// // The tick did not reach the sink; the scheduler start did.
/// assert_eq!(projected.into_inner().events, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Scheduling<T: Trace> {
    inner: T,
}

impl<T: Trace> Scheduling<T> {
    /// Project onto `inner`.
    pub const fn new(inner: T) -> Self {
        Self { inner }
    }

    /// The sink back out, to read whatever it collected.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// The sink, borrowed.
    pub const fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Trace> Trace for Scheduling<T> {
    // Whatever the sink needs. A projection reads no name of its own.
    const WANTS_NAMES: bool = T::WANTS_NAMES;

    fn note_exits(&mut self, exits: u64) {
        self.inner.note_exits(exits);
    }

    fn event(&mut self, tick: u64, event: Event<'_>) {
        if !is_suppressible(&event) {
            self.inner.event(tick, event);
        }
    }
}

/// A sink that counts events, for tests and for the work-count parity rule
/// ("compare a count before a duration").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CountTrace {
    /// Events seen.
    pub events: u64,
    /// `TASK_SWITCHED_IN` events seen: the context-switch count.
    pub switches: u64,
    /// `TASK_INCREMENT_TICK` events seen.
    pub ticks: u64,
}

impl Trace for CountTrace {
    // Only the count is kept, so the name is never read.
    const WANTS_NAMES: bool = false;

    fn event(&mut self, _tick: u64, event: Event<'_>) {
        self.events = self.events.saturating_add(1);
        match event {
            Event::TaskSwitchedIn { .. } => self.switches = self.switches.saturating_add(1),
            Event::TaskIncrementTick { .. } => self.ticks = self.ticks.saturating_add(1),
            _ => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::handle::Handle;

    /// A scripted scenario: a tick heartbeat with scheduling decided around
    /// it. `ticks` stands for whether the port fired the heartbeat -- which
    /// is exactly the thing a tickless sleep stops doing.
    fn play<T: Trace>(sink: &mut T, ticks: bool, switch_at_six: bool) {
        let a: TaskHandle = Handle::from_parts(1, 1);
        let b: TaskHandle = Handle::from_parts(2, 1);
        for tick in 0..8u64 {
            if ticks {
                sink.event(tick, Event::TaskIncrementTick { tick });
                sink.event(tick, Event::LowPowerIdleBegin);
                sink.event(tick, Event::LowPowerIdleEnd);
            }
            if tick == 6 && !switch_at_six {
                continue;
            }
            if tick % 2 == 0 {
                sink.event(tick, Event::TaskSwitchedOut { task: a, name: "A" });
                sink.event(tick, Event::TaskSwitchedIn { task: b, name: "B" });
            } else {
                sink.event(tick, Event::MovedTaskToReadyState { task: a, name: "A" });
            }
        }
    }

    fn projected(ticks: bool, switch_at_six: bool) -> CountTrace {
        let mut sink = Scheduling::new(CountTrace::default());
        play(&mut sink, ticks, switch_at_six);
        sink.into_inner()
    }

    /// The property the whole tickless claim rests on: a run that slept
    /// through its ticks and a run that did not project to the SAME thing.
    ///
    /// This is what lets a tickless port be gated at all. A raw trace diff
    /// cannot survive tick suppression -- 15.2% of the conformance corpus is
    /// `TASK_INCREMENT_TICK` -- and this says precisely which part of the
    /// trace is allowed to move and which is not.
    #[test]
    fn the_projection_is_invariant_under_tick_suppression() {
        let with = projected(true, true);
        let without = projected(false, true);

        assert_eq!(
            with, without,
            "suppressing the heartbeat changed the projected schedule"
        );
        assert_eq!(
            with.ticks, 0,
            "a tick reached the sink through the projection"
        );
        assert_eq!(with.switches, 4, "the switches are what must survive");
    }

    /// And it is not invariant under anything else -- the poison for the test
    /// above. Take one scheduling decision away and the projection says so.
    #[test]
    fn the_projection_is_not_invariant_under_a_lost_switch() {
        let whole = projected(true, true);
        let poisoned = projected(true, false);

        assert_ne!(
            whole, poisoned,
            "a dropped context switch survived the projection, which would              make the tickless gate blind to the thing it exists to catch"
        );
    }

    /// The boundary, named rather than implied: three events are suppressible
    /// and every other kind goes through. A variant added on the wrong side of
    /// this line would silently widen what a tickless port is allowed to
    /// change.
    #[test]
    fn exactly_three_event_kinds_are_suppressible() {
        let t: TaskHandle = Handle::from_parts(1, 1);

        for event in [
            Event::TaskIncrementTick { tick: 0 },
            Event::LowPowerIdleBegin,
            Event::LowPowerIdleEnd,
        ] {
            assert!(
                is_suppressible(&event),
                "{} should be suppressible",
                event.name()
            );
        }

        for event in [
            Event::StartingScheduler,
            Event::TaskSwitchedIn { task: t, name: "A" },
            Event::TaskSwitchedOut { task: t, name: "A" },
            Event::MovedTaskToReadyState { task: t, name: "A" },
            Event::MovedTaskToDelayedList { task: t, name: "A" },
            Event::TaskDelete { task: t, name: "A" },
            Event::TaskDelay {
                task: t,
                name: "A",
                ticks: 1,
            },
        ] {
            assert!(
                !is_suppressible(&event),
                "{} is part of the schedule and must not be suppressible",
                event.name()
            );
        }
    }

    #[test]
    fn names_are_the_c_macro_suffixes() {
        let t: TaskHandle = Handle::from_parts(1, 1);
        let e = Event::TaskSwitchedIn {
            task: t,
            name: "IDLE",
        };
        assert_eq!(e.name(), "TASK_SWITCHED_IN");
        assert_eq!(e.subject(), Some("IDLE"));
        assert_eq!(Event::TaskIncrementTick { tick: 3 }.subject(), None);
        assert_eq!(Event::StartingScheduler.name(), "STARTING_SCHEDULER");
    }

    #[test]
    fn the_counting_sink_counts() {
        let t: TaskHandle = Handle::from_parts(1, 1);
        let mut c = CountTrace::default();
        c.event(0, Event::StartingScheduler);
        c.event(0, Event::TaskSwitchedIn { task: t, name: "a" });
        c.event(1, Event::TaskIncrementTick { tick: 1 });
        c.event(1, Event::TaskSwitchedIn { task: t, name: "b" });
        assert_eq!((c.events, c.switches, c.ticks), (4, 2, 1));
        NoTrace.event(0, Event::LowPowerIdleEnd);
    }
}
