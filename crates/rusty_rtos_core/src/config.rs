//! `FreeRTOSConfig.h` as a type.
//!
//! Every geometry knob is an associated constant on [`Config`]; the kernel is
//! generic over it, so a wrong value is a compile error and a change is a
//! visible diff. Subsystems are cargo features on the kernel, hooks are the
//! [`crate::hooks::Hooks`] trait, and the classification of all 94 upstream
//! knobs is `docs/CONFIG-MAP.md` in the umbrella.
//!
//! [`DefaultConfig`] mirrors the upstream
//! `examples/template_configuration/FreeRTOSConfig.h` value for value.

use crate::error::{Error, Result};
use crate::tick::{Bits32, Bits64, TickWidth};

/// The kernel configuration, as constants a type carries.
///
/// Implement it on a unit struct; [`Config::validate`] is the checked form of
/// the `#error` lines in `FreeRTOS.h` and the kernel runs it before it starts.
pub trait Config {
    /// `configTICK_TYPE_WIDTH_IN_BITS`.
    type Tick: TickWidth;

    /// `configTICK_RATE_HZ`.
    const TICK_RATE_HZ: u32;
    /// `configINITIAL_TICK_COUNT`.
    const INITIAL_TICK_COUNT: u64 = 0;
    /// `configMAX_PRIORITIES`.
    const MAX_PRIORITIES: u8;
    /// `configMINIMAL_STACK_SIZE`, in words.
    const MINIMAL_STACK_SIZE: usize;
    /// `configMAX_TASK_NAME_LEN`, including the terminator the C kernel keeps.
    const MAX_TASK_NAME_LEN: usize;
    /// `configUSE_PREEMPTION`.
    const USE_PREEMPTION: bool = true;
    /// `configUSE_TIME_SLICING`.
    const USE_TIME_SLICING: bool = true;
    /// `configIDLE_SHOULD_YIELD`.
    const IDLE_SHOULD_YIELD: bool = true;

    /// `configUSE_TICK_HOOK`: whether the kernel calls the tick hook from
    /// inside `xTaskIncrementTick`.
    ///
    /// It is a config flag rather than "install a hook and it runs" because
    /// the C compiles the call out entirely, and a call that is compiled out
    /// cannot be where a `FromISR` call happens. Both of the C's call sites
    /// are mirrored: the one guarded by `xPendedTicks == 0`, and the one in
    /// the branch that only pends the tick.
    const USE_TICK_HOOK: bool = false;

    /// `configUSE_TIMERS`: whether the software timer daemon exists.
    const USE_TIMERS: bool = true;

    /// `sizeof( configMESSAGE_BUFFER_LENGTH_TYPE )`: how many bytes a
    /// message buffer spends on each message's length prefix.
    ///
    /// The C defaults the type to `size_t`, so this is the *host's* pointer
    /// width — eight on the machine the oracle runs on, four on a 32-bit
    /// chip. It is in the arithmetic of every message send, so a
    /// configuration that claims to match a given kernel has to say which.
    const MESSAGE_LENGTH_BYTES: usize = 4;
    /// `configTASK_NOTIFICATION_ARRAY_ENTRIES`.
    const NOTIFICATION_ARRAY_ENTRIES: usize = 1;
    /// `configNUM_THREAD_LOCAL_STORAGE_POINTERS`.
    const NUM_TLS_POINTERS: usize = 0;
    /// `configQUEUE_REGISTRY_SIZE`.
    const QUEUE_REGISTRY_SIZE: usize = 0;
    /// `configTIMER_TASK_PRIORITY`.
    const TIMER_TASK_PRIORITY: u8;
    /// `configTIMER_QUEUE_LENGTH`.
    const TIMER_QUEUE_LENGTH: usize = 10;
    /// `configTIMER_TASK_STACK_DEPTH`, in words.
    const TIMER_TASK_STACK_DEPTH: usize;
    /// `configCHECK_FOR_STACK_OVERFLOW`: 0 off, 1 the stack-pointer method,
    /// 2 the pattern method.
    const CHECK_FOR_STACK_OVERFLOW: u8 = 2;
    /// `configNUMBER_OF_CORES`.
    const NUMBER_OF_CORES: u8 = 1;
    /// `configEXPECTED_IDLE_TIME_BEFORE_SLEEP`, in ticks (tickless idle).
    const EXPECTED_IDLE_TIME_BEFORE_SLEEP: u64 = 2;
    /// `configTOTAL_HEAP_SIZE`, for `rusty_rtos_heap`'s region, in bytes.
    const TOTAL_HEAP_SIZE: usize = 4096;

