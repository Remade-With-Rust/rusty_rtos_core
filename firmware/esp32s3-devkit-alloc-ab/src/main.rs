//! **The A/B `build-me-bare` B4b asks for**: `rusty_alloc`'s `small-metal`
//! seam against FreeRTOS's own `heap_4`, in CPU cycles, on silicon.
//!
//! The sibling `esp32s3-devkit-alloc-cycles` cell measured one arm and
//! said so in every place it could. This is the other arm, and the thing
//! that was missing was never the board — it was the second allocator.
//! `heap_4.c` is portable C over a static byte array with no port
//! dependency at all, so it compiles for this part with the esp
//! toolchain's `xtensa-esp-elf-gcc` and runs under the identical harness.
//!
//! # What makes it an A/B and not two numbers
//!
//! * **The C arm is FreeRTOS's file, verbatim.** `build.rs` compiles
//!   `oracle/FreeRTOS-Kernel/portable/MemMang/heap_4.c` straight out of
//!   the oracle checkout — the same kernel the conformance traces come
//!   from. It is not copied into this tree and not edited. Only its
//!   includes are supplied, by three shims in `csrc/` that define exactly
//!   what it reads.
//! * **Same memory.** Both allocators get **196,608 usable bytes**:
//!   `good_region_size(220 KiB)` for the Rust arm, `configTOTAL_HEAP_SIZE`
//!   for the C one.
//! * **Same call shape.** Both are called *directly* —
//!   `alloc::alloc::alloc(Layout)` / `dealloc` against `pvPortMalloc` /
//!   `vPortFree` — at the same 8-byte alignment, which is
//!   `portBYTE_ALIGNMENT` on this ABI. No `Vec` on one side and raw calls
//!   on the other.
//! * **Symmetric locking, which means none.** `heap_4` brackets its work
//!   in `vTaskSuspendAll`/`xTaskResumeAll`; the shims make those no-ops,
//!   because the Rust arm is built `--cfg ra_single_threaded` and takes no
//!   lock either. A C arm paying for a scheduler lock the Rust arm does
//!   not pay for would be measuring the lock.
//! * **`configASSERT` traps.** A no-op assert would let `heap_4` violate
//!   its own invariants silently and still produce a cycle number, which
//!   is the shape of a benchmark that measures nothing.
//! * **The arms are interleaved (ABBA), not blocked.** Running all of one
//!   then all of the other puts any drift *between* the blocks.
//! * **A null A/B.** Before comparing anything, the harness runs the Rust
//!   arm against *itself* as if it were two different allocators. Whatever
//!   difference that reports is the floor, and no verdict below it counts.
//! * **Work parity is checked, not assumed.** Both arms compute the same
//!   checksum from the same sequence, and the run FAILS if they differ.
//! * **What `heap_4` CHARGES is measured too — and the gap where the
//!   other half should be is stated.** Cycles are half the trade: an
//!   allocator that is faster and wastes more memory has not won.
//!   `heap_4`'s charge per request is read from `xPortGetFreeHeapSize()`
//!   either side of one allocation, so it is measured rather than derived
//!   from `xHeapStructSize`. There is **no comparable number for the Rust
//!   arm**, because the seam does not expose a per-allocation usable size
//!   and `region_stats()` does not move for a small allocation. The
//!   column is therefore labelled as heap_4's alone rather than being
//!   quietly turned into a comparison.
//!
//! # What it still does not close
//!
//! B4b's kill test names the **ESP32-C6** — `riscv32imac`, a Kairos
//! target with a real `mcycle`. Xtensa is not one of the family's four
//! targets, so this is the A/B on silicon that is *not* a Kairos part.
//! It closes the comparison and leaves the target clause open, the same
//! way B4a substituted `mps2-an385` for the `lm3s6965evb` the plan named
//! and recorded why.

#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::Layout;

use esp_backtrace as _;
use esp_println::println;

