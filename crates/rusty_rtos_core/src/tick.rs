//! The tick count, with its width as a type.
//!
//! FreeRTOS's `TickType_t` is 16, 32 or 64 bits wide
//! (`configTICK_TYPE_WIDTH_IN_BITS`), and the kernel's delayed-list overflow
//! handling depends on the count wrapping at exactly that width. A width that
//! is a type parameter cannot be transcribed wrongly in one place and right in
//! another; `Tick<Bits16>` wraps at 65 535 on the host exactly as it does on
//! a chip, which is where the wrap tests live.

use core::fmt;
use core::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}

/// A tick width: 16, 32 or 64 bits. Sealed; the three widths are the whole set.
pub trait TickWidth: sealed::Sealed + Copy + Clone + fmt::Debug + PartialEq + Eq + 'static {
    /// The width in bits.
    const BITS: u32;
    /// The largest representable count (`portMAX_DELAY` at this width).
    const MAX: u64;
}

/// `TICK_TYPE_WIDTH_16_BITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bits16;
/// `TICK_TYPE_WIDTH_32_BITS` — the common choice and the default profile's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bits32;
/// `TICK_TYPE_WIDTH_64_BITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bits64;

impl sealed::Sealed for Bits16 {}
impl sealed::Sealed for Bits32 {}
impl sealed::Sealed for Bits64 {}

impl TickWidth for Bits16 {
    const BITS: u32 = 16;
    const MAX: u64 = u16::MAX as u64;
}
impl TickWidth for Bits32 {
    const BITS: u32 = 32;
    const MAX: u64 = u32::MAX as u64;
}
impl TickWidth for Bits64 {
    const BITS: u32 = 64;
    const MAX: u64 = u64::MAX;
}

/// A tick count of width `W`: monotonic since scheduler start, wrapping at
/// `W::MAX` exactly as the C kernel's `xTickCount` does.
///
/// Stored as a `u64` masked to the width, so the same code path serves every
/// width and every arithmetic test can be run at all three on the host.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick<W: TickWidth> {
    raw: u64,
    width: PhantomData<W>,
}

impl<W: TickWidth> Tick<W> {
    /// Tick zero.
    pub const ZERO: Self = Self::new(0);
    /// The largest count; also `portMAX_DELAY`, the "forever" timeout.
    pub const MAX: Self = Self::new(W::MAX);

    /// A count, masked to the width (a value above `W::MAX` wraps).
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self {
            raw: raw & W::MAX,
            width: PhantomData,
        }
    }

    /// The count as a number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.raw
    }

    /// The width in bits.
    #[must_use]
    pub const fn bits() -> u32 {
        W::BITS
    }

    /// `self + n`, wrapping at the width (the tick interrupt's increment).
    #[must_use]
    pub const fn wrapping_add(self, n: u64) -> Self {
        Self::new(self.raw.wrapping_add(n))
    }

    /// `self - n`, wrapping at the width.
    #[must_use]
    pub const fn wrapping_sub(self, n: u64) -> Self {
        Self::new(self.raw.wrapping_sub(n))
    }

    /// `self + n`, or `None` if the result would wrap. Blocking APIs use this
    /// to know whether a wake time lands in the overflow delayed list.
    #[must_use]
    pub const fn checked_add(self, n: u64) -> Option<Self> {
        let sum = self.raw.wrapping_add(n) & W::MAX;
        if sum < self.raw || n > W::MAX {
            None
        } else {
            Some(Self::new(sum))
        }
    }

    /// Ticks elapsed from `earlier` to `self`, wrapping (the C kernel's
    /// `xTickCount - xTimeOnEntering` idiom).
    #[must_use]
    pub const fn since(self, earlier: Self) -> u64 {
        self.raw.wrapping_sub(earlier.raw) & W::MAX
    }

    /// Whether adding `n` to `self` crosses the wrap point (the wake time
    /// belongs in the overflow delayed list).
    #[must_use]
    pub const fn overflows_by(self, n: u64) -> bool {
        self.checked_add(n).is_none()
    }
}

impl<W: TickWidth> Default for Tick<W> {
    fn default() -> Self {
        Self::ZERO
    }
}

impl<W: TickWidth> fmt::Debug for Tick<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tick<{}>({})", W::BITS, self.raw)
    }
}

impl<W: TickWidth> fmt::Display for Tick<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn widths_wrap_where_the_c_kernel_does() {
        assert_eq!(Tick::<Bits16>::MAX.get(), 65_535);
        assert_eq!(Tick::<Bits16>::MAX.wrapping_add(1), Tick::ZERO);
        assert_eq!(Tick::<Bits32>::MAX.get(), 4_294_967_295);
        assert_eq!(Tick::<Bits32>::MAX.wrapping_add(1), Tick::ZERO);
        assert_eq!(Tick::<Bits64>::MAX.wrapping_add(1), Tick::ZERO);
        assert_eq!(Tick::<Bits16>::new(70_000).get(), 70_000 & 0xFFFF);
    }

    #[test]
    fn since_survives_the_wrap() {
        let before = Tick::<Bits16>::new(65_530);
        let after = before.wrapping_add(10);
        assert_eq!(after.get(), 4);
        assert_eq!(after.since(before), 10);
        assert_eq!(Tick::<Bits32>::ZERO.since(Tick::MAX), 1);
    }

    #[test]
    fn checked_add_names_the_overflow_list() {
        let t = Tick::<Bits16>::new(65_000);
        assert_eq!(t.checked_add(535).map(Tick::get), Some(65_535));
        assert!(t.checked_add(536).is_none());
        assert!(t.overflows_by(536));
        assert!(!t.overflows_by(535));
        assert!(Tick::<Bits32>::ZERO.checked_add(u64::MAX).is_none());
        assert_eq!(
            Tick::<Bits64>::ZERO.checked_add(u64::MAX).map(Tick::get),
            Some(u64::MAX)
        );
    }

    #[test]
    fn a_pinned_value_so_moving_it_is_a_diff() {
        // The default profile's width, asserted so a change is a conscious act.
        assert_eq!(Tick::<Bits32>::bits(), 32);
        assert_eq!(core::mem::size_of::<Tick<Bits16>>(), 8);
    }
}