    /// Whether this configuration mirrors a C kernel that takes kernel
    /// objects from a heap.
    ///
    /// This kernel never allocates — every object comes from an arena
    /// fixed at compile time — so the flag buys no memory and changes no
    /// behaviour. What it buys is *time*. Every `heap_N.c` wraps its
    /// `malloc` in `vTaskSuspendAll()` / `xTaskResumeAll()`, and
    /// `xTaskResumeAll` is a critical section, so on the C side creating a
    /// queue or a task after the scheduler has started costs one more
    /// outermost critical-section exit than creating it before. A
    /// configuration that claims to be trace-identical to such a kernel has
    /// to spend that exit too.
    ///
    /// Leave it `false` for silicon: a static build has no heap and the
    /// exit is not there to spend.
    const DYNAMIC_ALLOCATION: bool = false;

    // ----- Kairos-only: arena capacities (no C equivalent; see CONFIG-MAP) --

    /// How many tasks may exist at once, the idle and timer tasks included.
    const MAX_TASKS: usize = 16;
    /// How many queues, semaphores and mutexes may exist at once.
    const MAX_QUEUES: usize = 16;
    /// How many software timers may exist at once.
    const MAX_TIMERS: usize = 8;
    /// How many event groups may exist at once.
    const MAX_EVENT_GROUPS: usize = 4;
    /// How many stream and message buffers may exist at once.
    const MAX_STREAM_BUFFERS: usize = 4;

    /// The checks `FreeRTOS.h` makes with `#error`, as a value.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] naming nothing more specific; the doc of
    /// each constant says what must hold. The kernel refuses to start on
    /// `Err`.
    fn validate() -> Result<()> {
        let ok = Self::TICK_RATE_HZ > 0
            && Self::MAX_PRIORITIES >= 1
            && Self::TIMER_TASK_PRIORITY < Self::MAX_PRIORITIES
            && Self::MINIMAL_STACK_SIZE > 0
            && Self::TIMER_TASK_STACK_DEPTH >= Self::MINIMAL_STACK_SIZE
            && Self::MAX_TASK_NAME_LEN >= 1
            && Self::NOTIFICATION_ARRAY_ENTRIES >= 1
            // The kernel's per-task arrays are sized at a fixed maximum
            // because stable Rust cannot size one from an associated
            // const; asking for more would silently lose slots.
            && Self::NOTIFICATION_ARRAY_ENTRIES <= 4
            // The command ring beside the timer queue is a fixed maximum,
            // for the same reason the notification arrays are.
            && Self::TIMER_QUEUE_LENGTH <= 32
            && Self::TIMER_QUEUE_LENGTH >= 1
            && Self::CHECK_FOR_STACK_OVERFLOW <= 2
            && Self::NUMBER_OF_CORES >= 1
            && Self::MAX_TASKS >= 2
            && Self::INITIAL_TICK_COUNT <= <Self::Tick as TickWidth>::MAX
            && Self::MAX_TASKS <= 0x7FFF
            && Self::MAX_QUEUES <= 0x7FFF
            && Self::MAX_TIMERS <= 0x7FFF
            && Self::MAX_EVENT_GROUPS <= 0x7FFF
            && Self::MAX_STREAM_BUFFERS <= 0x7FFF;
        if ok {
            Ok(())
        } else {
            Err(Error::InvalidArgument)
        }
    }

    /// `pdMS_TO_TICKS(ms)`: `ms * TICK_RATE_HZ / 1000`, floored, saturating
    /// at the tick width's maximum.
    #[must_use]
    fn ms_to_ticks(ms: u64) -> u64 {
        let hz = u64::from(Self::TICK_RATE_HZ);
        let ticks = ms
            .checked_mul(hz)
            .map_or(<Self::Tick as TickWidth>::MAX, |n| n / 1000);
        ticks.min(<Self::Tick as TickWidth>::MAX)
    }

    /// `pdTICKS_TO_MS(ticks)`: `ticks * 1000 / TICK_RATE_HZ`, floored,
    /// saturating.
    #[must_use]
    fn ticks_to_ms(ticks: u64) -> u64 {
        let hz = u64::from(Self::TICK_RATE_HZ).max(1);
        ticks
            .checked_mul(1000)
            .map_or(u64::MAX, |n| n.checked_div(hz).unwrap_or(0))
    }
}

/// The upstream template configuration
/// (`examples/template_configuration/FreeRTOSConfig.h`, V11.3.1), value for
/// value: 100 Hz, 5 priorities, 128-word stacks, 16-character names, 32-bit
/// ticks, preemption on, time slicing off, the timer task at the highest
/// priority with a 10-deep queue, stack overflow method 2, a 4 KiB heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DefaultConfig;

impl Config for DefaultConfig {
    type Tick = Bits32;
    const TICK_RATE_HZ: u32 = 100;
    const MAX_PRIORITIES: u8 = 5;
    const MINIMAL_STACK_SIZE: usize = 128;
    const MAX_TASK_NAME_LEN: usize = 16;
    const USE_TIME_SLICING: bool = false;
    const TIMER_TASK_PRIORITY: u8 = 4;
    const TIMER_TASK_STACK_DEPTH: usize = 128;
}