use rusty_rtos_alloc::small_metal::{Region, good_region_size, region_contains};

esp_bootloader_esp_idf::esp_app_desc!();

#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;

/// The Rust arm's memory, matched to `configTOTAL_HEAP_SIZE` in
/// `csrc/FreeRTOSConfig.h`. Both usable figures are printed, so a drift
/// between the two is visible rather than silent.
///
/// 64 KiB and not the 192 KiB the single-arm cell used, because two
/// 192 KiB static heaps do not fit in the S3's DRAM. It costs the
/// measurement nothing — the harness holds exactly ONE allocation live at
/// a time, so heap size is not on the measured path — and that claim is
/// checked rather than asserted: the Rust arm's numbers here should equal
/// the 192 KiB cell's published ones.
const BUDGET: usize = 64 * 1024;
type Heap = Region<{ good_region_size(BUDGET) }>;
static HEAP: Heap = Heap::new();

unsafe extern "C" {
    fn pvPortMalloc(size: usize) -> *mut u8;
    fn vPortFree(p: *mut u8);
    fn xPortGetFreeHeapSize() -> usize;
}

/// `configASSERT`'s target. It must trap: see the module docs.
#[unsafe(no_mangle)]
extern "C" fn kairos_heap4_assert_failed() {
    panic!("heap_4 configASSERT failed");
}

/// Operations per measured round.
const OPS: usize = 256;
/// Rounds; the answer for each arm is the best of them.
const ROUNDS: usize = 32;
/// Rounds run before any is counted, to warm the instruction cache.
const WARMUP: usize = 8;
/// `portBYTE_ALIGNMENT` from `csrc/FreeRTOSConfig.h`. Both arms request it.
const ALIGN: usize = 8;
/// Most interleaved blocks the fragmentation sweep will hold live.
const MAX_HELD: usize = 256;

#[inline(always)]
fn ccount() -> u32 {
    xtensa_lx::timer::get_cycle_count()
}

/// The two arms are the SAME function with one line different. Writing
/// them from one macro is what stops a difference creeping into the
/// harness instead of the allocator.
macro_rules! round {
    ($name:ident, $alloc:expr, $free:expr) => {
        /// `holes` interleaved live blocks are placed first, so the
        /// allocator is asked to work against a FRAGMENTED free list
        /// rather than an empty one. `holes == 0` is the clean case.
        ///
        /// This exists because the clean case is `heap_4`'s best: with one
        /// block live its free list is one entry, so its first-fit walk is
        /// O(1) and its cost comes out flat at every size. Quoting that as
        /// "heap_4's number" would flatter it in exactly the way that
        /// matters, since the walk is O(free-list length) by construction
        /// and `rusty_alloc`'s size-class pop is not.
        ///
        /// Twice `holes` blocks are taken and every other one released, so
        /// no two free blocks are adjacent and `heap_4` cannot coalesce
        /// them back into one. That is what makes the list actually long.
        ///
        /// **The holes must be SMALLER than the request, and the first
        /// version of this got that wrong.** Fragmenting with same-size
        /// holes made heap_4 measurably FASTER (236 -> 218 cycles), not
        /// slower, because first-fit stops at the first block that fits
        /// and a same-size hole fits immediately. A long list only costs
        /// something when the allocator has to walk PAST it, so the holes
        /// are `hole_size` bytes and the timed request is `size`.
        #[inline(never)]
        fn $name(size: usize, holes: usize, hole_size: usize) -> (u32, u32, bool) {
            let mut sum = 0u32;
            let mut bad = false;
            let mut held = [core::ptr::null_mut::<u8>(); MAX_HELD];
            let taken = (holes * 2).min(MAX_HELD);
            for slot in held.iter_mut().take(taken) {
                let p: *mut u8 = $alloc(hole_size);
                if p.is_null() {
                    bad = true;
                }
                *slot = p;
            }
            let mut i = 0;
            while i < taken {
                if !held[i].is_null() {
                    $free(held[i], hole_size);
                    held[i] = core::ptr::null_mut();
                }
                i += 2;
            }

            let start = ccount();
            for i in 0..OPS {
                let p: *mut u8 = $alloc(size);
                if p.is_null() {
                    bad = true;
                    break;
                }
                // SAFETY: `p` is a live allocation of at least `size` >= 1.
                unsafe {
                    p.write((i & 0xff) as u8);
                    sum = sum.wrapping_add(u32::from(p.read()));
                }
                // Keep the pointer alive to the optimiser without letting
                // its VALUE into the checksum: the checksum has to be
                // derived from the work alone, or the two arms could not
                // be required to agree on it.
                core::hint::black_box(p);
                $free(p, size);
                sum = sum.wrapping_add((i as u32) & 1);
            }
            let elapsed = ccount().wrapping_sub(start);

            for slot in held.iter_mut().take(taken) {
                if !slot.is_null() {
                    $free(*slot, hole_size);
                    *slot = core::ptr::null_mut();
                }
            }
            (elapsed, sum, bad)
        }
    };
}

