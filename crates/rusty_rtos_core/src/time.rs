//! Durations and timeouts in the units a caller thinks in.
//!
//! The C API takes a `TickType_t` everywhere and expects the caller to
//! convert with `pdMS_TO_TICKS`. Here a [`Duration`] is milliseconds or
//! microseconds until it meets a [`Config`], and a [`Timeout`] says which of
//! the three things a blocking call can mean: do not block, block for so
//! many ticks, or block forever (`portMAX_DELAY`).

use crate::config::Config;
use crate::tick::TickWidth;

/// A span of wall time, converted to ticks against a configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
    micros: u64,
}

impl Duration {
    /// Zero.
    pub const ZERO: Self = Self { micros: 0 };

    /// `ms` milliseconds (saturating).
    #[must_use]
    pub const fn millis(ms: u64) -> Self {
        Self {
            micros: ms.saturating_mul(1000),
        }
    }

    /// `us` microseconds.
    #[must_use]
    pub const fn micros(us: u64) -> Self {
        Self { micros: us }
    }

    /// `s` seconds (saturating).
    #[must_use]
    pub const fn secs(s: u64) -> Self {
        Self {
            micros: s.saturating_mul(1_000_000),
        }
    }

    /// The span in microseconds.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.micros
    }

    /// The span in whole milliseconds, floored.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.micros / 1000
    }

    /// The span in ticks of `C`, floored like `pdMS_TO_TICKS`, saturating at
    /// the tick width's maximum. A non-zero span shorter than a tick is
    /// zero ticks, exactly as in C — a caller who needs "at least one" adds it.
    #[must_use]
    pub fn to_ticks<C: Config>(self) -> u64 {
        let hz = u64::from(C::TICK_RATE_HZ);
        let max = <C::Tick as TickWidth>::MAX;
        // micros * hz / 1_000_000, without overflowing on the way.
        let whole_secs = self.micros / 1_000_000;
        let rem_micros = self.micros % 1_000_000;
        let from_secs = whole_secs.checked_mul(hz);
        let from_rem = rem_micros.checked_mul(hz).map(|n| n / 1_000_000);
        match (from_secs, from_rem) {
            (Some(a), Some(b)) => a.saturating_add(b).min(max),
            _ => max,
        }
    }
}

/// How long a blocking call may wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Timeout {
    /// Do not block: a tick count of `0` in C.
    #[default]
    None,
    /// Block for this many ticks at most.
    Ticks(u64),
    /// Block until the condition holds: `portMAX_DELAY` in C.
    Forever,
}

impl Timeout {
    /// The C-side tick count: `0`, the count, or the width's maximum.
    #[must_use]
    pub fn to_ticks<C: Config>(self) -> u64 {
        let max = <C::Tick as TickWidth>::MAX;
        match self {
            Self::None => 0,
            Self::Ticks(n) => n.min(max),
            Self::Forever => max,
        }
    }

    /// From a C-side tick count: `0` is [`Timeout::None`], the width's
    /// maximum is [`Timeout::Forever`], anything else is that many ticks.
    #[must_use]
    pub fn from_ticks<C: Config>(ticks: u64) -> Self {
        let max = <C::Tick as TickWidth>::MAX;
        if ticks == 0 {
            Self::None
        } else if ticks >= max {
            Self::Forever
        } else {
            Self::Ticks(ticks)
        }
    }

    /// A timeout of `d`, converted against `C`.
    #[must_use]
    pub fn after<C: Config>(d: Duration) -> Self {
        Self::from_ticks::<C>(d.to_ticks::<C>())
    }

    /// Whether the call may block at all.
    #[must_use]
    pub const fn blocks(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::config::{DefaultConfig, PosixDemoConfig};

    #[test]
    fn durations_convert_like_pd_ms_to_ticks() {
        assert_eq!(Duration::millis(10).to_ticks::<DefaultConfig>(), 1);
        assert_eq!(Duration::millis(15).to_ticks::<DefaultConfig>(), 1);
        assert_eq!(Duration::millis(250).to_ticks::<PosixDemoConfig>(), 250);
        assert_eq!(Duration::micros(999).to_ticks::<PosixDemoConfig>(), 0);
        assert_eq!(Duration::secs(1).to_ticks::<DefaultConfig>(), 100);
        assert_eq!(
            Duration::secs(u64::MAX).to_ticks::<DefaultConfig>(),
            u64::from(u32::MAX)
        );
        assert_eq!(Duration::millis(1).as_micros(), 1000);
        assert_eq!(Duration::micros(2500).as_millis(), 2);
    }

    #[test]
    fn timeouts_round_trip_the_c_convention() {
        assert_eq!(Timeout::None.to_ticks::<DefaultConfig>(), 0);
        assert_eq!(
            Timeout::Forever.to_ticks::<DefaultConfig>(),
            u64::from(u32::MAX)
        );
        assert_eq!(Timeout::Ticks(5).to_ticks::<DefaultConfig>(), 5);
        assert_eq!(Timeout::from_ticks::<DefaultConfig>(0), Timeout::None);
        assert_eq!(
            Timeout::from_ticks::<DefaultConfig>(u64::from(u32::MAX)),
            Timeout::Forever
        );
        assert_eq!(
            Timeout::from_ticks::<DefaultConfig>(u64::MAX),
            Timeout::Forever
        );
        assert_eq!(Timeout::from_ticks::<DefaultConfig>(7), Timeout::Ticks(7));
        assert_eq!(
            Timeout::after::<DefaultConfig>(Duration::millis(5)),
            Timeout::None
        );
        assert!(Timeout::Ticks(1).blocks());
        assert!(!Timeout::None.blocks());
    }
}