/// The `Posix_GCC` demo's configuration, which the C oracle harness uses:
/// 1000 Hz, 7 priorities, 12-character names, a 20-deep timer queue, a
/// 20-entry queue registry (so every queue has a name in the trace), time
/// slicing on, stack overflow checking off, a 65 KiB heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PosixDemoConfig;

impl Config for PosixDemoConfig {
    /// The Posix port types `TickType_t` as `unsigned long`, 64 bits on the
    /// x86_64 Linux host the oracle runs on; `portMAX_DELAY` prints as 2^64-1.
    type Tick = Bits64;
    const TICK_RATE_HZ: u32 = 1000;
    const DYNAMIC_ALLOCATION: bool = true;
    const MAX_PRIORITIES: u8 = 7;
    const MINIMAL_STACK_SIZE: usize = 128;
    const MAX_TASK_NAME_LEN: usize = 12;
    const QUEUE_REGISTRY_SIZE: usize = 20;
    const TIMER_TASK_PRIORITY: u8 = 6;
    const TIMER_QUEUE_LENGTH: usize = 20;
    const TIMER_TASK_STACK_DEPTH: usize = 256;
    const CHECK_FOR_STACK_OVERFLOW: u8 = 0;
    const USE_TICK_HOOK: bool = true;
    const NOTIFICATION_ARRAY_ENTRIES: usize = 3;
    // The oracle host is x86-64, so `size_t` is eight bytes wide.
    const MESSAGE_LENGTH_BYTES: usize = 8;
    const TOTAL_HEAP_SIZE: usize = 65 * 1024;
    const MAX_TASKS: usize = 64;
    const MAX_QUEUES: usize = 64;
    const MAX_TIMERS: usize = 32;
    const MAX_EVENT_GROUPS: usize = 8;
    const MAX_STREAM_BUFFERS: usize = 16;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_default_profile_is_the_upstream_template_value_for_value() {
        // Pinned on purpose: moving any of these is a conscious act with a diff.
        assert_eq!(DefaultConfig::TICK_RATE_HZ, 100);
        assert_eq!(DefaultConfig::MAX_PRIORITIES, 5);
        assert_eq!(DefaultConfig::MINIMAL_STACK_SIZE, 128);
        assert_eq!(DefaultConfig::MAX_TASK_NAME_LEN, 16);
        const { assert!(DefaultConfig::USE_PREEMPTION) };
        const { assert!(!DefaultConfig::USE_TIME_SLICING) };
        const { assert!(DefaultConfig::IDLE_SHOULD_YIELD) };
        assert_eq!(DefaultConfig::NOTIFICATION_ARRAY_ENTRIES, 1);
        assert_eq!(DefaultConfig::QUEUE_REGISTRY_SIZE, 0);
        assert_eq!(DefaultConfig::NUM_TLS_POINTERS, 0);
        assert_eq!(DefaultConfig::TIMER_TASK_PRIORITY, 4);
        assert_eq!(DefaultConfig::TIMER_QUEUE_LENGTH, 10);
        assert_eq!(DefaultConfig::TIMER_TASK_STACK_DEPTH, 128);
        assert_eq!(DefaultConfig::CHECK_FOR_STACK_OVERFLOW, 2);
        assert_eq!(DefaultConfig::TOTAL_HEAP_SIZE, 4096);
        assert_eq!(<DefaultConfig as Config>::Tick::BITS, 32);
        DefaultConfig::validate().unwrap();
        PosixDemoConfig::validate().unwrap();
    }

    #[test]
    fn tick_conversions_floor_like_the_c_macros() {
        // pdMS_TO_TICKS at 100 Hz: 15 ms -> 1 tick (floor), 20 ms -> 2.
        assert_eq!(DefaultConfig::ms_to_ticks(15), 1);
        assert_eq!(DefaultConfig::ms_to_ticks(20), 2);
        assert_eq!(DefaultConfig::ms_to_ticks(0), 0);
        assert_eq!(PosixDemoConfig::ms_to_ticks(7), 7);
        assert_eq!(DefaultConfig::ticks_to_ms(3), 30);
        // Saturation, never a panic or a wrap.
        assert_eq!(DefaultConfig::ms_to_ticks(u64::MAX), u64::from(u32::MAX));
        assert_eq!(DefaultConfig::ticks_to_ms(u64::MAX), u64::MAX);
    }

    struct Broken;
    impl Config for Broken {
        type Tick = Bits32;
        const TICK_RATE_HZ: u32 = 100;
        const MAX_PRIORITIES: u8 = 3;
        const MINIMAL_STACK_SIZE: usize = 64;
        const MAX_TASK_NAME_LEN: usize = 8;
        const TIMER_TASK_PRIORITY: u8 = 3; // == MAX_PRIORITIES: the #error case
        const TIMER_TASK_STACK_DEPTH: usize = 64;
    }

    #[test]
    fn validate_refuses_what_freertos_h_errors_on() {
        assert_eq!(Broken::validate(), Err(Error::InvalidArgument));
    }
}