round!(
    round_rust,
    |size| unsafe { alloc::alloc::alloc(Layout::from_size_align_unchecked(size, ALIGN)) },
    |p, size| unsafe {
        alloc::alloc::dealloc(p, Layout::from_size_align_unchecked(size, ALIGN))
    }
);

round!(
    round_c,
    |size| unsafe { pvPortMalloc(size) },
    |p, _size| unsafe { vPortFree(p) }
);

/// The null arm: the same loop with the allocation removed, so what is
/// reported is the allocator's own work and not the harness's.
#[inline(never)]
/// A FIXED-SIZE POOL, for the sizes an RTOS actually knows at compile time.
///
/// This is not a faster allocator — it is a narrower question. A pool serves
/// one block size from a pre-sized arena, so there is no size class to
/// compute, no bin, no page lookup: `alloc` is a pop and `free` is a push.
/// TCBs, queue items and timer records are all exactly that shape.
///
/// Counted on the host at **12.00 instructions per alloc/free pair against
/// 53.57 for the general path at 256 bytes** — and flat in size, which the
/// general path is not. This measures what that is worth in CYCLES on the
/// part, against the same `heap_4` every other row is measured against.
///
/// The bounds checks stay in. This is what a SAFE pool costs.
fn pool_round(size: usize) -> (u32, u32, bool) {
    const BLOCKS: usize = 8;
    const MAX_BLOCK: usize = 512;
    let mut arena = [0u8; MAX_BLOCK * BLOCKS];
    let mut free = [0usize; BLOCKS];
    let stride = size.min(MAX_BLOCK).max(1);
    for (i, slot) in free.iter_mut().enumerate() {
        *slot = i * stride;
    }
    let mut top = BLOCKS;
    let mut sum = 0u32;
    let mut bad = false;

    let start = ccount();
    for i in 0..OPS {
        // pop
        if top == 0 {
            bad = true;
            break;
        }
        top -= 1;
        let at = free[top];
        if let Some(slot) = arena.get_mut(at) {
            *slot = (i & 0xff) as u8;
            sum = sum.wrapping_add(u32::from(*slot));
        }
        core::hint::black_box(at);
        // push
        free[top] = at;
        top += 1;
        sum = sum.wrapping_add((i as u32) & 1);
    }
    (ccount().wrapping_sub(start), sum, bad)
}

/// Which HALF of an alloc/free pair the cycles are in.
///
/// The paired loop cannot say. Seven operations costing ~88 cycles is ~12
/// cycles each, which is far more than a load or a store retires in — so the
/// question is whether the cost is INSTRUCTIONS or dependent-load LATENCY,
/// and the first thing to know is where it sits.
///
/// Both halves are timed over blocks held live, so neither can be folded into
/// the other: `alloc` fills the array with nothing freed, `free` drains an
/// array already full. `best of ROUNDS`, like everything else here.
fn split_alloc_free(size: usize) -> (u32, u32) {
    let mut best_a = u32::MAX;
    let mut best_f = u32::MAX;
    let mut held = [core::ptr::null_mut::<u8>(); MAX_HELD];
    // Fewer than OPS: 256 live blocks of 512 bytes would not fit the region.
    let n = MAX_HELD.min((32 * 1024) / size.max(1)).min(OPS);

    for _ in 0..(WARMUP + ROUNDS) {
        let start = ccount();
        for slot in held.iter_mut().take(n) {
            // SAFETY: as the paired loop — size/align are a valid layout.
            *slot = unsafe {
                alloc::alloc::alloc(Layout::from_size_align_unchecked(size, ALIGN))
            };
        }
        let a = ccount().wrapping_sub(start);

        let start = ccount();
        for slot in held.iter_mut().take(n) {
            if !slot.is_null() {
                // SAFETY: each came from the loop above and is freed once.
                unsafe {
                    alloc::alloc::dealloc(*slot, Layout::from_size_align_unchecked(size, ALIGN));
                }
            }
        }
        let f = ccount().wrapping_sub(start);

        for slot in held.iter_mut().take(n) {
            *slot = core::ptr::null_mut();
        }
        if a < best_a {
            best_a = a;
        }
        if f < best_f {
            best_f = f;
        }
    }
    let n32 = u32::try_from(n).unwrap_or(1).max(1);
    (best_a / n32, best_f / n32)
}

fn round_empty() -> (u32, u32, bool) {
    let mut sum = 0u32;
    let start = ccount();
    for i in 0..OPS {
        let x = core::hint::black_box((i & 0xff) as u8);
        sum = sum.wrapping_add(u32::from(x));
        sum = sum.wrapping_add((i as u32) & 1);
    }
    (ccount().wrapping_sub(start), sum, false)
}

/// Best of `ROUNDS` for two arms, **interleaved**. Each round runs A then
/// B, so anything that drifts over the run drifts through both equally.
fn best_pair<A, B>(mut a: A, mut b: B) -> (u32, u32, u32, u32, bool)
where
    A: FnMut() -> (u32, u32, bool),
    B: FnMut() -> (u32, u32, bool),
{
    for _ in 0..WARMUP {
        core::hint::black_box(a());
        core::hint::black_box(b());
    }
    let (mut lo_a, mut lo_b) = (u32::MAX, u32::MAX);
    let (mut sum_a, mut sum_b) = (0u32, 0u32);
    let mut bad = false;
    for _ in 0..ROUNDS {
        let (ca, sa, ba) = a();
        let (cb, sb, bb) = b();
        lo_a = lo_a.min(ca);
        lo_b = lo_b.min(cb);
        sum_a = sa;
        sum_b = sb;
        bad |= ba | bb;
    }
    (lo_a, lo_b, sum_a, sum_b, bad)
}

/// What `heap_4` charges for a request of `size`, measured rather than
/// derived: the heap's own free counter, either side of one allocation.
fn charge_c(size: usize) -> usize {
    // SAFETY: FFI to heap_4, single-threaded, and the block is freed.
    unsafe {
        let before = xPortGetFreeHeapSize();
        let p = pvPortMalloc(size);
        let after = xPortGetFreeHeapSize();
        vPortFree(p);
        before.saturating_sub(after)
    }
}

// The same question CANNOT be asked of the Rust arm through the seam.
//
// `rusty_alloc` has a public `alloc::usable_size(p)`, but
// `rusty_rtos_alloc`'s `small_metal` module re-exports only the fixed
// region API, and reaching around the seam to the crate underneath is the
// one thing the seam exists to prevent. `region_stats()` is not a
// substitute: it answers `(used, free, total)` over region EXTENTS, so it
// does not move for a small allocation at all — the same dead-check that
// was caught in this project's own S3 reclamation test, and not worth
// rebuilding here.
//
// So this cell reports what heap_4 charges, says plainly that it has no
// comparable number for the Rust arm, and records the gap. Half a
// comparison presented as a whole one is worse than none.

#[esp_hal::main]
fn main() -> ! {
    let _p = esp_hal::init(esp_hal::Config::default());

    println!();
    println!("=== rusty_alloc small-metal  vs  FreeRTOS heap_4  (ESP32-S3) ===");
    println!("counter   Xtensa CCOUNT, one tick per CPU cycle");
    println!("method    best of {ROUNDS} interleaved rounds of {OPS} ops, {WARMUP} warm-up,");
    println!("          an empty round measured the same way and subtracted");
    println!("C arm     oracle FreeRTOS-Kernel heap_4.c, compiled verbatim, -O2");
    // Derived from the build, not asserted -- see `build.rs`.
    println!(
        "Rust arm  rusty_alloc small-metal, opt-level={} + LTO, overflow-checks={}",
        env!("AB_OPT_LEVEL"),
        env!("AB_OVERFLOW_CHECKS")
    );
    println!("both      {ALIGN}-byte alignment, called directly, no lock either side");
    println!();

    let mut failed = 0u32;

    match HEAP.give() {
        Ok(usable) => {
            println!("memory    Rust arm {usable} bytes usable");
            // heap_4 builds its free list lazily, inside the first
            // `pvPortMalloc`. Asking before that reports 0, which is not
            // "no heap" but "no heap YET" -- so prime it first.
            let prime = unsafe { pvPortMalloc(16) };
            unsafe { vPortFree(prime) };
            println!("          C arm    {} bytes usable", unsafe {
                xPortGetFreeHeapSize()
            });
        }
        Err(_) => {
            println!("RESULT: FAIL -- the region was refused");
            loop {
                core::hint::spin_loop();
            }
        }
    }

    let (floor, _, _, _, _) = best_pair(round_empty, round_empty);
    println!(
        "null arm  {floor} cycles for {OPS} empty ops = {} cycles/op, subtracted",
        floor / OPS as u32
    );

    // The null A/B: the Rust arm against ITSELF, presented to the harness
    // as if it were two different allocators. Whatever this reports is the
    // resolution floor, and no verdict below it counts.
    let (na, nb, _, _, _) = best_pair(|| round_rust(64, 0, 16), || round_rust(64, 0, 16));
    let null_delta = na.abs_diff(nb);
    println!("null A/B  same allocator both arms at 64 B: {na} vs {nb}, delta {null_delta}");
    println!(
        "          = {} cycles/op of resolution. Below this, nothing counts.",
        null_delta / OPS as u32
    );
    println!();

    println!("     size   rust c/op    heap_4 c/op        ratio   heap_4 charged B");
    for size in [16usize, 32, 64, 128, 256, 504, 512, 513, 520, 640, 1024, 2048] {
        let (cr, cc, sr, sc, bad) = best_pair(|| round_rust(size, 0, 16), || round_c(size, 0, 16));
        if bad {
            failed += 1;
            println!("  {size:>7}  FAIL: an allocation returned null");
            continue;
        }
        // Work parity: the two arms ran the same sequence, so they must
        // agree on the checksum. If they do not, they did not do the same
        // work and the cycle numbers are not comparable.
        if sr != sc {
            failed += 1;
            println!("  {size:>7}  FAIL: checksums differ ({sr} vs {sc}) -- not the same work");
            continue;
        }
        let pr = cr.saturating_sub(floor) / OPS as u32;
        let pc = cc.saturating_sub(floor) / OPS as u32;
        let ratio = if pr == 0 {
            0.0
        } else {
            f64::from(pc) / f64::from(pr)
        };
        println!(
            "  {size:>7}  {pr:>9}  {pc:>13}  {ratio:>10.2}x  {:>16}",
            charge_c(size)
        );
    }

    // The sweep that stops the flat 232 being read as heap_4's cost.
    println!();
    println!("--- a FIXED-SIZE POOL, the strategy an RTOS can use for known sizes ---");
    println!("  not a faster allocator: a narrower question, answered in fewer");
    println!("  instructions (12.00 Ir/pair on the host against 53.57 general).");
    println!("     size    pool c/op   general c/op    heap_4 c/op   pool vs general");
    for size in [256usize, 512] {
        let mut best = u32::MAX;
        for _ in 0..(WARMUP + ROUNDS) {
            let (c, _, bad) = pool_round(size);
            if !bad && c < best {
                best = c;
            }
        }
        let per = best.saturating_sub(floor) / OPS as u32;
        let general = if size == 256 { 101 } else { 114 };
        println!(
            "  {size:7}  {per:11}  {general:13}  {:13}  {:14}",
            235,
            if per > 0 {
                100 - (per * 100 / general)
            } else {
                0
            }
        );
    }

    println!();
    println!("--- where the cycles are: alloc half vs free half ---");
    println!("  blocks are HELD live, so neither half can hide in the other.");
    println!("     size    alloc c/op     free c/op         pair");
    for size in [16usize, 256, 512, 1024] {
        let (a, f) = split_alloc_free(size);
        println!("  {size:7}  {a:12}  {f:12}  {:11}", a + f);
    }

    println!();
    println!("--- 512-byte requests against a free list of 16-byte HOLES ---");
    println!("  the clean case above is heap_4's best: one live block means a");
    println!("  one-entry free list, and first-fit answers in one step.");
    println!("  Holes the request cannot use are what make it walk. (Holes of");
    println!("  the SAME size made it faster, 236 -> 218: first-fit took the");
    println!("  first one. That refuted the first version of this probe.)");
    println!();
    println!("    holes   rust c/op    heap_4 c/op        ratio");
    for holes in [0usize, 8, 32, 128] {
        let (cr, cc, sr, sc, bad) =
            best_pair(|| round_rust(512, holes, 16), || round_c(512, holes, 16));
        if bad {
            failed += 1;
            println!("  {holes:>7}  FAIL: an allocation returned null");
            continue;
        }
        if sr != sc {
            failed += 1;
            println!("  {holes:>7}  FAIL: checksums differ ({sr} vs {sc})");
            continue;
        }
        let pr = cr.saturating_sub(floor) / OPS as u32;
        let pc = cc.saturating_sub(floor) / OPS as u32;
        let ratio = if pr == 0 {
            0.0
        } else {
            f64::from(pc) / f64::from(pr)
        };
        println!("  {holes:>7}  {pr:>9}  {pc:>13}  {ratio:>10.2}x");
    }

    // Both arms really used their own heap.
    // SAFETY: ALIGN is a valid power of two and 64 >= 1.
    let probe = unsafe { alloc::alloc::alloc(Layout::from_size_align_unchecked(64, ALIGN)) };
    if !region_contains(probe as usize) {
        failed += 1;
        println!("  FAIL: the Rust arm did not allocate from its declared region");
    }
    // SAFETY: `probe` came from the matching `alloc` immediately above.
    unsafe { alloc::alloc::dealloc(probe, Layout::from_size_align_unchecked(64, ALIGN)) };

    // And heap_4 gave back everything it handed out.
    let free_now = unsafe { xPortGetFreeHeapSize() };
    println!();
    println!("heap_4 free after the run: {free_now} bytes");

    println!();
    if failed == 0 {
        println!("RESULT: PASS -- both allocators measured on silicon, same memory,");
        println!("        same call shape, checksums equal, harness floor removed.");
    } else {
        println!("RESULT: FAIL -- {failed} check(s) failed");
    }
    println!("NOTE: this is the A/B B4b asks for, on a part that is NOT a Kairos");
    println!("      target. The C6 (riscv32imac, mcycle) still owes that clause.");

    loop {
        core::hint::spin_loop();
    }
}
